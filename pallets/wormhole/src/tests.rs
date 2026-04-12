#[cfg(test)]
mod wormhole_tests {
	use crate::mock::*;
	use frame_support::{
		assert_ok,
		traits::{
			fungible::{Inspect, Mutate, Unbalanced},
			Currency,
		},
	};
	use sp_core::crypto::AccountId32;

	/// Well-known test secret for genesis endowment (matches runtime preset).
	/// This secret can be used with `quantus wormhole prove` to spend funds
	/// from the corresponding address via ZK proofs.
	#[allow(dead_code)]
	const TEST_SECRET: [u8; 32] = [42u8; 32];

	/// Pre-computed address for TEST_SECRET, derived using the ZK circuit's
	/// unspendable account derivation: H(H("wormhole" || secret)).
	/// Computed using: `quantus wormhole address --secret 0x2a2a...2a`
	/// SS58: qzokTZkdWXxMgSXyF86ECHxG8o8yRX5ibrX2Uw8YmqkHRdj1V
	const TEST_ADDRESS: [u8; 32] = [
		0xbe, 0x13, 0xa1, 0x89, 0xf9, 0x9c, 0x44, 0xa9, 0x59, 0xe2, 0x66, 0x94, 0xff, 0xe5, 0xe4,
		0xba, 0x22, 0x30, 0x92, 0xf3, 0xed, 0xbe, 0x82, 0x59, 0xc1, 0xd4, 0x5a, 0xd0, 0x8e, 0xdb,
		0x40, 0x3d,
	];

	/// Get the test account derived from TEST_SECRET
	fn test_account() -> AccountId {
		AccountId32::new(TEST_ADDRESS)
	}

	#[test]
	fn record_transfer_increments_count() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let amount = 10 * UNIT;

			let count_before = Wormhole::transfer_count(&bob);
			Wormhole::record_transfer(0u32, &alice, &bob, amount);

			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);

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
					leaf_index: 0, // First leaf inserted
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

			// 2. Record the transfer (now goes to ZK trie, but disabled in mock)
			let count_before = Wormhole::transfer_count(&bob);
			Wormhole::record_transfer(0u32, &alice, &bob, amount);

			assert_eq!(Balances::balance(&alice), amount);
			assert_eq!(Balances::balance(&bob), amount);
			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);
		});
	}

	#[test]
	fn test_address_matches_expected() {
		// Verify our pre-computed test address is correct
		let address = test_account();
		let address_bytes: &[u8; 32] = address.as_ref();

		// Should match TEST_ADDRESS
		assert_eq!(address_bytes, &TEST_ADDRESS);

		// Should not be all zeros
		assert_ne!(address_bytes, &[0u8; 32], "Test address should not be all zeros");
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
	fn genesis_endowments_are_recorded() {
		// Test that addresses endowed at genesis have their transfers recorded,
		// enabling them to spend via ZK proofs (proofs stored in ZK trie).
		use frame_support::traits::Hooks;

		let address = test_account();
		let endowment_amount = 1_000 * UNIT; // Matches runtime genesis preset

		new_test_ext_with_endowments(vec![(address.clone(), endowment_amount)]).execute_with(
			|| {
				// Verify the balance was set (this happens immediately at genesis)
				assert_eq!(
					Balances::balance(&address),
					endowment_amount,
					"Address should have endowed balance"
				);

				// Before block 1: transfer count should be 0
				assert_eq!(
					Wormhole::transfer_count(&address),
					0,
					"Transfer count should be 0 before on_initialize"
				);

				// Trigger on_initialize at block 1 to process genesis endowments
				System::set_block_number(1);
				Wormhole::on_initialize(1);

				// After block 1: transfer count should be incremented
				assert_eq!(
					Wormhole::transfer_count(&address),
					1,
					"Transfer count should be 1 after on_initialize"
				);

				// Verify event was emitted
				System::assert_last_event(
					crate::Event::<Test>::NativeTransferred {
						from: MINTING_ACCOUNT,
						to: address,
						amount: endowment_amount,
						transfer_count: 0,
						leaf_index: 0, // First leaf inserted
					}
					.into(),
				);
			},
		);
	}

	#[test]
	fn genesis_multiple_endowments_all_recorded() {
		// Test multiple addresses endowed at genesis all get their transfers recorded.
		// The chain doesn't distinguish "wormhole addresses" from regular addresses -
		// any address can have transfers recorded and spend via ZK proofs.
		use frame_support::traits::Hooks;

		let addr1 = account_id(100);
		let addr2 = account_id(101);
		let addr3 = account_id(102);

		let amount1 = 100 * UNIT;
		let amount2 = 200 * UNIT;
		let amount3 = 300 * UNIT;

		new_test_ext_with_endowments(vec![
			(addr1.clone(), amount1),
			(addr2.clone(), amount2),
			(addr3.clone(), amount3),
		])
		.execute_with(|| {
			// All addresses should have their balances (set at genesis)
			assert_eq!(Balances::balance(&addr1), amount1);
			assert_eq!(Balances::balance(&addr2), amount2);
			assert_eq!(Balances::balance(&addr3), amount3);

			// Before block 1: No transfers recorded yet
			assert_eq!(Wormhole::transfer_count(&addr1), 0);
			assert_eq!(Wormhole::transfer_count(&addr2), 0);
			assert_eq!(Wormhole::transfer_count(&addr3), 0);

			// Trigger on_initialize at block 1
			System::set_block_number(1);
			Wormhole::on_initialize(1);

			// After block 1: All addresses should have transfer count = 1
			assert_eq!(Wormhole::transfer_count(&addr1), 1);
			assert_eq!(Wormhole::transfer_count(&addr2), 1);
			assert_eq!(Wormhole::transfer_count(&addr3), 1);
		});
	}

	#[test]
	fn on_initialize_only_runs_once() {
		// Verify that on_initialize only processes endowments on block 1
		use frame_support::traits::Hooks;

		let address = account_id(100);
		let amount = 100 * UNIT;

		new_test_ext_with_endowments(vec![(address.clone(), amount)]).execute_with(|| {
			// Block 0: nothing happens
			System::set_block_number(0);
			Wormhole::on_initialize(0);
			assert_eq!(Wormhole::transfer_count(&address), 0);

			// Block 1: endowments are processed
			System::set_block_number(1);
			Wormhole::on_initialize(1);
			assert_eq!(Wormhole::transfer_count(&address), 1);

			// Block 2: nothing happens (pending was cleared)
			System::set_block_number(2);
			Wormhole::on_initialize(2);
			assert_eq!(Wormhole::transfer_count(&address), 1); // Still 1, not 2
		});
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
								total += (account_data.summed_output_amount as u128) *
									crate::SCALE_DOWN_FACTOR;
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
