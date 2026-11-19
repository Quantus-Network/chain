//! A high-security account can only move funds through `schedule_transfer`, which
//! always lands in the scheduler agenda at `now + delay`. Anyone can fill that
//! agenda's `LOWEST_PRIORITY` slots in advance with one-unit one-time transfers.
//! The transfer then cannot be scheduled, which is a public-chain availability
//! property, but a doomed attempt must not be included: included, it pays a fee
//! and spends one of the account's sixteen daily quota slots, and sixteen of them
//! lock the account out of every extrinsic for a day after the attacker stops.
//!
//! `ReversibleTransactionExtension::validate` therefore rejects the extrinsic
//! before `prepare` records the quota. Everything here goes through
//! `Executive::apply_extrinsic` with real signed extrinsics.

use super::high_security_tip::{bogus_cancel, funded_ext, pair, signed_call};
use frame_support::{
	assert_ok,
	pallet_prelude::{InvalidTransaction, TransactionValidityError},
	traits::{schedule::LOWEST_PRIORITY, Currency},
};
use pallet_reversible_transfers::{HighSecurityTxQuota, PendingTransfersBySender};
use qp_scheduler::{BlockNumberOrTimestamp, DispatchTime};
use quantus_runtime::{
	transaction_extensions::HIGH_SECURITY_AGENDA_FULL, Balances, Executive, ReversibleTransfers,
	Runtime, RuntimeCall, RuntimeOrigin, System, UncheckedExtrinsic, UNIT,
};
use sp_core::Pair;
use sp_runtime::{traits::IdentifyAccount, AccountId32, MultiAddress};

fn recipient() -> AccountId32 {
	crate::common::TestCommons::account_id(4)
}

fn schedule_transfer() -> RuntimeCall {
	RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::schedule_transfer {
		dest: MultiAddress::Id(recipient()),
		amount: UNIT,
	})
}

fn batch_of(calls: Vec<RuntimeCall>) -> RuntimeCall {
	RuntimeCall::Utility(pallet_utility::Call::batch_all { calls })
}

fn agenda_full() -> TransactionValidityError {
	TransactionValidityError::Invalid(InvalidTransaction::Custom(HIGH_SECURITY_AGENDA_FULL))
}

/// The victim's enrolled delay and the agenda its transfers scheduled now would use.
fn victim_target(
	victim: &AccountId32,
) -> (BlockNumberOrTimestamp<u32, u64>, DispatchTime<u32, u64>) {
	let delay = ReversibleTransfers::is_high_security(victim)
		.expect("victim is high-security")
		.delay;
	(delay, ReversibleTransfers::dispatch_time_for(&delay))
}

/// Occupy all but `leave` of the `LOWEST_PRIORITY` slots at the victim's target agenda
/// with one-unit one-time self-transfers from ordinary accounts, as the attacker does.
fn fill_victim_agenda(victim: &AccountId32, leave: u32) {
	let (delay, when) = victim_target(victim);
	let per_account = <Runtime as pallet_reversible_transfers::Config>::MaxPendingPerAccount::get();
	let mut seed = 101u8;
	while pallet_scheduler::Pallet::<Runtime>::free_slots(when, LOWEST_PRIORITY) > leave {
		let attacker = crate::common::TestCommons::account_id(seed);
		Balances::make_free_balance_be(&attacker, 10 * UNIT);
		for _ in 0..per_account {
			if pallet_scheduler::Pallet::<Runtime>::free_slots(when, LOWEST_PRIORITY) == leave {
				break;
			}
			assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
				RuntimeOrigin::signed(attacker.clone()),
				MultiAddress::Id(attacker.clone()),
				1,
				delay,
			));
		}
		seed += 1;
	}
	assert_eq!(pallet_scheduler::Pallet::<Runtime>::free_slots(when, LOWEST_PRIORITY), leave);
}

fn assert_untouched(victim: &AccountId32, free_before: u128, quota_before: usize) {
	assert_eq!(Balances::free_balance(victim), free_before, "no fee was charged");
	assert_eq!(HighSecurityTxQuota::<Runtime>::get(victim).len(), quota_before, "no quota spent");
	assert!(PendingTransfersBySender::<Runtime>::get(victim).is_empty());
}

#[test]
fn unschedulable_high_security_transfer_is_rejected_before_inclusion() {
	let victim_pair = pair();
	let victim = victim_pair.public().into_account();
	funded_ext(&victim, true).execute_with(|| {
		fill_victim_agenda(&victim, 0);
		let free_before = Balances::free_balance(&victim);

		// Sixteen attempts, as many as the daily quota: none is included, none costs anything.
		let quota =
			<Runtime as pallet_reversible_transfers::Config>::MaxHighSecurityTxsPerWindow::get();
		for _ in 0..quota {
			let xt = signed_call(&victim_pair, victim.clone(), schedule_transfer(), 0, 0);
			assert_eq!(Executive::apply_extrinsic(xt).unwrap_err(), agenda_full());
			assert_untouched(&victim, free_before, 0);
		}

		// Calls that take no agenda slot are unaffected while the agenda is full.
		let xt = signed_call(&victim_pair, victim.clone(), bogus_cancel(), 0, 0);
		assert!(Executive::apply_extrinsic(xt).is_ok(), "cancel is still included");
		assert_eq!(HighSecurityTxQuota::<Runtime>::get(&victim).len(), 1);

		// The next block's agenda is free, and so is the account: the transfer goes through.
		System::set_block_number(System::block_number() + 1);
		let xt = signed_call(&victim_pair, victim.clone(), schedule_transfer(), 1, 0);
		assert_ok!(Executive::apply_extrinsic(xt).expect("included"));
		assert_eq!(PendingTransfersBySender::<Runtime>::get(&victim).len(), 1);
	});
}

#[test]
fn batch_needs_a_slot_per_scheduled_transfer() {
	let victim_pair = pair();
	let victim = victim_pair.public().into_account();
	funded_ext(&victim, true).execute_with(|| {
		fill_victim_agenda(&victim, 1);
		let free_before = Balances::free_balance(&victim);

		let two = batch_of(vec![schedule_transfer(), schedule_transfer()]);
		let xt: UncheckedExtrinsic = signed_call(&victim_pair, victim.clone(), two, 0, 0);
		assert_eq!(Executive::apply_extrinsic(xt).unwrap_err(), agenda_full());
		assert_untouched(&victim, free_before, 0);

		let one = batch_of(vec![schedule_transfer()]);
		let xt = signed_call(&victim_pair, victim.clone(), one, 0, 0);
		assert_ok!(Executive::apply_extrinsic(xt).expect("one transfer fits the last slot"));
		assert_eq!(PendingTransfersBySender::<Runtime>::get(&victim).len(), 1);
	});
}
