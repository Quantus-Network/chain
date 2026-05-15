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
	use qp_wormhole_verifier::{BytesDigest, PublicInputsByAccount};
	use sp_core::crypto::AccountId32;
	use sp_runtime::DigestItem;

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
	fn layer1_verifier_getter_reflects_build_configuration() {
		#[cfg(wormhole_layer1_verifier)]
		assert!(crate::get_layer1_verifier().is_ok());

		#[cfg(not(wormhole_layer1_verifier))]
		assert_eq!(crate::get_layer1_verifier().unwrap_err(), "Layer1 verifier not available");
	}

	#[test]
	fn layer1_deserialization_uses_layer1_artifact_state() {
		new_test_ext().execute_with(|| {
			let err = Wormhole::deserialize_layer1_proof(&[]).unwrap_err();

			#[cfg(wormhole_layer1_verifier)]
			assert!(matches!(err, crate::Error::<Test>::Layer1ProofDeserializationFailed));

			#[cfg(not(wormhole_layer1_verifier))]
			assert!(matches!(err, crate::Error::<Test>::Layer1VerifierNotAvailable));
		});
	}

	#[test]
	fn direct_l0_nullifier_validation_rejects_used_locked_and_duplicates() {
		new_test_ext().execute_with(|| {
			let n1 = [1u8; 32];
			let n2 = [2u8; 32];
			let bundle_id = [9u8; 32];

			assert_ok!(Wormhole::ensure_nullifiers_available_for_direct_settlement(&[n1, n2]));

			let err =
				Wormhole::ensure_nullifiers_available_for_direct_settlement(&[n1, n1]).unwrap_err();
			assert!(matches!(err, crate::Error::<Test>::DuplicateNullifier));

			assert_ok!(Wormhole::mark_nullifiers_used(&[n1]));
			let err =
				Wormhole::ensure_nullifiers_available_for_direct_settlement(&[n1]).unwrap_err();
			assert!(matches!(err, crate::Error::<Test>::NullifierAlreadyUsed));

			assert_ok!(Wormhole::lock_nullifiers_for_bundle(bundle_id, 10, &[n2]));
			let err =
				Wormhole::ensure_nullifiers_available_for_direct_settlement(&[n2]).unwrap_err();
			assert!(matches!(err, crate::Error::<Test>::NullifierLocked));
		});
	}

	#[test]
	fn nullifier_lock_helpers_manage_lock_lifecycle() {
		new_test_ext().execute_with(|| {
			let n1 = [1u8; 32];
			let n2 = [2u8; 32];
			let bundle_id = [7u8; 32];
			let other_bundle_id = [8u8; 32];

			assert_ok!(Wormhole::lock_nullifiers_for_bundle(bundle_id, 10, &[n1, n2]));
			assert!(Wormhole::is_nullifier_locked(&n1));
			assert!(Wormhole::is_nullifier_locked(&n2));
			assert_eq!(Wormhole::locked_nullifiers(n1).unwrap().bundle_id, bundle_id);

			let err = Wormhole::unlock_nullifiers_for_bundle(other_bundle_id, &[n1]).unwrap_err();
			assert!(matches!(err, crate::Error::<Test>::NullifierLockMismatch));

			assert_ok!(Wormhole::unlock_nullifiers_for_bundle(bundle_id, &[n1]));
			assert!(!Wormhole::is_nullifier_locked(&n1));
			assert!(Wormhole::is_nullifier_locked(&n2));

			assert_ok!(Wormhole::mark_locked_nullifiers_used(bundle_id, &[n2]));
			assert!(!Wormhole::is_nullifier_locked(&n2));
			assert!(Wormhole::is_nullifier_used(&n2));
		});
	}

	fn public_output(account: AccountId, summed_output_amount: u32) -> PublicInputsByAccount {
		PublicInputsByAccount {
			summed_output_amount,
			exit_account: BytesDigest::new_unchecked(*account.as_ref()),
		}
	}

	fn set_block_author(preimage: [u8; 32]) -> AccountId {
		let author = qp_wormhole::derive_wormhole_account(preimage);
		System::deposit_log(DigestItem::PreRuntime(*b"pow_", preimage.to_vec()));
		author
	}

	fn expected_total_fee(total_exit_amount: Balance) -> Balance {
		let fee_bps = VolumeFeeRateBps::get() as Balance;
		total_exit_amount
			.saturating_mul(fee_bps)
			.checked_div(10_000u128.saturating_sub(fee_bps))
			.unwrap_or(0)
	}

	fn expected_fee_split(
		total_exit_amount: Balance,
		delegated: bool,
		has_author: bool,
	) -> (Balance, Balance, Balance, Balance) {
		let total_fee = expected_total_fee(total_exit_amount);
		let base_burn = VolumeFeesBurnRate::get() * total_fee;
		let non_burned_fee = total_fee.saturating_sub(base_burn);
		let aggregation_prover_fee =
			if delegated { AggregationProverFeeShare::get() * non_burned_fee } else { 0 };
		let block_author_share = non_burned_fee.saturating_sub(aggregation_prover_fee);
		let block_author_fee = if has_author { block_author_share } else { 0 };
		let burn_amount =
			if has_author { base_burn } else { base_burn.saturating_add(block_author_share) };
		(total_fee, burn_amount, block_author_fee, aggregation_prover_fee)
	}

	#[test]
	fn public_output_settlement_prepare_rejects_below_minimum_without_writes() {
		new_test_ext().execute_with(|| {
			let recipient = account_id(3);
			let balance_before = Balances::balance(&recipient);
			let transfer_count_before = Wormhole::transfer_count(&recipient);

			let err = Wormhole::prepare_public_output_settlement(
				&[public_output(recipient.clone(), 1)],
				VolumeFeeRateBps::get(),
				crate::SettlementKind::DirectL0,
			)
			.unwrap_err();

			assert!(matches!(err, crate::Error::<Test>::TransferAmountBelowMinimum));
			assert_eq!(Balances::balance(&recipient), balance_before);
			assert_eq!(Wormhole::transfer_count(&recipient), transfer_count_before);
		});
	}

	#[test]
	fn public_output_settlement_prepare_and_apply_mints_and_records_transfer() {
		new_test_ext().execute_with(|| {
			let recipient = account_id(3);
			let balance_before = Balances::balance(&recipient);
			let transfer_count_before = Wormhole::transfer_count(&recipient);

			let prepared = Wormhole::prepare_public_output_settlement(
				&[public_output(recipient.clone(), 1_000)],
				VolumeFeeRateBps::get(),
				crate::SettlementKind::DirectL0,
			)
			.unwrap();

			assert_eq!(prepared.total_exit_amount, 10 * UNIT);
			assert_eq!(prepared.transfers.as_slice(), &[(recipient.clone(), 10 * UNIT)]);
			assert_eq!(prepared.block_author_fee, 0);

			assert_ok!(Wormhole::apply_public_output_settlement(prepared));

			assert_eq!(Balances::balance(&recipient), balance_before + 10 * UNIT);
			assert_eq!(Wormhole::transfer_count(&recipient), transfer_count_before + 1);
		});
	}

	#[test]
	fn direct_l0_fee_behavior_preserved() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let recipient = account_id(3);
			let author = set_block_author([7u8; 32]);
			let total_exit_amount = 10 * UNIT;
			let (total_fee, burn_amount, block_author_fee, aggregation_prover_fee) =
				expected_fee_split(total_exit_amount, false, true);
			let author_balance_before = Balances::balance(&author);

			let prepared = Wormhole::prepare_public_output_settlement(
				&[public_output(recipient, 1_000)],
				VolumeFeeRateBps::get(),
				crate::SettlementKind::DirectL0,
			)
			.unwrap();

			assert_eq!(prepared.total_fee, total_fee);
			assert_eq!(prepared.burn_amount, burn_amount);
			assert_eq!(prepared.block_author_fee, block_author_fee);
			assert_eq!(prepared.aggregation_prover_fee, aggregation_prover_fee);
			assert_eq!(prepared.block_author, Some(author.clone()));
			assert_eq!(prepared.aggregation_reward_account, None);

			assert_ok!(Wormhole::apply_public_output_settlement(prepared));

			assert_eq!(Balances::balance(&author), author_balance_before + block_author_fee);
			System::assert_has_event(
				crate::Event::<Test>::WormholeFeeSettled {
					total_fee,
					burn_amount,
					block_author_fee,
					aggregation_prover_fee,
				}
				.into(),
			);
		});
	}

	#[test]
	fn delegated_l1_pays_aggregation_prover_fee_share() {
		new_test_ext().execute_with(|| {
			let recipient = account_id(3);
			let reward_account = account_id(4);
			let total_exit_amount = 10 * UNIT;
			let (total_fee, burn_amount, block_author_fee, aggregation_prover_fee) =
				expected_fee_split(total_exit_amount, true, false);
			let reward_balance_before = Balances::balance(&reward_account);

			let prepared = Wormhole::prepare_public_output_settlement(
				&[public_output(recipient, 1_000)],
				VolumeFeeRateBps::get(),
				crate::SettlementKind::DelegatedL1 {
					aggregation_reward_account: reward_account.clone(),
				},
			)
			.unwrap();

			assert_eq!(prepared.total_fee, total_fee);
			assert_eq!(prepared.burn_amount, burn_amount);
			assert_eq!(prepared.block_author_fee, block_author_fee);
			assert_eq!(prepared.aggregation_prover_fee, aggregation_prover_fee);
			assert_eq!(prepared.aggregation_reward_account, Some(reward_account.clone()));

			assert_ok!(Wormhole::apply_public_output_settlement(prepared));

			assert_eq!(
				Balances::balance(&reward_account),
				reward_balance_before + aggregation_prover_fee
			);
		});
	}

	#[test]
	fn no_author_redirects_block_author_fee_share_to_burn() {
		new_test_ext().execute_with(|| {
			let recipient = account_id(3);
			let reward_account = account_id(4);
			let total_exit_amount = 10 * UNIT;
			let (_total_fee, burn_amount, block_author_fee, aggregation_prover_fee) =
				expected_fee_split(total_exit_amount, true, false);

			let prepared = Wormhole::prepare_public_output_settlement(
				&[public_output(recipient, 1_000)],
				VolumeFeeRateBps::get(),
				crate::SettlementKind::DelegatedL1 { aggregation_reward_account: reward_account },
			)
			.unwrap();

			assert_eq!(prepared.block_author, None);
			assert_eq!(prepared.block_author_fee, block_author_fee);
			assert_eq!(prepared.aggregation_prover_fee, aggregation_prover_fee);
			assert_eq!(prepared.burn_amount, burn_amount);
		});
	}

	#[test]
	fn settlement_helper_used_by_both_direct_and_delegated_paths() {
		new_test_ext().execute_with(|| {
			let recipient = account_id(3);
			let reward_account = account_id(4);

			let direct = Wormhole::prepare_public_output_settlement(
				&[public_output(recipient.clone(), 1_000)],
				VolumeFeeRateBps::get(),
				crate::SettlementKind::DirectL0,
			)
			.unwrap();
			let delegated = Wormhole::prepare_public_output_settlement(
				&[public_output(recipient, 1_000)],
				VolumeFeeRateBps::get(),
				crate::SettlementKind::DelegatedL1 {
					aggregation_reward_account: reward_account.clone(),
				},
			)
			.unwrap();

			assert_eq!(direct.total_exit_amount, delegated.total_exit_amount);
			assert_eq!(direct.total_fee, delegated.total_fee);
			assert_eq!(direct.transfers, delegated.transfers);
			assert_eq!(direct.aggregation_prover_fee, 0);
			assert!(delegated.aggregation_prover_fee > 0);
			assert_eq!(delegated.aggregation_reward_account, Some(reward_account));
		});
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
	fn test_verify_aggregated_proof_fails_with_locked_nullifier() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_aggregated_public_inputs(&proof).expect("Should parse");

			// Set up block hash to match the proof
			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			let block_hash = H256::from(block_hash_bytes);
			frame_system::BlockHash::<Test>::insert(block_number, block_hash);

			let nullifier = inputs.nullifiers.first().expect("fixture has nullifiers");
			let nullifier_bytes: [u8; 32] = nullifier.as_ref().try_into().unwrap();
			assert_ok!(Wormhole::lock_nullifiers_for_bundle(
				[7u8; 32],
				block_number + 10,
				&[nullifier_bytes]
			));

			let proof_bytes = get_test_proof_bytes();
			let result = Wormhole::verify_aggregated_proof(RawOrigin::None.into(), proof_bytes);

			assert!(result.is_err());
			let err = result.unwrap_err();
			assert_eq!(err.error, Error::<Test>::NullifierLocked.into());
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
