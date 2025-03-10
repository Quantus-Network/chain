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
	use primitive_types::U512;
	use sha2::{Digest, Sha256};
	use sha3::Sha3_512;
	use num_bigint::BigUint;
	use num_traits::Float;
	use frame_support::sp_runtime::traits::{Zero, One};
	use sp_std::prelude::*;


	pub const CHUNK_SIZE: usize = 32;
	pub const NUM_CHUNKS: usize = 512 / CHUNK_SIZE;
	pub const MAX_DISTANCE: u64 = (1u64 << CHUNK_SIZE) * NUM_CHUNKS as u64;
	pub const INITIAL_DIFFICULTY: u64 = 50255914621; // around 100 iterations

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	pub type LastBlockTime<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type CurrentDifficulty<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type BlocksInPeriod<T: Config> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	pub type BlockTimeHistory<T: Config> = StorageMap<_, Twox64Concat, u32, u64, ValueQuery>;

	// Indeks dla aktualnej pozycji w historii (bufor cykliczny)
	#[pallet::storage]
	pub type HistoryIndex<T: Config> = StorageValue<_, u32, ValueQuery>;

	// Aktualny rozmiar historii
	#[pallet::storage]
	pub type HistorySize<T: Config> = StorageValue<_, u32, ValueQuery>;


	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type WeightInfo: WeightInfo;

		#[pallet::constant]
		type TargetBlockTime: Get<u64>;

		#[pallet::constant]
		type AdjustmentPeriod: Get<u32>;

		#[pallet::constant]
		type MinDifficultyMultiplier: Get<(u64, u64)>;

		#[pallet::constant]
		type MaxDifficultyMultiplier: Get<(u64, u64)>;

		#[pallet::constant]
		type DampeningFactor: Get<u64>;

		#[pallet::constant]
		type BlockTimeHistorySize: Get<u32>;
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
				initial_difficulty: INITIAL_DIFFICULTY/2,
				_phantom: PhantomData,
			}
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			let initial_proof = [0u8; 64];
			<LatestProof<T>>::put(initial_proof);
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


	#[pallet::storage]
	#[pallet::getter(fn latest_proof)]
	pub type LatestProof<T> = StorageValue<_, [u8; 64]>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		ProofSubmitted {
			nonce: [u8; 64],
		},
		DifficultyAdjusted {
			old_difficulty: u64,
			new_difficulty: u64,
			average_block_time: u64,
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
				<CurrentDifficulty<T>>::put(INITIAL_DIFFICULTY);
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
		fn get_median_block_time() -> u64 {
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

			// Incerement number of blocks in period
			<BlocksInPeriod<T>>::put(blocks + 1);

			// Save if it's not the first block
			if last_time > 0 {
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

					let new_difficulty = Self::calculate_new_difficulty_advanced(
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
						average_block_time: median_block_time,
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


		fn adjust_difficulty_v1() {
			// Get current time from timestamp pallet (in milliseconds)
			let now = pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>();
			let last_time = <LastBlockTime<T>>::get();
			let blocks = <BlocksInPeriod<T>>::get();
			let current_difficulty = <CurrentDifficulty<T>>::get();

			// Increment block counter
			<BlocksInPeriod<T>>::put(blocks + 1);

			// Update last block time for future calculations
			<LastBlockTime<T>>::put(now);

			// Check if difficulty adjustment is needed (after specified number of blocks)
			if blocks >= T::AdjustmentPeriod::get() {
				if last_time > 0 { // Make sure this is not the first block
					// Calculate average time between blocks in milliseconds
					let time_diff = now.saturating_sub(last_time);
					let average_block_time = time_diff / (blocks as u64);

					// Adjust difficulty to approach target block time
					let target_time = T::TargetBlockTime::get();

					let new_difficulty = Self::calculate_new_difficulty_advanced(
						current_difficulty,
						average_block_time,
						target_time
					);

/*					// Calculate ratio (keeping precision with fixed-point arithmetic)
					let ratio = (average_block_time) as f32 / (target_time_u64) as f32;

					// Adjust difficulty to approach target block time
					let target_time_u64 = T::TargetBlockTime::get();

					let power_factor = <f64 as Float>::powf(ratio as f64, 1.0/16.0);

					log::info!("POWER FACTOR: {}",power_factor);

					// Parabolic adjustment
					let adjusted = (current_difficulty as f64 / power_factor) as u64;
					let init_diff = INITIAL_DIFFICULTY / 10;
					log::info!("Adjusted: {}, MD: {}, ID/10: {}", adjusted, MAX_DISTANCE, init_diff);
					let new_difficulty
						= adjusted.min(MAX_DISTANCE - 1).max(INITIAL_DIFFICULTY / 10);
*/
					// Save the new difficulty
					<CurrentDifficulty<T>>::put(new_difficulty);

					// Emit difficulty adjustment event
					Self::deposit_event(Event::DifficultyAdjusted {
						old_difficulty: current_difficulty,
						new_difficulty,
						average_block_time,
					});

					log::info!(
					   "Adjusted mining difficulty: {} -> {} (avg block time: {}ms, target: {}ms)",
					   current_difficulty,
					   new_difficulty,
					   average_block_time,
					   target_time
				   );
				}

				// Reset block counter for new adjustment period
				<BlocksInPeriod<T>>::put(0);
				<LastBlockTime<T>>::put(now);
			}
			else{
				if blocks == 0 {
					<LastBlockTime<T>>::put(now);
				}
			}
		}
		pub fn calculate_new_difficulty(
			current_difficulty: u64,
			average_block_time: u64,
			target_block_time: u64,
			) -> u64 {
			log::info!("");
            log::info!(
				"📊 Calculating new difficulty\n\t🟢 Current Difficulty: {}\n\t🕒 Average Block Time: {}ms\n\t🎯 Target Block Time: {}ms",
				current_difficulty,
				average_block_time,
				target_block_time
			);
				
			// Calculate ratio
			let ratio = (average_block_time as f32) / (target_block_time as f32);

			// Calculate power factor
			let power_factor = <f64 as Float>::powf(ratio as f64, 1.0/16.0);

			// Calculate adjusted difficulty
			let adjusted = (current_difficulty as f64 / power_factor) as u64;

			log::info!("POWER FACTOR: {}", power_factor);
			log::info!("Adjusted: {}, MD: {}, ID/10: {}",
				adjusted,
				MAX_DISTANCE,
				INITIAL_DIFFICULTY / 10
			);
			
			// Enforce bounds
			adjusted.min(MAX_DISTANCE - 1).max(INITIAL_DIFFICULTY / 10)
		}

		fn calculate_new_difficulty_advanced(
			current_difficulty: u64,
			average_block_time: u64,
			target_block_time: u64,
		) -> u64 {
			// Load configuration
			let (min_num, min_denom) = T::MinDifficultyMultiplier::get();
			let (max_num, max_denom) = T::MaxDifficultyMultiplier::get();
			let dampening = T::DampeningFactor::get();

			let min_multiplier = min_num as f64 / min_denom as f64;
			let max_multiplier = max_num as f64 / max_denom as f64;
			let dampening_factor = dampening as f64;

			log::info!("");
			log::info!(
				"📊 Calculating new difficulty\n\t🟢 Current Difficulty: {}\n\t🕒 Average Block Time: {}ms\n\t🎯 Target Block Time: {}ms",
				current_difficulty,
				average_block_time,
				target_block_time
			);

			// Calculate parameters
			let raw_ratio = (average_block_time as f64) / (target_block_time as f64);
			let clamped_ratio = raw_ratio.max(min_multiplier).min(max_multiplier);

			// Apply additional damping
			let damped_ratio = 1.0 + (clamped_ratio - 1.0) / dampening_factor;

			// Calculate new difficulty
			let adjusted = (current_difficulty as f64 / damped_ratio) as u64;

			log::info!("Raw ratio: {}, Clamped ratio: {}, Damped ratio: {}",
				raw_ratio,
				clamped_ratio,
				damped_ratio
			);
			log::info!("Adjusted difficulty: {}, MIN: {}, MAX: {}",
				adjusted,
				INITIAL_DIFFICULTY / 10,
				MAX_DISTANCE - 1
			);

			// Cut to the expected range
			adjusted.min(MAX_DISTANCE - 1).max(INITIAL_DIFFICULTY / 10)
		}

	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
	}

	impl<T: Config> Pallet<T> {
		pub fn get_nonce_distance(
			header: [u8; 32],  // 256-bit header
			nonce: [u8; 64], // 512-bit nonce
		) -> u64 {
			// s = 0 is cheating
			if nonce == [0u8; 64] {
				return 0u64
			}

			let (m, n) = Self::get_random_rsa(&header);
			let header_int = U512::from_big_endian(&header);
			let nonce_int = U512::from_big_endian(&nonce);

			let original_chunks = Self::hash_to_group_bigint_split(
				&header_int,
				&m,
				&n,
				&U512::zero()
			);

			// Compare PoW results
			let nonce_chunks = Self::hash_to_group_bigint_split(
				&header_int,
				&m,
				&n,
				&nonce_int
			);

			Self::l1_distance(&original_chunks, &nonce_chunks)
		}

		pub fn verify_nonce(header: [u8; 32], nonce: [u8; 64], difficulty: u64) -> bool {
			if nonce == [0u8; 64] {
				return false
			}
			let distance = Self::get_nonce_distance(header, nonce);
			let verified = distance <= MAX_DISTANCE - difficulty;
			if verified {
				<LatestProof<T>>::put(nonce);
				Self::deposit_event(Event::ProofSubmitted { nonce });
			}
			verified
		}

		/// Generates a pair of RSA-style numbers (m,n) deterministically from input header
		pub fn get_random_rsa(header: &[u8; 32]) -> (U512, U512) {
			// Generate m as random 256-bit number from SHA2-256
			let mut sha256 = Sha256::new();
			sha256.update(header);
			let m = U512::from_big_endian(sha256.finalize().as_slice());

			// Generate initial n as random 512-bit number from SHA3-512
			let mut sha3 = Sha3_512::new();
			sha3.update(header);
			let mut n = U512::from_big_endian(sha3.finalize().as_slice());

			// Keep hashing until we find composite coprime n > m
			while n.clone() % 2u32 == U512::zero() || n <= m || !Self::is_coprime(&m, &n) || Self::is_prime(&n)  {
				let mut sha3 = Sha3_512::new();
				let bytes = n.to_big_endian();
				sha3.update(&bytes);
				n = U512::from_big_endian(sha3.finalize().as_slice());
			}

			(m, n)
		}

		/// Check if two numbers are coprime using Euclidean algorithm
		pub fn is_coprime(a: &U512, b: &U512) -> bool {
			let mut x = *a;
			let mut y = *b;

			while y != U512::zero() {
				let tmp = y;
				y = x % y;
				x = tmp;
			}

			x == U512::one()
		}

		/// Split a 512-bit number into 32-bit chunks
		pub fn split_chunks(num: &U512) -> [u32; NUM_CHUNKS] {
			let mut chunks:[u32; 16] = [0u32; NUM_CHUNKS];
			let mask = (U512::one() << CHUNK_SIZE) - U512::one();

			for i in 0..NUM_CHUNKS {
				let shift = i * CHUNK_SIZE;
				let chunk = (num >> shift) & mask;
				chunks[i] = chunk.as_u32();
			}

			chunks
		}

		/// Calculate L1 distance between two chunk vectors
		fn l1_distance(original: &[u32], solution: &[u32]) -> u64 {
			original.iter().zip(solution.iter())
				.map(|(a, b)| if a > b { a - b } else { b - a })
				.map(|x| x as u64)
				.sum()
		}

		pub fn hash_to_group_bigint_split(
			h: &U512,
			m: &U512,
			n: &U512,
			solution: &U512
		) -> [u32; 16] {
			let result = Self::hash_to_group_bigint(h,m,n,solution);

			Self::split_chunks(&result)
		}

		// no split chunks by Nik
		pub fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512 {
			// Compute sum = h + solution
			let sum = h.saturating_add(*solution);
			//log::info!("ComputePoW: h={:?}, m={:?}, n={:?}, solution={:?}, sum={:?}", h, m, n, solution, sum);

			// Compute m^sum mod n using modular exponentiation
			let result = Self::mod_pow(&m, &sum, n);

			result
		}

		/// Modular exponentiation using Substrate's BigUint
		pub fn mod_pow(base: &U512, exponent: &U512, modulus: &U512) -> U512 {
			if modulus == &U512::zero() {
				panic!("Modulus cannot be zero");
			}

			// Convert inputs to BigUint
			let mut base = BigUint::from_bytes_be(&base.to_big_endian());
			let mut exp = BigUint::from_bytes_be(&exponent.to_big_endian());
			let modulus = BigUint::from_bytes_be(&modulus.to_big_endian());

			// Initialize result as 1
			let mut result = BigUint::one();

			// Square and multiply algorithm
			while !exp.is_zero() {
				if exp.bit(0) {
					result = (result * &base) % &modulus;
				}
				base = (&base * &base) % &modulus;
				exp >>= 1;
			}

			U512::from_big_endian(&result.to_bytes_be())
		}

		// Miller-Rabin primality test
		pub fn is_prime(n: &U512) -> bool {
			if *n <= U512::one() {
				return false;
			}
			if *n == U512::from(2u32) || *n == U512::from(3u32) {
				return true;
			}
			if *n % U512::from(2u32) == U512::zero() {
				return false;
			}

			// Write n-1 as d * 2^r
			let mut d = *n - U512::one();
			let mut r = 0u32;
			while d % U512::from(2u32) == U512::zero() {
				d = d / U512::from(2u32);
				r += 1;
			}

			// Generate test bases deterministically from n using SHA3
			let mut bases = [U512::zero(); 32]; // Initialize array of 32 zeros
			let mut base_count = 0;
			let mut sha3 = Sha3_512::new();
			let mut counter = U512::zero();

			while base_count < 32 {  // k = 32 tests put false positive rate at 1/2^64

				// Hash n concatenated with counter
				let mut bytes = [0u8; 128];
				let n_bytes = n.to_big_endian();
				let counter_bytes = counter.to_big_endian();

				bytes[..64].copy_from_slice(&n_bytes);
				bytes[64..128].copy_from_slice(&counter_bytes);

				sha3.update(&bytes);

				// Use the hash to generate a base between 2 and n-2
				let hash = U512::from_big_endian(sha3.finalize_reset().as_slice());
				let base = (hash % (*n - U512::from(4u32))) + U512::from(2u32);
				bases[base_count] = base;
				base_count += 1;

				counter = counter + U512::one();
			}

			'witness: for base in bases {
				let mut x = Self::mod_pow(&U512::from(base), &d, n);

				if x == U512::one() || x == *n - U512::one() {
					continue 'witness;
				}

				// Square r-1 times
				for _ in 0..r-1 {
					x = Self::mod_pow(&x, &U512::from(2u32), n);
					if x == *n - U512::one() {
						continue 'witness;
					}
					if x == U512::one() {
						return false;
					}
				}
				return false;
			}

			true
		}

		pub fn get_difficulty() -> u64 {
			let stored = <CurrentDifficulty<T>>::get();
			if stored == 0 {
				return GenesisConfig::<T>::default().initial_difficulty;
			}
			stored
		}

		pub fn log_info(message: &str){
			log::info!("From QPoW Pallet: {}",message);
		}
	}
}