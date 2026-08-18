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

//! Balances pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use crate::Pallet as Balances;

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_runtime::traits::Bounded;

const SEED: u32 = 0;
// existential deposit multiplier
const ED_MULTIPLIER: u32 = 10;

/// The unit the benchmarks scale their balances by.
///
/// This must be derived from the configured `T::ExistentialDeposit` — the
/// `insecure_zero_ed` feature only *permits* a zero ED, it does not guarantee one. A
/// stand-in is used only when the ED is actually zero, where a non-zero unit is still
/// needed to produce meaningful transfer amounts.
fn minimum_balance<T: Config<I>, I: 'static>() -> T::Balance {
	let ed = T::ExistentialDeposit::get();
	if ed.is_zero() {
		100u32.into()
	} else {
		ed
	}
}

/// The free balance the benchmarks expect `who` to be left with after its balance was
/// reduced from `balance` by `spent`: accounts whose leftover dips below the configured
/// existential deposit are reaped (the dust is removed), all others keep the leftover.
fn expected_leftover<T: Config<I>, I: 'static>(
	balance: T::Balance,
	spent: T::Balance,
) -> T::Balance {
	let leftover = balance - spent;
	if leftover < T::ExistentialDeposit::get() {
		Zero::zero()
	} else {
		leftover
	}
}

#[instance_benchmarks]
mod benchmarks {
	use super::*;

	// Benchmark `transfer` extrinsic with the worst possible conditions:
	// * Transfer will kill the sender account.
	// * Transfer will create the recipient account.
	#[benchmark]
	fn transfer_allow_death() {
		let existential_deposit: T::Balance = minimum_balance::<T, I>();
		let caller = whitelisted_caller();

		// Give some multiple of the existential deposit
		let balance = existential_deposit.saturating_mul(ED_MULTIPLIER.into()).max(1u32.into());
		let _ = <Balances<T, I> as Currency<_>>::make_free_balance_be(&caller, balance);

		// Transfer `e - 1` existential deposits + 1 unit, which guarantees to create one account,
		// and reap this user.
		let recipient: T::AccountId = account("recipient", 0, SEED);
		let recipient_lookup = T::Lookup::unlookup(recipient.clone());
		let transfer_amount =
			existential_deposit.saturating_mul((ED_MULTIPLIER - 1).into()) + 1u32.into();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), recipient_lookup, transfer_amount);

		assert_eq!(
			Balances::<T, I>::free_balance(&caller),
			expected_leftover::<T, I>(balance, transfer_amount)
		);
		assert_eq!(Balances::<T, I>::free_balance(&recipient), transfer_amount);
	}

	// Benchmark `transfer` with the best possible condition:
	// * Both accounts exist and will continue to exist.
	#[benchmark(extra)]
	fn transfer_best_case() {
		let caller = whitelisted_caller();
		let recipient: T::AccountId = account("recipient", 0, SEED);
		let recipient_lookup = T::Lookup::unlookup(recipient.clone());

		// Give the sender account max funds for transfer (their account will never reasonably be
		// killed).
		let _ =
			<Balances<T, I> as Currency<_>>::make_free_balance_be(&caller, T::Balance::max_value());

		// Give the recipient account existential deposit (thus their account already exists).
		let existential_deposit: T::Balance = minimum_balance::<T, I>();
		let _ =
			<Balances<T, I> as Currency<_>>::make_free_balance_be(&recipient, existential_deposit);
		let transfer_amount = existential_deposit.saturating_mul(ED_MULTIPLIER.into());

		#[extrinsic_call]
		transfer_allow_death(RawOrigin::Signed(caller.clone()), recipient_lookup, transfer_amount);

		assert!(!Balances::<T, I>::free_balance(&caller).is_zero());
		assert!(!Balances::<T, I>::free_balance(&recipient).is_zero());
	}

	// Benchmark `transfer_keep_alive` with the worst possible condition:
	// * The recipient account is created.
	#[benchmark]
	fn transfer_keep_alive() {
		let caller = whitelisted_caller();
		let recipient: T::AccountId = account("recipient", 0, SEED);
		let recipient_lookup = T::Lookup::unlookup(recipient.clone());

		// Give the sender account max funds, thus a transfer will not kill account.
		let _ =
			<Balances<T, I> as Currency<_>>::make_free_balance_be(&caller, T::Balance::max_value());
		let existential_deposit: T::Balance = minimum_balance::<T, I>();
		let transfer_amount = existential_deposit.saturating_mul(ED_MULTIPLIER.into());

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), recipient_lookup, transfer_amount);

		assert!(!Balances::<T, I>::free_balance(&caller).is_zero());
		assert_eq!(Balances::<T, I>::free_balance(&recipient), transfer_amount);
	}

	// This benchmark performs the same operation as `transfer` in the worst case scenario,
	// but additionally introduces many new users into the storage, increasing the the merkle
	// trie and PoV size.
	#[benchmark(extra)]
	fn transfer_increasing_users(u: Linear<0, 1_000>) {
		// 1_000 is not very much, but this upper bound can be controlled by the CLI.
		let existential_deposit: T::Balance = minimum_balance::<T, I>();
		let caller = whitelisted_caller();

		// Give some multiple of the existential deposit
		let balance = existential_deposit.saturating_mul(ED_MULTIPLIER.into());
		let _ = <Balances<T, I> as Currency<_>>::make_free_balance_be(&caller, balance);

		// Transfer `e - 1` existential deposits + 1 unit, which guarantees to create one account,
		// and reap this user.
		let recipient: T::AccountId = account("recipient", 0, SEED);
		let recipient_lookup = T::Lookup::unlookup(recipient.clone());
		let transfer_amount =
			existential_deposit.saturating_mul((ED_MULTIPLIER - 1).into()) + 1u32.into();

		// Create a bunch of users in storage.
		for i in 0..u {
			// The `account` function uses `blake2_256` to generate unique accounts, so these
			// should be quite random and evenly distributed in the trie.
			let new_user: T::AccountId = account("new_user", i, SEED);
			let _ = <Balances<T, I> as Currency<_>>::make_free_balance_be(&new_user, balance);
		}

		#[extrinsic_call]
		transfer_allow_death(RawOrigin::Signed(caller.clone()), recipient_lookup, transfer_amount);

		assert_eq!(
			Balances::<T, I>::free_balance(&caller),
			expected_leftover::<T, I>(balance, transfer_amount)
		);
		assert_eq!(Balances::<T, I>::free_balance(&recipient), transfer_amount);
	}

	// Benchmark `transfer_all` with the worst possible condition:
	// * The recipient account is created
	// * The sender is killed
	#[benchmark]
	fn transfer_all() {
		let caller = whitelisted_caller();
		let recipient: T::AccountId = account("recipient", 0, SEED);
		let recipient_lookup = T::Lookup::unlookup(recipient.clone());

		// Give some multiple of the existential deposit
		let existential_deposit: T::Balance = minimum_balance::<T, I>();
		let balance = existential_deposit.saturating_mul(ED_MULTIPLIER.into());
		let _ = <Balances<T, I> as Currency<_>>::make_free_balance_be(&caller, balance);

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), recipient_lookup, false);

		assert!(Balances::<T, I>::free_balance(&caller).is_zero());
		assert_eq!(Balances::<T, I>::free_balance(&recipient), balance);
	}

	/// Benchmark `burn` extrinsic with the worst possible condition - burn kills the account.
	#[benchmark]
	fn burn_allow_death() {
		let existential_deposit: T::Balance = minimum_balance::<T, I>();
		let caller = whitelisted_caller();

		// Give some multiple of the existential deposit
		let balance = existential_deposit.saturating_mul(ED_MULTIPLIER.into());
		let _ = <Balances<T, I> as Currency<_>>::make_free_balance_be(&caller, balance);

		// Burn enough to kill the account.
		let burn_amount = balance - existential_deposit + 1u32.into();

		#[extrinsic_call]
		burn(RawOrigin::Signed(caller.clone()), burn_amount, false);

		assert_eq!(
			Balances::<T, I>::free_balance(&caller),
			expected_leftover::<T, I>(balance, burn_amount)
		);
	}

	// Benchmark `burn` extrinsic with the case where account is kept alive.
	#[benchmark]
	fn burn_keep_alive() {
		let existential_deposit: T::Balance = minimum_balance::<T, I>();
		let caller = whitelisted_caller();

		// Give some multiple of the existential deposit
		let balance = existential_deposit.saturating_mul(ED_MULTIPLIER.into());
		let _ = <Balances<T, I> as Currency<_>>::make_free_balance_be(&caller, balance);

		// Burn minimum possible amount which should not kill the account.
		let burn_amount = 1u32.into();

		#[extrinsic_call]
		burn(RawOrigin::Signed(caller.clone()), burn_amount, true);

		assert_eq!(Balances::<T, I>::free_balance(&caller), balance - burn_amount);
	}

	impl_benchmark_test_suite! {
		Balances,
		crate::tests::ExtBuilder::default().build(),
		crate::tests::Test,
	}

	/// The benchmarks must derive their setup and post-conditions from the configured
	/// `T::ExistentialDeposit`, not from the `insecure_zero_ed` feature flag: that
	/// feature only *permits* a zero ED, it does not guarantee one. Run the
	/// reaping-sensitive benchmarks under an ED that is neither the mock default (1)
	/// nor covered by the old hardcoded feature stand-in (100), so a feature-based
	/// expectation cannot pass by luck.
	#[cfg(test)]
	#[test]
	fn benchmarks_hold_under_non_default_existential_deposit() {
		use crate::tests::{ExtBuilder, Test};
		use frame_support::assert_ok;
		type Bench = Balances<Test, ()>;
		ExtBuilder::default().existential_deposit(150).build().execute_with(|| {
			assert_ok!(Bench::test_benchmark_transfer_allow_death());
			assert_ok!(Bench::test_benchmark_transfer_all());
			assert_ok!(Bench::test_benchmark_burn_allow_death());
			assert_ok!(Bench::test_benchmark_transfer_keep_alive());
		});
	}
}
