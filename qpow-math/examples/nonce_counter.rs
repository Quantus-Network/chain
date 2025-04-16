// Renamed from benchmark to nonce counter
// No longer uses criterion

use primitive_types::U512; // Need U512 for incrementing nonce
use qpow_math::{get_nonce_distance, is_valid_nonce, MAX_DISTANCE}; // Import is_valid_nonce directly
use rand::{rngs::ThreadRng, thread_rng, RngCore};
use std::time::Instant; // Import Instant for timing // Import rand for random nonces

const NUM_SAMPLES: u32 = 50; // Number of times to find a nonce for averaging

// Function to find one valid nonce and return the count
fn find_one_nonce(difficulty: u64, rng: &mut ThreadRng, mining_hash: &[u8; 32]) -> u64 {
    // let mut nonce_u512 = U512::zero(); // Start nonce from 0
    let mut nonce_count: u64 = 0;
    let mut nonce_bytes = [0u8; 64]; // Buffer for nonce bytes

    // Loop until a valid nonce is found
    loop {
        nonce_count += 1;
        // let nonce_bytes = nonce_u512.to_big_endian();
        rng.fill_bytes(&mut nonce_bytes); // Generate random nonce bytes

        if is_valid_nonce(*mining_hash, nonce_bytes, difficulty) {
            //println!("Found nonce: {}", nonce_count);
            let nonce_distance = get_nonce_distance(*mining_hash, nonce_bytes);
            let nonce_difficulty = MAX_DISTANCE - nonce_distance;
            //println!("Nonce Difficulty: {}", nonce_difficulty);
            return nonce_count; // Return the number of attempts
        }

        // nonce_u512 += U512::one();

        if (nonce_count + 1) % (1000) == 0 {
            println!("  Nonce count {}", nonce_count);
        }

        // Basic safety break for extremely low difficulties or potential bugs
        // This limit might need adjustment depending on expected counts
        if nonce_count > difficulty.saturating_mul(100) && difficulty > 0 {
            // e.g., allow 100x expected attempts
            eprintln!(
                "Warning: Exceeded safety limit ({} nonces) for difficulty {}. Skipping.",
                nonce_count, difficulty
            );
            return u64::MAX; // Indicate an issue
        }
        if nonce_count == u64::MAX {
            eprintln!(
                "Warning: Nonce count reached u64::MAX for difficulty {}. Skipping.",
                difficulty
            );
            return u64::MAX; // Indicate an issue
        }
    }
}

fn main() {
    let mut rng = thread_rng(); // Initialize random number generator

    // Define the range of difficulties to test
    // Adjust these values based on your machine speed and desired range
    let difficulties = [
        10_000_000_000,
        46_000_000_000,
        47_000_000_000,
        48_000_000_000,
        49_000_000_000,
        50_000_000_000,
        51_000_000_000,
        52_000_000_000,
        53_000_000_000,
        53_980_000_000,
        54_000_000_000,
        55_000_000_000,
    ];

    println!("Difficulty,AverageNonceCount,TotalTimeSeconds"); // Updated CSV Header

    // Use the real header hash provided
    let header_hex = "e963a26e2f5712d662e5662e6ffd807b93d4a64f3c37861683dd18b922db7805";
    let mining_hash: [u8; 32] = hex::decode(header_hex)
        .expect("Failed to decode header hex")
        .try_into()
        .expect("Decoded hex is not 32 bytes");

    for difficulty in difficulties.iter().cloned() {
        // Clone difficulty for use
        if difficulty == 0 {
            continue;
        } // Skip difficulty 0
        let start_time = Instant::now(); // Start timer for this difficulty

        println!("Measuring difficulty: {}...", difficulty);
        let mut total_nonce_count: u128 = 0;
        let mut successful_samples = 0;

        for i in 0..NUM_SAMPLES {
            // Add some basic progress indication
            // if (i + 1) % (NUM_SAMPLES / 10) == 0 || NUM_SAMPLES <= 10 {
            //      println!("  Sample {}/{}...", i + 1, NUM_SAMPLES);
            // }
            let count = find_one_nonce(difficulty, &mut rng, &mining_hash);
            if count != u64::MAX {
                // Check if safety break was hit
                total_nonce_count += count as u128;
                successful_samples += 1;
            } else {
                eprintln!("  Skipping failed sample for difficulty {}", difficulty);
            }
        }

        let elapsed_time = start_time.elapsed(); // Stop timer for this difficulty

        let time = elapsed_time.as_secs_f64() / successful_samples as f64;

        if successful_samples > 0 {
            let average_nonce_count = total_nonce_count as f64 / successful_samples as f64;
            println!(
                "Difficulty: {}, Average Nonce Count: {:.2}, Average Time: {:.3}",
                difficulty, average_nonce_count, time
            ); // Updated CSV Row
        } else {
            println!("{},NaN,{:.3}", difficulty, time); // Indicate no successful samples but show time
        }
    }
    println!("Measurement complete.");
}
