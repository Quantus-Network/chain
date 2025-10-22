#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::string::{String, ToString};
use core::fmt::Write;

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
		sp_runtime::{traits::One, SaturatedConversion, Saturating},
		traits::{BuildGenesisConfig, Time},
	};
	use frame_system::pallet_prelude::BlockNumberFor;
	use qpow_math::{get_nonce_hash, is_valid_nonce};
	use sp_arithmetic::FixedU128;
	use sp_core::U512;

	/// Type definitions for QPoW pallet
	pub type NonceType = [u8; 64];
	pub type Difficulty = U512;
	pub type WorkValue = U512;
	pub type Timestamp = u64;
	pub type BlockDuration = u64;
	pub type PercentageClamp = u8;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	pub type LastBlockTime<T: Config> = StorageValue<_, Timestamp, ValueQuery>;

	#[pallet::storage]
	pub type LastBlockDuration<T: Config> = StorageValue<_, BlockDuration, ValueQuery>;

	#[pallet::storage]
	pub type CurrentDifficulty<T: Config> = StorageValue<_, Difficulty, ValueQuery>;

	#[pallet::storage]
	pub type TotalWork<T: Config> = StorageValue<_, WorkValue, ValueQuery>;

	// Exponential Moving Average of block times (in milliseconds)
	#[pallet::storage]
	pub type BlockTimeEma<T: Config> = StorageValue<_, BlockDuration, ValueQuery>;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		/// Pallet's weight info
		#[pallet::constant]
		type InitialDifficulty: Get<U512>;

		#[pallet::constant]
		type DifficultyAdjustPercentClamp: Get<PercentageClamp>;

		#[pallet::constant]
		type TargetBlockTime: Get<BlockDuration>;

		/// EMA smoothing factor (0-1000, where 1000 = 1.0)
		#[pallet::constant]
		type EmaAlpha: Get<u32>;

		#[pallet::constant]
		type MaxReorgDepth: Get<u32>;

		/// Fixed point scale for calculations (default: 10^18)
		#[pallet::constant]
		type FixedU128Scale: Get<u128>;

		/// Weight information for extrinsics in this pallet.
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
			let initial_difficulty = get_initial_difficulty::<T>();

			// Set current difficulty for the genesis block
			<CurrentDifficulty<T>>::put(initial_difficulty);

			log::info!(target: "qpow", "Genesis: Set initial difficulty to {}",
				print_u512_hex_prefix(initial_difficulty, 128));

			// Initialize EMA with target block time
			<BlockTimeEma<T>>::put(T::TargetBlockTime::get());

			// Initialize the total work with the genesis block's difficulty
			<TotalWork<T>>::put(WorkValue::one());
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

	#[pallet::error]
	pub enum Error<T> {
		InvalidSolution,
		ArithmeticOverflow,
	}

	pub fn get_initial_difficulty<T: Config>() -> Difficulty {
		T::InitialDifficulty::get()
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		// TODO: update this
		fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
			<T as crate::Config>::WeightInfo::on_finalize_max_history()
		}

		/// Called when there is remaining weight at the end of the block.
		/// TODO: do we need this?
		fn on_idle(_block_number: BlockNumberFor<T>, _remaining_weight: Weight) -> Weight {
			if <LastBlockTime<T>>::get() == 0 {
				<LastBlockTime<T>>::put(
					pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>(),
				);
				let initial_difficulty: U512 = get_initial_difficulty::<T>();
				<CurrentDifficulty<T>>::put(initial_difficulty);
			}
			Weight::zero()
		}

		/// Called at the end of each block.
		fn on_finalize(block_number: BlockNumberFor<T>) {
			let current_difficulty = <CurrentDifficulty<T>>::get();
			log::debug!(target: "qpow",
				"📢 QPoW: before submit at block {:?}, current_difficulty={}",
				block_number,
				print_u512_hex_prefix(current_difficulty, 128)
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

			// Prevent division by zero
			if a == U512::zero() {
				return (U512::zero(), b >= a);
			}

			let (larger, smaller) = if a > b { (a, b) } else { (b, a) };
			let abs_diff = larger - smaller;
			let change = abs_diff.saturating_mul(U512::from(100u64)) / a;

			(change, b >= a)
		}

		fn adjust_difficulty() {
			// Get current time
			let now = pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>();
			let last_time = <LastBlockTime<T>>::get();
			let current_difficulty = <CurrentDifficulty<T>>::get();
			let current_block_number = <frame_system::Pallet<T>>::block_number();

			// Update TotalWork
			let old_total_work = <TotalWork<T>>::get();
			let current_work = Self::get_difficulty();
			let new_total_work = old_total_work.saturating_add(current_work);
			<TotalWork<T>>::put(new_total_work);
			log::debug!(target: "qpow",
				"Total work: now={}, last_time={}, diff={}",
				new_total_work,
				old_total_work,
				new_total_work - old_total_work
			);

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

				Self::update_block_time_ema(block_time);
			}

			// Add last block time for the next calculations
			<LastBlockTime<T>>::put(now);

			let observed_block_time = <BlockTimeEma<T>>::get();
			let target_time = T::TargetBlockTime::get();

			let new_difficulty =
				Self::calculate_difficulty(current_difficulty, observed_block_time, target_time);

			// Save new difficulty
			<CurrentDifficulty<T>>::put(new_difficulty);

			log::debug!(target: "qpow", "Stored new difficulty: {}",
				print_u512_hex_prefix(new_difficulty, 128));

			// Propagate new Event
			Self::deposit_event(Event::DifficultyAdjusted {
				old_difficulty: current_difficulty,
				new_difficulty,
				observed_block_time,
			});

			let (pct_change, is_positive) =
				Self::percentage_change(current_difficulty, new_difficulty);

			log::debug!(target: "qpow",
				"🟢 Adjusted mining difficulty {}{}%: {} -> {} (observed block time: {}ms, target: {}ms) ",
				if is_positive {"+"} else {"-"},
				pct_change,
				print_u512_hex_prefix(current_difficulty, 128),
				print_u512_hex_prefix(new_difficulty, 128),
				observed_block_time,
				target_time
			);
		}

		pub fn calculate_difficulty(
			current_difficulty: U512,
			observed_block_time: u64,
			target_block_time: u64,
		) -> U512 {
			log::debug!(target: "qpow", "📊 Calculating new difficulty ---------------------------------------------");
			// Calculate ratio using FixedU128
			let clamp =
				FixedU128::from_rational(T::DifficultyAdjustPercentClamp::get() as u128, 100u128);
			let one = FixedU128::one();
			let ratio =
				FixedU128::from_rational(target_block_time as u128, observed_block_time as u128)
					.min(one.saturating_add(clamp))
					.max(one.saturating_sub(clamp));
			log::debug!(target: "qpow", "💧 Clamped block_time ratio as FixedU128: {} ", ratio);

			let ratio_512 = U512::from(ratio.into_inner());

			// For Bitcoin-style difficulty adjustment:
			// If observed_time > target_time (slow blocks), difficulty should decrease
			// If observed_time < target_time (fast blocks), difficulty should increase
			// new_difficulty = current_difficulty * target_time / observed_time
			let mut adjusted = match current_difficulty.checked_mul(ratio_512) {
				Some(numerator) => {
					// unchecked division, we know the denominator is not zero
					let result = numerator / U512::from(FixedU128::one().into_inner());
					log::debug!(target: "qpow",
					    "Difficulty calculation: current={}, target_time={}, observed_time={}, new={}",
						print_u512_hex_prefix(current_difficulty, 32), target_block_time, observed_block_time, print_u512_hex_prefix(result, 32));
					result
				},
				None => {
					panic!("Multiplication overflow in difficulty calculation");
				},
			};

			let min_difficulty = Self::get_min_difficulty();
			if adjusted < min_difficulty {
				log::warn!(
					"Min difficulty achieved, clipping to: {}",
					print_u512_hex_prefix(min_difficulty, 128)
				);

				adjusted = min_difficulty;
			} else {
				let max_difficulty = Self::get_max_difficulty();
				if adjusted > max_difficulty {
					log::warn!(
						"Max difficulty achieved, clipping to: {}",
						print_u512_hex_prefix(max_difficulty, 128)
					);
					adjusted = max_difficulty;
				}
			}

			log::debug!(target: "qpow",
				"🟢 Current Difficulty: {}",
				print_u512_hex_prefix(current_difficulty, 128)
			);
			log::debug!(target: "qpow", "🟢 Next Difficulty:    {}", print_u512_hex_prefix(adjusted, 128));
			log::debug!(target: "qpow", "🕒 Observed Block Time Sum: {}ms", observed_block_time);
			log::debug!(target: "qpow", "🎯 Target Block Time Sum:   {target_block_time}ms");

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

		pub fn get_initial_difficulty() -> Difficulty {
			get_initial_difficulty::<T>()
		}

		pub fn get_difficulty() -> Difficulty {
			let stored = <CurrentDifficulty<T>>::get();
			let initial = get_initial_difficulty::<T>();

			if stored == U512::zero() {
				log::warn!(target: "qpow", "Stored difficulty is zero, using initial: {}",
					print_u512_hex_prefix(initial, 128));
				return initial;
			}
			stored
		}

		pub fn get_min_difficulty() -> Difficulty {
			Difficulty::one()
		}

		pub fn get_max_difficulty() -> Difficulty {
			U512::MAX
		}

		pub fn get_total_work() -> WorkValue {
			<TotalWork<T>>::get()
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

	/// Helper function to print the first n hex digits of a U512
	pub fn print_u512_hex_prefix(value: U512, n: usize) -> String {
		let mut hex_string = String::new();
		let _ = write!(hex_string, "{:0128x}", value);
		let prefix_len = core::cmp::min(n, hex_string.len());
		hex_string[..prefix_len].to_string()
	}
}
