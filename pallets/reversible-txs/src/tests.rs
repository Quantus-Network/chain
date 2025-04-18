#![cfg(test)]

use super::*; // Import items from parent module (lib.rs)
use crate::mock::*; // Import mock runtime and types
use frame_support::traits::fungible::InspectHold;
use frame_support::traits::StorePreimage;
use frame_support::{assert_err, assert_ok};
use pallet_scheduler::Agenda;
use sp_core::H256;
use sp_runtime::traits::{BadOrigin, BlakeTwo256, Hash};

// Helper function to create a transfer call
fn transfer_call(dest: AccountId, amount: Balance) -> RuntimeCall {
    RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
        dest,
        value: amount,
    })
}

// Helper function to calculate TxId (matching the logic in schedule_transfer)
fn calculate_tx_id(who: AccountId, call: &RuntimeCall) -> H256 {
    BlakeTwo256::hash_of(&(who, call).encode())
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
fn set_reversibility_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let genesis_user = 1;

        // Check initial state
        assert_eq!(
            ReversibleTxs::is_reversible(&genesis_user),
            Some((10, DelayPolicy::Explicit))
        );

        // Set the delay
        let another_user = 3;
        let delay = 5u64;
        assert_ok!(ReversibleTxs::set_reversibility(
            RuntimeOrigin::signed(another_user),
            Some(delay),
            DelayPolicy::Intercept
        ));
        assert_eq!(
            ReversibleTxs::is_reversible(&another_user),
            Some((delay, DelayPolicy::Intercept))
        );
        System::assert_last_event(
            Event::ReversibilitySet {
                who: another_user,
                delay,
                policy: DelayPolicy::Intercept,
            }
            .into(),
        );

        // Calling this again should err
        assert_err!(
            ReversibleTxs::set_reversibility(
                RuntimeOrigin::signed(another_user),
                Some(delay),
                DelayPolicy::Intercept
            ),
            Error::<Test>::AccountAlreadyReversible
        );

        // Use default delay
        let default_user = 5;
        assert_ok!(ReversibleTxs::set_reversibility(
            RuntimeOrigin::signed(default_user),
            None,
            DelayPolicy::Explicit
        ));
        assert_eq!(
            ReversibleTxs::is_reversible(&default_user),
            Some((DefaultDelay::get(), DelayPolicy::Explicit))
        );
        System::assert_last_event(
            Event::ReversibilitySet {
                who: default_user,
                delay: DefaultDelay::get(),
                policy: DelayPolicy::Explicit,
            }
            .into(),
        );

        // Too short delay
        let short_delay = MinDelayPeriod::get() - 1;

        let new_user = 4;
        assert_err!(
            ReversibleTxs::set_reversibility(
                RuntimeOrigin::signed(new_user),
                Some(short_delay),
                DelayPolicy::Explicit
            ),
            Error::<Test>::DelayTooShort
        );
        assert_eq!(ReversibleTxs::is_reversible(&new_user), None);
    });
}

#[test]
fn set_reversibility_fails_delay_too_short() {
    new_test_ext().execute_with(|| {
        let user = 2; // User 2 is not reversible initially
        let short_delay = MinDelayPeriod::get() - 1;

        assert_err!(
            ReversibleTxs::set_reversibility(
                RuntimeOrigin::signed(user),
                Some(short_delay),
                DelayPolicy::Explicit
            ),
            Error::<Test>::DelayTooShort
        );
        assert_eq!(ReversibleTxs::is_reversible(&user), None);
    });
}

#[test]
fn schedule_transfer_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let user = 1; // Reversible from genesis
        let dest_user = 2;
        let amount = 100;
        let call = transfer_call(dest_user, amount);
        let tx_id = calculate_tx_id(user, &call);
        let (user_delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let expected_block = System::block_number() + user_delay;
        let bounded = Preimage::bound(call.clone()).unwrap();

        assert!(Agenda::<Test>::get(expected_block).len() == 0);

        // Simulate call from SignedExtension
        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest_user,
            amount,
        ));

        // Check storage
        assert_eq!(
            PendingTransfers::<Test>::get(tx_id).unwrap(),
            PendingTransfer {
                who: user,
                call: bounded,
                amount,
                count: 1,
            }
        );
        assert_eq!(ReversibleTxs::account_pending_index(user), 1);

        // Check scheduler
        assert!(Agenda::<Test>::get(expected_block).len() > 0);
    });
}

#[test]
fn schedule_transfer_fails_not_reversible() {
    new_test_ext().execute_with(|| {
        let user = 2; // Not reversible

        assert_err!(
            ReversibleTxs::schedule_transfer(RuntimeOrigin::signed(user), 3, 50),
            Error::<Test>::AccountNotReversible
        );
    });
}

#[test]
fn schedule_multiple_transfer_works() {
    new_test_ext().execute_with(|| {
        let user = 1;
        let dest_user = 2;
        let amount = 100;

        // Schedule first
        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest_user,
            amount
        ));

        // Try to schedule the same call again
        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest_user,
            amount
        ));

        // Check that the count of pending transactions for the user is 2
        assert_eq!(ReversibleTxs::account_pending_index(user), 2);
        // Check that the pending transactions are stored correctly
        let tx_id = calculate_tx_id(user, &transfer_call(dest_user, amount));
        let pending = PendingTransfers::<Test>::get(tx_id).unwrap();
        assert_eq!(pending.count, 2);

        // Check that the pending transaction count decreases to 1
        assert_ok!(ReversibleTxs::cancel(RuntimeOrigin::signed(user), tx_id));
        assert_eq!(ReversibleTxs::account_pending_index(user), 1);

        // Check that the pending transaction count decreases to 0 when executed
        let execute_block = System::block_number() + 10;
        run_to_block(execute_block);

        assert_eq!(ReversibleTxs::account_pending_index(user), 0);
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_none());
    });
}

#[test]
fn schedule_transfer_fails_too_many_pending() {
    new_test_ext().execute_with(|| {
        let user = 1;
        let max_pending = MaxReversibleTxs::get();

        // Fill up pending slots
        for i in 0..max_pending {
            assert_ok!(ReversibleTxs::schedule_transfer(
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
            ReversibleTxs::schedule_transfer(RuntimeOrigin::signed(user), 3, 100),
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
        let tx_id = calculate_tx_id(user, &call);
        let (user_delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let execute_block = System::block_number() + user_delay;

        assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

        // Schedule first
        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            2,
            50
        ));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());
        assert!(!ReversibleTxs::account_pending_index(user).is_zero());

        // Check the expected block agendas count
        assert_eq!(Agenda::<Test>::get(execute_block).len(), 1);

        // Now cancel
        assert_ok!(ReversibleTxs::cancel(RuntimeOrigin::signed(user), tx_id));

        // Check state cleared
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_none());
        assert!(ReversibleTxs::account_pending_index(user).is_zero());

        assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

        // Check event
        System::assert_last_event(Event::TransactionCancelled { who: user, tx_id }.into());
    });
}

#[test]
fn cancel_dispatch_fails_not_owner() {
    new_test_ext().execute_with(|| {
        let owner = 1;
        let attacker = 3;
        let call = transfer_call(2, 50);
        let tx_id = calculate_tx_id(owner, &call);

        // Schedule as owner
        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(owner),
            2,
            50
        ));

        // Attacker tries to cancel
        assert_err!(
            ReversibleTxs::cancel(RuntimeOrigin::signed(attacker), tx_id),
            Error::<Test>::NotOwner
        );

        // Check state not affected
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());
    });
}

#[test]
fn cancel_dispatch_fails_not_found() {
    new_test_ext().execute_with(|| {
        let user = 1;
        let non_existent_tx_id = H256::random();

        assert_err!(
            ReversibleTxs::cancel(RuntimeOrigin::signed(user), non_existent_tx_id),
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
        let tx_id = calculate_tx_id(user, &call);
        let (delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let execute_block = System::block_number() + delay;

        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest,
            amount
        ));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());

        run_to_block(execute_block - 1);

        // Execute the dispatch as a normal user. This should fail
        // because the origin should be `Signed(PalletId::into_account())`
        assert_err!(
            ReversibleTxs::execute_transfer(RuntimeOrigin::signed(user), tx_id),
            Error::<Test>::InvalidSchedulerOrigin,
        );

        // Check state cleared
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());

        // Even root origin should fail
        assert_err!(
            ReversibleTxs::execute_transfer(RuntimeOrigin::root(), tx_id),
            BadOrigin
        );
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
        let tx_id = calculate_tx_id(user, &call);
        let (delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let start_block = System::block_number();
        let execute_block = start_block + delay;

        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest,
            amount
        ));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());
        assert!(Agenda::<Test>::get(execute_block).len() > 0);
        assert_eq!(Balances::free_balance(user), initial_user_balance - 50); // Not executed yet, but on hold

        run_to_block(execute_block);

        // Event should be emitted by execute_transfer called by scheduler
        let expected_event = Event::TransactionExecuted {
            tx_id,
            result: Ok(().into()).into(),
        };
        assert!(
            System::events()
                .iter()
                .any(|rec| rec.event == expected_event.clone().into()),
            "Execute event not found"
        );

        assert_eq!(Balances::free_balance(user), initial_user_balance - amount);
        assert_eq!(Balances::free_balance(dest), initial_dest_balance + amount);

        assert!(ReversibleTxs::pending_dispatches(tx_id).is_none());
        assert!(ReversibleTxs::account_pending_index(user).is_zero());
        assert_eq!(Agenda::<Test>::get(execute_block).len(), 0); // Task removed after execution
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
        let tx_id = calculate_tx_id(user, &call);
        let (delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let start_block = System::block_number();
        let execute_block = start_block + delay;

        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest,
            amount
        ));
        // Amount is on hold
        assert_eq!(
            Balances::balance_on_hold(
                &RuntimeHoldReason::ReversibleTxs(HoldReason::ScheduledTransfer),
                &user
            ),
            amount
        );

        assert_ok!(ReversibleTxs::cancel(RuntimeOrigin::signed(user), tx_id));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_none());
        assert!(ReversibleTxs::account_pending_index(user).is_zero());

        // Run past the execution block
        run_to_block(execute_block + 1);

        // State is unchanged, amount is released
        // Amount is on hold
        assert_eq!(
            Balances::balance_on_hold(
                &RuntimeHoldReason::ReversibleTxs(HoldReason::ScheduledTransfer),
                &user
            ),
            0
        );
        assert_eq!(Balances::free_balance(user), initial_user_balance);
        assert_eq!(Balances::free_balance(dest), initial_dest_balance);

        // No events were emitted
        let expected_event_pattern = |e: &RuntimeEvent| match e {
            RuntimeEvent::ReversibleTxs(Event::TransactionExecuted { tx_id: tid, .. })
                if *tid == tx_id =>
            {
                true
            }
            _ => false,
        };
        assert!(
            !System::events()
                .iter()
                .any(|rec| expected_event_pattern(&rec.event)),
            "TransactionExecuted event should not exist"
        );
    });
}

/// The case we want to check:
///
/// 1. User 1 schedules a transfer to user 2 with amount 100
/// 2. User 1 schedules a transfer to user 2 with amount 200, after 2 blocks
/// 3. User 1 schedules a transfer to user 2 with amount 300, after 3 blocks
///
/// When the first transfer is executed, we thaw all frozen amounts, and then freeze the new amount again.
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

        let (delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let execute_block1 = System::block_number() + delay;
        let execute_block2 = System::block_number() + delay + 2;
        let execute_block3 = System::block_number() + delay + 3;

        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest,
            amount1
        ));

        System::set_block_number(3);

        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest,
            amount2
        ));

        System::set_block_number(4);

        assert_ok!(ReversibleTxs::schedule_transfer(
            RuntimeOrigin::signed(user),
            dest,
            amount3
        ));

        // Check frozen amounts
        assert_eq!(
            Balances::balance_on_hold(
                &RuntimeHoldReason::ReversibleTxs(HoldReason::ScheduledTransfer),
                &user
            ),
            amount1 + amount2 + amount3
        );
        // Check that the first transfer is executed and the frozen amounts are thawed
        assert_eq!(
            Balances::free_balance(user),
            user_initial_balance - amount1 - amount2 - amount3
        );

        run_to_block(execute_block1);

        // Check that the first transfer is executed and the frozen amounts are thawed
        assert_eq!(
            Balances::free_balance(user),
            user_initial_balance - amount1 - amount2 - amount3
        );
        assert_eq!(Balances::free_balance(dest), dest_initial_balance + amount1);

        // First amount is released
        assert_eq!(
            Balances::balance_on_hold(
                &RuntimeHoldReason::ReversibleTxs(HoldReason::ScheduledTransfer),
                &user
            ),
            amount2 + amount3
        );

        run_to_block(execute_block2);
        // Check that the second transfer is executed and the frozen amounts are thawed
        assert_eq!(
            Balances::free_balance(user),
            user_initial_balance - amount1 - amount2 - amount3
        );

        assert_eq!(
            Balances::free_balance(dest),
            dest_initial_balance + amount1 + amount2
        );

        // Second amount is released
        assert_eq!(
            Balances::balance_on_hold(
                &RuntimeHoldReason::ReversibleTxs(HoldReason::ScheduledTransfer),
                &user
            ),
            amount3
        );
        run_to_block(execute_block3);
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
                &RuntimeHoldReason::ReversibleTxs(HoldReason::ScheduledTransfer),
                &user
            ),
            0
        );

        // Check that the held amounts are released
        assert_eq!(
            Balances::balance_on_hold(
                &RuntimeHoldReason::ReversibleTxs(HoldReason::ScheduledTransfer),
                &user
            ),
            0
        );
    });
}
