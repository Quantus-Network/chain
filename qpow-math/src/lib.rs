#![cfg_attr(not(feature = "std"), no_std)]

use num_bigint::BigUint;
use num_traits::{One, Zero};
use primitive_types::U512;
use sha2::{Digest, Sha256};
use sha3::Sha3_512;

pub const CHUNK_SIZE: usize = 32;
pub const NUM_CHUNKS: usize = 512 / CHUNK_SIZE;
pub const DIMENSION_SIZE: u64 = 1 << CHUNK_SIZE;
pub const MAX_DISTANCE: u64 = DIMENSION_SIZE * NUM_CHUNKS as u64;

// Common verification logic
pub fn is_valid_nonce(header: [u8; 32], nonce: [u8; 64], difficulty: u64) -> bool {
    if nonce == [0u8; 64] {
        return false;
    }

    let distance = get_nonce_distance(header, nonce);
    distance <= MAX_DISTANCE - difficulty
}

pub fn get_nonce_distance(
    header: [u8; 32], // 256-bit header
    nonce: [u8; 64],  // 512-bit nonce
) -> u64 {
    // s = 0 is cheating
    if nonce == [0u8; 64] {
        return 0u64;
    }

    let (m, n) = get_random_rsa(&header);
    let header_int = U512::from_big_endian(&header);
    let nonce_int = U512::from_big_endian(&nonce);

    let original_chunks = hash_to_group_bigint_split(&header_int, &m, &n, &U512::zero());

    // Compare PoW results
    let nonce_chunks = hash_to_group_bigint_split(&header_int, &m, &n, &nonce_int);

    l1_distance(&original_chunks, &nonce_chunks)
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
    while n.clone() % 2u32 == U512::zero() || n <= m || !is_coprime(&m, &n) || is_prime(&n) {
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
    let mut chunks: [u32; NUM_CHUNKS] = [0u32; NUM_CHUNKS];
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
    original
        .iter()
        .zip(solution.iter())
        .map(|(a, b)| if a > b { a - b } else { b - a })
        .map(|x| x as u64)
        .map(|x| x.min(DIMENSION_SIZE - x))
        .sum()
}

pub fn hash_to_group_bigint_split(h: &U512, m: &U512, n: &U512, solution: &U512) -> [u32; NUM_CHUNKS] {
    let result = hash_to_group_bigint(h, m, n, solution);

    split_chunks(&result)
}

// no split chunks by Nik
pub fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512 {
    // Compute sum = h + solution
    let sum = h.saturating_add(*solution);
    //log::info!("ComputePoW: h={:?}, m={:?}, n={:?}, solution={:?}, sum={:?}", h, m, n, solution, sum);

    // Compute m^sum mod n using modular exponentiation
    let result = mod_pow(&m, &sum, n);

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
        let mut x = mod_pow(&U512::from(base), &d, n);

        if x == U512::one() || x == *n - U512::one() {
            continue 'witness;
        }

        // Square r-1 times
        for _ in 0..r - 1 {
            x = mod_pow(&x, &U512::from(2u32), n);
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
