#![cfg(test)]

use crate::tests::{
	mock::*,
	test_reversible_transfers::{calculate_tx_id, transfer_call},
};
use frame_support::assert_ok;

// NOTE: Many of the high security / reversibility behaviors are enforced via SignedExtension or
// external pallets (Recovery/Proxy). They are covered by integration tests in runtime.

#[test]
fn guardian_can_cancel_reversible_transactions_for_hs_account() {
	new_test_ext().execute_with(|| {
		let hs_user = alice(); // reversible from genesis with interceptor=2
		let guardian = bob();
		let dest = charlie();
		let treasury = treasury();
		let amount = 10_000u128; // Use larger amount so volume fee is visible

		// Record initial balances
		let initial_guardian_balance = Balances::free_balance(&guardian);
		let initial_treasury_balance = Balances::free_balance(&treasury);

		// Compute tx_id BEFORE scheduling (matches pallet logic using current GlobalNonce)
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
		// Expected fee: 10,000 * 100 / 10,000 = 100 tokens
		let expected_fee = 100;
		let expected_remaining = amount - expected_fee;

		// Check that guardian received the remaining amount (after fee)
		assert_eq!(
			Balances::free_balance(&guardian),
			initial_guardian_balance + expected_remaining,
			"Guardian should receive remaining amount after volume fee deduction"
		);

		// Check that treasury received the fee
		assert_eq!(
			Balances::free_balance(&treasury),
			initial_treasury_balance + expected_fee,
			"Treasury should receive volume fee from high-security account cancellation"
		);
	});
}
