//! Runtime-parameter consistency checks for the QPoW difficulty controller.
//!
//! These run against the *actual* `quantus_runtime` constants (not the mock),
//! so any future re-tuning that breaks the EIP-2 invariants fails CI here.

use frame_support::traits::Get;
use pallet_qpow::Config;
use primitive_types::U512;
use quantus_runtime::Runtime;

fn bucket() -> u64 {
	<Runtime as Config>::BlockTimeBucketMs::get()
}
fn target() -> u64 {
	<Runtime as Config>::TargetBlockTime::get()
}
fn max_up() -> i32 {
	<Runtime as Config>::MaxUpAdjFactor::get()
}
fn max_down() -> i32 {
	<Runtime as Config>::MaxDownAdjFactor::get()
}
fn divisor() -> U512 {
	<Runtime as Config>::DifficultyBoundDivisor::get()
}
fn initial() -> U512 {
	<Runtime as Config>::InitialDifficulty::get()
}
fn min_diff() -> U512 {
	pallet_qpow::Pallet::<Runtime>::get_min_difficulty()
}

#[test]
fn target_sits_inside_no_change_band() {
	// With `adj_factor = max_up - block_time / bucket` clamped to [max_down, max_up],
	// the no-change band is [max_up * bucket, (max_up + 1) * bucket). The target
	// must fall inside it, otherwise every on-target block adjusts difficulty.
	let band_lo = bucket().saturating_mul(max_up() as u64);
	let band_hi = bucket().saturating_mul(max_up() as u64 + 1);
	assert!(
		target() >= band_lo && target() < band_hi,
		"target {} not in no-change band [{}, {})",
		target(),
		band_lo,
		band_hi,
	);
}

#[test]
fn floor_is_liftable() {
	// `step = parent_difficulty / divisor` must be ≥ 1 at the floor, otherwise the
	// controller cannot escape it (the bug that motivated PR #564's review).
	assert!(
		min_diff() >= divisor(),
		"floor {} < divisor {} — controller cannot escape min",
		min_diff().low_u64(),
		divisor().low_u64(),
	);
}

#[test]
fn floor_matches_ethereum_minimum_difficulty() {
	assert_eq!(min_diff(), U512::from(131_072u64));
}

#[test]
fn down_cap_fires_only_on_real_stalls() {
	// -99 cap should require a single block ≥ 5× target — the EIP-2 §Rationale
	// "black swan" threshold, not routine slow blocks.
	let cap_ms = (max_up() - max_down()) as u64 * bucket();
	assert!(
		cap_ms > 5 * target(),
		"-99 cap fires at {}ms, only {}× target — too aggressive",
		cap_ms,
		cap_ms / target(),
	);
}

#[test]
fn initial_difficulty_above_floor() {
	assert!(
		initial() > min_diff(),
		"initial {} <= min {} — chain starts at the floor",
		initial().low_u64(),
		min_diff().low_u64(),
	);
	assert!(
		initial() >= divisor(),
		"initial must be ≥ divisor so the per-block step is at least 1 unit",
	);
}

#[test]
fn at_target_block_produces_no_change() {
	let d = U512::from(1_000_000u64);
	let r = pallet_qpow::Pallet::<Runtime>::calculate_difficulty(d, target(), target());
	assert_eq!(r, d, "at-target block must not adjust difficulty");
}

#[test]
fn upper_band_edge_still_flat() {
	let d = U512::from(1_000_000u64);
	let r = pallet_qpow::Pallet::<Runtime>::calculate_difficulty(d, 2 * bucket() - 1, target());
	assert_eq!(r, d, "block_time = 2*bucket - 1 must still produce no change");
}
