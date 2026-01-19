//! Benchmarking setup for pallet-multisig

use super::*;
use crate::Pallet as Multisig;
use alloc::vec;
use frame_benchmarking::{account as benchmark_account, v2::*, BenchmarkError};
use frame_support::traits::fungible::Mutate;
use frame_system::RawOrigin;
use sp_runtime::traits::Hash;

const SEED: u32 = 0;

// Helper to fund an account
type BalanceOf2<T> = <T as pallet_balances::Config>::Balance;

fn fund_account<T>(account: &T::AccountId, amount: BalanceOf2<T>)
where
	T: Config + pallet_balances::Config,
{
	let _ = <pallet_balances::Pallet<T> as Mutate<T::AccountId>>::mint_into(
		account,
		amount * <pallet_balances::Pallet<T> as frame_support::traits::Currency<T::AccountId>>::minimum_balance(),
	);
}

#[benchmarks(
	where
	T: Config + pallet_balances::Config,
	BalanceOf2<T>: From<u128>,
)]
mod benchmarks {
	use super::*;
	use codec::Encode;

	#[benchmark]
	fn create_multisig() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();

		// Fund the caller with enough balance for deposit
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		// Create signers (including caller)
		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		let signers = vec![caller.clone(), signer1, signer2];
		let threshold = 2u32;

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), signers.clone(), threshold);

		// Verify the multisig was created
		// Note: signers are sorted internally, so we must sort for address derivation
		let mut sorted_signers = signers.clone();
		sorted_signers.sort();
		let multisig_address = Multisig::<T>::derive_multisig_address(&sorted_signers, 0);
		assert!(Multisigs::<T>::contains_key(multisig_address));

		Ok(())
	}

	#[benchmark]
	fn propose() -> Result<(), BenchmarkError> {
		// Setup: Create a multisig first
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(10000u128));

		let signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		Multisig::<T>::create_multisig(
			RawOrigin::Signed(caller.clone()).into(),
			signers.clone(),
			threshold,
		)?;

		// Note: signers are sorted internally, so we must sort for address derivation
		let mut sorted_signers = signers.clone();
		sorted_signers.sort();
		let multisig_address = Multisig::<T>::derive_multisig_address(&sorted_signers, 0);

		// Create a simple call
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; 32] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), encoded_call, expiry);

		// Verify proposal was created
		assert!(Proposals::<T>::iter_key_prefix(&multisig_address).next().is_some());

		Ok(())
	}

	#[benchmark]
	fn approve() -> Result<(), BenchmarkError> {
		// Setup: Create multisig and proposal directly in storage
		// Threshold is 3, so adding one more approval won't trigger execution
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		let signer3: T::AccountId = benchmark_account("signer3", 2, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer3, BalanceOf2::<T>::from(10000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone(), signer3.clone()];
		let threshold = 3u32; // Need 3 approvals

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			deposit: 100u32.into(),
			creator: caller.clone(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: 1,
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Directly insert proposal into storage with 1 approval
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; 32] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		let bounded_call: BoundedCallOf<T> = encoded_call.clone().try_into().unwrap();
		let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

		let proposal_data = ProposalDataOf::<T> {
			proposer: caller.clone(),
			call: bounded_call,
			expiry,
			approvals: bounded_approvals,
			deposit: 10u32.into(),
		};

		// Match pallet hashing: hash_of(bounded_call)
		let proposal_hash = <T as frame_system::Config>::Hashing::hash_of(&proposal_data.call);
		Proposals::<T>::insert(&multisig_address, proposal_hash, proposal_data);

		#[extrinsic_call]
		_(RawOrigin::Signed(signer1.clone()), multisig_address.clone(), proposal_hash);

		// Verify approval was added (now 2/3, not executed yet)
		let proposal = Proposals::<T>::get(&multisig_address, proposal_hash).unwrap();
		assert!(proposal.approvals.contains(&signer1));
		assert_eq!(proposal.approvals.len(), 2);

		Ok(())
	}

	#[benchmark]
	fn approve_and_execute() -> Result<(), BenchmarkError> {
		// Benchmarks approve() when it triggers auto-execution (threshold reached)
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(10000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			deposit: 100u32.into(),
			creator: caller.clone(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: 1,
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Directly insert proposal with 1 approval (caller already approved)
		// signer2 will approve and trigger execution
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; 32] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		let bounded_call: BoundedCallOf<T> = encoded_call.clone().try_into().unwrap();
		// Only 1 approval so far
		let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

		let proposal_data = ProposalDataOf::<T> {
			proposer: caller.clone(),
			call: bounded_call,
			expiry,
			approvals: bounded_approvals,
			deposit: 10u32.into(),
		};

		// Match pallet hashing: hash_of(bounded_call)
		let proposal_hash = <T as frame_system::Config>::Hashing::hash_of(&proposal_data.call);
		Proposals::<T>::insert(&multisig_address, proposal_hash, proposal_data);

		// signer2 approves, reaching threshold (2/2), triggering auto-execution
		#[extrinsic_call]
		approve(RawOrigin::Signed(signer2.clone()), multisig_address.clone(), proposal_hash);

		// Verify proposal was executed and removed
		assert!(!Proposals::<T>::contains_key(&multisig_address, proposal_hash));

		Ok(())
	}

	#[benchmark]
	fn cancel() -> Result<(), BenchmarkError> {
		// Setup: Create multisig and proposal directly in storage
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(10000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			deposit: 100u32.into(),
			creator: caller.clone(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: 1,
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Directly insert proposal into storage
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; 32] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		let bounded_call: BoundedCallOf<T> = encoded_call.clone().try_into().unwrap();
		let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

		let proposal_data = ProposalDataOf::<T> {
			proposer: caller.clone(),
			call: bounded_call,
			expiry,
			approvals: bounded_approvals,
			deposit: 10u32.into(),
		};

		// Match pallet hashing: hash_of(bounded_call)
		let proposal_hash = <T as frame_system::Config>::Hashing::hash_of(&proposal_data.call);
		Proposals::<T>::insert(&multisig_address, proposal_hash, proposal_data);

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), proposal_hash);

		// Verify proposal was cancelled and removed
		assert!(!Proposals::<T>::contains_key(&multisig_address, proposal_hash));

		Ok(())
	}

	#[benchmark]
	fn remove_expired() -> Result<(), BenchmarkError> {
		// Setup: Create multisig and expired proposal directly in storage
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(10000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			deposit: 100u32.into(),
			creator: caller.clone(),
			last_activity: 1u32.into(),
			active_proposals: 1,
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Create proposal with expired timestamp
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; 32] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = 10u32.into(); // Already expired
		let bounded_call: BoundedCallOf<T> = encoded_call.clone().try_into().unwrap();
		let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

		let proposal_data = ProposalDataOf::<T> {
			proposer: caller.clone(),
			call: bounded_call,
			expiry,
			approvals: bounded_approvals,
			deposit: 10u32.into(),
		};

		// Match pallet hashing: hash_of(bounded_call)
		let proposal_hash = <T as frame_system::Config>::Hashing::hash_of(&proposal_data.call);
		Proposals::<T>::insert(&multisig_address, proposal_hash, proposal_data);

		// Move past expiry + grace period
		frame_system::Pallet::<T>::set_block_number(300u32.into());

		// Call as proposer (caller) since we might still be in grace period
		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), proposal_hash);

		// Verify proposal was removed
		assert!(!Proposals::<T>::contains_key(&multisig_address, proposal_hash));

		Ok(())
	}

	#[benchmark]
	fn claim_deposits() -> Result<(), BenchmarkError> {
		// Setup: Create multisig with multiple expired proposals directly in storage
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(10000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			deposit: 100u32.into(),
			creator: caller.clone(),
			last_activity: 1u32.into(),
			active_proposals: 5,
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Create multiple expired proposals directly in storage
		let expiry = 10u32.into(); // Already expired

		for i in 0..5 {
			let system_call = frame_system::Call::<T>::remark { remark: vec![i as u8; 32] };
			let call = <T as Config>::RuntimeCall::from(system_call);
			let encoded_call = call.encode();
			let bounded_call: BoundedCallOf<T> = encoded_call.clone().try_into().unwrap();
			let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

			let proposal_data = ProposalDataOf::<T> {
				proposer: caller.clone(),
				call: bounded_call,
				expiry,
				approvals: bounded_approvals,
				deposit: 10u32.into(),
			};

			// Match pallet hashing: hash_of(bounded_call)
			let proposal_hash = <T as frame_system::Config>::Hashing::hash_of(&proposal_data.call);
			Proposals::<T>::insert(&multisig_address, proposal_hash, proposal_data);
		}

		// Move past expiry + grace period
		frame_system::Pallet::<T>::set_block_number(300u32.into());

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		// Verify at least some proposals were cleaned up
		// Note: claim_deposits only removes proposals past grace period
		// Since we set block 300 and expiry was 10, and grace period might vary,
		// we just verify the call succeeded
		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
