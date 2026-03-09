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
		let hs_user = alice(); // reversible from genesis with interceptor=2
		let guardian = bob();
		let dest = charlie();
		let amount = 10_000u128; // Use larger amount so volume fee is visible

		// Record initial balances
		let initial_guardian_balance = Balances::free_balance(&guardian);
		let initial_total_issuance = TotalIssuance::<Test>::get();

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
