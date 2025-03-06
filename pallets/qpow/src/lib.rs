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


	pub const CHUNK_SIZE: usize = 32;
	pub const NUM_CHUNKS: usize = 512 / CHUNK_SIZE;
	pub const MAX_DISTANCE: u64 = (1u64 << CHUNK_SIZE) * NUM_CHUNKS as u64;
	pub const INITIAL_DIFFICULTY: u64 = 56255914621; // around 100 iterations

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	pub type LastBlockTime<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type CurrentDifficulty<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type BlocksInPeriod<T: Config> = StorageValue<_, u32, ValueQuery>;


	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_timestamp::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type WeightInfo: WeightInfo;

		#[pallet::constant]
		type TargetBlockTime: Get<u64>;

		#[pallet::constant]
		type AdjustmentPeriod: Get<u32>;
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
		fn on_initialize(block_number: BlockNumberFor<T>) -> Weight {
			log::info!("📢 QPoW: on_initialize called at block {:?}", block_number);
			Weight::zero()
		}

		/// Called at the end of each block.
		fn on_finalize(block_number: BlockNumberFor<T>) {
			let blocks = <BlocksInPeriod<T>>::get();
			let current_difficulty = <CurrentDifficulty<T>>::get();
			log::info!(
				"📢 QPoW: on_finalize called at block {:?}, blocks_in_period={}, current_difficulty={}",
				block_number,
				blocks,
				current_difficulty
			);
			Self::adjust_difficulty();
		}

		/// Called when there is remaining weight at the end of the block.
		fn on_idle(block_number: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
			log::info!(
			    "📢 QPoW: on_idle called at block {:?} with remaining weight {:?}",
			    block_number,
			    remaining_weight
			);
			if <LastBlockTime<T>>::get() == 0 {
				<LastBlockTime<T>>::put(pallet_timestamp::Pallet::<T>::now().saturated_into::<u64>());
				<CurrentDifficulty<T>>::put(INITIAL_DIFFICULTY);
			}
			Weight::zero()
		}

		/// Called whenever a runtime upgrade is applied.
		fn on_runtime_upgrade() -> Weight {
			log::info!("📢 QPoW: on_runtime_upgrade triggered!");
			Weight::zero()
		}

		/// Called for off-chain worker tasks.
		fn offchain_worker(block_number: BlockNumberFor<T>) {
			log::info!("📢 QPoW: offchain_worker triggered at block {:?}", block_number);
		}
	}

	impl<T: Config> Pallet<T> {
		fn adjust_difficulty() {
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
					let target_time_u64 = T::TargetBlockTime::get();

					// Calculate ratio (keeping precision with fixed-point arithmetic)
					let ratio = (average_block_time) as f32 / (target_time_u64) as f32;

					// Adjust difficulty to approach target block time
					let target_time_u64 = T::TargetBlockTime::get();
					// Calculate difference from ideal ratio (1.0)
					//let diff = (1.0 - ratio).max(0.0);


					let power_factor = <f64 as Float>::powf(ratio as f64, 1.0/48.0);

					log::info!("POWER FACTOR: {}",power_factor);

					// Parabolic adjustment
					let adjusted = (current_difficulty as f64 / power_factor) as u64;
					let init_diff = INITIAL_DIFFICULTY / 10;
					log::info!("Adjusted: {}, MD: {}, ID/10: {}", adjusted, MAX_DISTANCE, init_diff);
					let new_difficulty
						= adjusted.min(MAX_DISTANCE - 1).max(INITIAL_DIFFICULTY / 10);

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
               target_time_u64
           );
				}

				// Reset block counter for new adjustment period
				<BlocksInPeriod<T>>::put(0);
			}
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