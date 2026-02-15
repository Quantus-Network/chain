#[cfg(test)]
mod wormhole_tests {
	use crate::mock::*;
	use frame_support::{
		assert_ok,
		traits::fungible::{Inspect, Mutate},
	};

	#[test]
	fn record_transfer_creates_proof_and_increments_count() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			let amount = 10 * UNIT;

			let count_before = Wormhole::transfer_count(&bob);
			assert_ok!(Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount));

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
			assert_ok!(Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount));
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
			assert_ok!(Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount));

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
			assert_ok!(Wormhole::record_transfer(asset_id, alice.clone(), bob.clone(), amount));

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
			assert_ok!(Wormhole::record_transfer(0u32, alice.clone(), bob.clone(), amount));

			assert_eq!(Balances::balance(&alice), amount);
			assert_eq!(Balances::balance(&bob), amount);
			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);
			assert!(Wormhole::transfer_proof((0u32, count_before, alice, bob, amount)).is_some());
		});
	}
}
