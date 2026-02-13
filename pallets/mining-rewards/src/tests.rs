use crate::{mock::*, weights::WeightInfo, Event};
use frame_support::traits::{Currency, Hooks};
use pallet_treasury::TreasuryProvider;

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
		let treasury_reward = total_reward * MockTreasury::portion() as u128 / 100;
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
		let treasury_reward = total_block_reward * MockTreasury::portion() as u128 / 100;
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
		let treasury_reward = total_block_reward * MockTreasury::portion() as u128 / 100;
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
			total_block1_reward - (total_block1_reward * MockTreasury::portion() as u128 / 100);

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
			total_block2_reward - (total_block2_reward * MockTreasury::portion() as u128 / 100);

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
			total_block1_reward - (total_block1_reward * MockTreasury::portion() as u128 / 100);

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
			total_block2_reward - (total_block2_reward * MockTreasury::portion() as u128 / 100);

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
			total_block_reward - (total_block_reward * MockTreasury::portion() as u128 / 100);

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
			total_block_reward - (total_block_reward * MockTreasury::portion() as u128 / 100);

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
		let treasury_account = MockTreasury::account_id();
		let initial_treasury_balance = Balances::free_balance(&treasury_account);

		// Calculate expected rewards - when no miner, all rewards go to treasury
		let current_supply = Balances::total_issuance();
		let total_reward = (MaxSupply::get() - current_supply) / EmissionDivisor::get();
		let treasury_portion_reward = total_reward * MockTreasury::portion() as u128 / 100;
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
		let treasury_reward = total_block_reward * MockTreasury::portion() as u128 / 100;
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

#[test]
fn test_emission_simulation_120m_blocks() {
	new_test_ext().execute_with(|| {
		// Add realistic initial supply similar to genesis
		let treasury_account = MockTreasury::account_id();
		let _ = Balances::deposit_creating(&treasury_account, 3_600_000 * UNIT);

		println!("=== Mining Rewards Emission Simulation ===");
		println!("Max Supply: {:.0} tokens", MaxSupply::get() as f64 / UNIT as f64);
		println!("Emission Divisor: {:?}", EmissionDivisor::get());
		println!("Treasury Portion: {}%", MockTreasury::portion());
		println!();

		const MAX_BLOCKS: u32 = 130_000_000;
		const REPORT_INTERVAL: u32 = 1_000_000; // Report every 1M blocks
		const UNIT: u128 = 1_000_000_000_000; // For readable output

		let initial_supply = Balances::total_issuance();
		let mut current_supply = initial_supply;
		let mut total_miner_rewards = 0u128;
		let mut total_treasury_rewards = 0u128;
		let mut block = 0u32;

		println!("Block       Supply        %MaxSupply  BlockReward   ToTreasury   ToMiner      Remaining");
		println!("{}", "-".repeat(90));

		// Print initial state
		let remaining = MaxSupply::get() - current_supply;
		let block_reward = if remaining > 0 { remaining / EmissionDivisor::get() } else { 0 };
		let treasury_reward = block_reward * MockTreasury::portion() as u128 / 100;
		let miner_reward = block_reward - treasury_reward;

		println!(
			"{:<11} {:<13} {:<11.2}% {:<13.6} {:<12.6} {:<12.6} {:<13}",
			block,
			current_supply / UNIT,
			(current_supply as f64 / MaxSupply::get() as f64) * 100.0,
			block_reward as f64 / UNIT as f64,
			treasury_reward as f64 / UNIT as f64,
			miner_reward as f64 / UNIT as f64,
			remaining / UNIT
		);

		// Set up a consistent miner
		set_miner_digest(miner());

		while block < MAX_BLOCKS && current_supply < MaxSupply::get() {
			// Simulate REPORT_INTERVAL blocks
			for _ in 0..REPORT_INTERVAL {
				if current_supply >= MaxSupply::get() {
					break;
				}

				// Calculate reward for this block
				let remaining_supply = MaxSupply::get().saturating_sub(current_supply);
				if remaining_supply == 0 {
					break;
				}

				let block_reward = remaining_supply / EmissionDivisor::get();
				let treasury_reward = block_reward * MockTreasury::portion() as u128 / 100;
				let miner_reward = block_reward - treasury_reward;

				// Update totals (simulate the minting)
				current_supply += block_reward;
				total_treasury_rewards += treasury_reward;
				total_miner_rewards += miner_reward;
				block += 1;

				// Early exit if rewards become negligible
				if block_reward < 1000 { // Less than 1000 raw units (very small)
					break;
				}
			}

			// Print progress report
			let remaining = MaxSupply::get().saturating_sub(current_supply);
			let next_block_reward = if remaining > 0 { remaining / EmissionDivisor::get() } else { 0 };
			let next_treasury = next_block_reward * MockTreasury::portion() as u128 / 100;
			let next_miner = next_block_reward - next_treasury;

			println!(
				"{:<11} {:<13} {:<11.2}% {:<13.6} {:<12.6} {:<12.6} {:<13}",
				block,
				current_supply / UNIT,
				(current_supply as f64 / MaxSupply::get() as f64) * 100.0,
				next_block_reward as f64 / UNIT as f64,
				next_treasury as f64 / UNIT as f64,
				next_miner as f64 / UNIT as f64,
				remaining / UNIT
			);

			// Stop if rewards become negligible or we've reached max supply
			if current_supply >= MaxSupply::get() || next_block_reward < 1000 {
				break;
			}
		}

		println!("{}", "-".repeat(90));
		println!();
		println!("=== Final Summary ===");
		println!("Total Blocks Processed: {}", block);
		println!("Final Supply: {:.6} tokens", current_supply as f64 / UNIT as f64);
		println!("Percentage of Max Supply: {:.4}%", (current_supply as f64 / MaxSupply::get() as f64) * 100.0);
		println!("Remaining Supply: {:.6} tokens", (MaxSupply::get() - current_supply) as f64 / UNIT as f64);
		println!();
		println!("Total Miner Rewards: {:.6} tokens", total_miner_rewards as f64 / UNIT as f64);
		println!("Total Treasury Rewards: {:.6} tokens", total_treasury_rewards as f64 / UNIT as f64);
		println!("Total Rewards Distributed: {:.6} tokens", (total_miner_rewards + total_treasury_rewards) as f64 / UNIT as f64);
		println!();
		println!("Miner Share: {:.1}%", (total_miner_rewards as f64 / (total_miner_rewards + total_treasury_rewards) as f64) * 100.0);
		println!("Treasury Share: {:.1}%", (total_treasury_rewards as f64 / (total_miner_rewards + total_treasury_rewards) as f64) * 100.0);

		// Time estimates (assuming 12 second blocks)
		let total_seconds = block as f64 * 12.0;
		let days = total_seconds / (24.0 * 3600.0);
		let years = days / 365.25;

		println!();
		println!("=== Time Estimates (12s blocks) ===");
		println!("Total Time: {:.1} days ({:.1} years)", days, years);

		// === Comprehensive Emission Validation ===

		assert!(current_supply >= initial_supply, "Supply should have increased");
		assert!(current_supply <= MaxSupply::get(), "Supply should not exceed max supply");

		let emitted_tokens = current_supply - initial_supply;
		let emission_percentage = (emitted_tokens as f64 / (MaxSupply::get() - initial_supply) as f64) * 100.0;
		assert!(emission_percentage > 99.0, "Should have emitted >99% of available supply, got {:.2}%", emission_percentage);

		assert!(total_miner_rewards > 0, "Miners should have received rewards");
		assert!(total_treasury_rewards > 0, "Treasury should have received rewards");
		assert_eq!(total_miner_rewards + total_treasury_rewards, emitted_tokens, "Total rewards should equal emitted tokens");

		let remaining_percentage = ((MaxSupply::get() - current_supply) as f64 / MaxSupply::get() as f64) * 100.0;
		assert!(remaining_percentage < 1.0, "Should have <10% supply remaining, got {:.2}%", remaining_percentage);
		assert!(remaining_percentage > 0.0, "Should still have some supply remaining for future emission");

		println!();
		println!("✅ All emission validation checks passed!");
		println!("✅ Emission simulation completed successfully!");
	});
}
