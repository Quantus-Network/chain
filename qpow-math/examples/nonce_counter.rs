// use primitive_types::U512; // Need U512 for incrementing nonce
use qpow_math::is_valid_nonce;
// use qpow_math::get_nonce_distance;
// use qpow_math::MAX_DISTANCE;
use rand::{thread_rng, RngCore};
use std::time::Instant; 

use rayon::prelude::*; // Import Rayon traits

const NUM_SAMPLES: u32 = 50; // Number of times to find a nonce for averaging

// Function to find one valid nonce and return the count
fn find_one_nonce(difficulty: u64, mining_hash: &[u8; 32]) -> (u64, f64) {
    let mut rng = thread_rng(); // Create RNG inside the function for thread-safety
    // let mut nonce_u512 = U512::zero(); // Start nonce from 0
    let mut nonce_count: u64 = 0;
    let mut nonce_bytes = [0u8; 64]; // Buffer for nonce bytes
    let start_time = Instant::now(); 

    // Loop until a valid nonce is found
    loop {
        nonce_count += 1;

        // linear nonce
        // let nonce_bytes = nonce_u512.to_big_endian();
        // nonce_u512 += U512::one();

        // random nonce
        rng.fill_bytes(&mut nonce_bytes); // Generate random nonce bytes

        if is_valid_nonce(*mining_hash, nonce_bytes, difficulty) {
            //println!("Found nonce: {}", nonce_count);
            // let nonce_distance = get_nonce_distance(*mining_hash, nonce_bytes);
            // let nonce_difficulty = MAX_DISTANCE - nonce_distance;
            //println!("Nonce Difficulty: {}", nonce_difficulty);
            let elapsed_time = start_time.elapsed(); // Stop timer for this difficulty
            let elapsed_secs = elapsed_time.as_secs_f64();
    
            return (nonce_count, elapsed_secs); // Return the number of attempts
        }


        // if (nonce_count + 1) % (1000) == 0 {
        //     println!("  Nonce count {}", nonce_count);
        // }

        // Basic safety break for extremely low difficulties or potential bugs
        // This limit might need adjustment depending on expected counts
        if nonce_count > difficulty.saturating_mul(100) && difficulty > 0 {
            // e.g., allow 100x expected attempts
            eprintln!(
                "Warning: Exceeded safety limit ({} nonces) for difficulty {}. Skipping.",
                nonce_count, difficulty
            );
            return (u64::MAX, 0.0); // Indicate an issue
        }
        if nonce_count == u64::MAX {
            eprintln!(
                "Warning: Nonce count reached u64::MAX for difficulty {}. Skipping.",
                difficulty
            );
            return (u64::MAX, 0.0); // Indicate an issue
        }
    }
}

fn main() {
    // let mut rng = thread_rng(); // Initialize random number generator - removed as it's created per task now

    // Define the range of difficulties to test
    // Adjust these values based on your machine speed and desired range
    let difficulties = [
        40_000_000_000,
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
        55_500_000_000,
        56_000_000_000,
        57_000_000_000,
        58_000_000_000,
    ];

    // Use the real header hash provided
    let header_hex = "e963a26e2f5712d662e5662e6ffd807b93d4a64f3c37861683dd18b922db7805";
    // let header_hex =    "0000000000000000000000000000000000000000000000000000000000000000";

    let mining_hash: [u8; 32] = hex::decode(header_hex)
        .expect("Failed to decode header hex")
        .try_into()
        .expect("Decoded hex is not 32 bytes");

    println!("Mining hash: {:?}", header_hex);

    for difficulty in difficulties.iter().cloned() {
        // Clone difficulty for use
        if difficulty == 0 {
            continue;
        } // Skip difficulty 0
        let start_time = Instant::now(); 
        
        println!("Measuring difficulty: {} ({} samples)...", difficulty, NUM_SAMPLES);
        let mut total_nonce_count: u128 = 0;
        let mut successful_samples = 0;

        // Use Rayon to run samples in parallel
        let counts: Vec<(u64, f64)> = (0..NUM_SAMPLES).into_par_iter().map(|_| {
             find_one_nonce(difficulty, &mining_hash) // Call function for each sample index (no rng passed)
        }).collect();

        let mut total_elapsed_secs: f64 = 0.0;
        // Process the results sequentially
        for count in counts {
            if count.0 != u64::MAX { // Check if safety break was hit
                 total_nonce_count += count.0 as u128;
                 total_elapsed_secs += count.1;
                 successful_samples += 1;
            } else {
                eprintln!("  Skipping failed sample for difficulty {}", difficulty);
            }
        }

        let elapsed_time = start_time.elapsed(); // Stop timer for this difficulty
        let elapsed_secs = elapsed_time.as_secs_f64();

        if successful_samples > 0 {
            let average_nonce_count = total_nonce_count as f64 / successful_samples as f64;
            let average_elapsed_secs = total_elapsed_secs / successful_samples as f64;
            let aggregate_hash_rate = if elapsed_secs > 0.0 {
                total_nonce_count as f64 / elapsed_secs
            } else {
                0.0 // Avoid division by zero if time is negligible
            };
            println!("Difficulty: {}, Average Nonce Count: {:.2}, Avg Time: {:.3} s, Aggregate Hash Rate: {:.2} (solutions/s)", 
                     difficulty, average_nonce_count, average_elapsed_secs, aggregate_hash_rate);
        } else {
             println!("Difficulty: {},NaN,NaN,{:.3},0.0,0", difficulty, elapsed_secs); // Indicate no successful samples
        }
    }
     println!("Measurement complete.");
}
