// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Tests.

#![cfg(test)]

use crate::{
	self as pallet_balances, AccountData, Config, CreditOf, Error, Pallet, TotalIssuance,
	DEFAULT_ADDRESS_URI, MAX_DEV_ACCOUNTS,
};
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{
	assert_err, assert_noop, assert_ok, assert_storage_noop, derive_impl,
	dispatch::{DispatchInfo, GetDispatchInfo},
	parameter_types,
	traits::{
		fungible, ConstU32, ConstU8, Imbalance as ImbalanceT, OnUnbalanced, StorageMapShim,
		StoredMap, VariantCount, VariantCountOf, WhitelistedStorageKeys,
	},
	weights::{IdentityFee, Weight},
};
use frame_system::{self as system, RawOrigin};
use pallet_transaction_payment::{ChargeTransactionPayment, FungibleAdapter, Multiplier};
use scale_info::TypeInfo;
use sp_core::{hexdisplay::HexDisplay, sr25519::Pair as SrPair, Pair};
use sp_io;
use sp_runtime::{
	traits::{BadOrigin, Zero},
	ArithmeticError, BuildStorage, DispatchError, DispatchResult, FixedPointNumber, RuntimeDebug,
	TokenError,
};
use std::{collections::BTreeSet, sync::OnceLock};

/// Genesis `dev_accounts` count used by [`ExtBuilder`] when dev accounts are enabled.
///
/// Kept well below [`MAX_DEV_ACCOUNTS`]: each account pays for an sr25519 URI derivation. The
/// production cap stays high for the genesis DoS bound; tests only need to prove a non-trivial
/// set exists in storage and is counted as issuance.
const TEST_DEV_ACCOUNTS: u32 = 100;

mod consumer_limit_tests;
mod currency_tests;
mod dispatchable_tests;
mod fungible_and_currency;
mod fungible_conformance_tests;
mod fungible_tests;
mod general_tests;
mod reentrancy_tests;

type Block = frame_system::mocking::MockBlock<Test>;

#[derive(
	Encode,
	Decode,
	DecodeWithMemTracking,
	Copy,
	Clone,
	Eq,
	PartialEq,
	Ord,
	PartialOrd,
	MaxEncodedLen,
	TypeInfo,
	RuntimeDebug,
)]
pub enum TestId {
	Foo,
	Bar,
	Baz,
}

impl VariantCount for TestId {
	const VARIANT_COUNT: u32 = 3;
}

pub(crate) type AccountId = <Test as frame_system::Config>::AccountId;
pub(crate) type Balance = <Test as Config>::Balance;

frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		TransactionPayment: pallet_transaction_payment,
	}
);

parameter_types! {
	pub BlockWeights: frame_system::limits::BlockWeights =
		frame_system::limits::BlockWeights::simple_max(
			frame_support::weights::Weight::from_parts(1024, u64::MAX),
		);
	pub static ExistentialDeposit: u64 = 1;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountData = super::AccountData<u64>;
}

#[derive_impl(pallet_transaction_payment::config_preludes::TestDefaultConfig)]
impl pallet_transaction_payment::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type OnChargeTransaction = FungibleAdapter<Pallet<Test>, ()>;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightToFee = IdentityFee<u64>;
	type LengthToFee = IdentityFee<u64>;
}

parameter_types! {
	pub FooReason: TestId = TestId::Foo;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl Config for Test {
	type DustRemoval = DustTrap;
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = TestAccountStore;
	type MaxReserves = ConstU32<2>;
	type ReserveIdentifier = TestId;
	type RuntimeHoldReason = TestId;
	type RuntimeFreezeReason = TestId;
	type FreezeIdentifier = TestId;
	type MaxFreezes = VariantCountOf<TestId>;
}

#[derive(Clone)]
pub struct ExtBuilder {
	existential_deposit: u64,
	monied: bool,
	dust_trap: Option<u64>,
	dev_accounts: bool,
}
impl Default for ExtBuilder {
	fn default() -> Self {
		// Dev accounts are opt-in: their allocation is real, counted issuance, and most
		// tests assume a genesis whose issuance is exactly what the test itself creates.
		Self { existential_deposit: 1, monied: false, dust_trap: None, dev_accounts: false }
	}
}
impl ExtBuilder {
	pub fn existential_deposit(mut self, existential_deposit: u64) -> Self {
		self.existential_deposit = existential_deposit;
		self
	}
	pub fn dev_accounts(mut self, enable: bool) -> Self {
		self.dev_accounts = enable;
		self
	}
	pub fn monied(mut self, monied: bool) -> Self {
		self.monied = monied;
		if self.existential_deposit == 0 {
			self.existential_deposit = 1;
		}
		self
	}
	pub fn dust_trap(mut self, account: u64) -> Self {
		self.dust_trap = Some(account);
		self
	}
	#[cfg(feature = "try-runtime")]
	pub fn auto_try_state(self, auto_try_state: bool) -> Self {
		AutoTryState::set(auto_try_state);
		self
	}
	pub fn set_associated_consts(&self) {
		DUST_TRAP_TARGET.with(|v| v.replace(self.dust_trap));
		EXISTENTIAL_DEPOSIT.with(|v| v.replace(self.existential_deposit));
	}
	pub fn build(self) -> sp_io::TestExternalities {
		self.set_associated_consts();
		let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
		pallet_balances::GenesisConfig::<Test> {
			balances: if self.monied {
				vec![
					(1, 10 * self.existential_deposit),
					(2, 20 * self.existential_deposit),
					(3, 30 * self.existential_deposit),
					(4, 40 * self.existential_deposit),
					(12, 10 * self.existential_deposit),
				]
			} else {
				vec![]
			},
			dev_accounts: self.dev_accounts.then(|| {
				(TEST_DEV_ACCOUNTS, self.existential_deposit, Some(DEFAULT_ADDRESS_URI.to_string()))
			}),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}
	pub fn build_and_execute_with(self, f: impl Fn()) {
		let other = self.clone();
		UseSystem::set(false);
		other.build().execute_with(|| {
			f();
			if AutoTryState::get() {
				Balances::do_try_state(System::block_number()).unwrap();
			}
		});
		UseSystem::set(true);
		self.build().execute_with(|| {
			f();
			if AutoTryState::get() {
				Balances::do_try_state(System::block_number()).unwrap();
			}
		});
	}
}

parameter_types! {
	static DustTrapTarget: Option<u64> = None;
}

pub struct DustTrap;

impl OnUnbalanced<CreditOf<Test, ()>> for DustTrap {
	fn on_nonzero_unbalanced(amount: CreditOf<Test, ()>) {
		match DustTrapTarget::get() {
			None => drop(amount),
			Some(a) => {
				let result = <Balances as fungible::Balanced<_>>::resolve(&a, amount);
				debug_assert!(result.is_ok());
			},
		}
	}
}

parameter_types! {
	pub static UseSystem: bool = false;
	pub static AutoTryState: bool = true;
}

type BalancesAccountStore = StorageMapShim<super::Account<Test>, u64, super::AccountData<u64>>;
type SystemAccountStore = frame_system::Pallet<Test>;

pub struct TestAccountStore;
impl StoredMap<u64, super::AccountData<u64>> for TestAccountStore {
	fn get(k: &u64) -> super::AccountData<u64> {
		if UseSystem::get() {
			<SystemAccountStore as StoredMap<_, _>>::get(k)
		} else {
			<BalancesAccountStore as StoredMap<_, _>>::get(k)
		}
	}
	fn try_mutate_exists<R, E: From<DispatchError>>(
		k: &u64,
		f: impl FnOnce(&mut Option<super::AccountData<u64>>) -> Result<R, E>,
	) -> Result<R, E> {
		if UseSystem::get() {
			<SystemAccountStore as StoredMap<_, _>>::try_mutate_exists(k, f)
		} else {
			<BalancesAccountStore as StoredMap<_, _>>::try_mutate_exists(k, f)
		}
	}
	fn mutate<R>(
		k: &u64,
		f: impl FnOnce(&mut super::AccountData<u64>) -> R,
	) -> Result<R, DispatchError> {
		if UseSystem::get() {
			<SystemAccountStore as StoredMap<_, _>>::mutate(k, f)
		} else {
			<BalancesAccountStore as StoredMap<_, _>>::mutate(k, f)
		}
	}
	fn mutate_exists<R>(
		k: &u64,
		f: impl FnOnce(&mut Option<super::AccountData<u64>>) -> R,
	) -> Result<R, DispatchError> {
		if UseSystem::get() {
			<SystemAccountStore as StoredMap<_, _>>::mutate_exists(k, f)
		} else {
			<BalancesAccountStore as StoredMap<_, _>>::mutate_exists(k, f)
		}
	}
	fn insert(k: &u64, t: super::AccountData<u64>) -> Result<(), DispatchError> {
		if UseSystem::get() {
			<SystemAccountStore as StoredMap<_, _>>::insert(k, t)
		} else {
			<BalancesAccountStore as StoredMap<_, _>>::insert(k, t)
		}
	}
	fn remove(k: &u64) -> Result<(), DispatchError> {
		if UseSystem::get() {
			<SystemAccountStore as StoredMap<_, _>>::remove(k)
		} else {
			<BalancesAccountStore as StoredMap<_, _>>::remove(k)
		}
	}
}

pub fn events() -> Vec<RuntimeEvent> {
	let evt = System::events().into_iter().map(|evt| evt.event).collect::<Vec<_>>();
	System::reset_events();
	evt
}

/// create a transaction info struct from weight. Handy to avoid building the whole struct.
pub fn info_from_weight(w: Weight) -> DispatchInfo {
	DispatchInfo { call_weight: w, ..Default::default() }
}

/// Cached AccountIds for [`TEST_DEV_ACCOUNTS`] derived from [`DEFAULT_ADDRESS_URI`].
///
/// Deriving these is expensive; derive them once and share across tests.
fn test_dev_account_ids() -> &'static BTreeSet<AccountId> {
	static IDS: OnceLock<BTreeSet<AccountId>> = OnceLock::new();
	IDS.get_or_init(|| {
		(0..TEST_DEV_ACCOUNTS)
			.map(|index| {
				let derivation_string = DEFAULT_ADDRESS_URI.replace("{}", &index.to_string());
				let pair: SrPair =
					Pair::from_string(&derivation_string, None).expect("Invalid derivation string");
				AccountId::decode(&mut &pair.public().encode()[..]).unwrap()
			})
			.collect()
	})
}

/// Check that the total-issuance matches the sum of all accounts' total balances.
///
/// Every account is reconciled — including genesis dev accounts, whose allocation is
/// real, counted issuance.
pub fn ensure_ti_valid() {
	let mut sum = 0;

	// Iterate over all account keys (i.e., the account IDs).
	for acc in frame_system::Account::<Test>::iter_keys() {
		// Check if we are using the system pallet or some other custom storage for accounts.
		if UseSystem::get() {
			let data = frame_system::Pallet::<Test>::account(acc);
			sum += data.data.total();
		} else {
			let data = crate::Account::<Test>::get(acc);
			sum += data.total();
		}
	}

	// Ensure the total issuance matches the sum of the account balances
	assert_eq!(TotalIssuance::<Test>::get(), sum, "Total Issuance is incorrect");
}

#[test]
fn weights_sane() {
	let info = crate::Call::<Test>::transfer_allow_death { dest: 10, value: 4 }.get_dispatch_info();
	assert_eq!(<() as crate::WeightInfo>::transfer_allow_death(), info.call_weight);

	let info = crate::Call::<Test>::force_unreserve { who: 10, amount: 4 }.get_dispatch_info();
	assert_eq!(<() as crate::WeightInfo>::force_unreserve(), info.call_weight);
}

#[test]
fn derive_dev_account_rejects_counts_above_cap() {
	ExtBuilder::default().build().execute_with(|| {
		let ed = ExistentialDeposit::get();

		// A count within the cap is accepted (does real, but bounded, derivation work).
		assert_ok!(Balances::derive_dev_account(1, ed, DEFAULT_ADDRESS_URI));

		// A count above the cap is rejected before any derivation work is performed, so a
		// caller cannot force unbounded runtime work through the `dev_accounts` genesis field.
		assert_err!(
			Balances::derive_dev_account(MAX_DEV_ACCOUNTS + 1, ed, DEFAULT_ADDRESS_URI),
			"num_accounts exceeds the maximum allowed dev accounts"
		);
	});
}

/// `derive_dev_account` documents a Result-based contract: every reachable input
/// validation must produce an `Err` for the caller to handle, not abort execution.
/// The inputs all come from the (potentially external) chain specification.
#[test]
fn derive_dev_account_returns_errors_instead_of_panicking() {
	ExtBuilder::default().build_and_execute_with(|| {
		let ed = ExistentialDeposit::get();
		assert_err!(
			Balances::derive_dev_account(0, ed, DEFAULT_ADDRESS_URI),
			"num_accounts must be greater than zero"
		);
		assert_err!(
			Balances::derive_dev_account(1, ed - 1, DEFAULT_ADDRESS_URI),
			"the balance of any account should always be at least the existential deposit"
		);
		assert_err!(
			Balances::derive_dev_account(1, ed, "//Sender"),
			"invalid derivation, expected `{}` as part of the derivation"
		);
	});
}

/// An invalid `dev_accounts` entry must fail the genesis build with the specific
/// underlying reason, as a structured configuration failure rather than a bare assert.
#[test]
#[should_panic(expected = "Failed to derive dev accounts from genesis configuration: \
	invalid derivation, expected `{}` as part of the derivation")]
fn genesis_surfaces_dev_account_derivation_errors() {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	crate::GenesisConfig::<Test> {
		balances: vec![],
		dev_accounts: Some((1, ExistentialDeposit::get(), Some("//Sender".to_string()))),
	}
	.assimilate_storage(&mut t)
	.unwrap();
}

/// Genesis `dev_accounts` allocations are real issuance: every derived account must land
/// in storage endowed with the configured balance, and `TotalIssuance` must include the
/// allocation. It used to be computed from the explicit `balances` list only, silently
/// understating the on-chain supply whenever `dev_accounts` was enabled.
#[test]
fn genesis_dev_accounts_are_counted_in_total_issuance() {
	ExtBuilder::default().dev_accounts(true).build_and_execute_with(|| {
		let ed = ExistentialDeposit::get();
		for acc in test_dev_account_ids() {
			assert_eq!(Balances::free_balance(acc), ed, "dev account must be endowed");
		}
		assert_eq!(
			TotalIssuance::<Test>::get(),
			ed * u64::from(TEST_DEV_ACCOUNTS),
			"dev-account allocations must be counted in TotalIssuance"
		);
		ensure_ti_valid();
	});
}

/// Genesis must reject a configured balance that targets an account also produced
/// by the `dev_accounts` derivation: the two writes silently overwrite each other
/// (last write wins) and double-bump the account's provider reference, corrupting
/// the endowed state without any error.
#[test]
#[should_panic(expected = "collides with a dev account")]
fn genesis_endowed_balances_must_not_collide_with_dev_accounts() {
	let dev_account = *test_dev_account_ids().first().unwrap();
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	crate::GenesisConfig::<Test> {
		balances: vec![(dev_account, 100)],
		dev_accounts: Some((
			TEST_DEV_ACCOUNTS,
			ExistentialDeposit::get(),
			Some(DEFAULT_ADDRESS_URI.to_string()),
		)),
	}
	.assimilate_storage(&mut t)
	.unwrap();
}

#[test]
fn check_whitelist() {
	let whitelist: BTreeSet<String> = AllPalletsWithSystem::whitelisted_storage_keys()
		.iter()
		.map(|s| HexDisplay::from(&s.key).to_string())
		.collect();
	// Inactive Issuance
	assert!(whitelist.contains("c2261276cc9d1f8598ea4b6a74b15c2f1ccde6872881f893a21de93dfe970cd5"));
	// Total Issuance
	assert!(whitelist.contains("c2261276cc9d1f8598ea4b6a74b15c2f57c875e4cff74148e4628f264b974c80"));
}

/// This pallet runs tests twice, once with system as `type AccountStore` and once this pallet. This
/// function will return the right value based on the `UseSystem` flag.
pub(crate) fn get_test_account_data(who: AccountId) -> AccountData<Balance> {
	if UseSystem::get() {
		<SystemAccountStore as StoredMap<_, _>>::get(&who)
	} else {
		<BalancesAccountStore as StoredMap<_, _>>::get(&who)
	}
}

/// Same as `get_test_account_data`, but returns a `frame_system::AccountInfo` with the data filled
/// in.
pub(crate) fn get_test_account(
	who: AccountId,
) -> frame_system::AccountInfo<u32, AccountData<Balance>> {
	let mut system_account = frame_system::Account::<Test>::get(&who);
	let account_data = get_test_account_data(who);
	system_account.data = account_data;
	system_account
}
