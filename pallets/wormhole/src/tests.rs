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
			Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount);

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
			Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount);
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
			Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount);

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
			Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount);

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
