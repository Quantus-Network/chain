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
	use qpow_math::{achieved_difficulty_from_hash, get_nonce_hash, is_valid_nonce};
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

	// Exponential Moving Average of block times (in milliseconds)
	#[pallet::storage]
	pub type BlockTimeEma<T: Config> = StorageValue<_, BlockDuration, ValueQuery>;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		/// Genesis mining difficulty. Must satisfy
		/// `InitialDifficulty >= DifficultyBoundDivisor` so the per-block step
		/// `parent_difficulty / DifficultyBoundDivisor` is at least 1.
		#[pallet::constant]
		type InitialDifficulty: Get<U512>;

		/// Ethereum's `DIFF_BOUND_DIVISOR` (EIP-2). The per-block unit step is
		/// `parent_difficulty / DifficultyBoundDivisor`, applied additively in
		/// both directions. Standard Ethereum value is `2048`.
		#[pallet::constant]
		type DifficultyBoundDivisor: Get<U512>;

		/// Bucket size in milliseconds for computing the signed adjustment
		/// factor (EIP-2's `// 10` divisor, generalised). The factor is
		/// `MaxUpAdjFactor - (block_time_ms / BlockTimeBucketMs)`, then
		/// clamped to `[MaxDownAdjFactor, MaxUpAdjFactor]`. With
		/// `MaxUpAdjFactor = 1` the no-change band is `[bucket, 2*bucket)`;
		/// pick `bucket ≈ 2 * target / 3` to centre the band on the target.
		#[pallet::constant]
		type BlockTimeBucketMs: Get<u64>;

		/// Maximum upward adjustment factor (Ethereum Homestead = 1,
		/// Byzantium = 2 when the parent has uncles; Quantus has no uncles,
		/// so use 1).
		#[pallet::constant]
		type MaxUpAdjFactor: Get<i32>;

		/// Minimum (most-negative) adjustment factor cap. Ethereum uses
		/// `-99`, which triggers only when a single block takes
		/// `(MaxUpAdjFactor - MaxDownAdjFactor) * BlockTimeBucketMs` or
		/// longer (≈13 minutes for the standard `(1, -99, 8s)` set).
		#[pallet::constant]
		type MaxDownAdjFactor: Get<i32>;

		#[pallet::constant]
		type TargetBlockTime: Get<BlockDuration>;

		/// EMA smoothing factor used only for the observability runtime API
		/// `get_block_time_ema`. **Does not** drive the difficulty
		/// controller (see EIP-2 §Rationale). Scaled by 1000.
		#[pallet::constant]
		type EmaAlpha: Get<u32>;

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

			// Set current difficulty for the genesis block
			<CurrentDifficulty<T>>::put(initial_difficulty);

			log::info!(target: "qpow", "Genesis: Set initial difficulty to {:x}",
				initial_difficulty.low_u64());

			// Initialize EMA with target block time
			<BlockTimeEma<T>>::put(T::TargetBlockTime::get());
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

		/// Called at the end of each block to adjust mining difficulty based on
		/// observed block times using Exponential Moving Average (EMA).
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
		fn update_block_time_ema(block_time: u64) {
			let current_ema = <BlockTimeEma<T>>::get();
			let alpha = T::EmaAlpha::get();

			// Initialize EMA with target block time if this is the first block
			if current_ema == 0 {
				<BlockTimeEma<T>>::put(T::TargetBlockTime::get());
				return;
			}

			// Calculate EMA: new_ema = alpha * block_time + (1 - alpha) * current_ema
			// Alpha is scaled by 1000, so we divide by 1000
			let alpha_scaled = alpha as u64;
			let one_minus_alpha = 1000u64.saturating_sub(alpha_scaled);

			let weighted_current = block_time.saturating_mul(alpha_scaled);
			let weighted_ema = current_ema.saturating_mul(one_minus_alpha);
			let new_ema = (weighted_current.saturating_add(weighted_ema)) / 1000;

			<BlockTimeEma<T>>::put(new_ema);

			log::debug!(target: "qpow",
				"📊 Updated EMA: {}ms -> {}ms (new block: {}ms, alpha: {})",
				current_ema,
				new_ema,
				block_time,
				alpha_scaled
			);
		}

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
			let target_time = T::TargetBlockTime::get();

			// On the first non-genesis block we have no real previous timestamp,
			// so feed the controller `target_time` (i.e. adj_factor = 0).
			let block_time = if current_block_number > One::one() {
				let bt = now.saturating_sub(last_time);
				log::debug!(target: "qpow",
					"Time calculation: now={}, last_time={}, diff={}ms",
					now, last_time, bt
				);
				<LastBlockDuration<T>>::put(bt);
				Self::update_block_time_ema(bt);
				bt
			} else {
				target_time
			};

			<LastBlockTime<T>>::put(now);

			let new_difficulty =
				Self::calculate_difficulty(current_difficulty, block_time, target_time);

			<CurrentDifficulty<T>>::put(new_difficulty);

			Self::deposit_event(Event::DifficultyAdjusted {
				old_difficulty: current_difficulty,
				new_difficulty,
				observed_block_time: block_time,
			});

			let (pct_change, is_positive) =
				Self::percentage_change(current_difficulty, new_difficulty);

			log::debug!(target: "qpow",
				"🟢 Adjusted mining difficulty {}{}%: {:x} -> {:x} (block_time={}ms target={}ms)",
				if is_positive {"+"} else {"-"},
				pct_change,
				current_difficulty.low_u64(),
				new_difficulty.low_u64(),
				block_time,
				target_time
			);
		}

		/// Difficulty adjustment per Ethereum EIP-2 / EIP-100, in its additive form:
		///
		/// ```text
		/// adj_factor      = clamp(MaxUpAdjFactor - block_time_ms / BlockTimeBucketMs,
		///                         MaxDownAdjFactor, MaxUpAdjFactor)
		/// step            = parent_difficulty / DifficultyBoundDivisor
		/// new_difficulty  = clamp(parent_difficulty + step * adj_factor,
		///                         min_difficulty, max_difficulty)
		/// ```
		///
		/// Input is the **single block's** wall-clock time, not a moving average.
		/// `target_block_time` is unused but kept in the signature for ABI
		/// stability with callers that still pass it.
		pub fn calculate_difficulty(
			current_difficulty: U512,
			observed_block_time: u64,
			_target_block_time: u64,
		) -> U512 {
			let bucket = T::BlockTimeBucketMs::get().max(1);
			let max_up = T::MaxUpAdjFactor::get();
			let max_down = T::MaxDownAdjFactor::get();
			let divisor = T::DifficultyBoundDivisor::get();

			if divisor.is_zero() {
				log::error!(
					target: "qpow",
					"DifficultyBoundDivisor is zero; controller is misconfigured. Returning current difficulty."
				);
				return current_difficulty;
			}
			if max_down > max_up {
				log::error!(
					target: "qpow",
					"MaxDownAdjFactor ({}) > MaxUpAdjFactor ({}); controller is misconfigured. Returning current difficulty.",
					max_down, max_up
				);
				return current_difficulty;
			}

			let buckets_elapsed_u64 = observed_block_time / bucket;
			let buckets_elapsed: i32 = if buckets_elapsed_u64 > i32::MAX as u64 {
				i32::MAX
			} else {
				buckets_elapsed_u64 as i32
			};
			let adj_factor = max_up.saturating_sub(buckets_elapsed).max(max_down);

			let step = current_difficulty / divisor;
			let abs_adj = U512::from(adj_factor.unsigned_abs());
			let delta = step.saturating_mul(abs_adj);

			let raw_adjusted = if adj_factor >= 0 {
				current_difficulty.saturating_add(delta)
			} else {
				current_difficulty.saturating_sub(delta)
			};

			let min_difficulty = Self::get_min_difficulty();
			let max_difficulty = Self::get_max_difficulty();
			let adjusted = raw_adjusted.max(min_difficulty).min(max_difficulty);

			log::debug!(target: "qpow",
				"📊 current={:x} block_time={}ms buckets={} adj={} step={:x} delta={:x} new={:x}",
				current_difficulty.low_u64(),
				observed_block_time,
				buckets_elapsed,
				adj_factor,
				step.low_u64(),
				delta.low_u64(),
				adjusted.low_u64()
			);

			adjusted
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

		/// Verify nonce validity and return achieved difficulty in a single call.
		/// This avoids computing the nonce hash twice when both validation and
		/// achieved difficulty are needed during block import.
		///
		/// Note: This is called via runtime API from the client side. Runtime API
		/// calls execute in a temporary context where state changes are discarded,
		/// so we don't emit events here.
		pub fn verify_and_get_achieved_difficulty(
			block_hash: [u8; 32],
			nonce: NonceType,
		) -> (bool, U512) {
			let (valid, _, hash_achieved) = Self::verify_nonce_internal(block_hash, nonce);
			let achieved_difficulty =
				if valid { achieved_difficulty_from_hash(hash_achieved) } else { U512::zero() };
			(valid, achieved_difficulty)
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
			// Constraint: `min_difficulty >= DifficultyBoundDivisor`, otherwise the
			// per-block step `min_difficulty / DifficultyBoundDivisor` floors to
			// zero and the controller cannot ever lift difficulty off the floor.
			// We additionally floor at Ethereum's `MinimumDifficulty` (2^17 =
			// 131_072) so the smallest valid network still requires real work.
			let divisor = T::DifficultyBoundDivisor::get();
			U512::from(131_072u64).max(divisor)
		}

		pub fn get_max_difficulty() -> Difficulty {
			U512::MAX
		}

		pub fn get_block_time_ema() -> u64 {
			<BlockTimeEma<T>>::get()
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
