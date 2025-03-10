use crate::mock::*;
use primitive_types::U512;
use crate::{INITIAL_DIFFICULTY, MAX_DISTANCE};

#[test]
fn test_submit_valid_proof() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let header = [1u8; 32];
        let mut nonce = [0u8; 64];

        // lower difficulty
        let difficulty = 54975581388u64;
        nonce[63] = 4;

        // Submit an invalid proof
        assert!(!QPow::verify_nonce(
            header,
            nonce,
            difficulty
        ));

        nonce[63] = 5;

        // Submit a valid proof
        assert!(QPow::verify_nonce(
            header,
            nonce,
            difficulty
        ));

        assert_eq!(QPow::latest_proof(), Some(nonce));

        // medium difficulty
        let difficulty = 56349970922u64;

        nonce[63] = 13;

        // Submit an invalid proof
        assert!(!QPow::verify_nonce(
            header,
            nonce,
            difficulty
        ));

        nonce[63] = 14;

        // Submit a valid proof
        assert!(QPow::verify_nonce(
            header,
            nonce,
            difficulty
        ));

        assert_eq!(QPow::latest_proof(), Some(nonce));

        // higher difficulty
        let difficulty = 58411555223u64;

        nonce[62] = 0x11;
        nonce[63] = 0xf1;

        // Submit an invalid proof
        assert!(!QPow::verify_nonce(
            header,
            nonce,
            difficulty
        ));

        nonce[62] = 0x11;
        nonce[63] = 0xf2;


        // Submit a valid proof
        assert!(QPow::verify_nonce(
            header,
            nonce,
            difficulty
        ));

        assert_eq!(QPow::latest_proof(), Some(nonce));

        // TODO: debug why this fails
        // Check event was emitted
        // System::assert_has_event(Event::ProofSubmitted {
        //     who,
        //     nonce
        // }.into());
    });
}

#[test]
fn test_submit_invalid_proof() {
    new_test_ext().execute_with(|| {
        let header = [1u8; 32];
        let invalid_nonce = [0u8; 64];  // Invalid nonce
        let difficulty = 64975581388u64;

        // Should fail with invalid nonce
        assert!(
            !QPow::verify_nonce(
                header,
                invalid_nonce,
                difficulty
            )
        );

        let invalid_nonce2 = [2u8; 64];  // Invalid nonce

        // Should fail with invalid nonce
        assert!(
            !QPow::verify_nonce(
                header,
                invalid_nonce2,
                difficulty
            )
        );

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

#[test]
fn test_difficulty_adjustment_boundaries() {
    new_test_ext().execute_with(|| {
        // 1. Test minimum difficulty boundary

        // A. If initial difficulty is already at minimum, it should stay there
        let min_difficulty = INITIAL_DIFFICULTY / 10;
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
