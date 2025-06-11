#[cfg(test)]
mod tests {
    //    use common::TestCommons;
    // Imports from the runtime crate
    use resonance_runtime::configs::{TreasuryPalletId, TreasuryPayoutPeriod};
    use resonance_runtime::governance::pallet_custom_origins;
    use resonance_runtime::{
        AccountId,
        Balance,
        Balances,
        BlockNumber,
        OriginCaller, // Added OriginCaller
        Runtime,
        RuntimeCall,
        RuntimeEvent,
        RuntimeOrigin,
        System,
        TreasuryPallet,
        EXISTENTIAL_DEPOSIT, // DAYS, HOURS are unused, consider removing if not needed elsewhere
        MICRO_UNIT,
        UNIT,
    };
    // Additional pallets for referenda tests
    use resonance_runtime::{ConvictionVoting, Preimage, Referenda, Scheduler};

    // Codec & Hashing
    use codec::Encode;
    use sp_runtime::traits::Hash as RuntimeTraitHash;

    // Frame and Substrate traits & types
    use crate::common::TestCommons;
    use frame_support::{
        assert_ok,
        pallet_prelude::Hooks, // For Scheduler hooks
        traits::{
            schedule::DispatchTime as ScheduleDispatchTime,
            Bounded, // Added Bounded
            Currency,
            PreimageProvider, // Added PreimageProvider
            UnfilteredDispatchable,
        },
    };
    use frame_system::RawOrigin;
    use pallet_referenda::{self, ReferendumIndex, TracksInfo};
    use pallet_treasury;
    use resonance_runtime::governance::definitions::CommunityTracksInfo;
    use sp_runtime::{
        traits::{AccountIdConversion, StaticLookup},
        BuildStorage,
    };

    // Type aliases
    type TestRuntimeCall = RuntimeCall;
    type TestRuntimeOrigin = <TestRuntimeCall as UnfilteredDispatchable>::RuntimeOrigin; // This is available if RuntimeOrigin direct import is an issue

    // Test specific constants
    const BENEFICIARY_ACCOUNT_ID: AccountId = AccountId::new([1u8; 32]); // Example AccountId
    const PROPOSER_ACCOUNT_ID: AccountId = AccountId::new([2u8; 32]); // For referendum proposer
    const VOTER_ACCOUNT_ID: AccountId = AccountId::new([3u8; 32]); // For referendum voter

    // Minimal ExtBuilder for setting up storage
    // In a real project, this would likely be more sophisticated and in common.rs
    pub struct ExtBuilder {
        balances: Vec<(AccountId, Balance)>,
        treasury_genesis: bool,
    }

    impl Default for ExtBuilder {
        fn default() -> Self {
            Self {
                balances: vec![],
                treasury_genesis: true,
            }
        }
    }

    impl ExtBuilder {
        pub fn with_balances(mut self, balances: Vec<(AccountId, Balance)>) -> Self {
            self.balances = balances;
            self
        }

        #[allow(dead_code)]
        pub fn without_treasury_genesis(mut self) -> Self {
            self.treasury_genesis = false;
            self
        }

        pub fn build(self) -> sp_io::TestExternalities {
            let mut t = frame_system::GenesisConfig::<Runtime>::default()
                .build_storage()
                .unwrap();

            pallet_balances::GenesisConfig::<Runtime> {
                balances: self.balances,
            }
            .assimilate_storage(&mut t)
            .unwrap();

            // Pallet Treasury genesis (optional, as we fund it manually)
            // If your pallet_treasury::GenesisConfig needs setup, do it here.
            // For this test, we manually fund the treasury account.

            let mut ext = sp_io::TestExternalities::new(t);
            ext.execute_with(|| System::set_block_number(1));
            ext
        }
    }

    // Helper function to get treasury account ID
    fn treasury_account_id() -> AccountId {
        TreasuryPalletId::get().into_account_truncating()
    }

    /// Tests the basic treasury spend flow:
    /// 1. Root proposes a spend.
    /// 2. Spend is approved.
    /// 3. Beneficiary payouts the spend.
    /// 4. Spend status is checked and spend is removed.
    #[test]
    fn propose_and_payout_spend_as_root_works() {
        ExtBuilder::default()
            .with_balances(vec![])
            .build()
            .execute_with(|| {
                let beneficiary_lookup_source =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();

                let initial_treasury_balance = 1000 * UNIT;
                let spend_amount = 100 * UNIT;

                let _ = <Balances as Currency<AccountId>>::deposit_creating(
                    &treasury_pot,
                    initial_treasury_balance,
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance
                );
                let initial_beneficiary_balance = Balances::free_balance(&BENEFICIARY_ACCOUNT_ID);

                let call =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount,
                        beneficiary: Box::new(beneficiary_lookup_source.clone()),
                        valid_from: None,
                    });

                let dispatch_result = call.dispatch_bypass_filter(RawOrigin::Root.into());
                assert_ok!(dispatch_result);

                let spend_index = 0;

                System::assert_last_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::AssetSpendApproved {
                        index: spend_index,
                        asset_kind: (),
                        amount: spend_amount,
                        beneficiary: BENEFICIARY_ACCOUNT_ID,
                        valid_from: System::block_number(),
                        expire_at: System::block_number() + TreasuryPayoutPeriod::get(),
                    },
                ));

                assert!(
                    pallet_treasury::Spends::<Runtime>::get(spend_index).is_some(),
                    "Spend should exist in storage"
                );

                assert_ok!(TreasuryPallet::payout(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index
                ));

                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::Paid {
                        index: spend_index,
                        payment_id: 0,
                    },
                ));

                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance - spend_amount
                );
                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance + spend_amount
                );

                assert_ok!(TreasuryPallet::check_status(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index
                ));

                System::assert_last_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::SpendProcessed { index: spend_index },
                ));

                assert!(
                    pallet_treasury::Spends::<Runtime>::get(spend_index).is_none(),
                    "Spend should be removed after check_status"
                );
            });
    }

    /// Tests treasury spend functionality using a custom origin (SmallSpender).
    /// 1. SmallSpender proposes a spend within its limit - succeeds.
    /// 2. Beneficiary payouts the spend.
    /// 3. SmallSpender attempts to propose a spend above its limit - fails.
    #[test]
    fn propose_spend_as_custom_origin_works() {
        ExtBuilder::default()
            .with_balances(vec![(BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT)])
            .build()
            .execute_with(|| {
                let beneficiary_lookup_source =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();
                let small_spender_origin: TestRuntimeOrigin =
                    pallet_custom_origins::Origin::SmallSpender.into();

                let initial_treasury_balance = 1000 * UNIT;
                let _ = <Balances as Currency<AccountId>>::deposit_creating(
                    &treasury_pot,
                    initial_treasury_balance,
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance
                );
                let initial_beneficiary_balance = Balances::free_balance(&BENEFICIARY_ACCOUNT_ID);
                assert_eq!(initial_beneficiary_balance, EXISTENTIAL_DEPOSIT);

                let spend_amount_within_limit = 250 * 3 * MICRO_UNIT;
                let call_within_limit =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount_within_limit,
                        beneficiary: Box::new(beneficiary_lookup_source.clone()),
                        valid_from: None,
                    });

                assert_ok!(call_within_limit
                    .clone()
                    .dispatch_bypass_filter(small_spender_origin.clone()));

                let spend_index_within_limit = 0;
                System::assert_last_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::AssetSpendApproved {
                        index: spend_index_within_limit,
                        asset_kind: (),
                        amount: spend_amount_within_limit,
                        beneficiary: BENEFICIARY_ACCOUNT_ID,
                        valid_from: System::block_number(),
                        expire_at: System::block_number() + TreasuryPayoutPeriod::get(),
                    },
                ));
                assert!(
                    pallet_treasury::Spends::<Runtime>::get(spend_index_within_limit).is_some()
                );

                assert_ok!(TreasuryPallet::payout(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index_within_limit
                ));
                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::Paid {
                        index: spend_index_within_limit,
                        payment_id: 0,
                    },
                ));

                assert_ok!(TreasuryPallet::check_status(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index_within_limit
                ));
                System::assert_last_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::SpendProcessed {
                        index: spend_index_within_limit,
                    },
                ));
                assert!(
                    pallet_treasury::Spends::<Runtime>::get(spend_index_within_limit).is_none()
                );

                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance + spend_amount_within_limit
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance - spend_amount_within_limit
                );

                let spend_amount_above_limit = (100 * UNIT) + 1; // Przekroczenie limitu SmallSpender
                let call_above_limit =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount_above_limit,
                        beneficiary: Box::new(beneficiary_lookup_source.clone()),
                        valid_from: None,
                    });

                let dispatch_result_above_limit =
                    call_above_limit.dispatch_bypass_filter(small_spender_origin);
                assert!(
                    dispatch_result_above_limit.is_err(),
                    "Dispatch should fail for amount above SmallSpender limit"
                );

                assert!(
                    pallet_treasury::Spends::<Runtime>::get(spend_index_within_limit + 1).is_none(),
                    "No new spend should be created for the failed attempt"
                );
            });
    }

    /// Tests that a SmallSpender origin cannot approve a spend that exceeds its defined limit.
    /// 1. Fund treasury.
    /// 2. SmallSpender attempts to approve a spend amount greater than its allowance.
    /// 3. Dispatch should fail.
    /// 4. No spend should be created in storage.
    /// 5. Balances should remain unchanged.
    #[test]
    fn small_spender_cannot_spend_above_limit() {
        ExtBuilder::default()
            .with_balances(vec![(BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT)])
            .build()
            .execute_with(|| {
                let beneficiary_lookup =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();
                let small_spender_origin: TestRuntimeOrigin =
                    pallet_custom_origins::Origin::SmallSpender.into();

                let initial_treasury_balance = 1000 * UNIT;
                let _ = <Balances as Currency<AccountId>>::deposit_creating(
                    &treasury_pot,
                    initial_treasury_balance,
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance
                );

                // Try to spend more than SmallSpender's limit (100 * UNIT)
                let spend_amount_above_limit = 101 * UNIT;
                let call_above_limit =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount_above_limit,
                        beneficiary: Box::new(beneficiary_lookup.clone()),
                        valid_from: None,
                    });

                let dispatch_result = call_above_limit.dispatch_bypass_filter(small_spender_origin);
                assert!(
                    dispatch_result.is_err(),
                    "SmallSpender should not be able to spend more than their limit"
                );

                // Verify that no spend was created
                assert!(
                    pallet_treasury::Spends::<Runtime>::get(0).is_none(),
                    "No spend should be created for the failed attempt"
                );

                // Verify that balances remain unchanged
                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    EXISTENTIAL_DEPOSIT
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance
                );

                // Verify that no AssetSpendApproved event was emitted
                let spend_approved_event_found = System::events().iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::TreasuryPallet(
                            pallet_treasury::Event::AssetSpendApproved { .. }
                        )
                    )
                });
                assert!(
                    !spend_approved_event_found,
                    "No spend should have been approved"
                );
            });
    }

    /// Tests the expiry of a treasury spend proposal.
    /// 1. Root approves a spend.
    /// 2. Time is advanced beyond the PayoutPeriod.
    /// 3. Attempting to payout the expired spend should fail.
    /// 4. `check_status` is called to process the expired spend.
    /// 5. Spend should be removed from storage.
    #[test]
    fn treasury_spend_proposal_expires_if_not_paid_out() {
        ExtBuilder::default()
            .with_balances(vec![
                (BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT),
                (TreasuryPallet::account_id(), 1000 * UNIT),
            ])
            .build()
            .execute_with(|| {
                System::set_block_number(1);
                let beneficiary_lookup =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();
                let initial_treasury_balance = Balances::free_balance(&treasury_pot);
                let initial_beneficiary_balance = Balances::free_balance(&BENEFICIARY_ACCOUNT_ID);

                let spend_amount = 50 * UNIT;
                let spend_index = 0;

                // Approve a spend
                let call =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount,
                        beneficiary: Box::new(beneficiary_lookup.clone()),
                        valid_from: None,
                    });
                assert_ok!(call.dispatch_bypass_filter(RawOrigin::Root.into()));

                let expected_expiry_block = System::block_number() + TreasuryPayoutPeriod::get();
                System::assert_last_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::AssetSpendApproved {
                        index: spend_index,
                        asset_kind: (),
                        amount: spend_amount,
                        beneficiary: BENEFICIARY_ACCOUNT_ID,
                        valid_from: System::block_number(),
                        expire_at: expected_expiry_block,
                    },
                ));
                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_some());

                // Advance time beyond the PayoutPeriod
                TestCommons::run_to_block(expected_expiry_block + 1);

                // Try to payout the expired spend
                let payout_result = TreasuryPallet::payout(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index,
                );
                // Check if the payout result is an error, without specifying the exact error type
                assert!(payout_result.is_err(), "Payout of expired spend should fail");

                // Balances should remain unchanged
                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance
                );
                assert_eq!(Balances::free_balance(&treasury_pot), initial_treasury_balance);

                // Attempt to process the spend status after failed payout
                assert_ok!(TreasuryPallet::check_status(
                    RuntimeOrigin::signed(PROPOSER_ACCOUNT_ID).into(), // Use a signed origin
                    spend_index
                ));

                // Verify the spend is removed after check_status
                assert!(
                    pallet_treasury::Spends::<Runtime>::get(spend_index).is_none(),
                    "Spend should be removed after check_status is called on an expired spend"
                );

                // Ensure no payment event was emitted
                let paid_event_found = System::events().iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::TreasuryPallet(pallet_treasury::Event::Paid { index, .. }) if index == spend_index
                    )
                });
                assert!(!paid_event_found, "Paid event should not be emitted for expired spend");
            });
    }

    /// Tests treasury spend behavior when funds are insufficient.
    /// 1. Treasury is initialized with a small balance.
    /// 2. Root proposes a spend greater than the treasury balance.
    /// 3. Attempting to payout the spend fails due to insufficient funds.
    /// 4. Treasury is topped up.
    /// 5. Payout is attempted again and succeeds.
    #[test]
    fn treasury_spend_insufficient_funds() {
        ExtBuilder::default()
            .with_balances(vec![
                (BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT),
                (TreasuryPallet::account_id(), 20 * UNIT), // Treasury starts with less than spend amount
            ])
            .build()
            .execute_with(|| {
                System::set_block_number(1);
                let beneficiary_lookup =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();
                let initial_treasury_balance = Balances::free_balance(&treasury_pot);
                assert_eq!(initial_treasury_balance, 20 * UNIT);
                let initial_beneficiary_balance = Balances::free_balance(&BENEFICIARY_ACCOUNT_ID);

                let spend_amount_above_balance = 50 * UNIT;
                let spend_index = 0;

                // Propose spend greater than current treasury balance - should be fine
                let call_above_balance =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount_above_balance,
                        beneficiary: Box::new(beneficiary_lookup.clone()),
                        valid_from: None,
                    });
                assert_ok!(call_above_balance.dispatch_bypass_filter(RawOrigin::Root.into()));

                // Capture the event and assert specific fields, like index
                let captured_event = System::events().pop().expect("Expected an event").event;
                if let RuntimeEvent::TreasuryPallet(pallet_treasury::Event::AssetSpendApproved { index, .. }) = captured_event {
                    assert_eq!(index, spend_index, "Event index mismatch for AssetSpendApproved");
                } else {
                    panic!("Expected TreasuryPallet::AssetSpendApproved event");
                }

                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_some());

                // Try to payout the spend when treasury funds are insufficient
                TestCommons::run_to_block(System::block_number() + 5);

                let payout_result_insufficient = TreasuryPallet::payout(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index,
                );
                assert!(payout_result_insufficient.is_err(), "Payout with insufficient funds should fail");

                // Balances should remain unchanged
                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance
                );
                assert_eq!(Balances::free_balance(&treasury_pot), initial_treasury_balance);
                let paid_event_found = System::events().iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::TreasuryPallet(pallet_treasury::Event::Paid { index, .. }) if index == spend_index
                    )
                });
                assert!(!paid_event_found, "Paid event should not be emitted if funds are insufficient");
                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_some(), "Spend should still exist");

                // Now, fund the treasury sufficiently
                let top_up_amount = 100 * UNIT;
                let new_treasury_balance_target = initial_treasury_balance + top_up_amount;
                assert_ok!(Balances::force_set_balance(
                    RawOrigin::Root.into(),
                    <Runtime as frame_system::Config>::Lookup::unlookup(treasury_pot.clone()),
                    new_treasury_balance_target
                ));
                assert_eq!(Balances::free_balance(&treasury_pot), new_treasury_balance_target);

                // Try to payout again
                assert_ok!(TreasuryPallet::payout(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index
                ));
                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::Paid { index: spend_index, payment_id: 0 },
                ));
                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance + spend_amount_above_balance
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    new_treasury_balance_target - spend_amount_above_balance
                );

                assert_ok!(TreasuryPallet::check_status(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index
                ));
                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_none());
            });
    }

    /// Tests treasury spend behavior with a `valid_from` field set in the future.
    /// 1. Root approves a spend with `valid_from` set to a future block.
    /// 2. Attempting to payout before `valid_from` block fails.
    /// 3. Time is advanced to `valid_from` block.
    /// 4. Payout is attempted again and succeeds.
    #[test]
    fn treasury_spend_with_valid_from_in_future() {
        ExtBuilder::default()
            .with_balances(vec![
                (BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT),
                (TreasuryPallet::account_id(), 1000 * UNIT),
            ])
            .build()
            .execute_with(|| {
                System::set_block_number(1);
                let beneficiary_lookup =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();
                let initial_treasury_balance = Balances::free_balance(&treasury_pot);
                let initial_beneficiary_balance = Balances::free_balance(&BENEFICIARY_ACCOUNT_ID);

                let spend_amount = 50 * UNIT;
                let spend_index = 0;
                let valid_from_block = System::block_number() + 10;

                let call =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount,
                        beneficiary: Box::new(beneficiary_lookup.clone()),
                        valid_from: Some(valid_from_block),
                    });
                assert_ok!(call.dispatch_bypass_filter(RawOrigin::Root.into()));

                // Capture the event and assert specific fields
                let captured_event = System::events().pop().expect("Expected an event").event;
                if let RuntimeEvent::TreasuryPallet(pallet_treasury::Event::AssetSpendApproved { index, valid_from, .. }) = captured_event {
                    assert_eq!(index, spend_index, "Event index mismatch");
                    assert_eq!(valid_from, valid_from_block, "Event valid_from mismatch");
                } else {
                    panic!("Expected TreasuryPallet::AssetSpendApproved event");
                }

                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_some());

                // Try to payout before valid_from_block
                TestCommons::run_to_block(valid_from_block - 1);
                let payout_result_before_valid = TreasuryPallet::payout(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index,
                );
                assert!(payout_result_before_valid.is_err(), "Payout before valid_from should fail");

                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance
                );
                let paid_event_found_before = System::events().iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::TreasuryPallet(pallet_treasury::Event::Paid { index, .. }) if index == spend_index
                    )
                });
                assert!(!paid_event_found_before);

                // Advance to valid_from_block
                TestCommons::run_to_block(valid_from_block);
                assert_ok!(TreasuryPallet::payout(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index
                ));
                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::Paid { index: spend_index, payment_id: 0 },
                ));
                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance + spend_amount
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance - spend_amount
                );

                assert_ok!(TreasuryPallet::check_status(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(),
                    spend_index
                ));
                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_none());
            });
    }

    /// Tests that a treasury spend can be paid out by an account different from the beneficiary.
    /// 1. Root approves a spend.
    /// 2. A different account (PROPOSER_ACCOUNT_ID) successfully calls payout.
    /// 3. Beneficiary receives the funds.
    #[test]
    fn treasury_spend_payout_by_different_account() {
        ExtBuilder::default()
            .with_balances(vec![
                (BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT),
                (PROPOSER_ACCOUNT_ID, EXISTENTIAL_DEPOSIT), // Payer account
                (TreasuryPallet::account_id(), 1000 * UNIT),
            ])
            .build()
            .execute_with(|| {
                System::set_block_number(1);
                let beneficiary_lookup =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();
                let initial_treasury_balance = Balances::free_balance(&treasury_pot);
                let initial_beneficiary_balance = Balances::free_balance(&BENEFICIARY_ACCOUNT_ID);
                let initial_proposer_balance = Balances::free_balance(&PROPOSER_ACCOUNT_ID);

                let spend_amount = 50 * UNIT;
                let spend_index = 0;

                let call =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount,
                        beneficiary: Box::new(beneficiary_lookup.clone()),
                        valid_from: None,
                    });
                assert_ok!(call.dispatch_bypass_filter(RawOrigin::Root.into()));
                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_some());

                // Payout by PROPOSER_ACCOUNT_ID
                assert_ok!(TreasuryPallet::payout(
                    RuntimeOrigin::signed(PROPOSER_ACCOUNT_ID).into(),
                    spend_index
                ));

                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::Paid {
                        index: spend_index,
                        payment_id: 0,
                    },
                ));

                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    initial_beneficiary_balance + spend_amount
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance - spend_amount
                );
                // Proposer's balance should be unchanged (ignoring tx fees)
                assert_eq!(
                    Balances::free_balance(&PROPOSER_ACCOUNT_ID),
                    initial_proposer_balance
                );

                assert_ok!(TreasuryPallet::check_status(
                    RuntimeOrigin::signed(BENEFICIARY_ACCOUNT_ID).into(), // Can be anyone
                    spend_index
                ));
                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_none());
            });
    }

    /// Tests that a treasury spend proposal submitted via a general community referendum track
    /// (Track 0) with an incorrect origin for that track (e.g. a regular signed origin instead of
    /// the track's specific origin) is not approved and funds are not spent.
    /// 1. Proposer submits a treasury spend call as a preimage.
    /// 2. Proposer submits this preimage to Referenda Track 0 using their own signed origin
    ///    (which is not the correct origin for Track 0 governance actions).
    /// 3. Voter votes aye.
    /// 4. Time is advanced through all referendum phases.
    /// 5. Referendum should NOT be confirmed due to origin mismatch.
    /// 6. No treasury spend should be approved or paid out.
    #[test]
    fn treasury_spend_via_community_referendum_origin_mismatch() {
        ExtBuilder::default()
            .with_balances(vec![
                (PROPOSER_ACCOUNT_ID, 10_000 * UNIT),
                (VOTER_ACCOUNT_ID, 10_000 * UNIT),
                (BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT),
            ])
            .build()
            .execute_with(|| {
                let proposal_origin_for_preimage =
                    RuntimeOrigin::signed(PROPOSER_ACCOUNT_ID.clone());
                let proposal_origin_for_referendum_submission =
                    RuntimeOrigin::signed(PROPOSER_ACCOUNT_ID.clone());
                let voter_origin = RuntimeOrigin::signed(VOTER_ACCOUNT_ID.clone());

                let beneficiary_lookup =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();

                let initial_treasury_balance = 1000 * UNIT;
                let _ = <Balances as Currency<AccountId>>::deposit_creating(
                    &treasury_pot,
                    initial_treasury_balance,
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance
                );

                let spend_amount = 50 * UNIT;

                let treasury_spend_call =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount,
                        beneficiary: Box::new(beneficiary_lookup.clone()),
                        valid_from: None,
                    });

                let encoded_call = treasury_spend_call.encode();
                assert_ok!(Preimage::note_preimage(
                    proposal_origin_for_preimage,
                    encoded_call.clone()
                ));

                let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);
                let h256_preimage_hash: sp_core::H256 = preimage_hash.into();
                assert!(Preimage::have_preimage(&h256_preimage_hash));

                let track_id = 0u16;
                type RuntimeTracks = <Runtime as pallet_referenda::Config>::Tracks;

                let proposal_for_referenda = Bounded::Lookup {
                    hash: preimage_hash,
                    len: encoded_call.len() as u32,
                };

                assert_ok!(Referenda::submit(
                    proposal_origin_for_referendum_submission,
                    Box::new(OriginCaller::system(RawOrigin::Signed(
                        PROPOSER_ACCOUNT_ID.clone()
                    ))),
                    proposal_for_referenda.clone(),
                    ScheduleDispatchTime::After(1u32.into())
                ));

                let referendum_index: ReferendumIndex = 0;

                let track_info =
                    <RuntimeTracks as TracksInfo<Balance, BlockNumber>>::info(track_id)
                        .expect("Track info should be available for track 0");

                System::set_block_number(System::block_number() + track_info.prepare_period);

                assert_ok!(ConvictionVoting::vote(
                    voter_origin,
                    referendum_index,
                    pallet_conviction_voting::AccountVote::Standard {
                        vote: pallet_conviction_voting::Vote {
                            aye: true,
                            conviction: pallet_conviction_voting::Conviction::None
                        },
                        balance: Balances::free_balance(&VOTER_ACCOUNT_ID),
                    }
                ));

                let mut current_block = System::block_number();
                current_block += track_info.decision_period;
                System::set_block_number(current_block);
                current_block += track_info.confirm_period;
                System::set_block_number(current_block);
                current_block += track_info.min_enactment_period;
                current_block += 1;
                System::set_block_number(current_block);

                <Scheduler as Hooks<BlockNumber>>::on_initialize(System::block_number());

                // Check that the referendum was not confirmed
                let confirmed_event = System::events().iter().find_map(|event_record| {
                    if let RuntimeEvent::Referenda(pallet_referenda::Event::Confirmed {
                        index,
                        tally,
                    }) = &event_record.event
                    {
                        if *index == referendum_index {
                            Some(tally.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                assert!(
                    confirmed_event.is_none(),
                    "Referendum should not be confirmed with incorrect origin"
                );

                // Check that funds were not spent
                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    EXISTENTIAL_DEPOSIT
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance
                );

                // Check that there is no AssetSpendApproved event
                let spend_approved_event_found = System::events().iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::TreasuryPallet(
                            pallet_treasury::Event::AssetSpendApproved { .. }
                        )
                    )
                });
                assert!(
                    !spend_approved_event_found,
                    "Treasury spend should not have been approved via this referendum track"
                );
            });
    }

    /// Tests the successful flow of a treasury spend through a dedicated spender track in referenda.
    /// 1. Proposer submits a treasury spend call as a preimage.
    /// 2. Proposer submits this preimage to Referenda Track 2 (SmallSpender) using the correct
    ///    SmallSpender origin.
    /// 3. Proposer places the decision deposit.
    /// 4. Voter votes aye.
    /// 5. Time is advanced through all referendum phases (prepare, decision, confirm, enactment).
    /// 6. Referendum should be confirmed and the treasury spend dispatched via scheduler.
    /// 7. AssetSpendApproved event should be emitted.
    /// 8. Beneficiary successfully payouts the spend.
    /// 9. Spend is processed and removed.
    #[test]
    fn treasury_spend_via_dedicated_spender_track_works() {
        const SPEND_AMOUNT: Balance = 200 * MICRO_UNIT;
        // Use common::account_id for consistency
        let proposer_account_id = TestCommons::account_id(123);
        let voter_account_id = TestCommons::account_id(124);
        let beneficiary_account_id = TestCommons::account_id(125);

        ExtBuilder::default()
            .with_balances(vec![
                (proposer_account_id.clone(), 10000 * UNIT),
                (voter_account_id.clone(), 10000 * UNIT),
                (beneficiary_account_id.clone(), EXISTENTIAL_DEPOSIT),
                (TreasuryPallet::account_id(), 1000 * UNIT),
            ])
            .build()
            .execute_with(|| {
                System::set_block_number(1); // Start at block 1
                let initial_treasury_balance = TreasuryPallet::pot();
                let initial_beneficiary_balance = Balances::free_balance(&beneficiary_account_id);
                let initial_spend_index = 0u32;

                let call_to_spend = RuntimeCall::TreasuryPallet(pallet_treasury::Call::spend {
                    asset_kind: Box::new(()),
                    amount: SPEND_AMOUNT,
                    beneficiary: Box::new(<Runtime as frame_system::Config>::Lookup::unlookup(
                        beneficiary_account_id.clone(),
                    )),
                    valid_from: None,
                });

                let encoded_call_to_spend = call_to_spend.encode();
                let hash_of_call_to_spend =
                    <Runtime as frame_system::Config>::Hashing::hash(&encoded_call_to_spend);

                assert_ok!(Preimage::note_preimage(
                    RuntimeOrigin::signed(proposer_account_id.clone()),
                    encoded_call_to_spend.clone()
                ));
                System::assert_last_event(RuntimeEvent::Preimage(pallet_preimage::Event::Noted {
                    hash: hash_of_call_to_spend,
                }));

                // Revert to original: Target Track 2
                let proposal_origin_for_track_selection = Box::new(OriginCaller::Origins(
                    pallet_custom_origins::Origin::SmallSpender,
                ));

                let proposal_for_referenda = Bounded::Lookup {
                    hash: hash_of_call_to_spend,
                    len: encoded_call_to_spend.len() as u32,
                };

                let track_info_2 = CommunityTracksInfo::info(2).unwrap();

                let dispatch_time = ScheduleDispatchTime::After(1u32.into());
                const TEST_REFERENDUM_INDEX: ReferendumIndex = 0;
                let referendum_index: ReferendumIndex = TEST_REFERENDUM_INDEX;

                assert_ok!(Referenda::submit(
                    RuntimeOrigin::signed(proposer_account_id.clone()),
                    proposal_origin_for_track_selection,
                    proposal_for_referenda.clone(),
                    dispatch_time
                ));

                System::assert_has_event(RuntimeEvent::Referenda(
                    pallet_referenda::Event::Submitted {
                        index: referendum_index,
                        track: 2,
                        proposal: proposal_for_referenda.clone(),
                    },
                ));

                assert_ok!(Referenda::place_decision_deposit(
                    RuntimeOrigin::signed(proposer_account_id.clone()),
                    referendum_index
                ));

                // Start of new block advancement logic using run_to_block
                let block_after_decision_deposit = System::block_number();

                // Advance past prepare_period
                let end_of_prepare_period =
                    block_after_decision_deposit + track_info_2.prepare_period;

                TestCommons::run_to_block(end_of_prepare_period);

                assert_ok!(ConvictionVoting::vote(
                    RuntimeOrigin::signed(voter_account_id.clone()),
                    referendum_index,
                    pallet_conviction_voting::AccountVote::Standard {
                        vote: pallet_conviction_voting::Vote {
                            aye: true,
                            conviction: pallet_conviction_voting::Conviction::None
                        },
                        balance: Balances::free_balance(&voter_account_id),
                    }
                ));
                let block_vote_cast = System::block_number();

                // Advance 1 block for scheduler to potentially process vote related actions
                let block_for_vote_processing = block_vote_cast + 1;

                TestCommons::run_to_block(block_for_vote_processing);

                // Advance by confirm_period from the block where vote was processed
                let block_after_vote_processing = System::block_number();
                let end_of_confirm_period =
                    block_after_vote_processing + track_info_2.confirm_period;

                TestCommons::run_to_block(end_of_confirm_period);

                // Wait for approval phase
                let block_after_confirm = System::block_number();
                let approval_period = track_info_2.decision_period / 2; // Half of decision period for approval
                let target_approval_block = block_after_confirm + approval_period;

                TestCommons::run_to_block(target_approval_block);

                let confirmed_event = System::events()
                    .iter()
                    .find_map(|event_record| {
                        if let RuntimeEvent::Referenda(pallet_referenda::Event::Confirmed {
                            index,
                            tally,
                        }) = &event_record.event
                        {
                            if *index == referendum_index {
                                Some(tally.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .expect("Confirmed event should be present");
                System::assert_has_event(RuntimeEvent::Referenda(
                    pallet_referenda::Event::Confirmed {
                        index: referendum_index,
                        tally: confirmed_event,
                    },
                ));

                // Advance past min_enactment_period (relative to when enactment can start)
                let block_after_approved = System::block_number();
                let target_enactment_block =
                    block_after_approved + track_info_2.min_enactment_period;
                TestCommons::run_to_block(target_enactment_block);

                // Add a small buffer for scheduler to pick up and dispatch
                let final_check_block = System::block_number() + 5;
                TestCommons::run_to_block(final_check_block);

                System::events().iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::Scheduler(pallet_scheduler::Event::Dispatched {
                            task: (qp_scheduler::BlockNumberOrTimestamp::BlockNumber(86402), 0),
                            id: _,
                            result: Ok(())
                        })
                    )
                });

                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::AssetSpendApproved {
                        index: initial_spend_index,
                        asset_kind: (),
                        amount: SPEND_AMOUNT,
                        beneficiary: beneficiary_account_id.clone(),
                        valid_from: 216002,
                        expire_at: 216002 + TreasuryPayoutPeriod::get(),
                    },
                ));

                assert_ok!(TreasuryPallet::payout(
                    RuntimeOrigin::signed(beneficiary_account_id.clone()),
                    initial_spend_index
                ));

                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::Paid {
                        index: initial_spend_index,
                        payment_id: 0,
                    },
                ));

                assert_ok!(TreasuryPallet::check_status(
                    RuntimeOrigin::signed(beneficiary_account_id.clone()),
                    initial_spend_index
                ));
                System::assert_has_event(RuntimeEvent::TreasuryPallet(
                    pallet_treasury::Event::SpendProcessed {
                        index: initial_spend_index,
                    },
                ));
                assert_eq!(
                    Balances::free_balance(&beneficiary_account_id),
                    initial_beneficiary_balance + SPEND_AMOUNT
                );
                assert_eq!(
                    TreasuryPallet::pot(),
                    initial_treasury_balance - SPEND_AMOUNT
                );
            });
    }
}
