//! Tests for the governance-controlled treasury account configuration.

#[cfg(test)]
mod tests {
	use crate::common::TestCommons;
	use codec::Encode;
	use frame_support::{assert_err, assert_ok, traits::Currency};
	use frame_system::RawOrigin;
	use pallet_referenda::TracksInfo;
	use quantus_runtime::{
		configs::{TechReferendaInstance, TreasuryPalletId},
		genesis_config_presets::governance_member_seed,
		governance::definitions::TechCollectiveTracksInfo, AccountId, Balances, OriginCaller,
		Preimage, Runtime, RuntimeCall, RuntimeOrigin, System, TechCollective, TechReferenda,
		TreasuryPallet, UNIT,
	};
	use sp_runtime::{traits::AccountIdConversion, traits::Hash, BuildStorage, MultiAddress};

	fn treasury_account_id() -> AccountId {
		TreasuryPalletId::get().into_account_truncating()
	}

	fn new_test_ext() -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();

		pallet_balances::GenesisConfig::<Runtime> {
			balances: vec![(treasury_account_id(), 1000 * UNIT)],
			dev_accounts: None,
		}
		.assimilate_storage(&mut t)
		.unwrap();

		pallet_treasury::GenesisConfig::<Runtime> { treasury_account: Some(treasury_account_id()) }
			.assimilate_storage(&mut t)
			.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}

	fn new_unconfigured_test_ext() -> sp_io::TestExternalities {
		let t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();
		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}

	#[test]
	fn genesis_sets_treasury_config() {
		new_test_ext().execute_with(|| {
			assert_eq!(TreasuryPallet::account_id(), treasury_account_id());
		});
	}

	#[test]
	fn set_treasury_account_works() {
		new_test_ext().execute_with(|| {
			let new_account = AccountId::new([99u8; 32]);
			assert_ok!(TreasuryPallet::set_treasury_account(
				RawOrigin::Root.into(),
				new_account.clone()
			));
			assert_eq!(TreasuryPallet::account_id(), new_account);
		});
	}

	#[test]
	fn root_sets_treasury_after_unconfigured_genesis() {
		new_unconfigured_test_ext().execute_with(|| {
			assert!(TreasuryPallet::treasury_account().is_none());
			let treasury = AccountId::new([99u8; 32]);
			assert_ok!(TreasuryPallet::set_treasury_account(
				RawOrigin::Root.into(),
				treasury.clone(),
			));
			assert_eq!(TreasuryPallet::treasury_account(), Some(treasury));
		});
	}

	#[test]
	fn tech_collective_sets_treasury_after_unconfigured_genesis() {
		new_unconfigured_test_ext().execute_with(|| {
			for member in 1..=10u8 {
				assert_ok!(TechCollective::add_member(
					RuntimeOrigin::root(),
					MultiAddress::from(TestCommons::account_id(member)),
				));
			}
			let proposer = TestCommons::account_id(1);
			Balances::make_free_balance_be(&proposer, governance_member_seed());
			let treasury = AccountId::new([99u8; 32]);
			let call = RuntimeCall::TreasuryPallet(pallet_treasury::Call::set_treasury_account {
				account: treasury.clone(),
			});
			let encoded = call.encode();
			let hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded);
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded.clone(),
			));
			assert_ok!(TechReferenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				Box::new(OriginCaller::system(RawOrigin::Root)),
				frame_support::traits::Bounded::Lookup {
					hash,
					len: encoded.len() as u32,
				},
				frame_support::traits::schedule::DispatchTime::After(0),
			));
			let index =
				pallet_referenda::ReferendumCount::<Runtime, TechReferendaInstance>::get() - 1;
			assert_ok!(TechReferenda::place_decision_deposit(
				RuntimeOrigin::signed(proposer),
				index,
			));
			for member in 1..=10u8 {
				assert_ok!(TechCollective::vote(
					RuntimeOrigin::signed(TestCommons::account_id(member)),
					index,
					true,
				));
			}
			let track = <TechCollectiveTracksInfo as TracksInfo<_, _>>::info(0)
				.expect("Root track must exist");
			let target = System::block_number() +
				TestCommons::calculate_governance_blocks(
					track.prepare_period,
					track.decision_period,
					track.confirm_period,
					track.min_enactment_period,
				);
			TestCommons::run_to_block(target);
			assert!(matches!(
				pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(index),
				Some(pallet_referenda::ReferendumInfo::Approved(..))
			));
			assert_eq!(TreasuryPallet::treasury_account(), Some(treasury));
		});
	}

	#[test]
	fn set_treasury_account_requires_root() {
		new_test_ext().execute_with(|| {
			let new_account = AccountId::new([99u8; 32]);
			assert_err!(
				TreasuryPallet::set_treasury_account(
					RawOrigin::Signed(treasury_account_id()).into(),
					new_account
				),
				sp_runtime::DispatchError::BadOrigin
			);
		});
	}
}
