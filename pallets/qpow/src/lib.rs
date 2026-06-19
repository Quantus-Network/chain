#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

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
	use core::ops::Shr;
	use frame_support::{
		pallet_prelude::*,
		sp_runtime::{traits::One, SaturatedConversion},
		traits::{BuildGenesisConfig, Time},
	};
	use frame_system::pallet_prelude::BlockNumberFor;
	use qpow_math::{get_nonce_hash, is_valid_nonce};
	use sp_core::U512;

	pub type NonceType = [u8; 64];
	pub type Difficulty = U512;
	pub type WorkValue = U512;
	pub type Timestamp = u64;
	pub type BlockDuration = u64;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	pub type LastBlockTime<T: Config> = StorageValue<_, Timestamp, ValueQuery>;

	#[pallet::storage]
	pub type LastBlockDuration<T: Config> = StorageValue<_, BlockDuration, ValueQuery>;

	#[pallet::storage]
	pub type CurrentDifficulty<T: Config> = StorageValue<_, Difficulty, ValueQuery>;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		#[pallet::constant]
		type InitialDifficulty: Get<U512>;

		#[pallet::constant]
		type TargetBlockTime: Get<BlockDuration>;

		#[pallet::constant]
		type MaxReorgDepth: Get<u32>;

		type WeightInfo: WeightInfo;
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub initial_difficulty: Difficulty,
		#[serde(skip)]
		pub _phantom: PhantomData<T>,
	}

	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { initial_difficulty: T::InitialDifficulty::get(), _phantom: PhantomData }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			let initial_difficulty = T::InitialDifficulty::get();

			<CurrentDifficulty<T>>::put(initial_difficulty);

			log::info!(target: "qpow", "Genesis: Set initial difficulty to {:x}",
				initial_difficulty.low_u64());
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		ProofSubmitted {
			nonce: NonceType,
			difficulty: U512,
			hash_achieved: U512,
		},
		DifficultyAdjusted {
			old_difficulty: Difficulty,
			new_difficulty: Difficulty,
			observed_block_time: BlockDuration,
		},
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
			<T as crate::Config>::WeightInfo::on_finalize()
		}

		/// Called at the end of each block to adjust mining difficulty.
		fn on_finalize(block_number: BlockNumberFor<T>) {
			let current_difficulty = <CurrentDifficulty<T>>::get();
			log::debug!(target: "qpow",
				"📢 QPoW: before submit at block {:?}, current_difficulty={:?}",
				block_number,
				current_difficulty.low_u64()
			);

			Self::adjust_difficulty();
		}
	}

	impl<T: Config> Pallet<T> {
		fn percentage_change(big_a: U512, big_b: U512) -> (U512, bool) {
			let a = big_a.shr(10);
			let b = big_b.shr(10);

			let abs_diff = a.abs_diff(b);
			let change = abs_diff
				.saturating_mul(U512::from(100u64))
				.checked_div(a)
				.unwrap_or(U512::zero());

			(change, b >= a)
		}

		fn adjust_difficulty() {
			let now = pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>();
			let last_time = <LastBlockTime<T>>::get();
			let current_difficulty = <CurrentDifficulty<T>>::get();
			let current_block_number = <frame_system::Pallet<T>>::block_number();

			// Calculate block time (use target for genesis block)
			let block_time = if current_block_number > One::one() {
				let duration = now.saturating_sub(last_time);

				log::debug!(target: "qpow",
					"Time calculation: now={}, last_time={}, diff={}ms",
					now,
					last_time,
					duration
				);

				<LastBlockDuration<T>>::put(duration);

				duration
			} else {
				T::TargetBlockTime::get()
			};

			<LastBlockTime<T>>::put(now);

			let target_time = T::TargetBlockTime::get();
			let new_difficulty =
				Self::calculate_difficulty(current_difficulty, block_time, target_time);

			<CurrentDifficulty<T>>::put(new_difficulty);

			log::debug!(target: "qpow", "Stored new difficulty: {}",
				new_difficulty.low_u128());

			Self::deposit_event(Event::DifficultyAdjusted {
				old_difficulty: current_difficulty,
				new_difficulty,
				observed_block_time: block_time,
			});

			let (pct_change, is_positive) =
				Self::percentage_change(current_difficulty, new_difficulty);

			log::debug!(target: "qpow",
				"🟢 Adjusted mining difficulty {}{}%: {:x} -> {:x} (block time: {}ms, target: {}ms) ",
				if is_positive {"+"} else {"-"},
				pct_change,
				current_difficulty.low_u64(),
				new_difficulty.low_u64(),
				block_time,
				target_time
			);
		}

		/// Calculate new difficulty based on block time.
		/// Uses the same formula as Ethereum PoW:
		/// diff = parent_diff + (parent_diff / 2048) * max(1 - block_time / divisor, -99)
		///
		/// The divisor is 8 seconds for a 12s target (scales proportionally).
		/// This creates these zones:
		/// - < divisor: difficulty increases by 1/2048 (~0.05%)
		/// - divisor to 2*divisor: no change
		/// - 2*divisor to 3*divisor: difficulty decreases by 1/2048
		/// - etc, up to max decrease of 99/2048 (~4.8%)
		pub fn calculate_difficulty(
			parent_difficulty: U512,
			block_time_ms: u64,
			target_time_ms: u64,
		) -> U512 {
			log::debug!(target: "qpow", "📊 Calculating new difficulty ---------------------------------------------");

			// Divisor scales with target: 8s divisor for 12s target
			// divisor = target * 8 / 12 = target * 2 / 3
			let divisor_ms = (target_time_ms * 2 / 3).max(1);
			let time_factor = (block_time_ms / divisor_ms) as i64;
			let adjustment = core::cmp::max(1i64 - time_factor, -99i64);

			log::debug!(target: "qpow", "Block time: {}ms, divisor: {}ms, time_factor: {}, adjustment: {}", 
				block_time_ms, divisor_ms, time_factor, adjustment);

			// Difficulty increment = parent_diff / 2048
			let increment = parent_difficulty / U512::from(2048u64);

			// Calculate new difficulty
			let new_difficulty = if adjustment >= 0 {
				parent_difficulty
					.saturating_add(increment.saturating_mul(U512::from(adjustment as u64)))
			} else {
				let decrease = increment.saturating_mul(U512::from((-adjustment) as u64));
				parent_difficulty.saturating_sub(decrease)
			};

			// Apply min/max bounds
			let min_difficulty = Self::get_min_difficulty();
			let max_difficulty = Self::get_max_difficulty();

			let bounded = if new_difficulty < min_difficulty {
				log::warn!("Min difficulty achieved, clipping to: {:x}", min_difficulty.low_u64());
				min_difficulty
			} else if new_difficulty > max_difficulty {
				log::warn!("Max difficulty achieved, clipping to: {:x}", max_difficulty.low_u64());
				max_difficulty
			} else {
				new_difficulty
			};

			log::debug!(target: "qpow",
				"🟢 Current Difficulty: {:x}",
				parent_difficulty.low_u64()
			);
			log::debug!(target: "qpow", "🟢 Next Difficulty:    {:x}", bounded.low_u64());
			log::debug!(target: "qpow", "🕒 Block Time: {}ms", block_time_ms);

			bounded
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn is_valid_nonce(
			block_hash: [u8; 32],
			nonce: NonceType,
			difficulty: Difficulty,
		) -> (bool, U512) {
			is_valid_nonce(block_hash, nonce, difficulty)
		}

		pub fn get_nonce_hash(
			block_hash: [u8; 32], // 256-bit block hash
			nonce: NonceType,     // 512-bit nonce
		) -> U512 {
			get_nonce_hash(block_hash, nonce)
		}

		// Shared verification logic
		fn verify_nonce_internal(block_hash: [u8; 32], nonce: NonceType) -> (bool, U512, U512) {
			if nonce == [0u8; 64] {
				log::warn!(
					"verify_nonce should not be called with 0 nonce, but was for block_hash: {:?}",
					block_hash
				);
				return (false, U512::zero(), U512::zero());
			}
			let difficulty = Self::get_difficulty();
			let (valid, hash_achieved) = Self::is_valid_nonce(block_hash, nonce, difficulty);

			log::debug!(
				"verify_nonce_internal: block_hash: {:?}, nonce: {:?}, valid: {:?}, difficulty: {:?}, hash_achieved: {:?}",
				hex::encode(block_hash),
				nonce,
				valid,
				difficulty,
				hash_achieved
			);
			(valid, difficulty, hash_achieved)
		}

		// Block verification with event emission
		pub fn verify_nonce_on_import_block(block_hash: [u8; 32], nonce: NonceType) -> bool {
			let (valid, difficulty, hash_achieved) = Self::verify_nonce_internal(block_hash, nonce);
			if valid {
				Self::deposit_event(Event::ProofSubmitted { nonce, difficulty, hash_achieved });
			}

			valid
		}

		pub fn verify_nonce_local_mining(block_hash: [u8; 32], nonce: NonceType) -> bool {
			let (verify, _, _) = Self::verify_nonce_internal(block_hash, nonce);
			verify
		}

		/// Verify the nonce and return the block's work used for chain selection.
		///
		/// IMPORTANT: despite the legacy name, this returns the *target* difficulty the
		/// block had to satisfy (the network difficulty at this height), NOT the achieved
		/// difficulty derived from the winning hash. Target-based work matches Bitcoin
		/// (`2^256/(target+1)`) and Ethereum PoW (sum of the `difficulty` field): every
		/// block at a given difficulty contributes an identical, deterministic amount of
		/// work, so cumulative chain work tracks expended hash power instead of being
		/// dominated by a single lucky hash.
		///
		/// The runtime API name is intentionally left unchanged so this can ship as an
		/// on-chain-only upgrade: because the metric is determined by the value this
		/// returns (the client merely accumulates `parent_work + value`), upgrading the
		/// on-chain Wasm flips the whole network to target-based work at the `set_code`
		/// block, with no coordinated node-binary upgrade and no resync. Renaming the API
		/// would break that compatibility, so defer the rename to a later release once all
		/// nodes run a binary that expects the new name.
		///
		/// Note: This is called via runtime API from the client side. Runtime API
		/// calls execute in a temporary context where state changes are discarded,
		/// so we don't emit events here.
		pub fn verify_and_get_achieved_difficulty(
			block_hash: [u8; 32],
			nonce: NonceType,
		) -> (bool, U512) {
			let (valid, difficulty, _) = Self::verify_nonce_internal(block_hash, nonce);
			let block_work = if valid { difficulty } else { U512::zero() };
			(valid, block_work)
		}

		pub fn initial_difficulty() -> Difficulty {
			T::InitialDifficulty::get()
		}

		pub fn get_difficulty() -> Difficulty {
			let stored = <CurrentDifficulty<T>>::get();
			let initial = Self::initial_difficulty();

			if stored == U512::zero() {
				log::warn!(target: "qpow", "Stored difficulty is zero, using initial: {:x}", initial.low_u64());
				return initial;
			}
			stored
		}

		pub fn get_min_difficulty() -> Difficulty {
			// Minimum difficulty floor - same as Ethereum's minimum (2^17 = 131072)
			U512::from(131_072u64)
		}

		pub fn get_max_difficulty() -> Difficulty {
			U512::MAX
		}

		pub fn get_last_block_time() -> Timestamp {
			<LastBlockTime<T>>::get()
		}

		pub fn get_last_block_duration() -> BlockDuration {
			<LastBlockDuration<T>>::get()
		}

		pub fn get_max_reorg_depth() -> u32 {
			T::MaxReorgDepth::get()
		}
	}
}
