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

//! # Vesting Pallet
//!
//! - [`Config`]
//! - [`Call`]
//!
//! ## Overview
//!
//! A simple pallet providing a means of placing a linear curve on an account's locked balance. This
//! pallet ensures that there is a lock in place preventing the balance to drop below the *unvested*
//! amount for any reason other than the ones specified in `UnvestedFundsAllowedWithdrawReasons`
//! configuration value.
//!
//! Quantus fork: schedules unlock against wall-clock time (`Config::Moment`, milliseconds since
//! the unix epoch as reported by `Config::TimeProvider` / `pallet_timestamp`) instead of block
//! numbers, mirroring how the scheduler fork supports timestamp-based dispatch. The rate field of
//! a schedule is "amount unlocked per millisecond".
//!
//! As the amount vested increases over time, the amount unvested reduces. However, locks remain in
//! place and explicit action is needed on behalf of the user to ensure that the amount locked is
//! equivalent to the amount remaining to be vested. This is done through a dispatchable function,
//! either `vest` (in typical case where the sender is calling on their own behalf) or `vest_other`
//! in case the sender is calling on another account's behalf.
//!
//! ## Interface
//!
//! This pallet implements the `VestingSchedule` trait.
//!
//! ### Dispatchable Functions
//!
//! - `vest` - Update the lock, reducing it in line with the amount "vested" so far.
//! - `vest_other` - Update the lock of another account, reducing it in line with the amount
//!   "vested" so far.

#![cfg_attr(not(feature = "std"), no_std)]

mod benchmarking;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
mod vesting_info;

pub mod migrations;
pub mod weights;

extern crate alloc;

use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use core::{fmt::Debug, marker::PhantomData};
use frame_support::{
	dispatch::DispatchResult,
	ensure,
	storage::bounded_vec::BoundedVec,
	traits::{
		Currency, ExistenceRequirement, Get, LockIdentifier, LockableCurrency, Time,
		VestedTransfer, VestingSchedule, WithdrawReasons,
	},
	weights::Weight,
};
use scale_info::TypeInfo;
use sp_runtime::{
	traits::{
		AtLeast32BitUnsigned, Bounded, Convert, MaybeSerializeDeserialize, One, Saturating,
		StaticLookup, Zero,
	},
	DispatchError, RuntimeDebug,
};

pub use pallet::*;
pub use vesting_info::*;
pub use weights::WeightInfo;

type BalanceOf<T> =
	<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
/// The time unit vesting schedules are denominated in (milliseconds since the unix epoch).
pub type MomentOf<T> = <T as Config>::Moment;
/// The vesting schedule type used by this pallet.
pub type VestingInfoOf<T> =
	VestingInfo<BalanceOf<T>, MomentOf<T>, <T as frame_system::Config>::AccountId>;
type MaxLocksOf<T> =
	<<T as Config>::Currency as LockableCurrency<<T as frame_system::Config>::AccountId>>::MaxLocks;
type AccountIdLookupOf<T> = <<T as frame_system::Config>::Lookup as StaticLookup>::Source;

const VESTING_ID: LockIdentifier = *b"vesting ";

// A value placed in storage that represents the current version of the Vesting storage.
// This value is used by `on_runtime_upgrade` to determine whether we run storage migration logic.
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, RuntimeDebug, MaxEncodedLen, TypeInfo)]
pub enum Releases {
	V0,
	V1,
}

impl Default for Releases {
	fn default() -> Self {
		Releases::V0
	}
}

/// Actions to take against a user's `Vesting` storage entry.
#[derive(Clone, Copy)]
enum VestingAction {
	/// Do not actively remove any schedules.
	Passive,
	/// Remove the schedule specified by the index.
	Remove { index: usize },
	/// Remove the two schedules, specified by index, so they can be merged.
	Merge { index1: usize, index2: usize },
}

impl VestingAction {
	/// Whether or not the filter says the schedule index should be removed.
	fn should_remove(&self, index: usize) -> bool {
		match self {
			Self::Passive => false,
			Self::Remove { index: index1 } => *index1 == index,
			Self::Merge { index1, index2 } => *index1 == index || *index2 == index,
		}
	}

	/// Pick the schedules that this action dictates should continue vesting undisturbed.
	fn pick_schedules<T: Config>(
		&self,
		schedules: Vec<VestingInfoOf<T>>,
	) -> impl Iterator<Item = VestingInfoOf<T>> + '_ {
		schedules.into_iter().enumerate().filter_map(move |(index, schedule)| {
			if self.should_remove(index) {
				None
			} else {
				Some(schedule)
			}
		})
	}
}

// Wrapper for `T::MAX_VESTING_SCHEDULES` to satisfy `trait Get`.
pub struct MaxVestingSchedulesGet<T>(PhantomData<T>);
impl<T: Config> Get<u32> for MaxVestingSchedulesGet<T> {
	fn get() -> u32 {
		T::MAX_VESTING_SCHEDULES
	}
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The currency trait.
		type Currency: LockableCurrency<Self::AccountId>;

		/// The time unit used by vesting schedules: milliseconds since the unix epoch, as
		/// reported by [`Config::TimeProvider`].
		type Moment: AtLeast32BitUnsigned
			+ Parameter
			+ Default
			+ Copy
			+ MaxEncodedLen
			+ Bounded
			+ MaybeSerializeDeserialize;

		/// Query the current time. Vesting schedules unlock against this clock.
		///
		/// Must return monotonically increasing values when called from consecutive blocks
		/// (`pallet_timestamp` guarantees this).
		type TimeProvider: Time<Moment = Self::Moment>;

		/// Convert a moment into a balance.
		type MomentToBalance: Convert<Self::Moment, BalanceOf<Self>>;

		/// The minimum amount transferred to call `vested_transfer`.
		#[pallet::constant]
		type MinVestedTransfer: Get<BalanceOf<Self>>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Reasons that determine under which conditions the balance may drop below
		/// the unvested amount.
		type UnvestedFundsAllowedWithdrawReasons: Get<WithdrawReasons>;

		/// Maximum number of vesting schedules an account may have at a given moment.
		const MAX_VESTING_SCHEDULES: u32;
	}

	#[pallet::extra_constants]
	impl<T: Config> Pallet<T> {
		#[pallet::constant_name(MaxVestingSchedules)]
		fn max_vesting_schedules() -> u32 {
			T::MAX_VESTING_SCHEDULES
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			assert!(T::MAX_VESTING_SCHEDULES > 0, "`MaxVestingSchedules` must be greater than 0");
		}
	}

	/// Information regarding the vesting of a given account.
	#[pallet::storage]
	pub type Vesting<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		BoundedVec<VestingInfoOf<T>, MaxVestingSchedulesGet<T>>,
	>;

	/// Storage version of the pallet.
	///
	/// New networks start with latest version, as determined by the genesis build.
	#[pallet::storage]
	pub type StorageVersion<T: Config> = StorageValue<_, Releases, ValueQuery>;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		pub vesting:
			Vec<(T::AccountId, MomentOf<T>, MomentOf<T>, BalanceOf<T>, Option<T::AccountId>)>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			use sp_runtime::traits::Saturating;

			// Genesis uses the latest storage version.
			StorageVersion::<T>::put(Releases::V1);

			// Generate initial vesting configuration
			// * who - Account which we are generating vesting configuration for
			// * begin - Time (ms since the unix epoch) when the account will start to vest
			// * length - Number of milliseconds from `begin` until fully vested
			// * liquid - Number of units which can be spent before vesting begins
			// * repurchaser - Optional account allowed to remove the schedule (besides Root)
			for &(ref who, begin, length, liquid, ref repurchaser) in self.vesting.iter() {
				let balance = T::Currency::free_balance(who);
				assert!(!balance.is_zero(), "Currencies must be init'd before vesting");
				// Total genesis `balance` minus `liquid` equals funds locked for vesting
				let locked = balance.saturating_sub(liquid);
				let length_as_balance = T::MomentToBalance::convert(length);
				let per_ms = locked / length_as_balance.max(sp_runtime::traits::One::one());
				let mut vesting_info = VestingInfo::new(locked, per_ms, begin);
				if let Some(repurchaser) = repurchaser {
					vesting_info = vesting_info.with_repurchaser(repurchaser.clone());
				}
				if !vesting_info.is_valid() {
					panic!("Invalid VestingInfo params at genesis")
				};

				Vesting::<T>::try_append(who, vesting_info)
					.expect("Too many vesting schedules at genesis.");

				let reasons =
					WithdrawReasons::except(T::UnvestedFundsAllowedWithdrawReasons::get());

				T::Currency::set_lock(VESTING_ID, who, locked, reasons);
			}
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A vesting schedule has been created.
		VestingCreated { account: T::AccountId, schedule_index: u32 },
		/// The amount vested has been updated. This could indicate a change in funds available.
		/// The balance given is the amount which is left unvested (and thus locked).
		VestingUpdated { account: T::AccountId, unvested: BalanceOf<T> },
		/// An \[account\] has become fully vested.
		VestingCompleted { account: T::AccountId },
		/// A vesting schedule was moved from one account to another without changing its terms.
		VestingTransferred { from: T::AccountId, to: T::AccountId, schedule_index: u32 },
	}

	/// Error for the vesting pallet.
	#[pallet::error]
	pub enum Error<T> {
		/// The account given is not vesting.
		NotVesting,
		/// The account already has `MaxVestingSchedules` count of schedules and thus
		/// cannot add another one. Consider merging existing schedules in order to add another.
		AtMaxVestingSchedules,
		/// Amount being transferred is too low to create a vesting schedule.
		AmountLow,
		/// An index was out of bounds of the vesting schedules.
		ScheduleIndexOutOfBounds,
		/// Failed to create a new schedule because some parameter was invalid.
		InvalidScheduleParams,
		/// The two schedules cannot be merged because their repurchasers differ.
		RepurchaserMismatch,
		/// Source and destination accounts for a schedule transfer must differ.
		SameAccount,
		/// The schedule has already fully vested; there is nothing left to transfer.
		ScheduleFullyVested,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Unlock any vested funds of the sender account.
		///
		/// The dispatch origin for this call must be _Signed_ and the sender must have funds still
		/// locked under this pallet.
		///
		/// Emits either `VestingCompleted` or `VestingUpdated`.
		///
		/// ## Complexity
		/// - `O(1)`.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::vest_locked(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES)
			.max(T::WeightInfo::vest_unlocked(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES))
		)]
		pub fn vest(origin: OriginFor<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::do_vest(who)
		}

		/// Unlock any vested funds of a `target` account.
		///
		/// The dispatch origin for this call must be _Signed_.
		///
		/// - `target`: The account whose vested funds should be unlocked. Must have funds still
		/// locked under this pallet.
		///
		/// Emits either `VestingCompleted` or `VestingUpdated`.
		///
		/// ## Complexity
		/// - `O(1)`.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::vest_other_locked(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES)
			.max(T::WeightInfo::vest_other_unlocked(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES))
		)]
		pub fn vest_other(origin: OriginFor<T>, target: AccountIdLookupOf<T>) -> DispatchResult {
			ensure_signed(origin)?;
			let who = T::Lookup::lookup(target)?;
			Self::do_vest(who)
		}

		/// Create a vested transfer.
		///
		/// The dispatch origin for this call must be _Signed_.
		///
		/// - `target`: The account receiving the vested funds.
		/// - `schedule`: The vesting schedule attached to the transfer.
		///
		/// Emits `VestingCreated`.
		///
		/// NOTE: This will unlock all schedules through the current time.
		///
		/// ## Complexity
		/// - `O(1)`.
		#[pallet::call_index(2)]
		#[pallet::weight(
			T::WeightInfo::vested_transfer(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES)
		)]
		pub fn vested_transfer(
			origin: OriginFor<T>,
			target: AccountIdLookupOf<T>,
			schedule: VestingInfoOf<T>,
		) -> DispatchResult {
			let transactor = ensure_signed(origin)?;
			let target = T::Lookup::lookup(target)?;
			Self::do_vested_transfer(&transactor, &target, schedule)
		}

		/// Force a vested transfer.
		///
		/// The dispatch origin for this call must be _Root_.
		///
		/// - `source`: The account whose funds should be transferred.
		/// - `target`: The account that should be transferred the vested funds.
		/// - `schedule`: The vesting schedule attached to the transfer.
		///
		/// Emits `VestingCreated`.
		///
		/// NOTE: This will unlock all schedules through the current time.
		///
		/// ## Complexity
		/// - `O(1)`.
		#[pallet::call_index(3)]
		#[pallet::weight(
			T::WeightInfo::force_vested_transfer(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES)
		)]
		pub fn force_vested_transfer(
			origin: OriginFor<T>,
			source: AccountIdLookupOf<T>,
			target: AccountIdLookupOf<T>,
			schedule: VestingInfoOf<T>,
		) -> DispatchResult {
			ensure_root(origin)?;
			let target = T::Lookup::lookup(target)?;
			let source = T::Lookup::lookup(source)?;
			Self::do_vested_transfer(&source, &target, schedule)
		}

		/// Merge two vesting schedules together, creating a new vesting schedule that unlocks over
		/// the highest possible start and end times. If both schedules have already started the
		/// current time will be used as the schedule start; with the caveat that if one schedule
		/// is finished by the current time, the other will be treated as the new merged schedule,
		/// unmodified.
		///
		/// NOTE: If `schedule1_index == schedule2_index` this is a no-op.
		/// NOTE: This will unlock all schedules through the current time prior to merging.
		/// NOTE: If both schedules have ended by the current time, no new schedule will be created
		/// and both will be removed.
		///
		/// Merged schedule attributes:
		/// - `start`: `MAX(schedule1.start, scheduled2.start, now)`.
		/// - `ending_time`: `MAX(schedule1.ending_time, schedule2.ending_time)`.
		/// - `locked`: `schedule1.locked_at(now) + schedule2.locked_at(now)`.
		///
		/// Both schedules must have the same repurchaser (or both none), otherwise the merge
		/// fails with `RepurchaserMismatch`; the merged schedule keeps that repurchaser.
		///
		/// The dispatch origin for this call must be _Signed_.
		///
		/// - `schedule1_index`: index of the first schedule to merge.
		/// - `schedule2_index`: index of the second schedule to merge.
		#[pallet::call_index(4)]
		#[pallet::weight(
			T::WeightInfo::not_unlocking_merge_schedules(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES)
			.max(T::WeightInfo::unlocking_merge_schedules(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES))
		)]
		pub fn merge_schedules(
			origin: OriginFor<T>,
			schedule1_index: u32,
			schedule2_index: u32,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			if schedule1_index == schedule2_index {
				return Ok(())
			};
			let schedule1_index = schedule1_index as usize;
			let schedule2_index = schedule2_index as usize;

			let schedules = Vesting::<T>::get(&who).ok_or(Error::<T>::NotVesting)?;
			{
				let schedule1 =
					schedules.get(schedule1_index).ok_or(Error::<T>::ScheduleIndexOutOfBounds)?;
				let schedule2 =
					schedules.get(schedule2_index).ok_or(Error::<T>::ScheduleIndexOutOfBounds)?;
				ensure!(
					schedule1.repurchaser() == schedule2.repurchaser(),
					Error::<T>::RepurchaserMismatch
				);
			}
			let merge_action =
				VestingAction::Merge { index1: schedule1_index, index2: schedule2_index };

			let (schedules, locked_now) = Self::exec_action(schedules.to_vec(), merge_action)?;

			Self::write_vesting(&who, schedules)?;
			Self::write_lock(&who, locked_now);

			Ok(())
		}

		/// Force remove a vesting schedule.
		///
		/// The dispatch origin for this call must be _Root_, or _Signed_ by the schedule's
		/// designated repurchaser (if it has one).
		///
		/// When called by the repurchaser, the funds still unvested at the current time are
		/// transferred to the repurchaser; only the already-vested portion stays with `target`.
		/// When called by Root, the schedule is simply removed and all funds stay with
		/// `target` (unlocked).
		///
		/// - `target`: An account that has a vesting schedule
		/// - `schedule_index`: The vesting schedule index that should be removed
		#[pallet::call_index(5)]
		#[pallet::weight(
			T::WeightInfo::force_remove_vesting_schedule(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES)
		)]
		pub fn force_remove_vesting_schedule(
			origin: OriginFor<T>,
			target: <T::Lookup as StaticLookup>::Source,
			schedule_index: u32,
		) -> DispatchResultWithPostInfo {
			let who = T::Lookup::lookup(target)?;
			let schedules_count = Vesting::<T>::decode_len(&who).unwrap_or_default();

			// Root: unlock in place. Repurchaser: receive the still-unvested funds.
			let (schedule, maybe_signer) =
				Self::ensure_can_manage_schedule(origin, &who, schedule_index)?;
			let payout_to = maybe_signer.as_ref();

			Self::detach_schedule_and_pay_unvested(&who, schedule_index, &schedule, payout_to)?;

			Ok(Some(T::WeightInfo::force_remove_vesting_schedule(
				MaxLocksOf::<T>::get(),
				schedules_count as u32,
			))
			.into())
		}

		/// Transfer a vesting schedule from one account to another without modifying its terms.
		///
		/// The dispatch origin for this call must be _Root_, or _Signed_ by the schedule's
		/// designated repurchaser. Intended for recovering a schedule when the holder loses
		/// access to their wallet, or for moving a schedule onto a new account (e.g. a multisig).
		///
		/// The schedule itself (`locked`, `per_ms`, `start`, `repurchaser`) is preserved
		/// verbatim. The funds still unvested at the current time move with it to `dest`;
		/// any already-vested (unlocked) balance stays with `source`.
		///
		/// - `source`: Account currently holding the schedule
		/// - `dest`: Account that will receive the schedule and the unvested funds
		/// - `schedule_index`: Index of the schedule on `source` to transfer
		#[pallet::call_index(6)]
		#[pallet::weight(
			T::WeightInfo::transfer_vesting_schedule(MaxLocksOf::<T>::get(), T::MAX_VESTING_SCHEDULES)
		)]
		pub fn transfer_vesting_schedule(
			origin: OriginFor<T>,
			source: <T::Lookup as StaticLookup>::Source,
			dest: <T::Lookup as StaticLookup>::Source,
			schedule_index: u32,
		) -> DispatchResult {
			let source = T::Lookup::lookup(source)?;
			let dest = T::Lookup::lookup(dest)?;
			ensure!(source != dest, Error::<T>::SameAccount);

			let (schedule, _) = Self::ensure_can_manage_schedule(origin, &source, schedule_index)?;

			// A fully-vested schedule has nothing left to move; reject rather than let it
			// silently vanish on arrival (zero-locked schedules are filtered out on write).
			let now = T::TimeProvider::now();
			ensure!(
				!schedule.locked_at::<T::MomentToBalance>(now).is_zero(),
				Error::<T>::ScheduleFullyVested
			);

			// Ensure the destination can accept another schedule before mutating storage.
			Self::can_add_vesting_schedule(
				&dest,
				schedule.locked(),
				schedule.per_ms(),
				schedule.start(),
			)?;

			Self::detach_schedule_and_pay_unvested(
				&source,
				schedule_index,
				&schedule,
				Some(&dest),
			)?;

			// Re-attach the identical schedule on the destination.
			Self::do_add_vesting_schedule(&dest, schedule)?;

			Self::deposit_event(Event::<T>::VestingTransferred {
				from: source,
				to: dest,
				schedule_index,
			});

			Ok(())
		}
	}
}

impl<T: Config> Pallet<T> {
	// Public function for accessing vesting storage
	pub fn vesting(
		account: T::AccountId,
	) -> Option<BoundedVec<VestingInfoOf<T>, MaxVestingSchedulesGet<T>>> {
		Vesting::<T>::get(account)
	}

	/// Load `who`'s schedule at `schedule_index` and ensure `origin` is Root or that
	/// schedule's designated repurchaser.
	///
	/// Returns the schedule and `Some(signer)` when the origin was signed (the
	/// repurchaser), or `None` when the origin was Root.
	fn ensure_can_manage_schedule(
		origin: frame_system::pallet_prelude::OriginFor<T>,
		who: &T::AccountId,
		schedule_index: u32,
	) -> Result<(VestingInfoOf<T>, Option<T::AccountId>), DispatchError> {
		let maybe_signer = frame_system::ensure_signed_or_root(origin)?;
		let schedules = Vesting::<T>::get(who).ok_or(Error::<T>::NotVesting)?;
		let schedule = schedules
			.get(schedule_index as usize)
			.cloned()
			.ok_or(Error::<T>::InvalidScheduleParams)?;

		if let Some(ref signer) = maybe_signer {
			ensure!(schedule.repurchaser() == Some(signer), DispatchError::BadOrigin);
		}

		Ok((schedule, maybe_signer))
	}

	/// Remove `schedule_index` from `who` and, if `payout_to` is set, transfer the
	/// still-unvested amount to that account. Removing first drops the vesting lock by
	/// exactly that amount so the transfer cannot be blocked by it.
	fn detach_schedule_and_pay_unvested(
		who: &T::AccountId,
		schedule_index: u32,
		schedule: &VestingInfoOf<T>,
		payout_to: Option<&T::AccountId>,
	) -> DispatchResult {
		let now = T::TimeProvider::now();
		let unvested = schedule.locked_at::<T::MomentToBalance>(now);

		Self::remove_vesting_schedule(who, schedule_index)?;

		if let Some(dest) = payout_to {
			if !unvested.is_zero() {
				T::Currency::transfer(who, dest, unvested, ExistenceRequirement::AllowDeath)?;
			}
		}

		Ok(())
	}

	// Create a new `VestingInfo`, based off of two other `VestingInfo`s.
	// NOTE: We assume both schedules have had funds unlocked up through the current time.
	fn merge_vesting_info(
		now: MomentOf<T>,
		schedule1: VestingInfoOf<T>,
		schedule2: VestingInfoOf<T>,
	) -> Option<VestingInfoOf<T>> {
		let schedule1_ending_time = schedule1.ending_time_as_balance::<T::MomentToBalance>();
		let schedule2_ending_time = schedule2.ending_time_as_balance::<T::MomentToBalance>();
		let now_as_balance = T::MomentToBalance::convert(now);

		// Check if one or both schedules have ended.
		match (schedule1_ending_time <= now_as_balance, schedule2_ending_time <= now_as_balance) {
			// If both schedules have ended, we don't merge and exit early.
			(true, true) => return None,
			// If one schedule has ended, we treat the one that has not ended as the new
			// merged schedule.
			(true, false) => return Some(schedule2),
			(false, true) => return Some(schedule1),
			// If neither schedule has ended don't exit early.
			_ => {},
		}

		let locked = schedule1
			.locked_at::<T::MomentToBalance>(now)
			.saturating_add(schedule2.locked_at::<T::MomentToBalance>(now));
		// This shouldn't happen because we know at least one ending time is greater than now,
		// thus at least a schedule has some locked balance.
		debug_assert!(
			!locked.is_zero(),
			"merge_vesting_info validation checks failed to catch a locked of 0"
		);

		let ending_time = schedule1_ending_time.max(schedule2_ending_time);
		let start = now.max(schedule1.start()).max(schedule2.start());

		let per_ms = {
			let duration =
				ending_time.saturating_sub(T::MomentToBalance::convert(start)).max(One::one());
			(locked / duration).max(One::one())
		};

		// Both schedules are required to share the same repurchaser before merging,
		// so the merged schedule inherits it from either one.
		let mut schedule = VestingInfo::new(locked, per_ms, start);
		if let Some(repurchaser) = schedule1.repurchaser() {
			schedule = schedule.with_repurchaser(repurchaser.clone());
		}
		debug_assert!(schedule.is_valid(), "merge_vesting_info schedule validation check failed");

		Some(schedule)
	}

	// Execute a vested transfer from `source` to `target` with the given `schedule`.
	fn do_vested_transfer(
		source: &T::AccountId,
		target: &T::AccountId,
		schedule: VestingInfoOf<T>,
	) -> DispatchResult {
		// Validate user inputs.
		ensure!(schedule.locked() >= T::MinVestedTransfer::get(), Error::<T>::AmountLow);
		if !schedule.is_valid() {
			return Err(Error::<T>::InvalidScheduleParams.into())
		};

		// Check we can add to this account prior to any storage writes.
		Self::can_add_vesting_schedule(
			target,
			schedule.locked(),
			schedule.per_ms(),
			schedule.start(),
		)?;

		T::Currency::transfer(source, target, schedule.locked(), ExistenceRequirement::AllowDeath)?;

		// We can't let this fail because the currency transfer has already happened.
		// Must be successful as it has been checked before.
		// Better to return error on failure anyway.
		// Note: we pass the full schedule so a designated repurchaser is preserved.
		let res = Self::do_add_vesting_schedule(target, schedule);
		debug_assert!(res.is_ok(), "Failed to add a schedule when we had to succeed.");

		Ok(())
	}

	/// Adds a vesting schedule to a given account, keeping all schedule attributes
	/// (including any designated repurchaser).
	///
	/// See [`VestingSchedule::add_vesting_schedule`] for details.
	fn do_add_vesting_schedule(
		who: &T::AccountId,
		vesting_schedule: VestingInfoOf<T>,
	) -> DispatchResult {
		if vesting_schedule.locked().is_zero() {
			return Ok(())
		}

		// Check for `per_ms` or `locked` of 0.
		if !vesting_schedule.is_valid() {
			return Err(Error::<T>::InvalidScheduleParams.into())
		};

		let mut schedules = Vesting::<T>::get(who).unwrap_or_default();

		// NOTE: we must push the new schedule so that `exec_action`
		// will give the correct new locked amount.
		ensure!(schedules.try_push(vesting_schedule).is_ok(), Error::<T>::AtMaxVestingSchedules);

		debug_assert!(schedules.len() > 0, "schedules cannot be empty after insertion");
		let schedule_index = schedules.len() - 1;
		Self::deposit_event(Event::<T>::VestingCreated {
			account: who.clone(),
			schedule_index: schedule_index as u32,
		});

		let (schedules, locked_now) =
			Self::exec_action(schedules.to_vec(), VestingAction::Passive)?;

		Self::write_vesting(who, schedules)?;
		Self::write_lock(who, locked_now);

		Ok(())
	}

	/// Iterate through the schedules to track the current locked amount and
	/// filter out completed and specified schedules.
	///
	/// Returns a tuple that consists of:
	/// - Vec of vesting schedules, where completed schedules and those specified
	/// 	by filter are removed. (Note the vec is not checked for respecting
	/// 	bounded length.)
	/// - The amount locked at the current time based on the given schedules.
	///
	/// NOTE: the amount locked does not include any schedules that are filtered out via `action`.
	fn report_schedule_updates(
		schedules: Vec<VestingInfoOf<T>>,
		action: VestingAction,
	) -> (Vec<VestingInfoOf<T>>, BalanceOf<T>) {
		let now = T::TimeProvider::now();

		let mut total_locked_now: BalanceOf<T> = Zero::zero();
		let filtered_schedules = action
			.pick_schedules::<T>(schedules)
			.filter(|schedule| {
				let locked_now = schedule.locked_at::<T::MomentToBalance>(now);
				let keep = !locked_now.is_zero();
				if keep {
					total_locked_now = total_locked_now.saturating_add(locked_now);
				}
				keep
			})
			.collect::<Vec<_>>();

		(filtered_schedules, total_locked_now)
	}

	/// Write an accounts updated vesting lock to storage.
	fn write_lock(who: &T::AccountId, total_locked_now: BalanceOf<T>) {
		if total_locked_now.is_zero() {
			T::Currency::remove_lock(VESTING_ID, who);
			Self::deposit_event(Event::<T>::VestingCompleted { account: who.clone() });
		} else {
			let reasons = WithdrawReasons::except(T::UnvestedFundsAllowedWithdrawReasons::get());
			T::Currency::set_lock(VESTING_ID, who, total_locked_now, reasons);
			Self::deposit_event(Event::<T>::VestingUpdated {
				account: who.clone(),
				unvested: total_locked_now,
			});
		};
	}

	/// Write an accounts updated vesting schedules to storage.
	fn write_vesting(
		who: &T::AccountId,
		schedules: Vec<VestingInfoOf<T>>,
	) -> Result<(), DispatchError> {
		let schedules: BoundedVec<VestingInfoOf<T>, MaxVestingSchedulesGet<T>> =
			schedules.try_into().map_err(|_| Error::<T>::AtMaxVestingSchedules)?;

		if schedules.len() == 0 {
			Vesting::<T>::remove(&who);
		} else {
			Vesting::<T>::insert(who, schedules)
		}

		Ok(())
	}

	/// Unlock any vested funds of `who`.
	fn do_vest(who: T::AccountId) -> DispatchResult {
		let schedules = Vesting::<T>::get(&who).ok_or(Error::<T>::NotVesting)?;

		let (schedules, locked_now) =
			Self::exec_action(schedules.to_vec(), VestingAction::Passive)?;

		Self::write_vesting(&who, schedules)?;
		Self::write_lock(&who, locked_now);

		Ok(())
	}

	/// Execute a `VestingAction` against the given `schedules`. Returns the updated schedules
	/// and locked amount.
	fn exec_action(
		schedules: Vec<VestingInfoOf<T>>,
		action: VestingAction,
	) -> Result<(Vec<VestingInfoOf<T>>, BalanceOf<T>), DispatchError> {
		let (schedules, locked_now) = match action {
			VestingAction::Merge { index1: idx1, index2: idx2 } => {
				// The schedule index is based off of the schedule ordering prior to filtering out
				// any schedules that may be ending at this time.
				let schedule1 =
					schedules.get(idx1).cloned().ok_or(Error::<T>::ScheduleIndexOutOfBounds)?;
				let schedule2 =
					schedules.get(idx2).cloned().ok_or(Error::<T>::ScheduleIndexOutOfBounds)?;

				// The length of `schedules` decreases by 2 here since we filter out 2 schedules.
				// Thus we know below that we can push the new merged schedule without error
				// (assuming initial state was valid).
				let (mut schedules, mut locked_now) =
					Self::report_schedule_updates(schedules.to_vec(), action);

				let now = T::TimeProvider::now();
				if let Some(new_schedule) = Self::merge_vesting_info(now, schedule1, schedule2) {
					// Merging created a new schedule so we:
					// (we use `locked_at` in case this is a schedule that started in the past)
					let new_schedule_locked = new_schedule.locked_at::<T::MomentToBalance>(now);
					// 1) need to add it to the accounts vesting schedule collection,
					schedules.push(new_schedule);
					// and 2) update the locked amount to reflect the schedule we just added.
					locked_now = locked_now.saturating_add(new_schedule_locked);
				} // In the None case there was no new schedule to account for.

				(schedules, locked_now)
			},
			_ => Self::report_schedule_updates(schedules.to_vec(), action),
		};

		debug_assert!(
			locked_now > Zero::zero() && schedules.len() > 0 ||
				locked_now == Zero::zero() && schedules.len() == 0
		);

		Ok((schedules, locked_now))
	}
}

impl<T: Config> VestingSchedule<T::AccountId> for Pallet<T>
where
	BalanceOf<T>: MaybeSerializeDeserialize + Debug,
{
	type Currency = T::Currency;
	type Moment = MomentOf<T>;

	/// Get the amount that is currently being vested and cannot be transferred out of this account.
	fn vesting_balance(who: &T::AccountId) -> Option<BalanceOf<T>> {
		if let Some(v) = Vesting::<T>::get(who) {
			let now = T::TimeProvider::now();
			let total_locked_now = v.iter().fold(Zero::zero(), |total, schedule| {
				schedule.locked_at::<T::MomentToBalance>(now).saturating_add(total)
			});
			Some(T::Currency::free_balance(who).min(total_locked_now))
		} else {
			None
		}
	}

	/// Adds a vesting schedule to a given account.
	///
	/// If the account has `MaxVestingSchedules`, an Error is returned and nothing
	/// is updated.
	///
	/// On success, a linearly reducing amount of funds will be locked. In order to realise any
	/// reduction of the lock over time as it diminishes, the account owner must use `vest` or
	/// `vest_other`.
	///
	/// It is a no-op if the amount to be vested is zero.
	///
	/// NOTE: This doesn't alter the free balance of the account.
	fn add_vesting_schedule(
		who: &T::AccountId,
		locked: BalanceOf<T>,
		per_ms: BalanceOf<T>,
		start: MomentOf<T>,
	) -> DispatchResult {
		// Schedules created through this trait interface have no repurchaser.
		Self::do_add_vesting_schedule(who, VestingInfo::new(locked, per_ms, start))
	}

	/// Ensure we can call `add_vesting_schedule` without error. This should always
	/// be called prior to `add_vesting_schedule`.
	fn can_add_vesting_schedule(
		who: &T::AccountId,
		locked: BalanceOf<T>,
		per_ms: BalanceOf<T>,
		start: MomentOf<T>,
	) -> DispatchResult {
		// Check for `per_ms` or `locked` of 0.
		if !VestingInfoOf::<T>::new(locked, per_ms, start).is_valid() {
			return Err(Error::<T>::InvalidScheduleParams.into())
		}

		ensure!(
			(Vesting::<T>::decode_len(who).unwrap_or_default() as u32) < T::MAX_VESTING_SCHEDULES,
			Error::<T>::AtMaxVestingSchedules
		);

		Ok(())
	}

	/// Remove a vesting schedule for a given account.
	fn remove_vesting_schedule(who: &T::AccountId, schedule_index: u32) -> DispatchResult {
		let schedules = Vesting::<T>::get(who).ok_or(Error::<T>::NotVesting)?;
		let remove_action = VestingAction::Remove { index: schedule_index as usize };

		let (schedules, locked_now) = Self::exec_action(schedules.to_vec(), remove_action)?;

		Self::write_vesting(who, schedules)?;
		Self::write_lock(who, locked_now);
		Ok(())
	}
}

/// An implementation that allows the Vesting Pallet to handle a vested transfer
/// on behalf of another Pallet.
impl<T: Config> VestedTransfer<T::AccountId> for Pallet<T>
where
	BalanceOf<T>: MaybeSerializeDeserialize + Debug,
{
	type Currency = T::Currency;
	type Moment = MomentOf<T>;

	fn vested_transfer(
		source: &T::AccountId,
		target: &T::AccountId,
		locked: BalanceOf<T>,
		per_ms: BalanceOf<T>,
		start: MomentOf<T>,
	) -> DispatchResult {
		use frame_support::storage::{with_transaction, TransactionOutcome};
		let schedule = VestingInfo::new(locked, per_ms, start);
		with_transaction(|| -> TransactionOutcome<DispatchResult> {
			let result = Self::do_vested_transfer(source, target, schedule);

			match &result {
				Ok(()) => TransactionOutcome::Commit(result),
				_ => TransactionOutcome::Rollback(result),
			}
		})
	}
}
