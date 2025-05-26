#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{pallet_prelude::*, traits::BuildGenesisConfig, traits::Time};
	use frame_support::sp_runtime::SaturatedConversion;
	use frame_system::pallet_prelude::BlockNumberFor;
	use sp_arithmetic::FixedU128;
	use frame_support::sp_runtime::Saturating;
	use frame_support::sp_runtime::traits::{One, Zero};
	use sp_core::U512;
	use core::ops::{Shl, Shr};
	use sp_std::prelude::*;
	use qpow_math::{is_valid_nonce, get_nonce_distance, get_random_rsa, hash_to_group_bigint};

	#[pallet::pallet]
	pub struct Pallet<T>(_);


	#[pallet::storage]
	pub type BlockDistanceThresholds<T: Config> = StorageMap<_,Twox64Concat,BlockNumberFor<T>, U512, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn latest_nonce)]
	pub type LatestNonce<T> = StorageValue<_, [u8; 64]>;

	#[pallet::storage]
	pub type LastBlockTime<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type LastBlockDuration<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type CurrentDistanceThreshold<T: Config> = StorageValue<_, U512, ValueQuery>;

	#[pallet::storage]
	pub type TotalWork<T: Config> = StorageValue<_, U512, ValueQuery>;

	#[pallet::storage]
	pub type BlocksInPeriod<T: Config> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	pub type BlockTimeHistory<T: Config> = StorageMap<_, Twox64Concat, u32, u64, ValueQuery>;

	// Index for current position in ring buffer
	#[pallet::storage]
	pub type HistoryIndex<T: Config> = StorageValue<_, u32, ValueQuery>;

	// Current history size
	#[pallet::storage]
	pub type HistorySize<T: Config> = StorageValue<_, u32, ValueQuery>;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type WeightInfo: WeightInfo;

		#[pallet::constant]
		type InitialDistanceThresholdExponent: Get<u32>;

		#[pallet::constant]
		type DifficultyAdjustPercentClamp: Get<u8>;

		#[pallet::constant]
		type TargetBlockTime: Get<u64>;

		#[pallet::constant]
		type AdjustmentPeriod: Get<u32>;

		#[pallet::constant]
		type BlockTimeHistorySize: Get<u32>;

		#[pallet::constant]
		type MaxReorgDepth: Get<u32>;
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub initial_distance: U512,
		#[serde(skip)]
		pub _phantom: PhantomData<T>,
	}

	//#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self {
				initial_distance: U512::one().shl(T::InitialDistanceThresholdExponent::get()),
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
			<TotalWork<T>>::put(U512::one());
		}
	}

	//TODO all this should be generated with benchmarks

	pub trait WeightInfo {
		fn submit_proof() -> Weight;
	}

	pub struct DefaultWeightInfo;

	impl WeightInfo for DefaultWeightInfo {
		fn submit_proof() -> Weight {
			Weight::from_parts(10_000, 0)
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		ProofSubmitted {
			nonce: [u8; 64],
		},
		DistanceThresholdAdjusted {
			old_distance_threshold: U512,
			new_distance_threshold: U512,
			observed_block_time: u64,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidSolution,
		ArithmeticOverflow
	}

	pub fn get_initial_distance_threshold<T: Config>() -> U512 {
		U512::one().shl(T::InitialDistanceThresholdExponent::get())
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
			Weight::zero()
		}

		/// Called when there is remaining weight at the end of the block.
		fn on_idle(_block_number: BlockNumberFor<T>, _remaining_weight: Weight) -> Weight {
			if <LastBlockTime<T>>::get() == 0 {
				<LastBlockTime<T>>::put(pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>());
				let initial_distance_threshold: U512 = get_initial_distance_threshold::<T>();
				<CurrentDistanceThreshold<T>>::put(initial_distance_threshold);
			}
			Weight::zero()
		}

		/// Called at the end of each block.
		fn on_finalize(block_number: BlockNumberFor<T>) {
			let blocks = <BlocksInPeriod<T>>::get();
			let current_distance_threshold = <CurrentDistanceThreshold<T>>::get();
			log::info!(
				"📢 QPoW: before submit at block {:?}, blocks_in_period={}, current_distance_threshold={}",
				block_number,
				blocks,
				current_distance_threshold
			);
			Self::adjust_distance_threshold();
		}
	}

	impl<T: Config> Pallet<T> {

		const FIXED_U128_SCALE: u128 = 1_000_000_000_000_000_000; // 10^18

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
			let new_size = if size < max_history { size.saturating_add(1) } else { max_history };

			<HistoryIndex<T>>::put(index);
			<HistorySize<T>>::put(new_size);

			log::info!(
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

			log::info!(
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

			log::info!("📊 Block times: {:?}", times);

			// Sort it
			times.sort();


			let median_time = if times.len() % 2 == 0u32 as usize {
				(times[times.len() / 2 - 1].saturating_add(times[times.len() / 2])) / 2
			} else {
				times[times.len() / 2]
			};

			log::info!(
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
			let new_total_work = old_total_work.saturating_add(current_work);
			<TotalWork<T>>::put(new_total_work);
			log::info!(
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

				log::info!(
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
						target_time
					);

					// Save new distance_threshold
					<CurrentDistanceThreshold<T>>::put(new_distance_threshold);

					// Propagate new Event
					Self::deposit_event(Event::DistanceThresholdAdjusted {
						old_distance_threshold: current_distance_threshold,
						new_distance_threshold,
						observed_block_time,
					});

					let (pct_change, is_positive) = Self::percentage_change(current_distance_threshold, new_distance_threshold);

					log::info!(
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
			}
			else if blocks == 0 {
				<LastBlockTime<T>>::put(now);
			}
		}

		pub fn calculate_distance_threshold(
			current_distance_threshold: U512,
			observed_block_time: u64,
			target_block_time: u64,
		) -> U512 {
			log::info!("📊 Calculating new distance_threshold ---------------------------------------------");
			// Calculate ratio using FixedU128
			let clamp = FixedU128::from_rational(T::DifficultyAdjustPercentClamp::get() as u128, 100u128);
			let one = FixedU128::from_rational(1u128, 1u128);
			let ratio = FixedU128::from_rational(observed_block_time as u128, target_block_time as u128)
				.min(one.saturating_add(clamp))
				.max(one.saturating_sub(clamp));
			log::info!("💧 Clamped block_time ratio as FixedU128: {} ", ratio);

			// Calculate adjusted distance_threshold
			let mut adjusted = if ratio == one {
				current_distance_threshold
			} else {
				let ratio_512 = U512::from(ratio.into_inner());

				// Apply to current distance_threshold, divide first because it's too big already
				let adj = current_distance_threshold.checked_div(U512::from(Self::FIXED_U128_SCALE));
				match adj {
					Some(value) => value.saturating_mul(ratio_512),
					None => {
						log::warn!("Division by zero or overflow in distance_threshold calculation");
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

			log::info!("🟢 Current Distance Threshold: {}..", current_distance_threshold.shr(100));
			log::info!("🟢 Next Distance Threshold:    {}..", adjusted.shr(100));
			log::info!("🕒 Observed Block Time Sum: {}ms", observed_block_time);
			log::info!("🎯 Target Block Time Sum:   {}ms", target_block_time);

			adjusted
		}
	}

	impl<T: Config> Pallet<T> {

		pub fn is_valid_nonce(header: [u8; 32], nonce: [u8; 64], threshold: U512) -> bool {
			is_valid_nonce(header, nonce, threshold)
		}

		pub fn get_nonce_distance(
			header: [u8; 32], // 256-bit header
			nonce: [u8; 64],  // 512-bit nonce
		) -> U512 {
			get_nonce_distance(header, nonce)
		}

		pub fn get_random_rsa(header: &[u8; 32]) -> (U512, U512) {
			get_random_rsa(header)
		}

		pub fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512 {
			hash_to_group_bigint(h, m, n, solution)
		}

		// Function used during block import from the network
		pub fn verify_for_import(header: [u8; 32], nonce: [u8; 64]) -> bool {
			// During import, we use the current network distance_threshold
			// This value will be correct because we're importing at the appropriate point in the chain
			let current_distance_threshold = Self::get_distance_threshold();

			// Verify using current distance_threshold
			let valid = Self::is_valid_nonce(header, nonce, current_distance_threshold);

			if valid {
				// Store the proof but don't emit event - imported blocks shouldn't trigger events
				<LatestNonce<T>>::put(nonce);
				// No new events for imported blocks
			}

			valid
		}

		// Function used to verify a block that's already in the chain
		pub fn verify_historical_block(header: [u8; 32], nonce: [u8; 64], block_number: BlockNumberFor<T>) -> bool {
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
		pub fn submit_nonce(header: [u8; 32], nonce: [u8; 64]) -> bool {
			let distance_threshold = Self::get_distance_threshold();
			let valid = Self::is_valid_nonce(header, nonce, distance_threshold);

			if valid {
				<LatestNonce<T>>::put(nonce);
				Self::deposit_event(Event::ProofSubmitted { nonce });
			}

			valid
		}

		pub fn get_distance_threshold() -> U512 {
			let stored = <CurrentDistanceThreshold<T>>::get();
			if stored == U512::zero() {
				return get_initial_distance_threshold::<T>();
			}
			stored
		}

		pub fn get_min_distance() -> U512 {
			U512::one()
		}

		pub fn get_max_distance() -> U512 {
			get_initial_distance_threshold::<T>().shl(2)
		}

		pub fn get_difficulty() -> U512 {
			Self::get_max_distance() / Self::get_distance_threshold()
		}

		pub fn get_distance_threshold_at_block(block_number: BlockNumberFor<T>) -> U512 {
			<BlockDistanceThresholds<T>>::get(block_number)
		}

		pub fn get_total_work() -> U512 {
			<TotalWork<T>>::get()
		}

		pub fn get_last_block_time() -> u64 {
			<LastBlockTime<T>>::get()
		}

		pub fn get_last_block_duration() -> u64 {
			<LastBlockDuration<T>>::get()
		}

		pub fn get_max_reorg_depth() -> u32 { T::MaxReorgDepth::get() }

	}
}