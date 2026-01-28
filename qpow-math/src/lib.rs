#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use primitive_types::U512;

// Bitcoin-style validation logic with double Poseidon2 hashing
pub fn is_valid_nonce(block_hash: [u8; 32], nonce: [u8; 64], difficulty: U512) -> (bool, U512) {
	if difficulty == U512::zero() {
		log::error!(
			"is_valid_nonce should not be called with 0 difficulty, but was for block_hash: {:?}",
			block_hash
		);
		return (false, U512::zero());
	}

	let hash_result = get_nonce_hash(block_hash, nonce);
	log::debug!(target: "math", "hash_result = {:x}, difficulty = {:x}",
		hash_result.low_u32() as u16, difficulty.low_u32() as u16);

	// In Bitcoin-style PoW, we check if hash < target
	// Where target = max_target / difficulty
	let max_target = U512::MAX;
	// Unchecked division because we catch difficulty == 0 above
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

	// Double hash with Poseidon2 (like Bitcoin's double SHA256)
	let first_hash = qp_poseidon_core::hash_squeeze_twice(&input);
	let second_hash = qp_poseidon_core::hash_squeeze_twice(&first_hash);

	// Convert to U512 for difficulty comparison
	let result = U512::from_big_endian(&second_hash);

	log::debug!(target: "math", "hash = {:x} block_hash = {}, nonce = {:?}",
		result.low_u32() as u16, hex::encode(block_hash), nonce);

	result
}

#[cfg(test)]
mod tests {
	use super::*;

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
