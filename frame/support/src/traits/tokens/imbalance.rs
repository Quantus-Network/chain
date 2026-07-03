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

//! The imbalance trait type and its associates, which handles keeps everything adding up properly
//! with unbalanced operations.

use crate::traits::misc::{SameOrOther, TryDrop};
use core::ops::Div;
use sp_runtime::traits::Saturating;

mod on_unbalanced;
mod signed_imbalance;
mod split_two_ways;
pub use on_unbalanced::{OnUnbalanced, ResolveAssetTo, ResolveTo};
pub use signed_imbalance::SignedImbalance;
pub use split_two_ways::SplitTwoWays;

/// A trait for a not-quite Linear Type that tracks an imbalance.
///
/// Functions that alter account balances return an object of this trait to
/// express how much account balances have been altered in aggregate. If
/// dropped, the currency system will take some default steps to deal with
/// the imbalance (`balances` module simply reduces or increases its
/// total issuance). Your module should generally handle it in some way,
/// good practice is to do so in a configurable manner using an
/// `OnUnbalanced` type for each situation in which your module needs to
/// handle an imbalance.
///
/// Imbalances can either be Positive (funds were added somewhere without
/// being subtracted elsewhere - e.g. a reward) or Negative (funds deducted
/// somewhere without an equal and opposite addition - e.g. a slash or
/// system fee payment).
///
/// Since they are unsigned, the actual type is always Positive or Negative.
/// The trait makes no distinction except to define the `Opposite` type.
///
/// New instances of zero value can be created (`zero`) and destroyed
/// (`drop_zero`).
///
/// Existing instances can be `split` and merged either consuming `self` with
/// `merge` or mutating `self` with `subsume`. If the target is an `Option`,
/// then `maybe_merge` and `maybe_subsume` might work better. Instances can
/// also be `offset` with an `Opposite` that is less than or equal to in value.
///
/// You can always retrieve the raw balance value using `peek`.
#[must_use]
pub trait Imbalance<Balance>: Sized + TryDrop + Default + TryMerge {
	/// The oppositely imbalanced type. They come in pairs.
	type Opposite: Imbalance<Balance>;

	/// The zero imbalance. Can be destroyed with `drop_zero`.
	fn zero() -> Self;

	/// Drop an instance cleanly. Only works if its `self.value()` is zero.
	fn drop_zero(self) -> Result<(), Self>;

	/// Consume `self` and return two independent instances; the first
	/// is guaranteed to be at most `amount` and the second will be the remainder.
	fn split(self, amount: Balance) -> (Self, Self);

	/// Mutate `self` by extracting a new instance with at most `amount` value, reducing `self`
	/// accordingly.
	fn extract(&mut self, amount: Balance) -> Self;

	/// Consume `self` and return two independent instances; the amounts returned will be in
	/// approximately the same ratio as `first`:`second`.
	///
	/// NOTE: This requires up to `first + second` room for a multiply. If `first + second`
	/// overflows a `u32`, both are halved, which preserves the ratio up to rounding. The multiply
	/// will safely saturate on overflow.
	fn ration(self, first: u32, second: u32) -> (Self, Self)
	where
		Balance: From<u32> + Saturating + Div<Output = Balance>,
	{
		// If `first + second` would overflow, halve both: this keeps the ratio (up to rounding)
		// while keeping the denominator exact. Saturating the denominator to `u32::MAX` instead
		// would distort the split, e.g. `MAX:MAX` would send (almost) the whole imbalance to the
		// first leg rather than half of it.
		let (first, second) = if first.checked_add(second).is_none() {
			(first >> 1, second >> 1)
		} else {
			(first, second)
		};
		let total = first + second;
		if total == 0 {
			return (Self::zero(), Self::zero())
		}
		let amount1 = self.peek().saturating_mul(first.into()) / total.into();
		self.split(amount1)
	}

	/// Consume self and add its two components, defined by the first component's balance,
	/// element-wise to two pre-existing Imbalances.
	///
	/// A convenient replacement for `split` and `merge`.
	fn split_merge(self, amount: Balance, others: (Self, Self)) -> (Self, Self) {
		let (a, b) = self.split(amount);
		(a.merge(others.0), b.merge(others.1))
	}

	/// Consume self and add its two components, defined by the ratio `first`:`second`,
	/// element-wise to two pre-existing Imbalances.
	///
	/// A convenient replacement for `split` and `merge`.
	fn ration_merge(self, first: u32, second: u32, others: (Self, Self)) -> (Self, Self)
	where
		Balance: From<u32> + Saturating + Div<Output = Balance>,
	{
		let (a, b) = self.ration(first, second);
		(a.merge(others.0), b.merge(others.1))
	}

	/// Consume self and add its two components, defined by the first component's balance,
	/// element-wise into two pre-existing Imbalance refs.
	///
	/// A convenient replacement for `split` and `subsume`.
	fn split_merge_into(self, amount: Balance, others: &mut (Self, Self)) {
		let (a, b) = self.split(amount);
		others.0.subsume(a);
		others.1.subsume(b);
	}

	/// Consume self and add its two components, defined by the ratio `first`:`second`,
	/// element-wise to two pre-existing Imbalances.
	///
	/// A convenient replacement for `split` and `merge`.
	fn ration_merge_into(self, first: u32, second: u32, others: &mut (Self, Self))
	where
		Balance: From<u32> + Saturating + Div<Output = Balance>,
	{
		let (a, b) = self.ration(first, second);
		others.0.subsume(a);
		others.1.subsume(b);
	}

	/// Consume `self` and an `other` to return a new instance that combines
	/// both.
	fn merge(self, other: Self) -> Self;

	/// Consume self to mutate `other` so that it combines both. Just like `subsume`, only with
	/// reversed arguments.
	fn merge_into(self, other: &mut Self) {
		other.subsume(self)
	}

	/// Consume `self` and maybe an `other` to return a new instance that combines
	/// both.
	fn maybe_merge(self, other: Option<Self>) -> Self {
		if let Some(o) = other {
			self.merge(o)
		} else {
			self
		}
	}

	/// Consume an `other` to mutate `self` into a new instance that combines
	/// both.
	fn subsume(&mut self, other: Self);

	/// Maybe consume an `other` to mutate `self` into a new instance that combines
	/// both.
	fn maybe_subsume(&mut self, other: Option<Self>) {
		if let Some(o) = other {
			self.subsume(o)
		}
	}

	/// Consume self and along with an opposite counterpart to return
	/// a combined result.
	///
	/// Returns `Ok` along with a new instance of `Self` if this instance has a
	/// greater value than the `other`. Otherwise returns `Err` with an instance of
	/// the `Opposite`. In both cases the value represents the combination of `self`
	/// and `other`.
	fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite>;

	/// The raw value of self.
	fn peek(&self) -> Balance;
}

/// Try to merge two imbalances.
pub trait TryMerge: Sized {
	/// Consume `self` and an `other` to return a new instance that combines both. Errors with
	/// Err(self, other) if the imbalances cannot be merged (e.g. imbalances of different assets).
	fn try_merge(self, other: Self) -> Result<Self, (Self, Self)>;
}

#[cfg(feature = "std")]
impl<Balance: Default> Imbalance<Balance> for () {
	type Opposite = ();
	fn zero() -> Self {
		()
	}
	fn drop_zero(self) -> Result<(), Self> {
		Ok(())
	}
	fn split(self, _: Balance) -> (Self, Self) {
		((), ())
	}
	fn extract(&mut self, _: Balance) -> Self {
		()
	}
	fn ration(self, _: u32, _: u32) -> (Self, Self)
	where
		Balance: From<u32> + Saturating + Div<Output = Balance>,
	{
		((), ())
	}
	fn split_merge(self, _: Balance, _: (Self, Self)) -> (Self, Self) {
		((), ())
	}
	fn ration_merge(self, _: u32, _: u32, _: (Self, Self)) -> (Self, Self)
	where
		Balance: From<u32> + Saturating + Div<Output = Balance>,
	{
		((), ())
	}
	fn split_merge_into(self, _: Balance, _: &mut (Self, Self)) {}
	fn ration_merge_into(self, _: u32, _: u32, _: &mut (Self, Self))
	where
		Balance: From<u32> + Saturating + Div<Output = Balance>,
	{
	}
	fn merge(self, _: Self) -> Self {
		()
	}
	fn merge_into(self, _: &mut Self) {}
	fn maybe_merge(self, _: Option<Self>) -> Self {
		()
	}
	fn subsume(&mut self, _: Self) {}
	fn maybe_subsume(&mut self, _: Option<Self>) {
		()
	}
	fn offset(self, _: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
		SameOrOther::None
	}
	fn peek(&self) -> Balance {
		Default::default()
	}
}

#[cfg(feature = "std")]
impl TryMerge for () {
	fn try_merge(self, _: Self) -> Result<Self, (Self, Self)> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use core::cell::RefCell;

	/// Minimal imbalance over `u64` so the trait's default method bodies are what gets
	/// exercised.
	#[derive(Default, Debug, PartialEq, Eq)]
	struct TestImbalance(u64);

	impl TryDrop for TestImbalance {
		fn try_drop(self) -> Result<(), Self> {
			if self.0 == 0 {
				Ok(())
			} else {
				Err(self)
			}
		}
	}

	impl TryMerge for TestImbalance {
		fn try_merge(self, other: Self) -> Result<Self, (Self, Self)> {
			Ok(Self(self.0 + other.0))
		}
	}

	impl Imbalance<u64> for TestImbalance {
		type Opposite = TestImbalance;
		fn zero() -> Self {
			Self(0)
		}
		fn drop_zero(self) -> Result<(), Self> {
			self.try_drop()
		}
		fn split(self, amount: u64) -> (Self, Self) {
			let first = self.0.min(amount);
			(Self(first), Self(self.0 - first))
		}
		fn extract(&mut self, amount: u64) -> Self {
			let extracted = self.0.min(amount);
			self.0 -= extracted;
			Self(extracted)
		}
		fn merge(self, other: Self) -> Self {
			Self(self.0 + other.0)
		}
		fn subsume(&mut self, other: Self) {
			self.0 += other.0
		}
		fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
			if self.0 >= other.0 {
				SameOrOther::Same(Self(self.0 - other.0))
			} else {
				SameOrOther::Other(Self(other.0 - self.0))
			}
		}
		fn peek(&self) -> u64 {
			self.0
		}
	}

	#[test]
	fn ration_splits_proportionally() {
		let (a, b) = TestImbalance(600).ration(1, 2);
		assert_eq!((a.peek(), b.peek()), (200, 400));

		let (a, b) = TestImbalance(600).ration(0, 0);
		assert_eq!((a.peek(), b.peek()), (0, 0));
	}

	#[test]
	fn ration_keeps_ratio_when_sum_overflows() {
		// `MAX:MAX` must behave as `1:1`. A denominator saturated to `u32::MAX` would instead
		// compute `peek * MAX / MAX` and send the whole imbalance to the first leg.
		let (a, b) = TestImbalance(1000).ration(u32::MAX, u32::MAX);
		assert_eq!((a.peek(), b.peek()), (500, 500));

		// Heavily skewed overflowing ratios keep their skew.
		let (a, b) = TestImbalance(1000).ration(1, u32::MAX);
		assert_eq!((a.peek(), b.peek()), (0, 1000));
		let (a, b) = TestImbalance(1000).ration(u32::MAX, 1);
		assert_eq!((a.peek(), b.peek()), (1000, 0));
	}

	thread_local! {
		static SPLIT_SINK: RefCell<(u64, u64)> = RefCell::new((0, 0));
	}

	struct Sink1;
	impl OnUnbalanced<TestImbalance> for Sink1 {
		fn on_nonzero_unbalanced(amount: TestImbalance) {
			SPLIT_SINK.with(|s| s.borrow_mut().0 += amount.peek());
		}
	}
	struct Sink2;
	impl OnUnbalanced<TestImbalance> for Sink2 {
		fn on_nonzero_unbalanced(amount: TestImbalance) {
			SPLIT_SINK.with(|s| s.borrow_mut().1 += amount.peek());
		}
	}

	#[test]
	fn split_two_ways_splits_by_configured_parts() {
		SPLIT_SINK.with(|s| *s.borrow_mut() = (0, 0));
		<SplitTwoWays<u64, TestImbalance, Sink1, Sink2, 3, 1> as OnUnbalanced<_>>::on_unbalanced(
			TestImbalance(100),
		);
		assert_eq!(SPLIT_SINK.with(|s| *s.borrow()), (75, 25));
	}
}
