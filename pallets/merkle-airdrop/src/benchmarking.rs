//! Benchmarking setup for pallet-merkle-airdrop

use super::*;

#[allow(unused)]
use crate::Pallet as MerkleAirdrop;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
extern crate alloc;
use alloc::vec;
use frame_support::traits::fungible::{Inspect, Mutate};
use frame_support::BoundedVec;
use sp_runtime::traits::Saturating;

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

        let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();
        AirdropMerkleRoots::<T>::insert(airdrop_id, merkle_root);
        AirdropCreators::<T>::insert(airdrop_id, caller.clone());
        NextAirdropId::<T>::put(airdrop_id + 1);

        let amount: BalanceOf<T> = 1u32.into();

        // Get ED and ensure caller has sufficient balance
        let ed = <T::Currency as Inspect<T::AccountId>>::minimum_balance();

        let caller_balance = ed.saturating_mul(10u32.into()).saturating_add(amount);
        <T::Currency as Mutate<T::AccountId>>::set_balance(&caller, caller_balance);

        <T::Currency as Mutate<T::AccountId>>::set_balance(&MerkleAirdrop::<T>::account_id(), ed);

        #[extrinsic_call]
        fund_airdrop(RawOrigin::Signed(caller), airdrop_id, amount);
    }

    #[benchmark]
    fn claim() {
        let caller: T::AccountId = whitelisted_caller();
        let recipient: T::AccountId = whitelisted_caller();

        let amount: BalanceOf<T> = 1u32.into();

        let leaf_hash = MerkleAirdrop::<T>::calculate_leaf_hash_blake2(&recipient, amount);
        let merkle_root = leaf_hash;

        let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();
        AirdropMerkleRoots::<T>::insert(airdrop_id, merkle_root);
        NextAirdropId::<T>::put(airdrop_id + 1);

        let ed = <T::Currency as Inspect<T::AccountId>>::minimum_balance();
        let large_balance = ed.saturating_mul(1_000_000u32.into());

        <T::Currency as Mutate<T::AccountId>>::set_balance(&caller, large_balance);
        <T::Currency as Mutate<T::AccountId>>::set_balance(&recipient, large_balance);
        <T::Currency as Mutate<T::AccountId>>::set_balance(
            &MerkleAirdrop::<T>::account_id(),
            large_balance,
        );

        AirdropBalances::<T>::insert(airdrop_id, large_balance);

        let empty_proof = vec![];
        let merkle_proof = BoundedVec::<MerkleHash, T::MaxProofs>::try_from(empty_proof)
            .expect("Empty proof should fit in bound");

        #[extrinsic_call]
        claim(RawOrigin::None, airdrop_id, recipient, amount, merkle_proof);
    }

    #[benchmark]
    fn delete_airdrop() {
        let caller: T::AccountId = whitelisted_caller();
        let merkle_root = [0u8; 32];

        // Create an airdrop first
        let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();
        AirdropMerkleRoots::<T>::insert(airdrop_id, merkle_root);
        AirdropCreators::<T>::insert(airdrop_id, caller.clone());
        NextAirdropId::<T>::put(airdrop_id + 1);

        let ed = <T::Currency as Inspect<T::AccountId>>::minimum_balance();
        let tiny_amount: BalanceOf<T> = 1u32.into();
        let large_balance = ed.saturating_mul(1_000_000u32.into());

        <T::Currency as Mutate<T::AccountId>>::set_balance(&caller, large_balance);
        <T::Currency as Mutate<T::AccountId>>::set_balance(
            &MerkleAirdrop::<T>::account_id(),
            large_balance,
        );

        AirdropBalances::<T>::insert(airdrop_id, tiny_amount);

        #[extrinsic_call]
        delete_airdrop(RawOrigin::Signed(caller), airdrop_id);
    }

    impl_benchmark_test_suite!(
        MerkleAirdrop,
        crate::mock::new_test_ext(),
        crate::mock::Test
    );
}
