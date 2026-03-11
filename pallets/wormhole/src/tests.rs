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
	use qp_wormhole::derive_wormhole_account;
	use sp_core::crypto::AccountId32;

	#[test]
	fn record_transfer_creates_proof_and_increments_count() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let amount = 10 * UNIT;

			let count_before = Wormhole::transfer_count(&bob);
			Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount);

			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);
			assert!(Wormhole::transfer_proof((
				0u32,
				count_before,
				alice.clone(),
				bob.clone(),
				amount
			))
			.is_some());

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
	fn record_transfer_emits_asset_transferred_event() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let asset_id = 1u32;
			let amount = 10 * UNIT;

			System::set_block_number(1);
			Wormhole::record_transfer(asset_id, alice.clone(), bob.clone(), amount);

			System::assert_last_event(
				crate::Event::<Test>::AssetTransferred {
					asset_id,
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
			assert!(Wormhole::transfer_proof((0u32, count_before, alice, bob, amount)).is_some());
		});
	}

	#[test]
	fn known_preimage_to_wormhole_address_all_zeros() {
		// Test vector: all zeros preimage
		let preimage = [0u8; 32];
		let address = derive_wormhole_account(preimage);

		// This is the expected wormhole address for preimage [0; 32]
		// Computed via: PoseidonHasher::hash_variable_length(preimage.to_felts())
		// SS58: 5GE628zL...
		let expected_bytes: [u8; 32] = [
			0xb8, 0x18, 0xc0, 0x2c, 0x58, 0x77, 0xcc, 0x44, 0x07, 0xf7, 0x1b, 0x9b, 0x34, 0xee,
			0x45, 0xc7, 0x99, 0x86, 0xa5, 0xaf, 0x12, 0x9b, 0xfd, 0xc9, 0xe7, 0x71, 0x51, 0x1f,
			0xb4, 0xd5, 0x20, 0x4f,
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
		// Computed via: PoseidonHasher::hash_variable_length(preimage.to_felts())
		// SS58: 5CRs5z8R...
		let expected_bytes: [u8; 32] = [
			0x10, 0x23, 0x39, 0x1d, 0x9b, 0xe8, 0xa3, 0x3b, 0xc5, 0xfa, 0x49, 0x65, 0xf6, 0xde,
			0x83, 0x36, 0xd5, 0xb2, 0x97, 0x2b, 0xe4, 0x95, 0x73, 0xca, 0x74, 0xf4, 0x55, 0xc8,
			0x19, 0x98, 0xa9, 0x97,
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
		// Computed via: PoseidonHasher::hash_variable_length(preimage.to_felts())
		// SS58: 5CZ8wxNm...
		let expected_bytes: [u8; 32] = [
			0x15, 0xaf, 0x55, 0xee, 0x62, 0xfd, 0xd5, 0xea, 0x01, 0x4a, 0x59, 0x74, 0x24, 0xe7,
			0xe5, 0xdc, 0x68, 0xd6, 0x82, 0xfd, 0x48, 0x0d, 0xf2, 0x50, 0x40, 0x1f, 0xa2, 0x15,
			0x85, 0x22, 0xec, 0xff,
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
