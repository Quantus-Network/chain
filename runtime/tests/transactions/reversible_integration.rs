use crate::common::TestCommons;
use frame_support::{assert_err, assert_ok};
use qp_scheduler::BlockNumberOrTimestamp;
use quantus_runtime::{
	Balances, Recovery, ReversibleTransfers, RuntimeCall, RuntimeOrigin, EXISTENTIAL_DEPOSIT,
};
use sp_runtime::MultiAddress;

fn acc(n: u8) -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(n)
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
        let a1_start = Balances::free_balance(&acc(1));
        let a2_start = Balances::free_balance(&acc(2));
        let a3_start = Balances::free_balance(&acc(3));
        let a4_start = Balances::free_balance(&acc(4));

        // 1) Enable high-security for account 1
        // Use a small delay in blocks for reversible transfers; recovery delay must be >= 7 DAYS
        let hs_delay = BlockNumberOrTimestamp::BlockNumber(5);
        assert_ok!(ReversibleTransfers::set_high_security(
            RuntimeOrigin::signed(acc(1)),
            hs_delay,
            acc(2), // interceptor
            acc(3), // recoverer
        ));

        // 2) Account 1 makes a normal balances transfer (schedule via pallet extrinsic)
        // NOTE: We exercise the pallet extrinsic path here to avoid manual signature building.
        let amount = 10 * EXISTENTIAL_DEPOSIT;
        assert_ok!(ReversibleTransfers::schedule_transfer(
            RuntimeOrigin::signed(acc(1)),
            MultiAddress::Id(acc(4)),
            amount,
        ));

        // Verify pending state
        let pending = pallet_reversible_transfers::PendingTransfersBySender::<quantus_runtime::Runtime>::get(acc(1));
        assert_eq!(pending.len(), 1, "one pending reversible transfer expected");
        let tx_id = pending[0];

        // 3) Guardian (account 2) reverses/cancels it on behalf of 1
        assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(acc(2)), tx_id));

        // Funds should have been moved from 1 to 2 (transfer_on_hold). 4 didn't receive anything.
        let a1_after_cancel = Balances::free_balance(&acc(1));
        let a2_after_cancel = Balances::free_balance(&acc(2));
        let a4_after_cancel = Balances::free_balance(&acc(4));

        assert!(a1_after_cancel <= a1_start - amount, "sender should lose at least the scheduled amount");
        assert_eq!(a2_after_cancel, a2_start + amount, "interceptor should receive the cancelled amount");
        assert_eq!(a4_after_cancel, a4_start, "recipient should not receive funds after cancel");

        // 4) Interceptor recovers all funds from account 1 via Recovery pallet
        let call = RuntimeCall::Balances(pallet_balances::Call::transfer_all {
            dest: MultiAddress::Id(acc(2)),
            keep_alive: false,
        });
        assert_ok!(Recovery::as_recovered(
            RuntimeOrigin::signed(acc(2)),
            MultiAddress::Id(acc(1)),
            Box::new(call),
        ));

        let a1_after_recovery = Balances::free_balance(&acc(1));
        let a2_after_recovery = Balances::free_balance(&acc(2));

        // Account 1 should be drained to existential deposit; account 2 increased accordingly
        assert_eq!(a1_after_recovery, EXISTENTIAL_DEPOSIT);
        assert!(a2_after_recovery >= a2_after_cancel, "interceptor balance should not decrease");

        // 5) HS account tries to schedule a one-time transfer with a custom delay -> should fail
        let different_delay = BlockNumberOrTimestamp::BlockNumber(10);
        assert_err!(
            ReversibleTransfers::schedule_transfer_with_delay(
                RuntimeOrigin::signed(acc(1)),
                MultiAddress::Id(acc(4)),
                EXISTENTIAL_DEPOSIT,
                different_delay,
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyReversibleCannotScheduleOneTime
        );

        // 6) HS account tries to call set_high_security again -> should fail
        assert_err!(
            ReversibleTransfers::set_high_security(
                RuntimeOrigin::signed(acc(1)),
                hs_delay,
                acc(2),
                acc(3),
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyHighSecurity
        );

        // Sanity: recoverer untouched
        let a3_final = Balances::free_balance(&acc(3));
        assert_eq!(a3_final, a3_start);
    });
}
