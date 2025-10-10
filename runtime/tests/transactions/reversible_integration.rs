use crate::common::TestCommons;
use frame_support::{assert_err, assert_ok};
use qp_scheduler::BlockNumberOrTimestamp;
use quantus_runtime::{
	Balances, Recovery, ReversibleTransfers, RuntimeCall, RuntimeOrigin, EXISTENTIAL_DEPOSIT, UNIT,
};
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
fn recoverer() -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(3)
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

        let hs_start = Balances::free_balance(&high_security_account());
        let interceptor_start = Balances::free_balance(&interceptor());
        let a4_start = Balances::free_balance(&acc(4));

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
        let hs_after_cancel = Balances::free_balance(&high_security_account());
        let interceptor_after_cancel = Balances::free_balance(&interceptor());
        let a4_after_cancel = Balances::free_balance(&acc(4));

        assert!(hs_after_cancel <= hs_start - amount, "sender should lose at least the scheduled amount");
        // With volume fee: amount = 10 * EXISTENTIAL_DEPOSIT = 10_000_000_000
        // Fee (10 basis points): 10_000_000_000 * 10 / 10000 = 10_000_000
        // Remaining to interceptor: 10_000_000_000 - 10_000_000 = 9_990_000_000
        let expected_fee = amount * 10 / 10000; // 10 basis points
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

        // 6) Interceptor recovers all funds from high sec account via Recovery pallet

        // 6.1 Interceptor initiates recovery
        assert_ok!(Recovery::initiate_recovery(
            RuntimeOrigin::signed(interceptor()),
            MultiAddress::Id(high_security_account()),
        ));

        // 6.2 Interceptor vouches on recovery
        assert_ok!(Recovery::vouch_recovery(
            RuntimeOrigin::signed(interceptor()),
            MultiAddress::Id(high_security_account()),
            MultiAddress::Id(interceptor()),
        ));

        // 6.3 Interceptor claims recovery
        assert_ok!(Recovery::claim_recovery(
            RuntimeOrigin::signed(interceptor()),
            MultiAddress::Id(high_security_account()),
        ));

        let interceptor_before_recovery = Balances::free_balance(&interceptor());

        // 6.4 Interceptor recovers all funds
        let call = RuntimeCall::Balances(pallet_balances::Call::transfer_all {
            dest: MultiAddress::Id(interceptor()),
            keep_alive: false,
        });
        assert_ok!(Recovery::as_recovered(
            RuntimeOrigin::signed(interceptor()),
            MultiAddress::Id(high_security_account()),
            Box::new(call),
        ));

        let hs_after_recovery = Balances::free_balance(&high_security_account());
        let interceptor_after_recovery = Balances::free_balance(&interceptor());

        // HS should be drained to existential deposit; account 2 increased accordingly
        assert_eq!(hs_after_recovery, EXISTENTIAL_DEPOSIT);

        // Fees - Interceptor spends 11 units in total for all the calls they are making.

        // Interceptor has hs account's balance now
        let estimated_fees = UNIT/100 * 101; // The final recover call costs 1.01 units.
        assert!(
            interceptor_after_recovery >= (hs_after_cancel + interceptor_before_recovery - estimated_fees),
            "recoverer {interceptor_after_recovery} should be at least {hs_after_cancel} + {interceptor_start} - {estimated_fees}"
        );
    });
}

#[test]
fn test_recovery_allows_multiple_recovery_configs() {
	// Test that Account 3 can recover both Account 1 (HS) and Account 2 (interceptor)
	// This proves our inheritance + high security use case will work
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		// Set up Account 1 as high security with Account 2 as interceptor
		let delay = BlockNumberOrTimestamp::BlockNumber(5);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(high_security_account()),
			delay,
			interceptor(),
		));

		// Account 2 initiates recovery of Account 1
		assert_ok!(Recovery::initiate_recovery(
			RuntimeOrigin::signed(interceptor()),
			MultiAddress::Id(high_security_account()),
		));
		assert_ok!(Recovery::vouch_recovery(
			RuntimeOrigin::signed(interceptor()),
			MultiAddress::Id(high_security_account()),
			MultiAddress::Id(interceptor()),
		));
		assert_ok!(Recovery::claim_recovery(
			RuntimeOrigin::signed(interceptor()),
			MultiAddress::Id(high_security_account()),
		));

		// Set up recovery for Account 2 with Account 3 as friend
		assert_ok!(Recovery::create_recovery(
			RuntimeOrigin::signed(interceptor()),
			vec![recoverer()],
			1,
			0,
		));

		// Now Account 3 can recover Account 2
		assert_ok!(Recovery::initiate_recovery(
			RuntimeOrigin::signed(recoverer()),
			MultiAddress::Id(interceptor()),
		));
		assert_ok!(Recovery::vouch_recovery(
			RuntimeOrigin::signed(recoverer()),
			MultiAddress::Id(interceptor()),
			MultiAddress::Id(recoverer()),
		));

		// This should succeed - Account 3 can recover Account 2
		assert_ok!(Recovery::claim_recovery(
			RuntimeOrigin::signed(recoverer()),
			MultiAddress::Id(interceptor()),
		));

		// Verify both proxies exist
		// Account 2 proxies Account 1
		assert_eq!(Recovery::proxy(interceptor()), Some(high_security_account()));
		// Account 3 proxies Account 2
		assert_eq!(Recovery::proxy(recoverer()), Some(interceptor()));

		// Give Account 1 some funds to test transfer
		let transfer_amount = 100 * UNIT;
		assert_ok!(Balances::force_set_balance(
			RuntimeOrigin::root(),
			MultiAddress::Id(high_security_account()),
			transfer_amount,
		));

		// Capture balances before nested transfer
		let hs_balance_before = Balances::free_balance(&high_security_account());
		let recoverer_balance_before = Balances::free_balance(&recoverer());

		// Now test nested as_recovered: Account 3 -> Account 2 -> Account 1
		let inner_call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
			dest: MultiAddress::Id(recoverer()),
			value: transfer_amount / 2, // Transfer half the amount
		});
		let outer_call = RuntimeCall::Recovery(pallet_recovery::Call::as_recovered {
			account: MultiAddress::Id(high_security_account()),
			call: Box::new(inner_call),
		});

		// Account 3 calls as_recovered on Account 2, which contains as_recovered on Account 1
		// This should succeed and transfer funds: Account 1 -> Account 3
		assert_ok!(Recovery::as_recovered(
			RuntimeOrigin::signed(recoverer()),
			MultiAddress::Id(interceptor()),
			Box::new(outer_call),
		));

		// Verify the transfer happened
		let hs_balance_after = Balances::free_balance(&high_security_account());
		let recoverer_balance_after = Balances::free_balance(&recoverer());

		assert_eq!(hs_balance_before, transfer_amount);
		assert!(hs_balance_after < hs_balance_before); // Account 1 lost funds
		assert!(recoverer_balance_after > recoverer_balance_before); // Account 3 gained funds
	});
}
