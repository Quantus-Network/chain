#[cfg(test)]
mod wormhole_tests {
    use crate::{mock::*, Error};
    use frame_support::{assert_noop, assert_ok};
    use hex;

    // Helper function to generate proof and inputs for a given n
    fn get_test_proof() -> Vec<u8> {
        let hex_proof = include_str!("../proof_from_bins.hex");
        hex::decode(hex_proof.trim()).expect("Failed to decode hex proof")
    }

    fn get_verifier_data() -> Vec<u8> {
        include_bytes!("../verifier.bin").to_vec()
    }

    fn get_common_data() -> Vec<u8> {
        include_bytes!("../common.bin").to_vec()
    }

    fn initialize() {
        let verifier_data = get_verifier_data();
        let common_data = get_common_data();
        assert_ok!(Wormhole::initialize_verifier(
            RuntimeOrigin::root(),
            verifier_data,
            common_data
        ));
    }

    #[test]
    fn test_initialize_verifier() {
        new_test_ext().execute_with(|| {
            initialize();
            // Verify that the verifier data is set
            assert!(!crate::VerifierData::<Test>::get().is_empty());
            assert!(!crate::CommonData::<Test>::get().is_empty());
        });
    }

    #[test]
    fn test_initialize_verifier_twice_fails() {
        new_test_ext().execute_with(|| {
            initialize();

            // Try to initialize again - should fail
            let verifier_data = get_verifier_data();
            let common_data = get_common_data();
            assert_noop!(
                Wormhole::initialize_verifier(RuntimeOrigin::root(), verifier_data, common_data),
                Error::<Test>::AlreadyInitialized
            );
        });
    }

    #[test]
    fn test_verify_empty_proof_fails() {
        new_test_ext().execute_with(|| {
            initialize();
            let empty_proof = vec![];
            assert_noop!(
                Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), empty_proof),
                Error::<Test>::ProofDeserializationFailed
            );
        });
    }

    #[test]
    fn test_verify_invalid_proof_data_fails() {
        new_test_ext().execute_with(|| {
            initialize();
            // Create some random bytes that will fail deserialization
            let invalid_proof = vec![1u8; 100];
            assert_noop!(
                Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), invalid_proof),
                Error::<Test>::ProofDeserializationFailed
            );
        });
    }

    #[test]
    fn test_verify_not_initialized_fails() {
        new_test_ext().execute_with(|| {
            // Don't initialize the verifier
            let proof = vec![1u8; 100];
            assert_noop!(
                Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof),
                Error::<Test>::NotInitialized
            );
        });
    }

    #[test]
    fn test_verify_valid_proof() {
        new_test_ext().execute_with(|| {
            initialize();
            let proof = get_test_proof();
            assert_ok!(Wormhole::verify_wormhole_proof(
                RuntimeOrigin::none(),
                proof
            ));
        });
    }

    #[test]
    fn test_verify_invalid_inputs() {
        new_test_ext().execute_with(|| {
            initialize();
            let mut proof = get_test_proof();

            if let Some(byte) = proof.get_mut(0) {
                *byte = !*byte; // Flip bits to make proof invalid
            }

            assert_noop!(
                Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof,),
                Error::<Test>::VerificationFailed
            );
        });
    }
}
