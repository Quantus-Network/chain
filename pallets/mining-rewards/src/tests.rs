use crate::{mock::*, weights::WeightInfo, Event};
use frame_support::traits::{Currency, Hooks};
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

		// Check that the miner received the block reward (no fees in this test)
		assert_eq!(
			Balances::free_balance(MINER),
			initial_balance + 50 // Initial + base reward only
		);

		// Check the event was emitted
		System::assert_has_event(Event::MinerRewarded { miner: MINER, reward: 50 }.into());
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
		System::assert_has_event(Event::FeesCollected { amount: 25, total: 25 }.into());

		// Run the on_finalize hook
		MiningRewards::on_finalize(1);

		// Check that the miner received the block reward + all fees
		// Current implementation: miner gets base reward (50) + all fees (25)
		assert_eq!(
			Balances::free_balance(MINER),
			initial_balance + 50 + 25 // Initial + base + all fees
		);

		// Check the events were emitted with the correct amounts
		// First event: treasury block reward
		System::assert_has_event(
			Event::TreasuryRewarded {
                reward: 50, // treasury block reward
            }
			.into(),
		);
		// Second event: miner reward for fees
		System::assert_has_event(
			Event::MinerRewarded {
				miner: MINER,
				reward: 25, // all fees go to miner
			}
			.into(),
		);
		// Third event: miner reward for base reward
		System::assert_has_event(
			Event::MinerRewarded {
				miner: MINER,
				reward: 50, // base reward
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

		// Check that the miner received the block reward + all fees
		// Check miner received rewards
		// Current implementation: miner gets base reward (50) + all fees (30)
		assert_eq!(
			Balances::free_balance(MINER),
			initial_balance + 50 + 30 // Initial + base + all fees
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

		// Current implementation: miner gets base reward (50) + all fees (10)
		let balance_after_block_1 = initial_balance + 50 + 10; // Initial + base + all fees
		assert_eq!(Balances::free_balance(MINER), balance_after_block_1);

		// Block 2
		set_miner_digest(MINER);
		MiningRewards::collect_transaction_fees(15);
		MiningRewards::on_finalize(2);

		// Check total rewards for both blocks
		// Block 1: 50 + 10 = 60, Block 2: 50 + 15 = 65, Total: 125
		assert_eq!(
			Balances::free_balance(MINER),
			initial_balance + 50 + 10 + 50 + 15 // Initial + block1 + block2
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
		// Current implementation: miner gets base reward (50) + all fees (10)
		let balance_after_block_1 = initial_balance_miner1 + 50 + 10; // Initial + base + all fees
		assert_eq!(Balances::free_balance(MINER), balance_after_block_1);

		// Block 2 - Second miner
		System::set_block_number(2);
		set_miner_digest(MINER2);
		MiningRewards::collect_transaction_fees(20);
		MiningRewards::on_finalize(2);

		// Check second miner balance
		// Current implementation: miner gets base reward (50) + all fees (20)
		assert_eq!(
			Balances::free_balance(MINER2),
			initial_balance_miner2 + 50 + 20 // Initial + base + all fees
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
		// Check that the miner received the block reward + all collected fees
		// Base reward: 50, Fees: 30 (from the collect_transaction_fees call)
		assert_eq!(
			Balances::free_balance(MINER),
			initial_balance + 50 + 30 // Initial + base + all fees
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
		assert_eq!(weight, <()>::on_finalize_rewarded_miner());

		// 2. Add some transaction fees during block execution
		MiningRewards::collect_transaction_fees(15);

		// 3. on_finalize - should reward the miner
		set_miner_digest(MINER);
		MiningRewards::on_finalize(1);

		// Check miner received rewards
		// Current implementation: miner gets base reward (50) + all fees (15 in this test)
		assert_eq!(
			Balances::free_balance(MINER),
			initial_balance + 50 + 15 // Initial + base reward + fees
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
		// Block 1: 50 (base) + 10 (fees) = 60
		// Block 2: 50 (base) + 0 (no new fees) = 50
		// Total: 110
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
		let initial_treasury_balance = Balances::free_balance(treasury_account);

		// Fund Treasury
		let treasury_funding = 1000 * UNIT;
		let _ = Balances::deposit_creating(&treasury_account, treasury_funding);

		// Create a block without a miner
		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		// Check that Treasury received the rewards
		// When no miner, treasury gets both miner reward and treasury block reward
		let expected_reward = BlockReward::get() + TreasuryBlockReward::get(); // 50 + 50 = 100
		assert_eq!(
			Balances::free_balance(treasury_account),
			initial_treasury_balance + treasury_funding + expected_reward
		);

		// Check that the events were emitted - treasury gets both miner reward and treasury reward
		System::assert_has_event(
			Event::TreasuryRewarded {
                reward: 50, // treasury block reward
            }
			.into(),
		);
		System::assert_has_event(
			Event::TreasuryRewarded {
				reward: 50, // miner reward (goes to treasury when no miner)
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
		let actual_initial_balance_after_creation = Balances::free_balance(miner);

		// Set transaction fees
		let tx_fees = 100;
		MiningRewards::collect_transaction_fees(tx_fees);

		// Create a block with a miner
		System::set_block_number(1);
		set_miner_digest(miner);

		// Run on_finalize
		MiningRewards::on_finalize(System::block_number());

		// Get Treasury account
		let treasury_account: u64 = TreasuryPalletId::get().into_account_truncating();

		// Get actual values from the system AFTER on_finalize
		let treasury_balance_after_finalize = Balances::free_balance(treasury_account);
		let miner_balance_after_finalize = Balances::free_balance(miner);

		// Calculate expected values using the same method as in the implementation
		// Current implementation: miner gets all fees, treasury gets block reward
		let expected_reward_component_for_miner = BlockReward::get().saturating_add(tx_fees);

		// Check Treasury balance - it should have the treasury block reward
		assert_eq!(
			treasury_balance_after_finalize,
			50, // TreasuryBlockReward
			"Treasury should receive block reward"
		);

		// Check miner balance
		assert_eq!(
			miner_balance_after_finalize,
			actual_initial_balance_after_creation + expected_reward_component_for_miner,
			"Miner should receive base reward + all fees"
		);

		// Verify events
		// Check events for proper reward distribution
		System::assert_has_event(
			Event::TreasuryRewarded {
                reward: 50, // treasury block reward
            }
			.into(),
		);

		System::assert_has_event(
			Event::MinerRewarded {
				miner,
				reward: 100, // all fees go to miner
			}
			.into(),
		);

		System::assert_has_event(
			Event::MinerRewarded {
				miner,
				reward: BlockReward::get(), // base reward
			}
			.into(),
		);
	});
}
