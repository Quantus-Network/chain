#[cfg(test)]
mod wormhole_tests {
	use crate::mock::*;
	use codec::Encode;
	use frame_support::{
		assert_ok,
		traits::{
			fungible::{Inspect, Mutate, Unbalanced},
			Currency,
		},
	};
	use qp_poseidon::PoseidonHasher;
	use qp_wormhole::derive_wormhole_account;
	use sp_core::crypto::AccountId32;

	/// Compute the expected leaf_inputs_hash for a transfer.
	/// This must match the computation in record_transfer.
	fn compute_leaf_inputs_hash(
		asset_id: u32,
		transfer_count: u64,
		from: &AccountId,
		to: &AccountId,
		amount: Balance,
	) -> [u8; 32] {
		let full_data: (u32, u64, AccountId, AccountId, Balance) =
			(asset_id, transfer_count, from.clone(), to.clone(), amount);
		PoseidonHasher::hash_storage::<(u32, u64, AccountId, AccountId, Balance)>(
			&full_data.encode(),
		)
	}

	#[test]
	fn record_transfer_creates_proof_and_increments_count() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let amount = 10 * UNIT;

			let count_before = Wormhole::transfer_count(&bob);
			Wormhole::record_transfer(0u32, &alice, &bob, amount);

			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);

			// Verify the stored hash matches the expected leaf_inputs_hash
			let expected_hash = compute_leaf_inputs_hash(0u32, count_before, &alice, &bob, amount);
			let stored_hash = Wormhole::transfer_proof((bob.clone(), count_before))
				.expect("transfer proof should exist");
			assert_eq!(
				stored_hash, expected_hash,
				"stored hash should match expected leaf_inputs_hash"
			);

			// Second transfer increments count again
			Wormhole::record_transfer(0u32, &alice, &bob, amount);
			assert_eq!(Wormhole::transfer_count(&bob), count_before + 2);
		});
	}

	#[test]
	fn record_transfer_emits_native_transferred_event() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let amount = 10 * UNIT;

			System::set_block_number(1);
			Wormhole::record_transfer(0u32, &alice, &bob, amount);

			System::assert_last_event(
				crate::Event::<Test>::NativeTransferred {
					from: alice,
					to: bob,
					amount,
					transfer_count: 0,
				}
				.into(),
			);
		});
	}

	#[test]
	fn balance_transfer_with_record_transfer_works() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let amount = 10 * UNIT;

			// Fund alice
			assert_ok!(Balances::mint_into(&alice, amount * 2));

			// Simulate what the WormholeProofRecorderExtension does:
			// 1. Transfer via Balances
			assert_ok!(<Balances as Mutate<_>>::transfer(
				&alice,
				&bob,
				amount,
				frame_support::traits::tokens::Preservation::Expendable,
			));

			// 2. Record the transfer proof
			let count_before = Wormhole::transfer_count(&bob);
			Wormhole::record_transfer(0u32, &alice, &bob, amount);

			assert_eq!(Balances::balance(&alice), amount);
			assert_eq!(Balances::balance(&bob), amount);
			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);

			// Verify the stored hash matches the expected leaf_inputs_hash
			let expected_hash = compute_leaf_inputs_hash(0u32, count_before, &alice, &bob, amount);
			let stored_hash =
				Wormhole::transfer_proof((bob, count_before)).expect("transfer proof should exist");
			assert_eq!(
				stored_hash, expected_hash,
				"stored hash should match expected leaf_inputs_hash"
			);
		});
	}

	#[test]
	fn known_preimage_to_wormhole_address_all_zeros() {
		// Test vector: all zeros preimage
		let preimage = [0u8; 32];
		let address = derive_wormhole_account(preimage);

		// This is the expected wormhole address for preimage [0; 32]
		// Computed via: qp_wormhole::derive_wormhole_account -> rehash_to_bytes(preimage)
		let expected_bytes: [u8; 32] = [
			0xca, 0x0a, 0xef, 0xbd, 0x2e, 0x87, 0xc9, 0xec, 0xc4, 0x71, 0x6b, 0x7d, 0xb8, 0xe8,
			0x39, 0x37, 0xa4, 0x5d, 0xfb, 0x06, 0xea, 0x10, 0xe1, 0xd6, 0x2a, 0x4c, 0x2f, 0x27,
			0x84, 0x00, 0x22, 0x90,
		];
		let expected = AccountId32::from(expected_bytes);

		assert_eq!(
			address, expected,
			"Wormhole address for all-zeros preimage should match known value"
		);
	}

	#[test]
	fn known_preimage_to_wormhole_address_all_ones() {
		// Test vector: all ones preimage
		let preimage = [1u8; 32];
		let address = derive_wormhole_account(preimage);

		// This is the expected wormhole address for preimage [1; 32]
		// Computed via: qp_wormhole::derive_wormhole_account -> rehash_to_bytes(preimage)
		let expected_bytes: [u8; 32] = [
			0x2d, 0x45, 0x48, 0xaf, 0xca, 0xc3, 0x11, 0xcd, 0xb8, 0x47, 0xe9, 0xf3, 0x9e, 0x4d,
			0x52, 0x55, 0xbf, 0x74, 0x6e, 0xdd, 0xd8, 0x6b, 0x71, 0x40, 0x32, 0xd9, 0x2d, 0x6c,
			0x0e, 0xd7, 0x08, 0xd1,
		];
		let expected = AccountId32::from(expected_bytes);

		assert_eq!(
			address, expected,
			"Wormhole address for all-ones preimage should match known value"
		);
	}

	#[test]
	fn known_preimage_to_wormhole_address_sequential() {
		// Test vector: sequential bytes 0..31
		let preimage: [u8; 32] = core::array::from_fn(|i| i as u8);
		let address = derive_wormhole_account(preimage);

		// This is the expected wormhole address for preimage [0, 1, 2, ..., 31]
		// Computed via: qp_wormhole::derive_wormhole_account -> rehash_to_bytes(preimage)
		let expected_bytes: [u8; 32] = [
			0xc0, 0x87, 0x74, 0x70, 0xeb, 0x2f, 0xbf, 0xc7, 0xcc, 0x6f, 0x22, 0xab, 0x70, 0x95,
			0x55, 0x09, 0xde, 0x54, 0xf3, 0xb8, 0x98, 0x56, 0xd6, 0xa5, 0x83, 0x99, 0xa7, 0xb7,
			0xd9, 0xd2, 0x62, 0x52,
		];
		let expected = AccountId32::from(expected_bytes);

		assert_eq!(
			address, expected,
			"Wormhole address for sequential preimage should match known value"
		);
	}

	#[test]
	fn preimage_to_wormhole_address_is_deterministic() {
		// Same preimage should always produce the same address
		let preimage = [42u8; 32];

		let address1 = derive_wormhole_account(preimage);
		let address2 = derive_wormhole_account(preimage);

		assert_eq!(address1, address2, "Same preimage should produce same wormhole address");
	}

	#[test]
	fn set_total_issuance_reduces_supply() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let initial_mint = 1000 * UNIT;
			let burn_amount = 100 * UNIT;

			assert_ok!(Balances::mint_into(&alice, initial_mint));
			let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

			let current = <Balances as Inspect<AccountId>>::total_issuance();
			<Balances as Unbalanced<AccountId>>::set_total_issuance(
				current.saturating_sub(burn_amount),
			);

			let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(issuance_after, issuance_before - burn_amount);
		});
	}

	#[test]
	fn currency_burn_drop_is_noop_regression() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let initial_mint = 1000 * UNIT;
			let burn_amount = 100 * UNIT;

			assert_ok!(Balances::mint_into(&alice, initial_mint));
			let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

			let _ = <Balances as Currency<AccountId>>::burn(burn_amount);

			let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(
				issuance_after, issuance_before,
				"Currency::burn + drop should be a no-op (PositiveImbalance re-adds on drop)"
			);
		});
	}

	#[test]
	fn different_preimages_produce_different_addresses() {
		let preimage1 = [1u8; 32];
		let preimage2 = [2u8; 32];

		let address1 = derive_wormhole_account(preimage1);
		let address2 = derive_wormhole_account(preimage2);

		assert_ne!(
			address1, address2,
			"Different preimages should produce different wormhole addresses"
		);
	}
}

/// Tests for aggregated proof verification
#[cfg(test)]
mod aggregated_proof_tests {
	use crate::{
		mock::*,
		pallet::{Error, UsedNullifiers},
	};
	use frame_support::{assert_noop, assert_ok};
	use frame_system::RawOrigin;
	use qp_wormhole_verifier::{parse_aggregated_public_inputs, ProofWithPublicInputs, C, F};
	use sp_core::H256;

	/// The D const parameter for plonky2 proofs (extension degree = 2)
	const D: usize = 2;

	/// Real aggregated proof for testing (hex-encoded).
	/// Generated using: `quantus wormhole multi round`
	const AGGREGATED_PROOF_HEX: &str = include_str!("../test-data/aggregated.hex");

	/// Helper to decode the test proof
	fn get_test_proof_bytes() -> Vec<u8> {
		hex::decode(AGGREGATED_PROOF_HEX.trim()).expect("Invalid hex in test proof")
	}

	/// Helper to deserialize the test proof
	fn deserialize_test_proof() -> ProofWithPublicInputs<F, C, D> {
		let proof_bytes = get_test_proof_bytes();
		let verifier = crate::get_aggregated_verifier().expect("Verifier should be available");
		ProofWithPublicInputs::<F, C, D>::from_bytes(proof_bytes, &verifier.circuit_data.common)
			.expect("Proof should deserialize")
	}

	#[test]
	fn test_proof_deserialization_succeeds() {
		// Just test that the proof deserializes correctly
		let proof = deserialize_test_proof();
		assert!(!proof.public_inputs.is_empty(), "Proof should have public inputs");
	}

	#[test]
	fn test_parse_aggregated_public_inputs_succeeds() {
		let proof = deserialize_test_proof();
		let inputs = parse_aggregated_public_inputs(&proof).expect("Should parse public inputs");

		// Verify basic structure
		assert_eq!(inputs.asset_id, 0, "Asset ID should be native (0)");
		assert_eq!(inputs.volume_fee_bps, 10, "Volume fee should be 10 bps");
		assert!(!inputs.nullifiers.is_empty(), "Should have nullifiers");
		assert!(!inputs.account_data.is_empty(), "Should have account data");

		println!("Parsed public inputs:");
		println!("  asset_id: {}", inputs.asset_id);
		println!("  volume_fee_bps: {}", inputs.volume_fee_bps);
		println!("  block_number: {}", inputs.block_data.block_number);
		println!("  block_hash: {:?}", inputs.block_data.block_hash);
		println!("  num_nullifiers: {}", inputs.nullifiers.len());
		println!("  num_accounts: {}", inputs.account_data.len());
	}

	#[test]
	fn test_verify_aggregated_proof_fails_with_wrong_origin() {
		new_test_ext().execute_with(|| {
			let proof_bytes = get_test_proof_bytes();

			// Should fail with signed origin (must be unsigned)
			assert_noop!(
				Wormhole::verify_aggregated_proof(
					RawOrigin::Signed(account_id(1)).into(),
					proof_bytes
				),
				sp_runtime::DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn test_verify_aggregated_proof_fails_with_invalid_bytes() {
		new_test_ext().execute_with(|| {
			// Random invalid bytes should fail deserialization
			let invalid_bytes = vec![0u8; 100];

			let result = Wormhole::verify_aggregated_proof(RawOrigin::None.into(), invalid_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::AggregatedProofDeserializationFailed.into());
		});
	}

	#[test]
	fn test_verify_aggregated_proof_fails_with_block_not_found() {
		new_test_ext().execute_with(|| {
			let proof_bytes = get_test_proof_bytes();

			// The proof references a block that doesn't exist in our mock
			// This should fail with BlockNotFound
			let result = Wormhole::verify_aggregated_proof(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::BlockNotFound.into());
		});
	}

	#[test]
	fn test_verify_aggregated_proof_fails_with_nullifier_already_used() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_aggregated_public_inputs(&proof).expect("Should parse");

			// Set up block hash to match the proof
			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			let block_hash = H256::from(block_hash_bytes);

			// Insert a matching block hash
			frame_system::BlockHash::<Test>::insert(block_number, block_hash);

			// Mark one of the nullifiers as already used
			if let Some(nullifier) = inputs.nullifiers.first() {
				let nullifier_bytes: [u8; 32] = nullifier.as_ref().try_into().unwrap();
				UsedNullifiers::<Test>::insert(nullifier_bytes, true);
			}

			let proof_bytes = get_test_proof_bytes();

			let result = Wormhole::verify_aggregated_proof(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::NullifierAlreadyUsed.into());
		});
	}

	#[test]
	fn test_verify_aggregated_proof_fails_with_wrong_block_hash() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_aggregated_public_inputs(&proof).expect("Should parse");

			// Set up a block at the right number but with wrong hash
			let block_number = inputs.block_data.block_number as u64;
			let wrong_hash = H256::from([0xABu8; 32]); // Wrong hash

			frame_system::BlockHash::<Test>::insert(block_number, wrong_hash);

			let proof_bytes = get_test_proof_bytes();

			let result = Wormhole::verify_aggregated_proof(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::InvalidPublicInputs.into());
		});
	}

	#[test]
	fn test_verify_aggregated_proof_succeeds_with_valid_state() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_aggregated_public_inputs(&proof).expect("Should parse");

			// Set up block hash to match the proof
			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			let block_hash = H256::from(block_hash_bytes);

			frame_system::BlockHash::<Test>::insert(block_number, block_hash);

			// Set current block number higher than the proof's block
			System::set_block_number(block_number + 10);

			let proof_bytes = get_test_proof_bytes();

			// This should succeed - proof is valid and state matches
			assert_ok!(Wormhole::verify_aggregated_proof(RawOrigin::None.into(), proof_bytes));

			// Verify nullifiers are now marked as used
			for nullifier in &inputs.nullifiers {
				let nullifier_bytes: [u8; 32] = nullifier.as_ref().try_into().unwrap();
				assert!(
					UsedNullifiers::<Test>::contains_key(nullifier_bytes),
					"Nullifier should be marked as used"
				);
			}

			// Verify event was emitted
			System::assert_has_event(
				crate::Event::<Test>::ProofVerified {
					exit_amount: {
						// Calculate expected exit amount from public inputs
						let mut total = 0u128;
						for account_data in &inputs.account_data {
							if account_data.summed_output_amount > 0 {
								total += (account_data.summed_output_amount as u128)
									* crate::SCALE_DOWN_FACTOR;
							}
						}
						total
					},
					nullifiers: inputs
						.nullifiers
						.iter()
						.map(|n| n.as_ref().try_into().unwrap())
						.collect(),
				}
				.into(),
			);
		});
	}

	#[test]
	fn test_verify_aggregated_proof_cannot_replay() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_aggregated_public_inputs(&proof).expect("Should parse");

			// Set up block hash to match the proof
			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			let block_hash = H256::from(block_hash_bytes);

			frame_system::BlockHash::<Test>::insert(block_number, block_hash);
			System::set_block_number(block_number + 10);

			let proof_bytes = get_test_proof_bytes();

			// First submission should succeed
			assert_ok!(Wormhole::verify_aggregated_proof(
				RawOrigin::None.into(),
				proof_bytes.clone()
			));

			// Second submission with same proof should fail (nullifiers already used)
			let result = Wormhole::verify_aggregated_proof(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::NullifierAlreadyUsed.into());
		});
	}
}
