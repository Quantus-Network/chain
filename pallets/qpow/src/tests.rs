use crate::{mock::*, Config};
use frame_support::{pallet_prelude::TypedGet, traits::Hooks};
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

		assert_eq!(min_difficulty, U512::from(1u64));
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

		// Total work should accumulate
		let total_work = QPow::get_total_work();
		assert!(total_work > U512::zero());
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

		// 5. Verify work accumulation
		let total_work = QPow::get_total_work();
		assert!(total_work > U512::zero(), "Total work should accumulate");
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
