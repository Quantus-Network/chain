use crate::tests::mock::*; // Import mock runtime and types
use crate::*; // Import items from parent module (lib.rs)
use frame_support::{
	assert_err, assert_ok,
	traits::{
		fungible::InspectHold, fungibles::Inspect as AssetsInspect,
		tokens::fungibles::InspectHold as AssetsInspectHold, StorePreimage, Time,
	},
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

// Helper to create and mint asset
fn create_asset(id: u32, owner: AccountId, supply: Option<Balance>) {
	assert_ok!(pallet_assets::Pallet::<Test>::create(
		RuntimeOrigin::signed(owner.clone()),
		codec::Compact(id),
		owner.clone(),
		1,
	));
	let amount = supply.unwrap_or(1_000_000_000_000);
	assert_ok!(pallet_assets::Pallet::<Test>::mint(
		RuntimeOrigin::signed(owner.clone()),
		codec::Compact(id),
		owner,
		amount,
	));
}

fn asset_balance(id: u32, who: &AccountId) -> Balance {
	pallet_assets::Pallet::<Test>::balance(id, who.clone())
}

// Test-only helper: amount held (by reversible pallet reason) for an asset account
fn asset_holds(id: u32, who: &AccountId) -> Balance {
	let reason: RuntimeHoldReason = HoldReason::ScheduledTransfer.into();
	<pallet_assets_holder::Pallet<Test> as AssetsInspectHold<_>>::balance_on_hold(id, &reason, who)
}

#[test]
fn set_high_security_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let genesis_user = alice();

		// Check initial state
		assert_eq!(
			ReversibleTransfers::is_high_security(&genesis_user),
			Some(HighSecurityAccountData {
				delay: BlockNumberOrTimestampOf::<Test>::BlockNumber(10),
				interceptor: bob(),
			})
		);

		// Set the delay
		let another_user = account_id(4);
		let interceptor = account_id(5);
		let delay = BlockNumberOrTimestampOf::<Test>::BlockNumber(5);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(another_user.clone()),
			delay,
			interceptor.clone(),
		));
		assert_eq!(
			ReversibleTransfers::is_high_security(&another_user),
			Some(HighSecurityAccountData { delay, interceptor: interceptor.clone() })
		);
		System::assert_last_event(
			Event::HighSecuritySet {
				who: another_user.clone(),
				interceptor: interceptor.clone(),
				delay,
			}
			.into(),
		);

		// Calling this again should err
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(another_user.clone()),
				delay,
				interceptor.clone(),
			),
			Error::<Test>::AccountAlreadyHighSecurity
		);

		// Use default delay
		let default_user = account_id(7);
		let default_interceptor = account_id(8);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(default_user.clone()),
			DefaultDelay::get(),
			default_interceptor.clone(),
		));
		assert_eq!(
			ReversibleTransfers::is_high_security(&default_user),
			Some(HighSecurityAccountData {
				delay: DefaultDelay::get(),
				interceptor: default_interceptor.clone(),
			})
		);
		System::assert_last_event(
			Event::HighSecuritySet {
				who: default_user,
				interceptor: default_interceptor.clone(),
				delay: DefaultDelay::get(),
			}
			.into(),
		);

		// Too short delay
		let _short_delay: BlockNumberOrTimestampOf<Test> =
			BlockNumberOrTimestamp::BlockNumber(MinDelayPeriodBlocks::get() - 1);

		let new_user = account_id(10);
		let new_interceptor = account_id(11);
		let short_delay = BlockNumberOrTimestampOf::<Test>::BlockNumber(1);
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(new_user.clone()),
				short_delay,
				new_interceptor.clone(),
			),
			Error::<Test>::DelayTooShort
		);

		// Explicit reverse can not be self
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(new_user.clone()),
				delay,
				new_user.clone(),
			),
			Error::<Test>::InterceptorCannotBeSelf
		);

		assert_eq!(ReversibleTransfers::is_high_security(&new_user), None);

		// Use explicit reverser
		let reversible_account = account_id(6);
		let interceptor = account_id(7);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			delay,
			interceptor.clone(),
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

		let user = account_id(4);
		// Assuming MinDelayPeriod allows for timestamp delays of this magnitude.
		// e.g., MinDelayPeriod is Timestamp(1000) and TimestampBucketSize is 1000.
		// A delay of 5 * TimestampBucketSize = 5000.
		let delay = BlockNumberOrTimestamp::Timestamp(5 * TimestampBucketSize::get());

		let interceptor = account_id(16);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user.clone()),
			delay,
			interceptor.clone(),
		));

		assert_eq!(
			ReversibleTransfers::is_high_security(&user),
			Some(HighSecurityAccountData { delay, interceptor })
		);

		// Try to set a delay that's too short - timestamp based
		// Assuming MinDelayPeriodTimestamp is, say, 2 * TimestampBucketSize::get().
		let short_delay_ts = BlockNumberOrTimestamp::Timestamp(TimestampBucketSize::get());
		let another_user = account_id(5);

		let another_interceptor = account_id(18);
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(another_user.clone()),
				short_delay_ts,
				another_interceptor.clone(),
			),
			Error::<Test>::DelayTooShort
		);
	});
}

#[test]
fn set_reversibility_fails_delay_too_short() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = account_id(20);
		let interceptor = account_id(21);
		let short_delay = BlockNumberOrTimestampOf::<Test>::BlockNumber(1);
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(user.clone()),
				short_delay,
				interceptor.clone(),
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
		let user = alice(); // Reversible from genesis
		let dest_user = bob();
		let amount = 100;
		let dest_user_balance = Balances::free_balance(&dest_user);
		let user_balance = Balances::free_balance(&user);

		let call = transfer_call(dest_user.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);
		let HighSecurityAccountData { delay: user_delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let expected_block = System::block_number() + user_delay.as_block_number().unwrap();
		let bounded = Preimage::bound(call.clone()).unwrap();
		let expected_block = BlockNumberOrTimestamp::BlockNumber(expected_block);

		assert!(Agenda::<Test>::get(expected_block).is_empty());

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest_user.clone(),
			amount,
		));

		// Check storage
		assert_eq!(
			PendingTransfers::<Test>::get(tx_id).unwrap(),
			PendingTransfer {
				from: user.clone(),
				to: dest_user.clone(),
				interceptor: bob(), // From genesis config
				call: bounded,
				amount,
			}
		);
		assert_eq!(ReversibleTransfers::account_pending_index(&user), 1);

		// Check scheduler
		assert!(!Agenda::<Test>::get(expected_block).is_empty());

		// Skip to the delay block
		run_to_block(expected_block.as_block_number().unwrap());

		// Check that the transfer is executed
		let eps: Balance = 10; // tolerate tiny fee differences
		assert!(approx_eq_balance(Balances::free_balance(&user), user_balance - amount, eps));
		assert_eq!(Balances::free_balance(&dest_user), dest_user_balance + amount);

		// Use explicit reverser
		let reversible_account = ferdie();
		let interceptor = user.clone();

		// Set reversibility
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			BlockNumberOrTimestamp::BlockNumber(10),
			interceptor.clone(),
		));

		let tx_id = calculate_tx_id::<Test>(reversible_account.clone(), &call);
		// Schedule transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(reversible_account.clone()),
			dest_user.clone(),
			amount,
		));

		// Try reversing with original user
		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(reversible_account.clone()), tx_id,),
			Error::<Test>::InvalidReverser
		);

		let interceptor_balance = Balances::free_balance(&interceptor);
		let reversible_account_balance = Balances::free_balance(&reversible_account);
		let interceptor_hold = Balances::balance_on_hold(
			&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
			&interceptor,
		);
		assert_eq!(interceptor_hold, 0);

		// Try reversing with explicit reverser
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(interceptor.clone()), tx_id,));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());

		// Funds should be release as free balance to `interceptor`
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&reversible_account
			),
			0
		);

		// With 100 tokens and 100 basis points (1%) fee: 100 * 100 / 10000 = 1 token fee
		let expected_fee = 1;
		let expected_amount_to_interceptor = amount - expected_fee;
		assert_eq!(
			Balances::free_balance(&interceptor),
			interceptor_balance + expected_amount_to_interceptor
		);

		// Unchanged balance for `reversible_account`
		assert_eq!(Balances::free_balance(&reversible_account), reversible_account_balance);

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
		let user = ferdie();
		let dest_user = bob();
		let amount = 100;
		let dest_user_balance = Balances::free_balance(&dest_user);
		let user_balance = Balances::free_balance(&user);

		let call = transfer_call(dest_user.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);

		// Set reversibility
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user.clone()),
			BlockNumberOrTimestamp::Timestamp(10_000),
			interceptor_255(), // Interceptor for ferdie
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

		assert!(Agenda::<Test>::get(expected_timestamp).is_empty());

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest_user.clone(),
			amount,
		));

		// Check storage
		assert_eq!(
			PendingTransfers::<Test>::get(tx_id).unwrap(),
			PendingTransfer {
				from: user.clone(),
				to: dest_user.clone(),
				interceptor: interceptor_255(), /* This should match the actual interceptor from
				                                 * the test setup */
				call: bounded,
				amount,
			}
		);
		assert_eq!(ReversibleTransfers::account_pending_index(&user), 1);

		// Check scheduler
		assert!(!Agenda::<Test>::get(expected_timestamp).is_empty());

		// Advance to expected execution time and ensure it executed
		MockTimestamp::<Test>::set_timestamp(expected_raw_timestamp);
		run_to_block(2);
		let eps: Balance = 10; // tolerate tiny fee differences
		assert!(approx_eq_balance(Balances::free_balance(&user), user_balance - amount, eps));
		assert_eq!(Balances::free_balance(&dest_user), dest_user_balance + amount);

		// Use explicit reverser
		let reversible_account = account_256();
		let interceptor = alice();

		// Set reversibility
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			BlockNumberOrTimestamp::BlockNumber(10),
			interceptor.clone(),
		));

		let call = transfer_call(dest_user.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(reversible_account.clone(), &call);
		// Schedule transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(reversible_account.clone()),
			dest_user.clone(),
			amount,
		));

		// Try reversing with original user
		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(reversible_account.clone()), tx_id,),
			Error::<Test>::InvalidReverser
		);

		let interceptor_balance = Balances::free_balance(&interceptor);
		let reversible_account_balance = Balances::free_balance(&reversible_account);
		let interceptor_hold = Balances::balance_on_hold(
			&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
			&interceptor,
		);
		assert_eq!(interceptor_hold, 0);

		// Try reversing with explicit reverser
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(interceptor.clone()), tx_id,));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());

		// Funds should be release as free balance to `interceptor`
		assert_eq!(
			Balances::balance_on_hold(
				&RuntimeHoldReason::ReversibleTransfers(HoldReason::ScheduledTransfer),
				&reversible_account
			),
			0
		);

		// With 100 tokens and 100 basis points (1%) fee: 100 * 100 / 10000 = 1 token fee
		let expected_fee = 1;
		let expected_amount_to_interceptor = amount - expected_fee;
		assert_eq!(
			Balances::free_balance(&interceptor),
			interceptor_balance + expected_amount_to_interceptor
		);

		// Unchanged balance for `reversible_account`
		assert_eq!(Balances::free_balance(&reversible_account), reversible_account_balance);

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
		let user = bob(); // Not reversible

		assert_err!(
			ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(user.clone()),
				charlie(),
				50
			),
			Error::<Test>::AccountNotHighSecurity
		);
	});
}

#[test]
fn schedule_multiple_transfer_works() {
	new_test_ext().execute_with(|| {
		let user = alice(); // User 1 is reversible from genesis with interceptor=2, recoverer=3
		let dest_user = bob();
		let amount = 100;

		let tx_id =
			calculate_tx_id::<Test>(user.clone(), &transfer_call(dest_user.clone(), amount));

		// Schedule first
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest_user.clone(),
			amount,
		));

		// Try to schedule the same call again
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest_user.clone(),
			amount
		));

		// Check that the count of pending transactions for the user is 2
		assert_eq!(ReversibleTransfers::account_pending_index(&user), 2);

		// Check that the pending transaction count decreases to 1
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(bob()), // interceptor from genesis config
			tx_id
		));
		assert_eq!(ReversibleTransfers::account_pending_index(&user), 1);

		// Check that the pending transaction count decreases to 0 when executed
		let execute_block = System::block_number() + 10;
		run_to_block(execute_block);

		assert_eq!(ReversibleTransfers::account_pending_index(&user), 0);
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
	});
}

#[test]
fn schedule_transfer_fails_too_many_pending() {
	new_test_ext().execute_with(|| {
		let user = alice();
		let max_pending = MaxReversibleTransfers::get();

		// Fill up pending slots
		for i in 0..max_pending {
			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(user.clone()),
				bob(),
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
			ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(user.clone()),
				charlie(),
				100
			),
			Error::<Test>::TooManyPendingTransactions
		);
	});
}

#[test]
fn cancel_dispatch_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = alice(); // High-security account from genesis
		let interceptor = bob();
		let treasury = treasury();
		let amount = 10_000;
		let call = transfer_call(interceptor.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);
		let HighSecurityAccountData { delay: user_delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let execute_block = BlockNumberOrTimestamp::BlockNumber(
			System::block_number() + user_delay.as_block_number().unwrap(),
		);

		// Record initial balances
		let initial_interceptor_balance = Balances::free_balance(&interceptor);
		let initial_treasury_balance = Balances::free_balance(&treasury);

		assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

		// Schedule first
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			interceptor.clone(),
			amount,
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
		assert!(!ReversibleTransfers::account_pending_index(&user).is_zero());

		// Check the expected block agendas count
		assert_eq!(Agenda::<Test>::get(execute_block).len(), 1);

		// Now cancel (must be called by interceptor, which is user 2 from genesis)
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(interceptor.clone()), // interceptor from genesis config
			tx_id
		));

		// Check state cleared
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(&user).is_zero());

		assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

		// Verify volume fee was applied for high-security account
		// Expected fee: 10,000 * 100 / 10,000 = 100 tokens
		let expected_fee = 100;
		let expected_remaining = amount - expected_fee;

		// Check that interceptor received the remaining amount (after fee)
		// Check final balances after cancellation
		assert_eq!(
			Balances::free_balance(&interceptor),
			initial_interceptor_balance + expected_remaining,
			"High-security account should have volume fee deducted"
		);

		assert_eq!(
			Balances::free_balance(&treasury),
			initial_treasury_balance + expected_fee,
			"Treasury should receive volume fee from high-security account cancellation"
		);

		// Check event
		System::assert_last_event(Event::TransactionCancelled { who: interceptor, tx_id }.into());
	});
}

#[test]
fn no_volume_fee_for_regular_reversible_accounts() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = charlie(); // Regular account (not high-security)
		let recipient = dave();
		let treasury = treasury();
		let amount = 10_000;

		// Check initial balances
		let initial_user_balance = Balances::free_balance(&user);
		let initial_recipient_balance = Balances::free_balance(&recipient);
		let initial_treasury_balance = Balances::free_balance(&treasury);

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);

		// Schedule transfer with delay (regular accounts use schedule_transfer_with_delay)
		let delay = BlockNumberOrTimestamp::BlockNumber(5);
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(user.clone()),
			recipient.clone(),
			amount,
			delay
		));

		// Cancel the transfer (user can cancel their own for regular accounts)
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(user.clone()), tx_id));

		// Verify user got full amount back (no volume fee for regular accounts)
		// For regular accounts, cancellation returns funds to the original sender
		assert_eq!(
			Balances::free_balance(&user),
			initial_user_balance, // Should be back to original balance
			"Regular accounts should get full refund with no volume fee deducted"
		);

		// Verify recipient balance unchanged (they never received the funds)
		assert_eq!(
			Balances::free_balance(&recipient),
			initial_recipient_balance,
			"Recipient should not receive funds when transaction is cancelled"
		);

		// Verify treasury balance unchanged
		assert_eq!(
			Balances::free_balance(&treasury),
			initial_treasury_balance,
			"Treasury should not receive fee from regular account cancellation"
		);

		// Should still have TransactionCancelled event
		System::assert_has_event(Event::TransactionCancelled { who: user, tx_id }.into());
	});
}

#[test]
fn cancel_dispatch_fails_not_owner() {
	new_test_ext().execute_with(|| {
		let owner = alice();
		let _attacker = charlie();
		let call = transfer_call(bob(), 50);
		let tx_id = calculate_tx_id::<Test>(owner.clone(), &call);

		// Schedule as owner
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(owner.clone()),
			bob(),
			50
		));

		// Attacker tries to cancel
		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(charlie()), tx_id),
			Error::<Test>::InvalidReverser
		);
	});
}

#[test]
fn cancel_dispatch_fails_not_found() {
	new_test_ext().execute_with(|| {
		let user = dave();
		let non_existent_tx_id = H256::random();

		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(user.clone()), non_existent_tx_id),
			Error::<Test>::PendingTxNotFound
		);
	});
}

#[test]
fn execute_transfer_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let user = alice(); // Reversible, delay 10
		let dest = bob();
		let amount = 50;
		let call = transfer_call(dest.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);

		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let execute_block = BlockNumberOrTimestampOf::<Test>::BlockNumber(
			System::block_number() + delay.as_block_number().unwrap(),
		);

		// Schedule as the same user who wants to be reversible
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
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

		let user = account_256();
		let dest_user = bob();
		let amount = 100;

		let bucket_size = TimestampBucketSize::get();
		let user_delay_duration = 5 * bucket_size; // e.g., 5000ms if bucket is 1000ms
		let user_timestamp_delay = BlockNumberOrTimestamp::Timestamp(user_delay_duration);

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user.clone()),
			user_timestamp_delay,
			interceptor_1(), // interceptor for alice
		));

		let user_balance_before = Balances::free_balance(&user);
		let dest_balance_before = Balances::free_balance(&dest_user);
		let call = transfer_call(dest_user.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);

		// Schedule a transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest_user.clone(),
			amount,
		));

		// The transfer should be scheduled at: current_time + user_delay_duration
		let expected_execution_time =
			BlockNumberOrTimestamp::Timestamp(initial_mock_time + user_delay_duration)
				.normalize(bucket_size);

		assert!(
			!Agenda::<Test>::get(expected_execution_time).is_empty(),
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

		assert_eq!(Balances::free_balance(&user), user_balance_before - amount);
		assert_eq!(Balances::free_balance(&dest_user), dest_balance_before);

		// Advance time to the exact execution moment
		MockTimestamp::<Test>::set_timestamp(expected_execution_time.as_timestamp().unwrap() - 1);
		run_to_block(3);

		// Check that the transfer is executed
		assert_eq!(Balances::free_balance(&user), user_balance_before - amount);
		assert_eq!(Balances::free_balance(&dest_user), dest_balance_before + amount);
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

		let user = alice(); // Reversible, delay 10
		let dest = bob();
		let amount = 50;
		let initial_user_balance = Balances::free_balance(&user);
		let initial_dest_balance = Balances::free_balance(&dest);
		let call = transfer_call(dest.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);
		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let start_block = BlockNumberOrTimestamp::BlockNumber(System::block_number());
		let execute_block = start_block.saturating_add(&delay).unwrap();

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
			amount,
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
		assert!(!Agenda::<Test>::get(execute_block).is_empty());
		assert_eq!(Balances::free_balance(&user), initial_user_balance - 50); // Not executed yet, but on hold

		run_to_block(execute_block.as_block_number().unwrap());

		// Event should be emitted by execute_transfer called by scheduler
		let expected_event = Event::TransactionExecuted { tx_id, result: Ok(().into()) };
		assert!(
			System::events().iter().any(|rec| rec.event == expected_event.clone().into()),
			"Execute event not found"
		);

		assert_eq!(Balances::free_balance(&user), initial_user_balance - amount);
		assert_eq!(Balances::free_balance(&dest), initial_dest_balance + amount);

		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(&user).is_zero());
		assert_eq!(Agenda::<Test>::get(execute_block).len(), 0); // Task removed after execution
	});
}

#[test]
fn full_flow_execute_with_timestamp_delay_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let initial_mock_time = 1_000_000;
		MockTimestamp::<Test>::set_timestamp(initial_mock_time);

		let user = ferdie();
		let dest = bob();
		let amount = 50;

		let user_delay_duration = 10 * TimestampBucketSize::get(); // e.g. 10s
		let user_timestamp_delay = BlockNumberOrTimestamp::Timestamp(user_delay_duration);

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user.clone()),
			user_timestamp_delay,
			interceptor_255(), // interceptor for ferdie
		));

		let initial_user_balance = Balances::free_balance(&user);
		let initial_dest_balance = Balances::free_balance(&dest);
		let call = transfer_call(dest.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
			amount,
		));

		let expected_execution_time =
			BlockNumberOrTimestamp::Timestamp(initial_mock_time + user_delay_duration)
				.normalize(TimestampBucketSize::get());

		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_some());
		assert!(!Agenda::<Test>::get(expected_execution_time).is_empty());
		assert_eq!(Balances::free_balance(&user), initial_user_balance - amount); // On hold

		// Advance time to execution
		MockTimestamp::<Test>::set_timestamp(expected_execution_time.as_timestamp().unwrap() - 1);
		run_to_block(2);

		let expected_event = Event::TransactionExecuted { tx_id, result: Ok(().into()) };
		assert!(
			System::events().iter().any(|rec| rec.event == expected_event.clone().into()),
			"Execute event not found"
		);

		assert_eq!(Balances::free_balance(&user), initial_user_balance - amount);
		assert_eq!(Balances::free_balance(&dest), initial_dest_balance + amount);
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(&user).is_zero());
		assert_eq!(Agenda::<Test>::get(expected_execution_time).len(), 0);
	});
}

#[test]
fn full_flow_cancel_prevents_execution() {
	new_test_ext().execute_with(|| {
		let user = alice();
		let dest = bob();
		let amount = 50;

		let initial_user_balance = Balances::free_balance(&user);
		let initial_dest_balance = Balances::free_balance(&dest);
		let call = transfer_call(dest.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);
		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&user).unwrap();
		let start_block = System::block_number();
		let execute_block = BlockNumberOrTimestampOf::<Test>::BlockNumber(
			start_block + delay.as_block_number().unwrap(),
		);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
			amount,
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
			RuntimeOrigin::signed(bob()), // interceptor from genesis config
			tx_id
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(&user).is_zero());

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
		assert_eq!(Balances::free_balance(&user), initial_user_balance - amount);
		// dest (user 2) is also the interceptor, so they receive the cancelled amount
		assert_eq!(Balances::free_balance(&dest), initial_dest_balance + amount);

		// No events were emitted
		let expected_event_pattern = |e: &RuntimeEvent| {
			matches!(e, RuntimeEvent::ReversibleTransfers(Event::TransactionExecuted {
			tx_id: tid, ..
		}) if *tid == tx_id)
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

		let user = ferdie();
		let dest = account_256();
		let amount = 50;
		let user_delay_duration = 10 * TimestampBucketSize::get();

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(user.clone()),
			BlockNumberOrTimestamp::Timestamp(user_delay_duration),
			interceptor_255(),
		));

		let initial_user_balance = Balances::free_balance(&user);
		let initial_dest_balance = Balances::free_balance(&dest);
		let call = transfer_call(dest.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(user.clone(), &call);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
			amount,
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
			RuntimeOrigin::signed(interceptor_255()), // interceptor from test setup
			tx_id
		));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert!(ReversibleTransfers::account_pending_index(&user).is_zero());
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

		assert_eq!(Balances::free_balance(&user), initial_user_balance - amount);
		assert_eq!(Balances::free_balance(&dest), initial_dest_balance);
		// Interceptor should have received the cancelled amount
		let interceptor_balance = Balances::free_balance(interceptor_255());
		assert_eq!(interceptor_balance, amount); // interceptor started with 0, now has the cancelled amount

		let expected_event_pattern = |e: &RuntimeEvent| {
			matches!(e, RuntimeEvent::ReversibleTransfers(Event::TransactionExecuted {
			tx_id: tid, ..
		}) if *tid == tx_id)
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
		let user = alice(); // Reversible, delay 10
		let dest = bob();
		let user_initial_balance = Balances::free_balance(&user);
		let dest_initial_balance = Balances::free_balance(&dest);

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
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
			amount1
		));

		System::set_block_number(3);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
			amount2
		));

		System::set_block_number(4);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(user.clone()),
			dest.clone(),
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
			Balances::free_balance(&user),
			user_initial_balance - amount1 - amount2 - amount3
		);

		run_to_block(execute_block1.as_block_number().unwrap());

		// Check that the first transfer is executed and the frozen amounts are thawed
		assert_eq!(
			Balances::free_balance(&user),
			user_initial_balance - amount1 - amount2 - amount3
		);
		assert_eq!(Balances::free_balance(&dest), dest_initial_balance + amount1);

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
			Balances::free_balance(&user),
			user_initial_balance - amount1 - amount2 - amount3
		);

		assert_eq!(Balances::free_balance(&dest), dest_initial_balance + amount1 + amount2);

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
			Balances::free_balance(&user),
			user_initial_balance - amount1 - amount2 - amount3
		);
		assert_eq!(
			Balances::free_balance(&dest),
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
		let sender: AccountId = eve(); // An account without a pre-configured policy
		let recipient: AccountId = ferdie();
		let amount: Balance = 1000;
		let custom_delay = BlockNumberOrTimestamp::BlockNumber(10); // A custom delay

		// Ensure the sender is not reversible initially
		assert_eq!(ReversibleTransfers::is_high_security(&sender), None);

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(sender.clone(), &call);

		// --- Test Happy Path ---
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
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
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(sender.clone()), tx_id));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert_eq!(Balances::balance_on_hold(&HoldReason::ScheduledTransfer.into(), &sender), 0);

		// --- Test Error Path ---
		let configured_sender: AccountId = alice(); // This account has a policy from genesis
		assert_err!(
			ReversibleTransfers::schedule_transfer_with_delay(
				RuntimeOrigin::signed(configured_sender.clone()),
				recipient.clone(),
				amount,
				custom_delay,
			),
			Error::<Test>::AccountAlreadyReversibleCannotScheduleOneTime
		);
	});
}

#[test]
fn schedule_asset_transfer_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = alice(); // has high-security from genesis
		let recipient: AccountId = dave();
		let asset_id: u32 = 42;
		let amount: Balance = 1_000;

		create_asset(asset_id, sender.clone(), None);
		let sender_asset_before = asset_balance(asset_id, &sender);
		let recipient_asset_before = asset_balance(asset_id, &recipient);

		// Schedule asset transfer using configured delay
		assert_ok!(ReversibleTransfers::schedule_asset_transfer(
			RuntimeOrigin::signed(sender.clone()),
			asset_id,
			recipient.clone(),
			amount,
		));

		// Should be frozen (assets path uses freeze, not balances hold)
		// Verify pending index increments
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Advance to execution and ensure balances moved
		let HighSecurityAccountData { delay, .. } =
			ReversibleTransfers::is_high_security(&sender).unwrap();
		let execute_block = System::block_number() + delay.as_block_number().unwrap();
		run_to_block(execute_block);

		assert_eq!(asset_balance(asset_id, &sender), sender_asset_before - amount);
		assert_eq!(asset_balance(asset_id, &recipient), recipient_asset_before + amount);
	});
}

#[test]
fn schedule_asset_transfer_with_delay_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = charlie(); // not configured; use one-time delay API
		let recipient: AccountId = dave();
		let asset_id: u32 = 77;
		let amount: Balance = 2_000;
		let custom_delay_blocks: u64 = 8;

		create_asset(asset_id, sender.clone(), None);
		let sender_asset_before = asset_balance(asset_id, &sender);
		let recipient_asset_before = asset_balance(asset_id, &recipient);

		assert_ok!(ReversibleTransfers::schedule_asset_transfer_with_delay(
			RuntimeOrigin::signed(sender.clone()),
			asset_id,
			recipient.clone(),
			amount,
			BlockNumberOrTimestamp::BlockNumber(custom_delay_blocks),
		));

		let execute_block = System::block_number() + custom_delay_blocks;
		run_to_block(execute_block);

		assert_eq!(asset_balance(asset_id, &sender), sender_asset_before - amount);
		assert_eq!(asset_balance(asset_id, &recipient), recipient_asset_before + amount);
		assert_eq!(asset_holds(asset_id, &sender), 0);
	});
}

#[test]
fn asset_hold_does_not_block_spending() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = alice(); // high-security from genesis
		let interceptor = bob(); // from genesis config for 1
		let recipient: AccountId = dave();
		let third_party: AccountId = account_id(9);
		let asset_id: u32 = 314;
		let spend_amount: Balance = 3_000;

		create_asset(asset_id, sender.clone(), None);
		let sender_before = asset_balance(asset_id, &sender);
		let recipient_before = asset_balance(asset_id, &recipient);
		let third_before = asset_balance(asset_id, &third_party);
		let hold_amount = sender_before - spend_amount / 2;

		// Schedule an asset transfer to create a hold on `sender`.
		assert_ok!(ReversibleTransfers::schedule_asset_transfer(
			RuntimeOrigin::signed(sender.clone()),
			asset_id,
			recipient.clone(),
			hold_amount,
		));

		// Hold exists; free balance reduced by hold amount.
		assert_eq!(asset_holds(asset_id, &sender), hold_amount);
		let free_after_hold = sender_before - hold_amount;
		assert_eq!(asset_balance(asset_id, &sender), free_after_hold);
		assert_eq!(asset_balance(asset_id, &recipient), recipient_before);

		// With holds, spending up to free balance is allowed; leave min_balance.
		let min = <pallet_assets::Pallet<Test> as AssetsInspect<_>>::minimum_balance(asset_id);
		let spend = free_after_hold.saturating_sub(min);
		assert_ok!(pallet_assets::Pallet::<Test>::transfer_keep_alive(
			RuntimeOrigin::signed(sender.clone()),
			codec::Compact(asset_id),
			third_party.clone(),
			spend,
		));

		// Verify spend succeeded while hold remains.
		assert_eq!(asset_holds(asset_id, &sender), hold_amount);
		assert_eq!(asset_balance(asset_id, &sender), min);
		assert_eq!(asset_balance(asset_id, &third_party), third_before + spend);

		// Pending remains and will execute later; cancel it now to clean up and credit interceptor.
		let ids = ReversibleTransfers::pending_transfers_by_sender(&sender);
		assert_eq!(ids.len(), 1);
		let tx_id = ids[0];
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(interceptor.clone()), tx_id));
		assert!(ReversibleTransfers::pending_dispatches(tx_id).is_none());
		assert_eq!(asset_holds(asset_id, &sender), 0);
	});
}

#[test]
fn asset_hold_blocks_only_held_portion() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = alice(); // high-security from genesis
		let recipient: AccountId = dave();
		let third_party: AccountId = account_id(9);
		let asset_id: u32 = 777;

		create_asset(asset_id, sender.clone(), None);
		let sender_before = asset_balance(asset_id, &sender);
		let third_before = asset_balance(asset_id, &third_party);
		let recipient_before = asset_balance(asset_id, &recipient);

		// Place a hold smaller than free so some spend is still allowed
		let hold_amount: Balance = sender_before / 10; // 10%
		assert_ok!(ReversibleTransfers::schedule_asset_transfer(
			RuntimeOrigin::signed(sender.clone()),
			asset_id,
			recipient.clone(),
			hold_amount,
		));
		assert_eq!(asset_holds(asset_id, &sender), hold_amount);
		assert_eq!(asset_balance(asset_id, &sender), sender_before - hold_amount);
		assert_eq!(asset_balance(asset_id, &recipient), recipient_before);

		// Attempt to cross the held barrier by 1; must fail
		let over = (sender_before - hold_amount).saturating_add(1);
		assert_err!(
			pallet_assets::Pallet::<Test>::transfer_keep_alive(
				RuntimeOrigin::signed(sender.clone()),
				codec::Compact(asset_id),
				third_party.clone(),
				over,
			),
			pallet_assets::Error::<Test>::BalanceLow
		);

		// Spend the free amount but keep account alive with min.
		let free_amount = sender_before - hold_amount;
		let min = <pallet_assets::Pallet<Test> as AssetsInspect<_>>::minimum_balance(asset_id);
		let spend = free_amount.saturating_sub(min);
		assert_ok!(pallet_assets::Pallet::<Test>::transfer_keep_alive(
			RuntimeOrigin::signed(sender.clone()),
			codec::Compact(asset_id),
			third_party.clone(),
			spend,
		));
		assert_eq!(asset_balance(asset_id, &sender), min);
		assert_eq!(asset_balance(asset_id, &third_party), third_before + spend);
	});
}

#[test]
fn asset_hold_prevents_spend_over_free() {
	// Testing asset hold because it was quite confusing in code
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = charlie(); // has system account in genesis
		let recipient: AccountId = dave(); // has system account in genesis
		let asset_id: u32 = 808;

		// Create asset and give sender 20 units
		create_asset(asset_id, sender.clone(), Some(20));

		// Create a 10-unit hold by scheduling an asset transfer with one-time delay (sender is not
		// high-security)
		assert_ok!(ReversibleTransfers::schedule_asset_transfer_with_delay(
			RuntimeOrigin::signed(sender.clone()),
			asset_id,
			recipient.clone(),
			10,
			BlockNumberOrTimestamp::BlockNumber(5),
		));
		assert_eq!(asset_holds(asset_id, &sender), 10);

		// Attempt to send 15 (free is only 10 after hold); must fail with BalanceLow
		assert_err!(
			pallet_assets::Pallet::<Test>::transfer_keep_alive(
				RuntimeOrigin::signed(sender.clone()),
				codec::Compact(asset_id),
				recipient.clone(),
				15,
			),
			pallet_assets::Error::<Test>::BalanceLow
		);
	});
}

#[test]
fn schedule_transfer_with_error_short_delay() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let sender: AccountId = charlie();
		let recipient: AccountId = dave();
		let amount: Balance = 1000;
		let custom_delay = BlockNumberOrTimestamp::BlockNumber(1);

		assert_err!(
			ReversibleTransfers::schedule_transfer_with_delay(
				RuntimeOrigin::signed(sender.clone()),
				recipient.clone(),
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
		let sender: AccountId = charlie();
		let recipient: AccountId = dave();
		let amount: Balance = 1000;
		let custom_delay_blocks = 10;
		let custom_delay = BlockNumberOrTimestamp::BlockNumber(custom_delay_blocks);

		let initial_sender_balance = Balances::free_balance(&sender);
		let initial_recipient_balance = Balances::free_balance(&recipient);

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(sender.clone(), &call);

		// Schedule the transfer
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
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
		assert_eq!(Balances::free_balance(&sender), initial_sender_balance - amount);
		assert_eq!(Balances::free_balance(&recipient), initial_recipient_balance + amount);

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

		let sender: AccountId = charlie();
		let recipient: AccountId = dave();
		let amount: Balance = 1000;
		let one_minute_ms = 1000 * 60;
		let custom_delay_ms = 10 * one_minute_ms; // 10 minutes
		let custom_delay = BlockNumberOrTimestamp::Timestamp(custom_delay_ms);

		let initial_sender_balance = Balances::free_balance(&sender);
		let initial_recipient_balance = Balances::free_balance(&recipient);

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(sender.clone(), &call);

		// Schedule the transfer
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
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
		assert_eq!(Balances::free_balance(&sender), initial_sender_balance - amount);

		// recipient balance not yet changed
		assert_eq!(Balances::free_balance(&recipient), initial_recipient_balance);

		// Set time past execution time
		MockTimestamp::<Test>::set_timestamp(1_000_000 + custom_delay_ms + 1);
		let execute_block = System::block_number() + 2;
		run_to_block(execute_block);

		// Check that the transfer was executed
		assert_eq!(Balances::free_balance(&sender), initial_sender_balance - amount);
		assert_eq!(Balances::free_balance(&recipient), initial_recipient_balance + amount);

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
		let sender: AccountId = alice(); // delay of 10
		let recipient: AccountId = dave();
		let amount: Balance = 1000;

		// Initially no pending transfers
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 0);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 0);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 0);

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(sender.clone(), &call);

		// Schedule transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
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
		let call2 = transfer_call(recipient.clone(), amount2);
		let tx_id2 = calculate_tx_id::<Test>(sender.clone(), &call2);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
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
		let sender: AccountId = charlie();
		let recipient: AccountId = dave();
		let amount: Balance = 1000;
		let delay_blocks = 10;

		System::set_block_number(start_block);

		// Schedule a transfer
		assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
			amount,
			BlockNumberOrTimestamp::BlockNumber(delay_blocks),
		));

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(sender.clone(), &call);

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
		let sender: AccountId = alice();
		let recipient: AccountId = dave();
		let amount: Balance = 1000;

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(sender.clone(), &call);

		// Schedule a transfer
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
			amount,
		));

		// Verify storage indexes are populated
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 1);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 1);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Cancel the transfer
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(bob()), // interceptor from genesis config
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
		let sender: AccountId = alice(); // delay of 10
		let recipient: AccountId = dave();
		let amount: Balance = 1000;

		let call = transfer_call(recipient.clone(), amount);
		let tx_id = calculate_tx_id::<Test>(sender.clone(), &call);

		// Schedule the same transfer twice (identical transfers)
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
			amount,
		));

		let tx_id1 = calculate_tx_id::<Test>(sender.clone(), &call);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender.clone()),
			recipient.clone(),
			amount
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
			RuntimeOrigin::signed(bob()), // interceptor from genesis config
			tx_id
		));

		// Indexes should still contain the transfer (since count > 1)
		assert_eq!(ReversibleTransfers::pending_transfers_by_sender(&sender).len(), 1);
		assert_eq!(ReversibleTransfers::pending_transfers_by_recipient(&recipient).len(), 1);
		assert_eq!(ReversibleTransfers::account_pending_index(&sender), 1);

		// Cancel the last instance
		assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(bob()), tx_id1));

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
		let sender: AccountId = alice();
		let recipient1: AccountId = dave();
		let recipient2: AccountId = eve();
		let amount: Balance = 1000;

		let call1 = transfer_call(recipient1.clone(), amount);
		let tx_id1 = calculate_tx_id::<Test>(sender.clone(), &call1);

		// Schedule transfers to different recipients
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender.clone()),
			recipient1.clone(),
			amount,
		));

		let call2 = transfer_call(recipient2.clone(), amount);
		let tx_id2 = calculate_tx_id::<Test>(sender.clone(), &call2);

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(sender.clone()),
			recipient2.clone(),
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
			RuntimeOrigin::signed(bob()), // interceptor from genesis config
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
		let reversible_account = account_id(100);
		let interceptor = account_id(101);
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Initially, interceptor should have empty list
		assert_eq!(ReversibleTransfers::interceptor_index(&interceptor).len(), 0);

		// Set up reversibility with explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			delay,
			interceptor.clone(),
		));

		// Verify interceptor index is updated
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 1);
		assert_eq!(interceptor_accounts[0], reversible_account.clone());

		// Verify account has correct reversibility data
		assert_eq!(
			ReversibleTransfers::is_high_security(&reversible_account),
			Some(HighSecurityAccountData { delay, interceptor: interceptor.clone() })
		);
	});
}

#[test]
fn interceptor_index_handles_multiple_accounts() {
	new_test_ext().execute_with(|| {
		let interceptor = account_id(100);
		let account1 = account_id(101);
		let account2 = account_id(102);
		let account3 = account_id(103);
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up multiple accounts with same interceptor
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account1.clone()),
			delay,
			interceptor.clone(),
		));

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account2.clone()),
			delay,
			interceptor.clone(),
		));

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account3.clone()),
			delay,
			interceptor.clone(),
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
		let reversible_account = account_id(100);
		let interceptor = account_id(101);
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up reversibility with explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			delay,
			interceptor.clone(),
		));

		// Verify initial state
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 1);
		assert_eq!(interceptor_accounts[0], reversible_account.clone());

		// Try to add the same account again (this should fail due to AccountAlreadyReversible)
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(reversible_account.clone()),
				delay,
				interceptor.clone(),
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
		let interceptor = account_id(100);
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Add accounts up to the limit (MaxInterceptorAccounts = 10 in mock)
		for i in 101..=110 {
			assert_ok!(ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(account_id(i)),
				delay,
				interceptor.clone(),
			));
		}

		// Verify we have the maximum number of accounts
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 10);

		// Try to add one more account - should fail
		assert_err!(
			ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(account_id(111)),
				delay,
				interceptor.clone(),
			),
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
		let non_interceptor = account_id(100);
		let reversible_account = account_id(101);
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up account without explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			delay,
			account_id(201),
		));

		// Verify non-interceptor has empty list
		assert_eq!(ReversibleTransfers::interceptor_index(&non_interceptor).len(), 0);
		assert_eq!(ReversibleTransfers::interceptor_index(&reversible_account).len(), 0);
	});
}

#[test]
fn interceptor_index_different_interceptors_separate_lists() {
	new_test_ext().execute_with(|| {
		let interceptor1 = account_id(101);
		let interceptor2 = account_id(102);
		let account1 = account_id(102);
		let account2 = account_id(103);
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up accounts with different interceptors
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account1.clone()),
			delay,
			interceptor1.clone(),
		));

		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account2.clone()),
			delay,
			interceptor2.clone(),
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
		let reversible_account = account_id(100);
		let interceptor = account_id(101);
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		// Set up reversibility with Intercept policy and explicit reverser
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			delay,
			interceptor.clone(),
		));

		// Verify interceptor index is updated regardless of policy
		let interceptor_accounts = ReversibleTransfers::interceptor_index(&interceptor);
		assert_eq!(interceptor_accounts.len(), 1);
		assert_eq!(interceptor_accounts[0], reversible_account.clone());

		// Verify account has correct policy
		assert_eq!(
			ReversibleTransfers::is_high_security(&reversible_account),
			Some(HighSecurityAccountData { delay, interceptor: interceptor.clone() })
		);
	});
}

#[test]
fn global_nonce_works() {
	new_test_ext().execute_with(|| {
		let nonce = ReversibleTransfers::global_nonce();
		assert_eq!(nonce, 0);

		// Perform a reversible transfer
		let reversible_account = account_id(100);
		let receiver = account_id(101);
		let amount = 100;
		let delay = BlockNumberOrTimestamp::BlockNumber(10);

		let interceptor = account_id(201);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(reversible_account.clone()),
			delay,
			interceptor.clone(),
		));

		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(reversible_account.clone()),
			receiver.clone(),
			amount,
		));

		let nonce = ReversibleTransfers::global_nonce();
		assert_eq!(nonce, 1);

		// batch call should have all unique tx ids and increment nonce
		assert_ok!(Utility::batch(
			RuntimeOrigin::signed(reversible_account.clone()),
			vec![
				ReversibleTransfersCall::schedule_transfer { dest: receiver.clone(), amount }
					.into(),
				ReversibleTransfersCall::schedule_transfer {
					dest: receiver.clone(),
					amount: amount + 1
				}
				.into(),
				ReversibleTransfersCall::schedule_transfer {
					dest: receiver.clone(),
					amount: amount + 2
				}
				.into(),
			],
		));

		assert_eq!(ReversibleTransfers::global_nonce(), 4);
	});
}
