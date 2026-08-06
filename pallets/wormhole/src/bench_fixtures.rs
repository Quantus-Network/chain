//! Shared pre-validation fixtures for benchmarks and the timing harness.
//!
//! Kept in one place so the runtime-benchmarks path and the ignored calibration
//! test cannot drift to different worst-case inputs.

use crate::{circuit_config, Config, ExitBundle, ExitSegment, UsedNullifiers};
use frame_support::traits::Get;
use qp_wormhole_verifier::{BlockData, BytesDigest, PublicInputsByAccount};

/// Populate `UsedNullifiers` with decoys; fixture nullifiers stay absent so every
/// segment remains valid and the full scan/dedup/claimed-set path runs.
pub fn insert_decoy_nullifiers<T: Config>(count: u32) {
	for i in 0..count {
		let mut nullifier = [0xFFu8; 32];
		nullifier[0..4].copy_from_slice(&i.to_le_bytes());
		UsedNullifiers::<T>::insert(nullifier, true);
	}
}

/// All-valid exit bundle: every segment non-inert with distinct unused
/// nullifiers (disjoint from decoy keys). Needed because proof fixtures use
/// dummy padding that `segment_validity` skips as inert.
pub fn worst_case_bundle<T: Config>(block_data: BlockData, num_segments: usize) -> ExitBundle {
	let slots_per_segment = circuit_config::NUM_LEAF_PROOFS * 2;
	let mut counter: u32 = 0;
	let segments = (0..num_segments)
		.map(|_| {
			let nullifiers = (0..circuit_config::NUM_LEAF_PROOFS)
				.map(|_| {
					counter += 1;
					let mut bytes = [0xABu8; 32];
					bytes[0..4].copy_from_slice(&counter.to_le_bytes());
					BytesDigest::new_unchecked(bytes)
				})
				.collect();
			let account_data = (0..slots_per_segment)
				.map(|_| PublicInputsByAccount {
					summed_output_amount: 1,
					exit_account: BytesDigest::new_unchecked([0xCDu8; 32]),
				})
				.collect();
			ExitSegment { account_data, nullifiers }
		})
		.collect();
	ExitBundle {
		asset_id: 0,
		volume_fee_bps: T::VolumeFeeRateBps::get(),
		block_data,
		aggregator_address: None,
		segments,
	}
}
