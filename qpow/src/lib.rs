#![cfg_attr(not(feature = "std"), no_std)]
use codec::{Decode, Encode};
use sc_consensus_pow::{Error, PowAlgorithm};
use sp_consensus_pow::{Seal as RawSeal};
use sp_core::{H256, U256};
use sp_runtime::generic::BlockId;
use sp_runtime::traits::Block as BlockT;
use num_bigint::BigUint;
use num_traits::{One, Zero};
use primitive_types::U512;
use sha2::{Digest, Sha256};
use sha3::Sha3_512;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub const INITIAL_DIFFICULTY: u64 = 56255914621; // around 100 iterations

#[derive(Clone, Debug, Encode, Decode, PartialEq)]
pub struct QPoWSeal {
    pub difficulty: U256,
    pub work: [u8; 64], // 512 bit work
    pub nonce: u64,
}

/// A not-yet-computed attempt to solve the proof of work. Calling the
/// compute method will compute the hash and return the seal.
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug)]
pub struct Compute {
    pub difficulty: U256,
    pub pre_hash: H256,
    pub nonce: u64,
}

impl Compute {
    pub fn compute(self) -> QPoWSeal {
        // Convert pre_hash into U512.
        let header_int = U512::from_big_endian(self.pre_hash.as_bytes());
        // Convert nonce into U512.
        let nonce_val = U512::from(self.nonce);
        // Get RSA-like parameters (m, n) deterministically from the pre_hash.
		let (m, n) = QPow::get_random_rsa(self.pre_hash.as_ref().try_into().unwrap());
		// Compute group element (an array of 16 u32 values) from header and nonce.
        let work = QPow::hash_to_group_bigint_2(&header_int, &m, &n, &nonce_val);

        QPoWSeal {
            nonce: self.nonce,
            difficulty: self.difficulty,
			work: work.to_big_endian().try_into().unwrap(),
		}
    }
}

/// A minimal PoW algorithm that uses Sha3 hashing.
/// Difficulty is fixed at 1_000_000
#[derive(Clone)]
pub struct MinimalQPowAlgorithm;

// Here we implement the general PowAlgorithm trait for our concrete Sha3Algorithm
impl<B: BlockT<Hash = H256>> PowAlgorithm<B> for MinimalQPowAlgorithm {
    type Difficulty = U256;

    fn difficulty(&self, _parent: B::Hash) -> Result<Self::Difficulty, Error<B>> {
        // Fixed difficulty hardcoded here
        Ok(U256::from(INITIAL_DIFFICULTY / 2)) // TODO setting to trivial difficulty for now
    }

    fn verify(
        &self,
        _parent: &BlockId<B>,
        pre_hash: &H256,
        _pre_digest: Option<&[u8]>,
        seal: &RawSeal,
        difficulty: Self::Difficulty,
    ) -> Result<bool, Error<B>> {
        // Try to construct a seal object by decoding the raw seal given
        let seal = match QPoWSeal::decode(&mut &seal[..]) {
            Ok(seal) => seal,
            Err(_) => return Ok(false),
        };
        // FILL IN using qpow

		// Convert pre_hash to [u8; 32] for verification
		let header = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

		// Verify the solution using QPoW
		if !QPow::verify_solution(header, seal.work, difficulty.low_u64()) {
			return Ok(false);
		}
		
        // Make sure the provided work actually comes from the correct pre_hash
        let compute = Compute {
            difficulty,
            pre_hash: *pre_hash,
            nonce: seal.nonce,
        };

        if compute.compute() != seal {
            return Ok(false);
        }

        Ok(true)
    }
}

pub const CHUNK_SIZE: usize = 32;
pub const NUM_CHUNKS: usize = 512 / CHUNK_SIZE;
pub const MAX_DISTANCE: u64 = (1u64 << CHUNK_SIZE) * NUM_CHUNKS as u64;

pub struct QPow;

impl QPow {
    pub fn get_solution_distance(
        header: [u8; 32],   // 256-bit header
        solution: [u8; 64], // 512-bit solution
    ) -> u64 {
        // s = 0 is cheating
        if solution == [0u8; 64] {
            return 0u64;
        }

        let (m, n) = Self::get_random_rsa(&header);
        let header_int = U512::from_big_endian(&header);
        let solution_int = U512::from_big_endian(&solution);

        let original_chunks = Self::hash_to_group_bigint(&header_int, &m, &n, &U512::zero());

        // Compare PoW results
        let solution_chunks = Self::hash_to_group_bigint(&header_int, &m, &n, &solution_int);

        Self::l1_distance(&original_chunks, &solution_chunks)
    }

    // meets difficulty
    // this returns true if the solution distance is below the difficulty
    // false otherwise
    pub fn verify_solution(header: [u8; 32], solution: [u8; 64], difficulty: u64) -> bool {
        if solution == [0u8; 64] {
            return false;
        }
        let distance = Self::get_solution_distance(header, solution);
        let verified = distance <= MAX_DISTANCE - difficulty;
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
        while n.clone() % 2u32 == U512::zero()
            || n <= m
            || !Self::is_coprime(&m, &n)
            || Self::is_prime(&n)
        {
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
        let mut chunks: [u32; 16] = [0u32; NUM_CHUNKS];
        let mask = (U512::one() << CHUNK_SIZE) - U512::one();

        for i in 0..NUM_CHUNKS {
            let shift = i * CHUNK_SIZE;
            let chunk = (num >> shift) & mask;
            chunks[i] = chunk.as_u32();
        }

        chunks
    }

    /// Calculate L1 distance between two chunk vectors
    pub fn l1_distance(original: &[u32], solution: &[u32]) -> u64 {
        original
            .iter()
            .zip(solution.iter())
            .map(|(a, b)| if a > b { a - b } else { b - a })
            .map(|x| x as u64)
            .sum()
    }

    /// Compute the proof of work function
    pub fn hash_to_group(
        h: &[u8; 32],
        m: &[u8; 32],
        n: &[u8; 64],
        solution: &[u8; 64],
    ) -> [u32; 16] {
        let h = U512::from_big_endian(h);
        let m = U512::from_big_endian(m);
        let n = U512::from_big_endian(n);
        let solution = U512::from_big_endian(solution);
        Self::hash_to_group_bigint(&h, &m, &n, &solution)
    }

    fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> [u32; 16] {
        // Compute sum = h + solution
        let sum = h.saturating_add(*solution);
        //log::info!("ComputePoW: h={:?}, m={:?}, n={:?}, solution={:?}, sum={:?}", h, m, n, solution, sum);

        // Compute m^sum mod n using modular exponentiation
        let result = Self::mod_pow(&m, &sum, n);
        //log::info!("ComputePoW: result={:?}", result);

        Self::split_chunks(&result)
    }

	// no split chunks
	fn hash_to_group_bigint_2(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512 {
        // Compute sum = h + solution
        let sum = h.saturating_add(*solution);
        //log::info!("ComputePoW: h={:?}, m={:?}, n={:?}, solution={:?}, sum={:?}", h, m, n, solution, sum);

        // Compute m^sum mod n using modular exponentiation
        let result = Self::mod_pow(&m, &sum, n);
        //log::info!("ComputePoW: result={:?}", result);

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

        while base_count < 32 {
            // k = 32 tests put false positive rate at 1/2^64

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
            for _ in 0..r - 1 {
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

    // pub fn get_difficulty() -> u64 {
    //     100
    // }

    pub fn log_info(message: &str) {
        log::info!("FROM PALL: {}", message);
    }
}
