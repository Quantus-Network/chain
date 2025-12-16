//! Benchmarking setup for pallet-vesting

use super::*;
use crate::Pallet as Vesting;
use frame_benchmarking::{account as benchmark_account, v2::*};
use frame_support::traits::fungible::Mutate;
use frame_system::RawOrigin;
use sp_runtime::traits::Zero;

const SEED: u32 = 0;

// Helper to fund an account
fn fund_account<T>(account: &T::AccountId, amount: T::Balance)
where
	T: Config + pallet_balances::Config,
{
	let _ = <pallet_balances::Pallet<T> as Mutate<T::AccountId>>::mint_into(account, amount);
}

#[benchmarks(
	where
		T: pallet_balances::Config,
		T::Balance: From<u128>,
		T::Moment: From<u64>,
)]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn create_vesting_schedule() {
		let caller: T::AccountId = benchmark_account("caller", 0, SEED);
		let beneficiary: T::AccountId = benchmark_account("beneficiary", 0, SEED);

		// Fund the caller
		let amount: T::Balance = 1_000_000_000_000u128.into();
		fund_account::<T>(&caller, amount * 2u128.into());

		let start: T::Moment = 1000u64.into();
		let end: T::Moment = 2000u64.into();

		#[extrinsic_call]
		create_vesting_schedule(
			RawOrigin::Signed(caller.clone()),
			beneficiary.clone(),
			amount,
			start,
			end,
		);

		// Verify schedule was created
		assert_eq!(ScheduleCounter::<T>::get(), 1);
		assert!(VestingSchedules::<T>::get(1).is_some());
	}

	#[benchmark]
	fn claim() {
		let creator: T::AccountId = benchmark_account("creator", 0, SEED);
		let beneficiary: T::AccountId = benchmark_account("beneficiary", 0, SEED);

		// Fund the creator
		let amount: T::Balance = 1_000_000_000_000u128.into();
		fund_account::<T>(&creator, amount * 2u128.into());

		// Create a vesting schedule
		let start: T::Moment = 1000u64.into();
		let end: T::Moment = 100000u64.into();

		let _ = Vesting::<T>::create_vesting_schedule(
			RawOrigin::Signed(creator.clone()).into(),
			beneficiary.clone(),
			amount,
			start,
			end,
		);

		let schedule_id = 1u64;

		// Set timestamp to middle of vesting period so some tokens are vested
		pallet_timestamp::Pallet::<T>::set_timestamp(50000u64.into());

		#[extrinsic_call]
		claim(RawOrigin::None, schedule_id);

		// Verify tokens were claimed
		let schedule = VestingSchedules::<T>::get(schedule_id).unwrap();
		assert!(schedule.claimed > T::Balance::zero());
	}

	#[benchmark]
	fn cancel_vesting_schedule() {
		let creator: T::AccountId = benchmark_account("creator", 0, SEED);
		let beneficiary: T::AccountId = benchmark_account("beneficiary", 0, SEED);

		// Fund the creator
		let amount: T::Balance = 1_000_000_000_000u128.into();
		fund_account::<T>(&creator, amount * 2u128.into());

		// Create a vesting schedule
		let start: T::Moment = 1000u64.into();
		let end: T::Moment = 100000u64.into();

		let _ = Vesting::<T>::create_vesting_schedule(
			RawOrigin::Signed(creator.clone()).into(),
			beneficiary.clone(),
			amount,
			start,
			end,
		);

		let schedule_id = 1u64;

		// Set timestamp to middle of vesting period
		pallet_timestamp::Pallet::<T>::set_timestamp(50000u64.into());

		#[extrinsic_call]
		cancel_vesting_schedule(RawOrigin::Signed(creator.clone()), schedule_id);

		// Verify schedule was removed
		assert!(VestingSchedules::<T>::get(schedule_id).is_none());
	}

	#[benchmark]
	fn create_vesting_schedule_with_cliff() {
		let caller: T::AccountId = benchmark_account("caller", 0, SEED);
		let beneficiary: T::AccountId = benchmark_account("beneficiary", 0, SEED);

		// Fund the caller
		let amount: T::Balance = 1_000_000_000_000u128.into();
		fund_account::<T>(&caller, amount * 2u128.into());

		let cliff: T::Moment = 50000u64.into();
		let end: T::Moment = 100000u64.into();

		#[extrinsic_call]
		create_vesting_schedule_with_cliff(
			RawOrigin::Signed(caller.clone()),
			beneficiary.clone(),
			amount,
			cliff,
			end,
		);

		// Verify schedule was created
		assert_eq!(ScheduleCounter::<T>::get(), 1);
		assert!(VestingSchedules::<T>::get(1).is_some());
	}

	#[benchmark]
	fn create_stepped_vesting_schedule() {
		let caller: T::AccountId = benchmark_account("caller", 0, SEED);
		let beneficiary: T::AccountId = benchmark_account("beneficiary", 0, SEED);

		// Fund the caller
		let amount: T::Balance = 1_000_000_000_000u128.into();
		fund_account::<T>(&caller, amount * 2u128.into());

		let start: T::Moment = 1000u64.into();
		let end: T::Moment = 100000u64.into();
		let step_duration: T::Moment = 10000u64.into();

		#[extrinsic_call]
		create_stepped_vesting_schedule(
			RawOrigin::Signed(caller.clone()),
			beneficiary.clone(),
			amount,
			start,
			end,
			step_duration,
		);

		// Verify schedule was created
		assert_eq!(ScheduleCounter::<T>::get(), 1);
		assert!(VestingSchedules::<T>::get(1).is_some());
	}

	impl_benchmark_test_suite!(Vesting, crate::mock::new_test_ext(), crate::mock::Test);
}
