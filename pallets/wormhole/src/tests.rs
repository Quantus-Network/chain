use toml;

#[cfg(test)]
mod tests {
    use std::fs;
    use crate::{mock::*, Error, Event};
    use frame_support::{assert_noop, assert_ok};


    // Helper function to generate proof and inputs for a given n
    fn get_test_proof() -> Vec<u8> {
        include_bytes! ("../proof.hex").to_vec()
    }

    #[test]
    fn test_verify_valid_proof() {
        new_test_ext().execute_with(|| {
            let proof = get_test_proof();
            assert_ok!(Wormhole::verify_wormhole_proof(
                RuntimeOrigin::none(),
                proof
            ));

            // System::assert_has_event(RuntimeEvent::Wormhole(
            //     Event::ProofVerified { public_values: EXPECTED_PUBLIC_INPUTS.to_vec() }
            // ));
        });
    }

    #[test]
    fn test_verify_invalid_inputs() {
        new_test_ext().execute_with(|| {
            let mut proof = get_test_proof();

            if let Some(byte) = proof.get_mut(0) {
                *byte = !*byte; // Flip bits to make proof invalid
            }

            assert_noop!(
                Wormhole::verify_wormhole_proof(
                    RuntimeOrigin::none(),
                    proof,
                ),
                Error::<Test>::VerificationFailed
            );
        });
    }

}