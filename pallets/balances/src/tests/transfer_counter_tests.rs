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

//! Tests for the global transfer counter functionality.

use super::*;
use crate::{TransferCount, TransferProof};
use sp_runtime::{ArithmeticError::Underflow, DispatchError::Arithmetic};

/// Alice account ID for more readable tests.
fn alice() -> AccountId {
	account_id(1)
}
/// Bob account ID for more readable tests.
fn bob() -> AccountId {
	account_id(2)
}
/// Charlie account ID for more readable tests.
fn charlie() -> AccountId {
	account_id(3)
}

#[test]
fn transfer_counter_starts_at_zero() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Transfer counter should start at 0
			assert_eq!(Balances::transfer_count(), 0);
		});
}

#[test]
fn transfer_allow_death_increments_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// Perform a transfer
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), bob(), 5));

			// Counter should increment to 1
			assert_eq!(Balances::transfer_count(), 1);

			// Perform another transfer
			assert_ok!(Balances::transfer_allow_death(Some(bob()).into(), charlie(), 3));

			// Counter should increment to 2
			assert_eq!(Balances::transfer_count(), 2);
		});
}

#[test]
fn transfer_keep_alive_increments_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// Perform a transfer_keep_alive
			assert_ok!(Balances::transfer_keep_alive(Some(alice()).into(), bob(), 5));

			// Counter should increment to 1
			assert_eq!(Balances::transfer_count(), 1);
		});
}

#[test]
fn force_transfer_increments_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// Perform a force transfer
			assert_ok!(Balances::force_transfer(RuntimeOrigin::root(), alice(), bob(), 5));

			// Counter should increment to 1
			assert_eq!(Balances::transfer_count(), 1);
		});
}

#[test]
fn transfer_all_increments_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// Perform a transfer_all
			assert_ok!(Balances::transfer_all(Some(alice()).into(), bob(), false));

			// Counter should increment to 1
			assert_eq!(Balances::transfer_count(), 1);
		});
}

#[test]
fn self_transfer_does_not_increment_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// Self transfer should not increment counter
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), alice(), 5));

			// Counter should remain 0 since it's a self-transfer
			assert_eq!(Balances::transfer_count(), 0);
		});
}

#[test]
fn transfer_proof_storage_is_created() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Perform a transfer
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), bob(), 5));

			// Check that transfer proof was stored with correct key
			let key = (0u64, alice(), bob(), 5);
			assert!(TransferProof::<Test>::contains_key(&key));
		});
}

#[test]
fn multiple_transfers_create_sequential_proofs() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// First transfer
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), bob(), 5));
			assert_eq!(Balances::transfer_count(), 1);

			// Check first proof exists
			let key1 = (0u64, alice(), bob(), 5u128);
			assert!(TransferProof::<Test>::contains_key(&key1));

			// Second transfer
			assert_ok!(Balances::transfer_allow_death(Some(bob()).into(), charlie(), 3));
			assert_eq!(Balances::transfer_count(), 2);

			// Check second proof exists
			let key2 = (1u64, bob(), charlie(), 3u128);
			assert!(TransferProof::<Test>::contains_key(&key2));

			// Third transfer with different amount
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), charlie(), 1));
			assert_eq!(Balances::transfer_count(), 3);

			// Check third proof exists
			let key3 = (2u64, alice(), charlie(), 1u128);
			assert!(TransferProof::<Test>::contains_key(&key3));
		});
}

#[test]
fn failed_transfers_do_not_increment_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// Attempt transfer with insufficient funds
			assert_noop!(
				Balances::transfer_allow_death(Some(alice()).into(), bob(), 1000),
				Arithmetic(Underflow)
			);

			// Counter should remain 0 since transfer failed
			assert_eq!(Balances::transfer_count(), 0);
		});
}

#[test]
fn transfer_proof_storage_key_generation() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			let transfer_count = 5u64;
			let from = alice();
			let to = bob();
			let amount = 100u128;

			// Generate storage key
			let key = Balances::transfer_proof_storage_key(
				transfer_count,
				from.clone(),
				to.clone(),
				amount,
			);

			// Key should not be empty
			assert!(!key.is_empty());

			// The same parameters should generate the same key
			let key2 = Balances::transfer_proof_storage_key(
				transfer_count,
				from.clone(),
				to.clone(),
				amount,
			);
			assert_eq!(key, key2);

			// Different parameters should generate different keys
			let key3 = Balances::transfer_proof_storage_key(transfer_count + 1, from, to, amount);
			assert_ne!(key, key3);
		});
}

#[test]
fn counter_saturates_at_max_value() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Set counter to near maximum value (u64::MAX - 1)
			let near_max = u64::MAX - 1;
			TransferCount::<Test>::put(near_max);

			assert_eq!(Balances::transfer_count(), near_max);

			// Perform a transfer - should increment to u64::MAX
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), bob(), 5));
			assert_eq!(Balances::transfer_count(), u64::MAX);

			// Perform another transfer - should saturate at u64::MAX
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), charlie(), 3));
			assert_eq!(Balances::transfer_count(), u64::MAX);
		});
}

#[test]
fn transfer_counter_persists_across_blocks() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Perform transfer in block 1
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), bob(), 5));
			assert_eq!(Balances::transfer_count(), 1);

			// Move to block 2
			System::set_block_number(2);

			// Counter should persist
			assert_eq!(Balances::transfer_count(), 1);

			// Perform another transfer in block 2
			assert_ok!(Balances::transfer_allow_death(Some(bob()).into(), charlie(), 3));
			assert_eq!(Balances::transfer_count(), 2);
		});
}

#[test]
fn zero_value_transfers_increment_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// Perform a zero-value transfer
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), bob(), 0));

			// Counter should increment even for zero-value transfers
			assert_eq!(Balances::transfer_count(), 1);

			// Transfer proof should be created
			let key = (0u64, alice(), bob(), 0u128);
			assert!(TransferProof::<Test>::contains_key(&key));
		});
}

#[test]
fn different_transfer_types_all_increment_counter() {
	ExtBuilder::default()
		.existential_deposit(1)
		.monied(true)
		.build_and_execute_with(|| {
			// Initial counter should be 0
			assert_eq!(Balances::transfer_count(), 0);

			// transfer_allow_death
			assert_ok!(Balances::transfer_allow_death(Some(alice()).into(), bob(), 1));
			assert_eq!(Balances::transfer_count(), 1);

			// transfer_keep_alive
			assert_ok!(Balances::transfer_keep_alive(Some(alice()).into(), charlie(), 1));
			assert_eq!(Balances::transfer_count(), 2);

			// force_transfer
			assert_ok!(Balances::force_transfer(RuntimeOrigin::root(), bob(), charlie(), 1));
			assert_eq!(Balances::transfer_count(), 3);

			// transfer_all (transfer remaining balance)
			let remaining = Balances::free_balance(alice());
			if remaining > 1 {
				assert_ok!(Balances::transfer_all(Some(alice()).into(), bob(), false));
				assert_eq!(Balances::transfer_count(), 4);
			}
		});
}
