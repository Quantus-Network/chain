use crate::{
	tests::{
		mock::*,
		test_reversible_transfers::{calculate_tx_id, transfer_call},
	},
	Event,
};
use frame_support::{assert_err, assert_ok};
use pallet_balances::TotalIssuance;

// NOTE: Many of the high security / reversibility behaviors are enforced via SignedExtension or
// external pallets (Proxy). They are covered by integration tests in runtime.

#[test]
fn guardian_can_recover_all_funds_from_high_security_account() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let hs_user = alice();
		let guardian = bob();

		let initial_hs_balance = Balances::free_balance(&hs_user);
		let initial_guardian_balance = Balances::free_balance(&guardian);

		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(guardian.clone()),
			hs_user.clone()
		));

		assert_eq!(Balances::free_balance(&hs_user), 0);
		assert_eq!(
			Balances::free_balance(&guardian),
			initial_guardian_balance + initial_hs_balance
		);

		System::assert_has_event(Event::FundsRecovered { account: hs_user, guardian }.into());
	});
}

#[test]
fn recover_funds_fails_if_caller_is_not_guardian() {
	new_test_ext().execute_with(|| {
		let hs_user = alice();
		let not_guardian = charlie();

		assert_err!(
			ReversibleTransfers::recover_funds(RuntimeOrigin::signed(not_guardian), hs_user),
			crate::Error::<Test>::InvalidReverser
		);
	});
}

#[test]
fn recover_funds_fails_for_non_high_security_account() {
	new_test_ext().execute_with(|| {
		let regular_user = charlie();
		let attacker = dave();

		assert_err!(
			ReversibleTransfers::recover_funds(RuntimeOrigin::signed(attacker), regular_user),
			crate::Error::<Test>::AccountNotHighSecurity
		);
	});
}

#[test]
fn guardian_can_cancel_reversible_transactions_for_hs_account() {
	new_test_ext().execute_with(|| {
		let hs_user = alice(); // reversible from genesis with guardian=2
		let guardian = bob();
		let dest = charlie();
		let amount = 10_000u128; // Use larger amount so volume fee is visible

		// Record initial balances
		let initial_guardian_balance = Balances::free_balance(&guardian);
		let initial_total_issuance = TotalIssuance::<Test>::get();

		// Compute tx_id BEFORE scheduling (matches pallet logic using current NextTransactionId)
		let call = transfer_call(dest.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(hs_user.clone(), &call);

		// Schedule a reversible transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(hs_user.clone()),
			dest.clone(),
			amount
		));

		// Guardian cancels it
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(guardian.clone()), tx_id));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());

		// Verify volume fee was applied for high-security account
		// Expected fee: 10,000 * 1% = 100 tokens
		let expected_fee = 100;
		let expected_remaining = amount - expected_fee;

		// Check that guardian received the remaining amount (after fee)
		assert_eq!(
			Balances::free_balance(&guardian),
			initial_guardian_balance + expected_remaining,
			"Guardian should receive remaining amount after volume fee deduction"
		);

		// Check that fee was burned (total issuance decreased)
		assert_eq!(
			TotalIssuance::<Test>::get(),
			initial_total_issuance - expected_fee,
			"Volume fee should be burned from total issuance"
		);
	});
}

#[test]
fn recover_funds_cancels_all_pending_transfers() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let hs_user = alice();
		let guardian = bob();
		let dest = charlie();

		let initial_hs_balance = Balances::free_balance(&hs_user);
		let initial_guardian_balance = Balances::free_balance(&guardian);
		let initial_total_issuance = TotalIssuance::<Test>::get();

		// Schedule multiple transfers
		let amount1 = 10_000u128;
		let amount2 = 20_000u128;

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(hs_user.clone()),
			dest.clone(),
			amount1
		));

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(hs_user.clone()),
			dest.clone(),
			amount2
		));

		// Verify pending transfers exist
		let pending = crate::PendingTransfersBySender::<Test>::get(&hs_user);
		assert_eq!(pending.len(), 2, "Should have 2 pending transfers");

		// Now recover all funds
		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(guardian.clone()),
			hs_user.clone()
		));

		// Verify all pending transfers were cancelled
		let pending_after = crate::PendingTransfersBySender::<Test>::get(&hs_user);
		assert_eq!(pending_after.len(), 0, "All pending transfers should be cancelled");

		// Verify hs_user is drained
		assert_eq!(Balances::free_balance(&hs_user), 0, "HS user should be drained");

		// Calculate expected amounts:
		// - Volume fee (1%) is burned for each cancelled transfer
		// - Remaining goes to guardian
		let fee1 = amount1 / 100; // 100
		let fee2 = amount2 / 100; // 200
		let total_fees = fee1 + fee2; // 300
		let remaining_from_cancels = (amount1 - fee1) + (amount2 - fee2); // 9900 + 19800 = 29700
		let free_balance_after_holds = initial_hs_balance - amount1 - amount2;

		// Guardian receives: remaining from cancelled transfers + free balance
		let expected_guardian_balance =
			initial_guardian_balance + remaining_from_cancels + free_balance_after_holds;
		assert_eq!(
			Balances::free_balance(&guardian),
			expected_guardian_balance,
			"Guardian should receive all funds minus volume fees"
		);

		// Total issuance should decrease by the volume fees burned
		assert_eq!(
			TotalIssuance::<Test>::get(),
			initial_total_issuance - total_fees,
			"Volume fees should be burned"
		);

		// Verify events were emitted for each cancelled transfer
		let events = System::events();
		let cancel_events: Vec<_> = events
			.iter()
			.filter(|e| {
				matches!(
					e.event,
					RuntimeEvent::ReversibleTransfers(Event::TransactionCancelled { .. })
				)
			})
			.collect();
		assert_eq!(
			cancel_events.len(),
			2,
			"Should emit TransactionCancelled for each pending transfer"
		);

		// Verify FundsRecovered event
		System::assert_has_event(
			Event::FundsRecovered { account: hs_user.clone(), guardian: guardian.clone() }.into(),
		);
	});
}

/// A failing final sweep must not roll back the pending-transfer cancellations that
/// `recover_funds` already performed. FRAME dispatchables are transactional, so if the
/// closing `transfer_all` error propagated out of the extrinsic, the hold releases and
/// scheduler cancellations would revert — re-arming the very transfers the compromised
/// account's guardian is trying to stop, and letting them execute at their scheduled
/// time. The sweep must be best-effort: keep the cancellations, emit
/// `RecoverySweepFailed`, and let the guardian retry.
#[test]
fn recover_funds_keeps_cancellations_when_final_sweep_fails() {
	use pallet_scheduler::Agenda;

	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let hs_user = alice();
		let guardian = bob();
		let dest = charlie();
		let amount = 10_000u128;

		let initial_hs_balance = Balances::free_balance(&hs_user);

		let call = transfer_call(dest.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(hs_user.clone(), &call);
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(hs_user.clone()),
			dest.clone(),
			amount
		));

		// Make the final sweep fail deterministically: the release below credits the
		// guardian with `amount - 1% fee` (9_900), landing its balance exactly on
		// `u128::MAX`, so the subsequent `transfer_all` of the remaining free balance
		// overflows the guardian's balance.
		let release_amount = amount - amount / 100;
		assert_ok!(Balances::force_set_balance(
			RuntimeOrigin::root(),
			guardian.clone(),
			u128::MAX - release_amount
		));

		// The recovery itself must succeed even though the sweep cannot.
		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(guardian.clone()),
			hs_user.clone()
		));

		// The cancellation work is preserved: pending metadata removed, hold released to
		// the guardian, and the scheduled task cancelled so it can never execute later.
		assert!(crate::PendingTransfers::<Test>::get(tx_id).is_none());
		assert!(crate::PendingTransfersBySender::<Test>::get(&hs_user).is_empty());
		assert_eq!(Balances::free_balance(&guardian), u128::MAX);
		assert!(
			Agenda::<Test>::iter().all(|(_, agenda)| agenda.iter().all(|slot| slot.is_none())),
			"the cancelled transfer must not remain scheduled"
		);

		// The failed sweep left the account's free balance untouched.
		assert_eq!(Balances::free_balance(&hs_user), initial_hs_balance - amount);

		// The outcome is reported truthfully: sweep failed, funds not (fully) recovered.
		System::assert_has_event(
			Event::RecoverySweepFailed { account: hs_user.clone(), guardian: guardian.clone() }
				.into(),
		);
		assert!(
			!System::events().iter().any(|e| matches!(
				e.event,
				RuntimeEvent::ReversibleTransfers(Event::FundsRecovered { .. })
			)),
			"FundsRecovered must not be emitted when the sweep failed"
		);
	});
}

#[test]
fn too_many_pending_transactions_error() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let hs_user = alice();
		let dest = charlie();
		let amount = 100u128;

		// Schedule MaxPendingPerAccount transfers (16)
		// Need to advance block number between batches to avoid scheduler max per block limit
		for i in 0u32..16 {
			// Advance block every 8 transfers to stay under scheduler's MaxScheduledPerBlock (10)
			if i > 0 && i.is_multiple_of(8) {
				System::set_block_number(System::block_number() + 1);
			}
			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(hs_user.clone()),
				dest.clone(),
				amount
			));
		}

		// Verify we have 16 pending
		let pending = crate::PendingTransfersBySender::<Test>::get(&hs_user);
		assert_eq!(pending.len(), 16, "Should have 16 pending transfers");

		// The 17th should fail
		assert_err!(
			ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(hs_user.clone()),
				dest.clone(),
				amount
			),
			crate::Error::<Test>::TooManyPendingTransactions
		);
	});
}

/// Mirrors the `recover_funds` benchmark's worst-case setup: one scheduled
/// transfer per block so cancellations touch `n` distinct `Scheduler::Agenda`
/// keys (not a handful of clustered buckets).
#[test]
fn recover_funds_cancels_across_distinct_agenda_buckets() {
	use pallet_scheduler::Agenda;
	use qp_scheduler::{BlockNumberOrTimestamp, ScheduleNamed};
	use std::collections::BTreeSet;

	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let hs_user = alice();
		let guardian = bob();
		let dest = charlie();
		let amount = 100u128;
		let n = MaxPendingPerAccount::get();

		for i in 0..n {
			if i > 0 {
				System::set_block_number(System::block_number() + 1);
			}
			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(hs_user.clone()),
				dest.clone(),
				amount
			));
		}

		let pending = crate::PendingTransfersBySender::<Test>::get(&hs_user);
		assert_eq!(pending.len() as u32, n);

		let mut agenda_keys = BTreeSet::new();
		for tx_id in pending.iter() {
			let schedule_id = ReversibleTransfers::make_schedule_id(tx_id).expect("schedule id");
			let when = <Scheduler as ScheduleNamed<u64, u64, RuntimeCall, OriginCaller>>::next_dispatch_time(
				schedule_id,
			)
			.expect("named task is scheduled");
			let agenda_key = BlockNumberOrTimestamp::BlockNumber(when);
			agenda_keys.insert(agenda_key);
			assert!(
				!Agenda::<Test>::get(agenda_key).is_empty(),
				"expected a non-empty agenda at {agenda_key:?}"
			);
		}
		assert_eq!(
			agenda_keys.len() as u32,
			n,
			"one schedule per block must produce n distinct Agenda keys; \
			 clustering would under-weight recover_funds cancellations"
		);

		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(guardian),
			hs_user.clone()
		));
		assert!(crate::PendingTransfersBySender::<Test>::get(&hs_user).is_empty());
		for when in agenda_keys {
			assert!(
				Agenda::<Test>::get(when).is_empty(),
				"recover_funds must clear agenda bucket at {when:?}"
			);
		}
	});
}

#[test]
fn recover_funds_weight_charges_agenda_per_pending_transfer() {
	use crate::weights::WeightInfo;

	let w0 = <() as WeightInfo>::recover_funds(0);
	let w1 = <() as WeightInfo>::recover_funds(1);
	let step = w1.saturating_sub(w0);
	// Matches the unmodified FRAME-generated per-n proof component (Agenda MEL).
	assert_eq!(
		step.proof_size(),
		12493,
		"recover_funds(n) must charge one Scheduler::Agenda proof per pending transfer"
	);
}
