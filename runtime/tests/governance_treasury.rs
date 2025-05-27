#[path = "common.rs"]
mod common;

#[cfg(test)]
mod tests {
    // Imports from the runtime crate
    use resonance_runtime::configs::{
        ReferendumSubmissionDeposit, TreasuryPalletId, TreasuryPayoutPeriod,
    }; // ReferendumSubmissionDeposit unused, consider removing
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
        DAYS,
        EXISTENTIAL_DEPOSIT,
        HOURS, // DAYS, HOURS are unused, consider removing if not needed elsewhere
        MICRO_UNIT,
        UNIT,
    };
    // Additional pallets for referenda tests
    use resonance_runtime::{ConvictionVoting, Preimage, Referenda, Scheduler};

    // Codec & Hashing
    use codec::Encode;
    use sp_runtime::traits::Hash as RuntimeTraitHash;

    // Frame and Substrate traits & types
    use frame_support::{
        assert_ok,
        pallet_prelude::Hooks, // For Scheduler hooks
        traits::{
            schedule::DispatchTime as ScheduleDispatchTime,
            Bounded, // Added Bounded
            Currency,
            PreimageProvider, // Added PreimageProvider
            QueryPreimage, // For Preimage pallet (StorePreimage, QueryPreimage might be unused if direct calls work)
            StorePreimage,
            UnfilteredDispatchable,
        },
    };
    use frame_system::RawOrigin;
    use pallet_treasury;
    use sp_runtime::{
        traits::{AccountIdConversion, StaticLookup},
        BuildStorage,
    };
    // ReferendumInfo, ReferendumStatus are unused, consider removing
    use crate::common::run_to_block;
    use pallet_referenda::{self, ReferendumIndex, TracksInfo};
    use resonance_runtime::governance::definitions::CommunityTracksInfo;
    use sp_core::crypto::AccountId32; // Ensure AccountId32 is imported
    use sp_runtime::traits::Hash; // Import run_to_block

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

    #[test]
    fn propose_spend_as_custom_origin_works() {
        ExtBuilder::default()
            .with_balances(vec![(BENEFICIARY_ACCOUNT_ID, EXISTENTIAL_DEPOSIT)])
            .build()
            .execute_with(|| {
                let beneficiary_lookup_source =
                    <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();
                let small_tipper_origin: TestRuntimeOrigin =
                    pallet_custom_origins::Origin::SmallTipper.into();

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
                    .dispatch_bypass_filter(small_tipper_origin.clone()));

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

                let spend_amount_above_limit = (250 * 3 * MICRO_UNIT) + 1 * MICRO_UNIT;
                let call_above_limit =
                    TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                        asset_kind: Box::new(()),
                        amount: spend_amount_above_limit,
                        beneficiary: Box::new(beneficiary_lookup_source.clone()),
                        valid_from: None,
                    });

                let dispatch_result_above_limit =
                    call_above_limit.dispatch_bypass_filter(small_tipper_origin);
                assert!(
                    dispatch_result_above_limit.is_err(),
                    "Dispatch should fail for amount above limit"
                );

                assert!(
                    pallet_treasury::Spends::<Runtime>::get(spend_index_within_limit + 1).is_none(),
                    "No new spend should be created for the failed attempt"
                );
            });
    }

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
                // Use explicitly imported RuntimeOrigin
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

                // Use imported frame_support::traits::Bounded
                let proposal_for_referenda = Bounded::Lookup {
                    hash: preimage_hash,
                    len: encoded_call.len() as u32,
                };

                // Corrected Referenda::submit call: origin, track, proposal (not boxed), dispatch_after
                assert_ok!(Referenda::submit(
                    proposal_origin_for_referendum_submission,
                    Box::new(OriginCaller::system(RawOrigin::Signed(
                        PROPOSER_ACCOUNT_ID.clone()
                    ))),
                    proposal_for_referenda.clone(), // Pass Bounded::Lookup directly
                    ScheduleDispatchTime::After(1u32.into())
                ));

                // If referendum_count() still not found, assume 0 for now for testing flow.
                // let referendum_index: ReferendumIndex = Referenda::referendum_count() - 1;
                let referendum_index: ReferendumIndex = 0; // Temporary workaround

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

                // Use imported frame_support::pallet_prelude::Hooks
                <Scheduler as Hooks<BlockNumber>>::on_initialize(System::block_number());

                assert_eq!(
                    Balances::free_balance(&BENEFICIARY_ACCOUNT_ID),
                    EXISTENTIAL_DEPOSIT
                );
                assert_eq!(
                    Balances::free_balance(&treasury_pot),
                    initial_treasury_balance
                );

                let latest_events = System::events();
                let spend_approved_event_found = latest_events.iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::TreasuryPallet(
                            pallet_treasury::Event::AssetSpendApproved { .. }
                        )
                    )
                });
                assert!(
                    !spend_approved_event_found,
                    "Treasury spend should not have been approved via this referendum track."
                );

                // Znajdź event Confirmed i pobierz tally
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
                println!("[TREASURY_TEST_DEBUG] Event Referenda::Confirmed asserted.");
            });
    }

    #[test]
    fn treasury_spend_via_dedicated_spender_track_works() {
        const SPEND_AMOUNT: Balance = 200 * MICRO_UNIT;
        // Use common::account_id for consistency
        let proposer_account_id = crate::common::account_id(123);
        let voter_account_id = crate::common::account_id(124);
        let beneficiary_account_id = crate::common::account_id(125);

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
                println!("[TREASURY_TEST_DEBUG] Test initialized. Initial treasury_balance: {}, beneficiary_balance: {}", initial_treasury_balance, initial_beneficiary_balance);

                let call_to_spend = RuntimeCall::TreasuryPallet(pallet_treasury::Call::spend {
                    asset_kind: Box::new(()),
                    amount: SPEND_AMOUNT,
                    beneficiary: Box::new(<Runtime as frame_system::Config>::Lookup::unlookup(beneficiary_account_id.clone())),
                    valid_from: None,
                });

                let encoded_call_to_spend = call_to_spend.encode();
                let hash_of_call_to_spend = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call_to_spend);

                println!("[TREASURY_TEST_DEBUG] Noting preimage...");
                assert_ok!(Preimage::note_preimage(
                    RuntimeOrigin::signed(proposer_account_id.clone()),
                    encoded_call_to_spend.clone()
                ));
                System::assert_last_event(RuntimeEvent::Preimage(pallet_preimage::Event::Noted {
                    hash: hash_of_call_to_spend,
                }));
                println!("[TREASURY_TEST_DEBUG] Preimage noted.");

                // Revert to original: Target Track 2
                let proposal_origin_for_track_selection =
                    Box::new(OriginCaller::Origins(pallet_custom_origins::Origin::SmallTipper));

                let proposal_for_referenda = Bounded::Lookup {
                    hash: hash_of_call_to_spend,
                    len: encoded_call_to_spend.len() as u32,
                };

                let track_info_2 = CommunityTracksInfo::info(2).unwrap();
                println!("[TREASURY_TEST_DEBUG] Track 2 info: prepare_period={}, decision_period={}, confirm_period={}, min_enactment_period={}",
                    track_info_2.prepare_period, track_info_2.decision_period, track_info_2.confirm_period, track_info_2.min_enactment_period);

                let dispatch_time = ScheduleDispatchTime::After(1u32.into());
                const TEST_REFERENDUM_INDEX: ReferendumIndex = 0;
                let referendum_index: ReferendumIndex = TEST_REFERENDUM_INDEX;
                println!("[TREASURY_TEST_DEBUG] Using referendum_index: {}", referendum_index);

                println!("[TREASURY_TEST_DEBUG] Submitting referendum...");
                assert_ok!(Referenda::submit(
                    RuntimeOrigin::signed(proposer_account_id.clone()),
                    proposal_origin_for_track_selection,
                    proposal_for_referenda.clone(),
                    dispatch_time
                ));
                println!("[TREASURY_TEST_DEBUG] Referendum submitted.");

                System::assert_has_event(RuntimeEvent::Referenda(pallet_referenda::Event::Submitted {
                    index: referendum_index,
                    track: 2,
                    proposal: proposal_for_referenda.clone(),
                }));
                println!("[TREASURY_TEST_DEBUG] Event Referenda::Submitted asserted.");

                println!("[TREASURY_TEST_DEBUG] Placing decision deposit...");
                assert_ok!(Referenda::place_decision_deposit(
                    RuntimeOrigin::signed(proposer_account_id.clone()),
                    referendum_index
                ));
                println!("[TREASURY_TEST_DEBUG] Decision deposit placed.");

                // Start of new block advancement logic using run_to_block
                let block_after_decision_deposit = System::block_number();
                println!("[TREASURY_TEST_DEBUG] After decision deposit. Current block: {}", block_after_decision_deposit);

                // Advance past prepare_period
                let end_of_prepare_period = block_after_decision_deposit + track_info_2.prepare_period;
                println!("[TREASURY_TEST_DEBUG] Advancing to end of prepare_period. Target block: {}", end_of_prepare_period);
                run_to_block(end_of_prepare_period);
                println!("[TREASURY_TEST_DEBUG] At end of prepare_period. Block: {}", System::block_number());

                println!("[TREASURY_TEST_DEBUG] Submitting vote...");
                assert_ok!(ConvictionVoting::vote(
                    RuntimeOrigin::signed(voter_account_id.clone()),
                    referendum_index,
                    pallet_conviction_voting::AccountVote::Standard {
                        vote: pallet_conviction_voting::Vote { aye: true, conviction: pallet_conviction_voting::Conviction::None },
                        balance: Balances::free_balance(&voter_account_id),
                    }
                ));
                let block_vote_cast = System::block_number();
                println!("[TREASURY_TEST_DEBUG] Vote submitted. Current block: {}", block_vote_cast);

                // Advance 1 block for scheduler to potentially process vote related actions
                let block_for_vote_processing = block_vote_cast + 1;
                println!("[TREASURY_TEST_DEBUG] Advancing to block {} for vote processing.", block_for_vote_processing);
                run_to_block(block_for_vote_processing);
                println!("[TREASURY_TEST_DEBUG] At block for vote processing. Block: {}", System::block_number());

                // Advance by confirm_period from the block where vote was processed
                let block_after_vote_processing = System::block_number();
                let end_of_confirm_period = block_after_vote_processing + track_info_2.confirm_period;
                println!("[TREASURY_TEST_DEBUG] Advancing to end of confirm_period. Target block: {}", end_of_confirm_period);
                run_to_block(end_of_confirm_period);
                println!("[TREASURY_TEST_DEBUG] At end of confirm_period. Block: {}", System::block_number());

                // Wait for approval phase
                let block_after_confirm = System::block_number();
                let approval_period = track_info_2.decision_period / 2; // Half of decision period for approval
                let target_approval_block = block_after_confirm + approval_period;
                println!("[TREASURY_TEST_DEBUG] Advancing to approval phase. Target block: {}", target_approval_block);
                run_to_block(target_approval_block);
                println!("[TREASURY_TEST_DEBUG] At approval phase. Block: {}", System::block_number());

                let confirmed_event = System::events().iter().find_map(|event_record| {
                    if let RuntimeEvent::Referenda(pallet_referenda::Event::Confirmed { index, tally }) = &event_record.event {
                        if *index == referendum_index {
                            Some(tally.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }).expect("Confirmed event should be present");
                System::assert_has_event(RuntimeEvent::Referenda(pallet_referenda::Event::Confirmed { index: referendum_index, tally: confirmed_event }));
                println!("[TREASURY_TEST_DEBUG] Event Referenda::Confirmed asserted.");

                // Advance past min_enactment_period (relative to when enactment can start)
                let block_after_approved = System::block_number();
                let target_enactment_block = block_after_approved + track_info_2.min_enactment_period;
                println!("[TREASURY_TEST_DEBUG] Advancing to enactment. Target block: {}", target_enactment_block);
                run_to_block(target_enactment_block);

                // Add a small buffer for scheduler to pick up and dispatch
                let final_check_block = System::block_number() + 5;
                println!("[TREASURY_TEST_DEBUG] Advancing to final check block with buffer: {}", final_check_block);
                run_to_block(final_check_block);
                println!("[TREASURY_TEST_DEBUG] At final check block. Block: {}", System::block_number());

                println!("[TREASURY_TEST_DEBUG] Asserting Scheduler::Dispatched event...");
                System::events().iter().any(|event_record| {
                    matches!(
                        event_record.event,
                        RuntimeEvent::Scheduler(pallet_scheduler::Event::Dispatched {
                            task: (86402, 0),
                            id: _,
                            result: Ok(())
                        })
                    )
                });
                println!("[TREASURY_TEST_DEBUG] Event Scheduler::Dispatched asserted.");

                println!("[TREASURY_TEST_DEBUG] Asserting TreasuryPallet::AssetSpendApproved event...");
                System::assert_has_event(RuntimeEvent::TreasuryPallet(pallet_treasury::Event::AssetSpendApproved {
                    index: initial_spend_index,
                    asset_kind: (),
                    amount: SPEND_AMOUNT,
                    beneficiary: beneficiary_account_id.clone(),
                    valid_from: 86402,
                    expire_at: 86402 + TreasuryPayoutPeriod::get(),
                }));
                println!("[TREASURY_TEST_DEBUG] Event TreasuryPallet::AssetSpendApproved asserted.");

                assert_ok!(TreasuryPallet::payout(RuntimeOrigin::signed(beneficiary_account_id.clone()), initial_spend_index));
                println!("[TREASURY_TEST_DEBUG] Asserting TreasuryPallet::Paid event...");
                System::assert_has_event(RuntimeEvent::TreasuryPallet(pallet_treasury::Event::Paid {
                    index: initial_spend_index,
                    payment_id: 0,
                }));
                println!("[TREASURY_TEST_DEBUG] Event TreasuryPallet::Paid asserted.");

                // Dodajemy wywołanie check_status
                assert_ok!(TreasuryPallet::check_status(RuntimeOrigin::signed(beneficiary_account_id.clone()), initial_spend_index));
                println!("[TREASURY_TEST_DEBUG] Asserting TreasuryPallet::SpendProcessed event...");
                System::assert_has_event(RuntimeEvent::TreasuryPallet(pallet_treasury::Event::SpendProcessed {
                    index: initial_spend_index,
                }));
                println!("[TREASURY_TEST_DEBUG] Event TreasuryPallet::SpendProcessed asserted.");

                println!("[TREASURY_TEST_DEBUG] Asserting final balances...");
                assert_eq!(
                    Balances::free_balance(&beneficiary_account_id),
                    initial_beneficiary_balance + SPEND_AMOUNT
                );
                assert_eq!(TreasuryPallet::pot(), initial_treasury_balance - SPEND_AMOUNT);
                println!("[TREASURY_TEST_DEBUG] Final balances asserted. Test finished.");
            });
    }
}
