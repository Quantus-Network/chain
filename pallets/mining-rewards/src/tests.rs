use crate::{mock::*, weights::WeightInfo, Event};
use frame_support::traits::{Currency, Hooks};
use qp_wormhole::derive_wormhole_address;
use sp_runtime::testing::Digest;

/// Block reward `on_finalize` will compute from the current issuance (and any
/// collected fees, which the pallet treats as already-burned supply).
fn expected_block_reward(tx_fees: Balance) -> Balance {
	let current_supply = Balances::total_issuance().saturating_add(tx_fees);
	(MaxSupply::get() - current_supply) / EmissionDivisor::get()
}

fn leaf_quantum() -> Balance {
	pallet_zk_tree::tree::AMOUNT_SCALE_DOWN_FACTOR
}

fn quantize(amount: Balance) -> (Balance, Balance) {
	let quantum = leaf_quantum();
	let dust = amount % quantum;
	(amount - dust, dust)
}

/// What `on_finalize` actually credits the miner: fees + emission, floored to the leaf quantum.
fn miner_payout(tx_fees: Balance) -> (Balance, Balance) {
	quantize(expected_block_reward(tx_fees) + tx_fees)
}

#[test]
fn miner_reward_works() {
	new_test_ext().execute_with(|| {
		let initial_balance = Balances::free_balance(MINER_1.account_id());
		set_miner_preimage_digest(MINER_1.preimage());

		let (miner_reward, _) = miner_payout(0);

		MiningRewards::on_finalize(1);

		assert_eq!(Balances::free_balance(MINER_1.account_id()), initial_balance + miner_reward);
		System::assert_has_event(
			Event::MinerRewarded { miner: MINER_1.account_id(), reward: miner_reward }.into(),
		);
	});
}

#[test]
fn miner_reward_with_transaction_fees_works() {
	new_test_ext().execute_with(|| {
		let initial_balance = Balances::free_balance(MINER_1.account_id());
		set_miner_preimage_digest(MINER_1.preimage());

		let fees: Balance = 25;
		MiningRewards::collect_transaction_fees(fees);
		System::assert_has_event(Event::FeesCollected { amount: 25, total: 25 }.into());

		let (miner_reward, _) = miner_payout(fees);

		MiningRewards::on_finalize(1);

		assert_eq!(Balances::free_balance(MINER_1.account_id()), initial_balance + miner_reward);
		System::assert_has_event(
			Event::MinerRewarded { miner: MINER_1.account_id(), reward: miner_reward }.into(),
		);
	});
}

#[test]
fn on_unbalanced_collects_fees() {
	new_test_ext().execute_with(|| {
		let initial_balance = Balances::free_balance(MINER_1.account_id());
		MiningRewards::collect_transaction_fees(30);
		assert_eq!(MiningRewards::collected_fees(), 30);

		let (miner_reward, _) = miner_payout(30);
		set_miner_preimage_digest(MINER_1.preimage());
		MiningRewards::on_finalize(1);

		assert_eq!(Balances::free_balance(MINER_1.account_id()), initial_balance + miner_reward);
	});
}

#[test]
fn multiple_blocks_accumulate_rewards() {
	new_test_ext().execute_with(|| {
		let initial_balance = Balances::free_balance(MINER_1.account_id());

		set_miner_preimage_digest(MINER_1.preimage());
		MiningRewards::collect_transaction_fees(10);
		let (miner_block1_reward, _) = miner_payout(10);
		MiningRewards::on_finalize(1);

		let balance_after_block_1 = initial_balance + miner_block1_reward;
		assert_eq!(Balances::free_balance(MINER_1.account_id()), balance_after_block_1);

		set_miner_preimage_digest(MINER_1.preimage());
		MiningRewards::collect_transaction_fees(15);
		let (miner_block2_reward, _) = miner_payout(15);
		MiningRewards::on_finalize(2);

		assert_eq!(
			Balances::free_balance(MINER_1.account_id()),
			initial_balance + miner_block1_reward + miner_block2_reward
		);
	});
}

#[test]
fn different_miners_get_different_rewards() {
	new_test_ext().execute_with(|| {
		let initial_balance_miner1 = Balances::free_balance(MINER_1.account_id());
		let initial_balance_miner2 = Balances::free_balance(MINER_2.account_id());

		set_miner_preimage_digest(MINER_1.preimage());
		MiningRewards::collect_transaction_fees(10);
		let (miner_block1_reward, _) = miner_payout(10);
		MiningRewards::on_finalize(1);

		let balance_after_block_1 = initial_balance_miner1 + miner_block1_reward;
		assert_eq!(Balances::free_balance(MINER_1.account_id()), balance_after_block_1);

		let block_1 = System::finalize();
		System::initialize(&2, &block_1.hash(), &Digest { logs: vec![] });
		set_miner_preimage_digest(MINER_2.preimage());
		MiningRewards::collect_transaction_fees(20);
		let (miner_block2_reward, _) = miner_payout(20);
		MiningRewards::on_finalize(2);

		assert_eq!(
			Balances::free_balance(MINER_2.account_id()),
			initial_balance_miner2 + miner_block2_reward
		);
		assert_eq!(Balances::free_balance(MINER_1.account_id()), balance_after_block_1);
	});
}

#[test]
fn transaction_fees_collector_works() {
	new_test_ext().execute_with(|| {
		let initial_balance = Balances::free_balance(MINER_1.account_id());

		MiningRewards::collect_transaction_fees(10);
		MiningRewards::collect_transaction_fees(15);
		MiningRewards::collect_transaction_fees(5);
		assert_eq!(MiningRewards::collected_fees(), 30);

		let (miner_reward, _) = miner_payout(30);
		set_miner_preimage_digest(MINER_1.preimage());
		MiningRewards::on_finalize(1);

		assert_eq!(Balances::free_balance(MINER_1.account_id()), initial_balance + miner_reward);
	});
}

#[test]
fn on_initialize_returns_correct_weight() {
	new_test_ext().execute_with(|| {
		let weight = MiningRewards::on_initialize(1);
		assert_eq!(weight, <()>::on_finalize_rewarded_miner());
	});
}

#[test]
fn test_run_to_block_helper() {
	new_test_ext().execute_with(|| {
		let initial_balance = Balances::free_balance(MINER_1.account_id());
		set_miner_preimage_digest(MINER_1.preimage());
		MiningRewards::collect_transaction_fees(10);
		let initial_supply = Balances::total_issuance();

		run_to_block(3);

		assert_eq!(System::block_number(), 3);
		assert!(
			Balances::free_balance(MINER_1.account_id()) > initial_balance,
			"Miner should have received rewards"
		);
		assert!(Balances::total_issuance() > initial_supply, "Total supply should have increased");
	});
}

#[test]
fn rewards_are_deferred_when_no_miner() {
	new_test_ext().execute_with(|| {
		let issuance_before = Balances::total_issuance();
		let miner_before = Balances::free_balance(MINER_1.account_id());
		let total_reward = expected_block_reward(0);

		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		assert_eq!(
			Balances::total_issuance(),
			issuance_before,
			"nothing is minted without a miner"
		);
		assert_eq!(Balances::free_balance(MINER_1.account_id()), miner_before);
		assert_eq!(MiningRewards::collected_fees(), total_reward);
		System::assert_has_event(Event::PayoutDeferred { amount: total_reward }.into());
	});
}

/// EQ-QNT-MINING-R-02: transaction fees stay with the miner credit when no miner is present.
#[test]
fn fees_are_deferred_when_no_miner() {
	new_test_ext().execute_with(|| {
		let issuance_before = Balances::total_issuance();
		let tx_fees: u128 = 500;
		let total_reward = expected_block_reward(tx_fees);
		MiningRewards::collect_transaction_fees(tx_fees);

		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		assert_eq!(Balances::total_issuance(), issuance_before);
		assert_eq!(MiningRewards::collected_fees(), total_reward + tx_fees);
		System::assert_has_event(Event::PayoutDeferred { amount: total_reward + tx_fees }.into());
	});
}

/// Failed miner mints are retained in CollectedFees and recovered later.
#[test]
fn failed_miner_mint_is_retained_and_recovered() {
	new_test_ext().execute_with(|| {
		let miner = MINER_1.account_id();
		let miner_before = Balances::free_balance(&miner);
		let issuance_before = Balances::total_issuance();

		let tx_fees: Balance = 1_000 * Unit::get();
		MiningRewards::collect_transaction_fees(tx_fees);
		set_miner_preimage_digest(MINER_1.preimage());

		let lost = expected_block_reward(tx_fees) + tx_fees;
		ExistentialDeposit::set(MaxSupply::get());
		MiningRewards::on_finalize(1);

		assert_eq!(Balances::free_balance(&miner), miner_before);
		assert_eq!(Balances::total_issuance(), issuance_before);
		System::assert_has_event(
			Event::MinerMintFailed { miner: miner.clone(), reward: quantize(lost).0 }.into(),
		);
		assert_eq!(
			MiningRewards::collected_fees(),
			lost,
			"quantized credit plus dust must both be retained for retry"
		);

		ExistentialDeposit::set(1);
		set_miner_preimage_digest(MINER_1.preimage());
		let (paid, dust) = quantize(lost + expected_block_reward(lost));
		MiningRewards::on_finalize(2);

		assert_eq!(Balances::free_balance(&miner), miner_before + paid);
		assert_eq!(MiningRewards::collected_fees(), dust);
	});
}

#[test]
fn unminted_rewards_accumulate_across_consecutive_blocks_without_a_miner() {
	new_test_ext().execute_with(|| {
		MiningRewards::on_finalize(1);
		let retained_after_1 = MiningRewards::collected_fees();
		assert!(retained_after_1 > 0, "a miner-less block must retain its rewards");

		MiningRewards::on_finalize(2);
		let retained_after_2 = MiningRewards::collected_fees();
		assert!(
			retained_after_2 > retained_after_1,
			"a second miner-less block must add its own rewards to the retained pool"
		);

		let miner_before = Balances::free_balance(MINER_1.account_id());
		set_miner_preimage_digest(MINER_1.preimage());
		let (paid, dust) = quantize(retained_after_2 + expected_block_reward(retained_after_2));
		MiningRewards::on_finalize(3);

		assert_eq!(Balances::free_balance(MINER_1.account_id()), miner_before + paid);
		assert_eq!(MiningRewards::collected_fees(), dust);
	});
}

#[test]
fn retried_rewards_follow_fee_destination_to_next_miner() {
	new_test_ext().execute_with(|| {
		MiningRewards::on_finalize(1);
		let retained = MiningRewards::collected_fees();
		assert!(retained > 0);

		let miner_before = Balances::free_balance(MINER_1.account_id());
		set_miner_preimage_digest(MINER_1.preimage());
		let (paid, dust) = quantize(retained + expected_block_reward(retained));
		MiningRewards::on_finalize(2);

		assert_eq!(MiningRewards::collected_fees(), dust);
		assert_eq!(
			Balances::free_balance(MINER_1.account_id()),
			miner_before + paid,
			"deferred rewards must reach the next block's miner via the fee path"
		);
	});
}

// =========================================================================
// EQ-QNT-WORMHOLE-F-03: Tests for extract_author_from_digest edge cases
// =========================================================================

#[test]
fn incorrect_engine_id_ignored() {
	new_test_ext().execute_with(|| {
		let total_reward = expected_block_reward(0);

		let wrong_engine_id: [u8; 4] = *b"FAKE";
		set_digest_with_engine_id(wrong_engine_id, MINER_1.preimage().to_vec());

		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		assert_eq!(MiningRewards::collected_fees(), total_reward);
		assert_eq!(
			Balances::free_balance(MINER_1.account_id()),
			ExistentialDeposit::get(),
			"Miner should not receive rewards when engine ID is incorrect"
		);
		System::assert_has_event(Event::PayoutDeferred { amount: total_reward }.into());
	});
}

#[test]
fn malformed_preimage_data_ignored() {
	new_test_ext().execute_with(|| {
		use sp_consensus_qpow::POW_ENGINE_ID;

		let total_reward = expected_block_reward(0);

		let short_data: Vec<u8> = vec![1, 2, 3, 4, 5];
		set_digest_with_engine_id(POW_ENGINE_ID, short_data);

		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		assert_eq!(MiningRewards::collected_fees(), total_reward);
		System::assert_has_event(Event::PayoutDeferred { amount: total_reward }.into());
	});
}

#[test]
fn empty_digest_defers_payout() {
	new_test_ext().execute_with(|| {
		let total_reward = expected_block_reward(0);

		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		assert_eq!(MiningRewards::collected_fees(), total_reward);
		System::assert_has_event(Event::PayoutDeferred { amount: total_reward }.into());
	});
}

#[test]
fn oversized_preimage_data_ignored() {
	new_test_ext().execute_with(|| {
		use sp_consensus_qpow::POW_ENGINE_ID;

		let total_reward = expected_block_reward(0);

		let long_data: Vec<u8> = vec![42u8; 64];
		set_digest_with_engine_id(POW_ENGINE_ID, long_data);

		System::set_block_number(1);
		MiningRewards::on_finalize(System::block_number());

		assert_eq!(MiningRewards::collected_fees(), total_reward);
		System::assert_has_event(Event::PayoutDeferred { amount: total_reward }.into());
	});
}

#[test]
fn test_fees_and_rewards_to_miner() {
	new_test_ext().execute_with(|| {
		let test_preimage = [42u8; 32];
		let miner_wormhole_address = sp_core::crypto::AccountId32::from(
			derive_wormhole_address(test_preimage).expect("test preimage limbs are canonical"),
		);
		let _ = Balances::deposit_creating(&miner_wormhole_address, 0);
		let actual_initial_balance_after_creation = Balances::free_balance(&miner_wormhole_address);

		let tx_fees = 100;
		MiningRewards::collect_transaction_fees(tx_fees);
		let (miner_reward, _) = miner_payout(tx_fees);

		System::set_block_number(1);
		set_miner_preimage_digest(test_preimage);
		MiningRewards::on_finalize(System::block_number());

		assert_eq!(
			Balances::free_balance(&miner_wormhole_address),
			actual_initial_balance_after_creation + miner_reward,
			"Miner should receive the quantized block reward + fees"
		);
		System::assert_has_event(
			Event::MinerRewarded { miner: miner_wormhole_address, reward: miner_reward }.into(),
		);
	});
}

#[test]
fn miner_payout_is_quantized_and_dust_is_held() {
	new_test_ext().execute_with(|| {
		let quantum = leaf_quantum();
		let initial_miner = Balances::free_balance(MINER_1.account_id());
		set_miner_preimage_digest(MINER_1.preimage());

		let fees: Balance = 25;
		MiningRewards::collect_transaction_fees(fees);
		let (quantized, dust) = miner_payout(fees);
		assert!(dust > 0, "test setup must produce a sub-quantum remainder");
		assert_eq!(quantized % quantum, 0);

		MiningRewards::on_finalize(1);

		assert_eq!(Balances::free_balance(MINER_1.account_id()), initial_miner + quantized);
		assert_eq!(MiningRewards::collected_fees(), dust);
		System::assert_has_event(
			Event::MinerRewarded { miner: MINER_1.account_id(), reward: quantized }.into(),
		);
	});
}

#[test]
fn combined_fee_and_reward_can_recover_a_quantum() {
	new_test_ext().execute_with(|| {
		let quantum = leaf_quantum();
		set_miner_preimage_digest(MINER_1.preimage());

		// Fee remainder is quantum-1, so any non-zero reward remainder crosses a quantum.
		let fees = quantum - 1;
		MiningRewards::collect_transaction_fees(fees);

		let reward = expected_block_reward(fees);
		assert!(reward % quantum > 0, "block reward must not already be aligned");
		let (combined, _dust) = quantize(reward + fees);
		let (reward_only, _) = quantize(reward);
		let (fee_only, _) = quantize(fees);
		assert!(
			combined > reward_only + fee_only,
			"summing before the floor must recover a quantum that two floors would drop"
		);

		let initial_miner = Balances::free_balance(MINER_1.account_id());
		MiningRewards::on_finalize(1);
		assert_eq!(Balances::free_balance(MINER_1.account_id()), initial_miner + combined);
	});
}

#[test]
fn sub_quantum_credit_does_not_record_a_leaf() {
	new_test_ext().execute_with(|| {
		MockProofRecorder::clear();
		set_miner_preimage_digest(MINER_1.preimage());

		let fees: Balance = 25;
		MiningRewards::collect_transaction_fees(fees);
		let (quantized, dust) = miner_payout(fees);
		assert!(dust > 0 && dust < leaf_quantum());

		MiningRewards::on_finalize(1);

		let proofs = MockProofRecorder::get_recorded_proofs();
		let miner_proofs: Vec<_> = proofs.iter().filter(|p| p.to == MINER_1.account_id()).collect();
		assert_eq!(miner_proofs.len(), 1);
		assert_eq!(miner_proofs[0].amount, quantized);
		assert_eq!(miner_proofs[0].amount % leaf_quantum(), 0);
		assert_eq!(MiningRewards::collected_fees(), dust);
		assert_eq!(proofs.len(), 1, "held dust must not record a wormhole leaf");
	});
}

#[test]
#[ignore] // This test takes a very long time (~120M blocks simulation), run manually with --ignored
fn test_emission_simulation_120m_blocks() {
	new_test_ext().execute_with(|| {
		println!("=== Mining Rewards Emission Simulation ===");
		println!("Max Supply: {:.0} tokens", MaxSupply::get() as f64 / UNIT as f64);
		println!("Emission Divisor: {:?}", EmissionDivisor::get());
		println!();

		const MAX_BLOCKS: u64 = 130_000_000;
		const REPORT_INTERVAL: u64 = 1_000_000;
		const UNIT: u128 = 1_000_000_000_000;
		const FOUR_YEARS_BLOCKS: u64 = 10_519_200;

		let initial_supply = Balances::total_issuance();
		let mut current_supply = initial_supply;
		let mut total_miner_rewards = 0u128;
		let mut block = 0u64;
		let mut four_year_stats: Option<(u128, u128)> = None;

		println!("Block       Supply        %MaxSupply  BlockReward   Remaining");
		println!("{}", "-".repeat(70));

		let remaining = MaxSupply::get() - current_supply;
		let block_reward = if remaining > 0 { remaining / EmissionDivisor::get() } else { 0 };
		println!(
			"{:<11} {:<13} {:<11.2}% {:<13.6} {:<13}",
			block,
			current_supply / UNIT,
			(current_supply as f64 / MaxSupply::get() as f64) * 100.0,
			block_reward as f64 / UNIT as f64,
			remaining / UNIT
		);

		set_miner_preimage_digest(MINER_1.preimage());

		loop {
			let remaining_supply = MaxSupply::get().saturating_sub(current_supply);
			let block_reward = remaining_supply / EmissionDivisor::get();
			if block_reward == 0 || block >= MAX_BLOCKS {
				break;
			}

			current_supply += block_reward;
			total_miner_rewards += block_reward;
			block += 1;

			if block == FOUR_YEARS_BLOCKS {
				four_year_stats = Some((current_supply, total_miner_rewards));
			}

			if block.is_multiple_of(REPORT_INTERVAL) {
				let remaining = MaxSupply::get().saturating_sub(current_supply);
				let next_block_reward =
					if remaining > 0 { remaining / EmissionDivisor::get() } else { 0 };
				println!(
					"{:<11} {:<13} {:<11.2}% {:<13.6} {:<13}",
					block,
					current_supply / UNIT,
					(current_supply as f64 / MaxSupply::get() as f64) * 100.0,
					next_block_reward as f64 / UNIT as f64,
					remaining / UNIT
				);
			}
		}

		let remaining = MaxSupply::get().saturating_sub(current_supply);
		let next_block_reward = if remaining > 0 { remaining / EmissionDivisor::get() } else { 0 };
		println!(
			"{:<11} {:<13} {:<11.2}% {:<13.6} {:<13} (final)",
			block,
			current_supply / UNIT,
			(current_supply as f64 / MaxSupply::get() as f64) * 100.0,
			next_block_reward as f64 / UNIT as f64,
			remaining / UNIT
		);

		println!("{}", "-".repeat(70));
		println!();
		println!("=== Final Summary ===");
		println!("Total Blocks Processed: {}", block);
		println!("Final Supply: {:.6} tokens", current_supply as f64 / UNIT as f64);
		println!(
			"Percentage of Max Supply: {:.4}%",
			(current_supply as f64 / MaxSupply::get() as f64) * 100.0
		);
		println!(
			"Remaining Supply: {:.6} tokens",
			(MaxSupply::get() - current_supply) as f64 / UNIT as f64
		);
		println!();
		println!("Total Miner Rewards: {:.6} tokens", total_miner_rewards as f64 / UNIT as f64);

		let total_seconds = block as f64 * 12.0;
		let days = total_seconds / (24.0 * 3600.0);
		let years = days / 365.25;
		println!();
		println!("=== Time Estimates (12s blocks) ===");
		println!("Total Time: {:.1} days ({:.1} years)", days, years);

		let (supply_4y, miner_4y) =
			four_year_stats.expect("simulation must run past the 4-year mark");
		let mineable_supply = MaxSupply::get() - initial_supply;
		let emitted_4y = supply_4y - initial_supply;
		let emitted_pct_4y = (emitted_4y as f64 / mineable_supply as f64) * 100.0;
		let miner_pct_4y = (miner_4y as f64 / mineable_supply as f64) * 100.0;

		println!();
		println!("=== 4-Year Checkpoint (block {}) ===", FOUR_YEARS_BLOCKS);
		println!(
			"Emitted: {:.6} tokens ({:.2}% of mineable supply)",
			emitted_4y as f64 / UNIT as f64,
			emitted_pct_4y
		);
		println!(
			"To Miners: {:.6} tokens ({:.2}% of mineable supply)",
			miner_4y as f64 / UNIT as f64,
			miner_pct_4y
		);

		assert!(
			(49.0..=51.0).contains(&emitted_pct_4y),
			"~50% of mineable supply should be emitted after 4 years, got {:.2}%",
			emitted_pct_4y
		);
		assert!(
			(49.0..=51.0).contains(&miner_pct_4y),
			"~50% of mineable supply should have gone to miners after 4 years, got {:.2}%",
			miner_pct_4y
		);

		assert!(current_supply >= initial_supply, "Supply should have increased");
		assert!(current_supply <= MaxSupply::get(), "Supply should not exceed max supply");

		let emitted_tokens = current_supply - initial_supply;
		let emission_percentage =
			(emitted_tokens as f64 / (MaxSupply::get() - initial_supply) as f64) * 100.0;
		assert!(
			emission_percentage > 99.0,
			"Should have emitted >99% of available supply, got {:.2}%",
			emission_percentage
		);

		assert!(total_miner_rewards > 0, "Miners should have received rewards");
		assert_eq!(
			total_miner_rewards, emitted_tokens,
			"Total miner rewards should equal emitted tokens"
		);

		let remaining_percentage =
			((MaxSupply::get() - current_supply) as f64 / MaxSupply::get() as f64) * 100.0;
		assert!(
			remaining_percentage < 1.0,
			"Should have <1% supply remaining, got {:.2}%",
			remaining_percentage
		);
		assert!(
			remaining_percentage > 0.0,
			"Should still have some supply remaining for future emission"
		);

		println!();
		println!("✅ All emission validation checks passed!");
		println!("✅ Emission simulation completed successfully!");
	});
}

// =========================================================================
// Tests for transfer proof recording during mining rewards
// =========================================================================

#[test]
fn miner_reward_records_transfer_proof() {
	new_test_ext().execute_with(|| {
		MockProofRecorder::clear();
		set_miner_preimage_digest(MINER_1.preimage());
		assert_eq!(MockProofRecorder::proof_count(), 0);

		MiningRewards::on_finalize(1);

		let proofs = MockProofRecorder::get_recorded_proofs();
		assert_eq!(proofs.len(), 1, "one combined miner leaf, no treasury split");

		let miner_proof = proofs.iter().find(|p| p.to == MINER_1.account_id());
		assert!(miner_proof.is_some(), "Should have a proof for miner reward");
		let miner_proof = miner_proof.unwrap();
		assert_eq!(miner_proof.asset_id, None, "Miner reward should be native token");
		assert_eq!(miner_proof.from, MintingAccount::get(), "From should be MintingAccount");
		assert!(miner_proof.amount > 0, "Miner reward amount should be positive");
	});
}

#[test]
fn miner_reward_with_fees_records_one_combined_proof() {
	new_test_ext().execute_with(|| {
		MockProofRecorder::clear();
		set_miner_preimage_digest(MINER_1.preimage());

		let fees: Balance = 100;
		MiningRewards::collect_transaction_fees(fees);
		let (expected, _) = miner_payout(fees);

		MiningRewards::on_finalize(1);

		let proofs = MockProofRecorder::get_recorded_proofs();
		let miner_proofs: Vec<_> = proofs.iter().filter(|p| p.to == MINER_1.account_id()).collect();
		assert_eq!(miner_proofs.len(), 1, "fees and block reward share one leaf");
		assert_eq!(miner_proofs[0].amount, expected);
		assert_eq!(miner_proofs[0].from, MintingAccount::get());
	});
}

#[test]
fn no_miner_defers_payout_without_a_leaf() {
	new_test_ext().execute_with(|| {
		MockProofRecorder::clear();
		let gross = expected_block_reward(0);

		MiningRewards::on_finalize(1);

		assert!(
			MockProofRecorder::get_recorded_proofs().is_empty(),
			"a deferred payout must not insert a leaf"
		);
		assert_eq!(MiningRewards::collected_fees(), gross);
		System::assert_has_event(Event::PayoutDeferred { amount: gross }.into());
	});
}

#[test]
fn zero_reward_does_not_record_proof() {
	new_test_ext().execute_with(|| {
		MockProofRecorder::clear();
		set_miner_preimage_digest(MINER_1.preimage());
		MiningRewards::on_finalize(1);
		let proof_count = MockProofRecorder::proof_count();

		MockProofRecorder::clear();
		MiningRewards::on_finalize(2);
		let proof_count_2 = MockProofRecorder::proof_count();

		assert!(proof_count > 0, "First block should have proofs");
		assert!(proof_count_2 > 0, "Second block should have proofs");
	});
}

#[test]
fn wormhole_miner_address_records_correct_proof() {
	new_test_ext().execute_with(|| {
		MockProofRecorder::clear();

		let preimage = [42u8; 32];
		let wormhole_miner = sp_core::crypto::AccountId32::from(
			derive_wormhole_address(preimage).expect("test preimage limbs are canonical"),
		);

		set_miner_preimage_digest(preimage);
		MiningRewards::on_finalize(1);

		let proofs = MockProofRecorder::get_recorded_proofs();
		let miner_proof = proofs.iter().find(|p| p.to == wormhole_miner);
		assert!(miner_proof.is_some(), "Should have proof for wormhole miner address");

		let proof = miner_proof.unwrap();
		assert_eq!(proof.from, MintingAccount::get());
		assert!(proof.amount > 0);
	});
}
