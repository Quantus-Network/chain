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

use crate::{self as pallet_balances, AccountData, Config, CreditOf, Error, Pallet, TotalIssuance};
use codec::{Decode, Encode, MaxEncodedLen};
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
use sp_core::hexdisplay::HexDisplay;
use sp_runtime::{
    traits::{BadOrigin, Zero},
    ArithmeticError, BuildStorage, DispatchError, DispatchResult, FixedPointNumber, RuntimeDebug,
    TokenError,
};
use std::collections::BTreeSet;

mod currency_tests;
mod dispatchable_tests;
mod fungible_conformance_tests;
mod fungible_tests;
mod general_tests;
mod reentrancy_tests;
mod transfer_counter_tests;
type Block = frame_system::mocking::MockBlock<Test>;

#[derive(
    Encode,
    Decode,
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

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        TransactionPayment: pallet_transaction_payment,
    }
);

type Balance = u128;

parameter_types! {
    pub BlockWeights: frame_system::limits::BlockWeights =
        frame_system::limits::BlockWeights::simple_max(
            frame_support::weights::Weight::from_parts(1024, u64::MAX),
        );
    pub static ExistentialDeposit: Balance = 1;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountData = super::AccountData<Balance>;
}

#[derive_impl(pallet_transaction_payment::config_preludes::TestDefaultConfig)]
impl pallet_transaction_payment::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = FungibleAdapter<Pallet<Test>, ()>;
    type OperationalFeeMultiplier = ConstU8<5>;
    type WeightToFee = IdentityFee<Balance>;
    type LengthToFee = IdentityFee<Balance>;
}

parameter_types! {
    pub FooReason: TestId = TestId::Foo;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl Config for Test {
    type Balance = Balance;
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
    existential_deposit: Balance,
    monied: bool,
    dust_trap: Option<u64>,
}
impl Default for ExtBuilder {
    fn default() -> Self {
        Self {
            existential_deposit: 1,
            monied: false,
            dust_trap: None,
        }
    }
}
impl ExtBuilder {
    pub fn existential_deposit(mut self, existential_deposit: Balance) -> Self {
        self.existential_deposit = existential_deposit;
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
    pub fn set_associated_consts(&self) {
        DUST_TRAP_TARGET.with(|v| v.replace(self.dust_trap));
        EXISTENTIAL_DEPOSIT.with(|v| v.replace(self.existential_deposit));
    }
    pub fn build(self) -> sp_io::TestExternalities {
        self.set_associated_consts();
        let mut t = frame_system::GenesisConfig::<Test>::default()
            .build_storage()
            .unwrap();
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
        other.build().execute_with(&f);
        UseSystem::set(true);
        self.build().execute_with(f);
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
            }
        }
    }
}

parameter_types! {
    pub static UseSystem: bool = false;
}

type BalancesAccountStore = StorageMapShim<super::Account<Test>, u64, super::AccountData<Balance>>;
type SystemAccountStore = frame_system::Pallet<Test>;

pub struct TestAccountStore;
impl StoredMap<u64, super::AccountData<Balance>> for TestAccountStore {
    fn get(k: &u64) -> super::AccountData<Balance> {
        if UseSystem::get() {
            <SystemAccountStore as StoredMap<_, _>>::get(k)
        } else {
            <BalancesAccountStore as StoredMap<_, _>>::get(k)
        }
    }
    fn try_mutate_exists<R, E: From<DispatchError>>(
        k: &u64,
        f: impl FnOnce(&mut Option<super::AccountData<Balance>>) -> Result<R, E>,
    ) -> Result<R, E> {
        if UseSystem::get() {
            <SystemAccountStore as StoredMap<_, _>>::try_mutate_exists(k, f)
        } else {
            <BalancesAccountStore as StoredMap<_, _>>::try_mutate_exists(k, f)
        }
    }
    fn mutate<R>(
        k: &u64,
        f: impl FnOnce(&mut super::AccountData<Balance>) -> R,
    ) -> Result<R, DispatchError> {
        if UseSystem::get() {
            <SystemAccountStore as StoredMap<_, _>>::mutate(k, f)
        } else {
            <BalancesAccountStore as StoredMap<_, _>>::mutate(k, f)
        }
    }
    fn mutate_exists<R>(
        k: &u64,
        f: impl FnOnce(&mut Option<super::AccountData<Balance>>) -> R,
    ) -> Result<R, DispatchError> {
        if UseSystem::get() {
            <SystemAccountStore as StoredMap<_, _>>::mutate_exists(k, f)
        } else {
            <BalancesAccountStore as StoredMap<_, _>>::mutate_exists(k, f)
        }
    }
    fn insert(k: &u64, t: super::AccountData<Balance>) -> Result<(), DispatchError> {
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
    let evt = System::events()
        .into_iter()
        .map(|evt| evt.event)
        .collect::<Vec<_>>();
    System::reset_events();
    evt
}

/// create a transaction info struct from weight. Handy to avoid building the whole struct.
pub fn info_from_weight(w: Weight) -> DispatchInfo {
    DispatchInfo {
        call_weight: w,
        ..Default::default()
    }
}

/// Check that the total-issuance matches the sum of all accounts' total balances.
pub fn ensure_ti_valid() {
    let mut sum = 0;

    for acc in frame_system::Account::<Test>::iter_keys() {
        if UseSystem::get() {
            let data = frame_system::Pallet::<Test>::account(acc);
            sum += data.data.total();
        } else {
            let data = crate::Account::<Test>::get(acc);
            sum += data.total();
        }
    }

    assert_eq!(TotalIssuance::<Test>::get(), sum, "Total Issuance wrong");
}

#[test]
fn weights_sane() {
    let info = crate::Call::<Test>::transfer_allow_death { dest: 10, value: 4 }.get_dispatch_info();
    assert_eq!(
        <() as crate::WeightInfo>::transfer_allow_death(),
        info.call_weight
    );

    let info = crate::Call::<Test>::force_unreserve { who: 10, amount: 4 }.get_dispatch_info();
    assert_eq!(
        <() as crate::WeightInfo>::force_unreserve(),
        info.call_weight
    );
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
