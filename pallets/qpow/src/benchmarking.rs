//! Benchmarking setup for pallet_qpow

use super::*;
use crate::Pallet as QPoW;
use frame_benchmarking::v2::*;
use frame_support::traits::Hooks;
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::U512;
use sp_runtime::traits::{Get, One, Zero};

#[benchmarks(
    where
    T: Send + Sync,
    T: Config + pallet_timestamp::Config<Moment = u64>,
)]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn on_finalize_normal_block() {
        // Setup state for a normal block (not genesis, not adjustment period)
        let block_number = BlockNumberFor::<T>::from(5u32);
        let initial_distance_threshold = get_initial_distance_threshold::<T>();

        // Set up storage state
        <CurrentDistanceThreshold<T>>::put(initial_distance_threshold);
        <BlocksInPeriod<T>>::put(3u32); // Less than adjustment period
        <HistorySize<T>>::put(3u32);
        <HistoryIndex<T>>::put(0u32);
        <TotalWork<T>>::put(U512::from(1000u64));

        // Set up block time history
        for i in 0..3u32 {
            <BlockTimeHistory<T>>::insert(i, T::TargetBlockTime::get());
        }

        // Set timestamp to simulate time passing
        let now = 5000u64;
        pallet_timestamp::Pallet::<T>::set_timestamp(now.into());
        <LastBlockTime<T>>::put(now.saturating_sub(T::TargetBlockTime::get()));

        #[block]
        {
            QPoW::<T>::on_finalize(block_number);
        }

        // Verify that distance threshold was stored and blocks counter incremented
        assert!(BlockDistanceThresholds::<T>::contains_key(
            block_number + One::one()
        ));
        assert_eq!(<BlocksInPeriod<T>>::get(), 4u32);
    }

    #[benchmark]
    fn on_finalize_adjustment_period() {
        // Setup state for a block that triggers difficulty adjustment
        let block_number = BlockNumberFor::<T>::from(100u32);
        let initial_distance_threshold = get_initial_distance_threshold::<T>();
        let adjustment_period = T::AdjustmentPeriod::get();

        // Set up storage state to trigger adjustment
        <CurrentDistanceThreshold<T>>::put(initial_distance_threshold);
        <BlocksInPeriod<T>>::put(adjustment_period); // Exactly at adjustment period
        <HistorySize<T>>::put(adjustment_period.min(T::BlockTimeHistorySize::get()));
        <HistoryIndex<T>>::put(0u32);
        <TotalWork<T>>::put(U512::from(10000u64));

        // Set up block time history with varying times to trigger adjustment
        let history_size = <HistorySize<T>>::get();
        for i in 0..history_size {
            // Simulate faster than target block times to trigger difficulty increase
            <BlockTimeHistory<T>>::insert(i, T::TargetBlockTime::get() / 2);
        }

        // Set timestamp
        let now = 10000u64;
        pallet_timestamp::Pallet::<T>::set_timestamp(now.into());
        <LastBlockTime<T>>::put(now.saturating_sub(T::TargetBlockTime::get() / 2));

        #[block]
        {
            QPoW::<T>::on_finalize(block_number);
        }

        assert_eq!(<BlocksInPeriod<T>>::get(), 0u32);
        assert!(BlockDistanceThresholds::<T>::contains_key(block_number));
        assert!(<TotalWork<T>>::get() > U512::from(10000u64));
    }

    #[benchmark]
    fn on_finalize_max_history() {
        // Setup state with maximum history size to test worst-case scenario
        let block_number = BlockNumberFor::<T>::from(1000u32);
        let initial_distance_threshold = get_initial_distance_threshold::<T>();
        let max_history = T::BlockTimeHistorySize::get();
        let adjustment_period = T::AdjustmentPeriod::get();

        // Set up storage state
        <CurrentDistanceThreshold<T>>::put(initial_distance_threshold);
        <BlocksInPeriod<T>>::put(adjustment_period);
        <HistorySize<T>>::put(max_history);
        <HistoryIndex<T>>::put(max_history / 2);
        <TotalWork<T>>::put(U512::from(100000u64));

        // Fill up entire history with block times
        for i in 0..max_history {
            <BlockTimeHistory<T>>::insert(
                i,
                T::TargetBlockTime::get().saturating_add(i as u64 * 10),
            );
        }

        // Set timestamp
        let now = 100000u64;
        pallet_timestamp::Pallet::<T>::set_timestamp(now.into());
        <LastBlockTime<T>>::put(now.saturating_sub(T::TargetBlockTime::get()));

        #[block]
        {
            QPoW::<T>::on_finalize(block_number);
        }

        assert!(BlockDistanceThresholds::<T>::contains_key(block_number));
        assert_eq!(<BlocksInPeriod<T>>::get(), 0u32); // Should be reset after adjustment
    }

    impl_benchmark_test_suite!(QPoW, crate::mock::new_test_ext(), crate::mock::Test);
}
