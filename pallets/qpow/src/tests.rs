use crate::{mock::*, Config};
use frame_support::{pallet_prelude::TypedGet, traits::Hooks};
use pallet_timestamp;
use primitive_types::U512;
use qpow_math::{get_nonce_hash, is_valid_nonce};

#[test]
fn test_submit_valid_proof() {
	new_test_ext().execute_with(|| {
		// Set up test data
		let block_hash = [1u8; 32];
		let nonce = [2u8; 64];

		// Test basic hash generation
		let hash_result = get_nonce_hash(block_hash, nonce);
		assert_ne!(hash_result, U512::zero());

		// Test with easy difficulty
		let easy_difficulty = U512::from(1u64);
		let (is_valid, _) = is_valid_nonce(block_hash, nonce, easy_difficulty);
		assert!(is_valid, "Should be valid with easy difficulty");
	});
}

#[test]
fn test_different_nonces_different_hashes() {
	new_test_ext().execute_with(|| {
		let block_hash = [1u8; 32];
		let nonce1 = [2u8; 64];
		let nonce2 = [3u8; 64];

		let hash1 = get_nonce_hash(block_hash, nonce1);
		let hash2 = get_nonce_hash(block_hash, nonce2);

		assert_ne!(hash1, hash2);
		assert_ne!(hash1, U512::zero());
		assert_ne!(hash2, U512::zero());
	});
}

#[test]
fn test_same_inputs_same_hash() {
	new_test_ext().execute_with(|| {
		let block_hash = [5u8; 32];
		let nonce = [7u8; 64];

		let hash1 = get_nonce_hash(block_hash, nonce);
		let hash2 = get_nonce_hash(block_hash, nonce);

		assert_eq!(hash1, hash2);
	});
}

#[test]
fn test_difficulty_validation() {
	new_test_ext().execute_with(|| {
		let block_hash = [1u8; 32];
		let nonce = [1u8; 64];

		// Very easy difficulty - should pass
		let easy_difficulty = U512::from(1u64);
		let (is_valid_easy, hash) = is_valid_nonce(block_hash, nonce, easy_difficulty);
		assert!(is_valid_easy);
		assert_ne!(hash, U512::zero());

		// Very hard difficulty - should fail
		let hard_difficulty = U512::MAX;
		let (is_valid_hard, _) = is_valid_nonce(block_hash, nonce, hard_difficulty);
		assert!(!is_valid_hard);
	});
}

#[test]
fn test_poseidon_double_hash() {
	new_test_ext().execute_with(|| {
		let block_hash = [0x42u8; 32];
		let nonce = [0x24u8; 64];

		// Manually verify double Poseidon2 hashing
		let mut input = [0u8; 96];
		input[..32].copy_from_slice(&block_hash);
		input[32..96].copy_from_slice(&nonce);

		let first_hash = qp_poseidon_core::hash_squeeze_twice(&input);
		let second_hash = qp_poseidon_core::hash_squeeze_twice(&first_hash);
		let expected = U512::from_big_endian(&second_hash);

		let actual = get_nonce_hash(block_hash, nonce);
		assert_eq!(actual, expected);
	});
}

#[test]
fn test_pallet_verification() {
	new_test_ext().execute_with(|| {
		let block_hash = [1u8; 32];
		let nonce = [2u8; 64];

		// Test pallet's verification functions
		let is_valid_import = QPow::verify_nonce_on_import_block(block_hash, nonce);
		let is_valid_mining = QPow::verify_nonce_local_mining(block_hash, nonce);

		// Both should return the same result
		assert_eq!(is_valid_import, is_valid_mining);
	});
}

#[test]
fn test_difficulty_bounds() {
	new_test_ext().execute_with(|| {
		let min_difficulty = QPow::get_min_difficulty();
		let max_difficulty = QPow::get_max_difficulty();
		let initial_difficulty = QPow::initial_difficulty();

		assert_eq!(min_difficulty, U512::from(1000u64));
		assert!(max_difficulty > initial_difficulty);
		assert!(initial_difficulty > min_difficulty);
	});
}

fn run_to_block(n: u64) {
	while System::block_number() < n {
		if System::block_number() > 1 {
			QPow::on_finalize(System::block_number());
			System::on_finalize(System::block_number());
		}
		System::set_block_number(System::block_number() + 1);
		System::on_initialize(System::block_number());
		QPow::on_initialize(System::block_number());
	}
}

fn run_block(block_num: u64, timestamp: u64) {
	System::set_block_number(block_num);
	pallet_timestamp::Pallet::<Test>::set_timestamp(timestamp);
	QPow::on_finalize(block_num);
}

#[test]
fn test_difficulty_adjustment() {
	new_test_ext().execute_with(|| {
		// Get initial difficulty
		let initial_difficulty = QPow::get_difficulty();
		assert!(initial_difficulty > U512::zero());

		// Run a few blocks
		run_to_block(3);

		// Difficulty should be tracked
		let current_difficulty = QPow::get_difficulty();
		assert!(current_difficulty > U512::zero());
	});
}

#[test]
fn test_difficulty_storage_and_retrieval() {
	new_test_ext().execute_with(|| {
		// 1. Test genesis block difficulty
		let genesis_difficulty = QPow::initial_difficulty();
		let initial_difficulty = <Test as Config>::InitialDifficulty::get();

		assert_eq!(
			genesis_difficulty, initial_difficulty,
			"Genesis block should have initial difficulty"
		);

		// 2. Simulate block production
		run_to_block(1);

		// 3. Check difficulty for block 1
		let block_1_difficulty = QPow::get_difficulty();
		assert_eq!(
			block_1_difficulty, initial_difficulty,
			"Block 1 should have same difficulty as initial"
		);

		// 4. Simulate adjustment period
		run_to_block(2);
	});
}

#[test]
fn test_ema_block_time_tracking() {
	new_test_ext().execute_with(|| {
		// Initial EMA should be target block time
		let target_time = <Test as Config>::TargetBlockTime::get();
		let initial_ema = QPow::get_block_time_ema();
		assert_eq!(initial_ema, target_time);

		// Run blocks and check EMA updates
		run_to_block(2);
		let updated_ema = QPow::get_block_time_ema();
		// EMA should still exist (exact value depends on timing)
		assert!(updated_ema > 0);
	});
}

#[test]
fn test_difficulty_calculation() {
	new_test_ext().execute_with(|| {
		let current_difficulty = U512::from(1000u64);
		let observed_time = 2000u64; // 2x target
		let target_time = 1000u64;

		// When blocks are slow, difficulty should decrease
		let new_difficulty =
			QPow::calculate_difficulty(current_difficulty, observed_time, target_time);

		// Should be bounded by min/max
		let min_difficulty = QPow::get_min_difficulty();
		let max_difficulty = QPow::get_max_difficulty();
		assert!(new_difficulty >= min_difficulty);
		assert!(new_difficulty <= max_difficulty);
	});
}

#[test]
fn test_event_emission() {
	new_test_ext().execute_with(|| {
		let block_hash = [1u8; 32];
		let nonce = [2u8; 64];

		// Verify nonce on import block should emit event if valid
		let is_valid = QPow::verify_nonce_on_import_block(block_hash, nonce);

		if is_valid {
			// Check that ProofSubmitted event was emitted
			let events = System::events();
			assert!(events.iter().any(|event| {
				matches!(event.event, RuntimeEvent::QPow(crate::Event::ProofSubmitted { .. }))
			}));
		}
	});
}

#[test]
fn test_bitcoin_style_pow_properties() {
	new_test_ext().execute_with(|| {
		let block_hash = [0x12u8; 32];

		// Test that hash distribution looks random-ish
		let mut hashes = Vec::new();
		for i in 0u64..10 {
			let mut nonce = [0u8; 64];
			nonce[0] = i as u8;
			let hash = get_nonce_hash(block_hash, nonce);
			hashes.push(hash);
		}

		// All hashes should be different
		for i in 0..hashes.len() {
			for j in i + 1..hashes.len() {
				assert_ne!(hashes[i], hashes[j]);
			}
		}

		// All hashes should be non-zero (except for zero nonce)
		for hash in &hashes {
			assert_ne!(*hash, U512::zero());
		}
	});
}

#[test]
fn test_calculate_achieved_difficulty() {
	new_test_ext().execute_with(|| {
		let block_hash = [0x42u8; 32];
		let nonce = [0x13u8; 64];

		// Get the nonce hash
		let nonce_hash = QPow::get_nonce_hash(block_hash, nonce);
		assert_ne!(nonce_hash, U512::zero(), "Nonce hash should not be zero");

		// Calculate achieved difficulty using the formula: U512::MAX / nonce_hash
		let achieved_diff = U512::MAX / nonce_hash;

		// Verify the formula makes sense
		assert!(achieved_diff > U512::zero(), "Achieved difficulty should be positive");

		// A lower nonce hash should result in higher achieved difficulty
		// (more work done = smaller hash = higher difficulty)
		let mut nonce2 = [0u8; 64];
		let hash1 = QPow::get_nonce_hash(block_hash, nonce);
		nonce2[0] = 1;
		let hash2 = QPow::get_nonce_hash(block_hash, nonce2);

		let diff1 = U512::MAX / hash1;
		let diff2 = U512::MAX / hash2;

		// If hash1 < hash2, then diff1 > diff2 (inverse relationship)
		if hash1 < hash2 {
			assert!(diff1 > diff2, "Lower hash should yield higher achieved difficulty");
		} else if hash1 > hash2 {
			assert!(diff1 < diff2, "Higher hash should yield lower achieved difficulty");
		}
		// If equal (extremely unlikely), difficulties would be equal too
	});
}

#[test]
fn test_verify_and_get_achieved_difficulty() {
	new_test_ext().execute_with(|| {
		let block_hash = [1u8; 32];
		let nonce = [2u8; 64];

		// Call the combined function
		let (valid, achieved_diff) = QPow::verify_and_get_achieved_difficulty(block_hash, nonce);

		// Check that verify_nonce_on_import_block returns the same validity
		let expected_valid = QPow::verify_nonce_on_import_block(block_hash, nonce);
		assert_eq!(valid, expected_valid, "Validity should match verify_nonce_on_import_block");

		if valid {
			let nonce_hash = QPow::get_nonce_hash(block_hash, nonce);
			let expected_from_hash = U512::MAX / nonce_hash;
			assert_eq!(
				achieved_diff, expected_from_hash,
				"Achieved difficulty should equal U512::MAX / nonce_hash"
			);
		} else {
			assert_eq!(
				achieved_diff,
				U512::zero(),
				"Invalid nonce should yield zero achieved difficulty"
			);
		}
	});
}

#[test]
fn test_difficulty_recovers_after_sleep() {
	new_test_ext().execute_with(|| {
		let target = <Test as Config>::TargetBlockTime::get();

		for i in 1u64..=10 {
			run_block(i, i * target);
		}

		let pre_sleep = QPow::get_difficulty();
		assert_eq!(pre_sleep, U512::from(1_000_000u64));

		// Simulate laptop sleep: 1-hour gap between blocks
		run_block(11, 10 * target + 3_600_000);

		// 20 normal blocks after waking
		for i in 12u64..=31 {
			run_block(i, 10 * target + 3_600_000 + (i - 11) * target);
		}

		let recovered = QPow::get_difficulty();
		// EMA smoothing limits the spike, but alpha=500 is aggressive so recovery
		// takes many blocks. 20 normal blocks bring difficulty to ~18% of pre-sleep.
		assert!(
			recovered > pre_sleep / 10,
			"Difficulty should stay above 10% after sleep. Pre: {}, Post: {}",
			pre_sleep.low_u64(),
			recovered.low_u64()
		);
	});
}

#[test]
fn test_zero_observed_block_time() {
	new_test_ext().execute_with(|| {
		let difficulty = U512::from(1_000_000u64);
		let result = QPow::calculate_difficulty(difficulty, 0, 1000);
		let min = QPow::get_min_difficulty();
		let max = QPow::get_max_difficulty();
		assert!(result >= min);
		assert!(result <= max);
	});
}

#[test]
fn test_min_difficulty_derived_from_clamp() {
	new_test_ext().execute_with(|| {
		assert_eq!(QPow::get_min_difficulty(), U512::from(1000u64));
	});
}

#[test]
fn test_min_difficulty_can_increase() {
	new_test_ext().execute_with(|| {
		let min_diff = QPow::get_min_difficulty();
		// Fast blocks → ratio clamped to 1.1 → floor(1000 * 1.1) = 1100
		let result = QPow::calculate_difficulty(min_diff, 1, 1000);
		assert!(
			result > min_diff,
			"Min difficulty must be able to increase: {} should be > {}",
			result.low_u64(),
			min_diff.low_u64()
		);
	});
}

#[test]
fn test_min_difficulty_floors_on_slow_blocks() {
	new_test_ext().execute_with(|| {
		let min_diff = QPow::get_min_difficulty();
		// Slow blocks → ratio clamped to 0.9 → floor(1000 * 0.9) = 900, clips to 1000
		let result = QPow::calculate_difficulty(min_diff, 100_000, 1000);
		assert_eq!(result, min_diff);
	});
}

#[test]
fn test_difficulty_below_min_clips_up() {
	new_test_ext().execute_with(|| {
		let min_diff = QPow::get_min_difficulty();
		// Starting at 1 (below min), any result clips to min_difficulty
		let result_fast = QPow::calculate_difficulty(U512::from(1u64), 1, 1000);
		let result_slow = QPow::calculate_difficulty(U512::from(1u64), 100_000, 1000);
		assert_eq!(result_fast, min_diff);
		assert_eq!(result_slow, min_diff);
	});
}
