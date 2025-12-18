#!/usr/bin/env python3
"""
Simulate the mining rewards emission schedule for the blockchain.

This script models the scheduled emission system where:
- Max supply: 21,000,000 tokens
- Emission divisor: 26,280,000
- Treasury portion: 50%
- Block reward = (max_supply - current_supply) / emission_divisor
"""


def simulate_emissions():
    # Configuration (matching the runtime config)
    MAX_SUPPLY = 21_000_000  # 21 million tokens
    EMISSION_DIVISOR = 26_280_000
    TREASURY_PORTION = 0.5  # 50% to treasury
    INITIAL_SUPPLY = 6_300_000  # ~6.3M tokens from genesis

    # Simulation parameters
    BLOCKS_PER_REPORT = 1_000_000  # Report every 1M blocks
    MAX_BLOCKS = 120_000_000  # Stop after 120M blocks or when supply is exhausted

    current_supply = INITIAL_SUPPLY
    total_miner_rewards = 0
    total_treasury_rewards = 0
    block = 0

    print("=== Blockchain Emission Simulation ===")
    print(f"Max Supply: {MAX_SUPPLY:,} tokens")
    print(f"Emission Divisor: {EMISSION_DIVISOR:,}")
    print(f"Treasury Portion: {TREASURY_PORTION:.1%}")
    print(f"Initial Supply: {INITIAL_SUPPLY:,} tokens")
    print(f"Available for Emission: {MAX_SUPPLY - INITIAL_SUPPLY:,} tokens")
    print()

    print(
        f"{'Block':<12} {'Supply':<15} {'%MaxSupply':<12} {'BlockReward':<12} {'ToTreasury':<12} {'ToMiner':<12} {'Remaining':<15}"
    )
    print("-" * 100)

    # Initial state
    remaining = MAX_SUPPLY - current_supply
    block_reward = remaining / EMISSION_DIVISOR if remaining > 0 else 0
    treasury_reward = block_reward * TREASURY_PORTION
    miner_reward = block_reward * (1 - TREASURY_PORTION)

    print(
        f"{block:<12,} {current_supply:<15,.0f} {current_supply / MAX_SUPPLY:<11.2%} {block_reward:<12.6f} {treasury_reward:<12.6f} {miner_reward:<12.6f} {remaining:<15,.0f}"
    )

    while block < MAX_BLOCKS and current_supply < MAX_SUPPLY:
        # Simulate blocks
        for _ in range(BLOCKS_PER_REPORT):
            if current_supply >= MAX_SUPPLY:
                break

            # Calculate reward for this block
            remaining_supply = MAX_SUPPLY - current_supply
            if remaining_supply <= 0:
                break

            block_reward = remaining_supply / EMISSION_DIVISOR
            treasury_reward = block_reward * TREASURY_PORTION
            miner_reward = block_reward * (1 - TREASURY_PORTION)

            # Update totals
            current_supply += block_reward
            total_treasury_rewards += treasury_reward
            total_miner_rewards += miner_reward
            block += 1

        # Print progress
        remaining = MAX_SUPPLY - current_supply
        next_block_reward = remaining / EMISSION_DIVISOR if remaining > 0 else 0
        next_treasury = next_block_reward * TREASURY_PORTION
        next_miner = next_block_reward * (1 - TREASURY_PORTION)

        print(
            f"{block:<12,} {current_supply:<15,.0f} {current_supply / MAX_SUPPLY:<11.2%} {next_block_reward:<12.6f} {next_treasury:<12.6f} {next_miner:<12.6f} {remaining:<15,.0f}"
        )

        # Stop if we've reached max supply or rewards are negligible
        if current_supply >= MAX_SUPPLY or next_block_reward < 0.000001:
            break

    print("-" * 100)
    print()
    print("=== Final Summary ===")
    print(f"Total Blocks Processed: {block:,}")
    print(f"Final Supply: {current_supply:,.6f} tokens")
    print(f"Percentage of Max Supply: {current_supply / MAX_SUPPLY:.4%}")
    print(f"Remaining Supply: {MAX_SUPPLY - current_supply:,.6f} tokens")
    print()
    print(f"Total Miner Rewards: {total_miner_rewards:,.6f} tokens")
    print(f"Total Treasury Rewards: {total_treasury_rewards:,.6f} tokens")
    print(
        f"Total Rewards Distributed: {total_miner_rewards + total_treasury_rewards:,.6f} tokens"
    )
    print()
    print(
        f"Miner Share: {total_miner_rewards / (total_miner_rewards + total_treasury_rewards):.1%}"
    )
    print(
        f"Treasury Share: {total_treasury_rewards / (total_miner_rewards + total_treasury_rewards):.1%}"
    )

    # Calculate time estimates (assuming 12 second blocks)
    seconds_per_block = 12
    total_seconds = block * seconds_per_block
    days = total_seconds / (24 * 3600)
    years = days / 365.25

    print()
    print("=== Time Estimates (12s blocks) ===")
    print(f"Total Time: {days:,.1f} days ({years:.1f} years)")
    print(
        f"Time to 99% of max supply: ~{days * 0.99:,.1f} days ({years * 0.99:.1f} years)"
    )


def analyze_emission_curve():
    """Analyze the emission curve characteristics."""
    MAX_SUPPLY = 21_000_000
    EMISSION_DIVISOR = 26_280_000
    INITIAL_SUPPLY = 1_400_000

    print("\n=== Emission Curve Analysis ===")

    # Calculate halving points
    supply_checkpoints = [0.5, 0.75, 0.90, 0.95, 0.99, 0.999]

    for checkpoint in supply_checkpoints:
        target_supply = INITIAL_SUPPLY + (MAX_SUPPLY - INITIAL_SUPPLY) * checkpoint
        remaining = MAX_SUPPLY - target_supply
        reward_at_point = remaining / EMISSION_DIVISOR

        print(
            f"At {target_supply:,.0f} tokens ({checkpoint:.1%} emission): reward = {reward_at_point:.8f} tokens/block"
        )


if __name__ == "__main__":
    simulate_emissions()
    analyze_emission_curve()
