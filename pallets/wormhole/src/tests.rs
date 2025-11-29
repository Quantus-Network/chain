#[cfg(test)]
mod wormhole_tests {
	use crate::{get_wormhole_verifier, mock::*};
	use codec::Encode;
	use frame_support::{
		assert_ok,
		traits::fungible::{Inspect, Mutate},
	};
	use plonky2::plonk::circuit_data::CircuitConfig;
	use qp_poseidon::PoseidonHasher;
	use qp_wormhole_circuit::{
		inputs::{CircuitInputs, PrivateCircuitInputs, PublicCircuitInputs},
		nullifier::Nullifier,
	};
	use qp_wormhole_prover::WormholeProver;
	use qp_wormhole_verifier::ProofWithPublicInputs;
	use qp_zk_circuits_common::{
		circuit::{C, F},
		storage_proof::prepare_proof_for_circuit,
		utils::{digest_felts_to_bytes, BytesDigest, Digest},
	};
	use sp_runtime::{traits::Header, DigestItem};

	// Helper function to generate proof and inputs for
	fn generate_proof(inputs: CircuitInputs) -> ProofWithPublicInputs<F, C, 2> {
		let config = CircuitConfig::standard_recursion_config();
		let prover = WormholeProver::new(config);
		let prover_next = prover.commit(&inputs).expect("proof failed");
		let proof = prover_next.prove().expect("valid proof");
		proof
	}

	#[test]
	fn test_wormhole_transfer_proof_generation() {
		// Setup accounts
		let alice = account_id(1);
		let secret: BytesDigest = [1u8; 32].try_into().expect("valid secret");
		let unspendable_account =
			qp_wormhole_circuit::unspendable_account::UnspendableAccount::from_secret(secret)
				.account_id;
		let unspendable_account_bytes_digest = digest_felts_to_bytes(unspendable_account);
		let unspendable_account_bytes: [u8; 32] = unspendable_account_bytes_digest
			.as_ref()
			.try_into()
			.expect("BytesDigest is always 32 bytes");
		let unspendable_account_id = AccountId::new(unspendable_account_bytes);
		let exit_account_id = AccountId::new([42u8; 32]);
		let funding_amount = 1_000_000_000_001u128;

		let mut ext = new_test_ext();

		// Execute the transfer and get the header
		let (storage_key, state_root, leaf_hash, event_transfer_count, header) =
			ext.execute_with(|| {
				System::set_block_number(1);

				// Add dummy digest items to match expected format
				let pre_runtime_data = vec![
					233, 182, 183, 107, 158, 1, 115, 19, 219, 126, 253, 86, 30, 208, 176, 70, 21,
					45, 180, 229, 9, 62, 91, 4, 6, 53, 245, 52, 48, 38, 123, 225,
				];
				let seal_data = vec![
					0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
					0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
					0, 0, 0, 0, 0, 0, 0, 0, 0, 30, 77, 142,
				];

				System::deposit_log(DigestItem::PreRuntime(*b"pow_", pre_runtime_data));
				System::deposit_log(DigestItem::Seal(*b"pow_", seal_data));

				assert_ok!(Balances::mint_into(&unspendable_account_id, funding_amount));
				assert_ok!(Balances::transfer_keep_alive(
					frame_system::RawOrigin::Signed(alice.clone()).into(),
					unspendable_account_id.clone(),
					funding_amount,
				));

				let transfer_count = pallet_balances::TransferCount::<Test>::get();
				let event_transfer_count = transfer_count - 1;

				let leaf_hash = PoseidonHasher::hash_storage::<AccountId>(
					&(
						event_transfer_count,
						alice.clone(),
						unspendable_account_id.clone(),
						funding_amount,
					)
						.encode(),
				);

				let proof_address = pallet_balances::TransferProof::<Test>::hashed_key_for(&(
					event_transfer_count,
					alice.clone(),
					unspendable_account_id.clone(),
					funding_amount,
				));
				let mut storage_key = proof_address;
				storage_key.extend_from_slice(&leaf_hash);

				let header = System::finalize();
				let state_root = *header.state_root();

				(storage_key, state_root, leaf_hash, event_transfer_count, header)
			});

		// Generate a storage proof for the specific storage key
		use sp_state_machine::prove_read;
		let proof = prove_read(ext.as_backend(), &[&storage_key])
			.expect("failed to generate storage proof");

		let proof_nodes_vec: Vec<Vec<u8>> = proof.iter_nodes().map(|n| n.to_vec()).collect();

		// Prepare the storage proof for the circuit
		let processed_storage_proof =
			prepare_proof_for_circuit(proof_nodes_vec, hex::encode(&state_root), leaf_hash)
				.expect("failed to prepare proof for circuit");

		// Build the header components
		let parent_hash = *header.parent_hash();
		let extrinsics_root = *header.extrinsics_root();
		let digest = header.digest().encode();
		let digest_array: [u8; 110] = digest.try_into().expect("digest should be 110 bytes");
		let block_number: u32 = (*header.number()).try_into().expect("block number fits in u32");

		// Compute block hash
		let block_hash = header.hash();

		// Assemble circuit inputs
		let circuit_inputs = CircuitInputs {
			private: PrivateCircuitInputs {
				secret,
				transfer_count: event_transfer_count,
				funding_account: BytesDigest::try_from(alice.as_ref() as &[u8])
					.expect("account is 32 bytes"),
				storage_proof: processed_storage_proof,
				unspendable_account: Digest::from(unspendable_account).into(),
				state_root: BytesDigest::try_from(state_root.as_ref())
					.expect("state root is 32 bytes"),
				extrinsics_root: BytesDigest::try_from(extrinsics_root.as_ref())
					.expect("extrinsics root is 32 bytes"),
				digest: digest_array,
			},
			public: PublicCircuitInputs {
				funding_amount,
				nullifier: Nullifier::from_preimage(secret, event_transfer_count).hash.into(),
				exit_account: BytesDigest::try_from(exit_account_id.as_ref() as &[u8])
					.expect("account is 32 bytes"),
				block_hash: BytesDigest::try_from(block_hash.as_ref())
					.expect("block hash is 32 bytes"),
				parent_hash: BytesDigest::try_from(parent_hash.as_ref())
					.expect("parent hash is 32 bytes"),
				block_number,
			},
		};

		// Generate the ZK proof
		let proof = generate_proof(circuit_inputs);

		// Verify the proof can be parsed
		let public_inputs =
			PublicCircuitInputs::try_from(&proof).expect("failed to parse public inputs");

		// Verify that the public inputs match what we expect
		assert_eq!(public_inputs.funding_amount, funding_amount);
		assert_eq!(
			public_inputs.exit_account,
			BytesDigest::try_from(exit_account_id.as_ref() as &[u8]).unwrap()
		);

		// Verify the proof using the verifier
		let verifier = get_wormhole_verifier().expect("verifier should be available");
		verifier.verify(proof.clone()).expect("proof should verify");

		// Serialize the proof to bytes for extrinsic testing
		let proof_bytes = proof.to_bytes();

		// Now test the extrinsic in a new environment
		new_test_ext().execute_with(|| {
			// Set up the blockchain state to have block 1
			System::set_block_number(1);

			// Add the same digest items
			let pre_runtime_data = vec![
				233, 182, 183, 107, 158, 1, 115, 19, 219, 126, 253, 86, 30, 208, 176, 70, 21, 45,
				180, 229, 9, 62, 91, 4, 6, 53, 245, 52, 48, 38, 123, 225,
			];
			let seal_data = vec![
				0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
				0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
				0, 0, 0, 0, 0, 30, 77, 142,
			];

			System::deposit_log(DigestItem::PreRuntime(*b"pow_", pre_runtime_data));
			System::deposit_log(DigestItem::Seal(*b"pow_", seal_data));

			// Execute the same transfer to recreate the exact state
			assert_ok!(Balances::mint_into(&unspendable_account_id, funding_amount));
			assert_ok!(Balances::transfer_keep_alive(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				unspendable_account_id.clone(),
				funding_amount,
			));

			// Finalize the block to get the same header and store the block hash
			let block_1_header = System::finalize();

			// Initialize block 2 to store block 1's hash
			System::reset_events();
			System::initialize(&2, &block_1_header.hash(), block_1_header.digest());

			// Check exit account balance before verification
			let balance_before = Balances::balance(&exit_account_id);
			assert_eq!(balance_before, 0);

			// Call the verify_wormhole_proof extrinsic
			assert_ok!(Wormhole::verify_wormhole_proof(
				frame_system::RawOrigin::None.into(),
				proof_bytes.clone()
			));

			// Check that the exit account received the funds (minus fees)
			let balance_after = Balances::balance(&exit_account_id);

			// The balance should be funding_amount minus fees
			// Weight fee + 0.1% volume fee
			assert!(balance_after > 0, "Exit account should have received funds");
			assert!(
				balance_after < funding_amount,
				"Exit account balance should be less than funding amount due to fees"
			);
		});

		// Test that proof fails when state doesn't match
		new_test_ext().execute_with(|| {
			// Set up block 1 but DON'T recreate the exact same state
			System::set_block_number(1);

			// Add different digest items with same 110-byte format but different content
			let pre_runtime_data = vec![1u8; 32]; // Different data
			let seal_data = vec![2u8; 64]; // Different data

			System::deposit_log(DigestItem::PreRuntime(*b"pow_", pre_runtime_data));
			System::deposit_log(DigestItem::Seal(*b"pow_", seal_data));

			// Finalize block 1 with different state
			let different_header = System::finalize();

			// Initialize block 2
			System::reset_events();
			System::initialize(&2, &different_header.hash(), different_header.digest());

			// Try to use the proof with the original header (which has different block hash)
			let result = Wormhole::verify_wormhole_proof(
				frame_system::RawOrigin::None.into(),
				proof_bytes.clone(),
			);

			// This should fail because the block hash in the proof doesn't match
			assert!(result.is_err(), "Proof verification should fail with mismatched state");
		});
	}
}
