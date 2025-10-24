//! Benchmarking setup for pallet_qpow

use super::*;
use crate::Pallet as QPoW;
use frame_benchmarking::v2::*;
use frame_support::traits::Hooks;
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U512;
use sp_runtime::traits::Get;

#[benchmarks(
    where
    T: Send + Sync,
    T: Config + pallet_timestamp::Config<Moment = u64>,
)]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn on_finalize_max_history() {
		// Setup state with maximum history size to test worst-case scenario
		let block_number = BlockNumberFor::<T>::from(1000u32);
		frame_system::Pallet::<T>::set_block_number(block_number);

		let initial_difficulty = QPoW::initial_difficulty();

		// Set up storage state
		<CurrentDifficulty<T>>::put(initial_difficulty);
		<TotalWork<T>>::put(U512::from(100000u64));

		// Set timestamp
		let now = 100000u64;
		pallet_timestamp::Pallet::<T>::set_timestamp(now);
		<LastBlockTime<T>>::put(now.saturating_sub(T::TargetBlockTime::get()));

		#[block]
		{
			QPoW::<T>::on_finalize(block_number);
		}
	}

	impl_benchmark_test_suite!(QPoW, crate::mock::new_test_ext(), crate::mock::Test);
}
