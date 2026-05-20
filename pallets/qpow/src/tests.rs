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

		let hash = qp_poseidon_core::hash_squeeze_twice(&input);
		let expected = U512::from_big_endian(&hash);

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
		let divisor = <Test as Config>::DifficultyBoundDivisor::get();

		assert!(min_difficulty >= divisor, "floor must allow non-zero step");
		assert_eq!(min_difficulty, U512::from(131_072u64));
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
		// Mid-difficulty value far from min/max so adjustments are visible.
		let current_difficulty = U512::from(1_000_000u64);

		// Slow block (2x target). With bucket=750ms and target=1000ms,
		// buckets_elapsed = 2000/750 = 2, adj_factor = 1 - 2 = -1.
		// step = 1_000_000 / 2048 = 488. Δ = -488. New = 999_512.
		let slower = QPow::calculate_difficulty(current_difficulty, 2000, 1000);
		assert!(slower < current_difficulty);

		// Fast block (sub-bucket). adj_factor = +1. Δ = +488. New = 1_000_488.
		let faster = QPow::calculate_difficulty(current_difficulty, 100, 1000);
		assert!(faster > current_difficulty);

		// At-target block sits in the no-change band [750, 1500).
		let unchanged = QPow::calculate_difficulty(current_difficulty, 1000, 1000);
		assert_eq!(unchanged, current_difficulty);

		let min_difficulty = QPow::get_min_difficulty();
		let max_difficulty = QPow::get_max_difficulty();
		assert!(slower >= min_difficulty && slower <= max_difficulty);
		assert!(faster >= min_difficulty && faster <= max_difficulty);
	});
}

#[test]
fn test_adj_factor_table() {
	new_test_ext().execute_with(|| {
		// With bucket=750ms, max_up=+1, max_down=-99, divisor=2048:
		// adj_factor = clamp(1 - block_time/750, -99, 1)
		let d = U512::from(2_048_000_000u64); // step = 1_000_000

		// block_time in [0, 750): buckets=0, adj=+1, Δ=+1_000_000
		let r = QPow::calculate_difficulty(d, 0, 1000);
		assert_eq!(r, d + U512::from(1_000_000u64));
		let r = QPow::calculate_difficulty(d, 749, 1000);
		assert_eq!(r, d + U512::from(1_000_000u64));

		// block_time in [750, 1500): buckets=1, adj=0, no change
		let r = QPow::calculate_difficulty(d, 750, 1000);
		assert_eq!(r, d);
		let r = QPow::calculate_difficulty(d, 1499, 1000);
		assert_eq!(r, d);

		// block_time in [1500, 2250): buckets=2, adj=-1
		let r = QPow::calculate_difficulty(d, 1500, 1000);
		assert_eq!(r, d - U512::from(1_000_000u64));

		// block_time in [75_000, 75_750): buckets=100, adj=-99 (cap)
		let r = QPow::calculate_difficulty(d, 75_000, 1000);
		assert_eq!(r, d - U512::from(99_000_000u64));

		// Far past the cap: still -99
		let r = QPow::calculate_difficulty(d, 10_000_000, 1000);
		assert_eq!(r, d - U512::from(99_000_000u64));

		// Pathological u64::MAX block_time: still -99, saturates cleanly
		let r = QPow::calculate_difficulty(d, u64::MAX, 1000);
		assert_eq!(r, d - U512::from(99_000_000u64));
	});
}

#[test]
fn test_min_difficulty_escape_from_floor() {
	// Critical regression: with the new additive form and a min derived from the
	// divisor, the floor must be escapable in a single fast block. The
	// pre-existing multiplicative-clamp implementation had a floor trap at
	// min=1000 with up-clamp=1/2048 where 1000*(1+1/2048) truncated back to 1000.
	new_test_ext().execute_with(|| {
		let min_diff = QPow::get_min_difficulty();
		assert!(
			min_diff >= U512::from(131_072u64),
			"min should be >= Ethereum's MinimumDifficulty"
		);

		let lifted = QPow::calculate_difficulty(min_diff, 0, 1000);
		assert!(
			lifted > min_diff,
			"floor must be liftable: lifted={} min={}",
			lifted.low_u64(),
			min_diff.low_u64()
		);

		// Step at the floor should equal exactly +(min/divisor).
		let divisor = <Test as Config>::DifficultyBoundDivisor::get();
		let expected_step = min_diff / divisor;
		assert_eq!(lifted, min_diff + expected_step);
	});
}

#[test]
fn test_overflow_saturation() {
	new_test_ext().execute_with(|| {
		// Max difficulty: fast block should saturate, not overflow.
		let max = QPow::get_max_difficulty();
		let r = QPow::calculate_difficulty(max, 0, 1000);
		assert_eq!(r, max);

		// Min difficulty: slow block should clip to min, not underflow.
		let min = QPow::get_min_difficulty();
		let r = QPow::calculate_difficulty(min, u64::MAX, 1000);
		assert_eq!(r, min);
	});
}

#[test]
fn test_no_change_when_at_target() {
	new_test_ext().execute_with(|| {
		let d = U512::from(1_000_000u64);
		// target=1000ms sits in the no-change band [750, 1500).
		let r = QPow::calculate_difficulty(d, 1000, 1000);
		assert_eq!(r, d);
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

		// Warm up at target — adj_factor = 0, no change.
		for i in 1u64..=10 {
			run_block(i, i * target);
		}
		let pre_sleep = QPow::get_difficulty();
		assert_eq!(pre_sleep, U512::from(1_000_000u64));

		// Simulate laptop sleep: 1-hour gap between blocks. With bucket=750ms,
		// 3_600_000 / 750 = 4800 buckets → adj_factor = max(1-4800, -99) = -99.
		// Step = 1_000_000 / 2048 = 488. Δ = -488*99 = -48_312. New = 951_688.
		run_block(11, 10 * target + 3_600_000);
		let post_sleep = QPow::get_difficulty();
		assert!(post_sleep < pre_sleep);
		assert!(post_sleep > pre_sleep * U512::from(95u64) / U512::from(100u64));

		// 20 normal blocks at target → adj_factor=0, difficulty stays put.
		// (Single-block input means the slow patch does not keep echoing into
		// future adjustments — exactly the property EIP-2 §Rationale calls out.)
		for i in 12u64..=31 {
			run_block(i, 10 * target + 3_600_000 + (i - 11) * target);
		}
		let after_normal = QPow::get_difficulty();
		assert_eq!(after_normal, post_sleep, "no slow tail after one bad block");
	});
}

#[test]
fn test_min_difficulty_matches_ethereum_floor() {
	new_test_ext().execute_with(|| {
		assert_eq!(QPow::get_min_difficulty(), U512::from(131_072u64));
	});
}

#[test]
fn test_difficulty_below_min_clips_up() {
	new_test_ext().execute_with(|| {
		let min_diff = QPow::get_min_difficulty();
		// Starting at 1 (below min): step = 1/2048 = 0, but post-adjustment clip
		// brings the value up to min_difficulty.
		let result_fast = QPow::calculate_difficulty(U512::from(1u64), 1, 1000);
		let result_slow = QPow::calculate_difficulty(U512::from(1u64), 100_000, 1000);
		assert_eq!(result_fast, min_diff);
		assert_eq!(result_slow, min_diff);
	});
}

#[cfg(test)]
mod proptests {
	use super::*;
	use crate::mock::{new_test_ext, QPow, Test};
	use proptest::prelude::*;

	fn arb_difficulty() -> impl Strategy<Value = U512> {
		prop_oneof![
			Just(U512::from(131_072u64)),
			Just(U512::MAX),
			Just(U512::from(2_700_000u64)),
			(1u128..=u128::MAX).prop_map(U512::from),
		]
	}

	fn run<T>(f: impl FnOnce() -> T) -> T {
		new_test_ext().execute_with(f)
	}

	proptest! {
		#[test]
		fn result_always_in_bounds(d in arb_difficulty(), bt in 0u64..=u64::MAX) {
			let (r, min, max) = run(|| (
				QPow::calculate_difficulty(d, bt, 1000),
				QPow::get_min_difficulty(),
				QPow::get_max_difficulty(),
			));
			prop_assert!(r >= min, "result {} < min {}", r.low_u64(), min.low_u64());
			prop_assert!(r <= max);
		}

		#[test]
		fn monotone_in_block_time(
			d in arb_difficulty(),
			bt1 in 0u64..1_000_000,
			bt2 in 0u64..1_000_000,
		) {
			let (a, b) = if bt1 <= bt2 { (bt1, bt2) } else { (bt2, bt1) };
			let (fast, slow) = run(|| (
				QPow::calculate_difficulty(d, a, 1000),
				QPow::calculate_difficulty(d, b, 1000),
			));
			prop_assert!(fast >= slow,
				"monotonicity broken: bt={} -> {}, bt={} -> {}",
				a, fast.low_u64(), b, slow.low_u64());
		}

		#[test]
		fn no_change_band_is_flat(d in arb_difficulty(), offset in 0u64..750u64) {
			let (r, expected) = run(|| {
				let bucket = <Test as Config>::BlockTimeBucketMs::get();
				let r = QPow::calculate_difficulty(d, bucket + offset, 1000);
				let min = QPow::get_min_difficulty();
				let max = QPow::get_max_difficulty();
				(r, d.max(min).min(max))
			});
			prop_assert_eq!(r, expected);
		}

		#[test]
		fn step_magnitude_bounded(d in arb_difficulty(), bt in 0u64..=u64::MAX) {
			let (r, min, divisor) = run(|| (
				QPow::calculate_difficulty(d, bt, 1000),
				QPow::get_min_difficulty(),
				<Test as Config>::DifficultyBoundDivisor::get(),
			));
			if d < min { return Ok(()); }
			let max_factor = U512::from(
				(<Test as Config>::MaxUpAdjFactor::get() as u32)
					.max(<Test as Config>::MaxDownAdjFactor::get().unsigned_abs()),
			);
			let max_delta = (d / divisor).saturating_mul(max_factor);
			let actual_delta = if r >= d { r - d } else { d - r };
			prop_assert!(actual_delta <= max_delta,
				"delta {} exceeds max step {}", actual_delta.low_u64(), max_delta.low_u64());
		}
	}
}
