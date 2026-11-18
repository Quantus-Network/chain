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

//! Tests for the Timestamp module.

use crate::mock::*;
use frame_support::assert_ok;

#[test]
fn timestamp_works() {
	new_test_ext().execute_with(|| {
		crate::Now::<Test>::put(46);
		assert_ok!(Timestamp::set(RuntimeOrigin::none(), 69));
		assert_eq!(crate::Now::<Test>::get(), 69);
		assert_eq!(Some(69), get_captured_moment());
	});
}

#[docify::export]
#[test]
#[should_panic(expected = "Timestamp must be updated only once in the block")]
fn double_timestamp_should_fail() {
	new_test_ext().execute_with(|| {
		Timestamp::set_timestamp(42);
		assert_ok!(Timestamp::set(RuntimeOrigin::none(), 69));
	});
}

#[docify::export]
#[test]
#[should_panic(
	expected = "Timestamp must increment by at least <MinimumPeriod> between sequential blocks"
)]
fn block_period_minimum_enforced() {
	new_test_ext().execute_with(|| {
		crate::Now::<Test>::put(44);
		let _ = Timestamp::set(RuntimeOrigin::none(), 46);
	});
}

/// The inherent check for a block stamped `block_time`, on a node whose clock reads `clock`.
fn check_inherent_at(clock: u64, block_time: u64) -> Result<(), sp_timestamp::InherentError> {
	use frame_support::inherent::ProvideInherent;

	let mut data = sp_inherents::InherentData::new();
	data.put_data(sp_timestamp::INHERENT_IDENTIFIER, &sp_timestamp::Timestamp::new(clock))
		.expect("the timestamp is put only once");
	Timestamp::check_inherent(&crate::Call::set { now: block_time }, &data)
}

#[test]
fn block_time_is_accepted_up_to_the_drift_ahead_of_the_local_clock() {
	new_test_ext().execute_with(|| {
		const CLOCK: u64 = 1_000_000;
		const DRIFT: u64 = 15_000;

		assert!(check_inherent_at(CLOCK, CLOCK + DRIFT).is_ok());
		assert!(matches!(
			check_inherent_at(CLOCK, CLOCK + DRIFT + 1),
			Err(sp_timestamp::InherentError::TooFarInFuture)
		));
		// The rejected block passes on the same node one millisecond later.
		assert!(check_inherent_at(CLOCK + 1, CLOCK + DRIFT + 1).is_ok());
	});
}
