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

	// =========================================================================
	// Soundness counter tracking
	// =========================================================================

	#[test]
	fn record_transfer_to_ambiguous_address_increases_potential_balance() {
		new_test_ext().execute_with(|| {
			let from = account_id(1);
			let to = account_id(2);
			let amount = 25 * UNIT;

			// `to` has never signed (nonce == 0), so it's ambiguous.
			assert_eq!(Wormhole::potential_wormhole_balance(), 0);
			Wormhole::record_transfer(0u32, &from, &to, amount);
			assert_eq!(Wormhole::potential_wormhole_balance(), amount);

			// A second deposit to another ambiguous address accumulates.
			let to2 = account_id(3);
			Wormhole::record_transfer(0u32, &from, &to2, amount);
			assert_eq!(Wormhole::potential_wormhole_balance(), amount * 2);
		});
	}

	#[test]
	fn record_transfer_to_revealed_address_does_not_change_potential_balance() {
		new_test_ext().execute_with(|| {
			let from = account_id(1);
			let to = account_id(2);
			let amount = 25 * UNIT;

			// Reveal `to` by bumping its nonce above zero.
			frame_system::Pallet::<Test>::inc_account_nonce(&to);

			Wormhole::record_transfer(0u32, &from, &to, amount);
			assert_eq!(
				Wormhole::potential_wormhole_balance(),
				0,
				"Transfers to revealed (nonce > 0) addresses must not add to the pool"
			);
		});
	}

	#[test]
	fn record_transfer_to_non_wormhole_account_does_not_change_potential_balance() {
		new_test_ext().execute_with(|| {
			let from = account_id(1);
			let to = crate::mock::excluded_account();
			let amount = 25 * UNIT;

			// `to` has nonce == 0 but is in the NonWormholeAccounts set (e.g. a multisig or known
			// keyless account), so it must not be treated as an ambiguous wormhole deposit.
			assert!(!Wormhole::is_ambiguous_account(&to));
			Wormhole::record_transfer(0u32, &from, &to, amount);
			assert_eq!(
				Wormhole::potential_wormhole_balance(),
				0,
				"Transfers to excluded (non-wormhole) addresses must not add to the pool"
			);
		});
	}

	#[test]
	fn reveal_account_subtracts_free_balance_from_potential_balance() {
		new_test_ext_with_endowments(vec![(account_id(7), 500 * UNIT)]).execute_with(|| {
			let revealed = account_id(7);
			let seeded = 1_000 * UNIT;
			crate::PotentialWormholeBalance::<Test>::put(seeded);

			// Mirrors funds being sent to a pre-computed multisig address before creation: when
			// the address is later revealed, its balance is removed from the pool.
			Wormhole::reveal_account(&revealed);
			assert_eq!(
				Wormhole::potential_wormhole_balance(),
				seeded - 500 * UNIT,
				"reveal_account must subtract the account's free balance from the pool"
			);

			// Idempotent against over-subtraction: revealing an empty account is a no-op.
			let before = Wormhole::potential_wormhole_balance();
			Wormhole::reveal_account(&account_id(99));
			assert_eq!(Wormhole::potential_wormhole_balance(), before);
		});
	}

	#[test]
	fn migration_seeds_potential_balance_to_total_issuance() {
		use frame_support::traits::UncheckedOnRuntimeUpgrade;

		new_test_ext().execute_with(|| {
			// The migration seeds the pool to current total issuance, a safe upper bound on the
			// value that could be sitting in ambiguous addresses.
			let alice = account_id(1);
			let minted = 750 * UNIT;
			assert_ok!(Balances::mint_into(&alice, minted));

			let issuance = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(issuance, minted);
			assert_eq!(Wormhole::potential_wormhole_balance(), 0);

			crate::migrations::v1::InitSoundnessCounters::<Test>::on_runtime_upgrade();

			assert_eq!(
				Wormhole::potential_wormhole_balance(),
				issuance,
				"Migration must seed PotentialWormholeBalance to total issuance"
			);
		});
	}
}

/// Tests for private-batch proof verification
#[cfg(test)]
mod private_batch_proof_tests {
	use crate::{
		mock::*,
		pallet::{Error, PotentialWormholeBalance, TotalWormholeExits, UsedNullifiers},
	};
	use frame_support::{assert_noop, assert_ok};
	use frame_system::RawOrigin;
	use qp_wormhole_verifier::{parse_private_batch_public_inputs, ProofWithPublicInputs, C, F};
	use sp_core::H256;

	/// The D const parameter for plonky2 proofs (extension degree = 2)
	const D: usize = 2;

	/// Real private-batch proof for testing (hex-encoded).
	/// Generated using: `quantus wormhole multi round`
	const PRIVATE_BATCH_PROOF_HEX: &str = include_str!("../test-data/private_batch.hex");

	/// Helper to decode the test proof
	fn get_test_proof_bytes() -> Vec<u8> {
		hex::decode(PRIVATE_BATCH_PROOF_HEX.trim()).expect("Invalid hex in test proof")
	}

	/// Helper to deserialize the test proof
	fn deserialize_test_proof() -> ProofWithPublicInputs<F, C, D> {
		let proof_bytes = get_test_proof_bytes();
		let verifier = crate::get_private_batch_verifier().expect("Verifier should be available");
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
	fn test_parse_private_batch_public_inputs_succeeds() {
		let proof = deserialize_test_proof();
		let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse public inputs");

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
	fn test_verify_private_batch_fails_with_wrong_origin() {
		new_test_ext().execute_with(|| {
			let proof_bytes = get_test_proof_bytes();

			// Should fail with signed origin (must be unsigned)
			assert_noop!(
				Wormhole::verify_private_batch(
					RawOrigin::Signed(account_id(1)).into(),
					proof_bytes
				),
				sp_runtime::DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn test_verify_private_batch_fails_with_invalid_bytes() {
		new_test_ext().execute_with(|| {
			// Random invalid bytes should fail deserialization
			let invalid_bytes = vec![0u8; 100];

			let result = Wormhole::verify_private_batch(RawOrigin::None.into(), invalid_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::ProofDeserializationFailed.into());
		});
	}

	#[test]
	fn test_verify_private_batch_fails_with_block_not_found() {
		new_test_ext().execute_with(|| {
			let proof_bytes = get_test_proof_bytes();

			// The proof references a block that doesn't exist in our mock
			// This should fail with BlockNotFound
			let result = Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::BlockNotFound.into());
		});
	}

	#[test]
	fn test_verify_private_batch_fails_with_nullifier_already_used() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");

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

			let result = Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::NullifierAlreadyUsed.into());
		});
	}

	#[test]
	fn test_verify_private_batch_fails_with_wrong_block_hash() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");

			// Set up a block at the right number but with wrong hash
			let block_number = inputs.block_data.block_number as u64;
			let wrong_hash = H256::from([0xABu8; 32]); // Wrong hash

			frame_system::BlockHash::<Test>::insert(block_number, wrong_hash);

			let proof_bytes = get_test_proof_bytes();

			let result = Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::InvalidPublicInputs.into());
		});
	}

	#[test]
	fn test_verify_private_batch_succeeds_with_valid_state() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");

			// Set up block hash to match the proof
			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			let block_hash = H256::from(block_hash_bytes);

			frame_system::BlockHash::<Test>::insert(block_number, block_hash);

			// Set current block number higher than the proof's block
			System::set_block_number(block_number + 10);

			// Seed the soundness pool so the exit doesn't trip the invariant.
			PotentialWormholeBalance::<Test>::put(1_000_000 * UNIT);

			let proof_bytes = get_test_proof_bytes();

			// Expected exit total from the proof's public inputs.
			let expected_exit: u128 = inputs
				.account_data
				.iter()
				.filter(|a| a.summed_output_amount > 0)
				.map(|a| (a.summed_output_amount as u128) * crate::SCALE_DOWN_FACTOR)
				.sum();

			// This should succeed - proof is valid and state matches
			assert_ok!(Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes));

			// TotalWormholeExits should now reflect the exit amount.
			assert_eq!(TotalWormholeExits::<Test>::get(), expected_exit);

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
	fn exit_to_ambiguous_address_rejected_when_margin_exhausted() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");

			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			frame_system::BlockHash::<Test>::insert(block_number, H256::from(block_hash_bytes));
			System::set_block_number(block_number + 10);

			let expected_exit: u128 = inputs
				.account_data
				.iter()
				.filter(|a| a.summed_output_amount > 0)
				.map(|a| (a.summed_output_amount as u128) * crate::SCALE_DOWN_FACTOR)
				.sum();

			// Margin is exactly zero: the pool has already been fully consumed by prior exits
			// (potential_balance == total_exits). Exiting into another wormhole (an ambiguous
			// address) is itself valid, but it must NOT be possible when there is no margin left,
			// regardless of where the exit lands. The exit account in the fixture is ambiguous.
			PotentialWormholeBalance::<Test>::put(expected_exit);
			TotalWormholeExits::<Test>::put(expected_exit);

			let proof_bytes = get_test_proof_bytes();
			let result = Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes);

			assert!(result.is_err());
			assert_eq!(
				result.unwrap_err().error,
				Error::<Test>::SoundnessInvariantViolation.into(),
				"exit must be rejected when margin is zero, even to an ambiguous address"
			);

			// State is unchanged: no tokens minted, no counters moved.
			assert_eq!(TotalWormholeExits::<Test>::get(), expected_exit);
			assert_eq!(PotentialWormholeBalance::<Test>::get(), expected_exit);
		});
	}

	#[test]
	fn test_verify_private_batch_cannot_replay() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");

			// Set up block hash to match the proof
			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			let block_hash = H256::from(block_hash_bytes);

			frame_system::BlockHash::<Test>::insert(block_number, block_hash);
			System::set_block_number(block_number + 10);

			// Seed the soundness pool so the exit doesn't trip the invariant.
			PotentialWormholeBalance::<Test>::put(1_000_000 * UNIT);

			let proof_bytes = get_test_proof_bytes();

			// First submission should succeed
			assert_ok!(Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes.clone()));

			// Second submission with same proof should fail (nullifiers already used)
			let result = Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::NullifierAlreadyUsed.into());
		});
	}

	#[test]
	fn test_verify_private_batch_fails_when_soundness_invariant_violated() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");

			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			let block_hash = H256::from(block_hash_bytes);

			frame_system::BlockHash::<Test>::insert(block_number, block_hash);
			System::set_block_number(block_number + 10);

			// PotentialWormholeBalance defaults to 0, so any exit must be rejected: the proof is
			// valid but there is no recorded deposit backing it.
			let proof_bytes = get_test_proof_bytes();
			let result = Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes);
			assert!(result.is_err());
			assert_eq!(
				result.unwrap_err().error,
				Error::<Test>::SoundnessInvariantViolation.into()
			);

			// Nothing should have been exited.
			assert_eq!(TotalWormholeExits::<Test>::get(), 0);
		});
	}

	/// Regenerate the test fixture when circuit parameters change (e.g., num_leaf_proofs).
	///
	/// Run with: cargo test -p pallet-wormhole --lib -- regenerate_test_fixture --nocapture
	/// --ignored
	///
	/// This generates a valid private-batch proof with proper block header validation.
	/// The proof uses well-known test inputs that match the test-helpers constants.
	#[test]
	#[ignore]
	fn regenerate_test_fixture() {
		use std::path::Path;

		// Use a temp directory for circuit binaries
		let tmp_dir = std::env::temp_dir().join("pallet-wormhole-fixture-gen");
		std::fs::create_dir_all(&tmp_dir).expect("Failed to create temp dir");

		// Generate circuit binaries with num_leaf_proofs=7 (matching DEFAULT)
		let num_leaf_proofs = 7usize;
		println!("Generating circuit binaries with num_leaf_proofs={}...", num_leaf_proofs);
		qp_wormhole_circuit_builder::generate_all_circuit_binaries(
			&tmp_dir,
			true,
			num_leaf_proofs,
			None,
		)
		.expect("Failed to generate circuit binaries");

		let aggregated_proof = super::fixture_gen::build_test_private_batch_proof(&tmp_dir);

		// Serialize to hex
		let proof_bytes = aggregated_proof.to_bytes();
		let proof_hex = hex::encode(&proof_bytes);

		// Write to test-data
		let fixture_path =
			Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/private_batch.hex");
		std::fs::write(&fixture_path, &proof_hex).expect("Failed to write fixture");

		println!("Fixture written to: {}", fixture_path.display());
		println!("Proof size: {} bytes ({} hex chars)", proof_bytes.len(), proof_hex.len());

		// Cleanup temp dir
		let _ = std::fs::remove_dir_all(&tmp_dir);
	}
}

/// Shared fixture-generation helpers, used only by the ignored `regenerate_*` tests.
#[cfg(test)]
mod fixture_gen {
	use std::path::Path;

	type Proof = plonky2::plonk::proof::ProofWithPublicInputs<
		qp_zk_circuits_common::circuit::F,
		qp_zk_circuits_common::circuit::C,
		2,
	>;

	/// Build a valid private-batch proof (1 real leaf, dummy-padded) from circuit
	/// binaries in `bins_dir`. Uses the well-known test inputs matching test-helpers.
	pub fn build_test_private_batch_proof(bins_dir: &Path) -> Proof {
		use qp_wormhole_aggregator::aggregator::{AggregationBackend, PrivateBatchAggregator};
		use qp_wormhole_circuit::{
			block_header::header::HeaderInputs,
			inputs::{CircuitInputs, PrivateCircuitInputs},
			nullifier::Nullifier,
			unspendable_account::UnspendableAccount,
		};
		use qp_wormhole_inputs::{BytesDigest, PublicCircuitInputs};
		use qp_wormhole_prover::WormholeProver;
		use qp_zk_circuits_common::utils::digest_to_bytes;

		// Create test inputs with real block header validation
		let secret: BytesDigest = BytesDigest::new_unchecked([42u8; 32]); // Well-known test secret
		let transfer_count = 1u64;
		// Use amounts above minimum (10 UNIT = 1000 quantized)
		// input_amount = 2000 quantized = 20 UNIT
		// output after 10 bps fee: 2000 - (2000 * 10 / 10000) = 2000 - 2 = 1998
		let input_amount = 2000u32;
		let output_amount = 1998u32;

		let nullifier = digest_to_bytes(Nullifier::from_preimage(secret, transfer_count).hash);
		let unspendable_account_digest = UnspendableAccount::from_secret(secret).account_id;
		let unspendable_account = digest_to_bytes(unspendable_account_digest);
		let exit_account = BytesDigest::new_unchecked([4u8; 32]);

		// For single-leaf tree: ZK tree root = leaf hash
		let zk_tree_root =
			compute_zk_leaf_hash(&unspendable_account, transfer_count, 0, input_amount);

		// Block header constants (from test-helpers)
		let block_number = 1u32;
		let parent_hash: [u8; 32] = [0u8; 32];
		let state_root: [u8; 32] = [
			0x7d, 0x5f, 0x04, 0x3e, 0x06, 0x8b, 0xe9, 0x69, 0x1e, 0xfb, 0xc3, 0xc1, 0xd4, 0x98,
			0x78, 0x8b, 0x5d, 0xc5, 0xc7, 0xd6, 0x5f, 0x41, 0xc0, 0xe2, 0x4e, 0x22, 0x11, 0xc3,
			0x99, 0x7c, 0x08, 0x11,
		];
		let extrinsics_root: [u8; 32] = [0u8; 32];
		let digest: [u8; 110] = [
			8, 6, 112, 111, 119, 95, 128, 233, 182, 183, 107, 158, 1, 115, 19, 219, 126, 253, 86,
			30, 208, 176, 70, 21, 45, 180, 229, 9, 62, 91, 4, 6, 53, 245, 52, 48, 38, 123, 225, 5,
			112, 111, 119, 95, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
			0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
			0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 79, 226,
		];

		// Compute block hash from header fields
		let header_inputs = HeaderInputs::new(
			BytesDigest::new_unchecked(parent_hash),
			block_number,
			BytesDigest::new_unchecked(state_root),
			BytesDigest::new_unchecked(extrinsics_root),
			BytesDigest::new_unchecked(zk_tree_root),
			&digest,
		)
		.expect("Failed to create header inputs");
		let block_hash = header_inputs.block_hash();
		println!("Computed block_hash: {:?}", block_hash.as_ref());

		let inputs = CircuitInputs {
			public: PublicCircuitInputs {
				asset_id: 0u32,
				output_amount_1: output_amount,
				output_amount_2: 0u32,
				volume_fee_bps: 10,
				nullifier,
				exit_account_1: exit_account,
				exit_account_2: BytesDigest::default(),
				block_hash,
				block_number,
			},
			private: PrivateCircuitInputs {
				secret,
				transfer_count,
				unspendable_account,
				parent_hash: BytesDigest::new_unchecked(parent_hash),
				state_root: BytesDigest::new_unchecked(state_root),
				extrinsics_root: BytesDigest::new_unchecked(extrinsics_root),
				digest,
				input_amount,
				zk_tree_root,
				zk_merkle_siblings: vec![],
				zk_merkle_positions: vec![],
			},
		};

		// Generate leaf proof
		println!("Generating leaf proof...");
		let prover_path = bins_dir.join("prover.bin");
		let common_path = bins_dir.join("common.bin");
		let prover = WormholeProver::new_from_files(&prover_path, &common_path)
			.expect("Failed to create prover");
		let leaf_proof = prover.commit(&inputs).unwrap().prove().unwrap();

		// Aggregate (with padding to fill batch)
		println!("Aggregating proof into a private batch...");
		let mut aggregator =
			PrivateBatchAggregator::new(bins_dir).expect("Failed to create aggregator");
		aggregator.push_proof(leaf_proof).expect("Failed to push proof");
		let aggregated_proof = aggregator.aggregate().expect("Failed to aggregate");

		// Verify locally
		println!("Verifying private-batch proof...");
		aggregator
			.verify(aggregated_proof.clone())
			.expect("Private-batch proof should verify");

		aggregated_proof
	}

	/// Helper to compute ZK leaf hash (must match circuit computation)
	fn compute_zk_leaf_hash(
		to_account: &[u8; 32],
		transfer_count: u64,
		asset_id: u32,
		input_amount: u32,
	) -> [u8; 32] {
		use plonky2::{field::types::Field, hash::poseidon2::Poseidon2Hash, plonk::config::Hasher};
		use qp_zk_circuits_common::{
			circuit::F,
			serialization::{bytes_to_digest, digest_to_bytes},
			utils::u64_to_felts,
		};

		let to_account_felts = bytes_to_digest(to_account);
		let transfer_count_felts = u64_to_felts(transfer_count);

		let mut preimage = Vec::new();
		preimage.extend(to_account_felts);
		preimage.extend(transfer_count_felts);
		preimage.push(F::from_canonical_u32(asset_id));
		preimage.push(F::from_canonical_u32(input_amount));

		let hash = Poseidon2Hash::hash_no_pad(&preimage);
		digest_to_bytes(&hash.elements)
	}
}

/// Tests for public-batch proof verification (second aggregation layer).
#[cfg(test)]
mod public_batch_proof_tests {
	use crate::{
		mock::*,
		pallet::{Error, PotentialWormholeBalance, TotalWormholeExits, UsedNullifiers},
	};
	use frame_support::{assert_noop, assert_ok, traits::fungible::Inspect};
	use frame_system::RawOrigin;
	use qp_wormhole_verifier::{
		parse_public_batch_public_inputs, ProofWithPublicInputs, PublicBatchPublicInputs, C, F,
	};
	use sp_core::{crypto::AccountId32, H256};
	use sp_runtime::Permill;

	/// The D const parameter for plonky2 proofs (extension degree = 2)
	const D: usize = 2;

	/// The aggregator address baked into the fixture (must decode to a valid AccountId32).
	/// Every 8-byte limb must be a canonical Goldilocks field element, which [7u8; 32] is.
	const AGGREGATOR_ADDRESS: [u8; 32] = [7u8; 32];

	/// Real public-batch proof for testing (hex-encoded): 1 real private batch
	/// (itself 1 real leaf + dummy leaf padding) + dummy private-batch padding.
	/// Regenerate with `regenerate_public_batch_fixture` below.
	const PUBLIC_BATCH_PROOF_HEX: &str = include_str!("../test-data/public_batch.hex");

	fn get_test_proof_bytes() -> Vec<u8> {
		hex::decode(PUBLIC_BATCH_PROOF_HEX.trim()).expect("Invalid hex in test proof")
	}

	fn deserialize_test_proof() -> ProofWithPublicInputs<F, C, D> {
		let proof_bytes = get_test_proof_bytes();
		let verifier = crate::get_public_batch_verifier().expect("Verifier should be available");
		ProofWithPublicInputs::<F, C, D>::from_bytes(proof_bytes, &verifier.circuit_data.common)
			.expect("Proof should deserialize")
	}

	fn parse_test_inputs() -> PublicBatchPublicInputs {
		let proof = deserialize_test_proof();
		parse_public_batch_public_inputs(
			&proof,
			crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
			crate::circuit_config::NUM_LEAF_PROOFS,
		)
		.expect("Should parse public-batch public inputs")
	}

	/// Insert the proof's referenced block hash into frame_system and advance past it.
	fn setup_matching_block_state(inputs: &PublicBatchPublicInputs) {
		let block_number = inputs.block_data.block_number as u64;
		let block_hash_bytes: [u8; 32] = inputs.block_data.block_hash.as_ref().try_into().unwrap();
		frame_system::BlockHash::<Test>::insert(block_number, H256::from(block_hash_bytes));
		System::set_block_number(block_number + 10);
		PotentialWormholeBalance::<Test>::put(1_000_000 * UNIT);
	}

	#[test]
	fn test_parse_public_batch_public_inputs_succeeds() {
		let inputs = parse_test_inputs();

		assert_eq!(inputs.asset_id, 0, "Asset ID should be native (0)");
		assert_eq!(inputs.volume_fee_bps, 10, "Volume fee should be 10 bps");
		assert_eq!(
			inputs.aggregator_address.as_ref(),
			&AGGREGATOR_ADDRESS,
			"Aggregator address should round-trip through the proof"
		);

		let expected_slots = crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS *
			crate::circuit_config::NUM_LEAF_PROOFS *
			2;
		assert_eq!(inputs.total_exit_slots as usize, expected_slots);
		assert_eq!(inputs.account_data.len(), expected_slots);
		assert_eq!(
			inputs.nullifiers.len(),
			crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS *
				crate::circuit_config::NUM_LEAF_PROOFS
		);

		// Exactly one real leaf exit; everything else is dummy padding.
		let real_slots = inputs.account_data.iter().filter(|a| a.summed_output_amount > 0).count();
		assert_eq!(real_slots, 1, "Fixture should contain exactly one real exit");

		// The one real private-batch segment carries NUM_LEAF_PROOFS non-zero nullifiers
		// (dummy *leaves* inside a real private batch get dummy nullifier preimages, not
		// zeros); the dummy private-batch segments are fully zeroed by the circuit.
		let non_zero_nullifiers =
			inputs.nullifiers.iter().filter(|n| n.as_ref() != &[0u8; 32]).count();
		assert_eq!(
			non_zero_nullifiers,
			crate::circuit_config::NUM_LEAF_PROOFS,
			"Only the real segment should carry non-zero nullifiers"
		);
	}

	#[test]
	fn test_verify_public_batch_fails_with_wrong_origin() {
		new_test_ext().execute_with(|| {
			let proof_bytes = get_test_proof_bytes();
			assert_noop!(
				Wormhole::verify_public_batch(RawOrigin::Signed(account_id(1)).into(), proof_bytes),
				sp_runtime::DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn test_verify_public_batch_fails_with_invalid_bytes() {
		new_test_ext().execute_with(|| {
			let result = Wormhole::verify_public_batch(RawOrigin::None.into(), vec![0u8; 100]);
			assert!(result.is_err());
			assert_eq!(result.unwrap_err().error, Error::<Test>::ProofDeserializationFailed.into());
		});
	}

	#[test]
	fn test_verify_public_batch_succeeds_and_pays_aggregator() {
		new_test_ext().execute_with(|| {
			let inputs = parse_test_inputs();
			setup_matching_block_state(&inputs);

			let aggregator = AccountId32::new(AGGREGATOR_ADDRESS);
			assert_eq!(Balances::balance(&aggregator), 0);

			// Expected exit total from the proof's public inputs (dummy slots are zero).
			let expected_exit: u128 = inputs
				.account_data
				.iter()
				.filter(|a| a.summed_output_amount > 0)
				.map(|a| (a.summed_output_amount as u128) * crate::SCALE_DOWN_FACTOR)
				.sum();

			assert_ok!(Wormhole::verify_public_batch(
				RawOrigin::None.into(),
				get_test_proof_bytes()
			));

			assert_eq!(TotalWormholeExits::<Test>::get(), expected_exit);

			// Real nullifiers marked used; zero (dummy) nullifiers never stored.
			for nullifier in &inputs.nullifiers {
				let bytes: [u8; 32] = nullifier.as_ref().try_into().unwrap();
				if bytes == [0u8; 32] {
					continue;
				}
				assert!(UsedNullifiers::<Test>::contains_key(bytes));
			}
			assert!(
				!UsedNullifiers::<Test>::contains_key([0u8; 32]),
				"Zero nullifiers from dummy padding must not be stored"
			);

			// Aggregator rebate: fee = exit * bps / (10000 - bps), burn bucket = 50% of
			// fee, and VolumeFeesAggregatorRate (50%) of that goes to the aggregator.
			let fee_bps = VolumeFeeRateBps::get() as u128;
			let total_fee = expected_exit * fee_bps / (10_000u128 - fee_bps);
			let burn_bucket = Permill::from_percent(50) * total_fee;
			let expected_rebate = Permill::from_percent(50) * burn_bucket;
			assert!(expected_rebate > 0, "Fixture fee should produce a nonzero rebate");
			assert_eq!(
				Balances::balance(&aggregator),
				expected_rebate,
				"Aggregator should receive its slice of the burn bucket"
			);
		});
	}

	#[test]
	fn test_verify_public_batch_cannot_replay() {
		new_test_ext().execute_with(|| {
			let inputs = parse_test_inputs();
			setup_matching_block_state(&inputs);

			assert_ok!(Wormhole::verify_public_batch(
				RawOrigin::None.into(),
				get_test_proof_bytes()
			));

			// All real segments are now spent; replay must be rejected outright
			// (dummy segments alone cannot make a bundle acceptable).
			let result =
				Wormhole::verify_public_batch(RawOrigin::None.into(), get_test_proof_bytes());
			assert!(result.is_err());
			assert_eq!(result.unwrap_err().error, Error::<Test>::NullifierAlreadyUsed.into());
		});
	}

	#[test]
	fn test_verify_public_batch_fails_with_nullifier_already_used() {
		new_test_ext().execute_with(|| {
			let inputs = parse_test_inputs();
			setup_matching_block_state(&inputs);

			// Mark the (single) real nullifier as used: the only real segment is then
			// denied, and a bundle with no valid segments is rejected.
			let real_nullifier = inputs
				.nullifiers
				.iter()
				.find(|n| n.as_ref() != &[0u8; 32])
				.expect("Fixture has a real nullifier");
			let bytes: [u8; 32] = real_nullifier.as_ref().try_into().unwrap();
			UsedNullifiers::<Test>::insert(bytes, true);

			let result =
				Wormhole::verify_public_batch(RawOrigin::None.into(), get_test_proof_bytes());
			assert!(result.is_err());
			assert_eq!(result.unwrap_err().error, Error::<Test>::NullifierAlreadyUsed.into());
		});
	}

	/// Regenerate the public-batch test fixture when circuit parameters change.
	///
	/// Run with: cargo test -p pallet-wormhole --release --lib --
	/// regenerate_public_batch_fixture --nocapture --ignored
	///
	/// Builds one real private batch (via the shared fixture helper), then aggregates it
	/// into a public batch with dummy private-batch padding and the well-known
	/// AGGREGATOR_ADDRESS.
	#[test]
	#[ignore]
	fn regenerate_public_batch_fixture() {
		use qp_wormhole_aggregator::aggregator::{AggregationBackend, PublicBatchAggregator};
		use qp_wormhole_inputs::BytesDigest;
		use std::path::Path;

		let tmp_dir = std::env::temp_dir().join("pallet-wormhole-public-batch-fixture-gen");
		std::fs::create_dir_all(&tmp_dir).expect("Failed to create temp dir");

		// Must match the pallet's embedded verifier (QP_NUM_LEAF_PROOFS /
		// QP_NUM_PRIVATE_BATCH_PROOFS defaults in build.rs).
		let num_leaf_proofs = crate::circuit_config::NUM_LEAF_PROOFS;
		let num_private_batch_proofs = crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS;
		println!(
			"Generating circuit binaries (num_leaf_proofs={}, num_private_batch_proofs={})...",
			num_leaf_proofs, num_private_batch_proofs
		);
		qp_wormhole_circuit_builder::generate_all_circuit_binaries(
			&tmp_dir,
			true,
			num_leaf_proofs,
			Some(num_private_batch_proofs),
		)
		.expect("Failed to generate circuit binaries");

		let private_batch_proof = super::fixture_gen::build_test_private_batch_proof(&tmp_dir);

		println!("Aggregating into a public batch (with dummy padding)...");
		let aggregator_address = BytesDigest::new_unchecked(AGGREGATOR_ADDRESS);
		let mut aggregator = PublicBatchAggregator::new(&tmp_dir, aggregator_address)
			.expect("Failed to create public-batch aggregator");
		aggregator.push_proof(private_batch_proof).expect("Failed to push proof");
		let public_batch_proof = aggregator.aggregate().expect("Failed to aggregate");

		println!("Verifying public-batch proof...");
		aggregator
			.verify(public_batch_proof.clone())
			.expect("Public-batch proof should verify");

		let proof_bytes = public_batch_proof.to_bytes();
		let proof_hex = hex::encode(&proof_bytes);

		let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/public_batch.hex");
		std::fs::write(&fixture_path, &proof_hex).expect("Failed to write fixture");

		println!("Fixture written to: {}", fixture_path.display());
		println!("Proof size: {} bytes ({} hex chars)", proof_bytes.len(), proof_hex.len());

		let _ = std::fs::remove_dir_all(&tmp_dir);
	}
}
