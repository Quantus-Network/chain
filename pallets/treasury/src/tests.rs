use crate::{
	mock::{
		account_id, new_test_ext, new_test_ext_with_treasury, new_test_ext_without_treasury, Test,
		Treasury,
	},
	pallet::{TreasuryAccount, TreasuryPortion},
	Error, Event, TreasuryProvider,
};
use frame_support::{assert_err, assert_ok};
use frame_system::Pallet as System;

#[test]
fn genesis_sets_treasury_config() {
	new_test_ext().execute_with(|| {
		// Explicit check: both storages must be populated by genesis
		assert!(TreasuryAccount::<Test>::get().is_some(), "TreasuryAccount must be set in genesis");
		assert!(TreasuryPortion::<Test>::get().is_some(), "TreasuryPortion must be set in genesis");
		assert_eq!(Treasury::account_id(), account_id(1));
		assert_eq!(Treasury::portion(), sp_runtime::Permill::from_percent(50));
	});
}

#[test]
fn set_treasury_account_works() {
	new_test_ext().execute_with(|| {
		let old_account = Treasury::account_id();
		assert_ok!(Treasury::set_treasury_account(
			frame_system::RawOrigin::Root.into(),
			account_id(99)
		));
		assert_eq!(Treasury::account_id(), account_id(99));
		System::<Test>::assert_has_event(
			Event::<Test>::TreasuryAccountUpdated {
				old_account: Some(old_account),
				new_account: account_id(99),
			}
			.into(),
		);
	});
}

#[test]
fn set_treasury_account_requires_root() {
	new_test_ext().execute_with(|| {
		assert_err!(
			Treasury::set_treasury_account(
				frame_system::RawOrigin::Signed(account_id(1)).into(),
				account_id(99)
			),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

#[test]
fn set_treasury_account_rejects_zero() {
	new_test_ext().execute_with(|| {
		let zero = sp_core::crypto::AccountId32::from([0u8; 32]);
		assert_err!(
			Treasury::set_treasury_account(frame_system::RawOrigin::Root.into(), zero),
			Error::<Test>::InvalidTreasuryAccount
		);
	});
}

#[test]
fn set_treasury_portion_works() {
	new_test_ext().execute_with(|| {
		let portion = sp_runtime::Permill::from_percent(30);
		assert_ok!(Treasury::set_treasury_portion(frame_system::RawOrigin::Root.into(), portion));
		assert_eq!(Treasury::portion(), portion);
		System::<Test>::assert_has_event(
			Event::<Test>::TreasuryPortionUpdated { new_portion: portion }.into(),
		);
	});
}

#[test]
fn set_treasury_portion_requires_root() {
	new_test_ext().execute_with(|| {
		assert_err!(
			Treasury::set_treasury_portion(
				frame_system::RawOrigin::Signed(account_id(1)).into(),
				sp_runtime::Permill::from_percent(30)
			),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

#[test]
fn set_treasury_portion_boundary_0_percent() {
	// 0% = treasury gets nothing, miner gets 100%
	new_test_ext_with_treasury(account_id(1), sp_runtime::Permill::zero()).execute_with(|| {
		assert_eq!(Treasury::portion(), sp_runtime::Permill::zero());
	});
}

#[test]
fn set_treasury_portion_accepts_100_percent() {
	// 100% = miner gets nothing, treasury gets 100%
	new_test_ext().execute_with(|| {
		assert_ok!(Treasury::set_treasury_portion(
			frame_system::RawOrigin::Root.into(),
			sp_runtime::Permill::one()
		));
		assert_eq!(Treasury::portion(), sp_runtime::Permill::one());
		System::<Test>::assert_has_event(
			Event::<Test>::TreasuryPortionUpdated { new_portion: sp_runtime::Permill::one() }
				.into(),
		);
	});
}

#[test]
#[should_panic(expected = "Treasury account must be set in genesis")]
fn account_id_panics_when_not_configured() {
	new_test_ext_without_treasury().execute_with(|| {
		let _ = Treasury::account_id();
	});
}

#[test]
#[should_panic(expected = "Treasury portion must be set in genesis")]
fn portion_panics_when_not_configured() {
	new_test_ext_without_treasury().execute_with(|| {
		let _ = Treasury::portion();
	});
}

#[test]
fn treasury_provider_trait_matches_pallet() {
	new_test_ext().execute_with(|| {
		// TreasuryProvider is the interface consumed by mining-rewards
		assert_eq!(<Treasury as TreasuryProvider>::account_id(), Treasury::account_id());
		assert_eq!(<Treasury as TreasuryProvider>::portion(), Treasury::portion());
	});
}
