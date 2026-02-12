#[cfg(test)]
mod wormhole_tests {
	use crate::mock::*;
	use frame_support::{
		assert_ok,
		traits::fungible::{Inspect, Mutate},
	};

	#[test]
	fn transfer_native_works() {
		new_test_ext().execute_with(|| {
			let alice = account_id(1);
			let bob = account_id(2);
			// Minimum transfer amount is 10 QUAN
			let amount = 10 * UNIT;

			assert_ok!(Balances::mint_into(&alice, amount * 2));

			let count_before = Wormhole::transfer_count(&bob);
			assert_ok!(Wormhole::transfer_native(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				bob.clone(),
				amount,
			));

			assert_eq!(Balances::balance(&alice), amount);
			assert_eq!(Balances::balance(&bob), amount);
			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);
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
			// Minimum transfer amount is 10 QUAN
			let amount = 10 * UNIT;

			// Need enough balance for asset creation deposit
			assert_ok!(Balances::mint_into(&alice, 10 * UNIT));
			assert_ok!(Balances::mint_into(&bob, 10 * UNIT));

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

			let count_before = Wormhole::transfer_count(&bob);
			assert_ok!(Wormhole::transfer_asset(
				frame_system::RawOrigin::Signed(alice.clone()).into(),
				asset_id,
				bob.clone(),
				amount,
			));

			assert_eq!(Assets::balance(asset_id, &alice), amount);
			assert_eq!(Assets::balance(asset_id, &bob), amount);
			assert_eq!(Wormhole::transfer_count(&bob), count_before + 1);
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
			let amount = 10 * UNIT;

			assert_ok!(Balances::mint_into(&alice, 10 * UNIT));

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
