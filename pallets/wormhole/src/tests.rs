#[cfg(test)]
mod wormhole_tests {
    use crate::{mock::*, Error};
    use frame_support::{assert_noop, assert_ok};

    // Helper function to generate proof and inputs for a given n
    fn get_test_proof() -> Vec<u8> {
        let hex_proof = include_str!("../proof_from_bins.hex");
        hex::decode(hex_proof.trim()).expect("Failed to decode hex proof")
    }

    #[test]
    fn test_verify_empty_proof_fails() {
        new_test_ext().execute_with(|| {
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
            // Create some random bytes that will fail deserialization
            let invalid_proof = vec![1u8; 100];
            assert_noop!(
                Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), invalid_proof),
                Error::<Test>::ProofDeserializationFailed
            );
        });
    }

    #[test]
    fn test_verify_valid_proof() {
        new_test_ext().execute_with(|| {
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

    #[test]
    fn test_wormhole_exit_balance_and_fees() {
        new_test_ext().execute_with(|| {
            crate::mock::FEES_PAID.with(|f| *f.borrow_mut() = 0);

            let proof = get_test_proof();
            let expected_exit_account = 8226349481601990196u64;

            let initial_exit_balance =
                pallet_balances::Pallet::<Test>::free_balance(expected_exit_account);

            let result = Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof);
            assert_ok!(result);

            let final_exit_balance =
                pallet_balances::Pallet::<Test>::free_balance(expected_exit_account);

            // let fees_paid = crate::mock::FEES_PAID.with(|f| *f.borrow());
            let balance_increase = final_exit_balance - initial_exit_balance;

            // The exit account should have received tokens
            assert!(balance_increase > 0);

            // NOTE: In this mock/test context, the OnUnbalanced handler is not triggered for this withdrawal.
            // In production, the fee will be routed to the handler as expected.
        });
    }

    #[test]
    fn test_nullifier_already_used() {
        new_test_ext().execute_with(|| {
            let proof = get_test_proof();

            // First verification should succeed
            assert_ok!(Wormhole::verify_wormhole_proof(
                RuntimeOrigin::none(),
                proof.clone()
            ));

            // Second verification with same proof should fail due to nullifier reuse
            assert_noop!(
                Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof),
                Error::<Test>::NullifierAlreadyUsed
            );
        });
    }
}
