//! Tests for the treasury config pallet (account + portion for mining-rewards).

#[cfg(test)]
mod tests {
	use frame_support::{assert_err, assert_ok};
	use frame_system::RawOrigin;
	use quantus_runtime::{
		configs::TreasuryPalletId, AccountId, Runtime, System, TreasuryPallet, UNIT,
	};
	use sp_runtime::{traits::AccountIdConversion, BuildStorage, Permill};

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

		pallet_treasury::GenesisConfig::<Runtime> {
			treasury_account: Some(treasury_account_id()),
			treasury_portion: Some(Permill::from_percent(50)),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}

	#[test]
	fn genesis_sets_treasury_config() {
		new_test_ext().execute_with(|| {
			assert_eq!(TreasuryPallet::account_id(), treasury_account_id());
			assert_eq!(TreasuryPallet::portion(), Permill::from_percent(50));
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

	#[test]
	fn set_treasury_portion_works() {
		new_test_ext().execute_with(|| {
			assert_ok!(TreasuryPallet::set_treasury_portion(
				RawOrigin::Root.into(),
				Permill::from_percent(30)
			));
			assert_eq!(TreasuryPallet::portion(), Permill::from_percent(30));
		});
	}

	#[test]
	fn set_treasury_portion_accepts_100_percent() {
		new_test_ext().execute_with(|| {
			// 100% is the upper bound
			assert_ok!(TreasuryPallet::set_treasury_portion(
				RawOrigin::Root.into(),
				Permill::one()
			));
			assert_eq!(TreasuryPallet::portion(), Permill::one());
		});
	}
}
