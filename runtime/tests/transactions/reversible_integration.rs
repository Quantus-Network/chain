use crate::common::TestCommons;
use frame_support::{assert_err, assert_ok};
use qp_scheduler::BlockNumberOrTimestamp;
use quantus_runtime::{Balances, ReversibleTransfers, RuntimeOrigin, System, EXISTENTIAL_DEPOSIT};
use sp_runtime::MultiAddress;

fn acc(n: u8) -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(n)
}

fn high_security_account() -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(1)
}
fn interceptor() -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(2)
}

#[test]
fn high_security_end_to_end_flow() {
	// Accounts:
	// 1 = HS account (sender)
	// 2 = interceptor/guardian
	// 3 = recoverer (friend)
	// 4 = recipient of the initial transfer
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
        // Set block number to 1 so events are deposited
        System::set_block_number(1);
        
        // Initial balances snapshot
        let hs_start = Balances::free_balance(high_security_account());
        let interceptor_start = Balances::free_balance(interceptor());
        let a4_start = Balances::free_balance(acc(4));

        // 1) Enable high-security for account 1
        // Use a small delay in blocks for reversible transfers; recovery delay must be >= 7 DAYS
        let hs_delay = BlockNumberOrTimestamp::BlockNumber(5);
        assert_ok!(ReversibleTransfers::set_high_security(
            RuntimeOrigin::signed(high_security_account()),
            hs_delay,
            interceptor(), // interceptor
        ));

        // 2) Account 1 makes a normal balances transfer (schedule via pallet extrinsic)
        // NOTE: We exercise the pallet extrinsic path here to avoid manual signature building.
        let amount = 10 * EXISTENTIAL_DEPOSIT;
        assert_ok!(ReversibleTransfers::schedule_transfer(
            RuntimeOrigin::signed(high_security_account()),
            MultiAddress::Id(acc(4)),
            amount,
        ));

        // Verify pending state - extract tx_id from the TransactionScheduled event
        let tx_id = System::events()
            .iter()
            .find_map(|record| {
                if let quantus_runtime::RuntimeEvent::ReversibleTransfers(
                    pallet_reversible_transfers::Event::TransactionScheduled { tx_id, .. }
                ) = &record.event {
                    Some(*tx_id)
                } else {
                    None
                }
            })
            .expect("TransactionScheduled event should be emitted");
        
        // Verify the pending transfer exists
        assert!(
            pallet_reversible_transfers::PendingTransfers::<quantus_runtime::Runtime>::get(tx_id).is_some(),
            "one pending reversible transfer expected"
        );

        // 3) Guardian (account 2) reverses/cancels it on behalf of 1
        assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(interceptor()), tx_id));

        // Funds should have been moved from 1 to 2 (transfer_on_hold). 4 didn't receive anything.
        let hs_after_cancel = Balances::free_balance(high_security_account());
        let interceptor_after_cancel = Balances::free_balance(interceptor());
        let a4_after_cancel = Balances::free_balance(acc(4));

        assert!(hs_after_cancel <= hs_start - amount, "sender should lose at least the scheduled amount");
        // With volume fee: amount = 10 * EXISTENTIAL_DEPOSIT = 10_000_000_000
        // Fee (1%): 10_000_000_000 * 1 / 100 = 100_000_000
        // Remaining to interceptor: 10_000_000_000 - 100_000_000 = 9_900_000_000
        let expected_fee = amount / 100; // 1% 
        let expected_amount_to_interceptor = amount - expected_fee;
        assert_eq!(interceptor_after_cancel, interceptor_start + expected_amount_to_interceptor, "interceptor should receive the cancelled amount minus volume fee");
        assert_eq!(a4_after_cancel, a4_start, "recipient should not receive funds after cancel");

        // 4) HS account tries to schedule a one-time transfer with a custom delay -> should fail
        let different_delay = BlockNumberOrTimestamp::BlockNumber(10);
        assert_err!(
            ReversibleTransfers::schedule_transfer_with_delay(
                RuntimeOrigin::signed(high_security_account()),
                MultiAddress::Id(acc(4)),
                EXISTENTIAL_DEPOSIT,
                different_delay,
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyReversibleCannotScheduleOneTime
        );

        // 5) HS account tries to call set_high_security again -> should fail
        assert_err!(
            ReversibleTransfers::set_high_security(
                RuntimeOrigin::signed(high_security_account()),
                hs_delay,
                interceptor(),
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyHighSecurity
        );

        // 6) Interceptor recovers all funds from high sec account via recover_funds
        let interceptor_before_recovery = Balances::free_balance(interceptor());

        assert_ok!(ReversibleTransfers::recover_funds(
            RuntimeOrigin::signed(interceptor()),
            high_security_account(),
        ));

        let hs_after_recovery = Balances::free_balance(high_security_account());
        let interceptor_after_recovery = Balances::free_balance(interceptor());

        // HS account should be drained completely (keep_alive: false)
        assert_eq!(hs_after_recovery, 0);

        // Interceptor should have received all the HS account's remaining funds
        assert!(
            interceptor_after_recovery > interceptor_before_recovery,
            "interceptor should have received funds from HS account"
        );
        assert_eq!(
            interceptor_after_recovery,
            interceptor_before_recovery + hs_after_cancel,
            "interceptor should have received the HS account's remaining balance"
        );
    });
}

#[test]
fn test_recover_funds_only_works_for_guardian() {
	// Test that only the guardian (interceptor) can call recover_funds
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		let delay = BlockNumberOrTimestamp::BlockNumber(5);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(high_security_account()),
			delay,
			interceptor(),
		));

		// Non-guardian (account 3) tries to recover funds - should fail
		assert_err!(
			ReversibleTransfers::recover_funds(
				RuntimeOrigin::signed(acc(3)),
				high_security_account(),
			),
			pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::InvalidReverser
		);

		// Guardian (account 2) can recover funds
		let hs_balance_before = Balances::free_balance(high_security_account());
		let interceptor_balance_before = Balances::free_balance(interceptor());

		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(interceptor()),
			high_security_account(),
		));

		// Verify funds were transferred
		let hs_balance_after = Balances::free_balance(high_security_account());
		let interceptor_balance_after = Balances::free_balance(interceptor());

		assert_eq!(hs_balance_after, 0);
		assert_eq!(
			interceptor_balance_after,
			interceptor_balance_before + hs_balance_before,
			"guardian should have received all HS account funds"
		);
	});
}

/// Test the chained guardian scenario where a guardian is also a high-security account.
///
/// Chain structure:
/// - Account 1 (HS) -> guardian is Account 2
/// - Account 2 (guardian of 1 + HS) -> guardian is Account 3
/// - Account 3 (guardian of 2, regular account)
///
/// This tests that:
/// 1. An account can be both a guardian AND a high-security account
/// 2. Guardian 2 can cancel transfers from Account 1
/// 3. Guardian 3 can cancel transfers from Account 2
/// 4. Guardian 3 can recover funds from Account 2
/// 5. The volume fee is applied correctly at each level
#[test]
fn chained_guardian_high_security_account_flow() {
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		// Set block number to 1 so events are deposited
		System::set_block_number(1);

		// Account setup:
		// acc(1) = High security account (bottom of chain)
		// acc(2) = Guardian of acc(1) AND also a high security account (middle)
		// acc(3) = Guardian of acc(2) (top of chain, regular account)
		// acc(4) = Recipient for transfers
		let account_1 = acc(1); // HS account
		let account_2 = acc(2); // Guardian of 1 + HS account
		let account_3 = acc(3); // Guardian of 2
		let recipient = acc(4);

		let delay = BlockNumberOrTimestamp::BlockNumber(5);

		// Step 1: Set up account 1 as high-security with account 2 as guardian
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account_1.clone()),
			delay,
			account_2.clone(),
		));

		// Step 2: Set up account 2 as high-security with account 3 as guardian
		// This makes account 2 both a guardian (of account 1) AND a high-security account
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(account_2.clone()),
			delay,
			account_3.clone(),
		));

		// Verify both accounts are now high-security
		assert!(
			pallet_reversible_transfers::Pallet::<quantus_runtime::Runtime>::is_high_security_account(&account_1),
			"Account 1 should be high-security"
		);
		assert!(
			pallet_reversible_transfers::Pallet::<quantus_runtime::Runtime>::is_high_security_account(&account_2),
			"Account 2 should be high-security"
		);
		assert!(
			!pallet_reversible_transfers::Pallet::<quantus_runtime::Runtime>::is_high_security_account(&account_3),
			"Account 3 should NOT be high-security"
		);

		// Verify guardian relationships
		assert_eq!(
			pallet_reversible_transfers::Pallet::<quantus_runtime::Runtime>::get_guardian(&account_1),
			Some(account_2.clone()),
			"Account 2 should be guardian of Account 1"
		);
		assert_eq!(
			pallet_reversible_transfers::Pallet::<quantus_runtime::Runtime>::get_guardian(&account_2),
			Some(account_3.clone()),
			"Account 3 should be guardian of Account 2"
		);

		// Record initial balances
		let _bal_1_start = Balances::free_balance(&account_1);
		let bal_2_start = Balances::free_balance(&account_2);
		let bal_3_start = Balances::free_balance(&account_3);

		// Step 3: Account 1 schedules a transfer
		let amount_1 = 10 * EXISTENTIAL_DEPOSIT;
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(account_1.clone()),
			MultiAddress::Id(recipient.clone()),
			amount_1,
		));

		// Extract tx_id from event
		let tx_id_1 = System::events()
			.iter()
			.rev()
			.find_map(|record| {
				if let quantus_runtime::RuntimeEvent::ReversibleTransfers(
					pallet_reversible_transfers::Event::TransactionScheduled { tx_id, from, .. }
				) = &record.event {
					if from == &account_1 {
						return Some(*tx_id);
					}
				}
				None
			})
			.expect("TransactionScheduled event for account 1 should be emitted");

		// Step 4: Guardian (account 2) cancels the transfer from account 1
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(account_2.clone()),
			tx_id_1
		));

		// Verify account 2 received the funds (minus volume fee)
		let expected_fee_1 = amount_1 / 100; // 1% volume fee
		let expected_to_guardian_1 = amount_1 - expected_fee_1;
		let bal_2_after_cancel = Balances::free_balance(&account_2);
		assert_eq!(
			bal_2_after_cancel,
			bal_2_start + expected_to_guardian_1,
			"Account 2 (guardian) should receive cancelled amount minus volume fee"
		);

		// Clear events for next step
		System::reset_events();

		// Step 5: Account 2 (which is also HS) schedules a transfer
		let amount_2 = 5 * EXISTENTIAL_DEPOSIT;
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(account_2.clone()),
			MultiAddress::Id(recipient.clone()),
			amount_2,
		));

		// Extract tx_id from event
		let tx_id_2 = System::events()
			.iter()
			.rev()
			.find_map(|record| {
				if let quantus_runtime::RuntimeEvent::ReversibleTransfers(
					pallet_reversible_transfers::Event::TransactionScheduled { tx_id, from, .. }
				) = &record.event {
					if from == &account_2 {
						return Some(*tx_id);
					}
				}
				None
			})
			.expect("TransactionScheduled event for account 2 should be emitted");

		// Step 6: Guardian (account 3) cancels the transfer from account 2
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(account_3.clone()),
			tx_id_2
		));

		// Verify account 3 received the funds (minus volume fee)
		let expected_fee_2 = amount_2 / 100; // 1% volume fee
		let expected_to_guardian_2 = amount_2 - expected_fee_2;
		let bal_3_after_cancel = Balances::free_balance(&account_3);
		assert_eq!(
			bal_3_after_cancel,
			bal_3_start + expected_to_guardian_2,
			"Account 3 (guardian) should receive cancelled amount minus volume fee"
		);

		// Step 7: Verify account 1 cannot cancel account 2's transfers (not its guardian)
		System::reset_events();
		
		// Schedule another transfer from account 2
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(account_2.clone()),
			MultiAddress::Id(recipient.clone()),
			amount_2,
		));

		let tx_id_3 = System::events()
			.iter()
			.rev()
			.find_map(|record| {
				if let quantus_runtime::RuntimeEvent::ReversibleTransfers(
					pallet_reversible_transfers::Event::TransactionScheduled { tx_id, from, .. }
				) = &record.event {
					if from == &account_2 {
						return Some(*tx_id);
					}
				}
				None
			})
			.expect("TransactionScheduled event should be emitted");

		// Account 1 tries to cancel account 2's transfer - should fail
		assert_err!(
			ReversibleTransfers::cancel(RuntimeOrigin::signed(account_1.clone()), tx_id_3),
			pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::InvalidReverser
		);

		// But account 3 (the actual guardian) can cancel it
		assert_ok!(ReversibleTransfers::cancel(
			RuntimeOrigin::signed(account_3.clone()),
			tx_id_3
		));

		// Step 8: Test recover_funds chain
		// Account 3 can recover funds from account 2
		let bal_2_before_recovery = Balances::free_balance(&account_2);
		let bal_3_before_recovery = Balances::free_balance(&account_3);

		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(account_3.clone()),
			account_2.clone(),
		));

		assert_eq!(
			Balances::free_balance(&account_2),
			0,
			"Account 2 should be drained after recovery"
		);
		assert_eq!(
			Balances::free_balance(&account_3),
			bal_3_before_recovery + bal_2_before_recovery,
			"Account 3 should receive all of account 2's funds"
		);

		// Step 9: Verify account 2 can still recover from account 1
		// (even though account 2 is now drained, it's still the guardian of account 1)
		let bal_1_before_recovery = Balances::free_balance(&account_1);
		let bal_2_after_own_recovery = Balances::free_balance(&account_2);

		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(account_2.clone()),
			account_1.clone(),
		));

		assert_eq!(
			Balances::free_balance(&account_1),
			0,
			"Account 1 should be drained after recovery"
		);
		assert_eq!(
			Balances::free_balance(&account_2),
			bal_2_after_own_recovery + bal_1_before_recovery,
			"Account 2 should receive all of account 1's funds"
		);
	});
}
