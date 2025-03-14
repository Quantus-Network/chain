use frame_support::pallet_prelude::TypedGet;
use frame_support::traits::Hooks;
use crate::mock::*;
use primitive_types::U512;
use crate::{BlockTimeHistory, HistoryIndex, HistorySize, MAX_DISTANCE};
use crate::Config;

#[test]
fn test_submit_valid_proof() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let header = [1u8; 32];

        // Get current difficulty
        let difficulty = QPow::get_difficulty();
        println!("Current difficulty: {}", difficulty);

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

            let threshold = MAX_DISTANCE - difficulty;

            // Check if we found a pair where one is valid and one is invalid
            if invalid_distance > threshold && valid_distance <= threshold {
                println!("Found test pair: invalid={}, valid={}", i, i+1);
                println!("Invalid distance: {}, Valid distance: {}, Threshold: {}",
                         invalid_distance, valid_distance, threshold);
                found_pair = true;
                break;
            }
        }

        if !found_pair {
            panic!("Could not find valid/invalid nonce pair for testing with difficulty {}", difficulty);
        }

        // Now run the test with our dynamically found values

        // Submit an invalid proof
        assert!(!QPow::submit_nonce(header, invalid_nonce),
                "Nonce should be invalid with distance {} > threshold {}",
                QPow::get_nonce_distance(header, invalid_nonce),
                MAX_DISTANCE - difficulty);

        // Submit a valid proof
        assert!(QPow::submit_nonce(header, valid_nonce),
                "Nonce should be valid with distance {} <= threshold {}",
                QPow::get_nonce_distance(header, valid_nonce),
                MAX_DISTANCE - difficulty);

        assert_eq!(QPow::latest_proof(), Some(valid_nonce));

        // Find a second valid nonce for medium difficulty test
        let mut second_valid = valid_nonce;
        let mut found_second = false;

        for i in valid_nonce[63]+1..255 {
            second_valid[63] = i;
            let distance = QPow::get_nonce_distance(header, second_valid);
            if distance <= MAX_DISTANCE - difficulty {
                println!("Found second valid nonce: {}", i);
                found_second = true;
                break;
            }
        }

        if found_second {
            // Submit the second valid proof
            assert!(QPow::submit_nonce(header, second_valid));
            assert_eq!(QPow::latest_proof(), Some(second_valid));
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
        let mut nonce = [0u8; 64];

        // Use a nonce that would be valid with current difficulty
        nonce[63] = 5;  // Assuming this gives a valid distance for the current difficulty

        // Verify the nonce during import
        assert!(QPow::verify_for_import(header, nonce));

        // Check that the latest proof was stored but no event was emitted
        assert_eq!(QPow::latest_proof(), Some(nonce));

        // TODO: Verify no event was emitted
        // This requires specific testing infrastructure to verify absence of events
    });
}

#[test]
fn test_verify_historical_block() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let header = [1u8; 32];

        // Get the genesis block difficulty
        let genesis_difficulty = QPow::get_difficulty_at_block(0);
        println!("Genesis difficulty: {}", genesis_difficulty);

        // Use a nonce that we know works better with our test difficulty
        let mut nonce = [0u8; 64];
        nonce[63] = 14;  // This seemed to work in other tests

        // Check if this nonce is valid for genesis difficulty
        let distance = QPow::get_nonce_distance(header, nonce);
        let threshold = MAX_DISTANCE - genesis_difficulty;

        println!("Nonce distance: {}, Threshold: {}", distance, threshold);

        if distance > threshold {
            println!("Test nonce is not valid for genesis difficulty - trying alternatives");

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
                panic!("Could not find a valid nonce for genesis difficulty. Test cannot proceed.");
            }
        }

        // Now we should have a valid nonce for genesis block
        assert!(QPow::verify_historical_block(header, nonce, 0),
                "Nonce with distance {} should be valid for threshold {}",
                distance, threshold);

        // Now let's create a block at height 1 with a specific difficulty
        run_to_block(1);

        // Get the difficulty that was stored for block 1
        let block_1_difficulty = QPow::get_difficulty_at_block(1);
        assert!(block_1_difficulty > 0, "Block 1 should have a stored difficulty");

        // Need to verify our nonce is still valid for block 1's difficulty
        let block_1_threshold = MAX_DISTANCE - block_1_difficulty;
        if distance > block_1_threshold {
            println!("Warning: Test nonce valid for genesis but not for block 1");
            println!("Block 1 difficulty: {}, threshold: {}", block_1_difficulty, block_1_threshold);
        }

        // Verify a nonce against block 1's difficulty with direct method
        assert!(QPow::is_valid_nonce(header, nonce, block_1_difficulty),
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
fn test_difficulty_storage_and_retrieval() {
    new_test_ext().execute_with(|| {
        // 1. Test genesis block difficulty
        let genesis_difficulty = QPow::get_difficulty_at_block(0);
        let initial_difficulty = <Test as Config>::InitialDifficulty::get();
        assert_eq!(genesis_difficulty, initial_difficulty,
                   "Genesis block should have initial difficulty");

        // 2. Simulate block production
        run_to_block(1);

        // 3. Check difficulty for block 1
        let block_1_difficulty = QPow::get_difficulty_at_block(1);
        assert_eq!(block_1_difficulty, initial_difficulty,
                   "Block 1 should have same difficulty as initial");

        // 4. Simulate adjustment period
        let adjustment_period = <Test as Config>::AdjustmentPeriod::get();
        run_to_block(adjustment_period + 1);

        // 5. Verify historical blocks maintain their difficulty
        let block_1_difficulty_after = QPow::get_difficulty_at_block(1);
        assert_eq!(block_1_difficulty_after, block_1_difficulty,
                   "Historical block difficulty should not change");

        // 6. Verify nonexistent block returns 0
        let latest_block = System::block_number();
        let future_block = latest_block + 1000;
        assert_eq!(QPow::get_difficulty_at_block(future_block), 0,
                   "Future block difficulty should be 0");
    });
}

/*
TODO - we should catch events in tests - this should be possible
#[test]
fn test_submit_nonce_emits_event() {
    new_test_ext().execute_with(|| {
        // Set up data
        let header = [1u8; 32];

        // Get the current difficulty so we know what we're targeting
        let difficulty = QPow::get_difficulty();
        println!("Current difficulty: {}", difficulty);

        // Create a nonce that we know works
        let mut nonce = [0u8; 64];
        nonce[63] = 14;

        // Calculate the distance for this nonce
        let distance = QPow::get_nonce_distance(header, nonce);
        let threshold = MAX_DISTANCE - difficulty;

        println!("Nonce: {:?}, Distance: {}, Threshold: {}",
                 &nonce[62..64], distance, threshold);

        // Wyczyść zdarzenia systemu przed wysłaniem nonce
        System::reset_events();

        // Submit nonce (zakładamy, że jest prawidłowy)
        assert!(QPow::submit_nonce(header, nonce));

        // Sprawdź, czy zdarzenie zostało wyemitowane
        let events = System::events();
        println!("Events: {:?}", events);

        // Sprawdź, czy istnieje zdarzenie ProofSubmitted
        let proof_submitted_event_found = events.iter().any(|record| {
            match &record.event {
                RuntimeEvent::QPow(Event::ProofSubmitted { nonce: event_nonce }) => {
                    // Sprawdź, czy nonce w zdarzeniu zgadza się z tym, który wysłaliśmy
                    *event_nonce == nonce
                },
                _ => false,
            }
        });

        assert!(proof_submitted_event_found, "ProofSubmitted event was not emitted or had incorrect data");
    });
}
*/

#[test]
fn test_integrated_verification_flow() {
    new_test_ext().execute_with(|| {
        // Set up data
        let header = [1u8; 32];

        // Get the current difficulty
        let difficulty = QPow::get_difficulty();
        println!("Current difficulty: {}", difficulty);

        // Use a nonce that we know works for our tests
        let mut nonce = [0u8; 64];
        nonce[63] = 14;  // This worked in your previous tests

        // Make sure it's actually valid
        let distance = QPow::get_nonce_distance(header, nonce);
        let threshold = MAX_DISTANCE - difficulty;
        println!("Nonce distance: {}, Threshold: {}", distance, threshold);

        if distance > threshold {
            println!("WARNING: Test nonce is not valid for current difficulty!");
            // Either generate a valid nonce here or fail the test
            assert!(distance <= threshold, "Cannot proceed with invalid test nonce");
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

        // Compute the result and the truncated result based on difficulty
        let hash = hash_to_group(&h, &m, &n, &nonce);

        let manual_mod = QPow::mod_pow(
            &U512::from_big_endian(&m),
            &(U512::from_big_endian(&h) + U512::from_big_endian(&nonce)),
            &U512::from_big_endian(&n)
        );
        let manual_chunks = QPow::split_chunks(&manual_mod);

        // Check if the result is computed correctly
        assert_eq!(hash, manual_chunks);
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

        // Compute the result and the truncated result based on difficulty
        let hash = hash_to_group(&h, &m, &n, &nonce);

        let manual_mod = QPow::mod_pow(
            &U512::from_big_endian(&m),
            &(U512::from_big_endian(&h) + U512::from_big_endian(&nonce)),
            &U512::from_big_endian(&n)
        );
        let manual_chunks = QPow::split_chunks(&manual_mod);

        // Check if the result is computed correctly
        assert_eq!(hash, manual_chunks);
    });
}

#[test]
fn test_get_random_rsa() {
    new_test_ext().execute_with(|| {
        let header = [1u8; 32];
        let (m, n) = QPow::get_random_rsa(&header);

        // Check that n > m
        assert!(U512::from(m) < n);

        // Check that numbers are coprime
        assert!(QPow::is_coprime(&m, &n));

        // Test determinism - same header should give same numbers
        let (m2, n2) = QPow::get_random_rsa(&header);
        assert_eq!(m, m2);
        assert_eq!(n, n2);
    });
}

#[test]
fn test_primality_check() {
    new_test_ext().execute_with(|| {
        // Test some known primes
        assert!(QPow::is_prime(&U512::from(2u32)));
        assert!(QPow::is_prime(&U512::from(3u32)));
        assert!(QPow::is_prime(&U512::from(5u32)));
        assert!(QPow::is_prime(&U512::from(7u32)));
        assert!(QPow::is_prime(&U512::from(11u32)));
        assert!(QPow::is_prime(&U512::from(104729u32)));
        assert!(QPow::is_prime(&U512::from(1299709u32)));
        assert!(QPow::is_prime(&U512::from(15485863u32)));
        assert!(QPow::is_prime(&U512::from(982451653u32)));
        assert!(QPow::is_prime(&U512::from(32416190071u64)));
        assert!(QPow::is_prime(&U512::from(2305843009213693951u64)));
        assert!(QPow::is_prime(&U512::from(162259276829213363391578010288127u128)));

        // Test some known composites
        assert!(!QPow::is_prime(&U512::from(4u32)));
        assert!(!QPow::is_prime(&U512::from(6u32)));
        assert!(!QPow::is_prime(&U512::from(8u32)));
        assert!(!QPow::is_prime(&U512::from(9u32)));
        assert!(!QPow::is_prime(&U512::from(10u32)));
        assert!(!QPow::is_prime(&U512::from(561u32)));
        assert!(!QPow::is_prime(&U512::from(1105u32)));
        assert!(!QPow::is_prime(&U512::from(1729u32)));
        assert!(!QPow::is_prime(&U512::from(2465u32)));
        assert!(!QPow::is_prime(&U512::from(15841u32)));
        assert!(!QPow::is_prime(&U512::from(29341u32)));
        assert!(!QPow::is_prime(&U512::from(41041u32)));
        assert!(!QPow::is_prime(&U512::from(52633u32)));
        assert!(!QPow::is_prime(&U512::from(291311u32)));
        assert!(!QPow::is_prime(&U512::from(9999999600000123u64)));
        assert!(!QPow::is_prime(&U512::from(1000000016000000063u64)));
    });
}
/// Difficulty adjustment
#[test]
fn test_difficulty_adjustment_boundaries() {
    new_test_ext().execute_with(|| {
        // 1. Test minimum difficulty boundary

        // A. If initial difficulty is already at minimum, it should stay there
        let min_difficulty = <Test as Config>::MinDifficulty::get();
        let current_difficulty = min_difficulty;  // Already at minimum

        let new_difficulty = QPow::calculate_difficulty(
            current_difficulty,
            10000,  // 10x target (extremely slow blocks)
            1000    // Target block time
        );

        // Should be clamped exactly to minimum
        assert_eq!(new_difficulty, min_difficulty,
                   "When already at minimum difficulty, it should stay at minimum: {}", min_difficulty);

        // B. If calculated difficulty would be below minimum, it should be clamped up
        let current_difficulty = min_difficulty + 100;  // Slightly above minimum

        // Set block time extremely high to force adjustment below minimum
        let extreme_block_time = 20000;  // 20x target

        let new_difficulty = QPow::calculate_difficulty(
            current_difficulty,
            extreme_block_time,
            1000    // Target block time
        );

        // Should be exactly at minimum
        assert_eq!(new_difficulty, min_difficulty,
                   "When adjustment would put difficulty below minimum, it should be clamped to minimum");

        // 2. Test maximum difficulty boundary

        // A. If initial difficulty is already at maximum, it should stay there
        let max_difficulty = MAX_DISTANCE - 1;
        let current_difficulty = max_difficulty+100;  // Above Maximum

        let new_difficulty = QPow::calculate_difficulty(
            current_difficulty,
            100,    // 0.1x target (extremely fast blocks)
            1000    // Target block time
        );

        // Should be clamped exactly to maximum
        assert_eq!(new_difficulty, max_difficulty,
                   "When already at maximum difficulty, it should stay at maximum: {}", max_difficulty);

        // B. If calculated difficulty would be above maximum, it should be clamped down
        let current_difficulty = max_difficulty - 1000;  // Slightly below maximum

        // Set block time extremely low to force adjustment above maximum
        let extreme_block_time = 10;  // 0.01x target

        let new_difficulty = QPow::calculate_difficulty(
            current_difficulty,
            extreme_block_time,
            1000    // Target block time
        );

        // Should be exactly at maximum
        assert_eq!(new_difficulty, max_difficulty,
                   "When adjustment would put difficulty above maximum, it should be clamped to maximum");
    });
}

#[test]
fn test_calculate_difficulty_normal_adjustment() {
    new_test_ext().execute_with(|| {
        // Start with a medium difficulty
        let current_difficulty = <Test as Config>::InitialDifficulty::get();
        let target_time = 1000; // 1000ms target

        // Test slight deviation (10% slower)
        let block_time_slower = 1100; // 1.1x target
        let new_difficulty_slower = QPow::calculate_difficulty(
            current_difficulty,
            block_time_slower,
            target_time
        );

        // Difficulty should decrease slightly but not drastically
        assert!(new_difficulty_slower < current_difficulty, "Difficulty should decrease when blocks are slower");
        let decrease_percentage = (current_difficulty - new_difficulty_slower) as f64 / current_difficulty as f64 * 100.0;
        assert!(decrease_percentage < 5.0, "For 10% slower blocks, difficulty should decrease by less than 5%, but decreased by {:.2}%", decrease_percentage);

        // Test slight deviation (10% faster)
        let block_time_faster = 900; // 0.9x target
        let new_difficulty_faster = QPow::calculate_difficulty(
            current_difficulty,
            block_time_faster,
            target_time
        );

        // Difficulty should increase slightly but not drastically
        assert!(new_difficulty_faster > current_difficulty, "Difficulty should increase when blocks are faster");
        let increase_percentage = (new_difficulty_faster - current_difficulty) as f64 / current_difficulty as f64 * 100.0;
        assert!(increase_percentage < 5.0, "For 10% faster blocks, difficulty should increase by less than 5%, but increased by {:.2}%", increase_percentage);
    });
}

#[test]
fn test_calculate_difficulty_dampening_effect() {
    new_test_ext().execute_with(|| {
        let current_difficulty = <Test as Config>::InitialDifficulty::get();
        let target_time = 1000;

        // Test significant deviation (2x slower blocks)
        let block_time_much_slower = 2000; // 2x target
        let new_difficulty_much_slower = QPow::calculate_difficulty(
            current_difficulty,
            block_time_much_slower,
            target_time
        );

        // The dampening should prevent the difficulty from halving immediately
        let decrease_percentage = (current_difficulty - new_difficulty_much_slower) as f64 / current_difficulty as f64 * 100.0;
        assert!(decrease_percentage < 25.0, "Even for 2x slower blocks, dampening should limit decrease to less than 25%, but got {:.2}%", decrease_percentage);

        // Test significant deviation (0.5x faster blocks)
        let block_time_much_faster = 500; // 0.5x target
        let new_difficulty_much_faster = QPow::calculate_difficulty(
            current_difficulty,
            block_time_much_faster,
            target_time
        );

        // The dampening should prevent the difficulty from doubling immediately
        let increase_percentage = (new_difficulty_much_faster - current_difficulty) as f64 / current_difficulty as f64 * 100.0;
        assert!(increase_percentage < 25.0, "Even for 2x faster blocks, dampening should limit increase to less than 25%, but got {:.2}%", increase_percentage);
    });
}

#[test]
fn test_calculate_difficulty_consecutive_adjustments() {
    new_test_ext().execute_with(|| {
        let mut current_difficulty = <Test as Config>::InitialDifficulty::get();
        let target_time = 1000;

        // First, measure the effect of a single adjustment
        let block_time = 1500; // 50% slower than target
        let new_difficulty = QPow::calculate_difficulty(
            current_difficulty,
            block_time,
            target_time
        );
        let single_adjustment_decrease = (current_difficulty - new_difficulty) as f64 / current_difficulty as f64 * 100.0;
        println!("Single adjustment decrease: {:.2}%", single_adjustment_decrease);

        // Reset and simulate 5 consecutive periods
        current_difficulty = <Test as Config>::InitialDifficulty::get();
        for i in 0..5 {
            let new_difficulty = QPow::calculate_difficulty(
                current_difficulty,
                block_time,
                target_time
            );

            // Each adjustment should decrease difficulty
            assert!(new_difficulty < current_difficulty,
                    "Difficulty should decrease with consistently slower blocks (iteration {})", i);

            println!("Adjustment {}: decreased by {:.2}%",
                     i + 1,
                     (current_difficulty - new_difficulty) as f64 / current_difficulty as f64 * 100.0);

            // Set up for next iteration
            current_difficulty = new_difficulty;
        }

        // After 5 consecutive adjustments, calculate total decrease
        let total_decrease_percentage = (<Test as Config>::InitialDifficulty::get() - current_difficulty) as f64 / <Test as Config>::InitialDifficulty::get() as f64 * 100.0;
        println!("Total difficulty decrease after 5 periods: {:.2}%", total_decrease_percentage);

        // Check that there is some decrease
        assert!(total_decrease_percentage > 0.0,
                "After 5 consecutive periods of 50% slower blocks, difficulty should decrease somewhat");

        // Verify the diminishing returns behavior
        assert!(total_decrease_percentage < single_adjustment_decrease * 5.0,
                "With strong dampening, total effect should be less than a single period effect multiplied by 5");
    });
}

#[test]
fn test_calculate_difficulty_oscillation_damping() {
    new_test_ext().execute_with(|| {
        let initial_difficulty = <Test as Config>::InitialDifficulty::get();
        let target_time = 1000;

        // Start with current difficulty
        let mut current_difficulty = initial_difficulty;

        // First adjustment: blocks 50% slower
        let first_adjustment = QPow::calculate_difficulty(
            current_difficulty,
            1500, // 50% slower
            target_time
        );

        // Difficulty should decrease
        assert!(first_adjustment < current_difficulty);
        current_difficulty = first_adjustment;

        // Second adjustment: blocks 50% faster than target (return to normal speed)
        let second_adjustment = QPow::calculate_difficulty(
            current_difficulty,
            500, // 50% faster
            target_time
        );

        // Difficulty should increase but should not overshoot initial difficulty significantly
        assert!(second_adjustment > current_difficulty);
        let overshoot_percentage = (second_adjustment as f64 - initial_difficulty as f64) / initial_difficulty as f64 * 100.0;

        // Due to dampening, we don't expect massive overshooting
        assert!(overshoot_percentage.abs() < 15.0,
                "After oscillating block times, difficulty should not overshoot initial value by more than 15%, but overshot by {:.2}%",
                overshoot_percentage);
    });
}

#[test]
fn test_calculate_difficulty_stability_over_time() {
    new_test_ext().execute_with(|| {
        let initial_difficulty = <Test as Config>::InitialDifficulty::get();
        let target_time = 1000;
        let mut current_difficulty = initial_difficulty;

        // Simulate slight random variance around target (normal mining conditions)
        let block_times = [950, 1050, 980, 1020, 990, 1010, 970, 1030, 960, 1040];

        // Apply 10 consecutive adjustments with minor variations around target
        for &block_time in &block_times {
            current_difficulty = QPow::calculate_difficulty(
                current_difficulty,
                block_time,
                target_time
            );
        }

        // After these minor variations, difficulty should remain relatively stable
        let final_change_percentage = (current_difficulty as f64 - initial_difficulty as f64) / initial_difficulty as f64 * 100.0;
        assert!(final_change_percentage.abs() < 10.0,
                "With minor variations around target time, difficulty should not change by more than 10%, but changed by {:.2}%",
                final_change_percentage);
    });
}

#[test]
fn test_calculate_difficulty_power_factor_effect() {
    new_test_ext().execute_with(|| {
        let current_difficulty = <Test as Config>::InitialDifficulty::get();
        let target_time = 1000;

        // Test with extreme deviations to check power factor effect
        let deviations = [
            (4000, "4x slower"), // 4x slower
            (2000, "2x slower"),
            (1500, "1.5x slower"),
            (667, "1.5x faster"),
            (500, "2x faster"),
            (250, "4x faster")
        ];

        for (block_time, description) in &deviations {
            let new_difficulty = QPow::calculate_difficulty(
                current_difficulty,
                *block_time,
                target_time
            );

            // Calculate the relative change
            let change_ratio = new_difficulty as f64 / current_difficulty as f64;

            // Verify the direction of change is correct
            if *block_time > target_time {
                assert!(change_ratio < 1.0, "For {} blocks, difficulty should decrease", description);
            } else {
                assert!(change_ratio > 1.0, "For {} blocks, difficulty should increase", description);
            }

            // Log the change for analysis
            println!("For {} blocks: difficulty changed by factor {:.3}", description, change_ratio);

            // Power factor (1/16) with dampening should make the change less dramatic than the time deviation
            let expected_max_change = if *block_time > target_time {
                // For slower blocks, difficulty decrease should be less dramatic than time increase
                1.0 / ((*block_time as f64 / target_time as f64).powf(0.25)) // 1/4 is much more aggressive than 1/16
            } else {
                // For faster blocks, difficulty increase should be less dramatic than time decrease
                (target_time as f64 / *block_time as f64).powf(0.25) // 1/4 is much more aggressive than 1/16
            };

            // Due to dampening, the actual change should be much less than even the 1/4 power
            assert!((change_ratio - 1.0).abs() < (expected_max_change - 1.0).abs(),
                    "For {} blocks, change ratio {:.3} should be less extreme than 1/4 power factor would suggest {:.3}",
                    description, change_ratio, expected_max_change);
        }
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
        let block_times = vec![1000, 3000, 2000, 5000, 4000];
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
        let block_times = vec![1000, 3000, 2000, 4000];
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
        let block_times = vec![1000, 2000, 2000, 2000, 3000];
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
        let initial_times = vec![1000, 2000, 3000, 4000, 5000];

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
fn test_block_difficulty_storage_and_retrieval() {
    new_test_ext().execute_with(|| {
        // 1. Test that genesis block difficulty is properly set
        let genesis_difficulty = QPow::get_difficulty_at_block(0);
        let initial_difficulty = <Test as Config>::InitialDifficulty::get();
        assert_eq!(genesis_difficulty, initial_difficulty,
                   "Genesis block should have initial difficulty");

        // 2. Simulate block production and difficulty adjustment
        run_to_block(1);
        let block_1_difficulty = QPow::get_difficulty_at_block(1);
        assert_eq!(block_1_difficulty, initial_difficulty,
                   "Block 1 should have same difficulty as initial");

        // 3. Simulate multiple blocks to trigger difficulty adjustment
        let adjustment_period = <Test as Config>::AdjustmentPeriod::get();
        run_to_block(adjustment_period + 1);

        // 4. Check that difficulty for early blocks hasn't changed
        let block_1_difficulty_after = QPow::get_difficulty_at_block(1);
        assert_eq!(block_1_difficulty_after, block_1_difficulty,
                   "Historical block difficulty should not change");

        // 5. Test non-existent block (future block)
        let latest_block = System::block_number();
        let future_block = latest_block + 1000;
        let future_difficulty = QPow::get_difficulty_at_block(future_block);
        assert_eq!(future_difficulty, 0,
                   "Future block difficulty should return 0");
    });
}

//////////// Support methods
pub fn hash_to_group(
    h: &[u8; 32],
    m: &[u8; 32],
    n: &[u8; 64],
    nonce: &[u8; 64]
) -> [u32; 16] {
    let h = U512::from_big_endian(h);
    let m = U512::from_big_endian(m);
    let n = U512::from_big_endian(n);
    let nonce_u = U512::from_big_endian(nonce);
    QPow::hash_to_group_bigint_split(&h, &m, &n, &nonce_u)
}

fn run_to_block(n: u32) {
    while System::block_number() < n as u64 {
        System::set_block_number(System::block_number() + 1);
        <QPow as Hooks<_>>::on_finalize(System::block_number());
    }
}
