use crate::common::TestCommons;
use frame_support::{assert_err, assert_ok};
use qp_scheduler::BlockNumberOrTimestamp;
use quantus_runtime::{Balances, ReversibleTransfers, RuntimeOrigin, EXISTENTIAL_DEPOSIT};
use sp_runtime::MultiAddress;

fn acc(n: u8) -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(n)
}

fn high_security_account() -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(1)
}
fn interceptor() -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(2)
}

#[test]
fn high_security_end_to_end_flow() {
	// Accounts:
	// 1 = HS account (sender)
	// 2 = interceptor/guardian
	// 3 = recoverer (friend)
	// 4 = recipient of the initial transfer
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
        // Initial balances snapshot

        let hs_start = Balances::free_balance(high_security_account());
        let interceptor_start = Balances::free_balance(interceptor());
        let a4_start = Balances::free_balance(acc(4));

        // 1) Enable high-security for account 1
        // Use a small delay in blocks for reversible transfers; recovery delay must be >= 7 DAYS
        let hs_delay = BlockNumberOrTimestamp::BlockNumber(5);
        assert_ok!(ReversibleTransfers::set_high_security(
            RuntimeOrigin::signed(high_security_account()),
            hs_delay,
            interceptor(), // interceptor
        ));

        // 2) Account 1 makes a normal balances transfer (schedule via pallet extrinsic)
        // NOTE: We exercise the pallet extrinsic path here to avoid manual signature building.
        let amount = 10 * EXISTENTIAL_DEPOSIT;
        assert_ok!(ReversibleTransfers::schedule_transfer(
            RuntimeOrigin::signed(high_security_account()),
            MultiAddress::Id(acc(4)),
            amount,
        ));

        // Verify pending state
        let pending = pallet_reversible_transfers::PendingTransfersBySender::<quantus_runtime::Runtime>::get(high_security_account());
        assert_eq!(pending.len(), 1, "one pending reversible transfer expected");
        let tx_id = pending[0];

        // 3) Guardian (account 2) reverses/cancels it on behalf of 1
        assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(interceptor()), tx_id));

        // Funds should have been moved from 1 to 2 (transfer_on_hold). 4 didn't receive anything.
        let hs_after_cancel = Balances::free_balance(high_security_account());
        let interceptor_after_cancel = Balances::free_balance(interceptor());
        let a4_after_cancel = Balances::free_balance(acc(4));

        assert!(hs_after_cancel <= hs_start - amount, "sender should lose at least the scheduled amount");
        // With volume fee: amount = 10 * EXISTENTIAL_DEPOSIT = 10_000_000_000
        // Fee (1%): 10_000_000_000 * 1 / 100 = 100_000_000
        // Remaining to interceptor: 10_000_000_000 - 100_000_000 = 9_900_000_000
        let expected_fee = amount / 100; // 1% 
        let expected_amount_to_interceptor = amount - expected_fee;
        assert_eq!(interceptor_after_cancel, interceptor_start + expected_amount_to_interceptor, "interceptor should receive the cancelled amount minus volume fee");
        assert_eq!(a4_after_cancel, a4_start, "recipient should not receive funds after cancel");

        // 4) HS account tries to schedule a one-time transfer with a custom delay -> should fail
        let different_delay = BlockNumberOrTimestamp::BlockNumber(10);
        assert_err!(
            ReversibleTransfers::schedule_transfer_with_delay(
                RuntimeOrigin::signed(high_security_account()),
                MultiAddress::Id(acc(4)),
                EXISTENTIAL_DEPOSIT,
                different_delay,
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyReversibleCannotScheduleOneTime
        );

        // 5) HS account tries to call set_high_security again -> should fail
        assert_err!(
            ReversibleTransfers::set_high_security(
                RuntimeOrigin::signed(high_security_account()),
                hs_delay,
                interceptor(),
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyHighSecurity
        );

        // 6) Interceptor recovers all funds from high sec account via recover_funds
        let interceptor_before_recovery = Balances::free_balance(interceptor());

        assert_ok!(ReversibleTransfers::recover_funds(
            RuntimeOrigin::signed(interceptor()),
            high_security_account(),
        ));

        let hs_after_recovery = Balances::free_balance(high_security_account());
        let interceptor_after_recovery = Balances::free_balance(interceptor());

        // HS account should be drained completely (keep_alive: false)
        assert_eq!(hs_after_recovery, 0);

        // Interceptor should have received all the HS account's remaining funds
        assert!(
            interceptor_after_recovery > interceptor_before_recovery,
            "interceptor should have received funds from HS account"
        );
        assert_eq!(
            interceptor_after_recovery,
            interceptor_before_recovery + hs_after_cancel,
            "interceptor should have received the HS account's remaining balance"
        );
    });
}

#[test]
fn test_recover_funds_only_works_for_guardian() {
	// Test that only the guardian (interceptor) can call recover_funds
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		let delay = BlockNumberOrTimestamp::BlockNumber(5);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(high_security_account()),
			delay,
			interceptor(),
		));

		// Non-guardian (account 3) tries to recover funds - should fail
		assert_err!(
			ReversibleTransfers::recover_funds(
				RuntimeOrigin::signed(acc(3)),
				high_security_account(),
			),
			pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::InvalidReverser
		);

		// Guardian (account 2) can recover funds
		let hs_balance_before = Balances::free_balance(high_security_account());
		let interceptor_balance_before = Balances::free_balance(interceptor());

		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(interceptor()),
			high_security_account(),
		));

		// Verify funds were transferred
		let hs_balance_after = Balances::free_balance(high_security_account());
		let interceptor_balance_after = Balances::free_balance(interceptor());

		assert_eq!(hs_balance_after, 0);
		assert_eq!(
			interceptor_balance_after,
			interceptor_balance_before + hs_balance_before,
			"guardian should have received all HS account funds"
		);
	});
}
