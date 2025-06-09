#[path = "common.rs"]
mod common;

use common::TestCommons;
use frame_support::assert_ok;
use resonance_runtime::{Balances, Recovery, RuntimeCall, RuntimeOrigin};
use sp_runtime::MultiAddress;

#[test]
fn test_create_and_initiate_recovery() {
    let mut ext = TestCommons::new_test_ext();
    ext.execute_with(|| {
        let friends = vec![TestCommons::account_id(2), TestCommons::account_id(3)];
        let threshold = 2;
        let delay_period = 10;

        // Create a recovery configuration
        assert_ok!(Recovery::create_recovery(
            RuntimeOrigin::signed(TestCommons::account_id(1)),
            friends.clone(),
            threshold,
            delay_period
        ));

        // Initiate the recovery process
        assert_ok!(Recovery::initiate_recovery(
            RuntimeOrigin::signed(TestCommons::account_id(4)), // A new account initiates
            MultiAddress::Id(TestCommons::account_id(1))
        ));
    });
}

#[test]
fn full_recovery_cycle_works() {
    TestCommons::new_test_ext().execute_with(|| {
        let lost_account = TestCommons::account_id(1);
        let friend_account = TestCommons::account_id(2);
        let recovery_account = TestCommons::account_id(3);

        // Capture initial balances for later verification.
        let initial_lost_balance = Balances::free_balance(&lost_account);
        let initial_recovery_balance = Balances::free_balance(&recovery_account);

        println!("Initial lost account balance: {}", initial_lost_balance);
        println!(
            "Initial recovery account balance: {}",
            initial_recovery_balance
        );

        // 1. Lost account sets up recovery with one friend and no delay.
        assert_ok!(Recovery::create_recovery(
            RuntimeOrigin::signed(lost_account.clone()),
            vec![friend_account.clone()],
            1, // threshold
            0, // delay period in blocks
        ));

        // 2. A new account initiates the recovery for the lost account.
        assert_ok!(Recovery::initiate_recovery(
            RuntimeOrigin::signed(recovery_account.clone()),
            MultiAddress::Id(lost_account.clone()),
        ));

        // 3. The friend vouches for the recovery attempt.
        assert_ok!(Recovery::vouch_recovery(
            RuntimeOrigin::signed(friend_account.clone()),
            MultiAddress::Id(lost_account.clone()),
            MultiAddress::Id(recovery_account.clone()),
        ));

        // 4. The recovery account claims access. This should succeed immediately.
        assert_ok!(Recovery::claim_recovery(
            RuntimeOrigin::signed(recovery_account.clone()),
            MultiAddress::Id(lost_account.clone()),
        ));

        // The balance of the lost account *before* the final transfer.
        let lost_balance_before_transfer = Balances::free_balance(&lost_account);
        println!(
            "Lost account balance before transfer: {}",
            lost_balance_before_transfer
        );

        // 5. As the recovery account, execute a `transfer_all` call on behalf of the lost account.
        let transfer_all_call =
            Box::new(RuntimeCall::Balances(pallet_balances::Call::transfer_all {
                dest: MultiAddress::Id(recovery_account.clone()),
                keep_alive: false, // Drains the account, allows it to be reaped.
            }));

        assert_ok!(Recovery::as_recovered(
            RuntimeOrigin::signed(recovery_account.clone()),
            MultiAddress::Id(lost_account.clone()),
            transfer_all_call,
        ));

        // 6. Verify the outcome.
        let final_lost_balance = Balances::free_balance(&lost_account);
        let final_recovery_balance = Balances::free_balance(&recovery_account);
        let existential_deposit = resonance_runtime::EXISTENTIAL_DEPOSIT;

        println!("Final lost account balance: {}", final_lost_balance);
        println!("Final recovery account balance: {}", final_recovery_balance);
        println!("Existential Deposit: {}", existential_deposit);

        // The lost account should be left with only the existential deposit.
        assert_eq!(final_lost_balance, existential_deposit);

        // The recovery account should have received the funds, minus what was left in the lost account.
        // We allow for a small margin of error to account for transaction fees.
        let expected_recovery_balance =
            initial_recovery_balance + (lost_balance_before_transfer - final_lost_balance);
        let tolerance = expected_recovery_balance / 1000; // 0.1% tolerance
        let lower_bound = expected_recovery_balance - tolerance;
        let upper_bound = expected_recovery_balance + tolerance;

        assert!(
            final_recovery_balance >= lower_bound && final_recovery_balance <= upper_bound,
            "Final recovery balance {} is not within 0.1% of expected {} lower_bound {} upper_bound {}",
            final_recovery_balance,
            expected_recovery_balance,
            lower_bound,
            upper_bound
        );
    });
}
