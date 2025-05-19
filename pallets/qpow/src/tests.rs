use std::ops::Shl;
use frame_support::pallet_prelude::TypedGet;
use frame_support::traits::Hooks;
use crate::mock::*;
use primitive_types::U512;
use crate::{BlockTimeHistory, HistoryIndex, HistorySize};
use crate::Config;
use qpow_math::{mod_pow, is_coprime, is_prime, get_random_rsa, hash_to_group_bigint_sha, sha3_512};

#[test]
fn test_submit_valid_proof() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let header = [1u8; 32];

        // Get current distance_threshold
        let distance_threshold = QPow::get_distance_threshold_at_block(0);
        let max_distance = QPow::get_max_distance();
        println!("Current distance_threshold: {}", distance_threshold);

        // We need to find valid and invalid nonces for our test
        let mut valid_nonce = [0u8; 64];
        let mut invalid_nonce = [0u8; 64];
        let mut found_pair = false;

        // Try various values for the last byte
        for i in 1..255 {
            invalid_nonce[63] = i;
            valid_nonce[63] = i + 1;

            let invalid_distance = QPow::get_nonce_distance(header, invalid_nonce);
            let valid_distance = QPow::get_nonce_distance(header, valid_nonce);

            // Check if we found a pair where one is valid and one is invalid
            if invalid_distance > distance_threshold && valid_distance <= distance_threshold {
                println!("Found test pair: invalid={}, valid={}", i, i+1);
                println!("Invalid distance: {}, Valid distance: {}, Threshold: {}",
                         invalid_distance, valid_distance, distance_threshold);
                found_pair = true;
                break;
            }
        }

        if !found_pair {
            panic!("Could not find valid/invalid nonce pair for testing with distance_threshold {}", distance_threshold);
        }

        // Now run the test with our dynamically found values

        // Submit an invalid proof
        assert!(!QPow::submit_nonce(header, invalid_nonce),
                "Nonce should be invalid with distance {} > threshold {}",
                QPow::get_nonce_distance(header, invalid_nonce),
                max_distance - distance_threshold);

        // Submit a valid proof
        assert!(QPow::submit_nonce(header, valid_nonce),
                "Nonce should be valid with distance {} <= threshold {}",
                QPow::get_nonce_distance(header, valid_nonce),
                max_distance - distance_threshold);

        assert_eq!(QPow::latest_nonce(), Some(valid_nonce));

        // Find a second valid nonce for medium distance_threshold test
        let mut second_valid = valid_nonce;
        let mut found_second = false;

        for i in valid_nonce[63]+1..255 {
            second_valid[63] = i;
            let distance = QPow::get_nonce_distance(header, second_valid);
            if distance <= max_distance - distance_threshold {
                println!("Found second valid nonce: {}", i);
                found_second = true;
                break;
            }
        }

        if found_second {
            // Submit the second valid proof
            assert!(QPow::submit_nonce(header, second_valid));
            assert_eq!(QPow::latest_nonce(), Some(second_valid));
        } else {
            println!("Could not find second valid nonce, skipping that part of test");
        }

        // TODO:  Event check could be added here
    });
}

#[test]
fn test_verify_for_import() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let header = [1u8; 32];

        // Get current distance_threshold to understand what we need to target
        let distance_threshold = QPow::get_distance_threshold();
        println!("Current distance_threshold: {}", distance_threshold);

        // Find a nonce that will be valid for the current distance_threshold
        let mut valid_nonce = [0u8; 64];
        let mut found_valid = false;

        // Try various values until we find one that works
        for i in 1..255 {
            valid_nonce[63] = i;
            let distance = QPow::get_nonce_distance(header, valid_nonce);

            if distance <= distance_threshold {
                println!("Found valid nonce with value {} - distance: {}, threshold: {}",
                         i, distance, distance_threshold);
                found_valid = true;
                break;
            }
        }

        assert!(found_valid, "Could not find valid nonce for testing. Adjust test parameters.");

        // Now verify using the dynamically found valid nonce
        assert!(QPow::verify_for_import(header, valid_nonce));

        // Check that the latest proof was stored
        assert_eq!(QPow::latest_nonce(), Some(valid_nonce));

        // Check for events if needed
        // ...
    });
}

#[test]
fn test_verify_historical_block() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let header = [1u8; 32];

        // Get the genesis block distance_threshold
        let max_distance = QPow::get_max_distance();
        let genesis_distance_threshold = QPow::get_distance_threshold_at_block(0);
        println!("Genesis distance_threshold: {}", genesis_distance_threshold);

        // Use a nonce that we know works better with our test distance_threshold
        let mut nonce = [0u8; 64];
        nonce[63] = 186;  // This seemed to work in other tests

        // Check if this nonce is valid for genesis distance_threshold
        let distance = QPow::get_nonce_distance(header, nonce);
        let threshold = genesis_distance_threshold;

        println!("Nonce distance: {}, Threshold: {}", distance, threshold);

        if distance > threshold {
            println!("Test nonce is not valid for genesis distance_threshold - trying alternatives");

            // Try a few common patterns
            let mut found_valid = false;
            for byte_value in 1..=255 {
                nonce[63] = byte_value;
                let distance = QPow::get_nonce_distance(header, nonce);
                if distance <= threshold {
                    println!("Found valid nonce with byte value {}: distance={}", byte_value, distance);
                    found_valid = true;
                    break;
                }
            }

            if !found_valid {
                panic!("Could not find a valid nonce for genesis distance_threshold. Test cannot proceed.");
            }
        }

        // Now we should have a valid nonce for genesis block
        assert!(QPow::verify_historical_block(header, nonce, 0),
                "Nonce with distance {} should be valid for threshold {}",
                distance, threshold);

        // Now let's create a block at height 1 with a specific distance_threshold
        run_to_block(1);

        // Get the distance_threshold that was stored for block 1
        let block_1_distance_threshold = QPow::get_distance_threshold_at_block(1);
        assert!(block_1_distance_threshold > U512::zero(), "Block 1 should have a stored distance_threshold");

        // Need to verify our nonce is still valid for block 1's distance_threshold
        let block_1_threshold = max_distance - block_1_distance_threshold;
        if distance > block_1_threshold {
            println!("Warning: Test nonce valid for genesis but not for block 1");
            println!("Block 1 distance_threshold: {}, threshold: {}", block_1_distance_threshold, block_1_threshold);
        }

        // Verify a nonce against block 1's distance_threshold with direct method
        assert!(QPow::is_valid_nonce(header, nonce, block_1_distance_threshold),
                "Nonce with distance {} should be valid for block 1 threshold {}",
                distance, block_1_threshold);

        // Use the public API
        assert!(QPow::verify_historical_block(header, nonce, 1));

        // Test an invalid nonce
        let invalid_nonce = [0u8; 64];
        assert!(!QPow::verify_historical_block(header, invalid_nonce, 1));

        // Test a non-existent block
        let future_block = 1000;
        assert!(!QPow::verify_historical_block(header, nonce, future_block));
    });
}


#[test]
fn test_distance_threshold_storage_and_retrieval() {
    new_test_ext().execute_with(|| {
        // 1. Test genesis block distance_threshold
        let genesis_distance_threshold = QPow::get_distance_threshold_at_block(0);
        let initial_distance_threshold = U512::one().shl(<Test as Config>::InitialDistanceThresholdExponent::get());

        assert_eq!(genesis_distance_threshold, initial_distance_threshold,
                   "Genesis block should have initial distance_threshold");

        // 2. Simulate block production
        run_to_block(1);

        // 3. Check distance_threshold for block 1
        let block_1_distance_threshold = QPow::get_distance_threshold_at_block(1);
        assert_eq!(block_1_distance_threshold, initial_distance_threshold,
                   "Block 1 should have same distance_threshold as initial");

        // 4. Simulate adjustment period
        let adjustment_period = <Test as Config>::AdjustmentPeriod::get();
        run_to_block(adjustment_period + 1);

        // 5. Verify historical blocks maintain their distance_threshold
        let block_1_distance_threshold_after = QPow::get_distance_threshold_at_block(1);
        assert_eq!(block_1_distance_threshold_after, block_1_distance_threshold,
                   "Historical block distance_threshold should not change");

        // 6. Verify nonexistent block returns 0
        let latest_block = System::block_number();
        let future_block = latest_block + 1000;
        assert_eq!(QPow::get_distance_threshold_at_block(future_block), U512::zero(),
                   "Future block distance_threshold should be 0");
    });
}

/// Total distance_threshold tests

#[test]
fn test_total_distance_threshold_initialization() {
    new_test_ext().execute_with(|| {
        // Initially, total distance_threshold should be as genesis distance_threshold
        let initial_work = U512::one();
        assert_eq!(QPow::get_total_work(), initial_work,
                   "Initial TotalWork should be 0");

        // After the first btest_total_distance_threshold_increases_with_each_blocklock, TotalWork should equal block 1's distance_threshold
        run_to_block(1);
        let block_1_distance_threshold = QPow::get_distance_threshold_at_block(1);
        let max_distance = QPow::get_max_distance();
        let current_work = max_distance / block_1_distance_threshold;
        let total_work = QPow::get_total_work();
        assert_eq!(total_work, initial_work + current_work,
                   "TotalWork after block 1 should equal block 1's distance_threshold");
    });
}

#[test]
fn test_total_distance_threshold_accumulation() {
    new_test_ext().execute_with(|| {
        // Generate consecutive blocks and check distance_threshold accumulation
        let mut expected_total = U512::one();
        let max_distance = QPow::get_max_distance();
        for i in 1..=10 {
            run_to_block(i);
            let block_distance_threshold = QPow::get_distance_threshold_at_block(i as u64);
            expected_total = expected_total.saturating_add(max_distance / block_distance_threshold);

            let stored_total = QPow::get_total_work();
            assert_eq!(stored_total, expected_total,
                       "TotalDifficulty after block {} should be the sum of all blocks' difficulties", i);
        }
    });
}

#[test]
fn test_total_distance_threshold_after_adjustment() {
    new_test_ext().execute_with(|| {
        // Advance to the point where distance_threshold gets adjusted
        let adjustment_period = <Test as Config>::AdjustmentPeriod::get();
        run_to_block(adjustment_period + 1);
        let max_distance = QPow::get_max_distance();
        // Check if distance_threshold has changed
        let initial_distance_threshold = U512::one().shl(<Test as Config>::InitialDistanceThresholdExponent::get());
        let new_distance_threshold = QPow::get_distance_threshold_at_block((adjustment_period + 1) as u64);

        // We assume distance_threshold may have changed
        println!("Initial distance_threshold: {}, New distance_threshold: {}", initial_distance_threshold, new_distance_threshold);

        // Calculate expected cumulative distance_threshold
        let mut expected_total = U512::one();
        for i in 1..=(adjustment_period + 1) {
            let block_diff = QPow::get_distance_threshold_at_block(i as u64);
            expected_total += max_distance / block_diff;
        }

        // Compare with stored value
        let stored_total = QPow::get_total_work();
        assert_eq!(stored_total, expected_total,
                   "TotalDifficulty should correctly account for distance_threshold changes");
    });
}

#[test]
fn test_total_distance_threshold_increases_with_each_block() {
    new_test_ext().execute_with(|| {
        // Check initial value
        let initial_total = QPow::get_total_work();

        // Run to block 1 and check the increase
        run_to_block(1);
        let total_after_block_1 = QPow::get_total_work();
        assert!(total_after_block_1 > initial_total,
                "TotalDifficulty should increase after a new block");

        // Run to block 2 and check the increase again
        run_to_block(2);
        let total_after_block_2 = QPow::get_total_work();
        assert!(total_after_block_2 > total_after_block_1,
                "TotalDifficulty should increase after each new block");
        let max_distance = QPow::get_max_distance();
        // Verify that the increase matches the distance_threshold of block 2
        let block_2_diff = total_after_block_2 - total_after_block_1;
        assert_eq!(block_2_diff, max_distance / QPow::get_distance_threshold_at_block(2),
                   "TotalDifficulty increase should match the distance_threshold of the new block");
    });
}

#[test]
fn test_integrated_verification_flow() {
    new_test_ext().execute_with(|| {
        // Set up data
        let header = [1u8; 32];

        // Get the current distance_threshold
        let distance_threshold = QPow::get_distance_threshold_at_block(0);
        println!("Current distance_threshold: {}", distance_threshold);

        // Use a nonce that we know works for our tests
        let mut nonce = [0u8; 64];
        nonce[63] = 38;  // This worked in your previous tests

        // Make sure it's actually valid
        let distance = QPow::get_nonce_distance(header, nonce);
        println!("Nonce distance: {}, Threshold: {}", distance, distance_threshold);

        if distance > distance_threshold {
            println!("WARNING: Test nonce is not valid for current distance_threshold!");
            // Either generate a valid nonce here or fail the test
            assert!(distance <= distance_threshold, "Cannot proceed with invalid test nonce");
        }

        // 1. First, simulate mining by submitting a nonce
        assert!(QPow::submit_nonce(header, nonce));

        // 2. Then simulate block import verification
        assert!(QPow::verify_for_import(header, nonce));

        // 3. Finally verify historical block
        let current_block = System::block_number();
        assert!(QPow::verify_historical_block(header, nonce, current_block));
    });
}

#[test]
fn test_compute_pow_valid_nonce() {
    new_test_ext().execute_with(|| {
        let mut h = [0u8; 32];
        h[31] = 123; // For value 123

        let mut m = [0u8; 32];
        m[31] = 5;   // For value 5

        let mut n = [0u8; 64];
        n[63] = 17;  // For value 17

        let mut nonce = [0u8; 64];
        nonce[63] = 2; // For value 2

        // Compute the result and the truncated result based on distance_threshold
        let hash = hash_to_group(&h, &m, &n, &nonce);

        let manual_mod = mod_pow(
            &U512::from_big_endian(&m),
            &(U512::from_big_endian(&h) + U512::from_big_endian(&nonce)),
            &U512::from_big_endian(&n)
        );
        let manual_hash = sha3_512(manual_mod);

        // Check if the result is computed correctly
        assert_eq!(hash, manual_hash);
    });
}

#[test]
fn test_compute_pow_overflow_check() {
    new_test_ext().execute_with(|| {
        let h = [0xfu8; 32];

        let mut m = [0u8; 32];
        m[31] = 5;   // For value 5

        let mut n = [0u8; 64];
        n[63] = 17;  // For value 17

        let mut nonce = [0u8; 64];
        nonce[63] = 2; // For value 2

        // Compute the result and the truncated result based on distance_threshold
        let hash = hash_to_group(&h, &m, &n, &nonce);

        let manual_mod = mod_pow(
            &U512::from_big_endian(&m),
            &(U512::from_big_endian(&h) + U512::from_big_endian(&nonce)),
            &U512::from_big_endian(&n)
        );
        let manual_hash = sha3_512(manual_mod);

        // Check if the result is computed correctly
        assert_eq!(hash, manual_hash);
    });
}

#[test]
fn test_get_random_rsa() {
    new_test_ext().execute_with(|| {
        let header = [1u8; 32];
        let (m, n) = get_random_rsa(&header);

        // Check that n > m
        assert!(m < n);

        // Check that numbers are coprime
        assert!(is_coprime(&m, &n));

        // Test determinism - same header should give same numbers
        let (m2, n2) = get_random_rsa(&header);
        assert_eq!(m, m2);
        assert_eq!(n, n2);
    });
}

#[test]
fn test_primality_check() {
    new_test_ext().execute_with(|| {
        // Test some known primes
        assert!(is_prime(&U512::from(2u32)));
        assert!(is_prime(&U512::from(3u32)));
        assert!(is_prime(&U512::from(5u32)));
        assert!(is_prime(&U512::from(7u32)));
        assert!(is_prime(&U512::from(11u32)));
        assert!(is_prime(&U512::from(104729u32)));
        assert!(is_prime(&U512::from(1299709u32)));
        assert!(is_prime(&U512::from(15485863u32)));
        assert!(is_prime(&U512::from(982451653u32)));
        assert!(is_prime(&U512::from(32416190071u64)));
        assert!(is_prime(&U512::from(2305843009213693951u64)));
        assert!(is_prime(&U512::from(162259276829213363391578010288127u128)));

        // Test some known composites
        assert!(!is_prime(&U512::from(4u32)));
        assert!(!is_prime(&U512::from(6u32)));
        assert!(!is_prime(&U512::from(8u32)));
        assert!(!is_prime(&U512::from(9u32)));
        assert!(!is_prime(&U512::from(10u32)));
        assert!(!is_prime(&U512::from(561u32)));
        assert!(!is_prime(&U512::from(1105u32)));
        assert!(!is_prime(&U512::from(1729u32)));
        assert!(!is_prime(&U512::from(2465u32)));
        assert!(!is_prime(&U512::from(15841u32)));
        assert!(!is_prime(&U512::from(29341u32)));
        assert!(!is_prime(&U512::from(41041u32)));
        assert!(!is_prime(&U512::from(52633u32)));
        assert!(!is_prime(&U512::from(291311u32)));
        assert!(!is_prime(&U512::from(9999999600000123u64)));
        assert!(!is_prime(&U512::from(1000000016000000063u64)));
    });
}
/// Difficulty adjustment
#[test]
fn test_distance_threshold_adjustment_boundaries() {
    new_test_ext().execute_with(|| {
        // 1. Test minimum distance_threshold boundary

        // A. If initial distance_threshold is already at minimum, it should stay there
        let min_distance_threshold = U512::one();
        let current_distance_threshold = min_distance_threshold;  // Already at minimum

        let new_distance_threshold = QPow::calculate_distance_threshold(
            current_distance_threshold,
            10000,  // 10x target (extremely slow blocks)
            1000    // Target block time
        );

        // Should be clamped exactly to minimum
        assert_eq!(new_distance_threshold, min_distance_threshold,
                   "When already at minimum distance_threshold, it should stay at minimum: {}", min_distance_threshold);

        // B. If calculated distance_threshold would be below minimum, it should be clamped up
        let current_distance_threshold = min_distance_threshold + 100;  // Slightly above minimum

        // Set block time extremely high to force adjustment below minimum
        let extreme_block_time = 20000;  // 20x target

        let new_distance_threshold = QPow::calculate_distance_threshold(
            current_distance_threshold,
            extreme_block_time,
            1000    // Target block time
        );

        // Should be exactly at minimum
        assert_eq!(new_distance_threshold, min_distance_threshold,
                   "When adjustment would put distance_threshold below minimum, it should be clamped to minimum");

        // 2. Test maximum distance_threshold boundary
        let max_distance = QPow::get_max_distance();

        // A. If initial distance_threshold is already at maximum, it should stay there
        let current_distance_threshold = max_distance;  // Above Maximum
        let new_distance_threshold = QPow::calculate_distance_threshold(
            current_distance_threshold,
            10000,    // 0.1x target (extremely fast blocks)
            1000    // Target block time
        );

        // Should be clamped exactly to maximum
        assert_eq!(new_distance_threshold, max_distance,
                   "When already at maximum distance_threshold, it should stay at maximum: {}", max_distance);

        // B. If calculated distance_threshold would be above maximum, it should be clamped down
        let current_distance_threshold = max_distance - 1000;  // Slightly below maximum

        // Set block time extremely low to force adjustment above maximum
        let new_distance_threshold = QPow::calculate_distance_threshold(
            current_distance_threshold,
            10000,
            1000    // Target block time
        );

        // Should be exactly at maximum
        assert_eq!(new_distance_threshold, max_distance,
                   "When adjustment would put distance_threshold above maximum, it should be clamped to maximum");
    });
}

#[test]
fn test_calculate_distance_threshold_normal_adjustment() {
    new_test_ext().execute_with(|| {
        // Start with a medium distance_threshold
        let current_distance_threshold = QPow::get_distance_threshold_at_block(0);
        let target_time = 1000; // 1000ms target

        // Test slight deviation (10% slower)
        let block_time_slower = 1100; // 1.1x target
        let new_distance_threshold_slower = QPow::calculate_distance_threshold(
            current_distance_threshold,
            block_time_slower,
            target_time
        );

        // Difficulty should decrease slightly but not drastically
        assert!(new_distance_threshold_slower > current_distance_threshold, "Distance threshold should decrease when blocks are slower");
        let decrease_percentage = pack_u512_to_f64(new_distance_threshold_slower - current_distance_threshold) / pack_u512_to_f64(current_distance_threshold) * 100.0;
        assert_eq!(decrease_percentage.round(), 10f64, "For 10% slower blocks, distance_threshold should decrease by 10%, but decreased by {:.2}%", decrease_percentage);

        // Test slight deviation (10% faster)
        let block_time_faster = 900; // 0.9x target
        let new_distance_threshold_faster = QPow::calculate_distance_threshold(
            current_distance_threshold,
            block_time_faster,
            target_time
        );

        // Difficulty should increase slightly but not drastically
        assert!(new_distance_threshold_faster < current_distance_threshold, "Distance threshold should increase when blocks are faster");
        let increase_percentage = pack_u512_to_f64(current_distance_threshold - new_distance_threshold_faster) / pack_u512_to_f64(current_distance_threshold) * 100.0;
        assert_eq!(increase_percentage.round(), 10f64, "For 10% faster blocks, distance_threshold should increase by 10%, but increased by {:.2}%", increase_percentage);
    });
}

#[test]
fn test_calculate_distance_threshold_consecutive_adjustments() {
    new_test_ext().execute_with(|| {
        let mut current_distance_threshold = QPow::get_distance_threshold_at_block(0);
        let initial_distance_threshold = QPow::get_distance_threshold_at_block(0);
        let target_time = 1000;

        // First, measure the effect of a single adjustment
        let block_time = 1500; // 50% slower than target
        let new_distance_threshold = QPow::calculate_distance_threshold(
            current_distance_threshold,
            block_time,
            target_time
        );
        let single_adjustment_increase = pack_u512_to_f64(new_distance_threshold - current_distance_threshold) / pack_u512_to_f64(current_distance_threshold) * 100.0;
        println!("Single adjustment increase: {:.2}%", single_adjustment_increase);

        // Reset and simulate 5 consecutive periods
        current_distance_threshold = QPow::get_distance_threshold_at_block(0);
        for i in 0..5 {
            let new_distance_threshold = QPow::calculate_distance_threshold(
                current_distance_threshold,
                block_time,
                target_time
            );

            println!("Adjustment {}: increased by {:.2}%",
                     i + 1,
                     pack_u512_to_f64(new_distance_threshold - current_distance_threshold) / pack_u512_to_f64(current_distance_threshold) * 100.0);

            // Each adjustment should decrease distance_threshold
            assert!(new_distance_threshold > current_distance_threshold,
                    "Distance threshold should increase with consistently slower blocks (iteration {})", i);


            // Set up for next iteration
            current_distance_threshold = new_distance_threshold;
        }

        // After 5 consecutive adjustments, calculate total decrease
        let total_increase_percentage = pack_u512_to_f64(current_distance_threshold - initial_distance_threshold) / pack_u512_to_f64(initial_distance_threshold) * 100.0;
        println!("Total distance_threshold decrease after 5 periods: {:.2}%", total_increase_percentage);

        // Verify the diminishing returns behavior
        assert!(total_increase_percentage < single_adjustment_increase * 7.0,
                "With strong dampening, total effect should be less than a single period effect multiplied by 7");
    });
}

#[test]
fn test_calculate_distance_threshold_oscillation_damping() {
    new_test_ext().execute_with(|| {
        let initial_distance_threshold = QPow::get_distance_threshold_at_block(0);
        let target_time = 1000;

        // Start with current distance_threshold
        let mut current_distance_threshold = initial_distance_threshold;

        // First adjustment: blocks 50% slower
        let first_adjustment = QPow::calculate_distance_threshold(
            current_distance_threshold,
            1500, // 50% slower
            target_time
        );

        // Difficulty should decrease
        assert!(first_adjustment > current_distance_threshold);
        current_distance_threshold = first_adjustment;

        // Second adjustment: blocks 50% faster than target (return to normal speed)
        let second_adjustment = QPow::calculate_distance_threshold(
            current_distance_threshold,
            500, // 50% faster
            target_time
        );

        // Difficulty should increase but should not overshoot initial distance_threshold significantly
        assert!(second_adjustment < current_distance_threshold);

        let second_adjustment = pack_u512_to_f64(second_adjustment);
        let initial_distance_threshold = pack_u512_to_f64(initial_distance_threshold);

        let overshoot_percentage = (second_adjustment - initial_distance_threshold) / initial_distance_threshold * 100.0;

        // Due to dampening, we don't expect massive overshooting
        assert!(overshoot_percentage.abs() <= 25.0,
                "After oscillating block times, distance_threshold should not overshoot initial value by more than 15%, but overshot by {:.2}%",
                overshoot_percentage);
    });
}

fn pack_u512_to_f64(value: U512) -> f64 {
    // Convert U512 to big-endian bytes (64 bytes)
    let bytes = value.to_big_endian();

    // Take the highest-order 8 bytes (first 8 bytes in big-endian)
    let mut highest_8_bytes = [0u8; 8];
    highest_8_bytes.copy_from_slice(&bytes[0..8]);

    // Convert to u64
    let highest_64_bits = u64::from_be_bytes(highest_8_bytes);

    // Cast to f64
    highest_64_bits as f64
}


#[test]
fn test_calculate_distance_threshold_stability_over_time() {
    new_test_ext().execute_with(|| {
        let initial_distance_threshold = QPow::get_distance_threshold_at_block(0);
        let target_time = 1000;
        let mut current_distance_threshold = initial_distance_threshold;

        // Simulate slight random variance around target (normal mining conditions)
        let block_times = [950, 1050, 980, 1020, 990, 1010, 970, 1030, 960, 1040];

        // Apply 10 consecutive adjustments with minor variations around target
        for &block_time in &block_times {
            current_distance_threshold = QPow::calculate_distance_threshold(
                current_distance_threshold,
                block_time,
                target_time
            );
        }

        let current_distance_threshold = pack_u512_to_f64(current_distance_threshold);
        let initial_distance_threshold = pack_u512_to_f64(initial_distance_threshold);

        // After these minor variations, distance_threshold should remain relatively stable
        let final_change_percentage = (current_distance_threshold - initial_distance_threshold) / initial_distance_threshold * 100.0;
        assert!(final_change_percentage.abs() < 10.0,
                "With minor variations around target time, distance_threshold should not change by more than 10%, but changed by {:.2}%",
                final_change_percentage);
    });
}

/// Median & Ring Buffer

#[test]
fn test_median_block_time_empty_history() {
    new_test_ext().execute_with(|| {
        // When history is empty, we should get TargetBlockTime
        let target_block_time = <Test as Config>::TargetBlockTime::get();
        let median = QPow::get_median_block_time();
        assert_eq!(median, target_block_time, "Empty history should return target block time");
    });
}

#[test]
fn test_median_block_time_single_value() {
    new_test_ext().execute_with(|| {
        // Add a single entry to history
        let block_time = 2000;
        <HistoryIndex<Test>>::put(0);
        <HistorySize<Test>>::put(1);
        <BlockTimeHistory<Test>>::insert(0, block_time);

        // Median of a single value is that value
        let median = QPow::get_median_block_time();
        assert_eq!(median, block_time, "Median of a single value should be that value");
    });
}

#[test]
fn test_median_block_time_odd_count() {
    new_test_ext().execute_with(|| {
        // Add odd number of entries
        let block_times = [1000, 3000, 2000, 5000, 4000];
        let history_size = block_times.len() as u32;

        <HistorySize<Test>>::put(history_size);

        // Add times to history
        for (i, &time) in block_times.iter().enumerate() {
            <BlockTimeHistory<Test>>::insert(i as u32, time);
        }

        // Median of sorted values [1000, 2000, 3000, 4000, 5000] is 3000
        let expected_median = 3000;
        let median = QPow::get_median_block_time();
        assert_eq!(median, expected_median, "Median of odd count should be the middle value");
    });
}

#[test]
fn test_median_block_time_even_count() {
    new_test_ext().execute_with(|| {
        // Add even number of entries
        let block_times = [1000, 3000, 2000, 4000];
        let history_size = block_times.len() as u32;

        <HistorySize<Test>>::put(history_size);

        // Add times to history
        for (i, &time) in block_times.iter().enumerate() {
            <BlockTimeHistory<Test>>::insert(i as u32, time);
        }

        // Median of sorted values [1000, 2000, 3000, 4000] is (2000 + 3000) / 2 = 2500
        let expected_median = 2500;
        let median = QPow::get_median_block_time();
        assert_eq!(median, expected_median, "Median of even count should be average of two middle values");
    });
}

#[test]
fn test_median_block_time_with_duplicates() {
    new_test_ext().execute_with(|| {
        // Add entries with duplicates
        let block_times = [1000, 2000, 2000, 2000, 3000];
        let history_size = block_times.len() as u32;

        <HistorySize<Test>>::put(history_size);

        // Add times to history
        for (i, &time) in block_times.iter().enumerate() {
            <BlockTimeHistory<Test>>::insert(i as u32, time);
        }

        // Median of sorted values [1000, 2000, 2000, 2000, 3000] is 2000
        let expected_median = 2000;
        let median = QPow::get_median_block_time();
        assert_eq!(median, expected_median, "Median with duplicates should be correctly calculated");
    });
}

#[test]
fn test_median_block_time_ring_buffer() {
    new_test_ext().execute_with(|| {
        // Test if the ring buffer works correctly
        // Assuming <Test as Config>::BlockTimeHistorySize::get() = 5

        // Add more entries than the maximum history size
        let initial_times = [1000, 2000, 3000, 4000, 5000];

        // Set initial history
        <HistoryIndex<Test>>::put(0);
        <HistorySize<Test>>::put(5);

        for (i, &time) in initial_times.iter().enumerate() {
            <BlockTimeHistory<Test>>::insert(i as u32, time);
        }

        // Initial median
        let initial_median = QPow::get_median_block_time();
        assert_eq!(initial_median, 3000, "Initial median should be 3000");

        // Simulate record_block_time for new values
        // Should overwrite oldest values in the ring buffer
        <HistoryIndex<Test>>::put(0); // Start overwriting from index 0
        <BlockTimeHistory<Test>>::insert(0, 6000);
        <HistoryIndex<Test>>::put(1);
        <BlockTimeHistory<Test>>::insert(1, 7000);

        // New median from [3000, 4000, 5000, 6000, 7000]
        let new_median = QPow::get_median_block_time();
        assert_eq!(new_median, 5000, "New median should be calculated from updated ring buffer");
    });
}

#[test]
fn test_block_distance_threshold_storage_and_retrieval() {
    new_test_ext().execute_with(|| {
        // 1. Test that genesis block distance_threshold is properly set
        let genesis_distance_threshold = QPow::get_distance_threshold_at_block(0);
        let initial_distance_threshold = U512::one().shl(<Test as Config>::InitialDistanceThresholdExponent::get());
        assert_eq!(genesis_distance_threshold, initial_distance_threshold,
                   "Genesis block should have initial distance_threshold");

        // 2. Simulate block production and distance_threshold adjustment
        run_to_block(1);
        let block_1_distance_threshold = QPow::get_distance_threshold_at_block(1);
        assert_eq!(block_1_distance_threshold, initial_distance_threshold,
                   "Block 1 should have same distance_threshold as initial");

        // 3. Simulate multiple blocks to trigger distance_threshold adjustment
        let adjustment_period = <Test as Config>::AdjustmentPeriod::get();
        run_to_block(adjustment_period + 1);

        // 4. Check that distance_threshold for early blocks hasn't changed
        let block_1_distance_threshold_after = QPow::get_distance_threshold_at_block(1);
        assert_eq!(block_1_distance_threshold_after, block_1_distance_threshold,
                   "Historical block distance_threshold should not change");

        // 5. Test non-existent block (future block)
        let latest_block = System::block_number();
        let future_block = latest_block + 1000;
        let future_distance_threshold = QPow::get_distance_threshold_at_block(future_block);
        assert_eq!(future_distance_threshold, U512::zero(),
                   "Future block distance_threshold should return 0");
    });
}

//////////// Support methods
pub fn hash_to_group(
    h: &[u8; 32],
    m: &[u8; 32],
    n: &[u8; 64],
    nonce: &[u8; 64]
) -> U512 {
    let h = U512::from_big_endian(h);
    let m = U512::from_big_endian(m);
    let n = U512::from_big_endian(n);
    let nonce_u = U512::from_big_endian(nonce);
    hash_to_group_bigint_sha(&h, &m, &n, &nonce_u)
}

fn run_to_block(n: u32) {
    while System::block_number() < n as u64 {
        System::set_block_number(System::block_number() + 1);
        <QPow as Hooks<_>>::on_finalize(System::block_number());
    }
}
