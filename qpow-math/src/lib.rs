#![cfg_attr(not(feature = "std"), no_std)]

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
		hash_result.low_u64(), difficulty.low_u64());

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
		result.low_u64(), hex::encode(block_hash), nonce);

	result
}

/// Calculate achieved difficulty from a pre-computed nonce hash.
/// Achieved difficulty = U512::MAX / nonce_hash
/// A lower nonce_hash means more work was done, resulting in higher achieved difficulty.
pub fn achieved_difficulty_from_hash(nonce_hash: U512) -> U512 {
	if nonce_hash == U512::zero() {
		// Perfect hash (virtually impossible) = maximum difficulty
		return U512::MAX;
	}
	U512::MAX / nonce_hash
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

	#[test]
	fn test_zero_difficulty_rejected() {
		let block_hash = [1u8; 32];
		let nonce = [1u8; 64];
		let (is_valid, hash) = is_valid_nonce(block_hash, nonce, U512::zero());
		assert!(!is_valid);
		assert_eq!(hash, U512::zero());
	}

	#[test]
	fn test_max_difficulty_rejects() {
		let block_hash = [1u8; 32];
		let nonce = [1u8; 64];
		let (is_valid, _) = is_valid_nonce(block_hash, nonce, U512::MAX);
		assert!(!is_valid);
	}

	#[test]
	fn test_achieved_difficulty_from_hash_zero_returns_max() {
		assert_eq!(achieved_difficulty_from_hash(U512::zero()), U512::MAX);
	}

	#[test]
	fn test_achieved_difficulty_from_hash_one_returns_max() {
		assert_eq!(achieved_difficulty_from_hash(U512::one()), U512::MAX);
	}

	#[test]
	fn test_achieved_difficulty_from_hash_known_value() {
		let hash = U512::from(1000u64);
		assert_eq!(achieved_difficulty_from_hash(hash), U512::MAX / hash);
	}

	#[test]
	fn test_boundary_hash_equal_to_target_is_invalid() {
		let block_hash = [1u8; 32];
		let nonce = [1u8; 64];
		let hash = get_nonce_hash(block_hash, nonce);

		let difficulty = U512::MAX / hash;
		let target = U512::MAX / difficulty;

		if target == hash {
			let (is_valid, _) = is_valid_nonce(block_hash, nonce, difficulty);
			assert!(!is_valid, "hash == target must be invalid (strict less-than)");
		}
	}

	#[test]
	fn test_valid_nonce_achieved_difficulty_consistency() {
		let block_hash = [1u8; 32];
		let nonce = [1u8; 64];
		let hash = get_nonce_hash(block_hash, nonce);

		let achieved = achieved_difficulty_from_hash(hash);
		let difficulty = achieved / 2;
		assert!(difficulty > U512::zero());

		let (is_valid, returned_hash) = is_valid_nonce(block_hash, nonce, difficulty);
		assert!(is_valid);
		assert!(
			achieved_difficulty_from_hash(returned_hash) >= difficulty,
			"achieved difficulty must be >= stated difficulty when nonce is valid"
		);
	}
}

#[cfg(test)]
mod proptests {
	use super::*;
	use proptest::prelude::*;

	fn arb_u512() -> impl Strategy<Value = U512> {
		prop::array::uniform8(any::<u64>()).prop_map(U512)
	}

	fn arb_nonzero_u512() -> impl Strategy<Value = U512> {
		arb_u512().prop_filter("must be nonzero", |v| *v != U512::zero())
	}

	fn arb_nonce() -> impl Strategy<Value = [u8; 64]> {
		(prop::array::uniform32(any::<u8>()), prop::array::uniform32(any::<u8>())).prop_map(
			|(a, b)| {
				let mut nonce = [0u8; 64];
				nonce[..32].copy_from_slice(&a);
				nonce[32..].copy_from_slice(&b);
				nonce
			},
		)
	}

	proptest! {
		#[test]
		fn achieved_difficulty_inverse(hash in arb_nonzero_u512()) {
			let achieved = achieved_difficulty_from_hash(hash);
			prop_assert!(achieved > U512::zero());
			prop_assert_eq!(achieved, U512::MAX / hash);
		}

		#[test]
		fn is_valid_nonce_deterministic(
			block_hash in prop::array::uniform32(any::<u8>()),
			nonce in arb_nonce(),
			difficulty in arb_nonzero_u512(),
		) {
			let (v1, h1) = is_valid_nonce(block_hash, nonce, difficulty);
			let (v2, h2) = is_valid_nonce(block_hash, nonce, difficulty);
			prop_assert_eq!(v1, v2);
			prop_assert_eq!(h1, h2);
		}

		#[test]
		fn valid_nonce_implies_achieved_gte_difficulty(
			block_hash in prop::array::uniform32(any::<u8>()),
			nonce in arb_nonce(),
			difficulty in arb_nonzero_u512(),
		) {
			let (valid, hash) = is_valid_nonce(block_hash, nonce, difficulty);
			if valid {
				let achieved = achieved_difficulty_from_hash(hash);
				prop_assert!(achieved >= difficulty);
			}
		}
	}
}
