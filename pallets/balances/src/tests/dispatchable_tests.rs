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

//! Tests regarding the functionality of the dispatchables/extrinsics.

use super::*;
use crate::Event;
use frame_support::traits::tokens::Preservation::Expendable;
use fungible::{hold::Mutate as HoldMutate, Inspect, Mutate};

#[test]
fn default_indexing_on_new_accounts_should_not_work2() {
	ExtBuilder::default()
		.existential_deposit(10)
		.monied(true)
		.build_and_execute_with(|| {
			// account 5 should not exist
			// ext_deposit is 10, value is 9, not satisfies for ext_deposit
			assert_noop!(
				Balances::transfer_allow_death(Some(1).into(), 5, 9),
				TokenError::BelowMinimum,
			);
			assert_eq!(Balances::free_balance(1), 100);
		});
}

/// A below-ED transfer to a dead dest must fail even when the sender would be reaped.
///
/// The well-funded case above leaves `dust = 0`, so it does not exercise the transfer helper
/// widening `can_deposit` to `amount + dust`. With a sender at ED, that widened gate would
/// admit the call, debit the sender, credit nothing, emit `Transfer`, and leave
/// `total_issuance` above the sum of balances.
#[test]
fn transfer_below_ed_from_reapable_sender_to_dead_dest_must_fail() {
	ExtBuilder::default().existential_deposit(10).build_and_execute_with(|| {
		let _ = Balances::mint_into(&1, 10);
		let issuance_before = Balances::total_issuance();

		assert_noop!(
			Balances::transfer_allow_death(Some(1).into(), 5, 9),
			TokenError::BelowMinimum,
		);

		assert_eq!(Balances::free_balance(1), 10);
		assert_eq!(Balances::free_balance(5), 0);
		assert_eq!(Balances::total_issuance(), issuance_before);
	});
}

#[test]
fn dust_account_removal_should_work() {
	ExtBuilder::default()
		.existential_deposit(100)
		.monied(true)
		.build_and_execute_with(|| {
			System::inc_account_nonce(&2);
			assert_eq!(System::account_nonce(&2), 1);
			assert_eq!(Balances::total_balance(&2), 2000);
			// index 1 (account 2) becomes zombie
			assert_ok!(Balances::transfer_allow_death(Some(2).into(), 5, 1901));
			assert_eq!(Balances::total_balance(&2), 0);
			assert_eq!(Balances::total_balance(&5), 1901);
			assert_eq!(System::account_nonce(&2), 0);
		});
}

#[test]
fn balance_transfer_works() {
	ExtBuilder::default().build_and_execute_with(|| {
		let _ = Balances::mint_into(&1, 111);
		assert_ok!(Balances::transfer_allow_death(Some(1).into(), 2, 69));
		assert_eq!(Balances::total_balance(&1), 42);
		assert_eq!(Balances::total_balance(&2), 69);
	});
}

#[test]
fn balance_transfer_when_on_hold_should_not_work() {
	ExtBuilder::default().build_and_execute_with(|| {
		let _ = Balances::mint_into(&1, 111);
		assert_ok!(Balances::hold(&TestId::Foo, &1, 69));
		assert_noop!(
			Balances::transfer_allow_death(Some(1).into(), 2, 69),
			TokenError::FundsUnavailable,
		);
	});
}

#[test]
fn transfer_keep_alive_works() {
	ExtBuilder::default().existential_deposit(1).build_and_execute_with(|| {
		let _ = Balances::mint_into(&1, 100);
		assert_noop!(
			Balances::transfer_keep_alive(Some(1).into(), 2, 100),
			TokenError::NotExpendable
		);
		assert_eq!(Balances::total_balance(&1), 100);
		assert_eq!(Balances::total_balance(&2), 0);
	});
}

#[test]
fn transfer_keep_alive_all_free_succeed() {
	ExtBuilder::default().existential_deposit(100).build_and_execute_with(|| {
		set_free_balance(1, 300);
		assert_ok!(Balances::hold(&TestId::Foo, &1, 100));
		assert_ok!(Balances::transfer_keep_alive(Some(1).into(), 2, 100));
		assert_eq!(Balances::total_balance(&1), 200);
		assert_eq!(Balances::total_balance(&2), 100);
	});
}

#[test]
fn transfer_all_works_1() {
	ExtBuilder::default().existential_deposit(100).build().execute_with(|| {
		// setup
		set_free_balance(1, 200);
		set_free_balance(2, 0);
		// transfer all and allow death
		assert_ok!(Balances::transfer_all(Some(1).into(), 2, false));
		assert_eq!(Balances::total_balance(&1), 0);
		assert_eq!(Balances::total_balance(&2), 200);
	});
}

#[test]
fn transfer_all_works_2() {
	ExtBuilder::default().existential_deposit(100).build().execute_with(|| {
		// setup
		set_free_balance(1, 200);
		set_free_balance(2, 0);
		// transfer all and keep alive
		assert_ok!(Balances::transfer_all(Some(1).into(), 2, true));
		assert_eq!(Balances::total_balance(&1), 100);
		assert_eq!(Balances::total_balance(&2), 100);
	});
}

#[test]
fn transfer_all_works_3() {
	ExtBuilder::default().existential_deposit(100).build().execute_with(|| {
		// setup
		set_free_balance(1, 210);
		assert_ok!(Balances::hold(&TestId::Foo, &1, 10));
		set_free_balance(2, 0);
		// transfer all and allow death w/ reserved
		assert_ok!(Balances::transfer_all(Some(1).into(), 2, false));
		assert_eq!(Balances::total_balance(&1), 110);
		assert_eq!(Balances::total_balance(&2), 100);
	});
}

#[test]
fn transfer_all_works_4() {
	ExtBuilder::default().existential_deposit(100).build().execute_with(|| {
		// setup
		set_free_balance(1, 210);
		assert_ok!(Balances::hold(&TestId::Foo, &1, 10));
		set_free_balance(2, 0);
		// transfer all and keep alive w/ reserved
		assert_ok!(Balances::transfer_all(Some(1).into(), 2, true));
		assert_eq!(Balances::total_balance(&1), 110);
		assert_eq!(Balances::total_balance(&2), 100);
	});
}

/// The `ensure_upgraded` failsafe tops a reserved-but-providerless legacy account up to
/// the existential deposit. That top-up mints new funds, so it must be counted in
/// `TotalIssuance` — otherwise every such upgrade silently inflates effective supply.
#[test]
fn ensure_upgraded_failsafe_mint_is_counted_in_total_issuance() {
	ExtBuilder::default()
		.existential_deposit(10)
		.monied(true)
		.build_and_execute_with(|| {
			// A legacy account with reserved funds but no provider refs: the pathological
			// state the failsafe defends against. Written raw, bypassing the provider
			// bookkeeping, since no healthy path can produce it.
			let data = AccountData {
				free: 0,
				reserved: 5,
				frozen: Zero::zero(),
				flags: crate::types::ExtraFlags::old_logic(),
			};
			if UseSystem::get() {
				frame_system::Account::<Test>::insert(
					7,
					frame_system::AccountInfo { data, ..Default::default() },
				);
			} else {
				crate::Account::<Test>::insert(7, data);
			}
			assert_eq!(System::providers(&7), 0);

			let ti_before = TotalIssuance::<Test>::get();
			assert!(Balances::ensure_upgraded(&7));

			// The failsafe minted an ED of free balance and restored the provider ref...
			assert_eq!(get_test_account_data(7).free, 10);
			assert_eq!(System::providers(&7), 1);
			// ...and the minted funds are recorded as issuance.
			assert_eq!(
				TotalIssuance::<Test>::get(),
				ti_before + 10,
				"the failsafe ED top-up must increase TotalIssuance"
			);
		});
}

#[test]
fn ensure_upgraded_should_work() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			System::inc_providers(&7);
			assert_ok!(<Test as Config>::AccountStore::try_mutate_exists(
				&7,
				|a| -> DispatchResult {
					*a = Some(AccountData {
						free: 5,
						reserved: 5,
						frozen: Zero::zero(),
						flags: crate::types::ExtraFlags::old_logic(),
					});
					Ok(())
				}
			));
			assert!(!get_test_account_data(7).flags.is_new_logic());
			assert_eq!(System::providers(&7), 1);
			assert_eq!(System::consumers(&7), 0);
			assert!(Balances::ensure_upgraded(&7));
			assert!(get_test_account_data(7).flags.is_new_logic());
			assert_eq!(System::providers(&7), 1);
			assert_eq!(System::consumers(&7), 1);

			<Balances as frame_support::traits::ReservableCurrency<_>>::unreserve(&7, 5);
			assert_ok!(<Balances as fungible::Mutate<_>>::transfer(&7, &1, 10, Expendable));
			assert_eq!(Balances::total_balance(&7), 0);
			assert_eq!(System::providers(&7), 0);
			assert_eq!(System::consumers(&7), 0);
		});
}

#[test]
fn burn_works() {
	ExtBuilder::default().build().execute_with(|| {
		// Prepare account with initial balance
		let (account, init_balance) = (1, 37);
		set_free_balance(account, init_balance);
		let init_issuance = pallet_balances::TotalIssuance::<Test>::get();
		let (keep_alive, allow_death) = (true, false);

		// 1. Cannot burn more than what's available
		assert_noop!(
			Balances::burn(Some(account).into(), init_balance + 1, allow_death),
			TokenError::FundsUnavailable,
		);

		// 2. Burn some funds, without reaping the account
		let burn_amount_1 = 1;
		assert_ok!(Balances::burn(Some(account).into(), burn_amount_1, allow_death));
		System::assert_last_event(RuntimeEvent::Balances(Event::Burned {
			who: account,
			amount: burn_amount_1,
		}));
		assert_eq!(pallet_balances::TotalIssuance::<Test>::get(), init_issuance - burn_amount_1);
		assert_eq!(Balances::total_balance(&account), init_balance - burn_amount_1);

		// 3. Cannot burn funds below existential deposit if `keep_alive` is `true`
		let burn_amount_2 =
			init_balance - burn_amount_1 - <Test as Config>::ExistentialDeposit::get() + 1;
		assert_noop!(
			Balances::burn(Some(account).into(), init_balance + 1, keep_alive),
			TokenError::FundsUnavailable,
		);

		// 4. Burn some more funds, this time reaping the account
		assert_ok!(Balances::burn(Some(account).into(), burn_amount_2, allow_death));
		System::assert_last_event(RuntimeEvent::Balances(Event::Burned {
			who: account,
			amount: burn_amount_2,
		}));
		assert_eq!(
			pallet_balances::TotalIssuance::<Test>::get(),
			init_issuance - burn_amount_1 - burn_amount_2
		);
		assert!(Balances::total_balance(&account).is_zero());
	});
}
