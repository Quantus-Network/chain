//! Benchmarking setup for pallet-wormhole
//!
//! Uses a pre-generated aggregated proof from aggregated_proof.hex and sets up
//! the storage to match the proof's public inputs.

use super::*;
use alloc::vec::Vec;
use codec::Decode;
use frame_benchmarking::v2::*;
use frame_support::ensure;
use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
use qp_wormhole_verifier::{parse_aggregated_public_inputs, ProofWithPublicInputs, C, D, F};

fn get_benchmark_aggregated_proof() -> Vec<u8> {
	let hex_proof = include_str!("../aggregated_proof.hex");
	hex::decode(hex_proof.trim()).expect("Failed to decode hex aggregated proof")
}

#[benchmarks]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn verify_aggregated_proof() -> Result<(), BenchmarkError> {
		let proof_bytes = get_benchmark_aggregated_proof();

		// Parse the proof to get public inputs
		let verifier = crate::get_aggregated_verifier()
			.map_err(|_| BenchmarkError::Stop("Aggregated verifier not available"))?;

		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			proof_bytes.clone(),
			&verifier.circuit_data.common,
		)
		.map_err(|_| BenchmarkError::Stop("Invalid aggregated proof data"))?;

		let aggregated_inputs = parse_aggregated_public_inputs(&proof)
			.map_err(|_| BenchmarkError::Stop("Invalid aggregated public inputs"))?;

		// Extract values from aggregated public inputs
		let block_number_u32 = aggregated_inputs.block_data.block_number;
		let block_hash_bytes = *aggregated_inputs.block_data.block_hash;

		// Ensure nullifiers haven't been used
		for nullifier in &aggregated_inputs.nullifiers {
			let nullifier_bytes: [u8; 32] = (*nullifier)
				.as_ref()
				.try_into()
				.map_err(|_| BenchmarkError::Stop("Invalid nullifier"))?;
			ensure!(
				!UsedNullifiers::<T>::contains_key(nullifier_bytes),
				BenchmarkError::Stop("Nullifier already used")
			);
		}

		// Verify the proof is valid (sanity check)
		verifier
			.verify(proof)
			.map_err(|_| BenchmarkError::Stop("Aggregated proof verification failed"))?;

		// Set up storage to match the proof's public inputs:
		// Set current block number to be >= proof's block_number
		let block_number: BlockNumberFor<T> = block_number_u32.into();
		frame_system::Pallet::<T>::set_block_number(block_number + 1u32.into());

		// Override block hash to match proof's block_hash
		let block_hash = T::Hash::decode(&mut &block_hash_bytes[..])
			.map_err(|_| BenchmarkError::Stop("Failed to decode block hash"))?;
		frame_system::BlockHash::<T>::insert(block_number, block_hash);

		#[extrinsic_call]
		verify_aggregated_proof(RawOrigin::None, proof_bytes);

		// Verify nullifiers were marked as used
		for nullifier in &aggregated_inputs.nullifiers {
			let nullifier_bytes: [u8; 32] = (*nullifier)
				.as_ref()
				.try_into()
				.map_err(|_| BenchmarkError::Stop("Invalid nullifier"))?;
			ensure!(
				UsedNullifiers::<T>::contains_key(nullifier_bytes),
				BenchmarkError::Stop("Nullifier should be marked as used after verification")
			);
		}

		Ok(())
	}
}
