use crate::common::TestCommons;
use frame_support::{assert_err, assert_ok, traits::Currency};
use qp_scheduler::BlockNumberOrTimestamp;
use quantus_runtime::{Balances, ReversibleTransfers, RuntimeOrigin, System, EXISTENTIAL_DEPOSIT};
use sp_runtime::MultiAddress;

fn acc(n: u8) -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(n)
}

fn high_security_account() -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(1)
}
fn guardian() -> sp_core::crypto::AccountId32 {
	TestCommons::account_id(2)
}

#[test]
fn high_security_end_to_end_flow() {
	// Accounts:
	// 1 = HS account (sender)
	// 2 = guardian
	// 3 = third party (friend)
	// 4 = recipient of the initial transfer
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
        // Set block number to 1 so events are deposited
        System::set_block_number(1);

        // Initial balances snapshot
        let hs_start = Balances::free_balance(high_security_account());
        let guardian_start = Balances::free_balance(guardian());
        let a4_start = Balances::free_balance(acc(4));

        // 1) Enable high-security for account 1
        // Use a small delay in blocks for reversible transfers; recovery delay must be >= 7 DAYS
        let hs_delay = BlockNumberOrTimestamp::BlockNumber(5);
        assert_ok!(ReversibleTransfers::set_high_security(
            RuntimeOrigin::signed(high_security_account()),
            hs_delay,
            guardian(),
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
        assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(guardian()), tx_id));

        // Funds should have been moved from 1 to 2 (transfer_on_hold). 4 didn't receive anything.
        let hs_after_cancel = Balances::free_balance(high_security_account());
        let guardian_after_cancel = Balances::free_balance(guardian());
        let a4_after_cancel = Balances::free_balance(acc(4));

        assert!(hs_after_cancel <= hs_start - amount, "sender should lose at least the scheduled amount");
        // With volume fee: amount = 10 * EXISTENTIAL_DEPOSIT = 10_000_000_000
        // Fee (1%): 10_000_000_000 * 1 / 100 = 100_000_000
        // Remaining to guardian: 10_000_000_000 - 100_000_000 = 9_900_000_000
        let expected_fee = amount / 100; // 1% 
        let expected_amount_to_guardian = amount - expected_fee;
        assert_eq!(guardian_after_cancel, guardian_start + expected_amount_to_guardian, "guardian should receive the cancelled amount minus volume fee");
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
                guardian(),
            ),
            pallet_reversible_transfers::Error::<quantus_runtime::Runtime>::AccountAlreadyHighSecurity
        );

        // 6) Guardian recovers all funds from high sec account via recover_funds
        let guardian_before_recovery = Balances::free_balance(guardian());

        assert_ok!(ReversibleTransfers::recover_funds(
            RuntimeOrigin::signed(guardian()),
            high_security_account(),
        ));

        let hs_after_recovery = Balances::free_balance(high_security_account());
        let guardian_after_recovery = Balances::free_balance(guardian());

        // HS account should be drained completely (keep_alive: false)
        assert_eq!(hs_after_recovery, 0);

        // Guardian should have received all the HS account's remaining funds
        assert!(
            guardian_after_recovery > guardian_before_recovery,
            "guardian should have received funds from HS account"
        );
        assert_eq!(
            guardian_after_recovery,
            guardian_before_recovery + hs_after_cancel,
            "guardian should have received the HS account's remaining balance"
        );
    });
}

#[test]
fn test_recover_funds_only_works_for_guardian() {
	// Test that only the guardian can call recover_funds
	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		let delay = BlockNumberOrTimestamp::BlockNumber(5);
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(high_security_account()),
			delay,
			guardian(),
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
		let guardian_balance_before = Balances::free_balance(guardian());

		assert_ok!(ReversibleTransfers::recover_funds(
			RuntimeOrigin::signed(guardian()),
			high_security_account(),
		));

		// Verify funds were transferred
		let hs_balance_after = Balances::free_balance(high_security_account());
		let guardian_balance_after = Balances::free_balance(guardian());

		assert_eq!(hs_balance_after, 0);
		assert_eq!(
			guardian_balance_after,
			guardian_balance_before + hs_balance_before,
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

/// The guardian holds instant, total seizure power (`recover_funds` sweeps
/// every hold plus the whole free balance to it, with no delay and no second
/// approver), so the recommended deployment is a multisig guardian. This pins
/// that the full guardian lifecycle actually works when the guardian is a
/// `pallet_multisig` address: cancelling a pending transfer and recovering
/// funds, each dispatched as the multisig through propose/approve/execute.
#[test]
fn multisig_guardian_protects_high_security_account() {
	use codec::Encode;
	use quantus_runtime::{Multisig, Runtime, RuntimeCall, RuntimeEvent};

	// Dispatch `call` as the multisig via the full 2-of-2 propose/approve/
	// execute round-trip and assert the inner dispatch succeeded.
	fn dispatch_as_multisig(
		multisig_address: &sp_core::crypto::AccountId32,
		proposal_id: u32,
		call: RuntimeCall,
	) {
		let encoded: pallet_multisig::BoundedCallOf<Runtime> = call.encode().try_into().unwrap();
		let expiry = System::block_number() + 100;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(acc(2)),
			multisig_address.clone(),
			encoded.clone(),
			expiry,
		));
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(acc(3)),
			multisig_address.clone(),
			proposal_id,
			encoded,
		));
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(acc(2)),
			multisig_address.clone(),
			proposal_id,
		));
		let inner_result = System::events()
			.iter()
			.rev()
			.find_map(|record| {
				if let RuntimeEvent::Multisig(pallet_multisig::Event::ProposalExecuted {
					proposal_id: id,
					result,
					..
				}) = &record.event
				{
					(*id == proposal_id).then_some(*result)
				} else {
					None
				}
			})
			.expect("ProposalExecuted event should be emitted");
		assert_ok!(inner_result);
	}

	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		System::set_block_number(1);

		// Accounts 2 and 3 form the 2-of-2 guardian multisig; account 1 is
		// the protected user, account 4 the transfer recipient.
		let signers = vec![acc(2), acc(3)];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(acc(2)), signers.clone(), 2, 0));
		let guardian_multisig = Multisig::derive_multisig_address(&signers, 2, 0);
		// The multisig account must exist to receive a recovery sweep.
		assert_ok!(Balances::transfer_keep_alive(
			RuntimeOrigin::signed(acc(3)),
			MultiAddress::Id(guardian_multisig.clone()),
			10 * EXISTENTIAL_DEPOSIT,
		));

		// Enroll with the multisig address as guardian.
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(acc(1)),
			BlockNumberOrTimestamp::BlockNumber(5),
			guardian_multisig.clone(),
		));
		assert_eq!(
			ReversibleTransfers::is_high_security(&acc(1)).map(|data| data.guardian),
			Some(guardian_multisig.clone())
		);

		// The multisig cancels a pending transfer during the delay window.
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(acc(1)),
			MultiAddress::Id(acc(4)),
			10 * EXISTENTIAL_DEPOSIT,
		));
		let tx_id = System::events()
			.iter()
			.find_map(|record| {
				if let RuntimeEvent::ReversibleTransfers(
					pallet_reversible_transfers::Event::TransactionScheduled { tx_id, .. },
				) = &record.event
				{
					Some(*tx_id)
				} else {
					None
				}
			})
			.expect("TransactionScheduled event should be emitted");
		dispatch_as_multisig(
			&guardian_multisig,
			0,
			RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::<Runtime>::cancel { tx_id },
			),
		);
		assert!(
			pallet_reversible_transfers::PendingTransfers::<Runtime>::get(tx_id).is_none(),
			"multisig guardian cancel must remove the pending transfer"
		);
		let recipient_start = 1000 * quantus_runtime::UNIT;
		assert_eq!(Balances::free_balance(acc(4)), recipient_start, "recipient got nothing");

		// The multisig seizes the account: holds and free balance sweep to it.
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(acc(1)),
			MultiAddress::Id(acc(4)),
			10 * EXISTENTIAL_DEPOSIT,
		));
		let user_funds_before = Balances::free_balance(acc(1));
		let multisig_funds_before = Balances::free_balance(&guardian_multisig);
		dispatch_as_multisig(
			&guardian_multisig,
			1,
			RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::<Runtime>::recover_funds { account: acc(1) },
			),
		);
		assert_eq!(Balances::free_balance(acc(1)), 0, "recovery must drain the protected account");
		assert!(
			Balances::free_balance(&guardian_multisig) > multisig_funds_before + user_funds_before,
			"the sweep (free balance plus released holds, less the volume fee) \
			 must land on the multisig"
		);
	});
}

/// A multisig guardian cannot be locked out by the high-security transaction
/// quota, even when the multisig address is itself enrolled as high-security
/// and its quota ring is completely full.
///
/// The quota lives in `ReversibleTransactionExtension` and keys on the *outer
/// signer* of each extrinsic. A multisig acts through `propose` / `approve` /
/// `execute` signed by its individual signers — the derived address never
/// signs anything, so its ring is never consulted. (An HS multisig is instead
/// constrained by pallet-multisig's propose-time whitelist check, which admits
/// `cancel` and `recover_funds`.) A single-key high-security guardian does
/// share its quota with its own traffic — that limitation is documented, and
/// this test pins that the recommended multisig deployment is immune.
///
/// Everything guardian-side runs through the full signed pipeline
/// (`Executive::apply_extrinsic`), which the quota actually gates.
#[test]
fn high_security_multisig_guardian_is_immune_to_quota_lockout() {
	use codec::Encode;
	use qp_dilithium_crypto::Dilithium65Pair;
	use quantus_runtime::{Executive, Multisig, Runtime, RuntimeCall, RuntimeEvent, UNIT};
	use sp_core::Pair;
	use sp_runtime::traits::IdentifyAccount;

	let signer_a_pair = Dilithium65Pair::from_seed_slice(&[52u8; 32]).expect("valid seed");
	let signer_b_pair = Dilithium65Pair::from_seed_slice(&[53u8; 32]).expect("valid seed");
	let signer_a = signer_a_pair.public().into_account();
	let signer_b = signer_b_pair.public().into_account();

	// Sign with the production extension tuple and require both inclusion and
	// successful dispatch.
	fn apply_signed(
		pair: &Dilithium65Pair,
		sender: sp_core::crypto::AccountId32,
		call: RuntimeCall,
		nonce: u32,
	) {
		let xt = TestCommons::signed_extrinsic(pair, sender, call, nonce, 0);
		let outcome =
			Executive::apply_extrinsic(xt).expect("guardian-side extrinsic must pass validation");
		assert_ok!(outcome);
	}

	fn assert_proposal_executed_ok(proposal_id: u32) {
		let inner_result = System::events()
			.iter()
			.rev()
			.find_map(|record| {
				if let RuntimeEvent::Multisig(pallet_multisig::Event::ProposalExecuted {
					proposal_id: id,
					result,
					..
				}) = &record.event
				{
					(*id == proposal_id).then_some(*result)
				} else {
					None
				}
			})
			.expect("ProposalExecuted event should be emitted");
		assert_ok!(inner_result);
	}

	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		System::set_block_number(1);
		Balances::make_free_balance_be(&signer_a, 1000 * UNIT);
		Balances::make_free_balance_be(&signer_b, 1000 * UNIT);

		// 2-of-2 multisig of the Dilithium signers, funded so it can receive
		// a recovery sweep.
		let signers = vec![signer_a.clone(), signer_b.clone()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(signer_a.clone()),
			signers.clone(),
			2,
			0
		));
		let guardian_multisig = Multisig::derive_multisig_address(&signers, 2, 0);
		assert_ok!(Balances::transfer_keep_alive(
			RuntimeOrigin::signed(signer_b.clone()),
			MultiAddress::Id(guardian_multisig.clone()),
			10 * EXISTENTIAL_DEPOSIT,
		));

		// The multisig itself enrolls as high-security...
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(guardian_multisig.clone()),
			BlockNumberOrTimestamp::BlockNumber(5),
			acc(3),
		));
		// ...and its own quota ring is filled to the brim: a single-key
		// guardian in this state would be mute for up to a day.
		while ReversibleTransfers::high_security_tx_quota(&guardian_multisig).len() < 16 {
			assert_ok!(ReversibleTransfers::record_high_security_tx(&guardian_multisig));
		}
		assert!(!ReversibleTransfers::high_security_tx_quota_allows(&guardian_multisig));

		// The protected user enrolls with the multisig as guardian and
		// schedules a transfer.
		assert_ok!(ReversibleTransfers::set_high_security(
			RuntimeOrigin::signed(acc(1)),
			BlockNumberOrTimestamp::BlockNumber(5),
			guardian_multisig.clone(),
		));
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(acc(1)),
			MultiAddress::Id(acc(4)),
			10 * EXISTENTIAL_DEPOSIT,
		));
		let tx_id = System::events()
			.iter()
			.find_map(|record| {
				if let RuntimeEvent::ReversibleTransfers(
					pallet_reversible_transfers::Event::TransactionScheduled { tx_id, .. },
				) = &record.event
				{
					Some(*tx_id)
				} else {
					None
				}
			})
			.expect("TransactionScheduled event should be emitted");

		// Cancel through the full signed pipeline: propose (signer A),
		// approve (signer B), execute (signer A).
		let cancel = RuntimeCall::ReversibleTransfers(
			pallet_reversible_transfers::Call::<Runtime>::cancel { tx_id },
		);
		let encoded_cancel: pallet_multisig::BoundedCallOf<Runtime> =
			cancel.encode().try_into().unwrap();
		let expiry = System::block_number() + 100;
		apply_signed(
			&signer_a_pair,
			signer_a.clone(),
			RuntimeCall::Multisig(pallet_multisig::Call::propose {
				multisig_address: guardian_multisig.clone(),
				call: encoded_cancel.clone(),
				expiry,
			}),
			0,
		);
		apply_signed(
			&signer_b_pair,
			signer_b.clone(),
			RuntimeCall::Multisig(pallet_multisig::Call::approve {
				multisig_address: guardian_multisig.clone(),
				proposal_id: 0,
				call: encoded_cancel,
			}),
			0,
		);
		apply_signed(
			&signer_a_pair,
			signer_a.clone(),
			RuntimeCall::Multisig(pallet_multisig::Call::execute {
				multisig_address: guardian_multisig.clone(),
				proposal_id: 0,
			}),
			1,
		);
		assert_proposal_executed_ok(0);
		assert!(
			pallet_reversible_transfers::PendingTransfers::<Runtime>::get(tx_id).is_none(),
			"quota-full HS multisig guardian must still cancel via the signed pipeline"
		);

		// Recover through the same pipeline.
		assert_ok!(ReversibleTransfers::schedule_transfer(
			RuntimeOrigin::signed(acc(1)),
			MultiAddress::Id(acc(4)),
			10 * EXISTENTIAL_DEPOSIT,
		));
		let recover =
			RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::<Runtime>::recover_funds { account: acc(1) },
			);
		let encoded_recover: pallet_multisig::BoundedCallOf<Runtime> =
			recover.encode().try_into().unwrap();
		apply_signed(
			&signer_a_pair,
			signer_a.clone(),
			RuntimeCall::Multisig(pallet_multisig::Call::propose {
				multisig_address: guardian_multisig.clone(),
				call: encoded_recover.clone(),
				expiry,
			}),
			2,
		);
		apply_signed(
			&signer_b_pair,
			signer_b.clone(),
			RuntimeCall::Multisig(pallet_multisig::Call::approve {
				multisig_address: guardian_multisig.clone(),
				proposal_id: 1,
				call: encoded_recover,
			}),
			1,
		);
		apply_signed(
			&signer_a_pair,
			signer_a.clone(),
			RuntimeCall::Multisig(pallet_multisig::Call::execute {
				multisig_address: guardian_multisig.clone(),
				proposal_id: 1,
			}),
			3,
		);
		assert_proposal_executed_ok(1);
		assert_eq!(Balances::free_balance(acc(1)), 0, "recovery must drain the protected account");

		// The multisig's own ring was never consulted or touched: still full.
		assert_eq!(ReversibleTransfers::high_security_tx_quota(&guardian_multisig).len(), 16);
		assert!(!ReversibleTransfers::high_security_tx_quota_allows(&guardian_multisig));
	});
}
