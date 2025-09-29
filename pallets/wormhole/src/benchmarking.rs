//! Benchmarking setup for pallet-wormhole

use super::*;
use alloc::vec::Vec;
use frame_benchmarking::v2::*;
use frame_support::{ensure, traits::fungible::Inspect};
use frame_system::RawOrigin;
use qp_wormhole_circuit::inputs::PublicCircuitInputs;
use qp_wormhole_verifier::ProofWithPublicInputs;
use qp_zk_circuits_common::circuit::{C, D, F};

fn get_benchmark_proof() -> Vec<u8> {
	let hex_proof = include_str!("../proof_from_bins.hex");
	hex::decode(hex_proof.trim()).expect("Failed to decode hex proof")
}

#[benchmarks(
    where
    T: Send + Sync,
    T: Config,
    BalanceOf<T>: Into<<<T as Config>::Currency as Inspect<T::AccountId>>::Balance>,
)]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn verify_wormhole_proof() -> Result<(), BenchmarkError> {
		let proof_bytes = get_benchmark_proof();

		let verifier = crate::get_wormhole_verifier()
			.map_err(|_| BenchmarkError::Stop("Verifier not available"))?;

		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			proof_bytes.clone(),
			&verifier.circuit_data.common,
		)
		.map_err(|_| BenchmarkError::Stop("Invalid proof data"))?;

		let public_inputs = PublicCircuitInputs::try_from(&proof)
			.map_err(|_| BenchmarkError::Stop("Invalid public inputs"))?;

		let nullifier_bytes = *public_inputs.nullifier;

		ensure!(
			!UsedNullifiers::<T>::contains_key(nullifier_bytes),
			BenchmarkError::Stop("Nullifier already used")
		);

		verifier
			.verify(proof)
			.map_err(|_| BenchmarkError::Stop("Proof verification failed"))?;

		let block_number = frame_system::Pallet::<T>::block_number();

		#[extrinsic_call]
		verify_wormhole_proof(RawOrigin::None, proof_bytes, block_number);

		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
