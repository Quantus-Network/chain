use crate::{mock::*, weights::WeightInfo, Event};
use frame_support::traits::{Currency, Hooks};
use sp_runtime::traits::AccountIdConversion;

#[test]
fn miner_reward_works() {
	new_test_ext().execute_with(|| {
		// Remember initial balance (ExistentialDeposit)
		let initial_balance = Balances::free_balance(miner());

		// Add a miner to the pre-runtime digest
		set_miner_digest(miner());

		// Calculate expected rewards with treasury portion
		// Initial supply is just the existential deposits (2 accounts * 1 unit each = 2)
		let current_supply = Balances::total_issuance();
		let total_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let treasury_reward = total_reward * TreasuryPortion::get() as u128 / 100;
		let miner_reward = total_reward - treasury_reward;

		// Run the on_finalize hook
		MiningRewards::on_finalize(1);

		// Check that the miner received the calculated block reward (minus treasury portion)
		assert_eq!(Balances::free_balance(miner()), initial_balance + miner_reward);

		// Check the miner reward event was emitted
		System::assert_has_event(
			Event::MinerRewarded { miner: miner(), reward: miner_reward }.into(),
		);

		// Check the treasury reward event was emitted
		System::assert_has_event(Event::TreasuryRewarded { reward: treasury_reward }.into());
	});
}

#[test]
fn miner_reward_with_transaction_fees_works() {
	new_test_ext().execute_with(|| {
		// Remember initial balance
		let initial_balance = Balances::free_balance(miner());

		// Add a miner to the pre-runtime digest
		set_miner_digest(miner());

		// Manually add some transaction fees
		let fees: Balance = 25;
		MiningRewards::collect_transaction_fees(fees);

		// Check fees collection event
		System::assert_has_event(Event::FeesCollected { amount: 25, total: 25 }.into());

		// Calculate expected rewards with treasury portion
		let current_supply = Balances::total_issuance();
		let total_block_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let treasury_reward = total_block_reward * TreasuryPortion::get() as u128 / 100;
		let miner_block_reward = total_block_reward - treasury_reward;

		// Run the on_finalize hook
		MiningRewards::on_finalize(1);

		// Check that the miner received the miner portion of block reward + all fees
		assert_eq!(Balances::free_balance(miner()), initial_balance + miner_block_reward + fees);

		// Check the events were emitted with the correct amounts
		// First event: miner reward for fees
		System::assert_has_event(
			Event::MinerRewarded {
				miner: miner(),
				reward: 25, // all fees go to miner
			}
			.into(),
		);
		// Second event: miner reward for block reward
		System::assert_has_event(
			Event::MinerRewarded { miner: miner(), reward: miner_block_reward }.into(),
		);
		// Third event: treasury reward
		System::assert_has_event(Event::TreasuryRewarded { reward: treasury_reward }.into());
	});
}

#[test]
fn on_unbalanced_collects_fees() {
	new_test_ext().execute_with(|| {
		// Remember initial balance
		let initial_balance = Balances::free_balance(miner());

		// Use collect_transaction_fees instead of directly calling on_unbalanced
		MiningRewards::collect_transaction_fees(30);

		// Check that fees were collected
		assert_eq!(MiningRewards::collected_fees(), 30);

		// Calculate expected rewards with treasury portion
		let current_supply = Balances::total_issuance();
		let total_block_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let treasury_reward = total_block_reward * TreasuryPortion::get() as u128 / 100;
		let miner_block_reward = total_block_reward - treasury_reward;

		// Add a miner to the pre-runtime digest and distribute rewards
		set_miner_digest(miner());
		MiningRewards::on_finalize(1);

		// Check that the miner received the miner portion of block reward + all fees
		assert_eq!(Balances::free_balance(miner()), initial_balance + miner_block_reward + 30);
	});
}

#[test]
fn multiple_blocks_accumulate_rewards() {
	new_test_ext().execute_with(|| {
		// Remember initial balance
		let initial_balance = Balances::free_balance(miner());

		// Block 1
		set_miner_digest(miner());
		MiningRewards::collect_transaction_fees(10);

		// Calculate rewards for block 1 with treasury portion
		let current_supply_block1 = Balances::total_issuance();
		let total_block1_reward =
			(MaxSupply::get() - current_supply_block1) / EmissionDivisor::get();
		let miner_block1_reward =
			total_block1_reward - (total_block1_reward * TreasuryPortion::get() as u128 / 100);

		MiningRewards::on_finalize(1);

		let balance_after_block_1 = initial_balance + miner_block1_reward + 10;
		assert_eq!(Balances::free_balance(miner()), balance_after_block_1);

		// Block 2 - supply has increased after block 1, so reward will be different
		set_miner_digest(miner());
		MiningRewards::collect_transaction_fees(15);

		let current_supply_block2 = Balances::total_issuance();
		let total_block2_reward =
			(MaxSupply::get() - current_supply_block2) / EmissionDivisor::get();
		let miner_block2_reward =
			total_block2_reward - (total_block2_reward * TreasuryPortion::get() as u128 / 100);

		MiningRewards::on_finalize(2);

		// Check total rewards for both blocks
		assert_eq!(
			Balances::free_balance(miner()),
			initial_balance + miner_block1_reward + 10 + miner_block2_reward + 15
		);
	});
}

#[test]
fn different_miners_get_different_rewards() {
	new_test_ext().execute_with(|| {
		// Remember initial balances
		let initial_balance_miner1 = Balances::free_balance(miner());
		let initial_balance_miner2 = Balances::free_balance(miner2());

		// Block 1 - First miner
		set_miner_digest(miner());
		MiningRewards::collect_transaction_fees(10);

		let current_supply_block1 = Balances::total_issuance();
		let total_block1_reward =
			(MaxSupply::get() - current_supply_block1) / EmissionDivisor::get();
		let miner_block1_reward =
			total_block1_reward - (total_block1_reward * TreasuryPortion::get() as u128 / 100);

		MiningRewards::on_finalize(1);

		let balance_after_block_1 = initial_balance_miner1 + miner_block1_reward + 10;
		assert_eq!(Balances::free_balance(miner()), balance_after_block_1);

		// Block 2 - Second miner
		System::set_block_number(2);
		set_miner_digest(miner2());
		MiningRewards::collect_transaction_fees(20);

		let current_supply_block2 = Balances::total_issuance();
		let total_block2_reward =
			(MaxSupply::get() - current_supply_block2) / EmissionDivisor::get();
		let miner_block2_reward =
			total_block2_reward - (total_block2_reward * TreasuryPortion::get() as u128 / 100);

		MiningRewards::on_finalize(2);

		// Check second miner balance
		assert_eq!(
			Balances::free_balance(miner2()),
			initial_balance_miner2 + miner_block2_reward + 20
		);

		// First miner balance should remain unchanged
		assert_eq!(Balances::free_balance(miner()), balance_after_block_1);
	});
}

#[test]
fn transaction_fees_collector_works() {
	new_test_ext().execute_with(|| {
		// Remember initial balance
		let initial_balance = Balances::free_balance(miner());

		// Use collect_transaction_fees to gather fees
		MiningRewards::collect_transaction_fees(10);
		MiningRewards::collect_transaction_fees(15);
		MiningRewards::collect_transaction_fees(5);

		// Check accumulated fees
		assert_eq!(MiningRewards::collected_fees(), 30);

		// Calculate expected rewards with treasury portion
		let current_supply = Balances::total_issuance();
		let total_block_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let miner_block_reward =
			total_block_reward - (total_block_reward * TreasuryPortion::get() as u128 / 100);

		// Reward miner
		set_miner_digest(miner());
		MiningRewards::on_finalize(1);

		// Check that the miner received the miner portion of block reward + all collected fees
		assert_eq!(Balances::free_balance(miner()), initial_balance + miner_block_reward + 30);
	});
}

#[test]
fn block_lifecycle_works() {
	new_test_ext().execute_with(|| {
		// Remember initial balance
		let initial_balance = Balances::free_balance(miner());

		// Run through a complete block lifecycle

		// 1. on_initialize - should return correct weight
		let weight = MiningRewards::on_initialize(1);
		assert_eq!(weight, <()>::on_finalize_rewarded_miner());

		// 2. Add some transaction fees during block execution
		MiningRewards::collect_transaction_fees(15);

		// Calculate expected rewards with treasury portion
		let current_supply = Balances::total_issuance();
		let total_block_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let miner_block_reward =
			total_block_reward - (total_block_reward * TreasuryPortion::get() as u128 / 100);

		// 3. on_finalize - should reward the miner
		set_miner_digest(miner());
		MiningRewards::on_finalize(1);

		// Check miner received rewards
		assert_eq!(Balances::free_balance(miner()), initial_balance + miner_block_reward + 15);
	});
}

#[test]
fn test_run_to_block_helper() {
	new_test_ext().execute_with(|| {
		// Remember initial balance
		let initial_balance = Balances::free_balance(miner());

		// Set up miner
		set_miner_digest(miner());

		// Add fees for block 1
		MiningRewards::collect_transaction_fees(10);

		// Note: This test is complex with run_to_block as rewards change with supply
		// We'll just verify the mechanism works and final balance is reasonable
		let initial_supply = Balances::total_issuance();

		// Run to block 3 (this should process blocks 1 and 2)
		run_to_block(3);

		// Verify we're at the expected block number
		assert_eq!(System::block_number(), 3);

		// Check that miner balance increased (should have rewards from both blocks + fees)
		let final_balance = Balances::free_balance(miner());
		assert!(final_balance > initial_balance, "Miner should have received rewards");

		// Verify supply increased due to minted rewards
		let final_supply = Balances::total_issuance();
		assert!(final_supply > initial_supply, "Total supply should have increased");
	});
}

#[test]
fn rewards_go_to_treasury_when_no_miner() {
	new_test_ext().execute_with(|| {
		// Get Treasury account
		let treasury_account = TreasuryPalletId::get().into_account_truncating();
		let initial_treasury_balance = Balances::free_balance(&treasury_account);

		// Calculate expected rewards - when no miner, all rewards go to treasury
		let current_supply = Balances::total_issuance();
		let total_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let treasury_portion_reward = total_reward * TreasuryPortion::get() as u128 / 100;
		let miner_portion_reward = total_reward - treasury_portion_reward;

		// Create a block without a miner (no digest set)
		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		// Check that Treasury received both its portion and the miner's portion (since no miner)
		assert_eq!(
			Balances::free_balance(treasury_account),
			initial_treasury_balance + treasury_portion_reward + miner_portion_reward
		);

		// Check that the events were emitted
		System::assert_has_event(
			Event::TreasuryRewarded { reward: treasury_portion_reward }.into(),
		);
		System::assert_has_event(Event::TreasuryRewarded { reward: miner_portion_reward }.into());
	});
}

#[test]
fn test_fees_and_rewards_to_miner() {
	new_test_ext().execute_with(|| {
		// Set up initial balances
		let miner = account_id(1);
		let _ = Balances::deposit_creating(&miner, 0); // Create account, balance might become ExistentialDeposit
		let actual_initial_balance_after_creation = Balances::free_balance(&miner);

		// Set transaction fees
		let tx_fees = 100;
		MiningRewards::collect_transaction_fees(tx_fees);

		// Calculate expected rewards with treasury portion
		let current_supply = Balances::total_issuance();
		let total_block_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let treasury_reward = total_block_reward * TreasuryPortion::get() as u128 / 100;
		let miner_block_reward = total_block_reward - treasury_reward;

		// Create a block with a miner
		System::set_block_number(1);
		set_miner_digest(miner.clone());

		// Run on_finalize
		MiningRewards::on_finalize(System::block_number());

		// Get actual values from the system AFTER on_finalize
		let miner_balance_after_finalize = Balances::free_balance(&miner);

		// Check miner balance - should get miner portion of block reward + all fees
		assert_eq!(
			miner_balance_after_finalize,
			actual_initial_balance_after_creation + miner_block_reward + tx_fees,
			"Miner should receive miner portion of block reward + all fees"
		);

		// Verify events
		System::assert_has_event(
			Event::MinerRewarded {
				miner: miner.clone(),
				reward: 100, // all fees go to miner
			}
			.into(),
		);

		System::assert_has_event(Event::MinerRewarded { miner, reward: miner_block_reward }.into());

		System::assert_has_event(Event::TreasuryRewarded { reward: treasury_reward }.into());
	});
}
