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

//! The crate's tests.

use super::*;
use crate::mock::{RefState::*, *};
use assert_matches::assert_matches;
use codec::Decode;
use frame_support::{
	assert_err, assert_noop, assert_ok, dispatch::RawOrigin, storage::with_storage_layer,
	traits::Contains,
};
use pallet_balances::Error as BalancesError;
use qp_scheduler::BlockNumberOrTimestamp;
use sp_runtime::DispatchError::BadOrigin;

#[test]
fn params_should_work() {
	ExtBuilder::default().build_and_execute(|| {
		assert_eq!(ReferendumCount::<Test>::get(), 0);
		assert_eq!(Balances::free_balance(42), 0);
		assert_eq!(pallet_balances::TotalIssuance::<Test>::get(), 600);
	});
}

#[test]
fn basic_happy_path_works() {
	ExtBuilder::default().build_and_execute(|| {
		// #1: submit
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(10),
		));
		assert_eq!(Balances::reserved_balance(&1), 2);
		assert_eq!(ReferendumCount::<Test>::get(), 1);
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));
		run_to(4);
		assert_eq!(DecidingCount::<Test>::get(0), 0);
		run_to(5);
		// #5: 4 blocks after submit - vote should now be deciding.
		assert_eq!(DecidingCount::<Test>::get(0), 1);
		run_to(6);
		// #6: Lots of ayes. Should now be confirming.
		set_tally(0, 100, 0);
		run_to(7);
		assert_eq!(confirming_until(0), 9);
		run_to(9);
		// #8: Should be confirmed & ended.
		assert_eq!(approved_since(0), 9);
		assert_ok!(Referenda::refund_decision_deposit(RuntimeOrigin::signed(2), 0));
		run_to(12);
		// #9: Should not yet be enacted.
		assert_eq!(Balances::free_balance(&42), 0);
		run_to(13);
		// #10: Proposal should be executed.
		assert_eq!(Balances::free_balance(&42), 1);
	});
}

#[test]
fn submission_bounded_by_max_active() {
	ExtBuilder::default().build_and_execute(|| {
		MaxActive::set(2);
		for _ in 0..2 {
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(1),
				Box::new(RawOrigin::Root.into()),
				set_balance_proposal_bounded(1),
				DispatchTime::At(10),
			));
		}
		// The global bound is reached: further submissions are rejected, even though
		// neither referendum occupies a deciding slot or `TrackQueue` entry (no decision
		// deposit was placed).
		assert_noop!(
			Referenda::submit(
				RuntimeOrigin::signed(1),
				Box::new(RawOrigin::Root.into()),
				set_balance_proposal_bounded(1),
				DispatchTime::At(10),
			),
			Error::<Test>::TooManyActive
		);
		// Concluding an ongoing referendum frees a slot again.
		assert_ok!(Referenda::cancel(RuntimeOrigin::signed(4), 0));
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(10),
		));
	});
}

#[test]
fn timed_out_referendum_frees_active_slot() {
	ExtBuilder::default().build_and_execute(|| {
		MaxActive::set(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(10),
		));
		assert_noop!(
			Referenda::submit(
				RuntimeOrigin::signed(1),
				Box::new(RawOrigin::Root.into()),
				set_balance_proposal_bounded(1),
				DispatchTime::At(10),
			),
			Error::<Test>::TooManyActive
		);
		// Without a decision deposit the referendum times out after `UndecidingTimeout`,
		// which must free its active slot.
		run_to(22);
		assert_matches!(ReferendumInfoFor::<Test>::get(0), Some(ReferendumInfo::TimedOut(..)));
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(30),
		));
	});
}

/// If the preferred enactment block's agenda is already full of mid-priority
/// tasks (which the LOWEST_PRIORITY reservation does not hold back), enactment
/// must slide forward to a free block instead of dropping the approved call.
///
/// Uses Root track 0 (`min_enactment_period = 4`) so confirmation alarms do not
/// land on the enactment block. Mirrors `basic_happy_path_works`: approved at
/// block 9, preferred enactment at 13 (`max(At(10), 9+4)`).
#[test]
fn full_enactment_block_agenda_retries_on_next_block() {
	ExtBuilder::default().build_and_execute(|| {
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(10),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));

		// Preferred enactment block once approved at 9: max(10, 9+4) = 13.
		let preferred_enactment = 13u64;
		let max = <<Test as pallet_scheduler::Config>::MaxScheduledPerBlock as frame_support::traits::Get<
			u32,
		>>::get();
		let filler = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
		// Priority 100 ≠ LOWEST_PRIORITY (255), so these fillers are not subject to
		// the reservation cap and can occupy the whole agenda including reserved slots.
		for _ in 0..max {
			assert_ok!(Scheduler::schedule(
				RuntimeOrigin::root(),
				preferred_enactment,
				100,
				Box::new(filler.clone()),
			));
		}

		run_to(6);
		set_tally(0, 100, 0);
		run_to(9);
		assert_eq!(approved_since(0), 9);

		// Still not enacted at the preferred (full) block.
		run_to(preferred_enactment);
		assert_eq!(Balances::free_balance(&42), 0);
		// Retry lands on preferred+1 and executes there.
		run_to(preferred_enactment + 1);
		assert_eq!(Balances::free_balance(&42), 1);
	});
}

/// V12 audit #161704+#162455: if the agenda at the alarm's target block is full,
/// `set_alarm` must slide forward to a later block instead of dropping the alarm, and
/// `status.alarm` must reflect the block that was actually scheduled.
///
/// A referendum submitted at block 1 gets its (no-deposit) timeout alarm at
/// `1 + UndecidingTimeout (20)` = block 21. With block 21's agenda completely full, the
/// alarm must land at block 22 and the referendum must time out there, not at 21.
#[test]
fn full_alarm_block_agenda_retries_on_next_block() {
	ExtBuilder::default().build_and_execute(|| {
		let target = 21u64;
		let max = <<Test as pallet_scheduler::Config>::MaxScheduledPerBlock as frame_support::traits::Get<
			u32,
		>>::get();
		let filler = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
		// Priority 100 != LOWEST_PRIORITY (255), so these fillers are not subject to
		// the reservation cap and can occupy the whole agenda including reserved slots.
		for _ in 0..max {
			assert_ok!(Scheduler::schedule(
				RuntimeOrigin::root(),
				target,
				100,
				Box::new(filler.clone()),
			));
		}

		// Submit: the alarm cannot go into the full block 21 and must land at 22.
		assert_ok!(propose_set_balance(1, 1, 1));
		let status = Referenda::ensure_ongoing(0).unwrap();
		assert_eq!(status.alarm.map(|(when, _)| when), Some(22));

		// Not timed out at the (full) block 21, since no alarm fired there ...
		run_to(21);
		assert_matches!(ReferendumInfoFor::<Test>::get(0), Some(ReferendumInfo::Ongoing(..)));
		// ... but the alarm fires at block 22 and the referendum times out there.
		run_to(22);
		assert_eq!(timed_out_since(0), 22);
	});
}

/// V12 audit #161743/#161418: a referendum evicted from a full `TrackQueue` would keep
/// `in_queue = true` in storage while being absent from the queue (ghost-queued) with no
/// alarm, stranding it forever. The eviction must be repaired *eagerly*: the moment the
/// evicting insertion happens, the displaced referendum's `in_queue` flag is cleared and its
/// undeciding-timeout alarm restored so it stays serviceable without any further nudge.
///
/// Track 0 has `max_deciding = 1` and `MaxQueued = 3`. Once the queue is full, ref4 with more
/// ayes than the queue's tail squeezes in via `force_insert_keep_right`, evicting the
/// lowest-ayes entry (ref1).
#[test]
fn evicted_from_full_queue_is_repaired_immediately() {
	ExtBuilder::default().build_and_execute(|| {
		// Block 1: ref0 occupies the single deciding slot of track 0 from block 5 on.
		assert_ok!(propose_set_balance(1, 0, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(1), 0));
		run_to(5);
		assert_eq!(deciding_and_failing_since(0), 5);

		// Refs 1-4 (accounts 2-5), with ayes pre-set so the queue fills as
		// [(1, 1), (2, 2), (3, 3)] and ref4 (2 ayes) then evicts ref1.
		for (who, ayes) in [(2u64, 1u32), (3, 2), (4, 3), (5, 2)] {
			let index = who as u32 - 1;
			assert_ok!(propose_set_balance(who, who, 0));
			assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(who), index));
			set_tally(index, ayes, 0);
		}
		run_to(9);
		// ref0 is rejected; refs 1, 2, 3 entered the queue and ref4 evicted ref1.
		assert_eq!(rejected_since(0), 9);
		assert_eq!(
			Vec::<_>::from(TrackQueue::<Test>::get(0)),
			vec![(4u32, 2u32), (2u32, 2u32), (3u32, 3u32)]
		);

		// #161418: ref1 was evicted, and repaired eagerly - no ghost state, no missing alarm.
		// Its `in_queue` flag is already cleared and its undeciding-timeout alarm (submitted at
		// block 5 + UndecidingTimeout 20 = block 25) restored, with no external nudge.
		let s1 = Referenda::ensure_ongoing(1).unwrap();
		assert!(!s1.in_queue);
		assert_eq!(s1.alarm.map(|(when, _)| when), Some(25));

		// When that alarm fires, the referendum makes progress instead of stranding.
		run_to(25);
		let stranded = matches!(
			ReferendumInfoFor::<Test>::get(1),
			Some(ReferendumInfo::Ongoing(ReferendumStatus {
				in_queue: false,
				deciding: None,
				alarm: None,
				..
			}))
		);
		assert!(!stranded, "evicted referendum must make progress after its timeout alarm");
	});
}

/// V12 audit #162455: releasing a deciding slot must not depend on the deferred
/// `one_fewer_deciding` alarm being schedulable. When a deciding referendum ends while
/// the agendas of the alarm's target block and of all 16 retry blocks are full, the
/// track's deciding slot must still be released inline instead of leaving
/// `DecidingCount` stuck at capacity forever (with `max_deciding = 1` this would
/// deadlock the whole governance lane).
///
/// The referendum is ended through the rejection path (its own decision-period alarm at
/// block 9), because that alarm has already been consumed by the scheduler when it
/// fires and so - unlike `cancel`/`kill`, which free the referendum's alarm slot first -
/// leaves no hole in any agenda for the `one_fewer_deciding` alarm to slip into.
#[test]
fn unschedulable_one_fewer_deciding_releases_deciding_slot_inline() {
	ExtBuilder::default().build_and_execute(|| {
		// ref0 occupies track 0's single deciding slot from block 5 on; with a failing
		// tally its decision period (4 blocks) ends with rejection at block 9.
		assert_ok!(propose_set_balance(1, 1, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(1), 0));
		run_to(5);
		assert_eq!(deciding_and_failing_since(0), 5);
		assert_eq!(DecidingCount::<Test>::get(0), 1);

		// Fill the agendas of the deferred `one_fewer_deciding` alarm's target block
		// (rejection block 9 + 1 = 10) and of all 16 retry blocks after it.
		let max = <<Test as pallet_scheduler::Config>::MaxScheduledPerBlock as frame_support::traits::Get<
			u32,
		>>::get();
		let filler = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
		for when in 10..=26 {
			for _ in 0..max {
				assert_ok!(Scheduler::schedule(
					RuntimeOrigin::root(),
					when,
					100,
					Box::new(filler.clone()),
				));
			}
		}

		// The rejection at block 9 cannot schedule `one_fewer_deciding` anywhere, so the
		// slot must be released inline, immediately. (This also proves the agendas really
		// were full - had the alarm been scheduled, the count could only drop once it
		// fired at block 10 or later.)
		run_to(9);
		assert_eq!(rejected_since(0), 9);
		assert_eq!(DecidingCount::<Test>::get(0), 0);

		// The freed slot is genuinely usable: once past the filled agendas, a fresh
		// referendum begins deciding after its prepare period instead of queueing behind
		// a phantom occupant.
		run_to(27);
		assert_ok!(propose_set_balance(2, 2, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 1));
		run_to(31);
		assert_eq!(deciding_since(1), 31);
	});
}

/// V12 audit #161327: if the undeciding-timeout alarm cannot be scheduled (its target block and
/// all retry blocks are full), `submit` must fail instead of recording a referendum that is never
/// serviced (no alarm, not queued, not deciding) and whose deposit is locked forever. Being
/// transactional, the `Err` rolls back the reserve and the `ReferendumCount` increment.
#[test]
fn submit_fails_when_timeout_alarm_unschedulable() {
	ExtBuilder::default().build_and_execute(|| {
		// Undeciding-timeout alarm target once submitted at block 1: 1 + 20 = 21. Fill it and all
		// 16 retry blocks so `set_alarm` returns `None`.
		let max = <<Test as pallet_scheduler::Config>::MaxScheduledPerBlock as frame_support::traits::Get<
			u32,
		>>::get();
		let filler = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
		for when in 21..=37 {
			for _ in 0..max {
				assert_ok!(Scheduler::schedule(
					RuntimeOrigin::root(),
					when,
					100,
					Box::new(filler.clone()),
				));
			}
		}
		assert_err!(
			with_storage_layer(|| propose_set_balance(1, 1, 0)),
			Error::<Test>::AlarmSchedulingFailed
		);
		// Nothing recorded, nothing reserved.
		assert_eq!(ReferendumCount::<Test>::get(), 0);
		assert_eq!(Balances::reserved_balance(1), 0);
	});
}

/// V12 audit #161329: when a full `TrackQueue` rejects a low-ayes referendum (it sorts at index 0
/// and is not inserted), the referendum must NOT be marked `in_queue`; it must keep its
/// undeciding-timeout alarm so it can time out or retry admission rather than strand with no queue
/// entry and no alarm.
#[test]
fn queue_admission_failure_keeps_timeout_alarm() {
	ExtBuilder::default().build_and_execute(|| {
		// ref0 holds track 0's single deciding slot from block 5 on.
		assert_ok!(propose_set_balance(1, 0, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(1), 0));
		run_to(5);
		assert_eq!(deciding_and_failing_since(0), 5);

		// Fill the queue with [(1,1),(2,2),(3,3)]; ref4 (0 ayes) sorts below the queue minimum, so
		// admission into the full queue fails.
		for (who, ayes) in [(2u64, 1u32), (3, 2), (4, 3), (5, 0)] {
			let index = who as u32 - 1;
			assert_ok!(propose_set_balance(who, who, 0));
			assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(who), index));
			set_tally(index, ayes, 0);
		}
		run_to(9);
		assert_eq!(
			Vec::<_>::from(TrackQueue::<Test>::get(0)),
			vec![(1u32, 1u32), (2u32, 2u32), (3u32, 3u32)]
		);
		// ref4 was not admitted: not `in_queue`, but keeps a wake-up (submitted 5 + 20 = 25).
		let s4 = Referenda::ensure_ongoing(4).unwrap();
		assert!(!s4.in_queue);
		assert_eq!(s4.alarm.map(|(when, _)| when), Some(25));
	});
}

/// V12 audit #160560: an approved referendum whose enactment call cannot be scheduled (its
/// preferred block and all retry blocks are full) must NOT be committed as `Approved` (which
/// discards the call/origin). It stays `Ongoing`/confirming and retries the enactment on a later
/// alarm, so the enactment is never lost.
#[test]
fn approved_referendum_retries_enactment_when_agendas_full() {
	ExtBuilder::default().build_and_execute(|| {
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(10),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));

		// Preferred enactment block once approved at 9: max(10, 9+4) = 13. Fill it and all 16
		// retry blocks so enactment scheduling fully fails at the approval attempt.
		let max = <<Test as pallet_scheduler::Config>::MaxScheduledPerBlock as frame_support::traits::Get<
			u32,
		>>::get();
		let filler = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
		for when in 13..=29 {
			for _ in 0..max {
				assert_ok!(Scheduler::schedule(
					RuntimeOrigin::root(),
					when,
					100,
					Box::new(filler.clone()),
				));
			}
		}

		run_to(6);
		set_tally(0, 100, 0);
		run_to(9);
		// Enactment could not be scheduled, so the referendum is NOT approved - it stays ongoing.
		assert_matches!(ReferendumInfoFor::<Test>::get(0), Some(ReferendumInfo::Ongoing(..)));
		assert_eq!(confirming_until(0), 9);

		// Once past the filled agendas the enactment schedules and the referendum is approved.
		run_to(30);
		assert_eq!(approved_since(0), 30);
		// The enactment then executes (earliest 30 + min_enactment 4 = 34).
		run_to(34);
		assert_eq!(Balances::free_balance(42), 1);
	});
}

/// V12 audit #161394: cancelling a queued referendum must drop its `TrackQueue` entry so terminal
/// referenda do not occupy the bounded queue's capacity.
#[test]
fn cancel_removes_queued_referendum_from_track_queue() {
	ExtBuilder::default().build_and_execute(|| {
		// ref0 takes track 0's single deciding slot; ref1 queues behind it.
		assert_ok!(propose_set_balance(1, 0, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(1), 0));
		run_to(3);
		assert_ok!(propose_set_balance(2, 2, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 1));
		run_to(7);
		assert!(Referenda::ensure_ongoing(1).unwrap().in_queue);
		assert!(Vec::<_>::from(TrackQueue::<Test>::get(0)).iter().any(|(i, _)| *i == 1));

		assert_ok!(Referenda::cancel(RuntimeOrigin::signed(4), 1));
		assert!(!Vec::<_>::from(TrackQueue::<Test>::get(0)).iter().any(|(i, _)| *i == 1));
	});
}

/// V12 audit #161394: killing a queued referendum must likewise drop its `TrackQueue` entry.
#[test]
fn kill_removes_queued_referendum_from_track_queue() {
	ExtBuilder::default().build_and_execute(|| {
		assert_ok!(propose_set_balance(1, 0, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(1), 0));
		run_to(3);
		assert_ok!(propose_set_balance(2, 2, 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 1));
		run_to(7);
		assert!(Referenda::ensure_ongoing(1).unwrap().in_queue);
		assert!(Vec::<_>::from(TrackQueue::<Test>::get(0)).iter().any(|(i, _)| *i == 1));

		assert_ok!(Referenda::kill(RuntimeOrigin::root(), 1));
		assert!(!Vec::<_>::from(TrackQueue::<Test>::get(0)).iter().any(|(i, _)| *i == 1));
	});
}

#[test]
fn insta_confirm_then_kill_works() {
	ExtBuilder::default().build_and_execute(|| {
		let r = Confirming { immediate: true }.create();
		run_to(6);
		assert_ok!(Referenda::kill(RuntimeOrigin::root(), r));
		assert_eq!(killed_since(r), 6);
	});
}

#[test]
fn confirm_then_reconfirm_with_elapsed_trigger_works() {
	ExtBuilder::default().build_and_execute(|| {
		let r = Confirming { immediate: false }.create();
		assert_eq!(confirming_until(r), 8);
		run_to(7);
		set_tally(r, 100, 99);
		run_to(8);
		assert_eq!(deciding_and_failing_since(r), 5);
		run_to(11);
		assert_eq!(approved_since(r), 11);
	});
}

#[test]
fn instaconfirm_then_reconfirm_with_elapsed_trigger_works() {
	ExtBuilder::default().build_and_execute(|| {
		let r = Confirming { immediate: true }.create();
		run_to(6);
		assert_eq!(confirming_until(r), 7);
		set_tally(r, 100, 99);
		run_to(7);
		assert_eq!(deciding_and_failing_since(r), 5);
		run_to(11);
		assert_eq!(approved_since(r), 11);
	});
}

#[test]
fn instaconfirm_then_reconfirm_with_voting_trigger_works() {
	ExtBuilder::default().build_and_execute(|| {
		let r = Confirming { immediate: true }.create();
		run_to(6);
		assert_eq!(confirming_until(r), 7);
		set_tally(r, 100, 99);
		run_to(7);
		assert_eq!(deciding_and_failing_since(r), 5);
		run_to(8);
		set_tally(r, 100, 0);
		run_to(9);
		assert_eq!(confirming_until(r), 11);
		run_to(11);
		assert_eq!(approved_since(r), 11);
	});
}

#[test]
fn voting_should_extend_for_late_confirmation() {
	ExtBuilder::default().build_and_execute(|| {
		let r = Passing.create();
		run_to(10);
		assert_eq!(confirming_until(r), 11);
		run_to(11);
		assert_eq!(approved_since(r), 11);
	});
}

#[test]
fn should_instafail_during_extension_confirmation() {
	ExtBuilder::default().build_and_execute(|| {
		let r = Passing.create();
		run_to(10);
		assert_eq!(confirming_until(r), 11);
		// Should insta-fail since it's now past the normal voting time.
		set_tally(r, 100, 101);
		run_to(11);
		assert_eq!(rejected_since(r), 11);
	});
}

#[test]
fn confirming_then_fail_works() {
	ExtBuilder::default().build_and_execute(|| {
		let r = Failing.create();
		// Normally ends at 5 + 4 (voting period) = 9.
		assert_eq!(deciding_and_failing_since(r), 5);
		set_tally(r, 100, 0);
		run_to(6);
		assert_eq!(confirming_until(r), 8);
		set_tally(r, 100, 101);
		run_to(9);
		assert_eq!(rejected_since(r), 9);
	});
}

#[test]
fn queueing_works() {
	ExtBuilder::default().build_and_execute(|| {
		// Submit a proposal into a track with a queue len of 1.
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(5),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(0),
			DispatchTime::After(0),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(5), 0));

		run_to(2);

		// Submit 3 more proposals into the same queue.
		for i in 1..=4 {
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(i),
				Box::new(RawOrigin::Root.into()),
				set_balance_proposal_bounded(i),
				DispatchTime::After(0),
			));
		}
		for i in [1, 2, 4] {
			assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(i), i as u32));
			// TODO: decision deposit after some initial votes with a non-highest voted coming
			// first.
		}
		assert_eq!(ReferendumCount::<Test>::get(), 5);

		run_to(5);
		// One should be being decided.
		assert_eq!(DecidingCount::<Test>::get(0), 1);
		assert_eq!(deciding_and_failing_since(0), 5);
		for i in 1..=4 {
			assert_eq!(waiting_since(i), 2);
		}

		// Vote to set order.
		set_tally(1, 1, 10);
		set_tally(2, 2, 20);
		set_tally(3, 3, 30);
		set_tally(4, 100, 0);
		run_to(6);
		println!("{:?}", Vec::<_>::from(TrackQueue::<Test>::get(0)));

		// Cancel the first.
		assert_ok!(Referenda::cancel(RuntimeOrigin::signed(4), 0));
		assert_eq!(cancelled_since(0), 6);

		// The other with the most approvals (#4) should be being decided.
		run_to(7);
		assert_eq!(DecidingCount::<Test>::get(0), 1);
		assert_eq!(deciding_since(4), 7);
		assert_eq!(confirming_until(4), 9);

		// Vote on the remaining two to change order.
		println!("Set tally #1");
		set_tally(1, 30, 31);
		println!("{:?}", Vec::<_>::from(TrackQueue::<Test>::get(0)));
		println!("Set tally #2");
		set_tally(2, 20, 20);
		println!("{:?}", Vec::<_>::from(TrackQueue::<Test>::get(0)));

		// Let confirmation period end.
		run_to(9);

		// #4 should have been confirmed.
		assert_eq!(approved_since(4), 9);

		// On to the next block to select the new referendum
		run_to(10);
		// #1 (the one with the most approvals) should now be being decided.
		assert_eq!(deciding_since(1), 10);

		// Let it end unsuccessfully.
		run_to(14);
		assert_eq!(rejected_since(1), 14);

		// Service queue.
		run_to(15);
		// #2 should now be being decided. It will (barely) pass.
		assert_eq!(deciding_and_failing_since(2), 15);

		// #2 moves into confirming at the last moment with a 50% approval.
		run_to(19);
		assert_eq!(confirming_until(2), 21);

		// #2 gets approved.
		run_to(21);
		assert_eq!(approved_since(2), 21);

		// The final one has since timed out.
		run_to(22);
		assert_eq!(timed_out_since(3), 22);
	});
}

#[test]
fn alarm_interval_works() {
	ExtBuilder::default().build_and_execute(|| {
		let call =
			<Test as Config>::Preimages::bound(CallOf::<Test, ()>::from(Call::nudge_referendum {
				index: 0,
			}))
			.unwrap();
		for n in 0..10 {
			let interval = n * n;
			let now = 100 * (interval + 1);
			System::set_block_number(now);
			AlarmInterval::set(interval);
			let when = now + 1;
			let (actual, _) = Referenda::set_alarm(call.clone(), when).unwrap();
			assert!(actual >= when);
			assert!(actual - interval <= when);
		}
	});
}

#[test]
fn decision_time_is_correct() {
	ExtBuilder::default().build_and_execute(|| {
		let decision_time = |since: u64| {
			let track = TestTracksInfo::tracks().next().unwrap();
			Pallet::<Test>::decision_time(
				&DecidingStatus { since: since.into(), confirming: None },
				&Tally { ayes: 100, nays: 5 },
				track.id,
				&track.info,
			)
		};

		for i in 0u64..=100 {
			assert!(decision_time(i) > i, "The decision time should be delayed by the curve");
		}
	});
}

#[test]
fn auto_timeout_should_happen_with_nothing_but_submit() {
	ExtBuilder::default().build_and_execute(|| {
		// #1: submit
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(20),
		));
		run_to(20);
		assert_matches!(ReferendumInfoFor::<Test>::get(0), Some(ReferendumInfo::Ongoing(..)));
		run_to(21);
		// #11: Timed out - ended.
		assert_matches!(
			ReferendumInfoFor::<Test>::get(0),
			Some(ReferendumInfo::TimedOut(21, _, None))
		);
	});
}

#[test]
fn tracks_are_distinguished() {
	ExtBuilder::default().build_and_execute(|| {
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(10),
		));
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(2),
			Box::new(RawOrigin::None.into()),
			set_balance_proposal_bounded(2),
			DispatchTime::At(20),
		));

		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(3), 0));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(4), 1));

		let mut i = ReferendumInfoFor::<Test>::iter().collect::<Vec<_>>();
		i.sort_by_key(|x| x.0);
		assert_eq!(
			i,
			vec![
				(
					0,
					ReferendumInfo::Ongoing(ReferendumStatus {
						track: 0,
						origin: OriginCaller::system(RawOrigin::Root),
						proposal: set_balance_proposal_bounded(1),
						enactment: DispatchTime::At(10),
						submitted: 1,
						submission_deposit: Deposit { who: 1, amount: 2 },
						decision_deposit: Some(Deposit { who: 3, amount: 10 }),
						deciding: None,
						tally: Tally { ayes: 0, nays: 0 },
						in_queue: false,
						alarm: Some((5, (BlockNumberOrTimestamp::BlockNumber(5), 0))),
					})
				),
				(
					1,
					ReferendumInfo::Ongoing(ReferendumStatus {
						track: 1,
						origin: OriginCaller::system(RawOrigin::None),
						proposal: set_balance_proposal_bounded(2),
						enactment: DispatchTime::At(20),
						submitted: 1,
						submission_deposit: Deposit { who: 2, amount: 2 },
						decision_deposit: Some(Deposit { who: 4, amount: 1 }),
						deciding: None,
						tally: Tally { ayes: 0, nays: 0 },
						in_queue: false,
						alarm: Some((3, (BlockNumberOrTimestamp::BlockNumber(3), 0))),
					})
				),
			]
		);
	});
}

#[test]
fn submit_errors_work() {
	ExtBuilder::default().build_and_execute(|| {
		let h = set_balance_proposal_bounded(1);
		// No track for Signed origins.
		assert_noop!(
			Referenda::submit(
				RuntimeOrigin::signed(1),
				Box::new(RawOrigin::Signed(2).into()),
				h.clone(),
				DispatchTime::At(10),
			),
			Error::<Test>::NoTrack
		);

		// No funds for deposit
		assert_noop!(
			Referenda::submit(
				RuntimeOrigin::signed(10),
				Box::new(RawOrigin::Root.into()),
				h,
				DispatchTime::At(10),
			),
			BalancesError::<Test>::InsufficientBalance
		);
	});
}

#[test]
fn decision_deposit_errors_work() {
	ExtBuilder::default().build_and_execute(|| {
		let e = Error::<Test>::NotOngoing;
		assert_noop!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0), e);

		let h = set_balance_proposal_bounded(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h,
			DispatchTime::At(10),
		));
		let e = BalancesError::<Test>::InsufficientBalance;
		assert_noop!(Referenda::place_decision_deposit(RuntimeOrigin::signed(10), 0), e);

		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));
		let e = Error::<Test>::HasDeposit;
		assert_noop!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0), e);
	});
}

#[test]
fn refund_deposit_works() {
	ExtBuilder::default().build_and_execute(|| {
		let e = Error::<Test>::BadReferendum;
		assert_noop!(Referenda::refund_decision_deposit(RuntimeOrigin::signed(1), 0), e);

		let h = set_balance_proposal_bounded(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h,
			DispatchTime::At(10),
		));
		let e = Error::<Test>::NoDeposit;
		assert_noop!(Referenda::refund_decision_deposit(RuntimeOrigin::signed(2), 0), e);

		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));
		let e = Error::<Test>::Unfinished;
		assert_noop!(Referenda::refund_decision_deposit(RuntimeOrigin::signed(3), 0), e);

		run_to(11);
		assert_ok!(Referenda::refund_decision_deposit(RuntimeOrigin::signed(3), 0));
	});
}

#[test]
fn refund_submission_deposit_works() {
	ExtBuilder::default().build_and_execute(|| {
		// refund of non existing referendum fails.
		let e = Error::<Test>::BadReferendum;
		assert_noop!(Referenda::refund_submission_deposit(RuntimeOrigin::signed(1), 0), e);
		// create a referendum.
		let h = set_balance_proposal_bounded(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h.clone(),
			DispatchTime::At(10),
		));
		// refund of an ongoing referendum fails.
		let e = Error::<Test>::BadStatus;
		assert_noop!(Referenda::refund_submission_deposit(RuntimeOrigin::signed(3), 0), e);
		// cancel referendum.
		assert_ok!(Referenda::cancel(RuntimeOrigin::signed(4), 0));
		// refund of canceled referendum works.
		assert_ok!(Referenda::refund_submission_deposit(RuntimeOrigin::signed(3), 0));
		// fails if already refunded.
		let e = Error::<Test>::NoDeposit;
		assert_noop!(Referenda::refund_submission_deposit(RuntimeOrigin::signed(2), 0), e);
		// create second referendum.
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h,
			DispatchTime::At(10),
		));
		// refund of a killed referendum fails.
		assert_ok!(Referenda::kill(RuntimeOrigin::root(), 1));
		let e = Error::<Test>::NoDeposit;
		assert_noop!(Referenda::refund_submission_deposit(RuntimeOrigin::signed(2), 0), e);
	});
}

#[test]
fn cancel_works() {
	ExtBuilder::default().build_and_execute(|| {
		let h = set_balance_proposal_bounded(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h,
			DispatchTime::At(10),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));

		run_to(8);
		assert_ok!(Referenda::cancel(RuntimeOrigin::signed(4), 0));
		assert_ok!(Referenda::refund_decision_deposit(RuntimeOrigin::signed(3), 0));
		assert_eq!(cancelled_since(0), 8);
	});
}

#[test]
fn cancel_errors_works() {
	ExtBuilder::default().build_and_execute(|| {
		let h = set_balance_proposal_bounded(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h,
			DispatchTime::At(10),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));
		assert_noop!(Referenda::cancel(RuntimeOrigin::signed(1), 0), BadOrigin);

		run_to(11);
		assert_noop!(Referenda::cancel(RuntimeOrigin::signed(4), 0), Error::<Test>::NotOngoing);
	});
}

#[test]
fn kill_works() {
	ExtBuilder::default().build_and_execute(|| {
		let h = set_balance_proposal_bounded(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h,
			DispatchTime::At(10),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));

		run_to(8);
		assert_ok!(Referenda::kill(RuntimeOrigin::root(), 0));
		let e = Error::<Test>::NoDeposit;
		assert_noop!(Referenda::refund_decision_deposit(RuntimeOrigin::signed(3), 0), e);
		assert_eq!(killed_since(0), 8);
	});
}

#[test]
fn kill_errors_works() {
	ExtBuilder::default().build_and_execute(|| {
		let h = set_balance_proposal_bounded(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			h,
			DispatchTime::At(10),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));
		assert_noop!(Referenda::kill(RuntimeOrigin::signed(4), 0), BadOrigin);

		run_to(11);
		assert_noop!(Referenda::kill(RuntimeOrigin::root(), 0), Error::<Test>::NotOngoing);
	});
}

#[test]
fn set_balance_proposal_is_correctly_filtered_out() {
	for i in 0..10 {
		let call = crate::mock::RuntimeCall::decode(&mut &set_balance_proposal(i)[..]).unwrap();
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
	}
}

#[test]
fn curve_handles_all_inputs() {
	let test_curve = Curve::LinearDecreasing {
		length: Perbill::one(),
		floor: Perbill::zero(),
		ceil: Perbill::from_percent(100),
	};

	let delay = test_curve.delay(Perbill::zero());
	assert_eq!(delay, Perbill::one());

	let threshold = test_curve.threshold(Perbill::one());
	assert_eq!(threshold, Perbill::zero());
}

#[test]
fn set_metadata_works() {
	ExtBuilder::default().build_and_execute(|| {
		// invalid preimage hash.
		let invalid_hash: <Test as frame_system::Config>::Hash = [1u8; 32].into();
		// fails to set metadata for a finished referendum.
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(1),
		));
		let index = ReferendumCount::<Test>::get() - 1;
		assert_ok!(Referenda::kill(RuntimeOrigin::root(), index));
		assert_noop!(
			Referenda::set_metadata(RuntimeOrigin::signed(1), index, Some(invalid_hash)),
			Error::<Test>::NotOngoing,
		);
		// no permission to set metadata.
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(1),
		));
		let index = ReferendumCount::<Test>::get() - 1;
		assert_noop!(
			Referenda::set_metadata(RuntimeOrigin::signed(2), index, Some(invalid_hash)),
			Error::<Test>::NoPermission,
		);
		// preimage does not exist.
		let index = ReferendumCount::<Test>::get() - 1;
		assert_noop!(
			Referenda::set_metadata(RuntimeOrigin::signed(1), index, Some(invalid_hash)),
			Error::<Test>::PreimageNotExist,
		);
		// metadata set.
		let index = ReferendumCount::<Test>::get() - 1;
		let hash = note_preimage(1);
		assert_ok!(Referenda::set_metadata(RuntimeOrigin::signed(1), index, Some(hash)));
		System::assert_last_event(RuntimeEvent::Referenda(crate::Event::MetadataSet {
			index,
			hash,
		}));
	});
}

#[test]
fn clear_metadata_works() {
	ExtBuilder::default().build_and_execute(|| {
		let hash = note_preimage(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			set_balance_proposal_bounded(1),
			DispatchTime::At(1),
		));
		let index = ReferendumCount::<Test>::get() - 1;
		assert_ok!(Referenda::set_metadata(RuntimeOrigin::signed(1), index, Some(hash)));
		assert_noop!(
			Referenda::set_metadata(RuntimeOrigin::signed(2), index, None),
			Error::<Test>::NoPermission,
		);
		assert_ok!(Referenda::set_metadata(RuntimeOrigin::signed(1), index, None),);
		System::assert_last_event(RuntimeEvent::Referenda(crate::Event::MetadataCleared {
			index,
			hash,
		}));
	});
}

#[test]
fn detects_incorrect_len() {
	ExtBuilder::default().build_and_execute(|| {
		let hash = note_preimage(1);
		assert_noop!(
			Referenda::submit(
				RuntimeOrigin::signed(1),
				Box::new(RawOrigin::Root.into()),
				frame_support::traits::Bounded::Lookup { hash, len: 3 },
				DispatchTime::At(1),
			),
			Error::<Test>::PreimageStoredWithDifferentLength
		);
	});
}

#[test]
fn submit_requires_and_requests_lookup_preimage() {
	ExtBuilder::default().build_and_execute(|| {
		// A lookup proposal whose preimage was never noted must be rejected: otherwise the
		// scheduler drops the enactment as terminal (`CallUnavailable`) after approval.
		let missing =
			<<Test as frame_system::Config>::Hashing as sp_runtime::traits::Hash>::hash(
				b"no such preimage",
			);
		assert_noop!(
			Referenda::submit(
				RuntimeOrigin::signed(1),
				Box::new(RawOrigin::Root.into()),
				frame_support::traits::Bounded::Lookup { hash: missing, len: 1 },
				DispatchTime::At(10),
			),
			Error::<Test>::PreimageNotExist
		);

		// A noted preimage is requested (pinned) by submission, so unnoting cannot delete
		// the bytes while the referendum is ongoing.
		let hash = note_preimage(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			frame_support::traits::Bounded::Lookup { hash, len: 1 },
			DispatchTime::At(10),
		));
		assert!(Preimage::is_requested(&hash));
	});
}

#[test]
fn timed_out_referendum_releases_preimage_request() {
	ExtBuilder::default().build_and_execute(|| {
		let hash = note_preimage(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			frame_support::traits::Bounded::Lookup { hash, len: 1 },
			DispatchTime::At(30),
		));
		assert!(Preimage::is_requested(&hash));
		run_to(21);
		assert_matches!(ReferendumInfoFor::<Test>::get(0), Some(ReferendumInfo::TimedOut(..)));
		assert!(!Preimage::is_requested(&hash));
	});
}

#[test]
fn rejected_referendum_releases_preimage_request() {
	ExtBuilder::default().build_and_execute(|| {
		let hash = note_preimage(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			frame_support::traits::Bounded::Lookup { hash, len: 1 },
			DispatchTime::At(30),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(2), 0));
		assert!(Preimage::is_requested(&hash));
		// No votes: deciding starts after the prepare period (4) and fails at the end of the
		// decision period (4).
		run_to(12);
		assert_matches!(ReferendumInfoFor::<Test>::get(0), Some(ReferendumInfo::Rejected(..)));
		assert!(!Preimage::is_requested(&hash));
	});
}

#[test]
fn killed_referendum_releases_preimage_request() {
	ExtBuilder::default().build_and_execute(|| {
		let hash = note_preimage(1);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Root.into()),
			frame_support::traits::Bounded::Lookup { hash, len: 1 },
			DispatchTime::At(30),
		));
		assert!(Preimage::is_requested(&hash));
		assert_ok!(Referenda::kill(RuntimeOrigin::root(), 0));
		assert!(!Preimage::is_requested(&hash));
	});
}

/// Ensures that `DispatchTime::After(0)` plus `min_enactment_period = 0` works.
#[test]
fn zero_enactment_delay_executes_proposal_at_next_block() {
	ExtBuilder::default().build_and_execute(|| {
		assert_eq!(Balances::free_balance(42), 0);
		assert_ok!(Referenda::submit(
			RuntimeOrigin::signed(1),
			Box::new(RawOrigin::Signed(1).into()),
			Preimage::bound(
				pallet_balances::Call::transfer_keep_alive { dest: 42, value: 20 }.into()
			)
			.unwrap(),
			DispatchTime::After(0),
		));
		assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(1), 0));
		assert_eq!(ReferendumCount::<Test>::get(), 1);
		set_tally(0, 100, 0);

		run_to(9);

		assert_eq!(Balances::free_balance(42), 20);
	});
}
