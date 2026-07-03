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

//! The traits for putting freezes within a single fungible token class.
//!
//! See the [`crate::traits::fungibles`] doc for more information about fungibles traits.

use crate::{ensure, traits::tokens::Fortitude};
use scale_info::TypeInfo;
use sp_arithmetic::{
	traits::{CheckedAdd, CheckedSub},
	ArithmeticError,
};
use sp_runtime::{DispatchResult, TokenError};

/// Trait for inspecting a fungible asset which can be frozen. Freezing is essentially setting a
/// minimum balance below which the total balance (inclusive of any funds placed on hold) may not
/// be normally allowed to drop. Generally, freezers will provide an "update" function such that
/// if the total balance does drop below the limit, then the freezer can update their housekeeping
/// accordingly.
pub trait Inspect<AccountId>: super::Inspect<AccountId> {
	/// An identifier for a freeze.
	type Id: codec::Encode + TypeInfo + 'static;

	/// Amount of funds held in reserve by `who` for the given `id`.
	fn balance_frozen(asset: Self::AssetId, id: &Self::Id, who: &AccountId) -> Self::Balance;

	/// The amount of the balance which can become frozen. Defaults to `total_balance()`.
	fn balance_freezable(asset: Self::AssetId, who: &AccountId) -> Self::Balance {
		Self::total_balance(asset, who)
	}

	/// Returns `true` if it's possible to introduce a freeze for the given `id` onto the
	/// account of `who`. This will be true as long as the implementor supports as many
	/// concurrent freeze locks as there are possible values of `id`.
	fn can_freeze(asset: Self::AssetId, id: &Self::Id, who: &AccountId) -> bool;
}

/// Trait for introducing, altering and removing locks to freeze an account's funds so they never
/// go below a set minimum.
pub trait Mutate<AccountId>: Inspect<AccountId> {
	/// Prevent actions which would reduce the balance of the account of `who` below the given
	/// `amount` and identify this restriction though the given `id`. Unlike `extend_freeze`, any
	/// outstanding freeze in place for `who` under the `id` are dropped.
	///
	/// If `amount` is zero, it is equivalent to using `thaw`.
	///
	/// Note that `amount` can be greater than the total balance, if desired.
	fn set_freeze(
		asset: Self::AssetId,
		id: &Self::Id,
		who: &AccountId,
		amount: Self::Balance,
	) -> DispatchResult;

	/// Prevent the balance of the account of `who` from being reduced below the given `amount` and
	/// identify this restriction though the given `id`. Unlike `set_freeze`, this does not
	/// counteract any pre-existing freezes in place for `who` under the `id`. Also unlike
	/// `set_freeze`, in the case that `amount` is zero, this is no-op and never fails.
	///
	/// Note that more funds can be locked than the total balance, if desired.
	fn extend_freeze(
		asset: Self::AssetId,
		id: &Self::Id,
		who: &AccountId,
		amount: Self::Balance,
	) -> DispatchResult;

	/// Remove an existing lock.
	fn thaw(asset: Self::AssetId, id: &Self::Id, who: &AccountId) -> DispatchResult;

	/// Attempt to alter the amount frozen under the given `id` to `amount`.
	///
	/// Fail if the account of `who` has fewer freezable funds than `amount`, unless `fortitude` is
	/// `Fortitude::Force`.
	fn set_frozen(
		asset: Self::AssetId,
		id: &Self::Id,
		who: &AccountId,
		amount: Self::Balance,
		fortitude: Fortitude,
	) -> DispatchResult {
		let force = fortitude == Fortitude::Force;
		ensure!(
			force || Self::balance_freezable(asset.clone(), who) >= amount,
			TokenError::FundsUnavailable
		);
		Self::set_freeze(asset, id, who, amount)
	}

	/// Attempt to set the amount frozen under the given `id` to `amount`, iff this would increase
	/// the amount frozen under `id`. Do nothing otherwise.
	///
	/// Fail if this would increase the amount frozen under `id` and the account of `who` has
	/// fewer freezable funds than `amount`, unless `fortitude` is `Fortitude::Force`.
	fn ensure_frozen(
		asset: Self::AssetId,
		id: &Self::Id,
		who: &AccountId,
		amount: Self::Balance,
		fortitude: Fortitude,
	) -> DispatchResult {
		let force = fortitude == Fortitude::Force;
		// Only require freezable funds if this call would actually increase the amount frozen
		// under `id`; otherwise `extend_freeze` is a no-op and must remain idempotent even when
		// an existing (permitted) over-freeze exceeds the current freezable balance.
		ensure!(
			force ||
				amount <= Self::balance_frozen(asset.clone(), id, who) ||
				Self::balance_freezable(asset.clone(), who) >= amount,
			TokenError::FundsUnavailable
		);
		Self::extend_freeze(asset, id, who, amount)
	}

	/// Decrease the amount which is being frozen for a particular lock, failing in the case of
	/// underflow.
	fn decrease_frozen(
		asset: Self::AssetId,
		id: &Self::Id,
		who: &AccountId,
		amount: Self::Balance,
	) -> DispatchResult {
		let a = Self::balance_frozen(asset.clone(), id, who)
			.checked_sub(&amount)
			.ok_or(ArithmeticError::Underflow)?;
		// Reducing a freeze never increases exposure, so use the unchecked primitive: routing
		// through `set_frozen(.., Polite)` would wrongly fail partial releases of a freeze that
		// (legitimately) exceeds the current freezable balance.
		Self::set_freeze(asset, id, who, a)
	}

	/// Increase the amount which is being frozen for a particular lock, failing in the case that
	/// too little balance is available for being frozen.
	fn increase_frozen(
		asset: Self::AssetId,
		id: &Self::Id,
		who: &AccountId,
		amount: Self::Balance,
	) -> DispatchResult {
		let a = Self::balance_frozen(asset.clone(), id, who)
			.checked_add(&amount)
			.ok_or(ArithmeticError::Overflow)?;
		Self::set_frozen(asset, id, who, a, Fortitude::Polite)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::traits::tokens::{
		DepositConsequence, Preservation, Provenance, WithdrawConsequence,
	};
	use core::cell::RefCell;

	thread_local! {
		static BALANCE: RefCell<u64> = RefCell::new(0);
		static FROZEN: RefCell<u64> = RefCell::new(0);
	}

	/// Minimal single-account, single-freeze-id asset that implements only the required
	/// primitives, so the trait's default method bodies are what gets exercised.
	struct TestAsset;

	impl super::super::Inspect<u64> for TestAsset {
		type AssetId = u32;
		type Balance = u64;
		fn total_issuance(_: u32) -> u64 {
			BALANCE.with(|b| *b.borrow())
		}
		fn minimum_balance(_: u32) -> u64 {
			1
		}
		fn total_balance(_: u32, _: &u64) -> u64 {
			BALANCE.with(|b| *b.borrow())
		}
		fn balance(_: u32, _: &u64) -> u64 {
			BALANCE.with(|b| *b.borrow())
		}
		fn reducible_balance(_: u32, _: &u64, _: Preservation, _: Fortitude) -> u64 {
			BALANCE.with(|b| *b.borrow())
		}
		fn can_deposit(_: u32, _: &u64, _: u64, _: Provenance) -> DepositConsequence {
			DepositConsequence::Success
		}
		fn can_withdraw(_: u32, _: &u64, _: u64) -> WithdrawConsequence<u64> {
			WithdrawConsequence::Success
		}
		fn asset_exists(_: u32) -> bool {
			true
		}
	}

	impl Inspect<u64> for TestAsset {
		type Id = ();
		fn balance_frozen(_: u32, _: &(), _: &u64) -> u64 {
			FROZEN.with(|f| *f.borrow())
		}
		fn can_freeze(_: u32, _: &(), _: &u64) -> bool {
			true
		}
	}

	impl Mutate<u64> for TestAsset {
		fn set_freeze(_: u32, _: &(), _: &u64, amount: u64) -> DispatchResult {
			FROZEN.with(|f| *f.borrow_mut() = amount);
			Ok(())
		}
		fn extend_freeze(_: u32, _: &(), _: &u64, amount: u64) -> DispatchResult {
			FROZEN.with(|f| {
				let mut f = f.borrow_mut();
				*f = (*f).max(amount);
			});
			Ok(())
		}
		fn thaw(_: u32, _: &(), _: &u64) -> DispatchResult {
			FROZEN.with(|f| *f.borrow_mut() = 0);
			Ok(())
		}
	}

	fn setup(balance: u64, frozen: u64) {
		BALANCE.with(|b| *b.borrow_mut() = balance);
		FROZEN.with(|f| *f.borrow_mut() = frozen);
	}

	#[test]
	fn ensure_frozen_is_idempotent_for_existing_over_freeze() {
		// A freeze above the freezable balance is an explicitly permitted state. Re-ensuring at
		// or below the existing freeze does not increase exposure, so it must not fail even
		// though the requested amount exceeds the freezable balance.
		setup(100, 200);
		assert_eq!(TestAsset::ensure_frozen(0, &(), &1, 150, Fortitude::Polite), Ok(()));
		assert_eq!(TestAsset::balance_frozen(0, &(), &1), 200);

		// An actual increase beyond the freezable balance still requires `Force`.
		assert_eq!(
			TestAsset::ensure_frozen(0, &(), &1, 300, Fortitude::Polite),
			Err(TokenError::FundsUnavailable.into())
		);
		assert_eq!(TestAsset::ensure_frozen(0, &(), &1, 300, Fortitude::Force), Ok(()));
		assert_eq!(TestAsset::balance_frozen(0, &(), &1), 300);

		// A plain increase within the freezable balance also still works.
		setup(100, 20);
		assert_eq!(TestAsset::ensure_frozen(0, &(), &1, 80, Fortitude::Polite), Ok(()));
		assert_eq!(TestAsset::balance_frozen(0, &(), &1), 80);
	}

	#[test]
	fn decrease_frozen_works_when_freeze_exceeds_freezable_balance() {
		// Partially releasing an over-freeze reduces exposure and must succeed even while the
		// remaining target is still above the freezable balance.
		setup(100, 200);
		assert_eq!(TestAsset::decrease_frozen(0, &(), &1, 50), Ok(()));
		assert_eq!(TestAsset::balance_frozen(0, &(), &1), 150);

		// Underflow is still rejected.
		assert_eq!(
			TestAsset::decrease_frozen(0, &(), &1, 151),
			Err(ArithmeticError::Underflow.into())
		);
		assert_eq!(TestAsset::balance_frozen(0, &(), &1), 150);
	}
}
