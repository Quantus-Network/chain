//! Benchmarking setup for pallet_wormhole

extern crate alloc;

use super::*;
use alloc::vec::Vec;
use frame_benchmarking::v2::*;
use frame_support::traits::Get;
use qp_wormhole_verifier::{ProofWithPublicInputs, C, F};

/// Real private-batch proof for benchmarking (hex-encoded).
/// Generated using: `quantus wormhole multi round`
/// This proof is used to benchmark the actual deserialization and verification cost.
const PRIVATE_BATCH_PROOF_HEX: &str = include_str!("../test-data/private_batch.hex");

/// Real public-batch proof for benchmarking (hex-encoded).
/// Must match `circuit_config::NUM_PRIVATE_BATCH_PROOFS` / `NUM_LEAF_PROOFS`.
/// Regenerate with `regenerate_public_batch_fixture` when those knobs change.
const PUBLIC_BATCH_PROOF_HEX: &str = include_str!("../test-data/public_batch.hex");

/// Maximum number of nullifiers in a private-batch proof: one per leaf proof, derived
/// from the compiled circuit so the benchmark tracks the `QP_NUM_LEAF_PROOFS` build knob.
const MAX_NULLIFIERS: u32 = crate::circuit_config::NUM_LEAF_PROOFS as u32;

/// Maximum number of nullifiers across a full public-batch proof.
const MAX_PUBLIC_NULLIFIERS: u32 = (crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS *
	crate::circuit_config::NUM_LEAF_PROOFS) as u32;

/// The D const parameter for plonky2 proofs (extension degree = 2)
const D: usize = 2;

#[benchmarks]
mod benchmarks {
	use super::*;
	use frame_system::pallet_prelude::BlockNumberFor;
	use qp_wormhole_verifier::{
		parse_private_batch_public_inputs, parse_public_batch_public_inputs,
	};

	/// Insert `block_data`'s referenced block hash into `frame_system::BlockHash` so
	/// the production pre-validation's block check passes for the fixture proof.
	fn insert_fixture_block_hash<T: Config>(block_data: &qp_wormhole_verifier::BlockData) {
		let mut hash = <T as frame_system::Config>::Hash::default();
		hash.as_mut().copy_from_slice(block_data.block_hash.as_ref());
		frame_system::BlockHash::<T>::insert(
			BlockNumberFor::<T>::from(block_data.block_number),
			hash,
		);
	}

	/// Populate `UsedNullifiers` with decoy entries so the production nullifier scan
	/// reads a realistically populated map. The fixture's own nullifiers stay absent:
	/// that is the worst case (every segment stays valid, so the scan, the
	/// intra-segment dedup and the cross-segment claimed-set all run to completion).
	fn insert_decoy_nullifiers<T: Config>(count: u32) {
		for i in 0..count {
			let mut nullifier = [0xFFu8; 32];
			nullifier[0..4].copy_from_slice(&i.to_le_bytes());
			pallet::UsedNullifiers::<T>::insert(nullifier, true);
		}
	}

	/// Build a worst-case exit bundle for segment validation: every segment is
	/// non-inert with `NUM_LEAF_PROOFS` distinct, unused nullifiers.
	///
	/// The embedded proof fixtures carry dummy padding (the public fixture has one
	/// real inner segment out of `NUM_PRIVATE_BATCH_PROOFS`; the private fixture
	/// has one real nullifier out of `NUM_LEAF_PROOFS`), and `segment_validity`
	/// skips inert segments entirely — so validating the fixture alone would miss
	/// almost all of the per-segment normalization, intra-segment dedup,
	/// cross-segment claimed-set checks, and `UsedNullifiers` reads. This bundle
	/// makes every one of those branches execute in full.
	///
	/// Nullifier patterns (`0xAB…` + counter) are disjoint from the decoy keys
	/// (`0xFF…` + counter), so every scan is an absent-key read against a
	/// populated map and all segments stay valid — the maximal path.
	fn worst_case_bundle<T: Config>(
		block_data: qp_wormhole_verifier::BlockData,
		num_segments: usize,
	) -> ExitBundle {
		let slots_per_segment = crate::circuit_config::NUM_LEAF_PROOFS * 2;
		let mut counter: u32 = 0;
		let segments = (0..num_segments)
			.map(|_| {
				let nullifiers = (0..crate::circuit_config::NUM_LEAF_PROOFS)
					.map(|_| {
						counter += 1;
						let mut bytes = [0xABu8; 32];
						bytes[0..4].copy_from_slice(&counter.to_le_bytes());
						qp_wormhole_verifier::BytesDigest::new_unchecked(bytes)
					})
					.collect();
				let account_data = (0..slots_per_segment)
					.map(|_| qp_wormhole_verifier::PublicInputsByAccount {
						summed_output_amount: 1,
						exit_account: qp_wormhole_verifier::BytesDigest::new_unchecked(
							[0xCDu8; 32],
						),
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

	/// Benchmark for the pre-validation phase of verify_private_batch.
	///
	/// Executes the *production* `pre_validate_private_batch_proof` path end to end:
	/// proof deserialization, public-input parsing, `ExitBundle` construction, and
	/// `validate_exit_bundle_common` (block-hash check, per-segment nullifier
	/// normalization + intra-segment dedup + cross-segment collision checks +
	/// used-nullifier reads, and `ensure_any_segment_valid`). Measuring anything
	/// less understates the CPU cost this weight is charged for — twice — on the
	/// inclusion path.
	///
	/// The fixture proof carries dummy nullifier padding that `segment_validity`
	/// skips, so a worst-case all-real segment validation is measured on top (see
	/// [`worst_case_bundle`]); the fixture's own (cheaper) validation stays in the
	/// block, slightly over-charging — the safe direction.
	#[benchmark]
	fn pre_validate_proof() {
		let proof_bytes: Vec<u8> =
			hex::decode(PRIVATE_BATCH_PROOF_HEX.trim()).expect("Invalid hex in test proof");

		// Parse the fixture (setup only) to learn which block hash it commits to.
		let verifier =
			crate::get_private_batch_verifier().expect("Private-batch verifier not available");
		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			proof_bytes.clone(),
			&verifier.circuit_data.common,
		)
		.expect("Failed to deserialize proof");
		let inputs = parse_private_batch_public_inputs(&proof).expect("Failed to parse inputs");

		insert_fixture_block_hash::<T>(&inputs.block_data);
		insert_decoy_nullifiers::<T>(MAX_NULLIFIERS);
		let worst_bundle = worst_case_bundle::<T>(inputs.block_data.clone(), 1);

		#[block]
		{
			let result = Pallet::<T>::pre_validate_private_batch_proof(&proof_bytes);
			assert!(result.is_ok(), "production pre-validation must succeed");
			let validity = Pallet::<T>::validate_exit_bundle_common(&worst_bundle);
			assert!(validity.is_ok(), "worst-case segment validation must succeed");
		}
	}

	/// Pre-validation for the public-batch path. Executes the *production*
	/// `pre_validate_public_batch_proof` end to end: deserialization, public-input
	/// parsing, `ExitBundle::from_public_batch` segment construction across all
	/// `NUM_PRIVATE_BATCH_PROOFS` inner segments, and `validate_exit_bundle_common`.
	///
	/// The fixture aggregates one real private batch plus dummy padding, and
	/// `segment_validity` skips the padded (inert) segments — so the fixture alone
	/// cannot exercise the all-valid worst case. A synthetic bundle with every
	/// segment non-inert (see [`worst_case_bundle`]) is validated in the same
	/// block, making all `NUM_PRIVATE_BATCH_PROOFS × NUM_LEAF_PROOFS` nullifier
	/// normalizations, dedup/claimed-set operations and `UsedNullifiers` reads
	/// execute. The fixture's own (cheaper) validation stays in the block,
	/// slightly over-charging — the safe direction.
	#[benchmark]
	fn pre_validate_public_batch_proof() {
		let proof_bytes: Vec<u8> =
			hex::decode(PUBLIC_BATCH_PROOF_HEX.trim()).expect("Invalid hex in public-batch proof");

		let verifier =
			crate::get_public_batch_verifier().expect("Public-batch verifier not available");
		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			proof_bytes.clone(),
			&verifier.circuit_data.common,
		)
		.expect("Failed to deserialize public-batch proof");
		let inputs = parse_public_batch_public_inputs(
			&proof,
			crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
			crate::circuit_config::NUM_LEAF_PROOFS,
		)
		.expect("Failed to parse public-batch inputs");

		insert_fixture_block_hash::<T>(&inputs.block_data);
		insert_decoy_nullifiers::<T>(MAX_PUBLIC_NULLIFIERS);
		let worst_bundle = worst_case_bundle::<T>(
			inputs.block_data.clone(),
			crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
		);

		#[block]
		{
			let result = Pallet::<T>::pre_validate_public_batch_proof(&proof_bytes);
			assert!(result.is_ok(), "production pre-validation must succeed");
			let validity = Pallet::<T>::validate_exit_bundle_common(&worst_bundle);
			assert!(validity.is_ok(), "worst-case segment validation must succeed");
		}
	}

	/// Benchmark for full ZK proof verification.
	///
	/// This measures the actual cost of verifying a private-batch plonky2 proof,
	/// which is the dominant cost in verify_private_batch extrinsic.
	/// Note: This only benchmarks the ZK verification itself, not the full extrinsic
	/// (which includes state writes that depend on proof contents).
	#[benchmark]
	fn verify_private_batch() {
		// Decode the hex proof to bytes
		let proof_bytes: Vec<u8> =
			hex::decode(PRIVATE_BATCH_PROOF_HEX.trim()).expect("Invalid hex in test proof");

		// Get verifier
		let verifier =
			crate::get_private_batch_verifier().expect("Private-batch verifier not available");

		// Deserialize proof (outside the measured block since pre_validate_proof covers this)
		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			proof_bytes,
			&verifier.circuit_data.common,
		)
		.expect("Failed to deserialize proof");

		#[block]
		{
			// Verify the ZK proof - this is the expensive cryptographic operation
			verifier.verify(proof.clone()).expect("Proof verification failed");
		}
	}

	/// Benchmark for full public-batch ZK verification (dominant cost of
	/// `verify_public_batch`). Fixture must match the compiled circuit size.
	#[benchmark]
	fn verify_public_batch() {
		let proof_bytes: Vec<u8> =
			hex::decode(PUBLIC_BATCH_PROOF_HEX.trim()).expect("Invalid hex in public-batch proof");

		let verifier =
			crate::get_public_batch_verifier().expect("Public-batch verifier not available");

		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			proof_bytes,
			&verifier.circuit_data.common,
		)
		.expect("Failed to deserialize public-batch proof");

		#[block]
		{
			verifier.verify(proof.clone()).expect("Public-batch proof verification failed");
		}
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
