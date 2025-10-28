#![cfg_attr(not(feature = "std"), no_std)]

use core::ops::BitXor;
use num_bigint::BigUint;
use num_traits::{One, Zero};
use primitive_types::U512;
use qp_poseidon_core::Poseidon2Core;

// Common verification logic
pub fn is_valid_nonce(block_hash: [u8; 32], nonce: [u8; 64], threshold: U512) -> (bool, U512) {
	if nonce == [0u8; 64] {
		log::error!(
			"is_valid_nonce should not be called with 0 nonce, but was for block_hash: {:?}",
			block_hash
		);
		return (false, U512::zero());
	}

	let distance_achieved = get_nonce_distance(block_hash, nonce);
	log::debug!(target: "math", "difficulty = {}..., threshold = {}...", distance_achieved, threshold);
	(distance_achieved <= threshold, distance_achieved)
}

pub fn get_nonce_distance(
	block_hash: [u8; 32], // 256-bit block_hash
	nonce: [u8; 64],      // 512-bit nonce
) -> U512 {
	// s = 0 is cheating
	if nonce == [0u8; 64] {
		log::debug!(target: "math", "zero nonce");
		return U512::zero();
	}

	let (m, n) = get_random_rsa(&block_hash);
	let block_hash_int = U512::from_big_endian(&block_hash);
	let nonce_int = U512::from_big_endian(&nonce);

	let target = hash_to_group_bigint_poseidon(&block_hash_int, &m, &n, &U512::zero());

	// Compare PoW results
	let nonce_element = hash_to_group_bigint_poseidon(&block_hash_int, &m, &n, &nonce_int);

	let distance = target.bitxor(nonce_element);
	log::debug!(target: "math", "distance = {} target = {} nonce = {:?}, nonce_element = {}, block_hash = {}, m = {}, n = {}", distance, target, nonce, nonce_element, hex::encode(block_hash), m, n);

	distance
}

/// Generates a pair of RSA-style numbers (m,n) deterministically from input block_hash
pub fn get_random_rsa(block_hash: &[u8; 32]) -> (U512, U512) {
	// Generate m as random 256-bit number from Poseidon2-256
	let poseidon = Poseidon2Core::new();
	let m_bytes = poseidon.hash_no_pad_bytes(block_hash);
	let m = U512::from_big_endian(&m_bytes);

	// Generate initial n as random 512-bit number from Poseidon2-512
	let mut n_bytes = poseidon.hash_512(&m_bytes);
	let mut n = U512::from_big_endian(&n_bytes);

	// Keep hashing until we find composite coprime n > m
	while n % 2u32 == U512::zero() || n <= m || !is_coprime(&m, &n) || is_prime(&n) {
		log::trace!("Rerolling rsa n = {}", n);
		n_bytes = poseidon.hash_512(&n_bytes);
		n = U512::from_big_endian(&n_bytes);
	}

	log::trace!(
		"Generated RSA pair (m, n) = ({}, {}) from block_hash {}",
		m,
		n,
		hex::encode(block_hash)
	);
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

pub fn hash_to_group_bigint_poseidon(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512 {
	let result = hash_to_group_bigint(h, m, n, solution);
	let poseidon = Poseidon2Core::new();
	U512::from_big_endian(&poseidon.hash_512(&result.to_big_endian()))
}

// no split chunks by Nik
pub fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512 {
	// Compute sum = h + solution
	let sum = h.saturating_add(*solution);
	//log::info!("ComputePoW: h={:?}, m={:?}, n={:?}, solution={:?}, sum={:?}", h, m, n, solution,
	// sum);

	// Compute m^sum mod n using modular exponentiation
	mod_pow(m, &sum, n)
}

/// Multiply previous power by base modulo n to advance exponent by +1
pub fn mod_pow_next(previous: &U512, base: &U512, modulus: &U512) -> U512 {
	// (base^(e) mod n) * base mod n = base^(e+1) mod n
	let a_bi = BigUint::from_bytes_be(&previous.to_big_endian());
	let b_bi = BigUint::from_bytes_be(&base.to_big_endian());
	let m_bi = BigUint::from_bytes_be(&modulus.to_big_endian());
	let result = (a_bi * b_bi) % m_bi;
	U512::from_big_endian(&result.to_bytes_be())
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

/// Mine a contiguous range of nonces using incremental exponentiation.
/// Returns the first valid nonce and its distance if one is found.
pub fn mine_range(
	block_hash: [u8; 32],
	start_nonce: [u8; 64],
	steps: u64,
	threshold: U512,
) -> Option<([u8; 64], U512)> {
	if steps == 0 {
		return None;
	}

	let (m, n) = get_random_rsa(&block_hash);
	let block_hash_int = U512::from_big_endian(&block_hash);

	let mut nonce_u = U512::from_big_endian(&start_nonce);

	// Precompute constant target element once
	let target = hash_to_group_bigint_poseidon(&block_hash_int, &m, &n, &U512::zero());

	// Compute initial value m^(h + nonce) mod
	// n
	let mut value = mod_pow(&m, &block_hash_int.saturating_add(nonce_u), &n);

	let poseidon = Poseidon2Core::new();
	for _ in 0..steps {
		let nonce_element = U512::from_big_endian(&poseidon.hash_512(&value.to_big_endian()));
		if nonce_element == U512::zero() {
			log::debug!(target: "math", "zero nonce");
			continue;
		}
		let distance = target.bitxor(nonce_element);
		if distance <= threshold {
			log::debug!(target: "math", "💎 Local miner found nonce {} with distance {} and target {} and nonce_element {} and block_hash {:?} and m = {} and n = {}", nonce_u, distance, target, nonce_element, hex::encode(block_hash), m, n);
			return Some((nonce_u.to_big_endian(), distance));
		}
		// Advance to next nonce: exponent increases by 1
		value = mod_pow_next(&value, &m, &n);
		nonce_u = nonce_u.saturating_add(U512::from(1u64));
	}

	None
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
		d /= U512::from(2u32);
		r += 1;
	}

	// Generate test bases deterministically from n using SHA3
	let mut bases = [U512::zero(); 32]; // Initialize array of 32 zeros
	let mut base_count = 0;
	let poseidon = Poseidon2Core::new();
	let mut counter = U512::zero();

	while base_count < 32 {
		// k = 32 tests put false positive rate at 1/2^64

		// Hash n concatenated with counter
		let mut bytes = [0u8; 128];
		let n_bytes = n.to_big_endian();
		let counter_bytes = counter.to_big_endian();

		bytes[..64].copy_from_slice(&n_bytes);
		bytes[64..128].copy_from_slice(&counter_bytes);

		let poseidon_bytes = poseidon.hash_512(&bytes);

		// Use the hash to generate a base between 2 and n-2
		let hash = U512::from_big_endian(&poseidon_bytes);
		let base = (hash % (*n - U512::from(4u32))) + U512::from(2u32);
		bases[base_count] = base;
		base_count += 1;

		counter += U512::one();
	}

	'witness: for base in bases {
		let mut x = mod_pow(&base, &d, n);

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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mod_pow_next_matches_mod_pow_for_incrementing_nonce() {
		// Deterministic block_hash
		let block_hash = [7u8; 32];
		let (m, n) = get_random_rsa(&block_hash);
		let h = U512::from_big_endian(&block_hash);

		// Start at an arbitrary nonce
		let mut nonce = U512::from(123u64);

		// Initial value using full exponentiation
		let mut value = mod_pow(&m, &h.saturating_add(nonce), &n);

		// Check equality for 4 consecutive nonces
		for _ in 0..4u32 {
			let expected = mod_pow(&m, &h.saturating_add(nonce), &n);
			assert_eq!(value, expected, "incremental result must match full mod_pow");

			// Advance to next exponent (nonce + 1)
			value = mod_pow_next(&value, &m, &n);
			nonce = nonce.saturating_add(U512::from(1u64));
		}
	}
}
