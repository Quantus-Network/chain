use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};
use primitive_types::{U256, U512};

#[test]
fn test_submit_valid_proof() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let header = [1u8; 32];
        let mut solution = [0u8; 64];

        // lower difficulty
        let difficulty = 54975581388u64;
        solution[63] = 4;

        // Submit an invalid proof
        assert!(!QPow::verify_solution(
            header,
            solution,
            difficulty
        ));

        solution[63] = 5;

        // Submit a valid proof
        assert!(QPow::verify_solution(
            header,
            solution,
            difficulty
        ));

        assert_eq!(QPow::latest_proof(), Some(solution));

        // medium difficulty
        let difficulty = 56349970922u64;

        solution[63] = 13;

        // Submit an invalid proof
        assert!(!QPow::verify_solution(
            header,
            solution,
            difficulty
        ));

        solution[63] = 14;

        // Submit a valid proof
        assert!(QPow::verify_solution(
            header,
            solution,
            difficulty
        ));

        assert_eq!(QPow::latest_proof(), Some(solution));

        // higher difficulty
        let difficulty = 58411555223u64;

        solution[62] = 0x11;
        solution[63] = 0xf1;

        // Submit an invalid proof
        assert!(!QPow::verify_solution(
            header,
            solution,
            difficulty
        ));

        solution[62] = 0x11;
        solution[63] = 0xf2;


        // Submit a valid proof
        assert!(QPow::verify_solution(
            header,
            solution,
            difficulty
        ));

        assert_eq!(QPow::latest_proof(), Some(solution));

        // TODO: debug why this fails
        // Check event was emitted
        // System::assert_has_event(Event::ProofSubmitted {
        //     who,
        //     solution
        // }.into());
    });
}

#[test]
fn test_submit_invalid_proof() {
    new_test_ext().execute_with(|| {
        let header = [1u8; 32];
        let invalid_solution = [0u8; 64];  // Invalid solution
        let difficulty = 64975581388u64;

        // Should fail with invalid solution
        assert!(
            !QPow::verify_solution(
                header,
                invalid_solution,
                difficulty
            )
        );

        let invalid_solution2 = [2u8; 64];  // Invalid solution

        // Should fail with invalid solution
        assert!(
            !QPow::verify_solution(
                header,
                invalid_solution2,
                difficulty
            )
        );

    });
}

#[test]
fn test_compute_pow_valid_solution() {
    new_test_ext().execute_with(|| {
        let mut h = [0u8; 32];
        h[31] = 123; // For value 123

        let mut m = [0u8; 32];
        m[31] = 5;   // For value 5

        let mut n = [0u8; 64];
        n[63] = 17;  // For value 17

        let mut solution = [0u8; 64];
        solution[63] = 2; // For value 2

        // Compute the result and the truncated result based on difficulty
        let hash = QPow::hash_to_group(&h, &m, &n, &solution);

        let manual_mod = QPow::mod_pow(
            &U512::from_big_endian(&m),
            &(U512::from_big_endian(&h) + U512::from_big_endian(&solution)),
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
        let mut h = [0xfu8; 32];

        let mut m = [0u8; 32];
        m[31] = 5;   // For value 5

        let mut n = [0u8; 64];
        n[63] = 17;  // For value 17

        let mut solution = [0u8; 64];
        solution[63] = 2; // For value 2

        // Compute the result and the truncated result based on difficulty
        let hash = QPow::hash_to_group(&h, &m, &n, &solution);

        let manual_mod = QPow::mod_pow(
            &U512::from_big_endian(&m),
            &(U512::from_big_endian(&h) + U512::from_big_endian(&solution)),
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

        // Test some known composites
        assert!(!QPow::is_prime(&U512::from(4u32)));
        assert!(!QPow::is_prime(&U512::from(6u32)));
        assert!(!QPow::is_prime(&U512::from(8u32)));
        assert!(!QPow::is_prime(&U512::from(9u32)));
        assert!(!QPow::is_prime(&U512::from(10u32)));
    });
}