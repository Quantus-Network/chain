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
	const SCALE_DOWN_FACTOR: u128 = 10_000_000_000; // 10^10;

	fn generate_proof(inputs: CircuitInputs) -> ProofWithPublicInputs<F, C, 2> {
		let config = CircuitConfig::standard_recursion_zk_config();
		let prover = WormholeProver::new(config);
		let prover_next = prover.commit(&inputs).expect("proof failed");
		let proof = prover_next.prove().expect("valid proof");
		proof
	}

	// Ignoring for now, will fix once the no_random feature issue is resolved for test dependencies
	#[test]
	#[ignore]
	fn test_wormhole_transfer_proof_generation() {
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

		let (storage_key, state_root, leaf_hash, event_transfer_count, header) =
			ext.execute_with(|| {
				System::set_block_number(1);

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

				assert_ok!(Balances::mint_into(&alice, funding_amount));
				assert_ok!(Wormhole::transfer_native(
					frame_system::RawOrigin::Signed(alice.clone()).into(),
					unspendable_account_id.clone(),
					funding_amount,
				));

				let event_transfer_count = 0u64;

				let leaf_hash = PoseidonHasher::hash_storage::<crate::TransferProofKey<Test>>(
					&(
						0u32,
						event_transfer_count,
						alice.clone(),
						unspendable_account_id.clone(),
						funding_amount,
					)
						.encode(),
				);

				let proof_address = crate::pallet::TransferProof::<Test>::hashed_key_for(&(
					0u32,
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

		use sp_state_machine::prove_read;
		let proof = prove_read(ext.as_backend(), &[&storage_key])
			.expect("failed to generate storage proof");

		let proof_nodes_vec: Vec<Vec<u8>> = proof.iter_nodes().map(|n| n.to_vec()).collect();

		let processed_storage_proof =
			prepare_proof_for_circuit(proof_nodes_vec, hex::encode(&state_root), leaf_hash)
				.expect("failed to prepare proof for circuit");

		let parent_hash = *header.parent_hash();
		let extrinsics_root = *header.extrinsics_root();
		let digest = header.digest().encode();
		let digest_array: [u8; 110] = digest.try_into().expect("digest should be 110 bytes");
		let block_number: u32 = (*header.number()).try_into().expect("block number fits in u32");

		let block_hash = header.hash();

		let funding_amount_quantized: u32 = (funding_amount / SCALE_DOWN_FACTOR as u128)
			.try_into()
			.expect("funding amount fits in u32 after scaling down");

		let circuit_inputs = CircuitInputs {
			private: PrivateCircuitInputs {
				secret,
				storage_proof: processed_storage_proof,
				transfer_count: event_transfer_count,
				funding_account: BytesDigest::try_from(alice.as_ref() as &[u8])
					.expect("account is 32 bytes"),
				unspendable_account: Digest::from(unspendable_account).into(),
				state_root: BytesDigest::try_from(state_root.as_ref())
					.expect("state root is 32 bytes"),
				extrinsics_root: BytesDigest::try_from(extrinsics_root.as_ref())
					.expect("extrinsics root is 32 bytes"),
				digest: digest_array,
			},
			public: PublicCircuitInputs {
				asset_id: 0u32,
				funding_amount: funding_amount_quantized,
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

		let proof = generate_proof(circuit_inputs);

		let public_inputs =
			PublicCircuitInputs::try_from(&proof).expect("failed to parse public inputs");

		assert_eq!(public_inputs.funding_amount, funding_amount_quantized);
		assert_eq!(
			public_inputs.exit_account,
			BytesDigest::try_from(exit_account_id.as_ref() as &[u8]).unwrap()
		);

		let verifier = get_wormhole_verifier().expect("verifier should be available");
		verifier.verify(proof.clone()).expect("proof should verify");

		let proof_bytes = proof.to_bytes();

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

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

			assert_ok!(Balances::mint_into(&alice, funding_amount));
			assert_ok!(Wormhole::transfer_native(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				unspendable_account_id.clone(),
				funding_amount,
			));

			let block_1_header = System::finalize();

			System::reset_events();
			System::initialize(&2, &block_1_header.hash(), block_1_header.digest());

			let balance_before = Balances::balance(&exit_account_id);
			assert_eq!(balance_before, 0);

			assert_ok!(Wormhole::verify_wormhole_proof(
				frame_system::RawOrigin::None.into(),
				proof_bytes.clone()
			));

			let balance_after = Balances::balance(&exit_account_id);

			assert!(balance_after > 0, "Exit account should have received funds");
			assert!(
				balance_after < funding_amount,
				"Exit account balance should be less than funding amount due to fees"
			);
		});

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let pre_runtime_data = vec![1u8; 32];
			let seal_data = vec![2u8; 64];

			System::deposit_log(DigestItem::PreRuntime(*b"pow_", pre_runtime_data));
			System::deposit_log(DigestItem::Seal(*b"pow_", seal_data));

			let different_header = System::finalize();

			System::reset_events();
			System::initialize(&2, &different_header.hash(), different_header.digest());

			let result = Wormhole::verify_wormhole_proof(
				frame_system::RawOrigin::None.into(),
				proof_bytes.clone(),
			);

			assert!(result.is_err(), "Proof verification should fail with mismatched state");
		});
	}

	#[test]
	fn transfer_native_works() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let amount = 1000u128;

			assert_ok!(Balances::mint_into(&alice, amount * 2));

			let count_before = Wormhole::transfer_count();
			assert_ok!(Wormhole::transfer_native(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				bob.clone(),
				amount,
			));

			assert_eq!(Balances::balance(&alice), amount);
			assert_eq!(Balances::balance(&bob), amount);
			assert_eq!(Wormhole::transfer_count(), count_before + 1);
			assert!(Wormhole::transfer_proof((0u32, count_before, alice, bob, amount)).is_some());
		});
	}

	#[test]
	fn transfer_native_fails_on_self_transfer() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let amount = 1000u128;

			assert_ok!(Balances::mint_into(&alice, amount));

			let result = Wormhole::transfer_native(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				alice.clone(),
				amount,
			);

			assert!(result.is_err());
		});
	}

	#[test]
	fn transfer_asset_works() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let asset_id = 1u32;
			let amount = 1000u128;

			assert_ok!(Balances::mint_into(&alice, 1000));
			assert_ok!(Balances::mint_into(&bob, 1000));

			assert_ok!(Assets::create(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id.into(),
				alice.clone(),
				1,
			));
			assert_ok!(Assets::mint(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id.into(),
				alice.clone(),
				amount * 2,
			));

			let count_before = Wormhole::transfer_count();
			assert_ok!(Wormhole::transfer_asset(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id,
				bob.clone(),
				amount,
			));

			assert_eq!(Assets::balance(asset_id, &alice), amount);
			assert_eq!(Assets::balance(asset_id, &bob), amount);
			assert_eq!(Wormhole::transfer_count(), count_before + 1);
			assert!(
				Wormhole::transfer_proof((asset_id, count_before, alice, bob, amount)).is_some()
			);
		});
	}

	#[test]
	fn transfer_asset_fails_on_nonexistent_asset() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let asset_id = 999u32;
			let amount = 1000u128;

			let result = Wormhole::transfer_asset(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id,
				bob.clone(),
				amount,
			);

			assert!(result.is_err());
		});
	}

	#[test]
	fn transfer_asset_fails_on_self_transfer() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let asset_id = 1u32;
			let amount = 1000u128;

			assert_ok!(Balances::mint_into(&alice, 1000));

			assert_ok!(Assets::create(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id.into(),
				alice.clone(),
				1,
			));
			assert_ok!(Assets::mint(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id.into(),
				alice.clone(),
				amount,
			));

			let result = Wormhole::transfer_asset(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id,
				alice.clone(),
				amount,
			);

			assert!(result.is_err());
		});
	}
}
