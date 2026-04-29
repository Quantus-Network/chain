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

//! # Scheduler tests.

use super::*;
use crate::mock::{
	logger::{self, Threshold},
	new_test_ext, root, run_to_block, LoggerCall, RuntimeCall, Scheduler, Test, *,
};
use frame_support::{
	assert_err, assert_noop, assert_ok,
	traits::{Contains, OnFinalize, OnInitialize, QueryPreimage, StorePreimage},
};
use sp_runtime::traits::Hash;

#[test]
#[docify::export]
fn basic_scheduling_works() {
	new_test_ext().execute_with(|| {
		// Call to schedule
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// BaseCallFilter should be implemented to accept `Logger::log` runtime call which is
		// implemented for `BaseFilter` in the mock runtime
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));

		// Schedule call to be executed at the 4th block
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap()
		));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());

		run_to_block(4);
		// `log` runtime call should have executed at block 4
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
#[docify::export]
fn scheduling_with_preimages_works() {
	new_test_ext().execute_with(|| {
		// Call to schedule
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		let hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		let len = call.using_encoded(|x| x.len()) as u32;

		// Important to use here `Bounded::Lookup` to ensure that that the Scheduler can request the
		// hash from PreImage to dispatch the call
		let hashed = Bounded::Lookup { hash, len };

		// Schedule call to be executed at block 4 with the PreImage hash
		assert_ok!(Scheduler::do_schedule(DispatchTime::At(4), 127, root(), hashed));

		// Register preimage on chain
		assert_ok!(Preimage::note_preimage(RuntimeOrigin::signed(0), call.encode()));
		assert!(Preimage::is_requested(&hash));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());

		run_to_block(4);
		// preimage should not have been removed when executed by the scheduler
		assert!(!Preimage::len(&hash).is_some());
		assert!(!Preimage::is_requested(&hash));
		// `log` runtime call should have executed at block 4
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn schedule_after_works() {
	new_test_ext().execute_with(|| {
		run_to_block(2);
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		// This will schedule the call 3 blocks after the next block... so block 3 + 3 = 6
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::After(BlockNumberOrTimestamp::BlockNumber(3)),
			127,
			root(),
			Preimage::bound(call).unwrap()
		));
		run_to_block(5);
		assert!(logger::log().is_empty());
		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn schedule_after_zero_works() {
	new_test_ext().execute_with(|| {
		run_to_block(2);
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::After(BlockNumberOrTimestamp::BlockNumber(0)),
			127,
			root(),
			Preimage::bound(call).unwrap()
		));
		// Will trigger on the next block.
		run_to_block(3);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn retry_scheduling_works() {
	new_test_ext().execute_with(|| {
		// task fails until block 8 is reached
		Threshold::<Test>::put((8, 100));
		// task 42 at #4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// retry 10 times every 3 blocks
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(3)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// task should be retried in block 7
		run_to_block(4);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7))[0].is_some());
		assert!(logger::log().is_empty());
		run_to_block(6);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7))[0].is_some());
		assert!(logger::log().is_empty());
		// task still fails, should be retried in block 10
		run_to_block(7);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10))[0].is_some());
		assert!(logger::log().is_empty());
		run_to_block(8);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10))[0].is_some());
		assert!(logger::log().is_empty());
		run_to_block(9);
		assert!(logger::log().is_empty());
		assert_eq!(Retries::<Test>::iter().count(), 1);
		// finally it should succeed
		run_to_block(10);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		assert_eq!(Retries::<Test>::iter().count(), 0);
		run_to_block(11);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(12);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn named_retry_scheduling_works() {
	new_test_ext().execute_with(|| {
		// task fails until block 8 is reached
		Threshold::<Test>::put((8, 100));
		// task 42 at #4
		let call = RuntimeCall::Logger(logger::Call::timed_log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		});
		assert_eq!(
			Scheduler::do_schedule_named(
				[1u8; 32],
				DispatchTime::At(4),
				127,
				root(),
				Preimage::bound(call).unwrap(),
			)
			.unwrap(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0)
		);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// retry 10 times every 3 blocks
		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[1u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(3)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// task should be retried in block 7
		run_to_block(4);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7))[0].is_some());
		assert!(logger::log().is_empty());
		run_to_block(6);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7))[0].is_some());
		assert!(logger::log().is_empty());
		// task still fails, should be retried in block 10
		run_to_block(7);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10))[0].is_some());
		assert!(logger::log().is_empty());
		run_to_block(8);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10))[0].is_some());
		assert!(logger::log().is_empty());
		run_to_block(9);
		assert!(logger::log().is_empty());
		assert_eq!(Retries::<Test>::iter().count(), 1);
		// finally it should succeed
		run_to_block(10);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		assert_eq!(Retries::<Test>::iter().count(), 0);
		run_to_block(11);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(12);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn retry_scheduling_multiple_tasks_works() {
	new_test_ext().execute_with(|| {
		// task fails until block 8 is reached
		Threshold::<Test>::put((8, 100));
		// task 20 at #4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 20,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		// task 42 at #4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// task 20 will be retried 3 times every block
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			3,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		// task 42 will be retried 10 times every 3 blocks
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 1),
			10,
			BlockNumberOrTimestamp::BlockNumber(3)
		));
		assert_eq!(Retries::<Test>::iter().count(), 2);
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// both tasks fail
		run_to_block(4);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).is_empty());
		// 20 is rescheduled for next block
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(5)).len(), 1);
		// 42 is rescheduled for block 7
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).len(), 1);
		assert!(logger::log().is_empty());
		// 20 still fails
		run_to_block(5);
		// 20 rescheduled for next block
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(6)).len(), 1);
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).len(), 1);
		assert_eq!(Retries::<Test>::iter().count(), 2);
		assert!(logger::log().is_empty());
		// 20 still fails
		run_to_block(6);
		// rescheduled for next block together with 42
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).len(), 2);
		assert_eq!(Retries::<Test>::iter().count(), 2);
		assert!(logger::log().is_empty());
		// both tasks will fail, for 20 it was the last retry so it's dropped
		run_to_block(7);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(8)).is_empty());
		// 42 is rescheduled for block 10
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10)).len(), 1);
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(logger::log().is_empty());
		run_to_block(8);
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10)).len(), 1);
		assert!(logger::log().is_empty());
		run_to_block(9);
		assert!(logger::log().is_empty());
		assert_eq!(Retries::<Test>::iter().count(), 1);
		// 42 runs successfully
		run_to_block(10);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		assert_eq!(Retries::<Test>::iter().count(), 0);
		run_to_block(11);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(12);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn retry_scheduling_multiple_named_tasks_works() {
	new_test_ext().execute_with(|| {
		// task fails until we reach block 8
		Threshold::<Test>::put((8, 100));
		// task 20 at #4
		assert_ok!(Scheduler::do_schedule_named(
			[20u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 20,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		// task 42 at #4
		assert_ok!(Scheduler::do_schedule_named(
			[42u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// task 20 will be retried 3 times every block
		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[20u8; 32],
			3,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		// task 42 will be retried 10 times every 3 block
		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[42u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(3)
		));
		assert_eq!(Retries::<Test>::iter().count(), 2);
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// both tasks fail
		run_to_block(4);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).is_empty());
		// 42 is rescheduled for block 7
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).len(), 1);
		// 20 is rescheduled for next block
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(5)).len(), 1);
		assert!(logger::log().is_empty());
		// 20 still fails
		run_to_block(5);
		// 20 rescheduled for next block
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(6)).len(), 1);
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).len(), 1);
		assert_eq!(Retries::<Test>::iter().count(), 2);
		assert!(logger::log().is_empty());
		// 20 still fails
		run_to_block(6);
		// 20 rescheduled for next block together with 42
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).len(), 2);
		assert_eq!(Retries::<Test>::iter().count(), 2);
		assert!(logger::log().is_empty());
		// both tasks will fail, for 20 it was the last retry so it's dropped
		run_to_block(7);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(8)).is_empty());
		// 42 is rescheduled for block 10
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10)).len(), 1);
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(logger::log().is_empty());
		run_to_block(8);
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(10)).len(), 1);
		assert!(logger::log().is_empty());
		run_to_block(9);
		assert!(logger::log().is_empty());
		assert_eq!(Retries::<Test>::iter().count(), 1);
		// 42 runs successfully
		run_to_block(10);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		assert_eq!(Retries::<Test>::iter().count(), 0);
		run_to_block(11);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(12);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn retry_scheduling_expires() {
	new_test_ext().execute_with(|| {
		// task will fail if we're past block 3
		Threshold::<Test>::put((1, 3));
		// task 42 at #4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// task 42 will be retried 3 times every block
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			3,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);
		run_to_block(3);
		assert!(logger::log().is_empty());
		// task 42 is scheduled for next block
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// task fails because we're past block 3
		run_to_block(4);
		// task is scheduled for next block
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(5))[0].is_some());
		// one retry attempt is consumed
		assert_eq!(
			Retries::<Test>::get((BlockNumberOrTimestamp::BlockNumber(5), 0))
				.unwrap()
				.remaining,
			2
		);
		assert!(logger::log().is_empty());
		// task fails again
		run_to_block(5);
		// task is scheduled for next block
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(5)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(6))[0].is_some());
		// another retry attempt is consumed
		assert_eq!(
			Retries::<Test>::get((BlockNumberOrTimestamp::BlockNumber(6), 0))
				.unwrap()
				.remaining,
			1
		);
		assert!(logger::log().is_empty());
		// task fails again
		run_to_block(6);
		// task is scheduled for next block
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(6)).is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7))[0].is_some());
		// another retry attempt is consumed
		assert_eq!(
			Retries::<Test>::get((BlockNumberOrTimestamp::BlockNumber(7), 0))
				.unwrap()
				.remaining,
			0
		);
		assert!(logger::log().is_empty());
		// task fails again
		run_to_block(7);
		// task ran out of retries so it gets dropped
		assert_eq!(Agenda::<Test>::iter().count(), 0);
		assert_eq!(Retries::<Test>::iter().count(), 0);
		assert!(logger::log().is_empty());
	});
}

#[test]
fn set_retry_bad_origin() {
	new_test_ext().execute_with(|| {
		// task 42 at #4 with account 101 as origin
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			RawOrigin::Signed(101).into(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// try to change the retry config with a different (non-root) account
		let res: Result<(), DispatchError> = Scheduler::set_retry(
			RuntimeOrigin::signed(102),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(2),
		);
		assert_eq!(res, Err(BadOrigin.into()));
	});
}

#[test]
fn set_named_retry_bad_origin() {
	new_test_ext().execute_with(|| {
		// task 42 at #4 with account 101 as origin
		assert_ok!(Scheduler::do_schedule_named(
			[42u8; 32],
			DispatchTime::At(4),
			127,
			RawOrigin::Signed(101).into(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// try to change the retry config with a different (non-root) account
		let res: Result<(), DispatchError> = Scheduler::set_retry_named(
			RuntimeOrigin::signed(102),
			[42u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(2),
		);
		assert_eq!(res, Err(BadOrigin.into()));
	});
}

#[test]
fn set_retry_works() {
	new_test_ext().execute_with(|| {
		// task 42 at #4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// make sure the retry configuration was stored
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(2)
		));
		assert_eq!(
			Retries::<Test>::get((BlockNumberOrTimestamp::BlockNumber(4), 0)),
			Some(RetryConfig {
				total_retries: 10,
				remaining: 10,
				period: BlockNumberOrTimestamp::BlockNumber(2)
			})
		);
	});
}

#[test]
fn set_named_retry_works() {
	new_test_ext().execute_with(|| {
		// task 42 at #4 with account 101 as origin
		assert_ok!(Scheduler::do_schedule_named(
			[42u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))[0].is_some());
		// make sure the retry configuration was stored
		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[42u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(2)
		));
		let address = Lookup::<Test>::get([42u8; 32]).unwrap();
		assert_eq!(
			Retries::<Test>::get(address),
			Some(RetryConfig {
				total_retries: 10,
				remaining: 10,
				period: BlockNumberOrTimestamp::BlockNumber(2)
			})
		);
	});
}

#[test]
fn reschedule_works() {
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		assert_eq!(
			Scheduler::do_schedule(
				DispatchTime::At(4),
				127,
				root(),
				Preimage::bound(call).unwrap()
			)
			.unwrap(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0)
		);

		run_to_block(3);
		assert!(logger::log().is_empty());

		assert_eq!(
			Scheduler::do_reschedule(
				(BlockNumberOrTimestamp::BlockNumber(4), 0),
				DispatchTime::At(6)
			)
			.unwrap(),
			(BlockNumberOrTimestamp::BlockNumber(6), 0)
		);

		assert_noop!(
			Scheduler::do_reschedule(
				(BlockNumberOrTimestamp::BlockNumber(6), 0),
				DispatchTime::At(6)
			),
			Error::<Test>::RescheduleNoChange
		);

		run_to_block(4);
		assert!(logger::log().is_empty());

		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn reschedule_named_works() {
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		assert_eq!(
			Scheduler::do_schedule_named(
				[1u8; 32],
				DispatchTime::At(4),
				127,
				root(),
				Preimage::bound(call).unwrap(),
			)
			.unwrap(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0)
		);

		run_to_block(3);
		assert!(logger::log().is_empty());

		assert_eq!(
			Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(6)).unwrap(),
			(BlockNumberOrTimestamp::BlockNumber(6), 0)
		);

		assert_noop!(
			Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(6)),
			Error::<Test>::RescheduleNoChange
		);

		run_to_block(4);
		assert!(logger::log().is_empty());

		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn cancel_named_scheduling_works_with_normal_cancel() {
	new_test_ext().execute_with(|| {
		// at #4.
		Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
				i: 69,
				weight: Weight::from_parts(10, 0),
			}))
			.unwrap(),
		)
		.unwrap();
		let i = Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
				i: 42,
				weight: Weight::from_parts(10, 0),
			}))
			.unwrap(),
		)
		.unwrap();
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));
		assert_ok!(Scheduler::do_cancel(None, i));
		run_to_block(100);
		assert!(logger::log().is_empty());
	});
}

#[test]
fn scheduler_respects_weight_limits() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 69, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// 69 and 42 do not fit together
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(5);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 69u32)]);
	});
}

#[test]
fn retry_respects_weight_limits() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		// schedule 42
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(8),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// schedule 20 with a call that will fail until we reach block 8
		Threshold::<Test>::put((8, 100));
		let call = RuntimeCall::Logger(LoggerCall::timed_log { i: 20, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// set a retry config for 20 for 10 retries every block
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		// 20 should fail and be retried later
		run_to_block(4);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(5))[0].is_some());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(8))[0].is_some());
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(logger::log().is_empty());
		// 20 still fails but is scheduled next block together with 42
		run_to_block(7);
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(8)).len(), 2);
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(logger::log().is_empty());
		// 20 and 42 do not fit together
		// 42 is executed as it was first in the queue
		// 20 is still on the 8th block's agenda
		run_to_block(8);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(8))[0].is_none());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(8))[1].is_some());
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		// 20 is executed and the schedule is cleared
		run_to_block(9);
		assert_eq!(Agenda::<Test>::iter().count(), 0);
		assert_eq!(Retries::<Test>::iter().count(), 0);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 20u32)]);
	});
}

#[test]
fn try_schedule_retry_respects_weight_limits() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let service_agendas_weight = <Test as Config>::WeightInfo::service_agendas_base();
		let service_agenda_weight = <Test as Config>::WeightInfo::service_agenda_base(
			<Test as Config>::MaxScheduledPerBlock::get(),
		);
		let actual_service_agenda_weight = <Test as Config>::WeightInfo::service_agenda_base(1);
		// Some weight for `service_agenda` will be refunded, so we need to make sure the weight
		// `try_schedule_retry` is going to ask for is greater than this difference, and we take a
		// safety factor of 10 to make sure we're over that limit.
		let meter = WeightMeter::with_limit(
			<Test as Config>::WeightInfo::schedule_retry(
				<Test as Config>::MaxScheduledPerBlock::get(),
			) / 10,
		);
		assert!(meter.can_consume(service_agenda_weight - actual_service_agenda_weight));

		let reference_call =
			RuntimeCall::Logger(LoggerCall::timed_log { i: 20, weight: max_weight / 3 * 2 });
		let bounded = <Test as Config>::Preimages::bound(reference_call).unwrap();
		let base_weight = <Test as Config>::WeightInfo::service_task(
			bounded.lookup_len().map(|x| x as usize),
			false,
		);
		// we make the call cost enough so that all checks have enough weight to run aside from
		// `try_schedule_retry`
		let call_weight = max_weight - service_agendas_weight - service_agenda_weight - base_weight;
		let call = RuntimeCall::Logger(LoggerCall::timed_log { i: 20, weight: call_weight });
		// schedule 20 with a call that will fail until we reach block 8
		Threshold::<Test>::put((8, 100));

		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// set a retry config for 20 for 10 retries every block
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		// 20 should fail and, because of insufficient weight, it should not be scheduled again
		run_to_block(4);
		// nothing else should be scheduled
		assert_eq!(Agenda::<Test>::iter().count(), 0);
		assert_eq!(Retries::<Test>::iter().count(), 0);
		assert_eq!(logger::log(), vec![]);
		// check the `RetryFailed` event happened
		let events = frame_system::Pallet::<Test>::events();
		let system_event: <Test as frame_system::Config>::RuntimeEvent =
			Event::RetryFailed { task: (BlockNumberOrTimestamp::BlockNumber(4), 0), id: None }
				.into();
		// compare to the last event record
		let frame_system::EventRecord { event, .. } = &events[events.len() - 1];
		assert_eq!(event, &system_event);
	});
}

/// Permanently overweight calls are removed from the agenda after emitting an event.
#[test]
fn scheduler_removes_permanently_overweight_call() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		run_to_block(4);
		assert_eq!(logger::log(), vec![]);

		assert!(System::events().iter().any(|e| e.event ==
			crate::Event::PermanentlyOverweight {
				task: (BlockNumberOrTimestamp::BlockNumber(4), 0),
				id: None
			}
			.into()));
		assert_eq!(Agenda::<Test>::iter().count(), 0);
	});
}

#[test]
fn scheduler_respects_priority_ordering() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 3 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			1,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 69, weight: max_weight / 3 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			0,
			root(),
			Preimage::bound(call).unwrap(),
		));
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 69u32), (root(), 42u32)]);
	});
}

#[test]
fn scheduler_respects_priority_ordering_with_soft_deadlines() {
	new_test_ext().execute_with(|| {
		let max_weight: Weight = <Test as Config>::MaximumWeight::get();
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 5 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			255,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 69, weight: max_weight / 5 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 2600, weight: max_weight / 5 * 4 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			126,
			root(),
			Preimage::bound(call).unwrap(),
		));

		// 2600 does not fit with 69 or 42, but has higher priority, so will go through
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 2600u32)]);
		// 69 and 42 fit together
		run_to_block(5);
		assert_eq!(logger::log(), vec![(root(), 2600u32), (root(), 69u32), (root(), 42u32)]);
	});
}

#[test]
fn on_initialize_weight_is_correct() {
	new_test_ext().execute_with(|| {
		MockTimestamp::set_timestamp(10000);

		let call_weight = Weight::from_parts(25, 0);

		// Named
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 3,
			weight: call_weight + Weight::from_parts(1, 0),
		});
		assert_ok!(Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(3),
			255,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: call_weight + Weight::from_parts(2, 0),
		});
		// Anon
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(2),
			128,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: call_weight + Weight::from_parts(3, 0),
		});
		// Anon
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(2),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// Named
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 2600,
			weight: call_weight + Weight::from_parts(4, 0),
		});
		assert_ok!(Scheduler::do_schedule_named(
			[2u8; 32],
			DispatchTime::At(1),
			126,
			root(),
			Preimage::bound(call).unwrap(),
		));

		// Will include the named only
		assert_eq!(
			Scheduler::on_initialize(1),
			TestWeightInfo::service_agendas_base() +
				TestWeightInfo::service_agenda_base(1) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, true) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(4, 0) +
				Weight::from_parts(2, 0) // add for time based
		);
		assert_eq!(IncompleteBlockSince::<Test>::get(), None);
		assert_eq!(logger::log(), vec![(root(), 2600u32)]);

		// Will include both anon tasks
		assert_eq!(
			Scheduler::on_initialize(2),
			TestWeightInfo::service_agendas_base() +
				TestWeightInfo::service_agenda_base(2) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, false) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(3, 0) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, false) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(2, 0) +
				Weight::from_parts(2, 0) // add for time based
		);
		assert_eq!(IncompleteBlockSince::<Test>::get(), None);
		assert_eq!(logger::log(), vec![(root(), 2600u32), (root(), 69u32), (root(), 42u32)]);

		// Will include named only
		assert_eq!(
			Scheduler::on_initialize(3),
			TestWeightInfo::service_agendas_base() +
				TestWeightInfo::service_agenda_base(1) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, true) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(1, 0) +
				Weight::from_parts(2, 0) // add for time based
		);
		assert_eq!(IncompleteBlockSince::<Test>::get(), None);
		assert_eq!(
			logger::log(),
			vec![(root(), 2600u32), (root(), 69u32), (root(), 42u32), (root(), 3u32)]
		);

		// Will contain none
		let actual_weight = Scheduler::on_initialize(4);
		assert_eq!(
			actual_weight,
			TestWeightInfo::service_agendas_base() +
				TestWeightInfo::service_agenda_base(0) +
				Weight::from_parts(2, 0) // add for time based
		);
	});
}

#[test]
fn root_calls_works() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_ok!(Scheduler::schedule_named(RuntimeOrigin::root(), [1u8; 32], 4, 127, call,));
		assert_ok!(Scheduler::schedule(RuntimeOrigin::root(), 4, 127, call2));
		run_to_block(3);
		// Scheduled calls are in the agenda.
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		assert!(logger::log().is_empty());
		assert_ok!(Scheduler::cancel_named(RuntimeOrigin::root(), [1u8; 32]));
		assert_ok!(Scheduler::cancel(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::BlockNumber(4),
			1
		));
		// Scheduled calls are made NONE, so should not effect state
		run_to_block(100);
		assert!(logger::log().is_empty());
	});
}

#[test]
fn fails_to_schedule_task_in_the_past() {
	new_test_ext().execute_with(|| {
		run_to_block(3);

		let call1 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		let call3 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));

		assert_noop!(
			Scheduler::schedule_named(RuntimeOrigin::root(), [1u8; 32], 2, 127, call1),
			Error::<Test>::TargetBlockNumberInPast,
		);

		assert_noop!(
			Scheduler::schedule(RuntimeOrigin::root(), 2, 127, call2),
			Error::<Test>::TargetBlockNumberInPast,
		);

		assert_noop!(
			Scheduler::schedule(RuntimeOrigin::root(), 3, 127, call3),
			Error::<Test>::TargetBlockNumberInPast,
		);
	});
}

#[test]
fn should_use_origin() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_ok!(Scheduler::schedule_named(
			system::RawOrigin::Signed(1).into(),
			[1u8; 32],
			4,
			127,
			call,
		));
		assert_ok!(Scheduler::schedule(system::RawOrigin::Signed(1).into(), 4, 127, call2,));
		run_to_block(3);
		// Scheduled calls are in the agenda.
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		assert!(logger::log().is_empty());
		assert_ok!(Scheduler::cancel_named(system::RawOrigin::Signed(1).into(), [1u8; 32]));
		assert_ok!(Scheduler::cancel(
			system::RawOrigin::Signed(1).into(),
			BlockNumberOrTimestamp::BlockNumber(4),
			1
		));
		// Scheduled calls are made NONE, so should not effect state
		run_to_block(100);
		assert!(logger::log().is_empty());
	});
}

#[test]
fn should_check_origin() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_noop!(
			Scheduler::schedule_named(system::RawOrigin::Signed(2).into(), [1u8; 32], 4, 127, call),
			BadOrigin
		);
		assert_noop!(
			Scheduler::schedule(system::RawOrigin::Signed(2).into(), 4, 127, call2),
			BadOrigin
		);
	});
}

#[test]
fn should_check_origin_for_cancel() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log_without_filter {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log_without_filter {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_ok!(Scheduler::schedule_named(
			system::RawOrigin::Signed(1).into(),
			[1u8; 32],
			4,
			127,
			call,
		));
		assert_ok!(Scheduler::schedule(system::RawOrigin::Signed(1).into(), 4, 127, call2,));
		run_to_block(3);
		// Scheduled calls are in the agenda.
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		assert!(logger::log().is_empty());
		assert_noop!(
			Scheduler::cancel_named(system::RawOrigin::Signed(2).into(), [1u8; 32]),
			BadOrigin
		);
		assert_noop!(
			Scheduler::cancel(
				system::RawOrigin::Signed(2).into(),
				BlockNumberOrTimestamp::BlockNumber(4),
				1
			),
			BadOrigin
		);
		assert_noop!(Scheduler::cancel_named(system::RawOrigin::Root.into(), [1u8; 32]), BadOrigin);
		assert_noop!(
			Scheduler::cancel(
				system::RawOrigin::Root.into(),
				BlockNumberOrTimestamp::BlockNumber(4),
				1
			),
			BadOrigin
		);
		run_to_block(5);
		assert_eq!(
			logger::log(),
			vec![
				(system::RawOrigin::Signed(1).into(), 69u32),
				(system::RawOrigin::Signed(1).into(), 42u32)
			]
		);
	});
}

#[test]
fn cancel_removes_retry_entry() {
	new_test_ext().execute_with(|| {
		// task fails until block 99 is reached
		Threshold::<Test>::put((99, 100));
		// task 20 at #4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 20,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		// named task 42 at #4
		assert_ok!(Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// task 20 will be retried 3 times every block
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		// task 42 will be retried 10 times every 3 blocks
		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[1u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		assert_eq!(Retries::<Test>::iter().count(), 2);
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// both tasks fail
		run_to_block(4);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).is_empty());
		// 42 and 20 are rescheduled for next block
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(5)).len(), 2);
		assert!(logger::log().is_empty());
		// 42 and 20 still fail
		run_to_block(5);
		// 42 and 20 rescheduled for next block
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(6)).len(), 2);
		assert_eq!(Retries::<Test>::iter().count(), 2);
		assert!(logger::log().is_empty());

		// even though 42 is being retried, the tasks scheduled for retries are not named
		assert_eq!(Lookup::<Test>::iter().count(), 0);
		assert_ok!(Scheduler::cancel(root().into(), BlockNumberOrTimestamp::BlockNumber(6), 0));

		// 20 is removed, 42 still fails
		run_to_block(6);
		// 42 rescheduled for next block
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(7)).len(), 1);
		// 20's retry entry is removed
		assert!(!Retries::<Test>::contains_key((BlockNumberOrTimestamp::BlockNumber(4), 0)));
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(logger::log().is_empty());

		assert_ok!(Scheduler::cancel(root().into(), BlockNumberOrTimestamp::BlockNumber(7), 0));

		// both tasks are canceled, everything is removed now
		run_to_block(7);
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(8)).is_empty());
		assert_eq!(Retries::<Test>::iter().count(), 0);
	});
}

#[test]
fn cancel_retries_works() {
	new_test_ext().execute_with(|| {
		// task fails until block 99 is reached
		Threshold::<Test>::put((99, 100));
		// task 20 at #4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 20,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		// named task 42 at #4
		assert_ok!(Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));

		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// task 20 will be retried 3 times every block
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		// task 42 will be retried 10 times every 3 blocks
		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[1u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(1)
		));
		assert_eq!(Retries::<Test>::iter().count(), 2);
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);
		// cancel the retry config for 20
		assert_ok!(Scheduler::cancel_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);
		// cancel the retry config for 42
		assert_ok!(Scheduler::cancel_retry_named(root().into(), [1u8; 32]));
		assert_eq!(Retries::<Test>::iter().count(), 0);
		run_to_block(4);
		// both tasks failed and there are no more retries, so they are evicted
		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 0);
		assert_eq!(Retries::<Test>::iter().count(), 0);
	});
}

#[test]
fn unavailable_preimage_removes_task_completely() {
	// When a preimage is unavailable, the task is a terminal failure: it cannot execute
	// without its call data. The task is completely removed (Agenda, Lookup, Retries).
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(1000, 0) });
		let hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		let len = call.using_encoded(|x| x.len()) as u32;
		// Important to use here `Bounded::Lookup` to ensure that we request the hash.
		let hashed = Bounded::Lookup { hash, len };
		let name: [u8; 32] = hash.as_ref().try_into().unwrap();

		let _address =
			Scheduler::do_schedule_named(name, DispatchTime::At(4), 127, root(), hashed.clone())
				.unwrap();
		assert!(Preimage::is_requested(&hash));
		assert!(Lookup::<Test>::contains_key(name));

		// Run to a very large block - preimage never submitted.
		run_to_block(10);

		// Task was not executed.
		assert!(logger::log().is_empty());

		// CallUnavailable event was emitted.
		assert!(System::events().iter().any(|e| matches!(
			e.event,
			RuntimeEvent::Scheduler(crate::Event::CallUnavailable {
				task: (BlockNumberOrTimestamp::BlockNumber(4), 0),
				id: Some(_)
			})
		)));

		// Preimage reference was dropped.
		assert!(!Preimage::is_requested(&hash));

		// Lookup is removed.
		assert!(!Lookup::<Test>::contains_key(name));

		// Agenda is cleaned up - task is completely removed.
		assert_eq!(Agenda::<Test>::iter().count(), 0);
	});
}

#[test]
fn overweight_named_task_keeps_lookup_and_can_be_cancelled() {
	// When a named task is deferred due to weight limits (not permanently overweight),
	// the Lookup is preserved so the task can be cancelled by name.
	new_test_ext().execute_with(|| {
		// Max block weight is 2_000_000_000_000. Use a call heavy enough to exceed
		// remaining weight after a light task, but light enough to fit when alone.
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(1_500_000_000_000, 0),
		});
		let bounded = Preimage::bound(call).unwrap();
		let name: [u8; 32] = [1u8; 32];

		// Schedule a light task that consumes significant weight.
		let light_call = RuntimeCall::Logger(LoggerCall::log {
			i: 1,
			weight: Weight::from_parts(1_000_000_000_000, 0),
		});
		Scheduler::do_schedule(
			DispatchTime::At(4),
			100,
			root(),
			Preimage::bound(light_call).unwrap(),
		)
		.unwrap();

		// Schedule the heavy named task with lower priority.
		Scheduler::do_schedule_named(name, DispatchTime::At(4), 127, root(), bounded.clone())
			.unwrap();
		assert!(Lookup::<Test>::contains_key(name));

		// Run to block 4 - light task executes, heavy task deferred.
		run_to_block(4);

		// Light task executed.
		assert_eq!(logger::log(), vec![(root(), 1u32)]);

		// Heavy task was not executed.
		assert!(logger::log().iter().all(|(_, i)| *i != 42));

		// Lookup is preserved for overweight-deferred tasks.
		assert!(Lookup::<Test>::contains_key(name));

		// Cancel the deferred task by name - this should succeed.
		assert_ok!(Scheduler::do_cancel_named(None, name));

		// Lookup is removed after cancellation.
		assert!(!Lookup::<Test>::contains_key(name));

		// Canceled event was emitted.
		assert!(System::events()
			.iter()
			.any(|e| matches!(e.event, RuntimeEvent::Scheduler(crate::Event::Canceled { .. }))));

		// Run to block 100 - the cancelled task should not execute.
		run_to_block(100);
		assert!(logger::log().iter().all(|(_, i)| *i != 42));
	});
}

#[test]
fn overweight_named_task_keeps_lookup_and_can_be_rescheduled() {
	// When a named task is deferred due to weight limits (not permanently overweight),
	// the Lookup is preserved so the task can be rescheduled by name.
	new_test_ext().execute_with(|| {
		// Max block weight is 2_000_000_000_000. Use a call heavy enough to exceed
		// remaining weight after a light task, but light enough to fit when alone.
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(1_500_000_000_000, 0),
		});
		let bounded = Preimage::bound(call).unwrap();
		let name: [u8; 32] = [1u8; 32];

		// Schedule a light task that consumes significant weight.
		let light_call = RuntimeCall::Logger(LoggerCall::log {
			i: 1,
			weight: Weight::from_parts(1_000_000_000_000, 0),
		});
		Scheduler::do_schedule(
			DispatchTime::At(4),
			100,
			root(),
			Preimage::bound(light_call).unwrap(),
		)
		.unwrap();

		// Schedule the heavy named task with lower priority (higher number = lower priority).
		Scheduler::do_schedule_named(name, DispatchTime::At(4), 127, root(), bounded.clone())
			.unwrap();
		assert!(Lookup::<Test>::contains_key(name));

		// Run to block 4 - light task executes (1T weight), heavy task deferred
		// because remaining weight (1T) < heavy task weight (1.5T).
		run_to_block(4);

		// Light task executed.
		assert_eq!(logger::log(), vec![(root(), 1u32)]);

		// Heavy task was not executed (deferred due to overweight).
		assert!(logger::log().iter().all(|(_, i)| *i != 42));

		// Lookup is preserved for overweight-deferred tasks.
		assert!(Lookup::<Test>::contains_key(name));

		// Reschedule the deferred task by name to a future block.
		let new_address = Scheduler::do_reschedule_named(name, DispatchTime::At(100)).unwrap();
		assert_eq!(new_address.0, BlockNumberOrTimestamp::BlockNumber(100));

		// Lookup now points to new address.
		assert_eq!(Lookup::<Test>::get(name), Some(new_address));

		// Run to block 100 - now the heavy task is first (full 2T available) and can execute.
		run_to_block(100);

		// Heavy task executed (1.5T < 2T max).
		assert!(logger::log().iter().any(|(_, i)| *i == 42));

		// Lookup is removed after successful execution.
		assert!(!Lookup::<Test>::contains_key(name));
	});
}

/// Using the scheduler as `v3::Anon` works.
#[test]
fn scheduler_v3_anon_basic_works() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// Schedule a call.
		let _address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());
		// Executes in block 4.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		// ... but not again.
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn scheduler_v3_anon_cancel_works() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule a call.
		let address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();
		// Cancel the call.
		assert_ok!(<Scheduler as Anon<_, _, _>>::cancel(address));
		// It did not get executed.
		run_to_block(100);
		assert!(logger::log().is_empty());
		// Cannot cancel again.
		assert_err!(<Scheduler as Anon<_, _, _>>::cancel(address), DispatchError::Unavailable);
	});
}

#[test]
fn scheduler_v3_anon_reschedule_works() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// Schedule a call.
		let address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Cannot re-schedule into the same block.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(4)),
			Error::<Test>::RescheduleNoChange
		);
		// Cannot re-schedule into the past.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(3)),
			Error::<Test>::TargetBlockNumberInPast
		);
		// Re-schedule to block 5.
		assert_ok!(<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(5)));
		// Scheduled for block 5.
		run_to_block(4);
		assert!(logger::log().is_empty());
		run_to_block(5);
		// Does execute in block 5.
		assert_eq!(logger::log(), vec![(root(), 42)]);
		// Cannot re-schedule executed task.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(10)),
			DispatchError::Unavailable
		);
	});
}

#[test]
fn scheduler_v3_anon_next_schedule_time_works() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule a call.
		let address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Scheduled for block 4.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(address), Ok(4));
		// Block 4 executes it.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42)]);

		// It has no dispatch time anymore.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::next_dispatch_time(address),
			DispatchError::Unavailable
		);
	});
}

/// Cancelled task returns Unavailable for next_dispatch_time (empty slot check).
#[test]
fn scheduler_v3_anon_next_dispatch_time_returns_unavailable_for_cancelled_task() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// Schedule a call.
		let address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		// Verify it's scheduled.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(address), Ok(4));

		// Cancel the task - this leaves an empty slot (None) in the agenda.
		assert_ok!(<Scheduler as Anon<_, _, _>>::cancel(address));

		// next_dispatch_time should return Unavailable for the empty slot,
		// not succeed just because the index exists in the agenda vector.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::next_dispatch_time(address),
			DispatchError::Unavailable
		);
	});
}

/// Re-scheduling a task changes its next dispatch time.
#[test]
fn scheduler_v3_anon_reschedule_and_next_schedule_time_work() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule a call.
		let old_address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Scheduled for block 4.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(old_address), Ok(4));
		// Re-schedule to block 5.
		let address =
			<Scheduler as Anon<_, _, _>>::reschedule(old_address, DispatchTime::At(5)).unwrap();
		assert!(address != old_address);
		// Scheduled for block 5.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(address), Ok(5));

		// Block 4 does nothing.
		run_to_block(4);
		assert!(logger::log().is_empty());
		// Block 5 executes it.
		run_to_block(5);
		assert_eq!(logger::log(), vec![(root(), 42)]);
	});
}

#[test]
fn scheduler_v3_anon_schedule_agenda_overflows() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};

	let max: u32 = <Test as Config>::MaxScheduledPerBlock::get();

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule the maximal number allowed per block.
		for _ in 0..max {
			<Scheduler as Anon<_, _, _>>::schedule(
				DispatchTime::At(4),
				None,
				127,
				root(),
				bound.clone(),
			)
			.unwrap();
		}

		// One more time and it errors.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::schedule(DispatchTime::At(4), None, 127, root(), bound,),
			DispatchError::Exhausted
		);

		run_to_block(4);
		// All scheduled calls are executed.
		assert_eq!(logger::log().len() as u32, max);
	});
}

/// Cancelling and scheduling does not overflow the agenda but fills holes.
#[test]
fn scheduler_v3_anon_cancel_and_schedule_fills_holes() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};

	let max: u32 = <Test as Config>::MaxScheduledPerBlock::get();
	assert!(max > 3, "This test only makes sense for MaxScheduledPerBlock > 3");

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let mut addrs = Vec::<_>::default();

		// Schedule the maximal number allowed per block.
		for _ in 0..max {
			addrs.push(
				<Scheduler as Anon<_, _, _>>::schedule(
					DispatchTime::At(4),
					None,
					127,
					root(),
					bound.clone(),
				)
				.unwrap(),
			);
		}
		// Cancel three of them.
		for addr in addrs.into_iter().take(3) {
			<Scheduler as Anon<_, _, _>>::cancel(addr).unwrap();
		}
		// Schedule three new ones.
		for i in 0..3 {
			let (_block, index) = <Scheduler as Anon<_, _, _>>::schedule(
				DispatchTime::At(4),
				None,
				127,
				root(),
				bound.clone(),
			)
			.unwrap();
			assert_eq!(i, index);
		}

		run_to_block(4);
		// Maximum number of calls are executed.
		assert_eq!(logger::log().len() as u32, max);
	});
}

/// Re-scheduling does not overflow the agenda but fills holes.
#[test]
fn scheduler_v3_anon_reschedule_fills_holes() {
	use frame_support::traits::schedule::{v3::Anon, DispatchTime};
	let max: u32 = <Test as Config>::MaxScheduledPerBlock::get();
	assert!(max > 3, "pre-condition: This test only makes sense for MaxScheduledPerBlock > 3");

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let mut addrs = Vec::<_>::default();

		// Schedule the maximal number allowed per block.
		for _ in 0..max {
			addrs.push(
				<Scheduler as Anon<_, _, _>>::schedule(
					DispatchTime::At(4),
					None,
					127,
					root(),
					bound.clone(),
				)
				.unwrap(),
			);
		}
		let mut new_addrs = Vec::<_>::default();
		// Reversed last three elements of block 4.
		let last_three = addrs.into_iter().rev().take(3).collect::<Vec<_>>();
		// Re-schedule three of them to block 5.
		for addr in last_three.iter().cloned() {
			new_addrs
				.push(<Scheduler as Anon<_, _, _>>::reschedule(addr, DispatchTime::At(5)).unwrap());
		}
		// Re-scheduling them back into block 3 should result in the same addrs.
		for (old, want) in new_addrs.into_iter().zip(last_three.into_iter().rev()) {
			let new = <Scheduler as Anon<_, _, _>>::reschedule(old, DispatchTime::At(4)).unwrap();
			assert_eq!(new, want);
		}

		run_to_block(4);
		// Maximum number of calls are executed.
		assert_eq!(logger::log().len() as u32, max);
	});
}

/// The scheduler can be used as `v3::Named` trait.
#[test]
fn scheduler_v3_named_basic_works() {
	use frame_support::traits::schedule::{v3::Named, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let name = [1u8; 32];

		// Schedule a call.
		let _address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());
		// Executes in block 4.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		// ... but not again.
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

/// A named task can be cancelled by its name.
#[test]
fn scheduler_v3_named_cancel_named_works() {
	use frame_support::traits::schedule::{v3::Named, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let name = [1u8; 32];

		// Schedule a call.
		<Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();
		// Cancel the call by name.
		assert_ok!(<Scheduler as Named<_, _, _>>::cancel_named(name));
		// It did not get executed.
		run_to_block(100);
		assert!(logger::log().is_empty());
		// Cannot cancel again.
		assert_noop!(<Scheduler as Named<_, _, _>>::cancel_named(name), DispatchError::Unavailable);
	});
}

/// A named task can also be cancelled by its address.
#[test]
fn scheduler_v3_named_cancel_without_name_works() {
	use frame_support::traits::schedule::{
		v3::{Anon, Named},
		DispatchTime,
	};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let name = [1u8; 32];

		// Schedule a call.
		let address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();
		// Cancel the call by address.
		assert_ok!(<Scheduler as Anon<_, _, _>>::cancel(address));
		// It did not get executed.
		run_to_block(100);
		assert!(logger::log().is_empty());
		// Cannot cancel again.
		assert_err!(<Scheduler as Anon<_, _, _>>::cancel(address), DispatchError::Unavailable);
	});
}

/// A named task can be re-scheduled by its name but not by its address.
#[test]
fn scheduler_v3_named_reschedule_named_works() {
	use frame_support::traits::schedule::{
		v3::{Anon, Named},
		DispatchTime,
	};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let name = [1u8; 32];

		// Schedule a call.
		let address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Cannot re-schedule by address.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(10)),
			Error::<Test>::Named,
		);
		// Cannot re-schedule into the same block.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(4)),
			Error::<Test>::RescheduleNoChange
		);
		// Cannot re-schedule into the past.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(3)),
			Error::<Test>::TargetBlockNumberInPast
		);
		// Re-schedule to block 5.
		assert_ok!(<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(5)));
		// Scheduled for block 5.
		run_to_block(4);
		assert!(logger::log().is_empty());
		run_to_block(5);
		// Does execute in block 5.
		assert_eq!(logger::log(), vec![(root(), 42)]);
		// Cannot re-schedule executed task.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(10)),
			DispatchError::Unavailable
		);
		// Also not by address.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(10)),
			DispatchError::Unavailable
		);
	});
}

#[test]
fn scheduler_v3_named_next_schedule_time_works() {
	use frame_support::traits::schedule::{
		v3::{Anon, Named},
		DispatchTime,
	};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let name = [1u8; 32];

		// Schedule a call.
		let address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Scheduled for block 4.
		assert_eq!(<Scheduler as Named<_, _, _>>::next_dispatch_time(name), Ok(4));
		// Also works by address.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(address), Ok(4));
		// Block 4 executes it.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42)]);

		// It has no dispatch time anymore.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::next_dispatch_time(name),
			DispatchError::Unavailable
		);
		// Also not by address.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::next_dispatch_time(address),
			DispatchError::Unavailable
		);
	});
}

#[test]
fn cancel_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let address = Scheduler::do_schedule(
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		let address2 = Scheduler::do_schedule(
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 2);
		assert_ok!(Scheduler::do_cancel(None, address));
		// still two tasks at agenda, `None` and `Some`.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 2);
		// cancel last task from `when` agenda.
		assert_ok!(Scheduler::do_cancel(None, address2));
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 0);
	});
}

#[test]
fn cancel_named_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		Scheduler::do_schedule_named(
			[2u8; 32],
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 2);
		assert_ok!(Scheduler::do_cancel_named(None, [2u8; 32]));
		// removes trailing `None` and leaves one task.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 1);
		// cancel last task from `when` agenda.
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 0);
	});
}

#[test]
fn reschedule_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let address = Scheduler::do_schedule(
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		let address2 = Scheduler::do_schedule(
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 2);
		assert_ok!(Scheduler::do_cancel(None, address));
		// still two tasks at agenda, `None` and `Some`.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 2);
		// reschedule last task from `when` agenda.
		assert_eq!(
			Scheduler::do_reschedule(address2, DispatchTime::At(when + 1)).unwrap(),
			(BlockNumberOrTimestamp::BlockNumber(when + 1), 0)
		);
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 0);
	});
}

#[test]
fn reschedule_named_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		Scheduler::do_schedule_named(
			[2u8; 32],
			DispatchTime::At(when),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 2);
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));
		// still two tasks at agenda, `None` and `Some`.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 2);
		// reschedule last task from `when` agenda.
		assert_eq!(
			Scheduler::do_reschedule_named([2u8; 32], DispatchTime::At(when + 1)).unwrap(),
			(BlockNumberOrTimestamp::BlockNumber(when + 1), 0)
		);
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(when)).len() == 0);
	});
}

/// Ensures that an unavailable call sends an event.
#[test]
fn unavailable_call_is_detected() {
	use frame_support::traits::schedule::{v3::Named, DispatchTime};

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		let len = call.using_encoded(|x| x.len()) as u32;
		// Important to use here `Bounded::Lookup` to ensure that we request the hash.
		let bound = Bounded::Lookup { hash, len };

		let name = [1u8; 32];

		// Schedule a call.
		let _address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		// Ensure the preimage isn't available
		assert!(!Preimage::have(&bound));
		// But we have requested it
		assert!(Preimage::is_requested(&hash));

		// Executes in block 4.
		run_to_block(4);

		assert_eq!(
			System::events().last().unwrap().event,
			crate::Event::CallUnavailable {
				task: (BlockNumberOrTimestamp::BlockNumber(4), 0),
				id: Some(name)
			}
			.into()
		);
		// It should not be requested anymore.
		assert!(!Preimage::is_requested(&hash));
	});
}
#[test]
fn time_based_agenda_is_processed_correctly() {
	new_test_ext().execute_with(|| {
		// Bucket size is 10_000ms. Schedule a task for "after 15_000ms".
		// normalize(15000) = 20000, then +bucket = 30000 to ensure task fires after target.
		let schedule_time_ms = 15_000;

		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(schedule_time_ms),
			0,
			Box::new(RuntimeCall::Logger(LoggerCall::log {
				i: 1,
				weight: Weight::from_parts(100, 0)
			})),
		));

		// Verify task is scheduled in bucket 30_000 (normalized 20000 + extra bucket)
		assert!(
			Agenda::<Test>::get(BlockNumberOrTimestamp::Timestamp(30000))
				.iter()
				.any(|s| s.is_some()),
			"Task should be scheduled in bucket 30000"
		);

		// First block is at 0ms.
		MockTimestamp::set_timestamp(0);
		run_to_block(1); // This will trigger on_initialize which processes timestamp agendas

		// Assert nothing is logged yet.
		assert_eq!(logger::log(), vec![]);

		// Set time to 16_000ms - this is past target 15_000 but still before bucket 30_000's start
		// Task should NOT execute yet (bucket hasn't been reached)
		MockTimestamp::set_timestamp(16_000);
		run_to_block(2);
		assert_eq!(
			logger::log().len(),
			0,
			"Task should not execute before bucket 30000 is reached"
		);

		// Set time to 21_000ms - now we're in bucket 30000, task should execute
		MockTimestamp::set_timestamp(21_000);
		run_to_block(3);
		assert_eq!(logger::log().len(), 1);
		assert_eq!(logger::log()[0].1, 1);
	});
}

#[test]
fn time_based_agenda_prevents_bucket_skipping_after_time_skip() {
	new_test_ext().execute_with(|| {
		// Bucket size is 10_000ms.
		// Task 1: After(16000) -> normalize(16000) = 20000, +bucket = 30000
		// Task 2: After(100000) -> normalize(100000) = 110000, +bucket = 120000
		let schedule_time_ms = 16_000;
		let schedule_time_ms_2 = 100_000;

		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(schedule_time_ms),
			0,
			Box::new(RuntimeCall::Logger(LoggerCall::log {
				i: 1,
				weight: Weight::from_parts(100, 0)
			})),
		));

		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(schedule_time_ms_2),
			0,
			Box::new(RuntimeCall::Logger(LoggerCall::log {
				i: 2,
				weight: Weight::from_parts(100, 0)
			})),
		));

		// First block is at 0ms.
		MockTimestamp::set_timestamp(1000);
		run_to_block(2); // This will trigger on_initialize which processes timestamp agendas

		// Assert nothing is logged yet.
		assert_eq!(logger::log(), vec![]);

		// Jump time far ahead, skipping buckets - now past bucket 30000's start (20001)
		let future_time_ms = 50_000;
		MockTimestamp::set_timestamp(future_time_ms);
		// Process the block at the future time.
		run_to_block(3); // This will trigger on_initialize which processes timestamp agendas

		// Task 1 (bucket 30000) should execute since we're past 20001
		assert_eq!(logger::log().len(), 1);
		assert_eq!(logger::log()[0].1, 1);

		// Jump time ahead but not past bucket 120000's start (110001)
		let future_time_ms = 80_000;
		MockTimestamp::set_timestamp(future_time_ms);
		// Process the block at the future time.
		run_to_block(4); // This will trigger on_initialize which processes timestamp agendas

		// Task 2 (bucket 120000) should NOT execute yet since 80000 < 110001
		assert_eq!(logger::log().len(), 1);
		assert_eq!(logger::log()[0].1, 1);

		// Jump way past bucket 120000's start time
		let future_time_ms = 300_000;
		MockTimestamp::set_timestamp(future_time_ms);
		// Process the block at the future time.
		run_to_block(5); // This will trigger on_initialize which processes timestamp agendas

		// Now task 2 should execute
		assert_eq!(logger::log().len(), 2);
		assert_eq!(logger::log()[1].1, 2);
	});
}

#[test]
fn timestamp_scheduler_respects_weight_limits() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		// Start at timestamp 0
		MockTimestamp::set_timestamp(1);
		run_to_block(1);

		// Both tasks scheduled for After(15000) -> normalize(15000) = 20000, +bucket = 30000
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(15000),
			127,
			Box::new(call),
		));

		let call = RuntimeCall::Logger(LoggerCall::log { i: 69, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(15000),
			127,
			Box::new(call),
		));

		// Jump to timestamp 25000 - bucket 30000 starts at 20001, so this should process it
		MockTimestamp::set_timestamp(25000);
		run_to_block(2);

		// 69 and 42 do not fit together, so only one should execute
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		// The incomplete timestamp should be set to bucket 30000 since processing was incomplete
		assert_eq!(IncompleteTimestampSince::<Test>::get(), Some(30000));

		// Next block should process the remaining task from bucket 30000
		run_to_block(3);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 69u32)]);

		// After complete processing, IncompleteTimestampSince should be cleared
		assert_eq!(IncompleteTimestampSince::<Test>::get(), None);
	});
}

#[test]
fn timestamp_scheduler_removes_permanently_overweight_call() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		MockTimestamp::set_timestamp(1);
		run_to_block(1);

		// After(15000) -> normalize(15000) = 20000, +bucket = 30000
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight });
		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(15000),
			127,
			Box::new(call),
		));

		MockTimestamp::set_timestamp(25000);
		run_to_block(100);

		assert_eq!(logger::log(), vec![]);

		assert!(System::events().iter().any(|e| e.event ==
			crate::Event::PermanentlyOverweight {
				task: (BlockNumberOrTimestamp::Timestamp(30000), 0),
				id: None,
			}
			.into()));
		assert_eq!(Agenda::<Test>::iter().count(), 0);
	});
}

#[test]
fn timestamp_on_initialize_weight_is_correct() {
	new_test_ext().execute_with(|| {
		let call_weight = Weight::from_parts(25, 0);

		// === TASK DEFINITIONS ===
		// All tasks scheduled at timestamp 100
		MockTimestamp::set_timestamp(100);
		run_to_block(1);

		// Task A: After(5000) -> normalize(5000) = 10000, +bucket = 20000
		let call_a = RuntimeCall::Logger(LoggerCall::log {
			i: 2600,
			weight: call_weight + Weight::from_parts(4, 0),
		});
		assert_ok!(Scheduler::schedule_named_after(
			RuntimeOrigin::root(),
			[2u8; 32],
			BlockNumberOrTimestamp::Timestamp(5000),
			126,
			Box::new(call_a),
		));

		// Task B: After(15000) -> normalize(15000) = 20000, +bucket = 30000
		let call_b = RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: call_weight + Weight::from_parts(3, 0),
		});
		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(15000),
			127,
			Box::new(call_b),
		));

		// Task C: After(15000) -> normalize(15000) = 20000, +bucket = 30000
		let call_c = RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: call_weight + Weight::from_parts(2, 0),
		});
		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(15000),
			128,
			Box::new(call_c),
		));

		// Task D: After(25000) -> normalize(25000) = 30000, +bucket = 40000
		let call_d = RuntimeCall::Logger(LoggerCall::log {
			i: 3,
			weight: call_weight + Weight::from_parts(1, 0),
		});
		assert_ok!(Scheduler::schedule_named_after(
			RuntimeOrigin::root(),
			[1u8; 32],
			BlockNumberOrTimestamp::Timestamp(25000),
			255,
			Box::new(call_d),
		));

		// === EXPECTED BEHAVIOR ===
		// Task A in bucket 20000 (starts at 10001)
		// Tasks B, C in bucket 30000 (starts at 20001)
		// Task D in bucket 40000 (starts at 30001)

		// Block 1 at timestamp 15000:
		//   - Process buckets up to 20000 (bucket 20000 starts at 10001)
		//   - Execute: Task A (2600) from bucket 20000

		// Block 2 at timestamp 25000:
		//   - Process buckets up to 30000 (bucket 30000 starts at 20001)
		//   - Execute: Tasks B (69) and C (42) from bucket 30000

		// Block 3 at timestamp 35000:
		//   - Process buckets up to 40000 (bucket 40000 starts at 30001)
		//   - Execute: Task D (3) from bucket 40000

		// === EXECUTION AND VERIFICATION ===

		println!("=== Block 1: Jump to timestamp 15000 ===");
		MockTimestamp::set_timestamp(15000);
		let weight_1 = Scheduler::on_initialize(1);

		println!("Executed tasks: {:?}", logger::log());
		println!("IncompleteTimestampSince: {:?}", IncompleteTimestampSince::<Test>::get());
		println!("LastProcessedTimestamp: {:?}", LastProcessedTimestamp::<Test>::get());

		// Verify Task A from bucket 20000 executed
		let logs_after_block1 = logger::log();
		assert_eq!(logs_after_block1.len(), 1);
		assert!(logs_after_block1.contains(&(root(), 2600u32))); // Task A
		assert_eq!(IncompleteTimestampSince::<Test>::get(), None);

		println!("\n=== Block 2: Jump to timestamp 25000 ===");
		MockTimestamp::set_timestamp(25000);
		let weight_2 = Scheduler::on_initialize(2);

		println!("Executed tasks: {:?}", logger::log());
		println!("IncompleteTimestampSince: {:?}", IncompleteTimestampSince::<Test>::get());
		println!("LastProcessedTimestamp: {:?}", LastProcessedTimestamp::<Test>::get());

		// Verify Tasks B and C from bucket 30000 executed
		let logs_after_block2 = logger::log();
		assert_eq!(logs_after_block2.len(), 3);
		assert!(logs_after_block2.contains(&(root(), 69u32))); // Task B
		assert!(logs_after_block2.contains(&(root(), 42u32))); // Task C
		assert_eq!(IncompleteTimestampSince::<Test>::get(), None);

		println!("\n=== Block 3: Jump to timestamp 35000 ===");
		MockTimestamp::set_timestamp(35000);
		let weight_3 = Scheduler::on_initialize(3);

		println!("Executed tasks: {:?}", logger::log());
		println!("IncompleteTimestampSince: {:?}", IncompleteTimestampSince::<Test>::get());
		println!("LastProcessedTimestamp: {:?}", LastProcessedTimestamp::<Test>::get());

		// Verify Task D from bucket 40000 executed
		let logs_after_block3 = logger::log();
		assert_eq!(logs_after_block3.len(), 4);
		assert!(logs_after_block3.contains(&(root(), 3u32))); // Task D
		assert_eq!(IncompleteTimestampSince::<Test>::get(), None);

		// Verify weights are reasonable
		assert!(weight_1.ref_time() > 0);
		assert!(weight_2.ref_time() > 0);
		assert!(weight_3.ref_time() > 0);
	});
}

#[test]
fn timestamp_incomplete_processing_across_multiple_buckets() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		// === TASK DEFINITIONS ===
		// Start at timestamp 0
		MockTimestamp::set_timestamp(1);
		run_to_block(1);

		println!("=== SETUP ===");
		println!("Max weight: {:?}", max_weight);
		println!("Task weight (1/4 max): {:?}", max_weight / 4);
		println!("Service agendas base weight: {:?}", TestWeightInfo::service_agendas_base());
		println!("Service agenda base weight: {:?}", TestWeightInfo::service_agenda_base(5));

		// Schedule 5 heavy tasks across different buckets
		// Each task takes 1/4 of max weight, so theoretically 4 should fit
		// But there's overhead, so let's see what actually happens
		// With the fix: After(X) -> normalize(X) + bucket_size
		// So After(15000) -> 30000, After(25000) -> 40000, etc.
		for i in 0..5 {
			let call = RuntimeCall::Logger(LoggerCall::log {
				i: i + 100,
				weight: max_weight / 4, // Each task takes 1/4 of max weight
			});
			assert_ok!(Scheduler::schedule_after(
				RuntimeOrigin::root(),
				BlockNumberOrTimestamp::Timestamp(15000 + (i as u64) * 10000), /* Buckets 30000,
				                                                                * 40000, 50000,
				                                                                * 60000, 70000 */
				127,
				Box::new(call),
			));
			println!("Scheduled task {} in bucket {}", i + 100, 30000 + i * 10000);
		}

		// === EXPECTED BEHAVIOR ===
		// Jump to timestamp 75000 (bucket 80000) - should process all buckets 30000-70000
		// Weight limits should cause tasks to be spread across multiple blocks
		// Why not 4 tasks in first block? Because of overhead:
		// - service_agendas_base() weight
		// - service_agenda_base() weight for each bucket processed
		// - dispatch overhead for each task
		// - storage read/write overhead

		println!("\n=== EXECUTION ===");
		MockTimestamp::set_timestamp(75000);

		let mut total_executed = 0;
		let mut block = 1;

		// Process blocks until all tasks are executed
		while total_executed < 5 && block < 10 {
			let logs_before = logger::log().len();
			let incomplete_before = IncompleteTimestampSince::<Test>::get();

			println!("\n--- Block {} ---", block);
			println!(
				"Before: {} tasks executed, IncompleteTimestampSince: {:?}",
				logs_before, incomplete_before
			);

			let consumed_weight = Scheduler::on_initialize(block);

			let logs_after = logger::log().len();
			let incomplete_after = IncompleteTimestampSince::<Test>::get();
			let tasks_executed_this_block = logs_after - logs_before;
			total_executed = logs_after;

			println!(
				"After: {} tasks executed this block, total: {}",
				tasks_executed_this_block, total_executed
			);
			println!("Weight consumed: {:?}", consumed_weight);
			println!("IncompleteTimestampSince: {:?}", incomplete_after);

			// Add assertions based on what we expect
			if block == 1 {
				// First block: should execute some tasks but hit weight limit
				assert!(
					tasks_executed_this_block > 0,
					"First block should execute at least one task"
				);
				assert!(
					tasks_executed_this_block < 5,
					"First block shouldn't execute all tasks due to weight limits"
				);

				if tasks_executed_this_block < 5 {
					assert!(
						incomplete_after.is_some(),
						"Should have incomplete timestamp since if not all tasks executed"
					);
				}

				// Based on the output, we expect 3 tasks in first block
				assert_eq!(
					tasks_executed_this_block, 3,
					"Expected 3 tasks in first block based on weight limits"
				);
				assert_eq!(incomplete_after, Some(60000), "Should be incomplete at bucket 60000");
			} else if block == 2 {
				// Second block: should complete remaining tasks
				assert_eq!(
					tasks_executed_this_block, 2,
					"Expected 2 remaining tasks in second block"
				);
				assert_eq!(total_executed, 5, "Should have executed all 5 tasks by second block");
				assert_eq!(
					incomplete_after, None,
					"Should have no incomplete timestamp after all tasks executed"
				);
			}

			// Weight should never exceed maximum
			assert!(
				consumed_weight.ref_time() <= max_weight.ref_time(),
				"Consumed weight should not exceed maximum"
			);

			block += 1;
		}

		// === FINAL VERIFICATION ===
		println!("\n=== FINAL VERIFICATION ===");

		// All tasks should eventually execute
		assert_eq!(total_executed, 5, "All 5 tasks should eventually execute");
		assert!(
			block <= 3,
			"Should complete within 2 blocks (block counter is 3 after 2 executions)"
		);

		// After all processing is complete, IncompleteTimestampSince should be None
		assert_eq!(
			IncompleteTimestampSince::<Test>::get(),
			None,
			"No incomplete processing should remain"
		);

		// Verify all expected values were logged
		let logs = logger::log();
		let mut expected_values: Vec<u32> = (100..105).collect();
		expected_values.sort();
		let mut actual_values: Vec<u32> = logs.iter().map(|(_, v)| *v).collect();
		actual_values.sort();
		assert_eq!(actual_values, expected_values, "All expected task values should be logged");

		println!("✅ All tasks executed correctly across {} blocks", block - 1);
		println!("✅ Weight limits respected");
		println!("✅ Incomplete processing worked correctly");
	});
}

#[test]
fn last_processed_timestamp_updates_on_each_block() {
	new_test_ext().execute_with(|| {
		// In our mock runtime, the timestamp bucket size is 10_000ms.
		// The bucket for a given timestamp is calculated to be the next multiple
		// of the bucket size. For example, timestamps 0-9999ms fall into bucket 10000.

		// Block 1: Initial state check
		MockTimestamp::set_timestamp(0);
		assert_eq!(LastProcessedTimestamp::<Test>::get(), None, "Should be None initially");
		MockTimestamp::set_timestamp(10000);
		run_to_block(2);

		// `on_initialize` runs. Current timestamp is 0, so it processes up to bucket 10000.
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(20000),
			"Should be initialized to the first bucket"
		);

		// Block 2: Advance time into the next bucket
		MockTimestamp::set_timestamp(12000);
		run_to_block(3);
		// Current timestamp 12000 is in bucket 20000. `on_initialize` processes up to 20000.
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(20000),
			"Should advance to the new current bucket"
		);

		// Block 3: Advance time, but stay within the same bucket
		MockTimestamp::set_timestamp(18000);
		run_to_block(4);
		// Current timestamp 18000 is still in bucket 20000. No change expected.
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(20000),
			"Should not change when in the same bucket"
		);

		// Block 4: Time advances to a new bucket
		MockTimestamp::set_timestamp(25000);
		run_to_block(5);
		// Current timestamp 25000 is in bucket 30000.
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(30000),
			"Should advance to the next bucket"
		);

		// Block 5: Time jumps several buckets ahead
		MockTimestamp::set_timestamp(105000);
		run_to_block(6);
		// Current timestamp 105000 is in bucket 110000.
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(110000),
			"Should jump to the correct bucket after a large time skip"
		);

		// Block 6: No time change, LastProcessedTimestamp should not change.
		// `run_to_block` advances the block number, but the timestamp remains 105000.
		run_to_block(7);
		// Current timestamp is still 105000 (bucket 110000).
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(110000),
			"Should not change if timestamp does not change"
		);
	});
}

#[test]
fn last_processed_timestamp_initialization_and_update_works() {
	new_test_ext().execute_with(|| {
		// Timestamp is 0, so service_timestamp_agendas should not run.
		run_to_block(1);
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			None,
			"Should not be initialized at timestamp 0"
		);

		// Run a subsequent block with a realistic timestamp
		MockTimestamp::set_timestamp(123_000);
		run_to_block(2);
		let normalized_time = 130_000; // 123_000 rounded down to bucket size of 10_000
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(normalized_time),
			"Should be initialized to the current normalized time on first run"
		);

		// Run another block, it should advance the timestamp
		MockTimestamp::set_timestamp(135_000);
		run_to_block(3);
		let normalized_time_2 = 140_000;
		assert_eq!(
			LastProcessedTimestamp::<Test>::get(),
			Some(normalized_time_2),
			"Should advance to the new normalized time"
		);
	});
}

/// When the first task consumes nearly all weight, remaining tasks must be preserved
/// in the agenda for processing in a subsequent block.
#[test]
fn weight_exhaustion_preserves_remaining_tasks() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		// Overhead consumed before the second task's base weight check:
		//   service_agendas_base (1) + service_agenda_base(2) (514) +
		//   service_task_base (4) + execute_dispatch_unsigned (128) + call1_weight
		// = 647 + call1_weight
		//
		// For the second task's can_consume(service_task_base=4) to fail:
		//   647 + call1_weight + 4 > max_weight.ref_time()
		//   call1_weight > max_weight.ref_time() - 651
		//
		// For the first task's execute_dispatch to succeed:
		//   519 + 128 + call1_weight <= max_weight.ref_time()
		//   call1_weight <= max_weight.ref_time() - 647
		let call1_weight = Weight::from_parts(max_weight.ref_time() - 648, 0);
		let call1 = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: call1_weight });
		let call2 =
			RuntimeCall::Logger(LoggerCall::log { i: 69, weight: Weight::from_parts(10, 0) });

		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call1).unwrap(),
		));
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			128,
			root(),
			Preimage::bound(call2).unwrap(),
		));

		assert_eq!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4)).len(), 2);

		run_to_block(4);
		// First task executes.
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		// The second task MUST still be in the agenda (postponed, not lost).
		let agenda = Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4));
		assert!(
			agenda.iter().any(|s| s.is_some()),
			"Second task must remain in the agenda after weight exhaustion"
		);

		// The second task should execute in the next block.
		run_to_block(5);
		assert_eq!(
			logger::log(),
			vec![(root(), 42u32), (root(), 69u32)],
			"Second task should execute in the following block"
		);
	});
}

/// Cancelling a named task without an explicit origin (as trait callers do)
/// must still clean up retries and preimage references.
#[test]
fn cancel_named_without_origin_cleans_up_retries() {
	new_test_ext().execute_with(|| {
		Threshold::<Test>::put((99, 100));

		let call =
			RuntimeCall::Logger(LoggerCall::timed_log { i: 42, weight: Weight::from_parts(10, 0) });
		assert_ok!(Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));

		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[1u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(3)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);

		// Cancel via the trait path (origin = None), as other pallets would.
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));

		// The task slot should be cleared.
		assert!(
			Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(4))
				.iter()
				.all(|s| s.is_none()),
			"Task slot should be None after cancel"
		);

		// Retries should also be cleaned up.
		assert_eq!(
			Retries::<Test>::iter().count(),
			0,
			"Retries must be cleaned up after cancel with origin=None"
		);
	});
}

/// A retry whose period type mismatches the task domain (e.g. Timestamp retry period on a
/// BlockNumber task) emits RetryFailed rather than silently dropping the retry.
#[test]
fn mismatched_retry_period_emits_failure_event() {
	new_test_ext().execute_with(|| {
		Threshold::<Test>::put((99, 100));

		let call =
			RuntimeCall::Logger(LoggerCall::timed_log { i: 42, weight: Weight::from_parts(10, 0) });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));

		// Set a Timestamp-based retry period on a BlockNumber-based task. This is a type mismatch.
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::Timestamp(3000u64)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);

		// Task fails at block 4 (threshold not met). The retry should fire, but the
		// mismatched period means schedule_retry's saturating_add returns Err.
		run_to_block(4);

		// The retry config was consumed but no retry was actually scheduled.
		assert_eq!(
			Agenda::<Test>::iter().count(),
			0,
			"No retry should be scheduled when period type mismatches"
		);

		// There should be a RetryFailed event, but there won't be one because the code
		// silently returns without emitting any event when saturating_add fails.
		let events = System::events();
		let has_retry_failed = events
			.iter()
			.any(|e| matches!(e.event, RuntimeEvent::Scheduler(crate::Event::RetryFailed { .. })));
		assert!(
			has_retry_failed,
			"RetryFailed event must be emitted when retry period type mismatches"
		);
	});
}

/// A permanently overweight task is removed from the agenda after emitting
/// PermanentlyOverweight, and does not linger or emit further events.
#[test]
fn permanently_overweight_task_is_removed() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));

		run_to_block(4);
		assert_eq!(logger::log(), vec![]);

		// Task is permanently overweight — it should be removed after the event.
		let overweight_events_after_4 = System::events()
			.iter()
			.filter(|e| {
				matches!(
					e.event,
					RuntimeEvent::Scheduler(crate::Event::PermanentlyOverweight { .. })
				)
			})
			.count();
		assert_eq!(overweight_events_after_4, 1);

		// Run more blocks. The task should NOT keep emitting events.
		run_to_block(7);

		let unavailable_events = System::events()
			.iter()
			.filter(|e| {
				matches!(e.event, RuntimeEvent::Scheduler(crate::Event::CallUnavailable { .. }))
			})
			.count();
		assert_eq!(
			unavailable_events, 0,
			"No CallUnavailable events after PermanentlyOverweight removal"
		);

		// The agenda should be clean.
		assert_eq!(
			Agenda::<Test>::iter().count(),
			0,
			"Agenda must be empty after permanently overweight task is removed"
		);
	});
}

#[test]
fn reschedule_preserves_retry_config() {
	new_test_ext().execute_with(|| {
		// Task fails until block 20 is reached
		Threshold::<Test>::put((20, 100));
		// Schedule task at block 4
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		// Set retry config: 10 retries every 3 blocks
		assert_ok!(Scheduler::set_retry(
			root().into(),
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			10,
			BlockNumberOrTimestamp::BlockNumber(3)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(Retries::<Test>::contains_key((BlockNumberOrTimestamp::BlockNumber(4), 0)));

		// Reschedule to block 8
		let new_address = Scheduler::do_reschedule(
			(BlockNumberOrTimestamp::BlockNumber(4), 0),
			DispatchTime::At(8),
		)
		.unwrap();
		assert_eq!(new_address, (BlockNumberOrTimestamp::BlockNumber(8), 0));

		// Retry config should be transferred to the new address
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(!Retries::<Test>::contains_key((BlockNumberOrTimestamp::BlockNumber(4), 0)));
		assert!(Retries::<Test>::contains_key(new_address));

		let retry_config = Retries::<Test>::get(new_address).unwrap();
		assert_eq!(retry_config.total_retries, 10);
		assert_eq!(retry_config.remaining, 10);
		assert_eq!(retry_config.period, BlockNumberOrTimestamp::BlockNumber(3));

		// Run to block 8 - task should fail and be retried at block 11
		run_to_block(8);
		assert!(logger::log().is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(11))[0].is_some());

		// Retry config should now be at the retry address
		assert_eq!(Retries::<Test>::iter().count(), 1);

		// Run to block 20 - task should eventually succeed
		run_to_block(20);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		assert_eq!(Retries::<Test>::iter().count(), 0);
	});
}

#[test]
fn reschedule_named_preserves_retry_config() {
	new_test_ext().execute_with(|| {
		// Task fails until block 20 is reached
		Threshold::<Test>::put((20, 100));
		// Schedule named task at block 4
		assert_ok!(Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::timed_log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		// Set retry config: 10 retries every 3 blocks
		assert_ok!(Scheduler::set_retry_named(
			root().into(),
			[1u8; 32],
			10,
			BlockNumberOrTimestamp::BlockNumber(3)
		));
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(Retries::<Test>::contains_key((BlockNumberOrTimestamp::BlockNumber(4), 0)));

		// Reschedule to block 8
		let new_address = Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(8)).unwrap();
		assert_eq!(new_address, (BlockNumberOrTimestamp::BlockNumber(8), 0));

		// Retry config should be transferred to the new address
		assert_eq!(Retries::<Test>::iter().count(), 1);
		assert!(!Retries::<Test>::contains_key((BlockNumberOrTimestamp::BlockNumber(4), 0)));
		assert!(Retries::<Test>::contains_key(new_address));

		let retry_config = Retries::<Test>::get(new_address).unwrap();
		assert_eq!(retry_config.total_retries, 10);
		assert_eq!(retry_config.remaining, 10);
		assert_eq!(retry_config.period, BlockNumberOrTimestamp::BlockNumber(3));

		// Run to block 8 - task should fail and be retried at block 11
		run_to_block(8);
		assert!(logger::log().is_empty());
		assert!(Agenda::<Test>::get(BlockNumberOrTimestamp::BlockNumber(11))[0].is_some());

		// Retry config should now be at the retry address
		assert_eq!(Retries::<Test>::iter().count(), 1);

		// Run to block 20 - task should eventually succeed
		run_to_block(20);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		assert_eq!(Retries::<Test>::iter().count(), 0);
	});
}
#[test]
fn timestamp_task_not_permanently_overweight_after_block_tasks_execute() {
	// This test verifies that a timestamp task that doesn't fit in the current block
	// (because block-based tasks consumed weight) is NOT incorrectly marked as
	// permanently overweight. It should be kept and executed in the next block.
	//
	// The bug: block and timestamp agendas each have separate `executed` counters.
	// The `is_first` flag (used to determine if an overweight task is permanently
	// overweight) is based on `executed == 0`. When timestamp processing starts,
	// `executed` resets to 0 even if block tasks already ran, causing the first
	// timestamp task to be incorrectly marked as permanently overweight.
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		// Initialize timestamp
		MockTimestamp::set_timestamp(1);
		run_to_block(1);

		// Schedule a block task that takes ~67% of max scheduler weight - will execute successfully
		let block_weight = max_weight / 3 * 2;
		let block_call = RuntimeCall::Logger(LoggerCall::log { i: 1, weight: block_weight });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			127,
			root(),
			Preimage::bound(block_call).unwrap()
		));

		// Schedule a timestamp task that takes 50% of max scheduler weight
		// This would fit in a fresh block, but not after the block task runs (67% + 50% > 100%)
		// After(35000) -> normalize(35000) = 40000, +bucket = 50000
		let timestamp_weight = max_weight / 2;
		let timestamp_call =
			RuntimeCall::Logger(LoggerCall::log { i: 2, weight: timestamp_weight });
		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(35000), // Will be scheduled for bucket 50000
			127,
			Box::new(timestamp_call),
		));

		// Verify agenda scheduled correctly
		assert!(
			Agenda::<Test>::get(BlockNumberOrTimestamp::Timestamp(50000))
				.iter()
				.any(|s| s.is_some()),
			"Timestamp task should be scheduled in bucket 50000"
		);

		// Advance to block 4 without processing timestamp agenda yet
		// (timestamp stays at 1, which normalizes to bucket 10000, before our task at 50000)
		run_to_block(3);

		// Now at block 4, set timestamp to trigger both block and timestamp task in same block
		// Bucket 50000 starts at 40001
		MockTimestamp::set_timestamp(45000);
		<Scheduler as OnFinalize<u64>>::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		<Scheduler as OnInitialize<u64>>::on_initialize(System::block_number()); // This is block 4

		// Block task should have executed
		assert!(logger::log().iter().any(|&(_, i)| i == 1), "Block task should have executed");

		// The timestamp task should NOT be permanently overweight - it should still be
		// in the agenda waiting to execute in the next block
		let permanently_overweight_count = System::events()
			.iter()
			.filter(|e| {
				matches!(
					e.event,
					RuntimeEvent::Scheduler(crate::Event::PermanentlyOverweight { .. })
				)
			})
			.count();

		assert_eq!(
			permanently_overweight_count, 0,
			"Timestamp task should NOT be marked as permanently overweight"
		);

		// The timestamp task should still be in the agenda (IncompleteTimestampSince should be set)
		let incomplete_ts = IncompleteTimestampSince::<Test>::get();
		assert!(
			incomplete_ts.is_some() ||
				Agenda::<Test>::get(BlockNumberOrTimestamp::Timestamp(50000))
					.iter()
					.any(|s| s.is_some()),
			"Timestamp task should still be pending (either in agenda or incomplete)"
		);

		// Run another block - the timestamp task should now execute
		run_to_block(5);
		assert!(
			logger::log().iter().any(|&(_, i)| i == 2),
			"Timestamp task should execute in subsequent block"
		);
	});
}

#[test]
fn timestamp_task_does_not_fire_before_target_time() {
	// This test verifies that a task scheduled with DispatchTime::After(Timestamp(X))
	// does NOT execute before time X.
	//
	// The fix: Schedule tasks in the next bucket after normalization, guaranteeing
	// the bucket's start time is > target time.
	//
	// Example with bucket_size = 10000:
	// - Schedule task "after 35000ms"
	// - normalize(35000) = 40000, then +bucket = 50000
	// - Bucket 50000 starts at time 40001, which is > 35000 ✓
	new_test_ext().execute_with(|| {
		// Initialize at timestamp 1
		MockTimestamp::set_timestamp(1);
		run_to_block(1);

		// Schedule a task for "after 35000ms"
		// With bucket_size = 10000: normalize(35000) = 40000, +bucket = 50000
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert_ok!(Scheduler::schedule_after(
			RuntimeOrigin::root(),
			BlockNumberOrTimestamp::Timestamp(35000),
			127,
			Box::new(call),
		));

		// Verify task is scheduled in bucket 50000 (not 40000)
		assert!(
			Agenda::<Test>::get(BlockNumberOrTimestamp::Timestamp(50000))
				.iter()
				.any(|s| s.is_some()),
			"Task should be scheduled in bucket 50000"
		);

		// Advance to timestamp 36000 - this is AFTER target 35000 but bucket 50000 hasn't started
		// Task should NOT execute yet
		MockTimestamp::set_timestamp(36000);
		<Scheduler as OnFinalize<u64>>::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		<Scheduler as OnInitialize<u64>>::on_initialize(System::block_number());

		// Task should NOT have executed - bucket 50000 starts at 40001
		assert!(
			logger::log().is_empty(),
			"Task should NOT execute before bucket 50000 is reached (36000 < 40001)"
		);

		// Task should still be in the agenda
		assert!(
			Agenda::<Test>::get(BlockNumberOrTimestamp::Timestamp(50000))
				.iter()
				.any(|s| s.is_some()),
			"Task should still be in agenda"
		);

		// Now advance to timestamp 41000 - bucket 50000 has started (40001+)
		MockTimestamp::set_timestamp(41000);
		<Scheduler as OnFinalize<u64>>::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		<Scheduler as OnInitialize<u64>>::on_initialize(System::block_number());

		// NOW the task should execute
		assert_eq!(
			logger::log(),
			vec![(root(), 42u32)],
			"Task should execute once bucket 50000 is reached (41000 >= 40001)"
		);
	});
}
