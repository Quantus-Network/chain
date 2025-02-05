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
	use frame_support::{pallet_prelude::*, dispatch::DispatchResult, traits::BuildGenesisConfig};
	use frame_system::pallet_prelude::*;
	use primitive_types::{U256, U512};
	use sha2::{Digest, Sha256};
	use sha3::Sha3_512;
	use num_bigint::BigUint;
	use frame_support::sp_runtime::traits::{Zero, One};

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type WeightInfo: WeightInfo;
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub initial_difficulty: u32,
		#[serde(skip)]
		pub _phantom: PhantomData<T>,
	}

	//#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self {
				initial_difficulty: 16,
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
			who: T::AccountId,
			solution: [u8; 64],
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidSolution,
		ArithmeticOverflow
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::submit_proof())]
		pub fn submit_proof(
			origin: OriginFor<T>,
			header: [u8; 32],  // 256-bit header
			solution: [u8; 64], // 512-bit solution
			difficulty: u32
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// s = 0 is cheating
			if solution == [0u8; 64] {
				return Err(Error::<T>::InvalidSolution.into())
			}

			let (m, n) = Self::get_random_rsa(&header);
			let header_int = U256::from_big_endian(&header);
			let solution_int = U512::from_big_endian(&solution);

			let (_, original_trunc) = Self::compute_pow(
				&header_int,
				&m,
				&n,
				&U512::zero(),
				difficulty
			);

			// Compare PoW results
			let (_, solution_trunc) = Self::compute_pow(
				&header_int,
				&m,
				&n,
				&solution_int,
				difficulty
			);

			if original_trunc == solution_trunc {
				LatestProof::<T>::put(solution);

				// Emit an event
				Self::deposit_event(Event::ProofSubmitted {
					who,
					solution
				});

				Ok(())
			} else {
				Err(Error::<T>::InvalidSolution.into())
			}
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// Called at the beginning of each block.
		fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
			//log::info!("📢 QPoW: on_initialize called at block {:?}", block_number);
			Weight::zero()
		}

		/// Called at the end of each block.
		fn on_finalize(_block_number: BlockNumberFor<T>) {
			//log::info!("📢 QPoW: on_finalize called at block {:?}", block_number);
		}

		/// Called when there is remaining weight at the end of the block.
		fn on_idle(_block_number: BlockNumberFor<T>, _remaining_weight: Weight) -> Weight {
			// 	log::info!(
			//     "📢 QPoW: on_idle called at block {:?} with remaining weight {:?}",
			//     block_number,
			//     remaining_weight
			// );
			Weight::zero()
		}

		/// Called whenever a runtime upgrade is applied.
		fn on_runtime_upgrade() -> Weight {
			//log::info!("📢 QPoW: on_runtime_upgrade triggered!");
			Weight::zero()
		}

		/// Called for off-chain worker tasks.
		fn offchain_worker(_block_number: BlockNumberFor<T>) {
			//log::info!("📢 QPoW: offchain_worker triggered at block {:?}", block_number);
		}
	}

	impl<T: Config> Pallet<T> {
		/// Generates a pair of RSA-style numbers (m,n) deterministically from input header
		pub fn get_random_rsa(header: &[u8; 32]) -> (U256, U512) {
			// Generate m as random 256-bit number from SHA2-256
			let mut sha256 = Sha256::new();
			sha256.update(header);
			let m = U256::from_big_endian(sha256.finalize().as_slice());

			// Generate initial n as random 512-bit number from SHA3-512
			let mut sha3 = Sha3_512::new();
			sha3.update(header);
			let mut n = U512::from_big_endian(sha3.finalize().as_slice());

			// Keep hashing until we find composite coprime n > m
			while n <= U512::from(m) || !Self::is_coprime(&m, &n) || Self::is_prime(&n)  {
				let mut sha3 = Sha3_512::new();
				let bytes = n.to_big_endian();
				sha3.update(&bytes);
				n = U512::from_big_endian(sha3.finalize().as_slice());
			}

			(m, n)
		}

		/// Check if two numbers are coprime using Euclidean algorithm
		pub fn is_coprime(a: &U256, b: &U512) -> bool {
			let a = U512::from(*a);
			let mut x = a;
			let mut y = *b;

			while y != U512::zero() {
				let tmp = y;
				y = x % y;
				x = tmp;
			}

			x == U512::one()
		}

		/// Compute the proof of work function
		pub fn compute_pow(
			h: &U256,
			m: &U256,
			n: &U512,
			solution: &U512,
			difficulty: u32
		) -> (U512, U512) {
			let h = U512::from(*h);
			let m = U512::from(*m);

			// Compute sum = h + solution
			let sum = h.saturating_add(*solution);
			//log::info!("ComputePoW: h={:?}, m={:?}, n={:?}, solution={:?}, sum={:?}", h, m, n, solution, sum);

			// Compute m^sum mod n using modular exponentiation
			let result = Self::mod_pow(&m, &sum, n);
			//log::info!("ComputePoW: result={:?}", result);

			// Create difficulty mask
			let mask = (U512::one() << difficulty) - U512::one();
			//log::info!("ComputePoW: mask={:?}", mask);

			// Get truncated result
			let truncated = result & mask;
			//log::info!("ComputePoW: truncated={:?}", truncated);


			(result, truncated)
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

		pub fn get_difficulty() -> u32 {
			GenesisConfig::<T>::default().initial_difficulty
		}

		pub fn log_info(message: &str){
			log::info!("FROM PALL: {}",message);
		}
	}
}

