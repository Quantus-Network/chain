#[path = "common.rs"]
mod common;

#[cfg(test)]
mod tests {
    // Imports from the runtime crate
    use resonance_runtime::{
        AccountId, Balance, Balances, Runtime, RuntimeCall, RuntimeEvent, System,
        TreasuryPallet,
        UNIT, // Assuming UNIT is pub in resonance_runtime
    };
    use resonance_runtime::configs::{TreasuryPalletId, TreasuryPayoutPeriod}; // Added TreasuryPayoutPeriod

    // Frame and Substrate imports
    use frame_support::{
        assert_ok,
        traits::{
            Currency,
            UnfilteredDispatchable, // Use this for dispatch_bypass_filter
        },
    };
    use frame_system::RawOrigin;
    use sp_runtime::{
        traits::{AccountIdConversion, StaticLookup},
        BuildStorage, // Assuming MultiSignature for tests if not ResonanceSignature
    };
    use pallet_treasury; // Direct import for Call enum if needed

    // Type aliases for testing clarity (mirroring runtime/src/lib.rs if possible)
    type TestRuntimeCall = RuntimeCall;     // Use the runtime's RuntimeCall

    // Test specific constants
    const BENEFICIARY_ACCOUNT_ID: AccountId = AccountId::new([1u8; 32]); // Example AccountId

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
            .with_balances(vec![]) // No initial balances for others, treasury funded manually
            .build()
            .execute_with(|| {
                let beneficiary_lookup_source = <Runtime as frame_system::Config>::Lookup::unlookup(BENEFICIARY_ACCOUNT_ID);
                let treasury_pot = treasury_account_id();

                let initial_treasury_balance = 1000 * UNIT;
                let spend_amount = 100 * UNIT;

                // Fund the treasury account
                let _ = <Balances as Currency<AccountId>>::deposit_creating(&treasury_pot, initial_treasury_balance);
                assert_eq!(Balances::free_balance(&treasury_pot), initial_treasury_balance);
                let initial_beneficiary_balance = Balances::free_balance(&BENEFICIARY_ACCOUNT_ID);

                // Root creates and approves a spend via the `spend` extrinsic
                let call = TestRuntimeCall::TreasuryPallet(pallet_treasury::Call::<Runtime>::spend {
                    asset_kind: Box::new(()), // Native currency
                    amount: spend_amount,
                    beneficiary: Box::new(beneficiary_lookup_source.clone()),
                    valid_from: None, // Valid immediately
                });

                // Dispatch the call as Root using UnfilteredDispatchable
                let dispatch_result = call.dispatch_bypass_filter(RawOrigin::Root.into());
                assert_ok!(dispatch_result);

                let spend_index = 0; // Assuming it's the first spend

                // Check for AssetSpendApproved event
                System::assert_last_event(RuntimeEvent::TreasuryPallet(pallet_treasury::Event::AssetSpendApproved {
                    index: spend_index,
                    asset_kind: (),
                    amount: spend_amount,
                    beneficiary: BENEFICIARY_ACCOUNT_ID,
                    valid_from: System::block_number(), // Assuming valid_from is current block if None was passed to spend
                    expire_at: System::block_number() + TreasuryPayoutPeriod::get(), // Calculate expected expiry
                }));

                // Verify spend exists (though we can't inspect fields directly without events)
                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_some(), "Spend should exist in storage");

                // Beneficiary (or anyone) claims the payout
                assert_ok!(TreasuryPallet::payout(RawOrigin::Signed(BENEFICIARY_ACCOUNT_ID).into(), spend_index));

                // Check for Paid event ( payout_id might be tricky to get deterministically for dummy paymaster if not in event)
                // We can check that *a* Paid event occurred for the right index.
                // For a more robust check, RuntimeNativePaymaster could return a predictable ID or emit its own event.
                // For now, let's assume our dummy paymaster uses id 0 and the event reflects that.
                System::assert_has_event(RuntimeEvent::TreasuryPallet(pallet_treasury::Event::Paid {
                    index: spend_index,
                    payment_id: 0, // Assuming our RuntimeNativePaymaster uses ID 0 for successful payment
                }));

                // Check balances after payout
                assert_eq!(Balances::free_balance(&treasury_pot), initial_treasury_balance - spend_amount);
                assert_eq!(Balances::free_balance(&BENEFICIARY_ACCOUNT_ID), initial_beneficiary_balance + spend_amount);

                // Check status to finalize and remove the spend
                assert_ok!(TreasuryPallet::check_status(RawOrigin::Signed(BENEFICIARY_ACCOUNT_ID).into(), spend_index));

                // Check for SpendProcessed event
                System::assert_last_event(RuntimeEvent::TreasuryPallet(pallet_treasury::Event::SpendProcessed { index: spend_index }));

                assert!(pallet_treasury::Spends::<Runtime>::get(spend_index).is_none(), "Spend should be removed after check_status");
        });
    }
}