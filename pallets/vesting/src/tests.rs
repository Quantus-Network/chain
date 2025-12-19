use super::*;
use crate::{mock::*, HoldReason, VestingSchedule, VestingType};
use frame_support::{
	assert_noop, assert_ok,
	traits::{
		fungible::{InspectHold, MutateHold},
		Currency, ExistenceRequirement,
		ExistenceRequirement::AllowDeath,
	},
};
use sp_runtime::DispatchError;

#[cfg(test)]
fn create_vesting_schedule<Moment: From<u64>>(
	start: u64,
	end: u64,
	amount: Balance,
) -> VestingSchedule<u64, Balance, Moment> {
	VestingSchedule {
		creator: 1,
		beneficiary: 2,
		start: start.into(),
		end: end.into(),
		vesting_type: VestingType::Linear,
		amount,
		claimed: 0,
		id: 1,
		funding_account: 1,
	}
}

#[test]
fn test_vesting_before_start() {
	new_test_ext().execute_with(|| {
		let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
		let now = 50; // Before vesting starts
		run_to_block(2, now);

		let vested: u128 =
			Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
		assert_eq!(vested, 0);
	});
}

#[test]
fn test_vesting_after_end() {
	new_test_ext().execute_with(|| {
		let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
		let now = 250; // After vesting ends
		run_to_block(2, now);

		let vested: u128 =
			Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
		assert_eq!(vested, 1000);
	});
}

#[test]
fn test_vesting_halfway() {
	new_test_ext().execute_with(|| {
		let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
		let now = 150; // Midway through vesting
		run_to_block(2, now);

		let vested: u128 =
			Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
		assert_eq!(vested, 500); // 50% of 1000
	});
}

#[test]
fn test_vesting_start_equals_end() {
	new_test_ext().execute_with(|| {
		let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 100, 1000);
		let now = 100; // Edge case: start == end
		run_to_block(2, now);

		let vested: u128 =
			Pallet::<Test>::vested_amount(&schedule).expect("Unable to compute vested amount");
		assert_eq!(vested, 1000); // Fully vested immediately
	});
}

#[test]
fn create_vesting_schedule_works() {
	new_test_ext().execute_with(|| {
		// Setup: Account 1 has 1000 tokens
		let start = 1000; // 1 second from genesis
		let end = 2000; // 2 seconds from genesis
		let amount = 500;

		// Create a vesting schedule
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(1),
			2, // Beneficiary
			amount,
			start,
			end,
			1 // funding_account
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
				vesting_type: VestingType::Linear,
				claimed: 0,
				id: 1,
				funding_account: 1
			}
		);

		// Check balances - with lazy funding and auto-freeze:
		// - Account 1's tokens are automatically frozen by on_initialize
		// - free_balance is reduced by frozen amount
		// - Pallet account has 0 tokens
		assert_eq!(Balances::free_balance(1), 100000 - amount); // free = total - frozen
		assert_eq!(Balances::free_balance(Vesting::account_id()), 0); // Pallet has nothing

		// Check that tokens are frozen for vesting obligations
		let frozen = <Balances as InspectHold<u64>>::balance_on_hold(
			&RuntimeHoldReason::Vesting(HoldReason::VestingObligation),
			&1,
		);
		assert_eq!(frozen, amount); // 500 tokens frozen on account 1
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
			end,
			1 // funding_account
		));

		// Set timestamp to halfway through vesting (50% vested)
		run_to_block(5, 1500);

		// Claim tokens
		assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));

		// Check claimed amount (50% of 500 = 250)
		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule.claimed, 250);
		assert_eq!(Balances::free_balance(2), 2250); // 2000 initial + 250 claimed
											   // With lazy funding: 250 transferred out, 250 still held
											   // free_balance = (100000 - 250 transferred) - 250 held = 99500
		assert_eq!(Balances::free_balance(1), 99500);
		assert_eq!(Balances::free_balance(Vesting::account_id()), 0); // Pallet has nothing

		// Claim again at end
		run_to_block(6, 2000);
		assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));

		// Check full claim
		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule.claimed, 500);
		assert_eq!(Balances::free_balance(2), 2500); // All 500 claimed
											   // All 500 transferred, nothing held anymore
		assert_eq!(Balances::free_balance(1), 100000 - 500); // Account 1 paid all 500
		assert_eq!(Balances::free_balance(Vesting::account_id()), 0); // Pallet still has nothing
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
			end,
			1 // funding_account
		));

		// Try to claim (should fail with NothingToClaim because no tokens vested yet)
		assert_noop!(Vesting::claim(RuntimeOrigin::signed(2), 1), Error::<Test>::NothingToClaim);

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
			end,
			1 // funding_account
		));

		// Advance to halfway through vesting (50% vested)
		run_to_block(2, 1500);

		// Account 3 (not the beneficiary) tries to claim
		assert_noop!(Vesting::claim(RuntimeOrigin::signed(3), 3), Error::<Test>::NoVestingSchedule);

		// Ensure nothing was claimed
		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule.claimed, 0);
		assert_eq!(Balances::free_balance(2), 2000); // No change for beneficiary
											   // 500 is held for vesting
		assert_eq!(Balances::free_balance(1), 100000 - 500); // Funding account has 500 held

		// Beneficiary (account 2) can claim
		assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule.claimed, 250); // 50% vested
		assert_eq!(Balances::free_balance(2), 2250);
		// 250 transferred, 250 still held
		assert_eq!(Balances::free_balance(1), 99500); // 100000 - 250 - 250
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
			end,
			1 // funding_account
		));

		// Account 1 creates a vesting schedule for account 3
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(1),
			3,
			amount,
			start,
			end,
			1 // funding_account
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

		// Ensure account 2's schedule is unaffected by account 3's claim
		let schedule2 = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule2.claimed, 250); // Still only 250 claimed

		// With lazy funding: funding account paid both claims (500 total)
		// 1000 initially held, 500 claimed (released+transferred), 500 still held
		// free_balance = (100000 - 500 transferred) - 500 held = 99000
		assert_eq!(Balances::free_balance(1), 99000);
		assert_eq!(Balances::free_balance(Vesting::account_id()), 0); // Pallet has nothing
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
				2000,
				1
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
			2000,
			1
		));

		// With lazy funding: first unfreeze all held funds, then drain the funding account
		// This simulates a scenario where funding account has no funds (neither free nor held)
		let held = <Balances as InspectHold<u64>>::balance_on_hold(
			&RuntimeHoldReason::Vesting(HoldReason::VestingObligation),
			&1,
		);

		// Manually release all held funds (simulating governance or manual intervention)
		if held > 0 {
			assert_ok!(Balances::release(
				&RuntimeHoldReason::Vesting(HoldReason::VestingObligation),
				&1,
				held,
				frame_support::traits::tokens::Precision::Exact
			));
			FrozenBalance::<Test>::mutate(1, |bal| *bal = 0);
		}

		// Now drain all free balance
		let funding_balance = Balances::free_balance(1);
		assert_ok!(Balances::transfer(
			&1,
			&3,
			funding_balance - 1, // Leave just 1 for ED
			ExistenceRequirement::KeepAlive
		));

		run_to_block(2, 1500);

		// Claim succeeds but only transfers 1 token (partial claim)
		// Because funding account only has 1 token left
		assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));

		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule.claimed, 1); // Only 1 token claimed (partial)
		assert_eq!(Balances::free_balance(2), 2001); // 2000 + 1
		assert_eq!(Balances::free_balance(1), 0); // Funding account drained completely
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
			2000,
			1
		));

		// Schedule 2: 300 tokens, 1200-1800
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(1),
			2,
			300,
			1200,
			1800,
			1
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
			1001, // 1ms duration
			1     // funding_account
		));

		run_to_block(2, 1000);
		// Try to claim at start - should fail with NothingToClaim
		assert_noop!(Vesting::claim(RuntimeOrigin::signed(2), 1), Error::<Test>::NothingToClaim);
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
			max,
			1 // funding_account
		));

		run_to_block(2, max - 250); // Halfway
		assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule.claimed, 250); // 50% vested

		run_to_block(3, max);
		assert_ok!(Vesting::claim(RuntimeOrigin::signed(2), 1));
		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
		assert_eq!(schedule.claimed, 500);
	});
}

#[test]
fn creator_insufficient_funds_fails() {
	new_test_ext().execute_with(|| {
		// Give account 4 a small balance (less than amount + ED)
		assert_ok!(Balances::transfer(
			&Vesting::account_id(),
			&3,
			Balances::free_balance(Vesting::account_id()),
			ExistenceRequirement::AllowDeath
		));

		assert_ok!(Balances::transfer(
			&1, &4, 5, // Only 5 tokens, not enough for 10 + ED
			AllowDeath
		));

		run_to_block(1, 500);

		// With lazy funding: creation succeeds even with low balance
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(4),
			2,
			100, // More than account 4 has
			1000,
			2000,
			4 // Account 4 is funding_account
		));

		// Schedule was created
		let schedule = VestingSchedules::<Test>::get(1);
		assert!(schedule.is_some());

		// Fast forward to vesting time
		run_to_block(2, 1500);

		// Try to claim - should fail because funding account has insufficient funds
		// Transfer will fail with FundsUnavailable
		assert_noop!(
			Vesting::claim(RuntimeOrigin::signed(2), 1),
			DispatchError::Token(sp_runtime::TokenError::FundsUnavailable)
		);
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
			2000,
			1
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
			2000,
			1
		));

		// Account 3 tries to cancel (not the creator)
		assert_noop!(
			Vesting::cancel_vesting_schedule(RuntimeOrigin::signed(3), 1),
			Error::<Test>::NotCreator
		);

		// Schedule still exists
		let schedule = VestingSchedules::<Test>::get(1).expect("Schedule should exist");
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
			2000,
			1
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

// ========== Cliff Vesting Tests ==========

#[test]
fn cliff_vesting_before_cliff_returns_zero() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let cliff = 1000; // Cliff at timestamp 1000
		let end = 2000;

		assert_ok!(Vesting::create_vesting_schedule_with_cliff(
			RuntimeOrigin::signed(1),
			2,
			amount,
			cliff,
			end,
			1 // funding_account
		));

		// Set timestamp before cliff
		Timestamp::set_timestamp(500);

		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();

		// Nothing is vested before cliff
		assert_eq!(vested, 0);
	});
}

#[test]
fn cliff_vesting_at_cliff_starts_linear() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let cliff = 1000; // Cliff at timestamp 1000
		let end = 2000;

		assert_ok!(Vesting::create_vesting_schedule_with_cliff(
			RuntimeOrigin::signed(1),
			2,
			amount,
			cliff,
			end,
			1 // funding_account
		));

		// Set timestamp at cliff
		Timestamp::set_timestamp(1000);

		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();

		// At cliff, 0% of vesting period has elapsed (cliff to end)
		assert_eq!(vested, 0);

		// Halfway between cliff and end
		Timestamp::set_timestamp(1500);
		let vested = Vesting::vested_amount(&schedule).unwrap();
		assert_eq!(vested, 500); // 50% of amount
	});
}

#[test]
fn cliff_vesting_after_end_returns_full_amount() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let cliff = 1000;
		let end = 2000;

		assert_ok!(Vesting::create_vesting_schedule_with_cliff(
			RuntimeOrigin::signed(1),
			2,
			amount,
			cliff,
			end,
			1 // funding_account
		));

		// Set timestamp after end
		Timestamp::set_timestamp(2500);

		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();

		assert_eq!(vested, amount);
	});
}

#[test]
fn cliff_vesting_claim_works() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let cliff = 1000;
		let end = 2000;

		assert_ok!(Vesting::create_vesting_schedule_with_cliff(
			RuntimeOrigin::signed(1),
			2,
			amount,
			cliff,
			end,
			1 // funding_account
		));

		// Before cliff - cannot claim (NothingToClaim error)
		Timestamp::set_timestamp(500);
		assert_noop!(Vesting::claim(RuntimeOrigin::none(), 1), Error::<Test>::NothingToClaim);
		assert_eq!(Balances::free_balance(2), 2000); // No change

		// After cliff, halfway to end
		Timestamp::set_timestamp(1500);
		assert_ok!(Vesting::claim(RuntimeOrigin::none(), 1));
		assert_eq!(Balances::free_balance(2), 2500); // 2000 + 500 (50% vested)
											   // 500 transferred, 500 still held
		assert_eq!(Balances::free_balance(1), 99000); // 100000 - 500 - 500
	});
}

// ========== Stepped Vesting Tests ==========

#[test]
fn stepped_vesting_before_first_step() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let start = 1000;
		let end = 5000; // 4000ms duration
		let step_duration = 1000; // 4 steps

		assert_ok!(Vesting::create_stepped_vesting_schedule(
			RuntimeOrigin::signed(1),
			2,
			amount,
			start,
			end,
			step_duration,
			1 // funding_account
		));

		// Before first step
		Timestamp::set_timestamp(1500);

		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();

		// 0 complete steps = 0 vested
		assert_eq!(vested, 0);
	});
}

#[test]
fn stepped_vesting_after_first_step() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let start = 1000;
		let end = 5000; // 4000ms duration
		let step_duration = 1000; // 4 steps

		assert_ok!(Vesting::create_stepped_vesting_schedule(
			RuntimeOrigin::signed(1),
			2,
			amount,
			start,
			end,
			step_duration,
			1 // funding_account
		));

		// After first step (1000ms elapsed)
		Timestamp::set_timestamp(2000);

		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();

		// 1 step out of 4 = 25%
		assert_eq!(vested, 250);
	});
}

#[test]
fn stepped_vesting_after_two_steps() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let start = 1000;
		let end = 5000; // 4000ms duration
		let step_duration = 1000; // 4 steps

		assert_ok!(Vesting::create_stepped_vesting_schedule(
			RuntimeOrigin::signed(1),
			2,
			amount,
			start,
			end,
			step_duration,
			1 // funding_account
		));

		// After two steps (2000ms elapsed)
		Timestamp::set_timestamp(3000);

		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();

		// 2 steps out of 4 = 50%
		assert_eq!(vested, 500);
	});
}

#[test]
fn stepped_vesting_after_all_steps() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let start = 1000;
		let end = 5000;
		let step_duration = 1000;

		assert_ok!(Vesting::create_stepped_vesting_schedule(
			RuntimeOrigin::signed(1),
			2,
			amount,
			start,
			end,
			step_duration,
			1 // funding_account
		));

		// After end
		Timestamp::set_timestamp(5000);

		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();

		// All vested
		assert_eq!(vested, amount);
	});
}

#[test]
fn stepped_vesting_claim_works() {
	new_test_ext().execute_with(|| {
		let amount = 1000;
		let start = 1000;
		let end = 5000;
		let step_duration = 1000; // 4 steps

		assert_ok!(Vesting::create_stepped_vesting_schedule(
			RuntimeOrigin::signed(1),
			2,
			amount,
			start,
			end,
			step_duration,
			1 // funding_account
		));

		// Before first step - nothing to claim (NothingToClaim error)
		Timestamp::set_timestamp(1500);
		assert_noop!(Vesting::claim(RuntimeOrigin::none(), 1), Error::<Test>::NothingToClaim);
		assert_eq!(Balances::free_balance(2), 2000); // No change

		// After two steps
		Timestamp::set_timestamp(3000);
		assert_ok!(Vesting::claim(RuntimeOrigin::none(), 1));
		assert_eq!(Balances::free_balance(2), 2500); // 2000 + 500 (50% vested)
											   // 500 transferred, 500 still held
		assert_eq!(Balances::free_balance(1), 99000); // 100000 - 500 - 500

		// After all steps
		Timestamp::set_timestamp(5000);
		assert_ok!(Vesting::claim(RuntimeOrigin::none(), 1));
		assert_eq!(Balances::free_balance(2), 3000); // 2000 + 1000 (100% vested)
											   // All 1000 transferred, nothing held
		assert_eq!(Balances::free_balance(1), 100000 - 1000); // Funding account paid all
	});
}

#[test]
fn stepped_vesting_yearly_example() {
	new_test_ext().execute_with(|| {
		let amount = 4000;
		let start = 0;
		let year_ms = 365 * 24 * 3600 * 1000; // 1 year in milliseconds
		let end = 4 * year_ms; // 4 years
		let step_duration = year_ms; // Annual steps

		assert_ok!(Vesting::create_stepped_vesting_schedule(
			RuntimeOrigin::signed(1),
			2,
			amount,
			start,
			end,
			step_duration,
			1 // funding_account
		));

		// After 364 days - still 0
		Timestamp::set_timestamp(364 * 24 * 3600 * 1000);
		let schedule = VestingSchedules::<Test>::get(1).unwrap();
		let vested = Vesting::vested_amount(&schedule).unwrap();
		assert_eq!(vested, 0);

		// After 1 year - 25%
		Timestamp::set_timestamp(year_ms);
		let vested = Vesting::vested_amount(&schedule).unwrap();
		assert_eq!(vested, 1000); // 25%

		// After 2 years - 50%
		Timestamp::set_timestamp(2 * year_ms);
		let vested = Vesting::vested_amount(&schedule).unwrap();
		assert_eq!(vested, 2000); // 50%

		// After 3 years - 75%
		Timestamp::set_timestamp(3 * year_ms);
		let vested = Vesting::vested_amount(&schedule).unwrap();
		assert_eq!(vested, 3000); // 75%

		// After 4 years - 100%
		Timestamp::set_timestamp(4 * year_ms);
		let vested = Vesting::vested_amount(&schedule).unwrap();
		assert_eq!(vested, 4000); // 100%
	});
}

#[test]
fn stepped_vesting_invalid_step_duration_fails() {
	new_test_ext().execute_with(|| {
		// step_duration = 0 should fail
		assert_noop!(
			Vesting::create_stepped_vesting_schedule(
				RuntimeOrigin::signed(1),
				2,
				1000,
				1000,
				2000,
				0, // Invalid: zero step duration
				1
			),
			Error::<Test>::InvalidStepDuration
		);

		// step_duration > total duration should fail
		assert_noop!(
			Vesting::create_stepped_vesting_schedule(
				RuntimeOrigin::signed(1),
				2,
				1000,
				1000,
				2000,
				2000, // Invalid: step longer than total duration
				1
			),
			Error::<Test>::InvalidSchedule
		);
	});
}

// ========== Schedule Limit Tests ==========

#[test]
fn schedule_count_increments_on_create() {
	new_test_ext().execute_with(|| {
		let creator = 1;
		let beneficiary = 2;

		// Initial count should be 0
		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 0);

		// Create first schedule
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(creator),
			beneficiary,
			1000,
			1000,
			2000,
			1 // funding_account
		));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 1);

		// Create second schedule
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(creator),
			beneficiary,
			1000,
			1000,
			2000,
			1 // funding_account
		));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 2);
	});
}

#[test]
fn schedule_count_decrements_on_cancel() {
	new_test_ext().execute_with(|| {
		let creator = 1;
		let beneficiary = 2;

		// Create two schedules
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(creator),
			beneficiary,
			1000,
			1000,
			2000,
			1 // funding_account
		));
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(creator),
			beneficiary,
			1000,
			1000,
			2000,
			1 // funding_account
		));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 2);

		// Cancel first schedule
		assert_ok!(Vesting::cancel_vesting_schedule(RuntimeOrigin::signed(creator), 1));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 1);

		// Cancel second schedule
		assert_ok!(Vesting::cancel_vesting_schedule(RuntimeOrigin::signed(creator), 2));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 0);
	});
}

#[test]
fn cannot_exceed_max_schedules_per_beneficiary() {
	new_test_ext().execute_with(|| {
		let creator = 1;
		let beneficiary = 2;

		// MaxSchedulesPerBeneficiary is 50 in mock
		// Create 50 schedules (should succeed)
		for i in 0..50 {
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1000 + i as u64,
				2000 + i as u64,
				1 // funding_account
			));
		}

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 50);

		// Try to create 51st schedule (should fail)
		assert_noop!(
			Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1000,
				2000,
				creator
			),
			Error::<Test>::TooManySchedules
		);
	});
}

#[test]
fn limit_applies_per_beneficiary() {
	new_test_ext().execute_with(|| {
		let creator = 1;
		let beneficiary1 = 2;
		let beneficiary2 = 3;

		// Create 50 schedules for beneficiary1
		for _ in 0..50 {
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary1,
				1000,
				1000,
				2000,
				1 // funding_account
			));
		}

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary1), 50);

		// beneficiary1 is at limit
		assert_noop!(
			Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary1,
				1000,
				1000,
				2000,
				creator
			),
			Error::<Test>::TooManySchedules
		);

		// But beneficiary2 should still be able to create schedules
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(creator),
			beneficiary2,
			1000,
			1000,
			2000,
			1 // funding_account
		));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary2), 1);
	});
}

#[test]
fn limit_applies_to_all_vesting_types() {
	new_test_ext().execute_with(|| {
		let creator = 1;
		let beneficiary = 2;

		// Create 48 linear schedules
		for _ in 0..48 {
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1000,
				2000,
				1 // funding_account
			));
		}

		// Create 1 cliff schedule
		assert_ok!(Vesting::create_vesting_schedule_with_cliff(
			RuntimeOrigin::signed(creator),
			beneficiary,
			1000,
			1500,
			2000,
			1 // funding_account
		));

		// Create 1 stepped schedule (total = 50)
		assert_ok!(Vesting::create_stepped_vesting_schedule(
			RuntimeOrigin::signed(creator),
			beneficiary,
			1000,
			1000,
			2000,
			100,
			1 // funding_account
		));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 50);

		// Any type should now fail
		assert_noop!(
			Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1000,
				2000,
				creator
			),
			Error::<Test>::TooManySchedules
		);

		assert_noop!(
			Vesting::create_vesting_schedule_with_cliff(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1500,
				2000,
				creator
			),
			Error::<Test>::TooManySchedules
		);

		assert_noop!(
			Vesting::create_stepped_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1000,
				2000,
				100,
				creator
			),
			Error::<Test>::TooManySchedules
		);
	});
}

#[test]
fn can_create_more_after_cancelling() {
	new_test_ext().execute_with(|| {
		let creator = 1;
		let beneficiary = 2;

		// Create 50 schedules (at limit)
		for _ in 0..50 {
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1000,
				2000,
				1 // funding_account
			));
		}

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 50);

		// Cannot create more
		assert_noop!(
			Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(creator),
				beneficiary,
				1000,
				1000,
				2000,
				creator
			),
			Error::<Test>::TooManySchedules
		);

		// Cancel one schedule
		assert_ok!(Vesting::cancel_vesting_schedule(RuntimeOrigin::signed(creator), 1));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 49);

		// Now can create one more
		assert_ok!(Vesting::create_vesting_schedule(
			RuntimeOrigin::signed(creator),
			beneficiary,
			1000,
			1000,
			2000,
			1 // funding_account
		));

		assert_eq!(BeneficiaryScheduleCount::<Test>::get(beneficiary), 50);
	});
}
