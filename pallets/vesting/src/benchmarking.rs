//! Benchmarking setup for pallet-vesting.

use super::*;
use crate::pallet::{BalanceOf, NextScheduleId, Pallet as Vesting, Schedules, VestingSchedule};
use frame_benchmarking::v2::*;
use frame_support::traits::{
	fungible::{Inspect, Mutate},
	EnsureOrigin, Get,
};
use frame_system::RawOrigin;
use sp_runtime::traits::{Saturating, Zero};

fn set_time<T: pallet_timestamp::Config<Moment = u64>>(now_ms: u64) {
	pallet_timestamp::Now::<T>::put(now_ms);
}

const START: u64 = 0;
const CLIFF: u64 = 0;
const END: u64 = 1_000_000;

fn fund<T: Config>(who: &T::AccountId, amount: BalanceOf<T>) {
	T::Currency::mint_into(who, amount).expect("minting benchmark funds must succeed");
}

fn benchmark_total<T: Config>() -> BalanceOf<T> {
	T::MinimumPayout::get().saturating_mul(1000u32.into())
}

fn admin_origin<T: Config>() -> Result<T::RuntimeOrigin, BenchmarkError> {
	let origin: T::RuntimeOrigin = RawOrigin::Signed(treasury::<T>()?).into();
	T::AdminOrigin::try_origin(origin.clone())
		.map_err(|_| BenchmarkError::Stop("signed treasury is not an admin origin"))?;
	Ok(origin)
}

fn treasury<T: Config>() -> Result<T::AccountId, BenchmarkError> {
	T::TreasuryAccount::get().ok_or(BenchmarkError::Stop("treasury not configured"))
}

/// Insert a schedule directly, with the pot funded to cover it plus its ED buffer.
fn seed_schedule<T: Config>(beneficiary: T::AccountId, total: BalanceOf<T>) -> u64 {
	let schedule_id = NextScheduleId::<T>::get();
	NextScheduleId::<T>::put(schedule_id + 1);
	Schedules::<T>::insert(
		schedule_id,
		VestingSchedule {
			beneficiary,
			start: START,
			cliff: CLIFF,
			end: END,
			total,
			claimed: Zero::zero(),
			last_claim_at: None,
		},
	);
	fund::<T>(
		&Vesting::<T>::pot_account_id(),
		total.saturating_add(T::Currency::minimum_balance()),
	);
	schedule_id
}

#[benchmarks(where T: pallet_timestamp::Config<Moment = u64>)]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn claim() -> Result<(), BenchmarkError> {
		let beneficiary: T::AccountId = account("beneficiary", 0, 0);
		let total = benchmark_total::<T>();
		let schedule_id = seed_schedule::<T>(beneficiary.clone(), total);
		set_time::<T>(END);
		let caller: T::AccountId = whitelisted_caller();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller), schedule_id);

		assert_eq!(T::Currency::balance(&beneficiary), total);
		Ok(())
	}

	#[benchmark]
	fn create_schedule() -> Result<(), BenchmarkError> {
		let origin = admin_origin::<T>()?;
		let treasury = treasury::<T>()?;
		let ed = T::Currency::minimum_balance();
		let total = benchmark_total::<T>();
		fund::<T>(&treasury, total.saturating_mul(2u32.into()));
		fund::<T>(&Vesting::<T>::pot_account_id(), ed);
		let beneficiary: T::AccountId = account("beneficiary", 0, 0);

		#[extrinsic_call]
		_(origin as T::RuntimeOrigin, beneficiary.clone(), START, CLIFF, END, total);

		assert!(Schedules::<T>::iter().any(|(_, s)| s.beneficiary == beneficiary));
		Ok(())
	}

	#[benchmark]
	fn end_schedule() -> Result<(), BenchmarkError> {
		let origin = admin_origin::<T>()?;
		let treasury = treasury::<T>()?;
		fund::<T>(&treasury, T::Currency::minimum_balance());
		let beneficiary: T::AccountId = account("beneficiary", 0, 0);
		let total = benchmark_total::<T>();
		let schedule_id = seed_schedule::<T>(beneficiary.clone(), total);
		// Mid-vesting: both the beneficiary payout and the treasury refund execute.
		set_time::<T>(END / 2);

		#[extrinsic_call]
		_(origin as T::RuntimeOrigin, schedule_id);

		assert!(Schedules::<T>::get(schedule_id).is_none());
		assert!(!T::Currency::balance(&beneficiary).is_zero());
		Ok(())
	}

	#[benchmark]
	fn retarget_schedule() -> Result<(), BenchmarkError> {
		let origin = admin_origin::<T>()?;
		let beneficiary: T::AccountId = account("beneficiary", 0, 0);
		let new_beneficiary: T::AccountId = account("new-beneficiary", 0, 0);
		let total = benchmark_total::<T>();
		let schedule_id = seed_schedule::<T>(beneficiary, total);
		set_time::<T>(END / 2);

		#[extrinsic_call]
		_(origin as T::RuntimeOrigin, schedule_id, new_beneficiary.clone());

		assert_eq!(
			Schedules::<T>::get(schedule_id).expect("schedule persists").beneficiary,
			new_beneficiary
		);
		Ok(())
	}

	impl_benchmark_test_suite!(Vesting, crate::mock::new_test_ext(Vec::new()), crate::mock::Test);
}
