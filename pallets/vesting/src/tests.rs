use frame_support::{assert_noop, assert_ok};
use crate::{mock::*, VestingSchedule};
use frame_support::traits::{Currency, ExistenceRequirement};
use frame_support::traits::ExistenceRequirement::AllowDeath;
use sp_runtime::{DispatchError, TokenError};
use super::*;

#[cfg(test)]
fn create_vesting_schedule<Moment: From<u64>>(start: u64, end: u64, amount: Balance) -> VestingSchedule<u64, Balance, Moment> {
    VestingSchedule {
        creator: 1,
        beneficiary: 2,
        start: start.into(),
        end: end.into(),
        amount: amount.into(),
        claimed: 0,
        id: 1
    }
}

#[test]
fn test_vesting_before_start() {
    new_test_ext().execute_with(|| {
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
        let now = 50; // Before vesting starts
        run_to_block(2, now);

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
        assert_eq!(vested, 0);
    });
}

#[test]
fn test_vesting_after_end() {
    new_test_ext().execute_with(|| {
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
        let now = 250; // After vesting ends
        run_to_block(2, now);

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
        assert_eq!(vested, 1000);
    });
}

#[test]
fn test_vesting_halfway() {
    new_test_ext().execute_with(|| {
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
        let now = 150; // Midway through vesting
        run_to_block(2, now);

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
        assert_eq!(vested, 500); // 50% of 1000
    });
}

#[test]
fn test_vesting_start_equals_end() {
    new_test_ext().execute_with(|| {
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 100, 1000);
        let now = 100; // Edge case: start == end
        run_to_block(2, now);

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
        assert_eq!(vested, 1000); // Fully vested immediately
    });
}

#[test]
fn create_vesting_schedule_works() {
    new_test_ext().execute_with(|| {
        // Setup: Account 1 has 1000 tokens
        let start = 1000; // 1 second from genesis
        let end = 2000;   // 2 seconds from genesis
        let amount = 500;

        // Create a vesting schedule
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2, // Beneficiary
            amount,
            start,
            end
        ));

        // Check storage
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        let num_vesting_schedules = ScheduleCounter::<Test>::get();
        assert_eq!(num_vesting_schedules, 1);
        assert_eq!(
            schedule,
            VestingSchedule {
                creator: 1,
                beneficiary: 2,
                amount,
                start,
                end,
                claimed: 0,
                id: 1
            }
        );

        // Check balances
        assert_eq!(Balances::free_balance(1), 100000 - amount); // Sender loses tokens
        assert_eq!(Balances::free_balance(Vesting::account_id()), amount); // Pallet holds tokens
    });
}

#[test]
fn claim_vested_tokens_works() {
    new_test_ext().execute_with(|| {
        let start = 1000;
        let end = 2000;
        let amount = 500;

        // Create a vesting schedule
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            amount,
            start,
            end
        ));

        // Set timestamp to halfway through vesting (50% vested)
        run_to_block(5, 1500);

        // Claim tokens
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));

        // Check claimed amount (50% of 500 = 250)
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 250);
        assert_eq!(Balances::free_balance(2), 2250); // 2000 initial + 250 claimed
        assert_eq!(Balances::free_balance(Vesting::account_id()), 250); // Remaining in pallet

        // Claim again at end
        run_to_block(6, 2000);
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));

        // Check full claim
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 500);
        assert_eq!(Balances::free_balance(2), 2500); // All 500 claimed
        assert_eq!(Balances::free_balance(Vesting::account_id()), 0); // Pallet empty
    });
}

#[test]
fn claim_before_vesting_fails() {
    new_test_ext().execute_with(|| {
        let start = 1000;
        let end = 2000;
        let amount = 500;

        // Create a vesting schedule
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            amount,
            start,
            end
        ));

        // Try to claim (should not do anything)
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));

        // Check no changes
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 0);
        assert_eq!(Balances::free_balance(2), 2000); // No tokens claimed
    });
}

#[test]
fn non_beneficiary_cannot_claim() {
    new_test_ext().execute_with(|| {
        let start = 1000;
        let end = 2000;
        let amount = 500;


        // Start at block 1, timestamp 500
        run_to_block(1, 500);

        // Account 1 creates a vesting schedule for account 2
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2, // Beneficiary is account 2
            amount,
            start,
            end
        ));


        // Advance to halfway through vesting (50% vested)
        run_to_block(2, 1500);

        // Account 3 (not the beneficiary) tries to claim
        assert_noop!(
            Vesting::claim(RuntimeOrigin::signed(3), 3),
            Error::<Test>::NoVestingSchedule
        );

        // Ensure nothing was claimed
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 0);
        assert_eq!(Balances::free_balance(2), 2000); // No change for beneficiary
        assert_eq!(Balances::free_balance(Vesting::account_id()), 500); // Tokens still in pallet

        // Beneficiary (account 2) can claim
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 250); // 50% vested
        assert_eq!(Balances::free_balance(2), 2250);
    });
}

#[test]
fn multiple_beneficiaries_claim_own_schedules() {
    new_test_ext().execute_with(|| {
        let start = 1000;
        let end = 2000;
        let amount = 500;


        // Start at block 1, timestamp 500
        run_to_block(1, 500);

        // Account 1 creates a vesting schedule for account 2
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            amount,
            start,
            end
        ));

        // Account 1 creates a vesting schedule for account 3
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            3,
            amount,
            start,
            end
        ));

        // Advance to halfway through vesting (50% vested)
        run_to_block(2, 1500);

        // Account 2 claims their schedule
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        let schedule2 = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule2.claimed, 250); // 50% of 500
        assert_eq!(Balances::free_balance(2), 2250);

        // Account 3 claims their schedule
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(3), 2));
        let schedule3 = VestingSchedules::<Test>::get(2).expect("Schedule should exist");
        assert_eq!(schedule3.claimed, 250); // 50% of 500
        assert_eq!(Balances::free_balance(3), 250); // 0 initial + 250 claimed

        // Ensure account 2’s schedule is unaffected by account 3’s claim
        let schedule2 = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule2.claimed, 250); // Still only 250 claimed

        // Total in pallet account should reflect both claims
        assert_eq!(Balances::free_balance(Vesting::account_id()), 500); // 1000 - 250 - 250
    });
}

#[test]
fn zero_amount_schedule_fails() {
    new_test_ext().execute_with(|| {

        run_to_block(1, 500);

        assert_noop!(
            Vesting::create_vesting_schedule(
                RuntimeOrigin::signed(1),
                2,
                0, // Zero amount
                1000,
                2000
            ),
            Error::<Test>::InvalidSchedule
        );
    });
}

#[test]
fn claim_with_empty_pallet_fails() {
    new_test_ext().execute_with(|| {
        run_to_block(1, 500);

        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            500,
            1000,
            2000
        ));

        // Drain the pallet account (simulate external interference)
        assert_ok!(Balances::transfer(&Vesting::account_id(), &3, Balances::free_balance(Vesting::account_id()), ExistenceRequirement::AllowDeath));

        run_to_block(2, 1500);

        // Claim should fail due to insufficient funds in pallet
        assert_noop!(
            Vesting::claim(RuntimeOrigin::signed(2), 1),
            DispatchError::Token(TokenError::FundsUnavailable)
        );

        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 0); // No tokens claimed
    });
}

#[test]
fn multiple_schedules_same_beneficiary() {
    new_test_ext().execute_with(|| {
        run_to_block(1, 500);

        // Schedule 1: 500 tokens, 1000-2000
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            500,
            1000,
            2000
        ));

        // Schedule 2: 300 tokens, 1200-1800
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            300,
            1200,
            1800
        ));

        // At 1500: Schedule 1 is 50% (250), Schedule 2 is 50% (150)
        run_to_block(2, 1500);
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 2));

        let schedule1 = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        let schedule2 = VestingSchedules::<Test>::get(2).expect("Schedule should exist");
        let num_schedules = ScheduleCounter::<Test>::get();
        assert_eq!(num_schedules, 2);
        assert_eq!(schedule1.claimed, 250); // Schedule 1
        assert_eq!(schedule2.claimed, 150); // Schedule 2
        assert_eq!(Balances::free_balance(2), 2400); // 2000 + 250 + 150

        // At 2000: Schedule 1 is 100% (500), Schedule 2 is 100% (300)
        run_to_block(3, 2000);
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 2));

        let schedule1 = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        let schedule2 = VestingSchedules::<Test>::get(2).expect("Schedule should exist");
        assert_eq!(schedule1.claimed, 500);
        assert_eq!(schedule2.claimed, 300);
        assert_eq!(Balances::free_balance(2), 2800); // 2000 + 500 + 300
    });
}

#[test]
fn small_time_window_vesting() {
    new_test_ext().execute_with(|| {
        run_to_block(1, 500);

        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            500,
            1000,
            1001 // 1ms duration
        ));

        run_to_block(2, 1000);
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 0); // Not yet vested

        run_to_block(3, 1001);
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 500); // Fully vested
    });
}

#[test]
fn vesting_near_max_timestamp() {
    new_test_ext().execute_with(|| {
        let max = u64::MAX;
        run_to_block(1, max - 1000);

        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            500,
            max - 500,
            max
        ));

        run_to_block(2, max - 250); // Halfway
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        let schedule = VestingSchedules::<Test>::get(&1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 250); // 50% vested

        run_to_block(3, max);
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
        let schedule = VestingSchedules::<Test>::get(&1).expect("Schedule should exist");
        assert_eq!(schedule.claimed, 500);
    });
}

#[test]
fn creator_insufficient_funds_fails() {
    new_test_ext().execute_with(|| {
        // Give account 4 a small balance (less than amount + ED)
        assert_ok!(Balances::transfer(&Vesting::account_id(), &3, Balances::free_balance(Vesting::account_id()), ExistenceRequirement::AllowDeath));

        assert_ok!(Balances::transfer(
            &1,
            &4,
            5, // Only 5 tokens, not enough for 10 + ED
            AllowDeath
        ));

        run_to_block(1, 500);

        // Account 4 tries to create a vesting schedule with insufficient funds
        assert_noop!(
            Vesting::create_vesting_schedule(
                RuntimeOrigin::signed(4),
                2,
                10, // Amount greater than 4’s balance minus ED
                1000,
                2000
            ),
            DispatchError::Token(TokenError::FundsUnavailable)
        );

        // Ensure no schedule was created
        let schedule = VestingSchedules::<Test>::get(&1);
        assert_eq!(schedule, None);

        // Check balances
        assert_eq!(Balances::free_balance(4), 5); // No change
        assert_eq!(Balances::free_balance(Vesting::account_id()), 0); // Nothing transferred
    });
}

#[test]
fn creator_can_cancel_schedule() {
    new_test_ext().execute_with(|| {
        run_to_block(1, 500);

        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            500,
            1000,
            2000
        ));

        run_to_block(2, 1500);

        // Creator (account 1) cancels the schedule
        assert_ok!(Vesting::cancel_vesting_schedule(
            RuntimeOrigin::signed(1),
            1 // First schedule ID
        ));

        // Schedule is gone
        let schedule = VestingSchedules::<Test>::get(1);
        assert_eq!(schedule, None);
        assert_eq!(Balances::free_balance(1), 99750); // 100000 - 500 + 250 refunded
        assert_eq!(Balances::free_balance(2), 2250); // 2000 + 250 claimed
        assert_eq!(Balances::free_balance(Vesting::account_id()), 0);
    });
}

#[test]
fn non_creator_cannot_cancel() {
    new_test_ext().execute_with(|| {
        run_to_block(1, 500);

        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            500,
            1000,
            2000
        ));

        // Account 3 tries to cancel (not the creator)
        assert_noop!(
            Vesting::cancel_vesting_schedule(
                RuntimeOrigin::signed(3),
                1
            ),
            Error::<Test>::NotCreator
        );

        // Schedule still exists
        let schedule = VestingSchedules::<Test>::get(&1).expect("Schedule should exist");
        let num_schedules = ScheduleCounter::<Test>::get();
        assert_eq!(num_schedules, 1);
        assert_eq!(schedule.creator, 1);
    });
}

#[test]
fn creator_can_cancel_after_end() {
    new_test_ext().execute_with(|| {
        run_to_block(1, 500);

        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            500,
            1000,
            2000
        ));

        run_to_block(2, 2500);

        // Creator (account 1) cancels the schedule
        assert_ok!(Vesting::cancel_vesting_schedule(
            RuntimeOrigin::signed(1),
            1 // First schedule ID
        ));

        // Schedule is gone
        let schedule1 = VestingSchedules::<Test>::get(1);
        assert_eq!(schedule1, None);
        assert_eq!(Balances::free_balance(1), 99500); // 100000 - 500
        assert_eq!(Balances::free_balance(2), 2500); // 2000 + 250 claimed
        assert_eq!(Balances::free_balance(Vesting::account_id()), 0);
    });
}
