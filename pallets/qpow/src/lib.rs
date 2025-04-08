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
	use num_traits::Float;
	use frame_support::sp_runtime::traits::{One, Zero};
	use sp_core::U512;
use sp_std::prelude::*;
	use qpow_math::RsaShortcutMath;
	use qpow_math::MAX_DISTANCE;

	#[pallet::pallet]
	pub struct Pallet<T>(_);


	#[pallet::storage]
	pub type BlockDifficulties<T: Config> = StorageMap<_,Twox64Concat,BlockNumberFor<T>,u64,ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn latest_nonce)]
	pub type LatestNonce<T> = StorageValue<_, [u8; 64]>;

	#[pallet::storage]
	pub type LastBlockTime<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type LastBlockDuration<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type CurrentDifficulty<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type TotalDifficulty<T: Config> = StorageValue<_, u128, ValueQuery>;

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
		type InitialDifficulty: Get<u64>;

		#[pallet::constant]
		type MinDifficulty: Get<u64>;

		#[pallet::constant]
		type TargetBlockTime: Get<u64>;

		#[pallet::constant]
		type AdjustmentPeriod: Get<u32>;

		#[pallet::constant]
		type DampeningFactor: Get<u64>;

		#[pallet::constant]
		type BlockTimeHistorySize: Get<u32>;

		#[pallet::constant]
		type MaxReorgDepth: Get<u32>;
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub initial_difficulty: u64,
		#[serde(skip)]
		pub _phantom: PhantomData<T>,
	}

	//#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self {
				initial_difficulty: T::InitialDifficulty::get(),
				_phantom: PhantomData,
			}
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			let initial_proof = [0u8; 64];
			<LatestNonce<T>>::put(initial_proof);

			//Set current difficulty for the genesis block
			<CurrentDifficulty<T>>::put(self.initial_difficulty);

			//Save initial difficulty for the genesis block
			let genesis_block_number = BlockNumberFor::<T>::zero();
			<BlockDifficulties<T>>::insert(genesis_block_number, self.initial_difficulty);

			//Initialize the total difficulty with the genesis block's difficulty
			<TotalDifficulty<T>>::put(self.initial_difficulty as u128);
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
		DifficultyAdjusted {
			old_difficulty: u64,
			new_difficulty: u64,
			median_block_time: u64,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidSolution,
		ArithmeticOverflow
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
				<CurrentDifficulty<T>>::put(T::InitialDifficulty::get());
			}
			Weight::zero()
		}

		/// Called at the end of each block.
		fn on_finalize(block_number: BlockNumberFor<T>) {
			let blocks = <BlocksInPeriod<T>>::get();
			let current_difficulty = <CurrentDifficulty<T>>::get();
			log::info!(
				"📢 QPoW: before submit at block {:?}, blocks_in_period={}, current_difficulty={}",
				block_number,
				blocks,
				current_difficulty
			);
			Self::adjust_difficulty();
		}
	}

	impl<T: Config> Pallet<T> {

		// Block time recording for median calculation
		fn record_block_time(block_time: u64) {
			//History size limiter
			let max_history = T::BlockTimeHistorySize::get();
			let mut index = <HistoryIndex<T>>::get();
			let size = <HistorySize<T>>::get();

			//Save block time
			<BlockTimeHistory<T>>::insert(index, block_time);

			// Update index and time
			index = (index + 1) % max_history;
			let new_size = if size < max_history { size + 1 } else { max_history };

			<HistoryIndex<T>>::put(index);
			<HistorySize<T>>::put(new_size);

			log::info!(
				"📊 Recorded block time: {}ms, history size: {}/{}",
				block_time,
				new_size,
				max_history
			);
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

			// Sort it
			times.sort();

			let median_time = if times.len() % 2 == 0u32 as usize {
				(times[times.len() / 2 - 1] + times[times.len() / 2]) / 2
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

		fn adjust_difficulty() {
			// Get current time
			let now = pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>();
			let last_time = <LastBlockTime<T>>::get();
			let blocks = <BlocksInPeriod<T>>::get();
			let current_difficulty = <CurrentDifficulty<T>>::get();
			let current_block_number = <frame_system::Pallet<T>>::block_number();

			// Store difficulty for block
			<BlockDifficulties<T>>::insert(current_block_number, current_difficulty);

			let total_difficulty = <TotalDifficulty<T>>::get();
			let new_total_difficulty = total_difficulty.saturating_add(current_difficulty as u128);
			<TotalDifficulty<T>>::put(new_total_difficulty);

			// Increment number of blocks in period
			<BlocksInPeriod<T>>::put(blocks + 1);

			// Only calculate block time if we're past the genesis block
			if current_block_number > One::one() {
				let block_time = now.saturating_sub(last_time);

				log::info!(
					"Time calculation: now={}, last_time={}, diff={}ms",
					now,
					last_time,
					block_time
				);

				// Additional protection against super high block times
				let max_reasonable_time = T::TargetBlockTime::get() * 10;
				// takes smaller value
				let capped_time = block_time.min(max_reasonable_time);

				// Store the actual block duration
				<LastBlockDuration<T>>::put(capped_time);


				if block_time != capped_time {
					log::warn!(
						"Capped excessive block time from {}ms to {}ms",
						block_time,
						capped_time
					);
				}

				// record new block time
				Self::record_block_time(block_time);
			}

			// Add last block time for the next calculations
			<LastBlockTime<T>>::put(now);

			// Should we correct difficulty ?
			if blocks >= T::AdjustmentPeriod::get() {
				if <HistorySize<T>>::get() > 0 {
					let median_block_time = Self::get_median_block_time();
					let target_time = T::TargetBlockTime::get();

					let new_difficulty = Self::calculate_difficulty(
						current_difficulty,
						median_block_time,
						target_time
					);

					// Save new difficulty
					<CurrentDifficulty<T>>::put(new_difficulty);

					// Propagate new Event
					Self::deposit_event(Event::DifficultyAdjusted {
						old_difficulty: current_difficulty,
						new_difficulty,
						median_block_time,
					});

					log::info!(
                    "Adjusted mining difficulty: {} -> {} (median block time: {}ms, target: {}ms)",
                    current_difficulty,
                    new_difficulty,
                    median_block_time,
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

		pub fn calculate_difficulty(
			current_difficulty: u64,
			average_block_time: u64,
			target_block_time: u64,
			) -> u64 {

			log::info!("📊 Calculating new difficulty ---------------------------------------------");
			log::info!("🟢 Current Difficulty: {}",current_difficulty);
			log::info!("🕒 Average Block Time: {}ms",average_block_time);
			log::info!("🎯 Target Block Time: {}ms",target_block_time);
				
			// Calculate ratio
			let ratio = (average_block_time as f32) / (target_block_time as f32);

			// Calculate power factor
			let change_factor = <f64 as Float>::powf(ratio as f64, 1.0/16.0);
			//let change_factor = <f64 as Float>::exp(<f64 as Float>::ln(ratio as f64) / 16 as f64);

			let dampening = T::DampeningFactor::get();
			let dampening_factor = dampening as f64;

			// Apply additional damping
			let damped_ratio = 1.0 + (change_factor - 1.0) / dampening_factor;

			log::info!("Change factor: {}, damped ratio: {}", change_factor, damped_ratio);

			// Calculate adjusted difficulty
			//let adjusted = (current_difficulty as f64 / change_factor) as u64;
			let adjusted = (current_difficulty as f64 / damped_ratio) as u64;
			
			// Enforce bounds
			adjusted.min(MAX_DISTANCE - 1).max(T::MinDifficulty::get())
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {	
	}

	impl<T: Config> Pallet<T> {

		pub fn is_valid_nonce(header: [u8; 32], nonce: [u8; 64], difficulty: u64) -> bool {
			return RsaShortcutMath::is_valid_nonce(header, nonce, difficulty);
		}

		pub fn get_nonce_distance(
			header: [u8; 32], // 256-bit header
			nonce: [u8; 64],  // 512-bit nonce
		) -> u64 {
			return RsaShortcutMath::get_nonce_distance(header, nonce);
		}
		pub fn get_random_rsa(header: &[u8; 32]) -> (U512, U512) {
			return RsaShortcutMath::get_random_rsa(header);
		}

		pub fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512 {
			return RsaShortcutMath::hash_to_group_bigint(h, m, n, solution);
		}

		// Function used during block import from the network
		pub fn verify_for_import(header: [u8; 32], nonce: [u8; 64]) -> bool {
			// During import, we use the current network difficulty
			// This value will be correct because we're importing at the appropriate point in the chain
			let current_difficulty = Self::get_difficulty();

			// Verify using current difficulty
			let valid = Self::is_valid_nonce(header, nonce, current_difficulty);

			if valid {
				// Store the proof but don't emit event - imported blocks shouldn't trigger events
				<LatestNonce<T>>::put(nonce);
				// No new events for imported blocks
			}

			valid
		}

		// Function used to verify a block that's already in the chain
		pub fn verify_historical_block(header: [u8; 32], nonce: [u8; 64], block_number: BlockNumberFor<T>) -> bool {
			// Get the stored difficulty for this specific block
			let block_difficulty = Self::get_difficulty_at_block(block_number);

			if block_difficulty == 0 {
				// No stored difficulty - cannot verify
				return false;
			}

			// Verify with historical difficulty
			Self::is_valid_nonce(header, nonce, block_difficulty)
		}


		// Function for local mining
		pub fn submit_nonce(header: [u8; 32], nonce: [u8; 64]) -> bool {
			let difficulty = Self::get_difficulty();
			let valid = Self::is_valid_nonce(header, nonce, difficulty);

			if valid {
				<LatestNonce<T>>::put(nonce);
				Self::deposit_event(Event::ProofSubmitted { nonce });
			}

			valid
		}

		pub fn get_difficulty() -> u64 {
			let stored = <CurrentDifficulty<T>>::get();
			if stored == 0 {
				return GenesisConfig::<T>::default().initial_difficulty;
			}
			stored
		}

		pub fn get_max_distance() -> u64 {
			MAX_DISTANCE
		}

		pub fn get_difficulty_at_block(block_number: BlockNumberFor<T>) -> u64 {
			let difficulty = <BlockDifficulties<T>>::get(block_number);
			if difficulty == 0 {
				0
			} else {
				difficulty
			}
		}

		pub fn get_total_difficulty() -> u128 {
			<TotalDifficulty<T>>::get()
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