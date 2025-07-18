#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod weights;
use weights::*;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use core::ops::{Shl, Shr};
    use frame_support::sp_runtime::traits::{One, Zero};
    use frame_support::sp_runtime::SaturatedConversion;
    use frame_support::sp_runtime::Saturating;
    use frame_support::{pallet_prelude::*, traits::BuildGenesisConfig, traits::Time};
    use frame_system::pallet_prelude::BlockNumberFor;
    use qpow_math::{get_nonce_distance, get_random_rsa, hash_to_group_bigint, is_valid_nonce};
    use sp_arithmetic::FixedU128;
    use sp_core::U512;
    use sp_std::prelude::*;

    /// Type definitions for QPoW pallet
    pub type NonceType = [u8; 64];
    pub type HeaderType = [u8; 32];
    pub type DistanceThreshold = U512;
    pub type WorkValue = U512;
    pub type Timestamp = u64;
    pub type BlockDuration = u64;
    pub type PeriodCount = u32;
    pub type HistoryIndexType = u32;
    pub type HistorySizeType = u32;
    pub type BlockCount = u32;
    pub type PercentageClamp = u8;
    pub type ThresholdExponent = u32;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::storage]
    pub type BlockDistanceThresholds<T: Config> =
        StorageMap<_, Twox64Concat, BlockNumberFor<T>, DistanceThreshold, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn latest_nonce)]
    pub type LatestNonce<T> = StorageValue<_, NonceType>;

    #[pallet::storage]
    pub type LastBlockTime<T: Config> = StorageValue<_, Timestamp, ValueQuery>;

    #[pallet::storage]
    pub type LastBlockDuration<T: Config> = StorageValue<_, BlockDuration, ValueQuery>;

    #[pallet::storage]
    pub type CurrentDistanceThreshold<T: Config> = StorageValue<_, DistanceThreshold, ValueQuery>;

    #[pallet::storage]
    pub type TotalWork<T: Config> = StorageValue<_, WorkValue, ValueQuery>;

    #[pallet::storage]
    pub type BlocksInPeriod<T: Config> = StorageValue<_, PeriodCount, ValueQuery>;

    #[pallet::storage]
    pub type BlockTimeHistory<T: Config> =
        StorageMap<_, Twox64Concat, HistoryIndexType, BlockDuration, ValueQuery>;

    // Index for current position in ring buffer
    #[pallet::storage]
    pub type HistoryIndex<T: Config> = StorageValue<_, HistoryIndexType, ValueQuery>;

    // Current history size
    #[pallet::storage]
    pub type HistorySize<T: Config> = StorageValue<_, HistorySizeType, ValueQuery>;

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_timestamp::Config {
        /// Overarching event type
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Pallet's weight info
        #[pallet::constant]
        type InitialDistanceThresholdExponent: Get<u32>;

        #[pallet::constant]
        type DifficultyAdjustPercentClamp: Get<PercentageClamp>;

        #[pallet::constant]
        type TargetBlockTime: Get<BlockDuration>;

        #[pallet::constant]
        type AdjustmentPeriod: Get<PeriodCount>;

        #[pallet::constant]
        type BlockTimeHistorySize: Get<HistorySizeType>;

        #[pallet::constant]
        type MaxReorgDepth: Get<BlockCount>;

        /// Fixed point scale for calculations (default: 10^18)
        #[pallet::constant]
        type FixedU128Scale: Get<u128>;

        /// Maximum distance threshold multiplier (default: 4)
        #[pallet::constant]
        type MaxDistanceMultiplier: Get<u32>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub initial_distance: DistanceThreshold,
        #[serde(skip)]
        pub _phantom: PhantomData<T>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                initial_distance: DistanceThreshold::one()
                    .shl(T::InitialDistanceThresholdExponent::get()),
                _phantom: PhantomData,
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            let initial_proof = [0u8; 64];
            <LatestNonce<T>>::put(initial_proof);
            let initial_distance_threshold = get_initial_distance_threshold::<T>();

            // Set current distance_threshold for the genesis block
            <CurrentDistanceThreshold<T>>::put(initial_distance_threshold);

            // Save initial distance_threshold for the genesis block
            let genesis_block_number = BlockNumberFor::<T>::zero();
            <BlockDistanceThresholds<T>>::insert(genesis_block_number, initial_distance_threshold);

            // Initialize the total distance_threshold with the genesis block's distance_threshold
            <TotalWork<T>>::put(WorkValue::one());
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ProofSubmitted {
            nonce: NonceType,
        },
        DistanceThresholdAdjusted {
            old_distance_threshold: DistanceThreshold,
            new_distance_threshold: DistanceThreshold,
            observed_block_time: BlockDuration,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        InvalidSolution,
        ArithmeticOverflow,
    }

    pub fn get_initial_distance_threshold<T: Config>() -> DistanceThreshold {
        DistanceThreshold::one().shl(T::InitialDistanceThresholdExponent::get())
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
            <T as crate::Config>::WeightInfo::on_finalize_max_history()
        }

        /// Called when there is remaining weight at the end of the block.
        fn on_idle(_block_number: BlockNumberFor<T>, _remaining_weight: Weight) -> Weight {
            if <LastBlockTime<T>>::get() == 0 {
                <LastBlockTime<T>>::put(
                    pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>(),
                );
                let initial_distance_threshold: U512 = get_initial_distance_threshold::<T>();
                <CurrentDistanceThreshold<T>>::put(initial_distance_threshold);
            }
            Weight::zero()
        }

        /// Called at the end of each block.
        fn on_finalize(block_number: BlockNumberFor<T>) {
            let blocks = <BlocksInPeriod<T>>::get();
            let current_distance_threshold = <CurrentDistanceThreshold<T>>::get();
            log::debug!(target: "qpow",
                "📢 QPoW: before submit at block {:?}, blocks_in_period={}, current_distance_threshold={}",
                block_number,
                blocks,
                current_distance_threshold.shr(300)
            );
            Self::adjust_distance_threshold();
        }
    }

    impl<T: Config> Pallet<T> {
        // Block time recording for median calculation
        fn record_block_time(block_time: u64) {
            // History size limiter
            let max_history = T::BlockTimeHistorySize::get();
            let mut index = <HistoryIndex<T>>::get();
            let size = <HistorySize<T>>::get();

            // Save block time
            <BlockTimeHistory<T>>::insert(index, block_time);

            // Update index and time
            index = (index.saturating_add(1)) % max_history;
            let new_size = if size < max_history {
                size.saturating_add(1)
            } else {
                max_history
            };

            <HistoryIndex<T>>::put(index);
            <HistorySize<T>>::put(new_size);

            log::debug!(target: "qpow",
                "📊 Recorded block time: {}ms, history size: {}/{}",
                block_time,
                new_size,
                max_history
            );
        }

        // Sum of block times
        pub fn get_block_time_sum() -> u64 {
            let size = <HistorySize<T>>::get();

            if size == 0 {
                return T::TargetBlockTime::get();
            }

            // Take all data
            let mut sum = 0;
            for i in 0..size {
                sum = sum.saturating_add(<BlockTimeHistory<T>>::get(i));
            }

            log::debug!(target: "qpow",
                "📊 Calculated total adjustment period time: {}ms from {} samples",
                sum,
                size
            );

            sum
        }

        // Median calculation
        pub fn get_median_block_time() -> u64 {
            let size = <HistorySize<T>>::get();

            if size == 0 {
                return T::TargetBlockTime::get();
            }

            // Take all data
            let mut times = Vec::with_capacity(size as usize);
            for i in 0..size {
                times.push(<BlockTimeHistory<T>>::get(i));
            }

            log::debug!(target: "qpow", "📊 Block times: {:?}", times);

            // Sort it
            times.sort();

            let median_time = if times.len() % 2 == 0u32 as usize {
                (times[times.len() / 2 - 1].saturating_add(times[times.len() / 2])) / 2
            } else {
                times[times.len() / 2]
            };

            log::debug!(target: "qpow",
                "📊 Calculated median block time: {}ms from {} samples",
                median_time,
                times.len()
            );

            median_time
        }

        fn percentage_change(big_a: U512, big_b: U512) -> (U512, bool) {
            let a = big_a.shr(10);
            let b = big_b.shr(10);
            let (larger, smaller) = if a > b { (a, b) } else { (b, a) };
            let abs_diff = larger - smaller;
            let change = abs_diff.saturating_mul(U512::from(100u64)) / a;

            (change, b >= a)
        }

        fn adjust_distance_threshold() {
            // Get current time
            let now = pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>();
            let last_time = <LastBlockTime<T>>::get();
            let blocks = <BlocksInPeriod<T>>::get();
            let current_distance_threshold = <CurrentDistanceThreshold<T>>::get();
            let current_block_number = <frame_system::Pallet<T>>::block_number();

            // Store distance_threshold for block
            <BlockDistanceThresholds<T>>::insert(current_block_number, current_distance_threshold);

            // Update TotalWork
            let old_total_work = <TotalWork<T>>::get();
            let current_work = Self::get_difficulty();
            // TODO: we should record the actual distance achieved somewhere
            let new_total_work = old_total_work.saturating_add(current_work);
            <TotalWork<T>>::put(new_total_work);
            log::debug!(target: "qpow",
                "Total work: now={}, last_time={}, diff={}",
                new_total_work,
                old_total_work,
                new_total_work - old_total_work
            );

            // Increment number of blocks in period
            <BlocksInPeriod<T>>::put(blocks.saturating_add(1));

            // Only calculate block time if we're past the genesis block
            if current_block_number > One::one() {
                let block_time = now.saturating_sub(last_time);

                log::debug!(target: "qpow",
                    "Time calculation: now={}, last_time={}, diff={}ms",
                    now,
                    last_time,
                    block_time
                );

                // Store the actual block duration
                <LastBlockDuration<T>>::put(block_time);

                // record new block time
                Self::record_block_time(block_time);
            }

            // Add last block time for the next calculations
            <LastBlockTime<T>>::put(now);

            // Should we correct distance_threshold ?
            if blocks >= T::AdjustmentPeriod::get() {
                let history_size = <HistorySize<T>>::get();
                if history_size > 0 {
                    let observed_block_time = Self::get_block_time_sum();
                    let target_time = T::TargetBlockTime::get().saturating_mul(history_size as u64);

                    let new_distance_threshold = Self::calculate_distance_threshold(
                        current_distance_threshold,
                        observed_block_time,
                        target_time,
                    );

                    // Save new distance_threshold
                    <CurrentDistanceThreshold<T>>::put(new_distance_threshold);

                    // Propagate new Event
                    Self::deposit_event(Event::DistanceThresholdAdjusted {
                        old_distance_threshold: current_distance_threshold,
                        new_distance_threshold,
                        observed_block_time,
                    });

                    let (pct_change, is_positive) =
                        Self::percentage_change(current_distance_threshold, new_distance_threshold);

                    log::debug!(target: "qpow",
                        "🟢 Adjusted mining distance threshold {}{}%: {}.. -> {}.. (observed block time: {}ms, target: {}ms) ",
                        if is_positive {"+"} else {"-"},
                        pct_change,
                        current_distance_threshold.shr(300),
                        new_distance_threshold.shr(300),
                        observed_block_time,
                        target_time
                    );
                }

                // Reset counters before new iteration
                <BlocksInPeriod<T>>::put(0);
                <LastBlockTime<T>>::put(now);
            } else if blocks == 0 {
                <LastBlockTime<T>>::put(now);
            }
        }

        pub fn calculate_distance_threshold(
            current_distance_threshold: U512,
            observed_block_time: u64,
            target_block_time: u64,
        ) -> U512 {
            log::debug!(target: "qpow", "📊 Calculating new distance_threshold ---------------------------------------------");
            // Calculate ratio using FixedU128
            let clamp =
                FixedU128::from_rational(T::DifficultyAdjustPercentClamp::get() as u128, 100u128);
            let one = FixedU128::one();
            let ratio =
                FixedU128::from_rational(observed_block_time as u128, target_block_time as u128)
                    .min(one.saturating_add(clamp))
                    .max(one.saturating_sub(clamp));
            log::debug!(target: "qpow", "💧 Clamped block_time ratio as FixedU128: {} ", ratio);

            // Calculate adjusted distance_threshold
            let mut adjusted = if ratio == one {
                current_distance_threshold
            } else {
                let ratio_512 = U512::from(ratio.into_inner());

                // Apply to current distance_threshold, divide first because it's too big already
                let adj =
                    current_distance_threshold.checked_div(U512::from(T::FixedU128Scale::get()));
                match adj {
                    Some(value) => value.saturating_mul(ratio_512),
                    None => {
                        log::warn!(target: "qpow",
                            "Division by zero or overflow in distance_threshold calculation"
                        );
                        return current_distance_threshold;
                    }
                }
            };

            let min_distance = Self::get_min_distance();
            if adjusted < min_distance {
                adjusted = min_distance;
            } else {
                let max_distance = Self::get_max_distance();
                if adjusted > max_distance {
                    adjusted = max_distance;
                }
            }

            log::debug!(target: "qpow",
                "🟢 Current Distance Threshold: {}..",
                current_distance_threshold.shr(300)
            );
            log::debug!(target: "qpow", "🟢 Next Distance Threshold:    {}..", adjusted.shr(300));
            log::debug!(target: "qpow", "🕒 Observed Block Time Sum: {}ms", observed_block_time);
            log::debug!(target: "qpow", "🎯 Target Block Time Sum:   {}ms", target_block_time);

            adjusted
        }
    }

    impl<T: Config> Pallet<T> {
        pub fn is_valid_nonce(
            header: HeaderType,
            nonce: NonceType,
            threshold: DistanceThreshold,
        ) -> bool {
            is_valid_nonce(header, nonce, threshold)
        }

        pub fn get_nonce_distance(
            header: HeaderType, // 256-bit header
            nonce: NonceType,   // 512-bit nonce
        ) -> DistanceThreshold {
            get_nonce_distance(header, nonce)
        }

        pub fn get_random_rsa(header: &HeaderType) -> (DistanceThreshold, DistanceThreshold) {
            get_random_rsa(header)
        }

        pub fn hash_to_group_bigint(
            h: &DistanceThreshold,
            m: &DistanceThreshold,
            n: &DistanceThreshold,
            solution: &DistanceThreshold,
        ) -> DistanceThreshold {
            hash_to_group_bigint(h, m, n, solution)
        }

        // Function used during block import from the network
        pub fn verify_for_import(header: HeaderType, nonce: NonceType) -> bool {
            // During import, we use the current network distance_threshold
            // This value will be correct because we're importing at the appropriate point in the chain
            let current_distance_threshold = Self::get_distance_threshold();

            // Verify using current distance_threshold
            let valid = Self::is_valid_nonce(header, nonce, current_distance_threshold);

            if valid {
                // Store the proof but don't emit event - imported blocks shouldn't trigger events
                <LatestNonce<T>>::put(nonce);
            }

            valid
        }

        // Function used to verify a block that's already in the chain
        pub fn verify_historical_block(
            header: HeaderType,
            nonce: NonceType,
            block_number: BlockNumberFor<T>,
        ) -> bool {
            // Get the stored distance_threshold for this specific block
            let block_distance_threshold = Self::get_distance_threshold_at_block(block_number);

            if block_distance_threshold == U512::zero() {
                // No stored distance_threshold - cannot verify
                return false;
            }

            // Verify with historical distance_threshold
            Self::is_valid_nonce(header, nonce, block_distance_threshold)
        }

        // Function for local mining
        pub fn submit_nonce(header: HeaderType, nonce: NonceType) -> bool {
            let distance_threshold = Self::get_distance_threshold();
            let valid = Self::is_valid_nonce(header, nonce, distance_threshold);

            if valid {
                <LatestNonce<T>>::put(nonce);
                Self::deposit_event(Event::ProofSubmitted { nonce });
            }

            valid
        }

        pub fn get_distance_threshold() -> DistanceThreshold {
            let stored = <CurrentDistanceThreshold<T>>::get();
            if stored == U512::zero() {
                return get_initial_distance_threshold::<T>();
            }
            stored
        }

        pub fn get_min_distance() -> DistanceThreshold {
            DistanceThreshold::one()
        }

        pub fn get_max_distance() -> DistanceThreshold {
            get_initial_distance_threshold::<T>().shl(T::MaxDistanceMultiplier::get())
        }

        pub fn get_difficulty() -> DistanceThreshold {
            Self::get_max_distance() / Self::get_distance_threshold()
        }

        pub fn get_distance_threshold_at_block(
            block_number: BlockNumberFor<T>,
        ) -> DistanceThreshold {
            <BlockDistanceThresholds<T>>::get(block_number)
        }

        pub fn get_total_work() -> WorkValue {
            <TotalWork<T>>::get()
        }

        pub fn get_last_block_time() -> Timestamp {
            <LastBlockTime<T>>::get()
        }

        pub fn get_last_block_duration() -> BlockDuration {
            <LastBlockDuration<T>>::get()
        }

        pub fn get_max_reorg_depth() -> BlockCount {
            T::MaxReorgDepth::get()
        }
    }
}
