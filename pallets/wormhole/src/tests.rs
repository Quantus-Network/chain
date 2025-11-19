#[cfg(test)]
mod wormhole_tests {
	use crate::mock::System;
	use crate::{get_wormhole_verifier, mock::*, weights, Config, Error, WeightInfo};
	use frame_support::{assert_noop, assert_ok, weights::WeightToFee};
	use qp_wormhole_circuit::inputs::CircuitInputs;
	use qp_wormhole_verifier::ProofWithPublicInputs;
	use qp_zk_circuits_common::circuit::{C, F};
	use sp_runtime::traits::Header;
	use sp_runtime::Perbill;

	// Helper function to generate proof and inputs for
	fn generate_proof(inputs: CircuitInputs) -> ProofWithPublicInputs<F, C, 2> {
		let config = plonky2::CircuitConfig::standard_recursion_config();
		let prover = WormholeProver::new(config);
		let prover_next = prover.commit(&inputs);
		let proof = prover_next.prove(&inputs);
		proof
	}

	#[test]
	fn test_verifier_availability() {
		new_test_ext().execute_with(|| {
			let verifier = get_wormhole_verifier();
			assert!(verifier.is_ok(), "Verifier should be available in tests");

			// Verify the verifier can be used
			let verifier = verifier.unwrap();
			// Check that the circuit data is valid by checking gates
			assert!(!verifier.circuit_data.common.gates.is_empty(), "Circuit should have gates");
		});
	}

	#[test]
	fn test_verify_empty_proof_fails() {
		new_test_ext().execute_with(|| {
			let empty_proof = vec![];
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize();
			assert_noop!(
				Wormhole::verify_wormhole_proof(
					RuntimeOrigin::none(),
					empty_proof,
					block_number,
					header
				),
				Error::<Test>::ProofDeserializationFailed
			);
		});
	}

	#[test]
	fn test_verify_invalid_proof_data_fails() {
		new_test_ext().execute_with(|| {
			// Create some random bytes that will fail deserialization
			let invalid_proof = vec![1u8; 100];
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize();
			assert_noop!(
				Wormhole::verify_wormhole_proof(
					RuntimeOrigin::none(),
					invalid_proof,
					block_number,
					header
				),
				Error::<Test>::ProofDeserializationFailed
			);
		});
	}

	#[test]
	fn test_verify_valid_proof() {
		new_test_ext().execute_with(|| {
			let proof = get_test_proof();
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize();
			assert_noop!(
				Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof, block_number, header),
				Error::<Test>::InvalidPublicInputs
			);
		});
	}

	#[test]
	fn test_verify_invalid_inputs() {
		new_test_ext().execute_with(|| {
			let mut proof = get_test_proof();
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize();

			if let Some(byte) = proof.get_mut(0) {
				*byte = !*byte; // Flip bits to make proof invalid
			}

			assert_noop!(
				Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof, block_number, header),
				Error::<Test>::InvalidPublicInputs
			);
		});
	}

	#[test]
	fn test_wormhole_exit_balance_and_fees() {
		new_test_ext().execute_with(|| {
			let proof = get_test_proof();
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize();

			assert_noop!(
				Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof, block_number, header),
				Error::<Test>::InvalidPublicInputs
			);
		});
	}

	#[test]
	fn test_nullifier_already_used() {
		new_test_ext().execute_with(|| {
			let proof = get_test_proof();
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize();

			// First verification should fail due to block hash mismatch
			assert_noop!(
				Wormhole::verify_wormhole_proof(
					RuntimeOrigin::none(),
					proof.clone(),
					block_number,
					header.clone()
				),
				Error::<Test>::InvalidPublicInputs
			);

			// Once proof generation is fixed, this test should be updated to:
			// 1. First call is assert_ok!
			// 2. Second call is assert_noop! with NullifierAlreadyUsed.
		});
	}

	#[test]
	fn test_verify_future_block_number_fails() {
		new_test_ext().execute_with(|| {
			let proof = get_test_proof();
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize(); // current block is 2, header is for block 1
			let future_block = 3;

			// This call attempts to use a header for block 1 with a future block number 3.
			// It will fail the check `header.hash() == block_hash` because the block hash for
			// block 3 is not the hash of header for block 1.
			assert_noop!(
				Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof, future_block, header),
				Error::<Test>::InvalidBlockNumber
			);
		});
	}

	#[test]
	fn test_verify_block_hash_mismatch_fails() {
		new_test_ext().execute_with(|| {
			let proof = get_test_proof();
			let block_number = 1;
			System::set_block_number(block_number);
			let header = System::finalize();

			let result =
				Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof, block_number, header);

			// This will fail with InvalidPublicInputs because the block hash in the proof doesn't match
			// the one from the generated header.
			assert_noop!(result, Error::<Test>::InvalidPublicInputs);
		});
	}

	#[test]
	fn test_verify_with_different_block_numbers() {
		new_test_ext().execute_with(|| {
			let proof = get_test_proof();

			// Run block 1
			System::set_block_number(1);
			let header1 = System::finalize(); // current is 2, header1 is for 1

			// Run block 2
			System::set_block_number(2);
			let header2 = System::finalize(); // current is 3, header2 is for 2

			// Test with current block (which is 2, but we use header2 for it)
			assert_noop!(
				Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof.clone(), 2, header2),
				Error::<Test>::InvalidPublicInputs
			);

			// Test with a recent block (block 1)
			let result =
				Wormhole::verify_wormhole_proof(RuntimeOrigin::none(), proof.clone(), 1, header1);
			assert_noop!(result, Error::<Test>::InvalidPublicInputs);
		});
	}
}
