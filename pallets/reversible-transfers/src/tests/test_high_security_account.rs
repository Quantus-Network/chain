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
		let hs_user = 1; // reversible from genesis with interceptor=2
		let guardian = 2;
		let dest = 3;
		let amount = 50u128;

		// Compute tx_id BEFORE scheduling (matches pallet logic using current GlobalNonce)
		let call = transfer_call(dest, amount);
		let tx_id = calculate_tx_id::<Test>(hs_user, &call);

		// Schedule a reversible transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(hs_user),
			dest,
			amount
		));

		// Guardian cancels it
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(guardian), tx_id));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
	});
}
