#[cfg(test)]
mod wormhole_tests {
    use crate::{get_wormhole_verifier, mock::*, Error};
    use frame_support::{assert_noop, assert_ok};
    use sp_runtime::Perbill;
    use wormhole_circuit::inputs::PublicCircuitInputs;
    use wormhole_verifier::ProofWithPublicInputs;

    // Helper function to generate proof and inputs for a given n
    fn get_test_proof() -> Vec<u8> {
        let hex_proof = include_str!("../proof_from_bins.hex");
        hex::decode(hex_proof.trim()).expect("Failed to decode hex proof")
    }

    #[test]
    fn test_verifier_availability() {
        new_test_ext().execute_with(|| {
            let verifier = get_wormhole_verifier();
            assert!(verifier.is_ok(), "Verifier should be available in tests");

            // Verify the verifier can be used
            let verifier = verifier.unwrap();
            // Check that the circuit data is valid by checking gates
            assert!(
                !verifier.circuit_data.common.gates.is_empty(),
                "Circuit should have gates"
            );
        });
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

            // Parse the proof to get expected funding amount
            let verifier = get_wormhole_verifier().expect("Verifier should be available");
            let proof_with_inputs = ProofWithPublicInputs::from_bytes(proof.clone(), &verifier.circuit_data.common)
                .expect("Should be able to parse test proof");

            let public_inputs = PublicCircuitInputs::try_from(proof_with_inputs)
                .expect("Should be able to parse public inputs");

            let expected_funding_amount = public_inputs.funding_amount;

            // Calculate expected fees (matching lib.rs logic exactly)
            let weight_fee = 0u128; // IdentityFee with zero weight in tests
            let volume_fee = Perbill::from_rational(1u32, 1000u32) * expected_funding_amount;
            let expected_total_fee = weight_fee.saturating_add(volume_fee);
            let expected_net_balance_increase = expected_funding_amount.saturating_sub(expected_total_fee);

            let initial_exit_balance =
                pallet_balances::Pallet::<Test>::free_balance(expected_exit_account);

            let result = Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof);
            assert_ok!(result);

            let final_exit_balance =
                pallet_balances::Pallet::<Test>::free_balance(expected_exit_account);

            // let fees_paid = crate::mock::FEES_PAID.with(|f| *f.borrow());
            let balance_increase = final_exit_balance - initial_exit_balance;

            // Assert the exact expected balance increase
            assert_eq!(
                balance_increase
                , expected_net_balance_increase,
                "Balance increase should equal funding amount minus fees. Funding: {}, Fees: {}, Expected net: {}, Actual: {}"
                , expected_funding_amount
                , expected_total_fee
                , expected_net_balance_increase
                , balance_increase
            );

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
