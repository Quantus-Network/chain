use crate::common::TestCommons;
use frame_support::{assert_err, assert_ok};
use qp_scheduler::BlockNumberOrTimestamp;
use quantus_runtime::{
	Balances, EXISTENTIAL_DEPOSIT, Recovery, ReversibleTransfers, RuntimeCall, RuntimeOrigin, UNIT
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
        let recoverer_start = Balances::free_balance(&recoverer());
        let a4_start = Balances::free_balance(&acc(4));

        // 1) Enable high-security for account 1
        // Use a small delay in blocks for reversible transfers; recovery delay must be >= 7 DAYS
        let hs_delay = BlockNumberOrTimestamp::BlockNumber(5);
        assert_ok!(ReversibleTransfers::set_high_security(
            RuntimeOrigin::signed(high_security_account()),
            hs_delay,
            interceptor(), // interceptor
            recoverer(), // recoverer
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
        assert_eq!(interceptor_after_cancel, interceptor_start + amount, "interceptor should receive the cancelled amount");
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
                recoverer(),
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyHighSecurity
        );

        // 6) Recoverer recovers all funds from high sec account via Recovery pallet

        // 6.1 Recoverer initiates recovery
        assert_ok!(Recovery::initiate_recovery(
            RuntimeOrigin::signed(recoverer()),
            MultiAddress::Id(high_security_account()),
        ));
        // 6.2 Recoverer vouches on recovery
		// #[pallet::weight(T::WeightInfo::vouch_recovery(T::MaxFriends::get()))]
		// pub fn vouch_recovery(
		// 	origin: OriginFor<T>,
		// 	lost: AccountIdLookupOf<T>,
		// 	rescuer: AccountIdLookupOf<T>,
        assert_ok!(Recovery::vouch_recovery(
            RuntimeOrigin::signed(recoverer()),
            MultiAddress::Id(high_security_account()),
            MultiAddress::Id(recoverer()),
        ));

        // 6.3 Recoverer claims recovery
        // pub fn claim_recovery(
		// 	origin: OriginFor<T>,
		// 	account: AccountIdLookupOf<T>,
        assert_ok!(Recovery::claim_recovery(
            RuntimeOrigin::signed(recoverer()),
            MultiAddress::Id(high_security_account()),
        ));

        let recoverer_before_recovery = Balances::free_balance(&recoverer());

        // 6.4 Recoverer recovers all funds
        let call = RuntimeCall::Balances(pallet_balances::Call::transfer_all {
            dest: MultiAddress::Id(recoverer()),
            keep_alive: false,
        });
        assert_ok!(Recovery::as_recovered(
            RuntimeOrigin::signed(recoverer()),
            MultiAddress::Id(high_security_account()),
            Box::new(call),
        ));

        let hs_after_recovery = Balances::free_balance(&high_security_account());
        let recoverer_after_recovery = Balances::free_balance(&recoverer());

        // HS should be drained to existential deposit; account 2 increased accordingly
        assert_eq!(hs_after_recovery, EXISTENTIAL_DEPOSIT);

        // Fees - recoverer spends 11 units in total for all the calls they are making.

        // Recoverer has hs account's balance now
        let estimated_fees = UNIT/100 * 101; // The final recover call costs 1.01 units.
        assert!(
            recoverer_after_recovery >= (hs_after_cancel + recoverer_before_recovery - estimated_fees), 
            "recoverer {recoverer_after_recovery} should be at least {hs_after_cancel} + {recoverer_start} - {estimated_fees}"
        );

    });
}
