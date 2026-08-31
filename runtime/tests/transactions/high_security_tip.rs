//! High-security accounts must not move value through the transaction tip.
//!
//! Tips are set the same way a wallet does:
//! `ChargeTransactionPayment::from(tip)` in the signed `TxExtension` extra,
//! then `Executive::apply_extrinsic`. The zero-tip policy is enforced by
//! `HighSecurityFungibleAdapter` inside `OnChargeTransaction`.

use crate::common::TestCommons;
use codec::Encode;
use frame_support::{
	assert_ok,
	dispatch::GetDispatchInfo,
	pallet_prelude::{InvalidTransaction, TransactionValidityError},
	traits::{Currency, Hooks},
};
use qp_dilithium_crypto::Dilithium65Pair;
use qp_scheduler::BlockNumberOrTimestamp;
use qp_wormhole::{derive_wormhole_address, POW_ENGINE_ID};
use quantus_runtime::{
	transaction_extensions::HIGH_SECURITY_TIP_FORBIDDEN, Balances, Executive, MiningRewards,
	ReversibleTransfers, Runtime, RuntimeCall, RuntimeEvent, RuntimeOrigin, System,
	UncheckedExtrinsic, EXISTENTIAL_DEPOSIT, MILLI_UNIT, UNIT,
};
use sp_core::Pair;
use sp_runtime::{generic::DigestItem, traits::IdentifyAccount, AccountId32, MultiAddress};

const STARTING_BALANCE: u128 = 1000 * UNIT;
const HS_DELAY_BLOCKS: u32 = 5;

pub(crate) fn pair() -> Dilithium65Pair {
	Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed")
}

fn guardian() -> AccountId32 {
	TestCommons::account_id(2)
}

fn recipient() -> AccountId32 {
	TestCommons::account_id(4)
}

fn miner_preimage() -> [u8; 32] {
	let mut buf = [0u8; 32];
	buf[..8].copy_from_slice(&1u64.to_le_bytes());
	buf
}

fn miner_account() -> AccountId32 {
	AccountId32::from(derive_wormhole_address(miner_preimage()).expect("canonical test preimage"))
}

fn test_ext(account: &AccountId32) -> sp_io::TestExternalities {
	funded_ext(account, true)
}

pub(crate) fn funded_ext(account: &AccountId32, high_security: bool) -> sp_io::TestExternalities {
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		Balances::make_free_balance_be(account, STARTING_BALANCE);
		Balances::make_free_balance_be(&miner_account(), EXISTENTIAL_DEPOSIT);
		System::set_block_number(1);
		if high_security {
			assert_ok_hs(account);
		}
	});
	ext
}

fn assert_ok_hs(account: &AccountId32) {
	frame_support::assert_ok!(ReversibleTransfers::set_high_security(
		RuntimeOrigin::signed(account.clone()),
		BlockNumberOrTimestamp::BlockNumber(HS_DELAY_BLOCKS),
		guardian(),
	));
	assert!(ReversibleTransfers::is_high_security(account).is_some());
}

pub(crate) fn signed_call(
	pair: &Dilithium65Pair,
	sender: AccountId32,
	call: RuntimeCall,
	nonce: u32,
	tip: u128,
) -> UncheckedExtrinsic {
	TestCommons::signed_extrinsic(pair, sender, call, nonce, tip)
}

fn inclusion_fee(xt: &UncheckedExtrinsic) -> u128 {
	pallet_transaction_payment::Pallet::<Runtime>::compute_fee(
		xt.encode().len() as u32,
		&xt.get_dispatch_info(),
		0,
	)
}

/// Largest tip `Preservation::Preserve` will accept for this call: leave ED plus
/// the zero-tip inclusion fee, with slack for the extra compact-encoded tip bytes.
fn max_preserve_tip(pair: &Dilithium65Pair, sender: &AccountId32, call: &RuntimeCall) -> u128 {
	let probe = signed_call(pair, sender.clone(), call.clone(), 0, 0);
	let fee = inclusion_fee(&probe);
	Balances::free_balance(sender)
		.saturating_sub(EXISTENTIAL_DEPOSIT)
		.saturating_sub(fee)
		.saturating_sub(EXISTENTIAL_DEPOSIT)
}

pub(crate) fn empty_batch_all() -> RuntimeCall {
	RuntimeCall::Utility(pallet_utility::Call::batch_all { calls: vec![] })
}

pub(crate) fn bogus_cancel() -> RuntimeCall {
	RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
		tx_id: Default::default(),
	})
}

fn recover_own_funds(account: &AccountId32) -> RuntimeCall {
	RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::recover_funds {
		account: account.clone(),
	})
}

fn schedule_small_transfer() -> RuntimeCall {
	RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::schedule_transfer {
		dest: MultiAddress::Id(recipient()),
		amount: 10 * UNIT,
	})
}

fn padded_schedule_transfer(pad: usize) -> RuntimeCall {
	RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::schedule_transfer {
		dest: MultiAddress::Raw(vec![0u8; pad]),
		amount: 10 * UNIT,
	})
}

/// `(actual_fee, tip)` from the payer's `TransactionFeePaid` event, or `None`
/// if no fee was charged.
fn fee_paid(who: &AccountId32) -> Option<(u128, u128)> {
	System::events().into_iter().find_map(|record| match record.event {
		RuntimeEvent::TransactionPayment(
			pallet_transaction_payment::Event::TransactionFeePaid { who: payer, actual_fee, tip },
		) if payer == *who => Some((actual_fee, tip)),
		_ => None,
	})
}

fn paid_tip(who: &AccountId32) -> Option<u128> {
	fee_paid(who).map(|(_, tip)| tip)
}

fn paid_fee_or_zero(who: &AccountId32) -> u128 {
	fee_paid(who).map_or(0, |(fee, _)| fee)
}

/// A high-security signer may lose at most the ordinary (zero-tip) inclusion fee,
/// plus any amount the call itself is allowed to lock on the delay path.
fn assert_tip_cannot_move_extra_value(
	pair: &Dilithium65Pair,
	account: &AccountId32,
	call: RuntimeCall,
	allowed_call_lock: u128,
) {
	let tip = max_preserve_tip(pair, account, &call);
	assert!(tip > 10 * UNIT, "fixture must leave a large tippable surplus, got {tip}");

	let xt = signed_call(pair, account.clone(), call, 0, tip);
	let fee_ceiling = inclusion_fee(&xt).saturating_add(allowed_call_lock);
	let free_before = Balances::free_balance(account);
	let total_before = Balances::total_balance(account);

	match Executive::apply_extrinsic(xt) {
		Err(_) => {
			assert_eq!(
				Balances::free_balance(account),
				free_before,
				"a rejected high-security extrinsic must not move free balance"
			);
		},
		Ok(_) => {
			let free_lost = free_before.saturating_sub(Balances::free_balance(account));
			let total_lost = total_before.saturating_sub(Balances::total_balance(account));
			assert!(
				free_lost <= fee_ceiling,
				"high-security free balance left via tip: lost {free_lost}, \
				 allowed {fee_ceiling} (inclusion fee + delayed lock)"
			);
			assert!(
				total_lost <= fee_ceiling,
				"high-security total balance left via tip: lost {total_lost}, \
				 allowed {fee_ceiling}"
			);
			assert_eq!(
				paid_tip(account).unwrap_or(u128::MAX),
				0,
				"HighSecurityFungibleAdapter must not accept a value-moving \
				 tip from a high-security account"
			);
		},
	}
}

/// The fee ceiling must admit every legitimate worst-case extrinsic with at
/// least 2x headroom, so re-benchmarking drift cannot lock a high-security
/// account out of its own whitelist (recovery included).
#[test]
fn high_security_fee_ceiling_admits_worst_case_legitimate_extrinsics() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		let ceiling = quantus_runtime::configs::MAX_HIGH_SECURITY_INCLUSION_FEE;
		let batch16 = RuntimeCall::Utility(pallet_utility::Call::batch_all {
			calls: vec![schedule_small_transfer(); 16],
		});
		for (name, call) in [
			("batch_all of 16 schedule_transfers", batch16),
			("recover_funds", recover_own_funds(&account)),
		] {
			let xt = signed_call(&pair, account.clone(), call, 0, 0);
			let fee = inclusion_fee(&xt);
			assert!(
				fee.saturating_mul(2) <= ceiling,
				"{name} must clear MAX_HIGH_SECURITY_INCLUSION_FEE with 2x headroom: \
				 fee {fee}, ceiling {ceiling}"
			);
		}
	});
}

#[test]
fn high_security_signed_schedule_transfer_nonzero_tip_is_rejected() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		let tip = 50 * UNIT;
		let before = Balances::free_balance(&account);
		let xt = signed_call(&pair, account.clone(), schedule_small_transfer(), 0, tip);

		assert_eq!(
			Executive::apply_extrinsic(xt).unwrap_err(),
			TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_TIP_FORBIDDEN
			))
		);
		assert_eq!(Balances::free_balance(&account), before);
		assert!(pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(&account)
			.is_empty());
		assert_eq!(paid_tip(&account), None);
	});
}

#[test]
fn high_security_padded_dest_is_rejected_before_fees() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		// 100 KiB of dest padding is ~0.1 UNIT of length fee (1 UNIT / MB).
		const PAD: usize = 100_000;
		let id_xt = signed_call(&pair, account.clone(), schedule_small_transfer(), 0, 0);
		let raw_xt = signed_call(&pair, account.clone(), padded_schedule_transfer(PAD), 0, 0);
		assert!(
			inclusion_fee(&raw_xt) > inclusion_fee(&id_xt) + 50 * MILLI_UNIT,
			"padded dest must inflate the chain-decided inclusion fee, got id={} raw={}",
			inclusion_fee(&id_xt),
			inclusion_fee(&raw_xt)
		);

		let before = Balances::free_balance(&account);
		assert_eq!(
			Executive::apply_extrinsic(raw_xt).unwrap_err(),
			TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
		);
		assert_eq!(Balances::free_balance(&account), before);
		assert!(pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(&account)
			.is_empty());
		assert_eq!(paid_tip(&account), None);
		assert_eq!(paid_fee_or_zero(&account), 0);
	});
}

#[test]
fn high_security_batch_all_padded_dest_is_rejected_before_fees() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
			calls: vec![padded_schedule_transfer(100_000)],
		});
		let before = Balances::free_balance(&account);
		let xt = signed_call(&pair, account.clone(), call, 0, 0);
		assert_eq!(
			Executive::apply_extrinsic(xt).unwrap_err(),
			TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
		);
		assert_eq!(Balances::free_balance(&account), before);
		assert_eq!(paid_fee_or_zero(&account), 0);
	});
}

#[test]
fn high_security_signed_schedule_transfer_zero_tip_is_included() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		let amount = 10 * UNIT;
		let xt = signed_call(&pair, account.clone(), schedule_small_transfer(), 0, 0);
		let fee = inclusion_fee(&xt);
		let before = Balances::free_balance(&account);

		assert_ok!(Executive::apply_extrinsic(xt).expect("zero-tip schedule_transfer is valid"));
		assert_eq!(paid_tip(&account), Some(0));
		assert_eq!(
			pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(&account).len(),
			1,
			"the delayed transfer must have been scheduled"
		);
		let lost = before - Balances::free_balance(&account);
		assert_eq!(
			lost,
			amount + paid_fee_or_zero(&account),
			"zero-tip schedule_transfer may only lock the amount and pay the inclusion fee"
		);
		assert!(lost - amount <= fee);
	});
}

#[test]
fn normal_account_signed_transfer_with_tip_is_included() {
	let pair = pair();
	let account = pair.public().into_account();
	funded_ext(&account, false).execute_with(|| {
		let tip = 5 * UNIT;
		let value = 10 * UNIT;
		let dest = recipient();
		let dest_before = Balances::free_balance(&dest);
		let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
			dest: MultiAddress::Id(dest.clone()),
			value,
		});
		let xt = signed_call(&pair, account.clone(), call, 0, tip);
		let before = Balances::free_balance(&account);

		assert_ok!(Executive::apply_extrinsic(xt).expect("tipped transfer is valid"));
		assert_eq!(paid_tip(&account), Some(tip));
		assert_eq!(Balances::free_balance(&dest), dest_before + value);
		assert_eq!(
			before - Balances::free_balance(&account),
			value + paid_fee_or_zero(&account),
			"normal account pays the transfer, inclusion fee, and the signed tip"
		);
		assert!(paid_fee_or_zero(&account) >= tip);
	});
}

// NOTE: plain whitelist rejections (empty `batch_all`, `Vesting::claim`, ...)
// are covered by the `check_call` unit tests in
// `runtime/src/transaction_extensions.rs`; the integration tests here only
// assert what those cannot — fee and balance effects through the full
// `Executive::apply_extrinsic` pipeline.

#[test]
fn high_security_empty_batch_all_cannot_drain_via_tip() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		assert_tip_cannot_move_extra_value(&pair, &account, empty_batch_all(), 0);
	});
}

#[test]
fn high_security_failed_cancel_cannot_drain_via_tip() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		assert_tip_cannot_move_extra_value(&pair, &account, bogus_cancel(), 0);
	});
}

#[test]
fn high_security_failed_recover_funds_cannot_drain_via_tip() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		// Owner is not the guardian; dispatch fails, but today's tip is still taken.
		assert_tip_cannot_move_extra_value(&pair, &account, recover_own_funds(&account), 0);
	});
}

#[test]
fn high_security_schedule_transfer_tip_cannot_take_more_than_the_delayed_amount() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		let scheduled = 10 * UNIT;
		assert_tip_cannot_move_extra_value(&pair, &account, schedule_small_transfer(), scheduled);
	});
}

#[test]
fn high_security_held_pending_transfer_survives_a_tip_on_remaining_free() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		let held = 400 * UNIT;
		frame_support::assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(account.clone()),
			MultiAddress::Id(recipient()),
			held,
		));
		assert_eq!(Balances::free_balance(&account), STARTING_BALANCE - held);
		assert_eq!(
			pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(&account).len(),
			1,
			"the delayed transfer must still be pending"
		);

		assert_tip_cannot_move_extra_value(&pair, &account, empty_batch_all(), 0);

		// The hold itself cannot be tipped — this must stay true whether the
		// leftover-free tip is rejected or only the inclusion fee is charged.
		assert_eq!(
			pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(&account).len(),
			1,
			"pending hold must remain for the guardian to cancel"
		);
		assert_eq!(
			Balances::total_balance(&account).saturating_sub(Balances::free_balance(&account)),
			held,
			"held delayed funds must still be on the account"
		);
	});
}

#[test]
fn high_security_tip_is_not_reminted_to_the_block_author() {
	let pair = pair();
	let account = pair.public().into_account();
	test_ext(&account).execute_with(|| {
		let call = empty_batch_all();
		let tip = max_preserve_tip(&pair, &account, &call);
		let xt = signed_call(&pair, account.clone(), call, 0, tip);
		let fee_ceiling = inclusion_fee(&xt);
		let miner = miner_account();

		let _ = Executive::apply_extrinsic(xt);

		// Whatever the inclusion outcome, a high-security tip must not be sitting
		// in CollectedFees waiting for the author.
		let collected = MiningRewards::collected_fees();
		assert!(
			collected <= fee_ceiling,
			"CollectedFees must not contain a high-security tip: {collected} > {fee_ceiling}"
		);

		System::deposit_log(DigestItem::PreRuntime(POW_ENGINE_ID, miner_preimage().to_vec()));
		MiningRewards::on_finalize(System::block_number());

		if collected > 0 {
			// First MinerRewarded is the fee remint (`mint_reward(miner, tx_fees)`);
			// block emission is a later event and is allowed.
			let fee_remint = System::events()
				.into_iter()
				.find_map(|record| match record.event {
					RuntimeEvent::MiningRewards(pallet_mining_rewards::Event::MinerRewarded {
						miner: who,
						reward,
					}) if who == miner => Some(reward),
					_ => None,
				})
				.expect("collected fees must be reminted to the author");
			assert_eq!(
				fee_remint, collected,
				"author remint must equal CollectedFees, not a high-security tip"
			);
			assert!(
				fee_remint <= fee_ceiling,
				"block author remint must not include a high-security tip: \
				 reminted {fee_remint}, allowed {fee_ceiling}"
			);
		}

		assert_eq!(
			paid_tip(&account).unwrap_or(0),
			0,
			"block author must not be paid a tip taken from a high-security account"
		);
	});
}
