use crate::{mock::*, Event};
use frame_support::traits::Currency;
use frame_support::traits::Hooks;
use frame_support::weights::Weight;
use sp_runtime::traits::AccountIdConversion;

const UNIT: u128 = 1_000_000_000_000;

#[test]
fn miner_reward_works() {
    new_test_ext().execute_with(|| {
        // Remember initial balance (ExistentialDeposit)
        let initial_balance = Balances::free_balance(MINER);

        // Add a miner to the pre-runtime digest
        set_miner_digest(MINER);

        // Run the on_finalize hook
        MiningRewards::on_finalize(1);

        // Check that the miner received the block reward
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 // Initial + base reward
        );

        // Check the event was emitted
        System::assert_has_event(
            Event::MinerRewarded {
                block: 1,
                miner: MINER,
                reward: 50,
            }
            .into(),
        );
    });
}

#[test]
fn miner_reward_with_transaction_fees_works() {
    new_test_ext().execute_with(|| {
        // Remember initial balance
        let initial_balance = Balances::free_balance(MINER);

        // Add a miner to the pre-runtime digest
        set_miner_digest(MINER);

        // Manually add some transaction fees
        let fees: Balance = 25;
        MiningRewards::collect_transaction_fees(fees);

        // Check fees collection event
        System::assert_has_event(
            Event::FeesCollected {
                amount: 25,
                total: 25,
            }
            .into(),
        );

        // Run the on_finalize hook
        MiningRewards::on_finalize(1);

        // Check that the miner received the block reward + remaining fees
        // Fees: 25. 10% to treasury (25 * 0.1 = 2.5, floor -> 2). Miner gets 25 - 2 = 23.
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 23 // Initial + base + (fees - treasury_cut)
        );

        // Check the event was emitted with the correct amount
        System::assert_has_event(
            Event::MinerRewarded {
                block: 1,
                miner: MINER,
                reward: 50 + 23, // base + (fees - treasury_cut)
            }
            .into(),
        );
    });
}

#[test]
fn on_unbalanced_collects_fees() {
    new_test_ext().execute_with(|| {
        // Remember initial balance
        let initial_balance = Balances::free_balance(MINER);

        // Use collect_transaction_fees instead of directly calling on_unbalanced
        MiningRewards::collect_transaction_fees(30);

        // Check that fees were collected
        assert_eq!(MiningRewards::collected_fees(), 30);

        // Add a miner to the pre-runtime digest and distribute rewards
        set_miner_digest(MINER);
        MiningRewards::on_finalize(1);

        // Check that the miner received the block reward + 90% of fees
        // Fees: 30. 10% to treasury = 3. 90% to miner = 27.
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 27 // Initial + base + 90% of fees
        );
    });
}

#[test]
fn multiple_blocks_accumulate_rewards() {
    new_test_ext().execute_with(|| {
        // Remember initial balance
        let initial_balance = Balances::free_balance(MINER);

        // Block 1
        set_miner_digest(MINER);
        MiningRewards::collect_transaction_fees(10);
        MiningRewards::on_finalize(1);

        // Fees: 10. 10% to treasury (10 * 0.1 = 1, floor -> 1). Miner gets 10 - 1 = 9.
        let balance_after_block_1 = initial_balance + 50 + 9; // Initial + base + (fees - treasury_cut)
        assert_eq!(Balances::free_balance(MINER), balance_after_block_1);

        // Block 2
        System::set_block_number(2);
        set_miner_digest(MINER);
        MiningRewards::collect_transaction_fees(15);
        MiningRewards::on_finalize(2);

        // Fees: 15. 10% to treasury (15 * 0.1 = 1.5, floor -> 1). Miner gets 15 - 1 = 14.
        assert_eq!(
            Balances::free_balance(MINER),
            balance_after_block_1 + 50 + 14 // Balance after block 1 + base + (fees - treasury_cut)
        );
    });
}

#[test]
fn different_miners_get_different_rewards() {
    new_test_ext().execute_with(|| {
        // Remember initial balances
        let initial_balance_miner1 = Balances::free_balance(MINER);
        let initial_balance_miner2 = Balances::free_balance(MINER2);

        // Block 1 - First miner
        set_miner_digest(MINER);
        MiningRewards::collect_transaction_fees(10);
        MiningRewards::on_finalize(1);

        // Check first miner balance
        // Fees: 10. 10% to treasury = 1. 90% to miner1 = 9.
        let balance_after_block_1 = initial_balance_miner1 + 50 + 9; // Initial + base + 90% of fees
        assert_eq!(Balances::free_balance(MINER), balance_after_block_1);

        // Block 2 - Second miner
        System::set_block_number(2);
        set_miner_digest(MINER2);
        MiningRewards::collect_transaction_fees(20);
        MiningRewards::on_finalize(2);

        // Check second miner balance
        // Fees: 20. 10% to treasury = 2. 90% to miner2 = 18.
        assert_eq!(
            Balances::free_balance(MINER2),
            initial_balance_miner2 + 50 + 18 // Initial + base + 90% of fees
        );

        // First miner balance should remain unchanged
        assert_eq!(Balances::free_balance(MINER), balance_after_block_1);
    });
}

#[test]
fn transaction_fees_collector_works() {
    new_test_ext().execute_with(|| {
        // Remember initial balance
        let initial_balance = Balances::free_balance(MINER);

        // Use collect_transaction_fees to gather fees
        MiningRewards::collect_transaction_fees(10);
        MiningRewards::collect_transaction_fees(15);
        MiningRewards::collect_transaction_fees(5);

        // Check accumulated fees
        assert_eq!(MiningRewards::collected_fees(), 30);

        // Reward miner
        set_miner_digest(MINER);
        MiningRewards::on_finalize(1);

        // Check miner got base reward + 90% of all fees
        // Fees: 30. 10% to treasury = 3. 90% to miner = 27.
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 27 // Initial + base + 90% of fees
        );
    });
}

#[test]
fn block_lifecycle_works() {
    new_test_ext().execute_with(|| {
        // Remember initial balance
        let initial_balance = Balances::free_balance(MINER);

        // Run through a complete block lifecycle

        // 1. on_initialize - should return correct weight
        let weight = MiningRewards::on_initialize(1);
        assert_eq!(weight, Weight::from_parts(10_000, 0));

        // 2. Add some transaction fees during block execution
        MiningRewards::collect_transaction_fees(15);

        // 3. on_finalize - should reward the miner
        set_miner_digest(MINER);
        MiningRewards::on_finalize(1);

        // Check miner received rewards
        // Fees: 15. 10% to treasury (15 * 0.1 = 1.5, floor -> 1). Miner gets 15 - 1 = 14.
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 14 // Initial + base + (fees - treasury_cut)
        );
    });
}

#[test]
fn test_run_to_block_helper() {
    new_test_ext().execute_with(|| {
        // Remember initial balance
        let initial_balance = Balances::free_balance(MINER);

        // Set up miner
        set_miner_digest(MINER);

        // Add fees for block 1
        MiningRewards::collect_transaction_fees(10);

        // Run to block 3 (this should process blocks 1 and 2)
        run_to_block(3);

        // Check that miner received rewards for blocks 1 and 2
        // Block 1: Initial + 50 (base) + 10*0.9 (fees) = Initial + 59
        // Block 2: (Initial + 59) + 50 (base) + 0 (no new fees for block 2 in this test) = Initial + 109
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 109 // Initial + 50 + 9 + 50
        );

        // Verify we're at the expected block number
        assert_eq!(System::block_number(), 3);
    });
}

#[test]
fn rewards_go_to_treasury_when_no_miner() {
    new_test_ext().execute_with(|| {
        // Get Treasury account
        let treasury_account = TreasuryPalletId::get().into_account_truncating();
        let initial_treasury_balance = Balances::free_balance(&treasury_account);

        // Fund Treasury
        let treasury_funding = 1000 * UNIT;
        let _ = Balances::deposit_creating(&treasury_account, treasury_funding);

        // Create a block without a miner
        System::set_block_number(1);
        MiningRewards::on_finalize(System::block_number());

        // Check that Treasury received the rewards
        let expected_reward = BlockReward::get() + 0; // No tx fees in this test
        assert_eq!(
            Balances::free_balance(&treasury_account),
            initial_treasury_balance + treasury_funding + expected_reward
        );

        // Check that the event was emitted
        System::assert_has_event(
            Event::TreasuryRewarded {
                reward: expected_reward,
            }
            .into(),
        );
    });
}

#[test]
fn test_fees_split_between_treasury_and_miner() {
    new_test_ext().execute_with(|| {
        // Set up initial balances
        let miner = 1;
        let _ = Balances::deposit_creating(&miner, 0); // Create account, balance might become ExistentialDeposit
        let actual_initial_balance_after_creation = Balances::free_balance(&miner);

        // Set transaction fees
        let mut tx_fees = 100;
        MiningRewards::collect_transaction_fees(tx_fees);

        // Create a block with a miner
        System::set_block_number(1);
        set_miner_digest(miner);

        // Run on_finalize
        MiningRewards::on_finalize(System::block_number());

        // Get Treasury account
        let treasury_account = TreasuryPalletId::get().into_account_truncating();

        // Get actual values from the system AFTER on_finalize
        let treasury_balance_after_finalize = Balances::free_balance(&treasury_account);
        let miner_balance_after_finalize = Balances::free_balance(&miner);

        // Calculate expected values using the same method as in the implementation
        let fees_to_treasury_percentage = FeesToTreasuryPermill::get();
        let fees_for_treasury = fees_to_treasury_percentage * tx_fees;
        tx_fees = tx_fees.saturating_sub(fees_for_treasury); // This tx_fees is now fees for miner
        let expected_reward_component_for_miner = BlockReward::get().saturating_add(tx_fees);

        // Check Treasury balance
        assert_eq!(
            treasury_balance_after_finalize, fees_for_treasury,
            "Treasury should receive correct percentage of fees"
        );

        // Check miner balance
        assert_eq!(
            miner_balance_after_finalize,
            actual_initial_balance_after_creation + expected_reward_component_for_miner,
            "Miner should receive initial balance + base reward + remaining fees"
        );

        // Verify events
        System::assert_has_event(
            Event::FeesRedirectedToTreasury {
                amount: fees_for_treasury,
            }
            .into(),
        );

        System::assert_has_event(
            Event::MinerRewarded {
                block: 1,
                miner,
                reward: expected_reward_component_for_miner, // This is the reward component, not the final balance change
            }
            .into(),
        );
    });
}
