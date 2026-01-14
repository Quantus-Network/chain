//! Benchmarking setup for pallet-wormhole
//!
//! Uses a pre-generated proof from proof_from_bins.hex and sets up the storage
//! to match the proof's public inputs.

use super::*;
use alloc::vec::Vec;
use codec::Decode;
use frame_benchmarking::v2::*;
use frame_support::ensure;
use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
use qp_wormhole_circuit::inputs::PublicCircuitInputs;
use qp_wormhole_verifier::ProofWithPublicInputs;
use qp_zk_circuits_common::circuit::{C, D, F};

fn get_benchmark_proof() -> Vec<u8> {
	let hex_proof = include_str!("../proof_from_bins.hex");
	hex::decode(hex_proof.trim()).expect("Failed to decode hex proof")
}

#[benchmarks]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn verify_wormhole_proof() -> Result<(), BenchmarkError> {
		let proof_bytes = get_benchmark_proof();

		// Parse the proof to get public inputs
		let verifier = crate::get_wormhole_verifier()
			.map_err(|_| BenchmarkError::Stop("Verifier not available"))?;

		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			proof_bytes.clone(),
			&verifier.circuit_data.common,
		)
		.map_err(|_| BenchmarkError::Stop("Invalid proof data"))?;

		let public_inputs = PublicCircuitInputs::try_from(&proof)
			.map_err(|_| BenchmarkError::Stop("Invalid public inputs"))?;

		// Extract values from public inputs
		let nullifier_bytes = *public_inputs.nullifier;
		let block_number_u32 = public_inputs.block_number;
		let block_hash_bytes: [u8; 32] = (*public_inputs.block_hash)
			.try_into()
			.map_err(|_| BenchmarkError::Stop("Invalid block hash length"))?;

		// Ensure nullifier hasn't been used
		ensure!(
			!UsedNullifiers::<T>::contains_key(nullifier_bytes),
			BenchmarkError::Stop("Nullifier already used")
		);

		// Verify the proof is valid (sanity check)
		verifier
			.verify(proof)
			.map_err(|_| BenchmarkError::Stop("Proof verification failed"))?;

		// Set up storage to match the proof's public inputs:
		// Set current block number to be >= proof's block_number
		let block_number: BlockNumberFor<T> = block_number_u32.into();
		frame_system::Pallet::<T>::set_block_number(block_number + 1u32.into());

		// Override block hash to match proof's block_hash
		let block_hash = T::Hash::decode(&mut &block_hash_bytes[..])
			.map_err(|_| BenchmarkError::Stop("Failed to decode block hash"))?;
		frame_system::BlockHash::<T>::insert(block_number, block_hash);

		#[extrinsic_call]
		verify_wormhole_proof(RawOrigin::None, proof_bytes);

		// Verify nullifier was marked as used
		ensure!(
			UsedNullifiers::<T>::contains_key(nullifier_bytes),
			BenchmarkError::Stop("Nullifier should be marked as used after verification")
		);

		Ok(())
	}
}
