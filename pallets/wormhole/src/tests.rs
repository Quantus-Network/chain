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
            // Reset fee tracking
            crate::mock::FEES_PAID.with(|f| *f.borrow_mut() = 0);

            // Get the proof
            let proof = get_test_proof();

            // The exit account decoded from the test proof
            let expected_exit_account = 8226349481601990196u64;

            // Check initial balance of the exit account
            let initial_exit_balance =
                pallet_balances::Pallet::<Test>::free_balance(expected_exit_account);
            println!("Initial exit account balance: {}", initial_exit_balance);

            // Verify the proof
            let result = Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof);
            println!("Proof verification result: {:?}", result);
            assert_ok!(result);

            // Check final balance of the exit account
            let final_exit_balance =
                pallet_balances::Pallet::<Test>::free_balance(expected_exit_account);
            println!("Final exit account balance: {}", final_exit_balance);

            // Check that fees were collected
            let fees_paid = crate::mock::FEES_PAID.with(|f| *f.borrow());
            println!("Fees paid: {}", fees_paid);

            // Verify that tokens were minted to the exit account
            let balance_increase = final_exit_balance - initial_exit_balance;
            println!("Balance increase: {}", balance_increase);

            assert!(
                balance_increase > 0,
                "Exit account should have received tokens"
            );
            assert!(fees_paid > 0, "Fees should have been collected");

            // The balance increase should be the minted amount minus any fees deducted from the exit account
            // Based on the logs, 1000000000 was minted and 1000000 fees were paid
            // The final balance shows the net effect
            assert!(
                final_exit_balance >= fees_paid,
                "Account should have enough balance to pay fees"
            );
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
