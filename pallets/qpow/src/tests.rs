use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};
use primitive_types::{U256, U512};

#[test]
fn test_submit_valid_proof() {
    new_test_ext().execute_with(|| {
        // Set up test data
        let who = 1;
        let header = [1u8; 32];
        let mut solution = [0u8; 64];
        solution[63] = 8; // Set the last byte to 8

        let difficulty = 10;

        // Submit a valid proof
        assert_ok!(QPow::submit_proof(
            RuntimeOrigin::signed(who),
            header,
            solution,
            difficulty
        ));

        // Check that proof was stored
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
        let who = 1;
        let header = [1u8; 32];
        let invalid_solution = [0u8; 64];  // Invalid solution
        let difficulty = 10;

        // Should fail with invalid solution
        assert_noop!(
            QPow::submit_proof(
                RuntimeOrigin::signed(who),
                header,
                invalid_solution,
                difficulty
            ),
            Error::<Test>::InvalidSolution
        );

        let invalid_solution2 = [2u8; 64];  // Invalid solution

        // Should fail with invalid solution
        assert_noop!(
            QPow::submit_proof(
                RuntimeOrigin::signed(who),
                header,
                invalid_solution2,
                difficulty
            ),
            Error::<Test>::InvalidSolution
        );

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