//! High-security accounts may include at most 16 signed extrinsics in a
//! rolling 24h window (`DAYS` blocks). The 17th is rejected even if it is a
//! whitelisted no-op.

use super::high_security_tip::{bogus_cancel, empty_batch_all, funded_ext, pair, signed_call};
use frame_support::pallet_prelude::{InvalidTransaction, TransactionValidityError};
use quantus_runtime::{
	transaction_extensions::HIGH_SECURITY_TX_QUOTA_EXCEEDED, Executive, System, DAYS,
};
use sp_core::Pair;
use sp_runtime::traits::IdentifyAccount;

#[test]
fn high_security_account_is_capped_at_sixteen_signed_extrinsics_per_rolling_day() {
	let pair = pair();
	let account = pair.public().into_account();
	funded_ext(&account, true).execute_with(|| {
		for nonce in 0..16u32 {
			let xt = signed_call(&pair, account.clone(), bogus_cancel(), nonce, 0);
			assert_included(xt, nonce);
		}

		let blocked = signed_call(&pair, account.clone(), bogus_cancel(), 16, 0);
		assert_eq!(
			Executive::apply_extrinsic(blocked).unwrap_err(),
			TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_TX_QUOTA_EXCEEDED
			))
		);

		// Oldest of the 16 was recorded at block 1. One block short of a day: still full.
		System::set_block_number(1 + DAYS - 1);
		let still_blocked = signed_call(&pair, account.clone(), bogus_cancel(), 16, 0);
		assert_eq!(
			Executive::apply_extrinsic(still_blocked).unwrap_err(),
			TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_TX_QUOTA_EXCEEDED
			))
		);

		System::set_block_number(1 + DAYS);
		let xt = signed_call(&pair, account.clone(), bogus_cancel(), 16, 0);
		assert_included(xt, 16);
	});
}

#[test]
fn normal_account_is_not_capped_by_the_high_security_quota() {
	let pair = pair();
	let account = pair.public().into_account();
	funded_ext(&account, false).execute_with(|| {
		for nonce in 0..17u32 {
			let xt = signed_call(&pair, account.clone(), empty_batch_all(), nonce, 0);
			assert_included(xt, nonce);
		}
	});
}

fn assert_included(xt: quantus_runtime::UncheckedExtrinsic, nonce: u32) {
	// The quota gates inclusion; the dispatch outcome is irrelevant here.
	let _ = Executive::apply_extrinsic(xt)
		.unwrap_or_else(|e| panic!("nonce {nonce} should be included: {e:?}"));
}
