//! Benchmarking setup for pallet_wormhole

extern crate alloc;

use super::*;
use alloc::vec::Vec;
use frame_benchmarking::v2::*;
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

	/// Benchmark for the pre-validation phase of verify_private_batch.
	///
	/// Executes the *production* `pre_validate_private_batch_proof` path end to end:
	/// proof deserialization, public-input parsing, `ExitBundle` construction, and
	/// `validate_exit_bundle_common` (block-hash check, per-segment nullifier
	/// normalization + intra-segment dedup + cross-segment collision checks +
	/// used-nullifier reads, and `ensure_any_segment_valid`). Measuring anything
	/// less understates the CPU cost this weight is charged for — twice — on the
	/// inclusion path.
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

		#[block]
		{
			let result = Pallet::<T>::pre_validate_private_batch_proof(&proof_bytes);
			assert!(result.is_ok(), "production pre-validation must succeed");
		}
	}

	/// Pre-validation for the public-batch path. Executes the *production*
	/// `pre_validate_public_batch_proof` end to end: deserialization, public-input
	/// parsing, `ExitBundle::from_public_batch` segment construction across all
	/// `NUM_PRIVATE_BATCH_PROOFS` inner segments, and `validate_exit_bundle_common`
	/// with the full nullifier scan / dedup / collision checks.
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

		#[block]
		{
			let result = Pallet::<T>::pre_validate_public_batch_proof(&proof_bytes);
			assert!(result.is_ok(), "production pre-validation must succeed");
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
