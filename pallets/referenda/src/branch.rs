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

//! Helpers for managing the different weights in various algorithmic branches.

use super::Config;
use crate::{weights::WeightInfo, Pallet};
use frame_support::{traits::Get, weights::Weight};

/// Worst-case overhead of the `set_alarm` full-agenda retry loop (V12 audit #161704 /
/// #162455): every failed `T::Scheduler::schedule` attempt reads the candidate block's
/// `Scheduler::Agenda` before sliding forward one block. Nothing is written until an
/// attempt succeeds (and `Preimages::drop` of the inline nudge call is storage-free), and
/// the succeeding attempt's cost is already part of the benchmarked base weight, so only
/// the extra attempts are charged here.
///
/// This is added to every branch that can (re)schedule a referendum alarm or defer
/// `one_fewer_deciding` through `set_alarm`. Kept in this hand-written module so it
/// survives regeneration of the benchmarked `weights.rs`.
pub fn alarm_retry_weight<T: Config<I>, I: 'static>() -> Weight {
	T::DbWeight::get()
		.reads(1)
		.saturating_mul(Pallet::<T, I>::MAX_ALARM_SCHEDULE_RETRIES as u64)
}

/// Worst-case overhead of repairing a referendum displaced from a full `TrackQueue` (V12 audit
/// #161743): one `ReferendumInfoFor` read+write plus the re-armed evicted referendum's alarm
/// retry overhead. Charged on the branches that insert into a possibly-full queue (`Queued` /
/// `RequeuedInsertion`).
pub fn queue_eviction_repair_weight<T: Config<I>, I: 'static>() -> Weight {
	T::DbWeight::get()
		.reads_writes(1, 1)
		.saturating_add(alarm_retry_weight::<T, I>())
}

/// Branches within the `begin_deciding` function.
pub enum BeginDecidingBranch {
	Passing,
	Failing,
}

/// Branches within the `service_referendum` function.
pub enum ServiceBranch {
	Fail,
	NoDeposit,
	Preparing,
	Queued,
	NotQueued,
	RequeuedInsertion,
	RequeuedSlide,
	BeginDecidingPassing,
	BeginDecidingFailing,
	BeginConfirming,
	ContinueConfirming,
	EndConfirming,
	ContinueNotConfirming,
	Approved,
	Rejected,
	TimedOut,
}

impl From<BeginDecidingBranch> for ServiceBranch {
	fn from(x: BeginDecidingBranch) -> Self {
		use BeginDecidingBranch::*;
		use ServiceBranch::*;
		match x {
			Passing => BeginDecidingPassing,
			Failing => BeginDecidingFailing,
		}
	}
}

impl ServiceBranch {
	/// Return the weight of the `nudge` function when it takes the branch denoted by `self`.
	pub fn weight_of_nudge<T: Config<I>, I: 'static>(self) -> frame_support::weights::Weight {
		use ServiceBranch::*;
		let base = match self {
			NoDeposit => T::WeightInfo::nudge_referendum_no_deposit(),
			Preparing => T::WeightInfo::nudge_referendum_preparing(),
			Queued => T::WeightInfo::nudge_referendum_queued(),
			NotQueued => T::WeightInfo::nudge_referendum_not_queued(),
			RequeuedInsertion => T::WeightInfo::nudge_referendum_requeued_insertion(),
			RequeuedSlide => T::WeightInfo::nudge_referendum_requeued_slide(),
			BeginDecidingPassing => T::WeightInfo::nudge_referendum_begin_deciding_passing(),
			BeginDecidingFailing => T::WeightInfo::nudge_referendum_begin_deciding_failing(),
			BeginConfirming => T::WeightInfo::nudge_referendum_begin_confirming(),
			ContinueConfirming => T::WeightInfo::nudge_referendum_continue_confirming(),
			EndConfirming => T::WeightInfo::nudge_referendum_end_confirming(),
			ContinueNotConfirming => T::WeightInfo::nudge_referendum_continue_not_confirming(),
			Approved => T::WeightInfo::nudge_referendum_approved(),
			Rejected => T::WeightInfo::nudge_referendum_rejected(),
			TimedOut | Fail => T::WeightInfo::nudge_referendum_timed_out(),
		};
		match self {
			// Branches that (re)schedule the referendum's alarm, or defer
			// `one_fewer_deciding` via `set_alarm` (`Approved`/`Rejected`), bear the
			// worst-case retry overhead.
			NoDeposit |
			Preparing |
			NotQueued |
			BeginDecidingPassing |
			BeginDecidingFailing |
			BeginConfirming |
			ContinueConfirming |
			EndConfirming |
			ContinueNotConfirming |
			Approved |
			Rejected => base.saturating_add(alarm_retry_weight::<T, I>()),
			// Inserting into a possibly-full queue may evict and repair another referendum
			// (#161743): one read/write plus its alarm-retry overhead.
			Queued | RequeuedInsertion =>
				base.saturating_add(queue_eviction_repair_weight::<T, I>()),
			// These branches leave the referendum without an alarm (or only cancel one).
			RequeuedSlide | TimedOut | Fail => base,
		}
	}

	/// Return the maximum possible weight of the `nudge` function.
	pub fn max_weight_of_nudge<T: Config<I>, I: 'static>() -> frame_support::weights::Weight {
		Weight::zero()
			.max(T::WeightInfo::nudge_referendum_no_deposit())
			.max(T::WeightInfo::nudge_referendum_preparing())
			.max(T::WeightInfo::nudge_referendum_queued())
			.max(T::WeightInfo::nudge_referendum_not_queued())
			.max(T::WeightInfo::nudge_referendum_requeued_insertion())
			.max(T::WeightInfo::nudge_referendum_requeued_slide())
			.max(T::WeightInfo::nudge_referendum_begin_deciding_passing())
			.max(T::WeightInfo::nudge_referendum_begin_deciding_failing())
			.max(T::WeightInfo::nudge_referendum_begin_confirming())
			.max(T::WeightInfo::nudge_referendum_continue_confirming())
			.max(T::WeightInfo::nudge_referendum_end_confirming())
			.max(T::WeightInfo::nudge_referendum_continue_not_confirming())
			.max(T::WeightInfo::nudge_referendum_approved())
			.max(T::WeightInfo::nudge_referendum_rejected())
			.max(T::WeightInfo::nudge_referendum_timed_out())
			.saturating_add(alarm_retry_weight::<T, I>())
			.saturating_add(queue_eviction_repair_weight::<T, I>())
	}

	/// Return the weight of the `place_decision_deposit` function when it takes the branch denoted
	/// by `self`.
	pub fn weight_of_deposit<T: Config<I>, I: 'static>(
		self,
	) -> Option<frame_support::weights::Weight> {
		use ServiceBranch::*;
		let ref_time_weight = match self {
			// `Preparing`, `NotQueued` and `BeginDeciding*` (re)schedule the referendum's
			// alarm and so bear the worst-case `set_alarm` retry overhead; `Queued` leaves
			// the referendum alarm-less.
			Preparing => T::WeightInfo::place_decision_deposit_preparing()
				.saturating_add(alarm_retry_weight::<T, I>()),
			Queued => T::WeightInfo::place_decision_deposit_queued()
				.saturating_add(queue_eviction_repair_weight::<T, I>()),
			NotQueued => T::WeightInfo::place_decision_deposit_not_queued()
				.saturating_add(alarm_retry_weight::<T, I>()),
			BeginDecidingPassing => T::WeightInfo::place_decision_deposit_passing()
				.saturating_add(alarm_retry_weight::<T, I>()),
			BeginDecidingFailing => T::WeightInfo::place_decision_deposit_failing()
				.saturating_add(alarm_retry_weight::<T, I>()),
			BeginConfirming |
			ContinueConfirming |
			EndConfirming |
			ContinueNotConfirming |
			Approved |
			Rejected |
			RequeuedInsertion |
			RequeuedSlide |
			TimedOut |
			Fail |
			NoDeposit => return None,
		};

		Some(ref_time_weight)
	}

	/// Return the maximum possible weight of the `place_decision_deposit` function.
	pub fn max_weight_of_deposit<T: Config<I>, I: 'static>() -> frame_support::weights::Weight {
		Weight::zero()
			.max(T::WeightInfo::place_decision_deposit_preparing())
			.max(T::WeightInfo::place_decision_deposit_queued())
			.max(T::WeightInfo::place_decision_deposit_not_queued())
			.max(T::WeightInfo::place_decision_deposit_passing())
			.max(T::WeightInfo::place_decision_deposit_failing())
			.saturating_add(alarm_retry_weight::<T, I>())
			.saturating_add(queue_eviction_repair_weight::<T, I>())
	}
}

/// Branches that the `one_fewer_deciding` function may take.
pub enum OneFewerDecidingBranch {
	QueueEmpty,
	BeginDecidingPassing,
	BeginDecidingFailing,
}

impl From<BeginDecidingBranch> for OneFewerDecidingBranch {
	fn from(x: BeginDecidingBranch) -> Self {
		use BeginDecidingBranch::*;
		use OneFewerDecidingBranch::*;
		match x {
			Passing => BeginDecidingPassing,
			Failing => BeginDecidingFailing,
		}
	}
}

impl OneFewerDecidingBranch {
	/// Return the weight of the `one_fewer_deciding` function when it takes the branch denoted
	/// by `self`.
	pub fn weight<T: Config<I>, I: 'static>(self) -> frame_support::weights::Weight {
		use OneFewerDecidingBranch::*;
		match self {
			QueueEmpty => T::WeightInfo::one_fewer_deciding_queue_empty(),
			// `begin_deciding` schedules the promoted referendum's alarm, bearing the
			// worst-case `set_alarm` retry overhead.
			BeginDecidingPassing => T::WeightInfo::one_fewer_deciding_passing()
				.saturating_add(alarm_retry_weight::<T, I>()),
			BeginDecidingFailing => T::WeightInfo::one_fewer_deciding_failing()
				.saturating_add(alarm_retry_weight::<T, I>()),
		}
	}

	/// Return the maximum possible weight of the `one_fewer_deciding` function.
	pub fn max_weight<T: Config<I>, I: 'static>() -> frame_support::weights::Weight {
		Weight::zero()
			.max(T::WeightInfo::one_fewer_deciding_queue_empty())
			.max(T::WeightInfo::one_fewer_deciding_passing())
			.max(T::WeightInfo::one_fewer_deciding_failing())
			.saturating_add(alarm_retry_weight::<T, I>())
	}
}
