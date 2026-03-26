//! Benchmarking setup for pallet_qpow

use super::*;
use crate::Pallet as QPoW;
use frame_benchmarking::v2::*;
use frame_support::traits::Hooks;
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::traits::Get;

#[benchmarks(
    where
    T: Send + Sync,
    T: Config + pallet_timestamp::Config<Moment = u64>,
)]
mod benchmarks {
	use super::*;

	/// Benchmark for the on_finalize hook which performs EMA-based difficulty adjustment.
	#[benchmark]
	fn on_finalize() {
		// Setup state with typical block for difficulty adjustment
		let block_number = BlockNumberFor::<T>::from(1000u32);
		frame_system::Pallet::<T>::set_block_number(block_number);

		let initial_difficulty = QPoW::<T>::initial_difficulty();

		// Set up storage state
		<CurrentDifficulty<T>>::put(initial_difficulty);

		// Set timestamp
		let now = 100000u64;
		pallet_timestamp::Pallet::<T>::set_timestamp(now);
		<LastBlockTime<T>>::put(now.saturating_sub(T::TargetBlockTime::get()));

		// Initialize EMA
		<BlockTimeEma<T>>::put(T::TargetBlockTime::get());

		#[block]
		{
			QPoW::<T>::on_finalize(block_number);
		}
	}

	impl_benchmark_test_suite!(QPoW, crate::mock::new_test_ext(), crate::mock::Test);
}
