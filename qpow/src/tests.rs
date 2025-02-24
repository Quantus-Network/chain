use crate::QPow;
use primitive_types::U512;
use sp_runtime::testing::ExtrinsicWrapper;
use super::*;
use sp_core::U256 as SpU256;
use sp_core::H256 as SpH256;


fn new_test_ext() -> () {
    // Simple test setup
    ()
}

trait TestExt {
    fn execute_with<F: FnOnce()>(&self, f: F) {
        f()
    }
}

impl TestExt for () {}

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
        let h = [0xfu8; 32];

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

#[test]
fn test_compute_compute() {
    let pre_hash = SpH256::from([1u8; 32]);
    let nonce = 123u64;
    let difficulty = SpU256::from(1000u64);

    let compute = Compute {
        difficulty,
        pre_hash,
        nonce,
    };

    let seal = compute.compute();

    assert_eq!(seal.nonce, nonce);
    assert_eq!(seal.difficulty, difficulty);
    assert_eq!(seal.work.len(), 64);
}

#[test]
fn test_qpow_seal_encode_decode() {
    let seal = QPoWSeal {
        difficulty: SpU256::from(1000),
        work: [1u8; 64],
        nonce: 123u64,
    };

    let encoded = seal.encode();
    let decoded = QPoWSeal::decode(&mut &encoded[..]).unwrap();

    assert_eq!(seal, decoded);
}

#[test]
fn test_minimal_qpow_algorithm_verify() {
    let algorithm = MinimalQPowAlgorithm;
    let pre_hash = SpH256::from([1u8; 32]);
    let difficulty = SpU256::from(2);

    // Create a valid seal
    let compute = Compute {
        difficulty,
        pre_hash,
        nonce: 123u64,
    };
    let seal = compute.compute();
    let raw_seal = seal.encode();

    let block_id: BlockId<sp_runtime::testing::Block<ExtrinsicWrapper<()>>> = BlockId::Number(0);    
    let result = algorithm.verify(
        &block_id,
        &pre_hash,
        None,
        &raw_seal,
        difficulty,
    );

    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_minimal_qpow_algorithm_verify_invalid() {
    let algorithm = MinimalQPowAlgorithm;
    let pre_hash = SpH256::from([1u8; 32]);
    let difficulty = SpU256::from(2);

    // Create an invalid seal
    let invalid_seal = QPoWSeal {
        difficulty,
        work: [0u8; 64], // Invalid work
        nonce: 123u64,
    };
    let raw_seal = invalid_seal.encode();

    // Verify the seal
    let block_id: BlockId<sp_runtime::testing::Block<ExtrinsicWrapper<()>>> = BlockId::Number(0);    

    let result = algorithm.verify(
        &block_id,
        &pre_hash,
        None,
        &raw_seal,
        difficulty,
    );

    assert!(result.is_ok());
    assert!(!result.unwrap());
}