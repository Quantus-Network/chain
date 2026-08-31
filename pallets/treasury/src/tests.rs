use crate::{
	mock::{account_id, new_test_ext, new_test_ext_without_treasury, Test, Treasury},
	pallet::TreasuryAccount,
	Error, Event, TreasuryProvider,
};
use frame_support::{assert_err, assert_ok};
use frame_system::Pallet as System;

#[test]
fn genesis_sets_treasury_config() {
	new_test_ext().execute_with(|| {
		assert!(TreasuryAccount::<Test>::get().is_some(), "TreasuryAccount must be set in genesis");
		assert_eq!(Treasury::account_id(), account_id(1));
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

/// The genesis default must not invent a treasury config: the old default account was
/// the `[1u8; 32]` minting sentinel, so any chain spec that omitted the treasury section
/// silently assigned all treasury funds to an unspendable address. The default must
/// configure nothing, leaving `account_id()` to panic on first use.
#[test]
fn default_genesis_configures_nothing() {
	let default = crate::GenesisConfig::<Test>::default();
	assert!(
		default.treasury_account.is_none(),
		"treasury_account must have no default; chain specs must configure it explicitly"
	);
}

#[test]
#[should_panic(expected = "Treasury account must be set in genesis")]
fn account_id_panics_when_not_configured() {
	new_test_ext_without_treasury().execute_with(|| {
		let _ = Treasury::account_id();
	});
}

#[test]
fn treasury_provider_trait_matches_pallet() {
	new_test_ext().execute_with(|| {
		assert_eq!(<Treasury as TreasuryProvider>::account_id(), Treasury::account_id());
	});
}
