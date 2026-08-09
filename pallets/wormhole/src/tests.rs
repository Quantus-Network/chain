/// Expected volume fee in base units for a quantized exit total.
///
/// Independent re-derivation of the quantized-ceiling fee rule the circuit
/// enforces (`out · 10000 ≤ input · (10000 − bps)` over quantized u32 amounts):
/// `fee_quanta = ceil(exit_quanta · bps / (10000 − bps))`, so any nonzero exit
/// pays at least one full quantum. Kept separate from the pallet's own
/// computation so fee assertions don't just mirror the code under test.
#[cfg(test)]
fn ceil_volume_fee(exit_quanta: u128, fee_bps: u128) -> u128 {
	exit_quanta.saturating_mul(fee_bps).div_ceil(10_000 - fee_bps) * crate::SCALE_DOWN_FACTOR
}

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

	/// Security regression test (native/asset-0 conflation).
	///
	/// The wormhole tags native leaves with `asset_id == 0`, but `pallet_assets` uses id 0 for
	/// an unrelated, independently-mintable token. Native deposits reach the recorder as `None`
	/// (from `Balances` events); a `pallet_assets` asset-0 credit reaches it as `Some(0)` (from
	/// `Assets::Issued`). These must NOT be conflated: a `Some(0)` credit is not backed by any
	/// native, so treating it as native would insert a natively-exitable leaf — minting
	/// unbacked native out of the wormhole.
	///
	/// This asserts that `record_transfer_proof(Some(0), ..)` inserts no ZK-tree leaf, while
	/// genuine native (`None`) still does.
	#[test]
	fn asset_zero_credit_is_not_treated_as_native_deposit() {
		use qp_wormhole::TransferProofRecorder;

		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = account_id(1);
			let to = account_id(9001);
			let amount = 1_000 * UNIT;

			assert_eq!(ZkTree::leaf_count(), 0);

			// A pallet_assets asset-0 mint (arrives as `Some(0)`) must be inert for the
			// native wormhole: no leaf — and must report as not recorded so weight
			// reconciliation does not treat the drop as a ZK-tree insert.
			assert!(
				!<Wormhole as TransferProofRecorder<AccountId, u32, u128>>::record_transfer_proof(
					Some(0u32),
					from.clone(),
					to.clone(),
					amount,
				),
				"an asset-0 credit must report as not recorded"
			);
			assert_eq!(
				ZkTree::leaf_count(),
				0,
				"an asset-0 credit must not insert a native-exitable ZK-tree leaf"
			);

			// Genuine native (arrives as `None`) is still recorded.
			assert!(
				<Wormhole as TransferProofRecorder<AccountId, u32, u128>>::record_transfer_proof(
					None, from, to, amount,
				),
				"a native deposit must report as recorded"
			);
			assert_eq!(ZkTree::leaf_count(), 1, "a native deposit must insert a leaf");
		});
	}

	/// A zero-amount credit moves no value, but recording it would still append a
	/// ZK-tree leaf, advance the recipient's transfer count, and emit an event.
	/// Zero-value `Balances::Transfer` events are reachable from permissionless
	/// surfaces (`transfer_keep_alive(dest, 0)`, zero-value scheduled transfers),
	/// so the recorder must drop zero-amount credits and report them as not
	/// recorded (so weight reconciliation doesn't count a leaf insert).
	#[test]
	fn zero_amount_credit_is_not_recorded() {
		use qp_wormhole::TransferProofRecorder;

		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = account_id(1);
			let to = account_id(9001);

			assert!(
				!<Wormhole as TransferProofRecorder<AccountId, u32, u128>>::record_transfer_proof(
					None,
					from.clone(),
					to.clone(),
					0,
				),
				"a zero-amount credit must report as not recorded"
			);
			assert_eq!(ZkTree::leaf_count(), 0, "no ZK-tree leaf for a zero-amount credit");
			assert_eq!(
				Wormhole::transfer_count(&to),
				0,
				"the recipient's transfer count must not advance"
			);

			// Sanity: the same credit with a nonzero amount is recorded.
			assert!(
				<Wormhole as TransferProofRecorder<AccountId, u32, u128>>::record_transfer_proof(
					None, from, to, 1,
				)
			);
			assert_eq!(ZkTree::leaf_count(), 1);
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

	/// Security regression test (leaf-encoding aliasing).
	///
	/// `hash_leaf` encodes the recipient with a lossy 8-byte/felt encoding that reduces
	/// each limb mod the Goldilocks prime. A "non-canonical alias" of a recipient (some
	/// limb increased by the prime) therefore encodes to the *same* felts. If the
	/// transfer-count sequence were keyed on the raw recipient bytes, a deposit to the
	/// alias would start its own count at 0 and produce a leaf identical to the canonical
	/// recipient's deposit 0 — the two deposits would then share one nullifier
	/// (`H(secret, transfer_count)`) and only one of them could ever be exited.
	///
	/// This test deposits the same amount to a canonical recipient and to its alias and
	/// asserts the resulting leaves are distinct.
	#[test]
	fn deposits_to_non_canonical_alias_do_not_collide_with_canonical_leaf() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = account_id(1);
			let amount = 10 * UNIT;

			const GOLDILOCKS_P: u64 = 0xFFFF_FFFF_0000_0001;

			// Canonical recipient: first 8-byte limb small (an alias exists only when a
			// limb is < 2^32 - 1, so that limb + p still fits in 8 bytes).
			let mut canonical_bytes = [0x11u8; 32];
			canonical_bytes[..8].copy_from_slice(&5u64.to_le_bytes());
			let canonical = AccountId32::new(canonical_bytes);

			// Alias: same account with the first limb increased by the prime. The lossy
			// leaf encoding reduces it back to 5, so both accounts encode identically.
			let mut alias_bytes = canonical_bytes;
			alias_bytes[..8].copy_from_slice(&(5u64 + GOLDILOCKS_P).to_le_bytes());
			let alias = AccountId32::new(alias_bytes);
			assert_ne!(canonical, alias);

			Wormhole::record_transfer(0u32, &from, &canonical, amount);
			Wormhole::record_transfer(0u32, &from, &alias, amount);

			let leaf0 = ZkTree::leaf(0).expect("first deposit must insert a leaf");
			let leaf1 = ZkTree::leaf(1).expect("second deposit must insert a leaf");
			let hash0 = pallet_zk_tree::tree::hash_leaf::<Test>(&leaf0);
			let hash1 = pallet_zk_tree::tree::hash_leaf::<Test>(&leaf1);
			assert_ne!(
				hash0, hash1,
				"a deposit to a non-canonical alias must not produce the same leaf \
				 (and thus the same nullifier) as a deposit to the canonical recipient"
			);

			// The alias shares the canonical recipient's count sequence: both deposits
			// are recorded under the canonical key, and the second leaf continues the
			// sequence rather than restarting at 0.
			assert_eq!(Wormhole::transfer_count(&canonical), 2);
			assert_eq!(Wormhole::transfer_count(&alias), 0);
			assert_eq!(leaf0.transfer_count, 0);
			assert_eq!(leaf1.transfer_count, 1);

			// Both leaves store the canonical recipient (what the hash actually
			// commits to), so leaf data is consistent with the leaf hash.
			assert_eq!(leaf0.to, canonical);
			assert_eq!(leaf1.to, canonical);
		});
	}

	/// Golden cross-check: the pallet's `hash_leaf` must stay byte-compatible with the
	/// ZK circuit's leaf hash (`ZkLeafTargets::collect_for_hash`), reproduced here by
	/// `fixture_gen::compute_zk_leaf_hash` on the real circuit crates. This pins the
	/// encoding so refactors (e.g. recipient canonicalization) cannot silently change
	/// the hash of existing canonical leaves.
	#[test]
	fn pallet_leaf_hash_matches_circuit_leaf_hash() {
		// (to, transfer_count, asset_id, quantized_amount). First case uses the
		// well-known fixture inputs: TEST_ADDRESS = WA([42u8; 32]), count 1, 2000
		// quantized (the same values the private-batch proof fixture commits to).
		let cases = [
			(test_account(), 1u64, 0u32, 2000u32),
			(account_id(1), 0u64, 0u32, 1000u32),
			(account_id(424242), 7u64, 5u32, 1u32),
		];

		// The zk-tree pallet's `hash_leaf_golden_vector` test pins this exact case to a
		// hardcoded hash; verifying it here against the circuit crates proves that the
		// pinned constant is circuit-correct, not just self-consistent.
		let golden_case = (AccountId32::new([0x11u8; 32]), 7u64, 5u32, 1234u32);
		let golden_hash: [u8; 32] = [
			195, 94, 210, 27, 96, 177, 127, 68, 16, 231, 47, 227, 104, 21, 175, 254, 219, 85, 224,
			111, 64, 162, 32, 119, 226, 89, 143, 126, 203, 254, 51, 93,
		];
		{
			let (to, transfer_count, asset_id, quantized_amount) = &golden_case;
			let to_bytes: &[u8; 32] = to.as_ref();
			assert_eq!(
				super::fixture_gen::compute_zk_leaf_hash(
					to_bytes,
					*transfer_count,
					*asset_id,
					*quantized_amount,
				),
				golden_hash,
				"golden vector constant is not circuit-correct"
			);
		}

		for (to, transfer_count, asset_id, quantized_amount) in
			cases.into_iter().chain([golden_case])
		{
			let leaf = pallet_zk_tree::ZkLeaf {
				to: to.clone(),
				transfer_count,
				asset_id,
				// `hash_leaf` quantizes by dividing by AMOUNT_SCALE_DOWN_FACTOR;
				// the circuit helper takes the already-quantized amount.
				amount: (quantized_amount as u128) * pallet_zk_tree::tree::AMOUNT_SCALE_DOWN_FACTOR,
			};
			let pallet_hash = pallet_zk_tree::tree::hash_leaf::<Test>(&leaf);

			let to_bytes: &[u8; 32] = to.as_ref();
			let circuit_hash = super::fixture_gen::compute_zk_leaf_hash(
				to_bytes,
				transfer_count,
				asset_id,
				quantized_amount,
			);

			assert_eq!(
				pallet_hash, circuit_hash,
				"pallet hash_leaf diverged from the circuit leaf hash for {to:?}"
			);
		}
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
	// Genesis proofs are derived from real balances (single source of truth)
	// =========================================================================
	//
	// There is no separate wormhole endowment list at genesis: `on_initialize(1)` derives
	// a transfer proof from every account that exists with a balance. An exitable leaf
	// that isn't backed by actually-issued value is therefore unrepresentable — the leaf
	// amount IS the genesis balance.

	#[test]
	fn genesis_proofs_derive_from_balances() {
		use frame_support::traits::Hooks;

		let addr1 = account_id(100);
		let addr2 = account_id(101);
		let amount1 = 100 * UNIT;
		let amount2 = 250 * UNIT;

		new_test_ext_with_endowments(vec![(addr1.clone(), amount1), (addr2.clone(), amount2)])
			.execute_with(|| {
				System::set_block_number(1);
				Wormhole::on_initialize(1);

				// One leaf per funded genesis account, amount = the real balance.
				assert_eq!(Wormhole::transfer_count(&addr1), 1);
				assert_eq!(Wormhole::transfer_count(&addr2), 1);
			});
	}

	// =========================================================================
	// Soundness counter removal migration
	// =========================================================================

	#[test]
	fn migration_removes_soundness_counters() {
		use frame_support::traits::UncheckedOnRuntimeUpgrade;

		new_test_ext().execute_with(|| {
			// Simulate leftover v1 storage from before the counters were removed.
			crate::migrations::v2::PotentialWormholeBalance::<Test>::put(1_000 * UNIT);
			crate::migrations::v2::TotalWormholeExits::<Test>::put(250 * UNIT);

			crate::migrations::v2::RemoveSoundnessCounters::<Test>::on_runtime_upgrade();

			assert!(
				!crate::migrations::v2::PotentialWormholeBalance::<Test>::exists(),
				"Migration must delete PotentialWormholeBalance"
			);
			assert!(
				!crate::migrations::v2::TotalWormholeExits::<Test>::exists(),
				"Migration must delete TotalWormholeExits"
			);
		});
	}
}

/// Tests for private-batch proof verification
#[cfg(test)]
mod private_batch_proof_tests {
	use crate::{
		mock::*,
		pallet::{Error, UsedNullifiers},
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
		assert_eq!(inputs.volume_fee_bps, 4, "Volume fee should be 4 bps");
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

			let proof_bytes = get_test_proof_bytes();

			// This should succeed - proof is valid and state matches
			assert_ok!(Wormhole::verify_private_batch(RawOrigin::None.into(), proof_bytes));

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

			// No author in the digest: the miner fee is burned instead of minted,
			// so no MinerVolumeFeePaid event may be emitted.
			assert!(
				!System::events().iter().any(|r| matches!(
					r.event,
					RuntimeEvent::Wormhole(crate::Event::<Test>::MinerVolumeFeePaid { .. })
				)),
				"no miner fee event should be emitted without a block author"
			);
		});
	}

	/// The runtime's `WormholeProofRecorderExtension` records transfer proofs by scanning
	/// `Balances::Transfer`/`Minted` events after a transaction. The exit path must therefore
	/// never emit either event: `process_exit_bundle` credits exits via
	/// `Unbalanced::increase_balance` (event-free) and records its own proof internally via
	/// `record_transfer`. If a refactor ever switched the exit credit to `mint_into` (which
	/// emits `Minted`), each exit could be recorded twice — inflating `TransferCount` and
	/// creating a duplicate, unspendable leaf.
	#[test]
	fn exit_credits_emit_no_scannable_transfer_events() {
		new_test_ext().execute_with(|| {
			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");

			// Set up block state so the proof's cheap bundle checks pass.
			let block_number = inputs.block_data.block_number as u64;
			let block_hash_bytes: [u8; 32] =
				inputs.block_data.block_hash.as_ref().try_into().unwrap();
			frame_system::BlockHash::<Test>::insert(block_number, H256::from(block_hash_bytes));
			System::set_block_number(block_number + 10);

			// Guard that the assertion below is meaningful: the proof must credit exits.
			let expected_exit: u128 = inputs
				.account_data
				.iter()
				.filter(|a| a.summed_output_amount > 0)
				.map(|a| (a.summed_output_amount as u128) * crate::SCALE_DOWN_FACTOR)
				.sum();
			assert!(expected_exit > 0, "test proof must credit at least one exit");

			System::reset_events();
			assert_ok!(Wormhole::verify_private_batch(
				RawOrigin::None.into(),
				get_test_proof_bytes()
			));

			// No `Transfer`/`Minted` events: nothing for an event-based recorder to pick up.
			for record in System::events() {
				assert!(
					!matches!(
						record.event,
						RuntimeEvent::Balances(
							pallet_balances::Event::<Test>::Transfer { .. } |
								pallet_balances::Event::<Test>::Minted { .. }
						)
					),
					"exit processing must not emit scannable Transfer/Minted events: {:?}",
					record.event
				);
			}
		});
	}

	/// Sets up the on-chain block state so the test proof's cheap bundle checks pass.
	fn setup_valid_block_state_for_test_proof() {
		let proof = deserialize_test_proof();
		let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");
		let block_number = inputs.block_data.block_number as u64;
		let block_hash_bytes: [u8; 32] = inputs.block_data.block_hash.as_ref().try_into().unwrap();
		frame_system::BlockHash::<Test>::insert(block_number, H256::from(block_hash_bytes));
		System::set_block_number(block_number + 10);
	}

	/// `ProofWithPublicInputs::from_bytes` reads the proof off the front of the buffer
	/// and silently ignores trailing bytes, so without an exact-framing check one valid
	/// proof has unboundedly many byte representations — each a distinct transaction
	/// hash whose full copy + parse every node re-pays at pool admission, fee-free.
	/// Pre-validation must accept exactly one canonical encoding per proof.
	#[test]
	fn pre_validation_rejects_padded_proof_bytes() {
		new_test_ext().execute_with(|| {
			setup_valid_block_state_for_test_proof();

			// The canonical encoding passes pre-validation.
			assert!(Wormhole::pre_validate_private_batch_proof(&get_test_proof_bytes()).is_ok());

			// The same proof with trailing junk must be rejected.
			let mut padded = get_test_proof_bytes();
			padded.extend_from_slice(&[0u8; 32]);
			assert!(matches!(
				Wormhole::pre_validate_private_batch_proof(&padded),
				Err(Error::<Test>::NonCanonicalProofEncoding)
			));
		});
	}

	/// Oversized blobs must be cut off by a length gate BEFORE the byte copy and the
	/// parser run — `ProofDeserializationFailed` after the fact means the work was
	/// already done.
	#[test]
	fn pre_validation_rejects_oversized_proof_bytes() {
		new_test_ext().execute_with(|| {
			let oversized = vec![0u8; crate::MAX_PROOF_BYTES + 1];
			assert!(matches!(
				Wormhole::pre_validate_private_batch_proof(&oversized),
				Err(Error::<Test>::ProofTooLarge)
			));
		});
	}

	/// The block-inclusion gate (`pre_dispatch`) must reject a proof that cannot be
	/// verified. Before this was fixed, `pre_dispatch` was a no-op that returned `Ok(())`
	/// for any `verify_*` call, so junk rode into blocks as failed `Pays::No` extrinsics;
	/// this assertion is red against that old behavior.
	#[test]
	fn pre_dispatch_rejects_unverifiable_proof() {
		use sp_runtime::traits::ValidateUnsigned;

		new_test_ext().execute_with(|| {
			let call = crate::Call::<Test>::verify_private_batch { proof_bytes: vec![0u8; 100] };
			assert!(
				<Wormhole as ValidateUnsigned>::pre_dispatch(&call).is_err(),
				"pre_dispatch must reject an unverifiable proof"
			);
		});
	}

	/// The core of the anti-DoS change: pool admission (`validate_unsigned`) must NOT run
	/// the expensive ZK verification, while the block-inclusion gate (`pre_dispatch`)
	/// must. A proof whose public inputs are intact (so the cheap bundle checks pass) but
	/// whose proof data is corrupted (so ZK verification fails) is therefore *admitted*
	/// by `validate_unsigned` yet *rejected* by `pre_dispatch`.
	#[test]
	fn validate_unsigned_skips_verify_but_pre_dispatch_enforces_it() {
		use sp_runtime::{traits::ValidateUnsigned, transaction_validity::TransactionSource};

		new_test_ext().execute_with(|| {
			setup_valid_block_state_for_test_proof();

			// Corrupt the proof data while leaving the public inputs (serialized after the
			// proof) intact, so the cheap checks still pass but ZK verification fails.
			let mut tampered = get_test_proof_bytes();
			for byte in tampered.iter_mut().take(64) {
				*byte ^= 0xFF;
			}
			let call = crate::Call::<Test>::verify_private_batch { proof_bytes: tampered.clone() };

			// Pool admission is verify-free, so it still admits the tampered proof.
			assert_ok!(<Wormhole as ValidateUnsigned>::validate_unsigned(
				TransactionSource::External,
				&call,
			));

			// The block-inclusion gate runs the ZK verify and rejects it.
			assert!(
				<Wormhole as ValidateUnsigned>::pre_dispatch(&call).is_err(),
				"pre_dispatch must reject a proof that fails ZK verification"
			);
		});
	}

	/// A valid proof against valid state passes both the pool path and the inclusion gate,
	/// and the pool dedup tag is derived from the proof's nullifiers (so byte-variants of
	/// the same logical exit collide instead of each earning a fresh pool entry).
	#[test]
	fn validate_unsigned_and_pre_dispatch_accept_valid_proof_with_semantic_tag() {
		use codec::Encode;
		use sp_runtime::{traits::ValidateUnsigned, transaction_validity::TransactionSource};

		new_test_ext().execute_with(|| {
			setup_valid_block_state_for_test_proof();

			let proof = deserialize_test_proof();
			let inputs = parse_private_batch_public_inputs(&proof).expect("Should parse");
			let call =
				crate::Call::<Test>::verify_private_batch { proof_bytes: get_test_proof_bytes() };

			let valid = <Wormhole as ValidateUnsigned>::validate_unsigned(
				TransactionSource::External,
				&call,
			)
			.expect("valid proof must be admitted to the pool");
			assert!(<Wormhole as ValidateUnsigned>::pre_dispatch(&call).is_ok());

			// The `provides` tag is `(prefix, blake2_256(nullifiers))` — recompute it here
			// and confirm the pool would dedup on the nullifier set rather than the raw
			// proof bytes.
			let mut preimage = Vec::new();
			for nullifier in &inputs.nullifiers {
				preimage.extend_from_slice(nullifier.as_ref());
			}
			let expected_tag =
				("WormholePrivateBatch", sp_io::hashing::blake2_256(&preimage)).encode();
			assert!(
				valid.provides.contains(&expected_tag),
				"pool dedup tag must be derived from the bundle nullifiers"
			);
		});
	}

	/// Pool admission must not derive `priority` from unverified public-input amounts.
	///
	/// With verification deferred to `pre_dispatch`, an attacker who sees a victim's
	/// gossiped exit can mutate the PI amounts (keeping the nullifiers so the semantic
	/// `provides` tag collides) and submit a higher-priority junk tx that usurps the
	/// victim's pool slot — the pool replaces same-tag txs only when the newcomer has
	/// strictly higher priority. A constant priority makes first-seen win and closes
	/// that free censorship path.
	#[test]
	fn validate_unsigned_priority_ignores_unverified_amounts() {
		use qp_plonky2_verifier::field::types::Field;
		use sp_runtime::{traits::ValidateUnsigned, transaction_validity::TransactionSource};

		new_test_ext().execute_with(|| {
			setup_valid_block_state_for_test_proof();

			let original = get_test_proof_bytes();
			let original_call =
				crate::Call::<Test>::verify_private_batch { proof_bytes: original.clone() };
			let original_valid = <Wormhole as ValidateUnsigned>::validate_unsigned(
				TransactionSource::External,
				&original_call,
			)
			.expect("valid proof must be admitted");

			// Inflate the first exit-slot amount in the PI section (layout: header of 8
			// felts, then [sum(1), exit(4)] · 2N). Nullifiers are left untouched so the
			// semantic provides tag stays identical to the victim's.
			let mut tampered_proof = deserialize_test_proof();
			assert!(
				tampered_proof.public_inputs.len() > 8,
				"test proof must have exit-slot public inputs"
			);
			tampered_proof.public_inputs[8] = F::from_canonical_u32(u32::MAX);
			let tampered_bytes = tampered_proof.to_bytes();
			assert_ne!(tampered_bytes, original, "mutation must change the encoded proof");

			let tampered_call =
				crate::Call::<Test>::verify_private_batch { proof_bytes: tampered_bytes };
			let tampered_valid = <Wormhole as ValidateUnsigned>::validate_unsigned(
				TransactionSource::External,
				&tampered_call,
			)
			.expect("amount-inflated junk still passes cheap pool checks");

			assert_eq!(
				tampered_valid.provides, original_valid.provides,
				"same nullifiers must yield the same provides tag"
			);
			assert_eq!(
				tampered_valid.priority, original_valid.priority,
				"priority must not track unverified PI amounts (else junk can usurp a real exit)"
			);
			assert_eq!(
				original_valid.priority,
				crate::UNSIGNED_EXIT_PRIORITY,
				"unsigned exits must use the fixed pool priority"
			);
		});
	}

	#[test]
	fn test_verify_private_batch_emits_miner_volume_fee_paid() {
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

			// Seed a block author via the pre-runtime digest (same path as QPoW)
			let miner_preimage = [7u8; 32];
			set_miner_preimage_digest(miner_preimage);
			let expected_author = sp_core::crypto::AccountId32::from(
				qp_wormhole::derive_wormhole_address(miner_preimage)
					.expect("test preimage limbs are canonical"),
			);

			// Expected miner fee from the proof's public inputs: the quantized-ceiling
			// volume fee; miner gets fee minus the 50% burn (mock: VolumeFeesBurnRate
			// = 50%).
			let exit_quanta: u128 = inputs
				.account_data
				.iter()
				.filter(|a| a.summed_output_amount > 0)
				.map(|a| a.summed_output_amount as u128)
				.sum();
			let fee_bps = VolumeFeeRateBps::get() as u128;
			let total_fee = super::ceil_volume_fee(exit_quanta, fee_bps);
			let expected_miner_fee = total_fee - total_fee / 2;
			assert!(expected_miner_fee > 0, "fixture should produce a non-zero miner fee");

			let author_balance_before = Balances::free_balance(expected_author.clone());

			assert_ok!(Wormhole::verify_private_batch(
				RawOrigin::None.into(),
				get_test_proof_bytes()
			));

			System::assert_has_event(
				crate::Event::<Test>::MinerVolumeFeePaid {
					miner: expected_author.clone(),
					amount: expected_miner_fee,
				}
				.into(),
			);
			assert_eq!(
				Balances::free_balance(expected_author),
				author_balance_before + expected_miner_fee
			);
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
		use qp_wormhole_aggregator::private_batch::prover::PrivateBatchProver;
		use qp_wormhole_circuit::{
			block_header::header::HeaderInputs,
			inputs::{CircuitInputs, PrivateCircuitInputs},
			nullifier::Nullifier,
			unspendable_account::UnspendableAccount,
		};
		use qp_wormhole_inputs::{BytesDigest, PublicCircuitInputs};
		use qp_zk_circuits_common::utils::digest_to_bytes;

		// Create test inputs with real block header validation
		let secret: BytesDigest = BytesDigest::new_unchecked([42u8; 32]); // Well-known test secret
		let transfer_count = 1u64;
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
				volume_fee_bps: 4,
				nullifier,
				exit_account_1: exit_account,
				exit_account_2: BytesDigest::default(),
				block_hash,
				block_number,
			},
			private: PrivateCircuitInputs {
				secret: secret.into(),
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

		// Generate leaf proof (leaf prover is always built from the canonical config;
		// it no longer loads prover.bin).
		println!("Generating leaf proof...");
		let leaf_proof = qp_wormhole_prover::build_fresh()
			.commit(&inputs)
			.expect("Failed to commit leaf inputs")
			.prove()
			.expect("Failed to prove leaf");

		// Aggregate (with dummy padding to fill the private batch)
		println!("Aggregating proof into a private batch...");
		let prover = PrivateBatchProver::new_from_binaries_dir(bins_dir)
			.expect("Failed to create private-batch prover");
		let aggregated_proof =
			prover.aggregate(vec![leaf_proof]).expect("Failed to aggregate private batch");

		// Cryptographic verification of the fixture happens in the pallet tests that
		// load the hex (and in regenerate_public_batch_fixture via PublicBatchAggregator).
		// We skip a local WormholeVerifier check here: aggregator and verifier crates
		// currently expose distinct plonky2 ProofWithPublicInputs types in this workspace.
		aggregated_proof
	}

	/// Helper to compute ZK leaf hash (must match circuit computation).
	///
	/// Also used by `pallet_leaf_hash_matches_circuit_leaf_hash` as the circuit-side
	/// reference to pin the pallet's `hash_leaf` encoding.
	pub fn compute_zk_leaf_hash(
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

/// Tests for the defense-in-depth profile checks applied when loading the
/// build-time-generated batch verifier artifacts (`ensure_batch_verifier_profile`).
///
/// The batch loader can't pin artifacts to keccak256 commitments the way the
/// canonical-leaf `WormholeVerifier::new_from_bytes` does (the batch bytes vary
/// with the `QP_NUM_*` sizing), so these tests assert that the config/security-bits/
/// PI-shape checks it enforces instead both accept the real artifacts and reject
/// doctored profiles.
#[cfg(test)]
mod verifier_profile_tests {
	use qp_wormhole_verifier::MIN_LEAF_SECURITY_BITS;

	#[test]
	fn embedded_batch_verifiers_pass_profile_checks() {
		// The lazy statics run the full loader, so these unwraps prove the real
		// build artifacts satisfy the profile checks end to end.
		let private = crate::get_private_batch_verifier()
			.expect("private-batch verifier must load under the profile checks");
		let public = crate::get_public_batch_verifier()
			.expect("public-batch verifier must load under the profile checks");

		// And the expected-PI formulas match the actual compiled circuits.
		assert_eq!(
			private.circuit_data.common.num_public_inputs,
			crate::private_batch_expected_public_inputs(),
		);
		assert_eq!(
			public.circuit_data.common.num_public_inputs,
			crate::public_batch_expected_public_inputs(),
		);
	}

	#[test]
	fn batch_configs_match_circuit_crate() {
		// The expected configs are replicated in the pallet because
		// qp-zk-circuits-common can't be a runtime dependency (it force-enables
		// qp-plonky2/std). Assert parity with the source of truth the build-time
		// circuit generation actually uses.
		assert_eq!(
			crate::private_batch_expected_config(),
			qp_zk_circuits_common::circuit::wormhole_private_batch_circuit_config(),
		);
		assert_eq!(
			crate::public_batch_expected_config(),
			qp_zk_circuits_common::circuit::wormhole_public_batch_circuit_config(),
		);
	}

	#[test]
	fn profile_check_rejects_wrong_public_input_count() {
		let common = crate::get_private_batch_verifier().unwrap().circuit_data.common.clone();
		let config = crate::private_batch_expected_config();
		let expected = crate::private_batch_expected_public_inputs();

		assert!(crate::ensure_batch_verifier_profile(&common, &config, expected).is_ok());
		assert!(
			crate::ensure_batch_verifier_profile(&common, &config, expected + 1).is_err(),
			"a PI-count mismatch must be rejected"
		);
	}

	#[test]
	fn profile_check_rejects_non_canonical_config() {
		let mut common = crate::get_private_batch_verifier().unwrap().circuit_data.common.clone();
		let config = crate::private_batch_expected_config();
		let expected = crate::private_batch_expected_public_inputs();

		// Any deviation from the canonical batch config must be rejected, e.g. an
		// artifact built with the zero-knowledge (row blinding) flag flipped.
		common.config.zero_knowledge = !common.config.zero_knowledge;
		assert!(crate::ensure_batch_verifier_profile(&common, &config, expected).is_err());
	}

	#[test]
	fn profile_check_rejects_low_security_bits() {
		let mut common = crate::get_private_batch_verifier().unwrap().circuit_data.common.clone();
		let expected = crate::private_batch_expected_public_inputs();

		common.config.security_bits = MIN_LEAF_SECURITY_BITS - 1;
		// Weaken the expected config the same way, so this exercises the
		// security-bits floor specifically rather than the equality check.
		let weak_config = common.config.clone();
		assert!(
			crate::ensure_batch_verifier_profile(&common, &weak_config, expected).is_err(),
			"a config below the security-bits floor must be rejected even if it matches"
		);
	}

	#[test]
	fn canonical_configs_meet_security_floor() {
		// Guards against the canonical batch configs themselves dropping below the
		// floor in a future qp-plonky2 bump (mirrors the upstream leaf check).
		assert!(crate::private_batch_expected_config().security_bits >= MIN_LEAF_SECURITY_BITS);
		assert!(crate::public_batch_expected_config().security_bits >= MIN_LEAF_SECURITY_BITS);
	}
}

/// Unit tests driving `segment_validity` / `process_exit_bundle` directly with
/// synthetic multi-segment bundles.
///
/// The real public-batch fixture contains exactly one real segment, so it cannot
/// exercise the partial-denial machinery. These tests cover what the fixture can't:
/// denying one segment while the rest execute (`SegmentsDenied` accounting), the
/// cross-segment claimed-set dedup (the double-mint fix for including the same
/// private batch twice in one bundle), and fee/rebate math when `total_exit_amount`
/// excludes a denied segment's value.
#[cfg(test)]
mod exit_bundle_tests {
	use crate::{
		mock::*,
		pallet::{Error, ExitBundle, ExitSegment, UsedNullifiers},
	};
	use frame_support::{
		assert_ok,
		traits::fungible::{Inspect, Mutate},
	};
	use qp_wormhole_verifier::{BlockData, BytesDigest, PublicInputsByAccount};
	use sp_core::crypto::AccountId32;
	use sp_runtime::Permill;

	/// Quantized circuit amounts (2 decimals). 2000 => 20 QUAN on-chain.
	const AMOUNT_A: u32 = 2000;
	const AMOUNT_B: u32 = 3000;

	fn digest(byte: u8) -> BytesDigest {
		BytesDigest::new_unchecked([byte; 32])
	}

	fn nullifier_bytes(byte: u8) -> [u8; 32] {
		[byte; 32]
	}

	fn scaled(amount: u32) -> u128 {
		(amount as u128) * crate::SCALE_DOWN_FACTOR
	}

	/// Build a segment from (nullifier byte-pattern) and (exit account byte-pattern, amount)
	/// lists. A byte of 0 produces a zero nullifier / dummy exit slot.
	fn segment(nullifiers: &[u8], exits: &[(u8, u32)]) -> ExitSegment {
		ExitSegment {
			account_data: exits
				.iter()
				.map(|(account_byte, amount)| PublicInputsByAccount {
					summed_output_amount: *amount,
					exit_account: digest(*account_byte),
				})
				.collect(),
			nullifiers: nullifiers.iter().map(|b| digest(*b)).collect(),
		}
	}

	fn bundle(segments: Vec<ExitSegment>, aggregator: Option<BytesDigest>) -> ExitBundle {
		ExitBundle {
			asset_id: 0,
			volume_fee_bps: VolumeFeeRateBps::get(),
			block_data: BlockData::default(),
			aggregator_address: aggregator,
			segments,
		}
	}

	#[test]
	fn segment_validity_denies_only_segment_with_used_nullifier() {
		new_test_ext().execute_with(|| {
			UsedNullifiers::<Test>::insert(nullifier_bytes(2), true);

			let b = bundle(
				vec![segment(&[1], &[(10, AMOUNT_A)]), segment(&[2], &[(11, AMOUNT_B)])],
				None,
			);
			let validity = Wormhole::segment_validity(&b).unwrap();
			assert_eq!(validity, vec![true, false]);
		});
	}

	#[test]
	fn segment_validity_cross_segment_dedup_denies_second_claim() {
		new_test_ext().execute_with(|| {
			// Segment 1 shares nullifier 2 with segment 0 (the double-spend attempt).
			// Segment 2 reuses nullifier 3, which segment 1 tried to claim — but a
			// denied segment claims nothing, so segment 2 must stay valid.
			let b = bundle(
				vec![
					segment(&[1, 2], &[(10, AMOUNT_A)]),
					segment(&[2, 3], &[(11, AMOUNT_B)]),
					segment(&[3], &[(12, AMOUNT_A)]),
				],
				None,
			);
			let validity = Wormhole::segment_validity(&b).unwrap();
			assert_eq!(validity, vec![true, false, true]);
		});
	}

	#[test]
	fn segment_validity_denies_duplicate_of_same_private_batch() {
		new_test_ext().execute_with(|| {
			// The same private batch included twice in one bundle: only the first
			// copy may be valid.
			let seg = || segment(&[1, 2], &[(10, AMOUNT_A)]);
			let validity = Wormhole::segment_validity(&bundle(vec![seg(), seg()], None)).unwrap();
			assert_eq!(validity, vec![true, false]);
		});
	}

	#[test]
	fn segment_validity_zero_nullifiers_are_exempt_from_collision_checks() {
		new_test_ext().execute_with(|| {
			// Segment 0 is dummy padding (all-zero): invalid but inert.
			// Segments 1 and 2 each contain a zero nullifier (dummy leaf inside a
			// real private batch); the shared zeros must not collide.
			let b = bundle(
				vec![
					segment(&[0, 0], &[(0, 0)]),
					segment(&[0, 1], &[(10, AMOUNT_A)]),
					segment(&[0, 2], &[(11, AMOUNT_B)]),
				],
				None,
			);
			let validity = Wormhole::segment_validity(&b).unwrap();
			assert_eq!(validity, vec![false, true, true]);
		});
	}

	#[test]
	fn process_exit_bundle_partial_denial_mints_only_valid_segments() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Segment 1 has one already-spent nullifier (3) and one fresh one (4):
			// the whole segment is denied and the fresh nullifier left unmarked.
			UsedNullifiers::<Test>::insert(nullifier_bytes(3), true);

			let exit_a = AccountId32::new([10u8; 32]);
			let exit_b = AccountId32::new([11u8; 32]);

			let b = bundle(
				vec![segment(&[1, 2], &[(10, AMOUNT_A)]), segment(&[3, 4], &[(11, AMOUNT_B)])],
				None,
			);
			assert_ok!(Wormhole::process_exit_bundle(b));

			// Only the valid segment minted; the denied segment's value is excluded
			// from the exit accounting.
			assert_eq!(Balances::balance(&exit_a), scaled(AMOUNT_A));
			assert_eq!(Balances::balance(&exit_b), 0, "denied segment must not mint");

			// Valid segment's nullifiers are consumed; the denied segment's fresh
			// nullifier is not, so its owner can still exit later.
			assert!(UsedNullifiers::<Test>::contains_key(nullifier_bytes(1)));
			assert!(UsedNullifiers::<Test>::contains_key(nullifier_bytes(2)));
			assert!(
				!UsedNullifiers::<Test>::contains_key(nullifier_bytes(4)),
				"denied segment's nullifiers must not be consumed"
			);

			System::assert_has_event(
				crate::Event::<Test>::SegmentsDenied { indices: vec![1] }.into(),
			);
			System::assert_has_event(
				crate::Event::<Test>::ProofVerified {
					exit_amount: scaled(AMOUNT_A),
					nullifiers: vec![nullifier_bytes(1), nullifier_bytes(2)],
				}
				.into(),
			);
		});
	}

	#[test]
	fn process_exit_bundle_same_private_batch_twice_mints_once() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let exit = AccountId32::new([10u8; 32]);
			let seg = || segment(&[1, 2], &[(10, AMOUNT_A)]);
			assert_ok!(Wormhole::process_exit_bundle(bundle(vec![seg(), seg()], None)));

			assert_eq!(
				Balances::balance(&exit),
				scaled(AMOUNT_A),
				"duplicate segment in one bundle must not double-mint"
			);
			System::assert_has_event(
				crate::Event::<Test>::SegmentsDenied { indices: vec![1] }.into(),
			);
		});
	}

	#[test]
	fn process_exit_bundle_fee_and_rebate_exclude_denied_segment() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Seed issuance so the burn is observable (set_total_issuance saturates at 0).
			assert_ok!(Balances::mint_into(&account_id(999), 1_000 * UNIT));
			let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

			// Deny segment 1 via an already-used nullifier.
			UsedNullifiers::<Test>::insert(nullifier_bytes(3), true);

			let aggregator = AccountId32::new([7u8; 32]);
			let b = bundle(
				vec![segment(&[1], &[(10, AMOUNT_A)]), segment(&[3], &[(11, AMOUNT_B)])],
				Some(digest(7)),
			);
			assert_ok!(Wormhole::process_exit_bundle(b));

			// The quantized-ceiling volume fee, computed on the total that EXCLUDES
			// the denied segment's value.
			let fee_bps = VolumeFeeRateBps::get() as u128;
			let fee = super::ceil_volume_fee(AMOUNT_A as u128, fee_bps);
			let fee_if_denied_included =
				super::ceil_volume_fee((AMOUNT_A + AMOUNT_B) as u128, fee_bps);
			assert_ne!(fee, fee_if_denied_included, "test must distinguish the two totals");

			let burn_bucket = Permill::from_percent(50) * fee;
			let expected_rebate = Permill::from_percent(50) * burn_bucket;
			assert!(expected_rebate > 0, "amounts must produce a nonzero rebate");
			assert_eq!(
				Balances::balance(&aggregator),
				expected_rebate,
				"aggregator rebate must be based on the valid segments only"
			);

			// No block author in tests, so the miner share is burned too. Issuance
			// drops by (burn_bucket - rebate) + (fee - burn_bucket) = fee - rebate.
			let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(
				issuance_before - issuance_after,
				fee - expected_rebate,
				"burn must be computed from the fee excluding the denied segment"
			);
		});
	}

	#[test]
	fn process_exit_bundle_rejects_when_no_segment_valid() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			UsedNullifiers::<Test>::insert(nullifier_bytes(1), true);

			let b = bundle(vec![segment(&[1], &[(10, AMOUNT_A)])], None);
			let result = Wormhole::process_exit_bundle(b);
			assert!(result.is_err());
			assert_eq!(result.unwrap_err().error, Error::<Test>::NullifierAlreadyUsed.into());
		});
	}

	#[test]
	fn process_exit_bundle_rejects_all_dummy_bundle_with_no_valid_segments() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// A bundle of only dummy padding segments carries no replayed nullifier,
			// so it must be reported as NoValidSegments, not NullifierAlreadyUsed.
			let b = bundle(vec![segment(&[0, 0], &[(0, 0)]), segment(&[0], &[(0, 0)])], None);
			let result = Wormhole::process_exit_bundle(b);
			assert!(result.is_err());
			assert_eq!(result.unwrap_err().error, Error::<Test>::NoValidSegments.into());
		});
	}

	#[test]
	fn process_exit_bundle_skips_below_ed_exit_without_reverting_others() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Raise the ED so a small exit to a fresh account cannot be minted, while a
			// larger co-bundled exit clears it. AMOUNT_A (20 QUAN) stays below the ED;
			// AMOUNT_B (30 QUAN) is above it.
			ExistentialDeposit::set(scaled(2500));

			let dust_exit = AccountId32::new([10u8; 32]);
			let good_exit = AccountId32::new([11u8; 32]);

			// Two independent segments (as in a public batch): a dust-to-fresh-account
			// exit and an honest above-ED exit. The dust one must be skipped, not abort
			// the whole bundle.
			let b = bundle(
				vec![segment(&[1], &[(10, AMOUNT_A)]), segment(&[2], &[(11, AMOUNT_B)])],
				None,
			);
			assert_ok!(Wormhole::process_exit_bundle(b));

			// The honest exit landed; the below-ED exit was skipped (account not created).
			assert_eq!(Balances::balance(&good_exit), scaled(AMOUNT_B));
			assert_eq!(
				Balances::balance(&dust_exit),
				0,
				"below-ED exit must be skipped, not create the account"
			);

			// A skip event was surfaced for the dust exit.
			System::assert_has_event(
				crate::Event::<Test>::ExitMintFailed {
					account: dust_exit.clone(),
					amount: scaled(AMOUNT_A),
				}
				.into(),
			);

			// Both segments' nullifiers are marked used: the skipped exit's deposit
			// cannot be re-exited (isolation, not a free retry).
			assert!(UsedNullifiers::<Test>::contains_key(nullifier_bytes(1)));
			assert!(UsedNullifiers::<Test>::contains_key(nullifier_bytes(2)));
		});
	}

	#[test]
	fn process_exit_bundle_settles_fee_and_event_from_minted_exits_only() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Seed issuance so the burn is observable.
			assert_ok!(Balances::mint_into(&account_id(999), 1_000 * UNIT));
			let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

			// Raise the ED so the AMOUNT_A exit to a fresh account fails and is
			// skipped, while the AMOUNT_B exit clears it (same setup as the
			// skip-below-ED test).
			ExistentialDeposit::set(scaled(2500));

			let b = bundle(
				vec![segment(&[1], &[(10, AMOUNT_A)]), segment(&[2], &[(11, AMOUNT_B)])],
				None,
			);
			assert_ok!(Wormhole::process_exit_bundle(b));

			let fee_bps = VolumeFeeRateBps::get() as u128;
			let fee_minted = super::ceil_volume_fee(AMOUNT_B as u128, fee_bps);
			let fee_attempted =
				super::ceil_volume_fee((AMOUNT_A + AMOUNT_B) as u128, fee_bps);
			assert_ne!(fee_minted, fee_attempted);

			let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(issuance_before - issuance_after, fee_minted);
			System::assert_has_event(
				crate::Event::<Test>::ProofVerified {
					exit_amount: scaled(AMOUNT_B),
					nullifiers: vec![nullifier_bytes(1), nullifier_bytes(2)],
				}
				.into(),
			);
		});
	}

	#[test]
	fn process_exit_bundle_settles_the_one_quantum_minimum_fee() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Seed issuance so the burn is observable (set_total_issuance saturates at 0).
			assert_ok!(Balances::mint_into(&account_id(999), 1_000 * UNIT));
			let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

			// Smallest valid exit: one quantum (0.01 QUAN). The circuit's integer fee
			// relation `out · 10000 ≤ input · (10000 − bps)` forces `input ≥ 2` quanta
			// here, i.e. the proof locked a full one-quantum fee. Settlement must
			// collect that quantum, not the ~0.04% of it (10^10 · 4 / 9996 = 4_001_600
			// base units) that truncating base-unit division yields.
			assert_ok!(Wormhole::process_exit_bundle(bundle(
				vec![segment(&[1], &[(10, 1)])],
				None
			)));

			// No aggregator and no block author, so the entire fee is burned.
			let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(
				issuance_before - issuance_after,
				crate::SCALE_DOWN_FACTOR,
				"a one-quantum exit must settle the full one-quantum minimum fee"
			);
		});
	}

	#[test]
	fn process_exit_bundle_rounds_the_volume_fee_up_to_whole_quanta() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Seed issuance so the burn is observable.
			assert_ok!(Balances::mint_into(&account_id(999), 1_000 * UNIT));
			let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

			// 5000 quanta at 4 bps: 5000 · 4 / 9996 = 2.0008… quanta, which the
			// quantized-ceiling rule settles as 3 whole quanta. Base-unit floor
			// division would settle 20_008_003_201 instead.
			let exit_quanta = 5_000u32;
			assert_ok!(Wormhole::process_exit_bundle(bundle(
				vec![segment(&[1], &[(10, exit_quanta)])],
				None
			)));

			let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(
				issuance_before - issuance_after,
				3 * crate::SCALE_DOWN_FACTOR,
				"the volume fee must be rounded up to whole quanta"
			);
		});
	}

	#[test]
	fn process_exit_bundle_burns_rebate_when_aggregator_mint_fails() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Seed issuance so the burn is observable.
			assert_ok!(Balances::mint_into(&account_id(999), 1_000 * UNIT));
			let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

			// Raise the ED above the rebate: minting the rebate into the nonexistent
			// aggregator account now fails. The bundle (users' exits) must still
			// succeed, with the rebate burned instead.
			ExistentialDeposit::set(1_000 * UNIT);

			let aggregator = AccountId32::new([7u8; 32]);
			let exit = AccountId32::new([10u8; 32]);
			// Exits must clear the raised ED so the user mints themselves succeed.
			let amount = 200_000u32; // 2000 QUAN scaled
			let b = bundle(vec![segment(&[1], &[(10, amount)])], Some(digest(7)));
			assert_ok!(Wormhole::process_exit_bundle(b));

			// User exit minted; aggregator got nothing.
			assert_eq!(Balances::balance(&exit), scaled(amount));
			assert_eq!(
				Balances::balance(&aggregator),
				0,
				"below-ED rebate must not create the aggregator account"
			);

			// The whole fee is burned: the rebate fell back into the burn bucket and
			// the miner share is burned too (no block author in tests).
			let fee_bps = VolumeFeeRateBps::get() as u128;
			let fee = super::ceil_volume_fee(amount as u128, fee_bps);
			let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();
			assert_eq!(
				issuance_before - issuance_after,
				fee,
				"failed rebate must be burned, not revert the bundle"
			);
		});
	}
}

/// Tests for public-batch proof verification (second aggregation layer).
#[cfg(test)]
mod public_batch_proof_tests {
	use crate::{
		mock::*,
		pallet::{Error, UsedNullifiers},
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
	}

	/// Public-batch twin of the private-batch exact-framing test: trailing bytes after
	/// a valid proof are silently ignored by the plonky2 parser, so they must be
	/// rejected by the canonical-encoding check.
	#[test]
	fn pre_validation_rejects_padded_proof_bytes() {
		new_test_ext().execute_with(|| {
			setup_matching_block_state(&parse_test_inputs());

			assert!(Wormhole::pre_validate_public_batch_proof(&get_test_proof_bytes()).is_ok());

			let mut padded = get_test_proof_bytes();
			padded.extend_from_slice(&[0u8; 32]);
			assert!(matches!(
				Wormhole::pre_validate_public_batch_proof(&padded),
				Err(Error::<Test>::NonCanonicalProofEncoding)
			));
		});
	}

	#[test]
	fn test_parse_public_batch_public_inputs_succeeds() {
		let inputs = parse_test_inputs();

		assert_eq!(inputs.asset_id, 0, "Asset ID should be native (0)");
		assert_eq!(inputs.volume_fee_bps, 4, "Volume fee should be 4 bps");
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

			// Expected exit total (in quanta) from the proof's public inputs (dummy
			// slots are zero).
			let exit_quanta: u128 = inputs
				.account_data
				.iter()
				.filter(|a| a.summed_output_amount > 0)
				.map(|a| a.summed_output_amount as u128)
				.sum();

			assert_ok!(Wormhole::verify_public_batch(
				RawOrigin::None.into(),
				get_test_proof_bytes()
			));

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

			// Aggregator rebate: quantized-ceiling volume fee, burn bucket = 50% of
			// fee, and VolumeFeesAggregatorRate (50%) of that goes to the aggregator.
			let fee_bps = VolumeFeeRateBps::get() as u128;
			let total_fee = super::ceil_volume_fee(exit_quanta, fee_bps);
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

	/// The aggregator rebate is deliberately permissionless: whoever performs the public-batch
	/// aggregation names its own payout address as a proof public input. The property that
	/// makes this safe is that the address is *bound* by the proof — a third party cannot take
	/// someone else's public batch and redirect the rebate to itself, because mutating the
	/// aggregator-address public inputs invalidates the proof, and `pre_dispatch` (the
	/// block-inclusion gate) runs full ZK verification.
	#[test]
	fn pre_dispatch_rejects_public_batch_with_redirected_aggregator_address() {
		use frame_support::pallet_prelude::ValidateUnsigned;
		use qp_plonky2_verifier::field::types::Field;

		new_test_ext().execute_with(|| {
			let inputs = parse_test_inputs();
			setup_matching_block_state(&inputs);

			// The genuine proof passes the block-inclusion gate.
			let original = get_test_proof_bytes();
			let call = crate::Call::<Test>::verify_public_batch { proof_bytes: original.clone() };
			assert!(
				<Wormhole as ValidateUnsigned>::pre_dispatch(&call).is_ok(),
				"the untampered fixture must pass pre_dispatch"
			);

			// An attacker rewrites the aggregator-address public inputs (the first 4 felts
			// of the public-batch PI layout) to point at an account they control.
			let mut tampered_proof = deserialize_test_proof();
			for felt in tampered_proof.public_inputs.iter_mut().take(4) {
				*felt = F::from_canonical_u32(0x42);
			}
			let tampered_bytes = tampered_proof.to_bytes();
			assert_ne!(tampered_bytes, original, "mutation must change the encoded proof");

			// The redirected address round-trips through parsing (i.e. the tampering is
			// well-formed at the PI level) ...
			let tampered_deser = ProofWithPublicInputs::<F, C, D>::from_bytes(
				tampered_bytes.clone(),
				&crate::get_public_batch_verifier().unwrap().circuit_data.common,
			)
			.expect("tampered PIs still deserialize");
			let tampered_inputs = parse_public_batch_public_inputs(
				&tampered_deser,
				crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
				crate::circuit_config::NUM_LEAF_PROOFS,
			)
			.expect("tampered PIs still parse");
			assert_ne!(
				tampered_inputs.aggregator_address.as_ref(),
				&AGGREGATOR_ADDRESS,
				"the payout address was redirected"
			);

			// ... but the proof no longer verifies, so the block-inclusion gate rejects it:
			// the rebate cannot be stolen off an existing proof.
			let tampered_call =
				crate::Call::<Test>::verify_public_batch { proof_bytes: tampered_bytes };
			assert!(
				<Wormhole as ValidateUnsigned>::pre_dispatch(&tampered_call).is_err(),
				"pre_dispatch must reject a proof whose aggregator address was redirected"
			);
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
		use qp_wormhole_aggregator::aggregator::PublicBatchAggregator;
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
		// BatchKey is derived from the proof's PI on push; pass it back to select the bucket.
		let batch_key = aggregator.push_proof(private_batch_proof).expect("Failed to push proof");
		let public_batch_proof = aggregator.aggregate(&batch_key).expect("Failed to aggregate");

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

/// Timing harness for `weights.rs` pre-validation constants.
/// `cargo test -p pallet-wormhole --release measure_pre_validation_time -- --ignored --nocapture`
#[cfg(test)]
mod pre_validation_timing {
	use crate::{
		bench_fixtures::{insert_decoy_nullifiers, worst_case_bundle},
		mock::*,
	};
	use qp_wormhole_verifier::{parse_private_batch_public_inputs, ProofWithPublicInputs, C, D, F};
	use sp_core::H256;
	use std::time::Instant;

	const PRIVATE_BATCH_PROOF_HEX: &str = include_str!("../test-data/private_batch.hex");
	const PUBLIC_BATCH_PROOF_HEX: &str = include_str!("../test-data/public_batch.hex");

	fn insert_block_hash(block_number: u32, block_hash: &[u8]) {
		let bytes: [u8; 32] = block_hash.try_into().unwrap();
		frame_system::BlockHash::<Test>::insert(block_number as u64, H256::from(bytes));
	}

	#[test]
	#[ignore] // manual calibration harness, not a correctness test
	fn measure_pre_validation_time() {
		new_test_ext().execute_with(|| {
			let iters: u32 = 50;

			// --- Private batch ---
			let proof_bytes =
				hex::decode(PRIVATE_BATCH_PROOF_HEX.trim()).expect("Invalid private hex");
			let verifier = crate::get_private_batch_verifier().unwrap();
			let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.clone(),
				&verifier.circuit_data.common,
			)
			.unwrap();
			let inputs = parse_private_batch_public_inputs(&proof).unwrap();
			insert_block_hash(
				inputs.block_data.block_number,
				inputs.block_data.block_hash.as_ref(),
			);
			insert_decoy_nullifiers::<Test>(crate::circuit_config::NUM_LEAF_PROOFS as u32);

			let t = Instant::now();
			for _ in 0..iters {
				let _ = ProofWithPublicInputs::<F, C, D>::from_bytes(
					proof_bytes.clone(),
					&verifier.circuit_data.common,
				)
				.unwrap();
			}
			println!("private deserialize-only: {:?}/iter", t.elapsed() / iters);

			let t = Instant::now();
			for _ in 0..iters {
				assert!(Wormhole::pre_validate_private_batch_proof(&proof_bytes).is_ok());
			}
			println!("private full pre-validate: {:?}/iter", t.elapsed() / iters);

			let worst = worst_case_bundle::<Test>(inputs.block_data.clone(), 1);
			let t = Instant::now();
			for _ in 0..iters {
				assert!(Wormhole::pre_validate_private_batch_proof(&proof_bytes).is_ok());
				assert!(Wormhole::validate_exit_bundle_common(&worst).is_ok());
			}
			println!("private pre-validate + worst-case segments: {:?}/iter", t.elapsed() / iters);

			// --- Public batch ---
			let proof_bytes =
				hex::decode(PUBLIC_BATCH_PROOF_HEX.trim()).expect("Invalid public hex");
			let verifier = crate::get_public_batch_verifier().unwrap();
			let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.clone(),
				&verifier.circuit_data.common,
			)
			.unwrap();
			let inputs = qp_wormhole_verifier::parse_public_batch_public_inputs(
				&proof,
				crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
				crate::circuit_config::NUM_LEAF_PROOFS,
			)
			.unwrap();
			insert_block_hash(
				inputs.block_data.block_number,
				inputs.block_data.block_hash.as_ref(),
			);
			insert_decoy_nullifiers::<Test>(
				(crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS *
					crate::circuit_config::NUM_LEAF_PROOFS) as u32,
			);

			let t = Instant::now();
			for _ in 0..iters {
				let _ = ProofWithPublicInputs::<F, C, D>::from_bytes(
					proof_bytes.clone(),
					&verifier.circuit_data.common,
				)
				.unwrap();
			}
			println!("public deserialize-only: {:?}/iter", t.elapsed() / iters);

			let t = Instant::now();
			for _ in 0..iters {
				assert!(Wormhole::pre_validate_public_batch_proof(&proof_bytes).is_ok());
			}
			println!("public full pre-validate: {:?}/iter", t.elapsed() / iters);

			let worst = worst_case_bundle::<Test>(
				inputs.block_data.clone(),
				crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
			);
			let t = Instant::now();
			for _ in 0..iters {
				assert!(Wormhole::pre_validate_public_batch_proof(&proof_bytes).is_ok());
				assert!(Wormhole::validate_exit_bundle_common(&worst).is_ok());
			}
			println!("public pre-validate + worst-case segments: {:?}/iter", t.elapsed() / iters);
		});
	}
}
