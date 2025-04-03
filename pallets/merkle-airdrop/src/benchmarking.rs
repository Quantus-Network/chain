//! Benchmarking setup for pallet-merkle-airdrop

use super::*;

#[allow(unused)]
use crate::Pallet as MerkleAirdrop;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn create_airdrop() {
        let caller: T::AccountId = whitelisted_caller();
        let merkle_root = [0u8; 32];

        #[extrinsic_call]
        create_airdrop(RawOrigin::Signed(caller), merkle_root);
    }

    #[benchmark]
    fn fund_airdrop() {
        let caller: T::AccountId = whitelisted_caller();
        let merkle_root = [0u8; 32];

        // Create an airdrop first
        let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();
        AirdropMerkleRoots::<T>::insert(airdrop_id, merkle_root);
        NextAirdropId::<T>::put(airdrop_id + 1);

        let amount = 1000u32.into();

        #[extrinsic_call]
        fund_airdrop(RawOrigin::Signed(caller), airdrop_id, amount);
    }

    #[benchmark]
    fn claim() {
        let caller: T::AccountId = whitelisted_caller();
        let merkle_root = [0u8; 32];

        // Create and fund an airdrop first
        let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();
        AirdropMerkleRoots::<T>::insert(airdrop_id, merkle_root);
        NextAirdropId::<T>::put(airdrop_id + 1);

        let amount = 1000u32.into();

        // Mock proof - in a real benchmark this would need to be valid
        let merkle_proof = vec![[0u8; 32]];

        #[extrinsic_call]
        claim(RawOrigin::Signed(caller), airdrop_id, amount, merkle_proof);
    }

    impl_benchmark_test_suite!(MerkleAirdrop, crate::mock::new_test_ext(), crate::mock::Test);
}