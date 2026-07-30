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
const MAX_PUBLIC_NULLIFIERS: u32 =
	(crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS * crate::circuit_config::NUM_LEAF_PROOFS) as u32;

/// The D const parameter for plonky2 proofs (extension degree = 2)
const D: usize = 2;

#[benchmarks]
mod benchmarks {
	use super::*;

	/// Benchmark for the pre-validation phase of verify_private_batch.
	///
	/// This measures the actual cost of:
	/// - Proof deserialization (using a real private-batch proof)
	/// - Public inputs parsing
	/// - Block hash lookup (1 read)
	/// - Nullifier existence checks (up to MAX_NULLIFIERS reads)
	#[benchmark]
	fn pre_validate_proof() {
		// Decode the hex proof to bytes
		let proof_bytes: Vec<u8> =
			hex::decode(PRIVATE_BATCH_PROOF_HEX.trim()).expect("Invalid hex in test proof");

		// Get verifier for deserialization
		let verifier =
			crate::get_private_batch_verifier().expect("Private-batch verifier not available");

		// Setup: Create nullifiers in storage to simulate worst-case reads
		let nullifiers: Vec<[u8; 32]> = (0..MAX_NULLIFIERS)
			.map(|i| {
				let mut nullifier = [0u8; 32];
				nullifier[0..4].copy_from_slice(&i.to_le_bytes());
				nullifier
			})
			.collect();

		// Insert nullifiers into storage (these are "other" nullifiers, not the ones we're
		// checking) This populates the storage map to make reads realistic
		for nullifier in &nullifiers {
			pallet::UsedNullifiers::<T>::insert(nullifier, true);
		}

		// Setup a block hash so the lookup succeeds
		let block_number = frame_system::Pallet::<T>::block_number();

		#[block]
		{
			// 1. Deserialize proof (the expensive part)
			let _proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.clone(),
				&verifier.circuit_data.common,
			)
			.expect("Failed to deserialize proof");

			// 2. Block hash lookup
			let _block_hash = frame_system::Pallet::<T>::block_hash(block_number);

			// 3. Nullifier existence checks (worst case: all checks performed)
			for nullifier in &nullifiers {
				let _exists = pallet::UsedNullifiers::<T>::contains_key(nullifier);
			}
		}
	}

	/// Pre-validation for the public-batch path: deserialize + scan every nullifier
	/// slot across all inner private-batch segments.
	#[benchmark]
	fn pre_validate_public_batch_proof() {
		let proof_bytes: Vec<u8> =
			hex::decode(PUBLIC_BATCH_PROOF_HEX.trim()).expect("Invalid hex in public-batch proof");

		let verifier =
			crate::get_public_batch_verifier().expect("Public-batch verifier not available");

		let nullifiers: Vec<[u8; 32]> = (0..MAX_PUBLIC_NULLIFIERS)
			.map(|i| {
				let mut nullifier = [0u8; 32];
				nullifier[0..4].copy_from_slice(&i.to_le_bytes());
				nullifier
			})
			.collect();

		for nullifier in &nullifiers {
			pallet::UsedNullifiers::<T>::insert(nullifier, true);
		}

		let block_number = frame_system::Pallet::<T>::block_number();

		#[block]
		{
			let _proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.clone(),
				&verifier.circuit_data.common,
			)
			.expect("Failed to deserialize public-batch proof");

			let _block_hash = frame_system::Pallet::<T>::block_hash(block_number);

			for nullifier in &nullifiers {
				let _exists = pallet::UsedNullifiers::<T>::contains_key(nullifier);
			}
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
