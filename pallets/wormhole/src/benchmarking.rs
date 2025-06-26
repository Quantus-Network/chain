// //! Benchmarking setup for pallet-wormhole

// use super::*;
// use crate::Pallet as Wormhole;
// use frame_benchmarking::v2::*;
// use frame_system::RawOrigin;

// #[benchmarks(
//     where
//     T: Send + Sync,
//     T: Config + pallet_balances::Config,
// )]
// mod benchmarks {
//     use super::*;

//     #[benchmark]
//     fn verify_wormhole_proof() {
//         // Load a valid proof from the test file
//         let proof_bytes = include_bytes!("../proof.hex").to_vec();

//         // Ensure the origin account has sufficient balance for potential operations
//         let caller: T::AccountId = whitelisted_caller();

//         // Set up any necessary initial state
//         // Make sure no nullifier conflicts exist by using a unique test nullifier
//         let test_nullifier = [1u8; 64];
//         // Ensure this nullifier is not already used
//         UsedNullifiers::<T>::remove(test_nullifier);

//         #[extrinsic_call]
//         verify_wormhole_proof(RawOrigin::None, proof_bytes);

//         // Verify that the proof was processed (though specific verification depends on the actual proof content)
//         // The function should complete without error if the proof is valid
//     }

//     impl_benchmark_test_suite!(Wormhole, crate::mock::new_test_ext(), crate::mock::Test);
// }
