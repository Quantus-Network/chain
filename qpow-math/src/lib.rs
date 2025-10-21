#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::string::{String, ToString};
use core::fmt::Write;
use primitive_types::U512;
use qp_poseidon_core::Poseidon2Core;

// Bitcoin-style validation logic with double Poseidon2 hashing
pub fn is_valid_nonce(block_hash: [u8; 32], nonce: [u8; 64], difficulty: U512) -> (bool, U512) {
	if nonce == [0u8; 64] {
		log::error!(
			"is_valid_nonce should not be called with 0 nonce, but was for block_hash: {:?}",
			block_hash
		);
		return (false, U512::zero());
	}

	let hash_result = get_nonce_hash(block_hash, nonce);
	log::debug!(target: "math", "hash_result = {}, difficulty = {}",
		print_u512_hex_prefix(hash_result, 32), print_u512_hex_prefix(difficulty, 32));

	// In Bitcoin-style PoW, we check if hash < target
	// Where target = max_target / difficulty
	let max_target = U512::MAX;
	let target = max_target / difficulty;

	(hash_result < target, hash_result)
}

// Bitcoin-style double hashing with Poseidon2
pub fn get_nonce_hash(
	block_hash: [u8; 32], // 256-bit block_hash
	nonce: [u8; 64],      // 512-bit nonce
) -> U512 {
	// Concatenate block hash + nonce (like Bitcoin does with header + nonce)
	let mut input = [0u8; 96]; // 32 + 64 bytes
	input[..32].copy_from_slice(&block_hash);
	input[32..96].copy_from_slice(&nonce);

	let poseidon = Poseidon2Core::new();

	// Double hash with Poseidon2 (like Bitcoin's double SHA256)
	let hash = poseidon.hash_squeeze_twice(&input);

	// Convert to U512 for difficulty comparison
	let result = U512::from_big_endian(&hash);

	log::debug!(target: "math", "hash = {} block_hash = {}, nonce = {:?}",
		print_u512_hex_prefix(result, 32), hex::encode(block_hash), nonce);

	result
}

/// Mine a contiguous range of nonces using simple incremental search.
/// Returns the first valid nonce and its hash if one is found.
pub fn mine_range(
	block_hash: [u8; 32],
	start_nonce: [u8; 64],
	steps: u64,
	difficulty: U512,
) -> Option<([u8; 64], U512)> {
	if steps == 0 {
		return None;
	}

	let mut nonce_u = U512::from_big_endian(&start_nonce);
	let max_target = U512::MAX;
	let target = max_target / difficulty;

	for _ in 0..steps {
		let nonce_bytes = nonce_u.to_big_endian();
		let hash_result = get_nonce_hash(block_hash, nonce_bytes);

		if hash_result < target {
			log::debug!(target: "math", "💎 Local miner found nonce {} with hash {} and target {} and block_hash {:?}",
				print_u512_hex_prefix(nonce_u, 32), print_u512_hex_prefix(hash_result, 32),
				print_u512_hex_prefix(target, 32), hex::encode(block_hash));
			return Some((nonce_bytes, hash_result));
		}

		// Advance to next nonce
		nonce_u = nonce_u.saturating_add(U512::from(1u64));
	}

	None
}

/// Helper function to print the first n hex digits of a U512
pub fn print_u512_hex_prefix(value: U512, n: usize) -> String {
	let mut hex_string = String::new();
	let _ = write!(hex_string, "{:0128x}", value);
	let prefix_len = core::cmp::min(n, hex_string.len());
	hex_string[..prefix_len].to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_zero_nonce_returns_zero_hash() {
		let block_hash = [0xABu8; 32];
		let nonce = [0u8; 64];
		let hash = get_nonce_hash(block_hash, nonce);
		assert_eq!(hash, U512::zero());
	}

	#[test]
	fn test_different_nonces_produce_different_hashes() {
		let block_hash = [1u8; 32];
		let nonce1 = [2u8; 64];
		let nonce2 = [3u8; 64];

		let hash1 = get_nonce_hash(block_hash, nonce1);
		let hash2 = get_nonce_hash(block_hash, nonce2);

		assert_ne!(hash1, hash2);
		assert_ne!(hash1, U512::zero());
		assert_ne!(hash2, U512::zero());
	}

	#[test]
	fn test_same_input_produces_same_hash() {
		let block_hash = [5u8; 32];
		let nonce = [7u8; 64];

		let hash1 = get_nonce_hash(block_hash, nonce);
		let hash2 = get_nonce_hash(block_hash, nonce);

		assert_eq!(hash1, hash2);
	}

	#[test]
	fn test_is_valid_nonce_with_easy_difficulty() {
		let block_hash = [1u8; 32];
		let nonce = [1u8; 64];
		let easy_difficulty = U512::from(1u32); // Very easy

		let (is_valid, hash) = is_valid_nonce(block_hash, nonce, easy_difficulty);

		// With difficulty 1, target is U512::MAX, so any non-zero hash should be valid
		assert!(is_valid);
		assert_ne!(hash, U512::zero());
	}
}
