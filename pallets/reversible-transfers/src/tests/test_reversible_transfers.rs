#![cfg(test)]

use crate::tests::mock::*; // Import mock runtime and types
use crate::*; // Import items from parent module (lib.rs)
use frame_support::{
	assert_err, assert_ok,
	traits::{fungible::InspectHold, StorePreimage, Time},
};
use pallet_scheduler::Agenda;
use qp_scheduler::BlockNumberOrTimestamp;
use sp_core::H256;
use sp_runtime::traits::{BadOrigin, BlakeTwo256, Hash};

// Helper function to create a transfer call
pub(crate) fn transfer_call(dest: AccountId, amount: Balance) -> RuntimeCall {
	RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest, value: amount })
}

// Helper: approximate equality for balances to tolerate fee deductions
fn approx_eq_balance(a: Balance, b: Balance, epsilon: Balance) -> bool {
	if a >= b {
		a - b <= epsilon
	} else {
		b - a <= epsilon
	}
}

// Helper function to calculate TxId (matching the logic in schedule_transfer)
pub(crate) fn calculate_tx_id<T: Config>(who: AccountId, call: &RuntimeCall) -> H256 {
	let global_nonce = GlobalNonce::<T>::get();
	BlakeTwo256::hash_of(&(who, call, global_nonce).encode())
}

// Helper to run to the next block
fn run_to_block(n: u64) {
	while System::block_number() < n {
		// Finalize previous block
		Scheduler::on_finalize(System::block_number());
		System::finalize();
		// Set next block number
		System::set_block_number(System::block_number() + 1);
		// Initialize next block
		System::on_initialize(System::block_number());
		Scheduler::on_initialize(System::block_number());
	}
}

#[test]
fn set_high_security_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let genesis_user = 1;

		// Check initial state
		assert_eq!(
			ReversibleTransfers::is_high_security(&genesis_user),
			Some(HighSecurityAccountData {
				delay: BlockNumberOrTimestampOf::<Test>::BlockNumber(10),
				interceptor: 2,
			})
		);

		// Set the delay
		let another_user = 4;
		let interceptor = 5;
		let delay = BlockNumberOrTimestampOf::<Test>::BlockNumber(5);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(another_user),
			delay,
			interceptor,
		));
		assert_eq!(
			ReversibleTransfers::is_high_security(&another_user),
			Some(HighSecurityAccountData { delay, interceptor })
		);
		System::assert_last_event(
			Event::HighSecuritySet { who: another_user, interceptor, delay }.into(),
		);

		// Calling this again should err
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(another_user),
				delay,
				interceptor,
			),
			Error::<Test>::AccountAlreadyHighSecurity
		);

		// Use default delay
		let default_user = 7;
		let default_interceptor = 8;
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(default_user),
			DefaultDelay::get(),
			default_interceptor,
		));
		assert_eq!(
			ReversibleTransfers::is_high_security(&default_user),
			Some(HighSecurityAccountData {
				delay: DefaultDelay::get(),
				interceptor: default_interceptor,
			})
		);
		System::assert_last_event(
			Event::HighSecuritySet {
				who: default_user,
				interceptor: default_interceptor,
				delay: DefaultDelay::get(),
			}
			.into(),
		);

		// Too short delay
		let _short_delay: BlockNumberOrTimestampOf<Test> =
			BlockNumberOrTimestamp::BlockNumber(MinDelayPeriodBlocks::get() - 1);

		let new_user = 10;
		let new_interceptor = 11;
		let short_delay = BlockNumberOrTimestampOf::<Test>::BlockNumber(1);
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(new_user),
				short_delay,
				new_interceptor,
			),
			Error::<Test>::DelayTooShort
		);

		// Explicit reverse can not be self
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(new_user),
				delay,
				new_user,
			),
			Error::<Test>::InterceptorCannotBeSelf
		);

		assert_eq!(ReversibleTransfers::is_high_security(&new_user), None);

		// Use explicit reverser
		let reversible_account = 6;
		let interceptor = 7;
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			delay,
			interceptor,
		));
		assert_eq!(
			ReversibleTransfers::is_high_security(&reversible_account),
			Some(HighSecurityAccountData { delay, interceptor })
		);
	});
}

#[test]
fn set_reversibility_with_timestamp_delay_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1); // Block number still relevant for system events, etc.
		MockTimestamp::<Test>::set_timestamp(1_000_000); // Initial mock time

		let user = 4;
		// Assuming MinDelayPeriod allows for timestamp delays of this magnitude.
		// e.g., MinDelayPeriod is Timestamp(1000) and TimestampBucketSize is 1000.
		// A delay of 5 * TimestampBucketSize = 5000.
		let delay = BlockNumberOrTimestamp::Timestamp(5 * TimestampBucketSize::get());

		let interceptor = 16;
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user),
			delay,
			interceptor,
		));
		assert_eq!(
			ReversibleTransfers::is_high_security(&user),
			Some(HighSecurityAccountData { delay, interceptor })
		);
		System::assert_last_event(Event::HighSecuritySet { who: user, interceptor, delay }.into());

		// Too short timestamp delay
		// This requires MinDelayPeriodTimestamp to be set and > 0 for this check to be meaningful
		// and not panic due to Ord issues if MinDelayPeriod is BlockNumber.
		// Assuming MinDelayPeriodTimestamp is, say, 2 * TimestampBucketSize::get().
		let short_delay_ts = BlockNumberOrTimestamp::Timestamp(TimestampBucketSize::get());
		let another_user = 5;

		let another_interceptor = 18;
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(another_user),
				short_delay_ts,
				another_interceptor,
			),
			Error::<Test>::DelayTooShort
		);
	});
}

#[test]
fn set_reversibility_fails_delay_too_short() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = 20;
		let interceptor = 21;
		let short_delay = BlockNumberOrTimestampOf::<Test>::BlockNumber(1);
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(user),
				short_delay,
				interceptor,
			),
			Error::<Test>::DelayTooShort
		);
		assert_eq!(ReversibleTransfers::is_high_security(&user), None);
	});
}

#[test]
fn schedule_transfer_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = 1; // Reversible from genesis
		let dest_user = 2;
		let amount = 100;
		let dest_user_balance = Balances::free_balance(dest_user);
		let user_balance = Balances::free_balance(user);

		let call = transfer_call(dest_user, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);
		let HighSecurityAccountData { delay: user_delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let expected_block = System::block_number() + user_delay.as_block_number().unwrap();
		let bounded = Preimage::bound(call.clone()).unwrap();
		let expected_block = BlockNumberOrTimestamp::BlockNumber(expected_block);

		assert!(Agenda::<Test>::get(expected_block).len() == 0);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest_user,
			amount,
		));

		// Check storage
		assert_eq!(
			PendingTransfers::<Test>::get(tx_id).unwrap(),
			PendingTransfer {
				from: user,
				to: dest_user,
				interceptor: 2, // From genesis config
				call: bounded,
				amount,
			}
		);
		assert_eq!(ReversibleTransfers::account_pending_index(user), 1);

		// Check scheduler
		assert!(Agenda::<Test>::get(expected_block).len() > 0);

		// Skip to the delay block
		run_to_block(expected_block.as_block_number().unwrap());

		// Check that the transfer is executed
		let eps: Balance = 10; // tolerate tiny fee differences
		assert!(approx_eq_balance(Balances::free_balance(user), user_balance - amount, eps));
		assert_eq!(Balances::free_balance(dest_user), dest_user_balance + amount);

		// Use explicit reverser
		let reversible_account = 255;
		let interceptor = user;

		// Set reversibility
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			BlockNumberOrTimestamp::BlockNumber(10),
			interceptor,
		));

		let tx_id = calculate_tx_id::<Test>(reversible_account, &call);
		// Schedule transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(reversible_account),
			dest_user,
			amount,
		));

		// Try reversing with original user
		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(reversible_account), tx_id,),
			Error::<Test>::InvalidReverser
		);

		let interceptor_balance = Balances::free_balance(interceptor);
		let reversible_account_balance = Balances::free_balance(reversible_account);
		let interceptor_hold = Balances::balance_on_hold(
			&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
			&interceptor,
		);
		assert_eq!(interceptor_hold, 0);

		// Try reversing with explicit reverser
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(interceptor), tx_id,));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());

		// Funds should be release as free balance to `interceptor`
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&reversible_account
			),
			0
		);

		assert_eq!(Balances::free_balance(interceptor), interceptor_balance + amount);

		// Unchanged balance for `reversible_account`
		assert_eq!(Balances::free_balance(reversible_account), reversible_account_balance);

		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&interceptor,
			),
			0
		);
	});
}

#[test]
fn schedule_transfer_with_timestamp_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = 255;
		let dest_user = 2;
		let amount = 100;
		let dest_user_balance = Balances::free_balance(dest_user);
		let user_balance = Balances::free_balance(user);

		let call = transfer_call(dest_user, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);

		// Set reversibility
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user),
			BlockNumberOrTimestamp::Timestamp(10_000),
			user + 100,
		));

		let timestamp_bucket_size = TimestampBucketSize::get();
		let current_time = MockTimestamp::<Test>::now();
		let HighSecurityAccountData { delay: user_delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let expected_raw_timestamp = (current_time / timestamp_bucket_size) * timestamp_bucket_size +
			user_delay.as_timestamp().unwrap();

		let bounded = Preimage::bound(call.clone()).unwrap();
		let expected_timestamp =
			BlockNumberOrTimestamp::Timestamp(expected_raw_timestamp + TimestampBucketSize::get());

		assert!(Agenda::<Test>::get(expected_timestamp).len() == 0);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest_user,
			amount,
		));

		// Check storage
		assert_eq!(
			PendingTransfers::<Test>::get(tx_id).unwrap(),
			PendingTransfer {
				from: user,
				to: dest_user,
				interceptor: user + 100, /* This should match the actual interceptor from the
				                          * test setup */
				call: bounded,
				amount,
			}
		);
		assert_eq!(ReversibleTransfers::account_pending_index(user), 1);

		// Check scheduler
		assert!(Agenda::<Test>::get(expected_timestamp).len() > 0);

		// Advance to expected execution time and ensure it executed
		MockTimestamp::<Test>::set_timestamp(expected_raw_timestamp);
		run_to_block(2);
		let eps: Balance = 10; // tolerate tiny fee differences
		assert!(approx_eq_balance(Balances::free_balance(user), user_balance - amount, eps));
		assert_eq!(Balances::free_balance(dest_user), dest_user_balance + amount);

		// Use explicit reverser
		let reversible_account = 256;
		let interceptor = 1;

		// Set reversibility
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			BlockNumberOrTimestamp::BlockNumber(10),
			interceptor,
		));

		let tx_id = calculate_tx_id::<Test>(reversible_account, &call);

		// Schedule transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(reversible_account),
			dest_user,
			amount,
		));

		// Try reversing with original user
		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(reversible_account), tx_id,),
			Error::<Test>::InvalidReverser
		);

		let interceptor_balance = Balances::free_balance(interceptor);
		let reversible_account_balance = Balances::free_balance(reversible_account);
		let interceptor_hold = Balances::balance_on_hold(
			&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
			&interceptor,
		);
		assert_eq!(interceptor_hold, 0);

		// Try reversing with explicit reverser
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(interceptor), tx_id,));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());

		// Funds should be release as free balance to `interceptor`
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&reversible_account
			),
			0
		);

		assert_eq!(Balances::free_balance(interceptor), interceptor_balance + amount);

		// Unchanged balance for `reversible_account`
		assert_eq!(Balances::free_balance(reversible_account), reversible_account_balance);

		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&interceptor,
			),
			0
		);
	});
}

#[test]
fn schedule_transfer_fails_not_reversible() {
	new_test_ext().execute_with(|| {
		let user = 2; // Not reversible

		assert_err!(
			ReversibleTransfers::schedule_transfer(RuntimeOrigin::signed(user), 3, 50),
			Error::<Test>::AccountNotHighSecurity
		);
	});
}

#[test]
fn schedule_multiple_transfer_works() {
	new_test_ext().execute_with(|| {
		let user = 1; // User 1 is reversible from genesis with interceptor=2, recoverer=3
		let dest_user = 2;
		let amount = 100;

		let tx_id = calculate_tx_id::<Test>(user, &transfer_call(dest_user, amount));

		// Schedule first
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest_user,
			amount,
		));

		// Try to schedule the same call again
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest_user,
			amount
		));

		// Check that the count of pending transactions for the user is 2
		assert_eq!(ReversibleTransfers::account_pending_index(user), 2);

		// Check that the pending transaction count decreases to 1
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(2), // interceptor from genesis config
			tx_id
		));
		assert_eq!(ReversibleTransfers::account_pending_index(user), 1);

		// Check that the pending transaction count decreases to 0 when executed
		let execute_block = System::block_number() + 10;
		run_to_block(execute_block);

		assert_eq!(ReversibleTransfers::account_pending_index(user), 0);
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
	});
}

#[test]
fn schedule_transfer_fails_too_many_pending() {
	new_test_ext().execute_with(|| {
		let user = 1;
		let max_pending = MaxReversibleTransfers::get();

		// Fill up pending slots
		for i in 0..max_pending {
			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(user),
				2,
				i as u128 + 1
			));
			// Max pending per block is 10, so we increment the block number
			// after every 10 calls
			if i % 10 == 9 {
				System::set_block_number(System::block_number() + 1);
			}
		}

		// Try to schedule one more
		assert_err!(
			ReversibleTransfers::schedule_transfer(RuntimeOrigin::signed(user), 3, 100),
			Error::<Test>::TooManyPendingTransactions
		);
	});
}

#[test]
fn cancel_dispatch_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = 1;
		let call = transfer_call(2, 50);
		let tx_id = calculate_tx_id::<Test>(user, &call);
		let HighSecurityAccountData { delay: user_delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let execute_block = BlockNumberOrTimestamp::BlockNumber(
			System::block_number() + user_delay.as_block_number().unwrap(),
		);

		assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

		// Schedule first
		assert_ok!(ReversibleTransfers::schedule_transfer(RuntimeOrigin::signed(user), 2, 50));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
		assert!(!ReversibleTransfers::account_pending_index(user).is_zero());

		// Check the expected block agendas count
		assert_eq!(Agenda::<Test>::get(execute_block).len(), 1);

		// Now cancel (must be called by interceptor, which is user 2 from genesis)
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(2), // interceptor from genesis config
			tx_id
		));

		// Check state cleared
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(user).is_zero());

		assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

		// Check event
		System::assert_last_event(Event::TransactionCancelled { who: 2, tx_id }.into());
	});
}

#[test]
fn cancel_dispatch_fails_not_owner() {
	new_test_ext().execute_with(|| {
		let owner = 1;
		let _attacker = 3;
		let call = transfer_call(2, 50);
		let tx_id = calculate_tx_id::<Test>(owner, &call);

		// Schedule as owner
		assert_ok!(ReversibleTransfers::schedule_transfer(RuntimeOrigin::signed(owner), 2, 50));

		// Attacker tries to cancel
		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(3), tx_id),
			Error::<Test>::InvalidReverser
		);

		// Check state not affected
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
	});
}

#[test]
fn cancel_dispatch_fails_not_found() {
	new_test_ext().execute_with(|| {
		let user = 1;
		let non_existent_tx_id = H256::random();

		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(user), non_existent_tx_id),
			Error::<Test>::PendingTxNotFound
		);
	});
}

#[test]
fn execute_transfer_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = 1; // Reversible, delay 10
		let dest = 2;
		let amount = 50;
		let call = transfer_call(dest, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);

		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let execute_block = BlockNumberOrTimestampOf::<Test>::BlockNumber(
			System::block_number() + delay.as_block_number().unwrap(),
		);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());

		run_to_block(execute_block.as_block_number().unwrap() - 1);

		// Execute the dispatch as a normal user. This should fail
		// because the origin should be `Signed(PalletId::into_account())`
		assert_err!(
			ReversibleTransfers::execute_transfer(RuntimeOrigin::signed(user), tx_id),
			Error::<Test>::InvalidSchedulerOrigin,
		);

		// Check state cleared
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());

		// Even root origin should fail
		assert_err!(ReversibleTransfers::execute_transfer(RuntimeOrigin::root(), tx_id), BadOrigin);
	});
}

#[test]
fn schedule_transfer_with_timestamp_delay_executes() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let initial_mock_time = MockTimestamp::<Test>::now();
		MockTimestamp::<Test>::set_timestamp(initial_mock_time);

		let user = 255;
		let dest_user = 2;
		let amount = 100;

		let bucket_size = TimestampBucketSize::get();
		let user_delay_duration = 5 * bucket_size; // e.g., 5000ms if bucket is 1000ms
		let user_timestamp_delay = BlockNumberOrTimestamp::Timestamp(user_delay_duration);

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user),
			user_timestamp_delay,
			user + 100,
		));

		let user_balance_before = Balances::free_balance(user);
		let dest_balance_before = Balances::free_balance(dest_user);
		let call = transfer_call(dest_user, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest_user,
			amount,
		));

		// The transfer should be scheduled at: current_time + user_delay_duration
		let expected_execution_time =
			BlockNumberOrTimestamp::Timestamp(initial_mock_time + user_delay_duration)
				.normalize(bucket_size);

		assert!(
			Agenda::<Test>::get(expected_execution_time).len() > 0,
			"Task not found in agenda for timestamp"
		);
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			amount
		);

		// Advance time to just before execution
		MockTimestamp::<Test>::set_timestamp(
			expected_execution_time.as_timestamp().unwrap() - bucket_size - 1,
		);
		run_to_block(2);

		assert_eq!(Balances::free_balance(user), user_balance_before - amount);
		assert_eq!(Balances::free_balance(dest_user), dest_balance_before);

		// Advance time to the exact execution moment
		MockTimestamp::<Test>::set_timestamp(expected_execution_time.as_timestamp().unwrap() - 1);
		run_to_block(3);

		// Check that the transfer is executed
		assert_eq!(Balances::free_balance(user), user_balance_before - amount);
		assert_eq!(Balances::free_balance(dest_user), dest_balance_before + amount);
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			0
		);
		System::assert_has_event(
			Event::TransactionExecuted { tx_id, result: Ok(().into()) }.into(),
		);
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert_eq!(Agenda::<Test>::get(expected_execution_time).len(), 0); // Task removed
	});
}

#[test]
fn full_flow_execute_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let user = 1; // Reversible, delay 10
		let dest = 2;
		let amount = 50;
		let initial_user_balance = Balances::free_balance(user);
		let initial_dest_balance = Balances::free_balance(dest);
		let call = transfer_call(dest, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);
		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let start_block = BlockNumberOrTimestamp::BlockNumber(System::block_number());
		let execute_block = start_block.saturating_add(&delay).unwrap();

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
		assert!(Agenda::<Test>::get(execute_block).len() > 0);
		assert_eq!(Balances::free_balance(user), initial_user_balance - 50); // Not executed yet, but on hold

		run_to_block(execute_block.as_block_number().unwrap());

		// Event should be emitted by execute_transfer called by scheduler
		let expected_event = Event::TransactionExecuted { tx_id, result: Ok(().into()) };
		assert!(
			System::events().iter().any(|rec| rec.event == expected_event.clone().into()),
			"Execute event not found"
		);

		assert_eq!(Balances::free_balance(user), initial_user_balance - amount);
		assert_eq!(Balances::free_balance(dest), initial_dest_balance + amount);

		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(user).is_zero());
		assert_eq!(Agenda::<Test>::get(execute_block).len(), 0); // Task removed after execution
	});
}

#[test]
fn full_flow_execute_with_timestamp_delay_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let initial_mock_time = 1_000_000;
		MockTimestamp::<Test>::set_timestamp(initial_mock_time);

		let user = 255;
		let dest = 2;
		let amount = 50;

		let user_delay_duration = 10 * TimestampBucketSize::get(); // e.g. 10s
		let user_timestamp_delay = BlockNumberOrTimestamp::Timestamp(user_delay_duration);

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user),
			user_timestamp_delay,
			user + 100,
		));

		let initial_user_balance = Balances::free_balance(user);
		let initial_dest_balance = Balances::free_balance(dest);
		let call = transfer_call(dest, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount
		));

		let expected_execution_time =
			BlockNumberOrTimestamp::Timestamp(initial_mock_time + user_delay_duration)
				.normalize(TimestampBucketSize::get());

		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
		assert!(Agenda::<Test>::get(expected_execution_time).len() > 0);
		assert_eq!(Balances::free_balance(user), initial_user_balance - amount); // On hold

		// Advance time to execution
		MockTimestamp::<Test>::set_timestamp(expected_execution_time.as_timestamp().unwrap() - 1);
		run_to_block(2);

		let expected_event = Event::TransactionExecuted { tx_id, result: Ok(().into()) };
		assert!(
			System::events().iter().any(|rec| rec.event == expected_event.clone().into()),
			"Execute event not found"
		);

		assert_eq!(Balances::free_balance(user), initial_user_balance - amount);
		assert_eq!(Balances::free_balance(dest), initial_dest_balance + amount);
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(user).is_zero());
		assert_eq!(Agenda::<Test>::get(expected_execution_time).len(), 0);
	});
}

#[test]
fn full_flow_cancel_prevents_execution() {
	new_test_ext().execute_with(|| {
		let user = 1;
		let dest = 2;
		let amount = 50;
		let initial_user_balance = Balances::free_balance(user);
		let initial_dest_balance = Balances::free_balance(dest);
		let call = transfer_call(dest, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);
		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let start_block = System::block_number();
		let execute_block = BlockNumberOrTimestampOf::<Test>::BlockNumber(
			start_block + delay.as_block_number().unwrap(),
		);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount
		));
		// Amount is on hold
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			amount
		);

		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(2), // interceptor from genesis config
			tx_id
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(user).is_zero());

		// Run past the execution block
		run_to_block(execute_block.as_block_number().unwrap() + 1);

		// State is unchanged, amount is released
		// Amount is on hold
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			0
		);
		assert_eq!(Balances::free_balance(user), initial_user_balance - amount);
		// dest (user 2) is also the interceptor, so they receive the cancelled amount
		assert_eq!(Balances::free_balance(dest), initial_dest_balance + amount);

		// No events were emitted
		let expected_event_pattern = |e: &RuntimeEvent| match e {
			RuntimeEvent::ReversibleTransfers(Event::TransactionExecuted {
				tx_id: tid, ..
			}) if *tid == tx_id => true,
			_ => false,
		};
		assert!(
			!System::events().iter().any(|rec| expected_event_pattern(&rec.event)),
			"TransactionExecuted event should not exist"
		);
	});
}

#[test]
fn full_flow_cancel_prevents_execution_with_timestamp_delay() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let initial_mock_time = 1_000_000;
		MockTimestamp::<Test>::set_timestamp(initial_mock_time);

		let user = 255;
		let dest = 2;
		let amount = 50;

		let user_delay_duration = 10 * TimestampBucketSize::get();
		let user_timestamp_delay = BlockNumberOrTimestamp::Timestamp(user_delay_duration);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user),
			user_timestamp_delay,
			user + 100,
		));

		let initial_user_balance = Balances::free_balance(user);
		let initial_dest_balance = Balances::free_balance(dest);
		let call = transfer_call(dest, amount);
		let tx_id = calculate_tx_id::<Test>(user, &call);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount
		));
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			amount
		);

		// Cancel before execution time
		MockTimestamp::<Test>::set_timestamp(initial_mock_time + user_delay_duration / 2);
		run_to_block(1);

		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(355), // interceptor from test setup (user + 100 = 255 + 100)
			tx_id
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(user).is_zero());
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			0 // Hold released
		);

		// Run past the original execution time
		let original_execution_time = initial_mock_time + user_delay_duration;
		MockTimestamp::<Test>::set_timestamp(original_execution_time + TimestampBucketSize::get());
		run_to_block(2);

		assert_eq!(Balances::free_balance(user), initial_user_balance - amount);
		assert_eq!(Balances::free_balance(dest), initial_dest_balance);
		// Interceptor (user 355) should have received the cancelled amount
		let interceptor_balance = Balances::free_balance(355);
		assert_eq!(interceptor_balance, amount); // interceptor started with 0, now has the cancelled amount

		let expected_event_pattern = |e: &RuntimeEvent| match e {
			RuntimeEvent::ReversibleTransfers(Event::TransactionExecuted {
				tx_id: tid, ..
			}) if *tid == tx_id => true,
			_ => false,
		};
		assert!(
			!System::events().iter().any(|rec| expected_event_pattern(&rec.event)),
			"TransactionExecuted event should not exist for timestamp delay"
		);
	});
}

/// The case we want to check:
///
/// 1. User 1 schedules a transfer to user 2 with amount 100
/// 2. User 1 schedules a transfer to user 2 with amount 200, after 2 blocks
/// 3. User 1 schedules a transfer to user 2 with amount 300, after 3 blocks
///
/// When the first transfer is executed, we thaw all frozen amounts, and then freeze the new amount
/// again.
#[test]
fn freeze_amount_is_consistent_with_multiple_transfers() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = 1; // Reversible, delay 10
		let dest = 2;
		let user_initial_balance = Balances::free_balance(user);
		let dest_initial_balance = Balances::free_balance(dest);

		let amount1 = 100;
		let amount2 = 200;
		let amount3 = 300;

		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let delay_blocks = delay.as_block_number().unwrap();
		let execute_block1 =
			BlockNumberOrTimestampOf::<Test>::BlockNumber(System::block_number() + delay_blocks);
		let execute_block2 = BlockNumberOrTimestampOf::<Test>::BlockNumber(
			System::block_number() + delay_blocks + 2,
		);
		let execute_block3 = BlockNumberOrTimestampOf::<Test>::BlockNumber(
			System::block_number() + delay_blocks + 3,
		);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount1
		));

		System::set_block_number(3);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount2
		));

		System::set_block_number(4);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user),
			dest,
			amount3
		));

		// Check frozen amounts
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			amount1 + amount2 + amount3
		);
		// Check that the first transfer is executed and the frozen amounts are thawed
		assert_eq!(
			Balances::free_balance(user),
			user_initial_balance - amount1 - amount2 - amount3
		);

		run_to_block(execute_block1.as_block_number().unwrap());

		// Check that the first transfer is executed and the frozen amounts are thawed
		assert_eq!(
			Balances::free_balance(user),
			user_initial_balance - amount1 - amount2 - amount3
		);
		assert_eq!(Balances::free_balance(dest), dest_initial_balance + amount1);

		// First amount is released
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			amount2 + amount3
		);

		run_to_block(execute_block2.as_block_number().unwrap());
		// Check that the second transfer is executed and the frozen amounts are thawed
		assert_eq!(
			Balances::free_balance(user),
			user_initial_balance - amount1 - amount2 - amount3
		);

		assert_eq!(Balances::free_balance(dest), dest_initial_balance + amount1 + amount2);

		// Second amount is released
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			amount3
		);
		run_to_block(execute_block3.as_block_number().unwrap());
		// Check that the third transfer is executed and the held amounts are released
		assert_eq!(
			Balances::free_balance(user),
			user_initial_balance - amount1 - amount2 - amount3
		);
		assert_eq!(
			Balances::free_balance(dest),
			dest_initial_balance + amount1 + amount2 + amount3
		);
		// Third amount is released
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			0
		);

		// Check that the held amounts are released
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&user
			),
			0
		);
	});
}

#[test]
fn schedule_transfer_with_delay_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = 3; // An account without a pre-configured policy
		let recipient: AccountId = 4;
		let amount: Balance = 1000;
		let custom_delay = BlockNumberOrTimestamp::BlockNumber(10); // A custom delay

		// Ensure the sender is not reversible initially
		assert_eq!(ReversibleTransfers::is_high_security(&sender), None);

		let call = transfer_call(recipient, amount);
		let tx_id = calculate_tx_id::<Test>(sender, &call);

		// --- Test Happy Path ---
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
			custom_delay,
		));

		// Check that the transfer is pending
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
		// Check that funds are held
		assert_eq!(
			Balances::balance_on_hold(&HoldReason::ScheduledTransfer.into(), &sender),
			amount
		);

		// --- Test Cancellation ---
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(sender), tx_id));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert_eq!(Balances::balance_on_hold(&HoldReason::ScheduledTransfer.into(), &sender), 0);

		// --- Test Error Path ---
		let configured_sender: AccountId = 1; // This account has a policy from genesis
		assert_err!(
			ReversibleTransfers::schedule_transfer_with_delay(
				RuntimeOrigin::signed(configured_sender),
				recipient,
				amount,
				custom_delay,
			),
			Error::<Test>::AccountAlreadyReversibleCannotScheduleOneTime
		);
	});
}

#[test]
fn schedule_transfer_with_error_short_delay() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = 3;
		let recipient: AccountId = 4;
		let amount: Balance = 1000;
		let custom_delay = BlockNumberOrTimestamp::BlockNumber(1);

		assert_err!(
			ReversibleTransfers::schedule_transfer_with_delay(
				RuntimeOrigin::signed(sender),
				recipient,
				amount,
				custom_delay,
			),
			Error::<Test>::DelayTooShort
		);
	});
}

#[test]
fn schedule_transfer_with_delay_executes_correctly() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = 3;
		let recipient: AccountId = 4;
		let amount: Balance = 1000;
		let custom_delay_blocks = 10;
		let custom_delay = BlockNumberOrTimestamp::BlockNumber(custom_delay_blocks);

		let initial_sender_balance = Balances::free_balance(sender);
		let initial_recipient_balance = Balances::free_balance(recipient);

		let call = transfer_call(recipient, amount);
		let tx_id = calculate_tx_id::<Test>(sender, &call);

		// Schedule the transfer
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
			custom_delay,
		));

		// Check that funds are held
		assert_eq!(
			Balances::balance_on_hold(&HoldReason::ScheduledTransfer.into(), &sender),
			amount
		);
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());

		// Run to the execution block
		let execute_block = System::block_number() + custom_delay_blocks;
		run_to_block(execute_block);

		// Check that the transfer was executed
		assert_eq!(Balances::free_balance(sender), initial_sender_balance - amount);
		assert_eq!(Balances::free_balance(recipient), initial_recipient_balance + amount);

		// Check that the hold is released
		assert_eq!(Balances::balance_on_hold(&HoldReason::ScheduledTransfer.into(), &sender), 0);

		// Check that the pending dispatch is removed
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());

		// Check for the execution event
		System::assert_has_event(
			Event::TransactionExecuted { tx_id, result: Ok(().into()) }.into(),
		);
	});
}

#[test]
fn schedule_transfer_with_timestamp_delay_executes_correctly() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		MockTimestamp::<Test>::set_timestamp(1_000_000); // Initial mock time

		let sender: AccountId = 3;
		let recipient: AccountId = 4;
		let amount: Balance = 1000;
		let one_minute_ms = 1000 * 60;
		let custom_delay_ms = 10 * one_minute_ms; // 10 minutes
		let custom_delay = BlockNumberOrTimestamp::Timestamp(custom_delay_ms);

		let initial_sender_balance = Balances::free_balance(sender);
		let initial_recipient_balance = Balances::free_balance(recipient);

		let call = transfer_call(recipient, amount);
		let tx_id = calculate_tx_id::<Test>(sender, &call);

		// Schedule the transfer
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
			custom_delay,
		));

		// Check that funds are held
		assert_eq!(
			Balances::balance_on_hold(&HoldReason::ScheduledTransfer.into(), &sender),
			amount
		);
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());

		// Verify storage indexes are properly updated after scheduling
		let sender_pending = ReversibleTransfers::pending_transfers_by_sender(&sender);
		let recipient_pending = ReversibleTransfers::pending_transfers_by_recipient(&recipient);
		assert_eq!(sender_pending.len(), 1);
		assert_eq!(sender_pending[0], tx_id);
		assert_eq!(recipient_pending.len(), 1);
		assert_eq!(recipient_pending[0], tx_id);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Set time before execution time
		MockTimestamp::<Test>::set_timestamp(1_000_000 + custom_delay_ms - one_minute_ms);
		let execute_block = System::block_number() + 3;
		run_to_block(execute_block);

		// Check that the transfer was not yet executed
		assert_eq!(Balances::free_balance(sender), initial_sender_balance - amount);

		// recipient balance not yet changed
		assert_eq!(Balances::free_balance(recipient), initial_recipient_balance);

		// Set time past execution time
		MockTimestamp::<Test>::set_timestamp(1_000_000 + custom_delay_ms + 1);
		let execute_block = System::block_number() + 2;
		run_to_block(execute_block);

		// Check that the transfer was executed
		assert_eq!(Balances::free_balance(sender), initial_sender_balance - amount);
		assert_eq!(Balances::free_balance(recipient), initial_recipient_balance + amount);

		// Check that the hold is released
		assert_eq!(Balances::balance_on_hold(&HoldReason::ScheduledTransfer.into(), &sender), 0);

		// Check that the pending dispatch is removed
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());

		// Verify storage indexes are cleaned up after execution
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 0);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 0);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 0);
		assert!(ReversibleTransfers::get_pending_transfer_details(&tx_id).is_none());

		// Check for the execution event
		System::assert_has_event(
			Event::TransactionExecuted { tx_id, result: Ok(().into()) }.into(),
		);
	});
}

#[test]
fn storage_indexes_maintained_correctly_on_schedule() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = 1; // delay of 10
		let recipient: AccountId = 4;
		let amount: Balance = 1000;

		// Initially no pending transfers
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 0);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 0);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 0);

		let call = transfer_call(recipient, amount);
		let tx_id = calculate_tx_id::<Test>(sender, &call);

		// Schedule a transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
		));

		// Verify storage indexes are properly updated
		let sender_pending = ReversibleTransfers::pending_transfers_by_sender(&sender);
		let recipient_pending = ReversibleTransfers::pending_transfers_by_recipient(&recipient);

		assert_eq!(sender_pending.len(), 1);
		assert_eq!(sender_pending[0], tx_id);
		assert_eq!(recipient_pending.len(), 1);
		assert_eq!(recipient_pending[0], tx_id);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Verify transfer details
		let transfer_details = ReversibleTransfers::get_pending_transfer_details(&tx_id);
		assert!(transfer_details.is_some());
		let details = transfer_details.unwrap();
		assert_eq!(details.from, sender);
		assert_eq!(details.amount, amount);

		// Schedule another transfer to the same recipient
		let amount2 = 2000;
		let call2 = transfer_call(recipient, amount2);
		let tx_id2 = calculate_tx_id::<Test>(sender, &call2);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender),
			recipient,
			amount2,
		));

		// Verify both transfers are indexed
		let sender_pending = ReversibleTransfers::pending_transfers_by_sender(&sender);
		let recipient_pending = ReversibleTransfers::pending_transfers_by_recipient(&recipient);

		assert_eq!(sender_pending.len(), 2);
		assert!(sender_pending.contains(&tx_id));
		assert!(sender_pending.contains(&tx_id2));
		assert_eq!(recipient_pending.len(), 2);
		assert!(recipient_pending.contains(&tx_id));
		assert!(recipient_pending.contains(&tx_id2));
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 2);
	});
}

#[test]
fn storage_indexes_maintained_correctly_on_execution() {
	new_test_ext().execute_with(|| {
		let start_block = 1;
		let sender: AccountId = 3;
		let recipient: AccountId = 4;
		let amount: Balance = 1000;
		let delay_blocks = 10;

		System::set_block_number(start_block);

		// Schedule a transfer
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
			BlockNumberOrTimestamp::BlockNumber(delay_blocks),
		));

		let call = transfer_call(recipient, amount);
		let tx_id = calculate_tx_id::<Test>(sender, &call);

		// Verify storage indexes are populated
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 1);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 1);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Execute the transfer by running to the delay block
		run_to_block(start_block + delay_blocks + 1);

		// Verify storage indexes are cleaned up
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 0);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 0);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 0);

		// Verify transfer is no longer in main storage
		assert!(ReversibleTransfers::get_pending_transfer_details(&tx_id).is_none());
	});
}

#[test]
fn storage_indexes_maintained_correctly_on_cancel() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = 1;
		let recipient: AccountId = 4;
		let amount: Balance = 1000;

		let call = transfer_call(recipient, amount);
		let tx_id = calculate_tx_id::<Test>(sender, &call);

		// Schedule a transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
		));

		// Verify storage indexes are populated
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 1);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 1);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Cancel the transfer
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(2), // interceptor from genesis config
			tx_id
		));

		// Verify storage indexes are cleaned up
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 0);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 0);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 0);

		// Verify transfer is no longer in main storage
		assert!(ReversibleTransfers::get_pending_transfer_details(&tx_id).is_none());
	});
}

#[test]
fn storage_indexes_handle_multiple_identical_transfers_correctly() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = 1; // delay of 10
		let recipient: AccountId = 4;
		let amount: Balance = 1000;

		let call = transfer_call(recipient, amount);
		let tx_id = calculate_tx_id::<Test>(sender, &call);

		// Schedule the same transfer twice (identical transfers)
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
		));

		let tx_id1 = calculate_tx_id::<Test>(sender, &call);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender),
			recipient,
			amount,
		));

		let sender_pending = ReversibleTransfers::pending_transfers_by_sender(&sender);
		let recipient_pending = ReversibleTransfers::pending_transfers_by_recipient(&recipient);

		assert_eq!(sender_pending.len(), 2);
		assert_eq!(sender_pending[0], tx_id);
		assert_eq!(sender_pending[1], tx_id1);
		assert_eq!(recipient_pending.len(), 2);
		assert_eq!(recipient_pending[0], tx_id);
		assert_eq!(recipient_pending[1], tx_id1);

		// But account count should reflect both transfers
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 2);

		// Cancel one instance
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(2), // interceptor from genesis config
			tx_id
		));

		// Indexes should still contain the transfer (since count > 1)
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 1);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 1);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Cancel the last instance
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(2), tx_id1));

		// Now indexes should be completely cleaned up
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 0);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 0);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 0);
		assert!(ReversibleTransfers::get_pending_transfer_details(&tx_id).is_none());
	});
}

#[test]
fn storage_indexes_handle_multiple_recipients_correctly() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = 1;
		let recipient1: AccountId = 4;
		let recipient2: AccountId = 5;
		let amount: Balance = 1000;

		let call1 = transfer_call(recipient1, amount);
		let tx_id1 = calculate_tx_id::<Test>(sender, &call1);

		// Schedule transfers to different recipients
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender),
			recipient1,
			amount,
		));

		let call2 = transfer_call(recipient2, amount);
		let tx_id2 = calculate_tx_id::<Test>(sender, &call2);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender),
			recipient2,
			amount,
		));

		// Sender should have both transfers
		let sender_pending = ReversibleTransfers::pending_transfers_by_sender(&sender);
		assert_eq!(sender_pending.len(), 2);
		assert!(sender_pending.contains(&tx_id1));
		assert!(sender_pending.contains(&tx_id2));

		// Each recipient should have their own transfer
		let recipient1_pending = ReversibleTransfers::pending_transfers_by_recipient(&recipient1);
		let recipient2_pending = ReversibleTransfers::pending_transfers_by_recipient(&recipient2);

		assert_eq!(recipient1_pending.len(), 1);
		assert_eq!(recipient1_pending[0], tx_id1);
		assert_eq!(recipient2_pending.len(), 1);
		assert_eq!(recipient2_pending[0], tx_id2);

		// Account count should reflect both transfers
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 2);

		// Cancel one transfer
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(2), // interceptor from genesis config
			tx_id1
		));

		// Verify selective cleanup
		let sender_pending = ReversibleTransfers::pending_transfers_by_sender(&sender);
		assert_eq!(sender_pending.len(), 1);
		assert_eq!(sender_pending[0], tx_id2);

		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient1).len(), 0);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient2).len(), 1);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);
	});
}

#[test]
fn interceptor_index_works_with_interceptor() {
	new_test_ext().execute_with(|| {
		let reversible_account = 100;
		let interceptor = 101;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Initially, interceptor should have empty list
		assert_eq!(ReversibleTransfers::interceptor_index(&interceptor).len(), 0);

		// Set up reversibility with explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			delay,
			interceptor,
		));

		// Verify interceptor index is updated
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 1);
		assert_eq!(interceptor_accounts[0], reversible_account);

		// Verify account has correct reversibility data
		assert_eq!(
			ReversibleTransfers::is_high_security(&reversible_account),
			Some(HighSecurityAccountData { delay, interceptor })
		);
	});
}

#[test]
fn interceptor_index_handles_multiple_accounts() {
	new_test_ext().execute_with(|| {
		let interceptor = 100;
		let account1 = 101;
		let account2 = 102;
		let account3 = 103;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up multiple accounts with same interceptor
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account1),
			delay,
			interceptor,
		));

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account2),
			delay,
			interceptor,
		));

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account3),
			delay,
			interceptor,
		));

		// Verify interceptor index contains all accounts
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 3);
		assert!(interceptor_accounts.contains(&account1));
		assert!(interceptor_accounts.contains(&account2));
		assert!(interceptor_accounts.contains(&account3));
	});
}

#[test]
fn interceptor_index_prevents_duplicates() {
	new_test_ext().execute_with(|| {
		let reversible_account = 100;
		let interceptor = 101;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up reversibility with explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			delay,
			interceptor,
		));

		// Verify initial state
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 1);
		assert_eq!(interceptor_accounts[0], reversible_account);

		// Try to add the same account again (this should fail due to AccountAlreadyReversible)
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(reversible_account),
				delay,
				interceptor,
			),
			Error::<Test>::AccountAlreadyHighSecurity
		);

		// Verify no duplicates in interceptor index
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 1);
	});
}

#[test]
fn interceptor_index_respects_max_limit() {
	new_test_ext().execute_with(|| {
		let interceptor = 100;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Add accounts up to the limit (MaxInterceptorAccounts = 10 in mock)
		for i in 101..=110 {
			assert_ok!(ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(i),
				delay,
				interceptor,
			));
		}

		// Verify we have the maximum number of accounts
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 10);

		// Try to add one more account - should fail
		assert_err!(
			ReversibleTransfers::set_high_security(RuntimeOrigin::signed(111), delay, interceptor,),
			Error::<Test>::TooManyInterceptorAccounts
		);

		// Verify count didn't change
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 10);
	});
}

#[test]
fn interceptor_index_empty_for_non_interceptors() {
	new_test_ext().execute_with(|| {
		let non_interceptor = 100;
		let reversible_account = 101;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up account without explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			delay,
			reversible_account + 100,
		));

		// Verify non-interceptor has empty list
		assert_eq!(ReversibleTransfers::interceptor_index(&non_interceptor).len(), 0);
		assert_eq!(ReversibleTransfers::interceptor_index(&reversible_account).len(), 0);
	});
}

#[test]
fn interceptor_index_different_interceptors_separate_lists() {
	new_test_ext().execute_with(|| {
		let interceptor1 = 101;
		let interceptor2 = 102;
		let account1 = 102;
		let account2 = 103;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up accounts with different interceptors
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account1),
			delay,
			interceptor1,
		));

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account2),
			delay,
			interceptor2,
		));

		// Verify each interceptor has their own separate list
		let interceptor1_accounts = ReversibleTransfers::interceptor_index(&interceptor1);
		assert_eq!(interceptor1_accounts.len(), 1);
		assert_eq!(interceptor1_accounts[0], account1);

		let interceptor2_accounts = ReversibleTransfers::interceptor_index(&interceptor2);
		assert_eq!(interceptor2_accounts.len(), 1);
		assert_eq!(interceptor2_accounts[0], account2);
	});
}

#[test]
fn interceptor_index_works_with_intercept_policy() {
	new_test_ext().execute_with(|| {
		let reversible_account = 100;
		let interceptor = 101;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up reversibility with Intercept policy and explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			delay,
			interceptor,
		));

		// Verify interceptor index is updated regardless of policy
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 1);
		assert_eq!(interceptor_accounts[0], reversible_account);

		// Verify account has correct policy
		assert_eq!(
			ReversibleTransfers::is_high_security(&reversible_account),
			Some(HighSecurityAccountData { delay, interceptor })
		);
	});
}

#[test]
fn global_nonce_works() {
	new_test_ext().execute_with(|| {
		let nonce = ReversibleTransfers::global_nonce();
		assert_eq!(nonce, 0);

		// Perform a reversible transfer
		let reversible_account = 100;
		let receiver = 101;
		let amount = 100;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		let interceptor = reversible_account + 1;
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account),
			delay,
			interceptor,
		));

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(reversible_account),
			receiver,
			amount,
		));

		let nonce = ReversibleTransfers::global_nonce();
		assert_eq!(nonce, 1);

		// batch call should have all unique tx ids and increment nonce
		assert_ok!(Utility::batch(
			RuntimeOrigin::signed(reversible_account),
			vec![
				ReversibleTransfersCall::schedule_transfer { dest: receiver, amount }.into(),
				ReversibleTransfersCall::schedule_transfer { dest: receiver, amount: amount + 1 }
					.into(),
				ReversibleTransfersCall::schedule_transfer { dest: receiver, amount: amount + 2 }
					.into(),
			],
		));

		assert_eq!(ReversibleTransfers::global_nonce(), 4);
	});
}
