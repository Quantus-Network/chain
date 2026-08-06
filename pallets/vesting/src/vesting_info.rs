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

//! Module to enforce private fields on `VestingInfo`.
//!
//! Quantus fork: schedules are denominated in wall-clock time (`Moment`, milliseconds
//! since the unix epoch as reported by `pallet_timestamp`) instead of block numbers.

use super::*;

/// Struct to encode the vesting schedule of an individual account.
#[derive(
	Encode,
	Decode,
	DecodeWithMemTracking,
	Copy,
	Clone,
	PartialEq,
	Eq,
	RuntimeDebug,
	MaxEncodedLen,
	TypeInfo,
)]
pub struct VestingInfo<Balance, Moment, AccountId> {
	/// Locked amount at schedule creation.
	locked: Balance,
	/// Amount that gets unlocked every millisecond after `start`.
	per_ms: Balance,
	/// Time (milliseconds since the unix epoch) at which unlocking (vesting) starts.
	start: Moment,
	/// Account allowed to repurchase this schedule via `force_remove_vesting_schedule`,
	/// in addition to Root. `None` means only Root can remove it.
	///
	/// When the repurchaser repurchases the schedule, the funds still unvested at that time
	/// are transferred to them; the already-vested portion stays with the schedule's holder.
	repurchaser: Option<AccountId>,
}

impl<Balance, Moment, AccountId> VestingInfo<Balance, Moment, AccountId>
where
	Balance: AtLeast32BitUnsigned + Copy,
	Moment: AtLeast32BitUnsigned + Copy + Bounded,
	AccountId: Clone + PartialEq,
{
	/// Instantiate a new `VestingInfo` without a repurchaser (only Root may force-remove it).
	pub fn new(
		locked: Balance,
		per_ms: Balance,
		start: Moment,
	) -> VestingInfo<Balance, Moment, AccountId> {
		VestingInfo { locked, per_ms, start, repurchaser: None }
	}

	/// Set the account allowed to repurchase this schedule (in addition to Root).
	pub fn with_repurchaser(mut self, repurchaser: AccountId) -> Self {
		self.repurchaser = Some(repurchaser);
		self
	}

	/// The account allowed to repurchase this schedule, if any.
	pub fn repurchaser(&self) -> Option<&AccountId> {
		self.repurchaser.as_ref()
	}

	/// Validate parameters for `VestingInfo`. Note that this does not check
	/// against `MinVestedTransfer`.
	pub fn is_valid(&self) -> bool {
		!self.locked.is_zero() && !self.raw_per_ms().is_zero()
	}

	/// Locked amount at schedule creation.
	pub fn locked(&self) -> Balance {
		self.locked
	}

	/// Amount that gets unlocked every millisecond after `start`. Corrects for `per_ms` of 0.
	/// We don't let `per_ms` be less than 1, or else the vesting will never end.
	/// This should be used whenever accessing `per_ms` unless explicitly checking for 0 values.
	pub fn per_ms(&self) -> Balance {
		self.per_ms.max(One::one())
	}

	/// Get the unmodified `per_ms`. Generally should not be used, but is useful for
	/// validating `per_ms`.
	pub(crate) fn raw_per_ms(&self) -> Balance {
		self.per_ms
	}

	/// Time at which unlocking (vesting) starts.
	pub fn start(&self) -> Moment {
		self.start
	}

	/// Amount locked at time `now`.
	pub fn locked_at<MomentToBalance: Convert<Moment, Balance>>(&self, now: Moment) -> Balance {
		// Milliseconds that count toward vesting;
		// saturating to 0 when now < start.
		let vested_ms = now.saturating_sub(self.start);
		let vested_ms = MomentToBalance::convert(vested_ms);
		// Return amount that is still locked in vesting.
		vested_ms
			.checked_mul(&self.per_ms()) // `per_ms` accessor guarantees at least 1.
			.map(|to_unlock| self.locked.saturating_sub(to_unlock))
			.unwrap_or(Zero::zero())
	}

	/// Time at which the schedule ends (as type `Balance`).
	pub fn ending_time_as_balance<MomentToBalance: Convert<Moment, Balance>>(&self) -> Balance {
		let start = MomentToBalance::convert(self.start);
		let duration = if self.per_ms() >= self.locked {
			// If `per_ms` is bigger than `locked`, the schedule will end
			// the millisecond after starting.
			One::one()
		} else {
			self.locked / self.per_ms() +
				if (self.locked % self.per_ms()).is_zero() {
					Zero::zero()
				} else {
					// `per_ms` does not perfectly divide `locked`, so we need an extra tick to
					// unlock some amount less than `per_ms`.
					One::one()
				}
		};

		start.saturating_add(duration)
	}
}
