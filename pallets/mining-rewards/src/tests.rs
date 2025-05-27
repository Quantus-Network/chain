use crate::{mock::*, Event};
use frame_support::traits::Hooks;
use frame_support::weights::Weight;
use sp_runtime::traits::AccountIdConversion;
use frame_support::traits::Currency;

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

        // Check that the miner received the block reward + fees
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 25 // Initial + base + fees
        );

        // Check the event was emitted with the correct amount
        System::assert_has_event(
            Event::MinerRewarded {
                block: 1,
                miner: MINER,
                reward: 75,
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

        // Check that the miner received the block reward + fees
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 30 // Initial + base + fees
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

        let balance_after_block_1 = initial_balance + 50 + 10; // Initial + base + fees
        assert_eq!(Balances::free_balance(MINER), balance_after_block_1);

        // Block 2
        System::set_block_number(2);
        set_miner_digest(MINER);
        MiningRewards::collect_transaction_fees(15);
        MiningRewards::on_finalize(2);

        assert_eq!(
            Balances::free_balance(MINER),
            balance_after_block_1 + 50 + 15 // Balance after block 1 + base + fees
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
        let balance_after_block_1 = initial_balance_miner1 + 50 + 10; // Initial + base + fees
        assert_eq!(Balances::free_balance(MINER), balance_after_block_1);

        // Block 2 - Second miner
        System::set_block_number(2);
        set_miner_digest(MINER2);
        MiningRewards::collect_transaction_fees(20);
        MiningRewards::on_finalize(2);

        // Check second miner balance
        assert_eq!(
            Balances::free_balance(MINER2),
            initial_balance_miner2 + 50 + 20 // Initial + base + fees
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

        // Check miner got all fees plus base reward
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 30 // Initial + base + fees
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
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 50 + 15 // Initial + base + fees
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
        // Block 1: Initial + 50 (base) + 10 (fees) = Initial + 60
        // Block 2: (Initial + 60) + 50 (base) = Initial + 110
        assert_eq!(
            Balances::free_balance(MINER),
            initial_balance + 110 // Initial + 50 + 10 + 50
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
        System::assert_has_event(Event::TreasuryRewarded {
            block: 1,
            reward: expected_reward,
        }.into());
    });
}