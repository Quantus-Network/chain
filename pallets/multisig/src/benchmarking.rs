//! Benchmarking setup for pallet-multisig

use super::*;
use crate::Pallet as Multisig;
use alloc::vec;
use frame_benchmarking::{account as benchmark_account, v2::*, BenchmarkError};
use frame_support::traits::{fungible::Mutate, ReservableCurrency};
use frame_system::RawOrigin;

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
	fn propose(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
		e: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, // expired proposals to cleanup
	) -> Result<(), BenchmarkError> {
		// Setup: Create a multisig first
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(100000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(100000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;
		signers.sort();

		// Create multisig directly in storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			proposal_nonce: e, // We'll insert e expired proposals
			creator: caller.clone(),
			deposit: T::MultisigDeposit::get(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: e,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Insert e expired proposals (worst case for auto-cleanup)
		let expired_block = 10u32.into();
		for i in 0..e {
			let system_call = frame_system::Call::<T>::remark { remark: vec![i as u8; 10] };
			let call = <T as Config>::RuntimeCall::from(system_call);
			let encoded_call = call.encode();
			let bounded_call: BoundedCallOf<T> = encoded_call.try_into().unwrap();
			let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

			let proposal_data = ProposalDataOf::<T> {
				proposer: caller.clone(),
				call: bounded_call,
				expiry: expired_block,
				approvals: bounded_approvals,
				deposit: 10u32.into(),
				status: ProposalStatus::Active,
			};
			Proposals::<T>::insert(&multisig_address, i, proposal_data);
		}

		// Move past expiry so proposals are expired
		frame_system::Pallet::<T>::set_block_number(100u32.into());

		// Create a new proposal (will auto-cleanup all e expired proposals)
		let system_call = frame_system::Call::<T>::remark { remark: vec![99u8; c as usize] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), encoded_call, expiry);

		// Verify new proposal was created and expired ones were cleaned
		let multisig = Multisigs::<T>::get(&multisig_address).unwrap();
		assert_eq!(multisig.active_proposals, 1); // Only new proposal remains

		Ok(())
	}

	#[benchmark]
	fn propose_high_security(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
		e: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, // expired proposals to cleanup
	) -> Result<(), BenchmarkError> {
		// Benchmarks propose() for high-security multisigs (includes decode + whitelist check)
		// This is more expensive than normal propose due to:
		// 1. is_high_security() check (1 DB read from ReversibleTransfers::HighSecurityAccounts)
		// 2. RuntimeCall decode (O(c) overhead - scales with call size)
		// 3. is_whitelisted() pattern matching
		//
		// NOTE: This benchmark measures the OVERHEAD of high-security checks,
		// not the functionality. The actual HighSecurity implementation is runtime-specific.
		// Mock implementation in tests would need to recognize this multisig as HS,
		// but for weight measurement, we're benchmarking the worst-case: full decode path.
		//
		// In production, the runtime's HighSecurityConfig will check:
		// - pallet_reversible_transfers::HighSecurityAccounts storage
		// - Pattern match against RuntimeCall variants

		// Setup: Create a high-security multisig
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(100000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(100000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;
		signers.sort();

		// Create multisig directly in storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			proposal_nonce: e,
			creator: caller.clone(),
			deposit: T::MultisigDeposit::get(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: e,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// IMPORTANT: Set this multisig as high-security for benchmarking
		// This ensures we measure the actual HS code path:
		// - is_high_security() will return true
		// - propose() will decode the call and check whitelist
		// - This adds ~25M base + ~50k/byte overhead vs normal propose
		#[cfg(feature = "runtime-benchmarks")]
		T::HighSecurity::set_high_security_for_benchmarking(&multisig_address);

		// Insert e expired proposals (worst case for auto-cleanup)
		let expired_block = 10u32.into();
		for i in 0..e {
			let system_call = frame_system::Call::<T>::remark { remark: vec![i as u8; 10] };
			let call = <T as Config>::RuntimeCall::from(system_call);
			let encoded_call = call.encode();
			let bounded_call: BoundedCallOf<T> = encoded_call.try_into().unwrap();
			let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

			let proposal_data = ProposalDataOf::<T> {
				proposer: caller.clone(),
				call: bounded_call,
				expiry: expired_block,
				approvals: bounded_approvals,
				deposit: 10u32.into(),
				status: ProposalStatus::Active,
			};
			Proposals::<T>::insert(&multisig_address, i, proposal_data);
		}

		// Move past expiry so proposals are expired
		frame_system::Pallet::<T>::set_block_number(100u32.into());

		// Create a whitelisted call for high-security
		// IMPORTANT: Use remark with variable size 'c' to measure decode overhead
		// The benchmark must vary the call size to properly measure O(c) decode cost
		// system::remark is used as proxy - in production this would be
		// ReversibleTransfers::schedule_transfer
		let system_call = frame_system::Call::<T>::remark { remark: vec![99u8; c as usize] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();

		// Verify we're testing with actual variable size
		assert!(encoded_call.len() >= c as usize, "Call size should scale with parameter c");

		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();

		#[extrinsic_call]
		propose(RawOrigin::Signed(caller.clone()), multisig_address.clone(), encoded_call, expiry);

		// Verify new proposal was created and expired ones were cleaned
		let multisig = Multisigs::<T>::get(&multisig_address).unwrap();
		assert_eq!(multisig.active_proposals, 1);

		Ok(())
	}

	#[benchmark]
	fn approve(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
		e: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, // expired proposals to cleanup
	) -> Result<(), BenchmarkError> {
		// Setup: Create multisig and proposal directly in storage
		// Threshold is 3, so adding one more approval won't trigger execution
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		let signer3: T::AccountId = benchmark_account("signer3", 2, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(100000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(100000u128));
		fund_account::<T>(&signer3, BalanceOf2::<T>::from(100000u128));

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
			proposal_nonce: e + 1, // We'll insert e expired proposals + 1 active
			creator: caller.clone(),
			deposit: T::MultisigDeposit::get(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: e + 1,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Insert e expired proposals (worst case for auto-cleanup)
		let expired_block = 10u32.into();
		for i in 0..e {
			let system_call = frame_system::Call::<T>::remark { remark: vec![i as u8; 10] };
			let call = <T as Config>::RuntimeCall::from(system_call);
			let encoded_call = call.encode();
			let bounded_call: BoundedCallOf<T> = encoded_call.try_into().unwrap();
			let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

			let proposal_data = ProposalDataOf::<T> {
				proposer: caller.clone(),
				call: bounded_call,
				expiry: expired_block,
				approvals: bounded_approvals,
				deposit: 10u32.into(),
				status: ProposalStatus::Active,
			};
			Proposals::<T>::insert(&multisig_address, i, proposal_data);
		}

		// Move past expiry so proposals are expired
		frame_system::Pallet::<T>::set_block_number(100u32.into());

		// Directly insert active proposal into storage with 1 approval
		// Create a remark call where the remark itself is c bytes
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; c as usize] };
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
			status: ProposalStatus::Active,
		};

		let proposal_id = e; // Active proposal after expired ones
		Proposals::<T>::insert(&multisig_address, proposal_id, proposal_data);

		#[extrinsic_call]
		_(RawOrigin::Signed(signer1.clone()), multisig_address.clone(), proposal_id);

		// Verify approval was added (now 2/3, not executed yet)
		let proposal = Proposals::<T>::get(&multisig_address, proposal_id).unwrap();
		assert!(proposal.approvals.contains(&signer1));
		assert_eq!(proposal.approvals.len(), 2);

		Ok(())
	}

	#[benchmark]
	fn approve_and_execute(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
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
			proposal_nonce: 1, // We'll insert proposal with id 0
			creator: caller.clone(),
			deposit: T::MultisigDeposit::get(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: 1,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Directly insert proposal with 1 approval (caller already approved)
		// signer2 will approve and trigger execution
		// Create a remark call where the remark itself is c bytes
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; c as usize] };
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
			status: ProposalStatus::Active,
		};

		let proposal_id = 0u32;
		Proposals::<T>::insert(&multisig_address, proposal_id, proposal_data);

		// signer2 approves, reaching threshold (2/2), triggering auto-execution
		#[extrinsic_call]
		approve(RawOrigin::Signed(signer2.clone()), multisig_address.clone(), proposal_id);

		// Verify proposal was removed from storage (auto-deleted after execution)
		assert!(!Proposals::<T>::contains_key(&multisig_address, proposal_id));

		Ok(())
	}

	#[benchmark]
	fn cancel(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
		e: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, // expired proposals to cleanup
	) -> Result<(), BenchmarkError> {
		// Setup: Create multisig and proposal directly in storage
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(100000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(100000u128));

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
			proposal_nonce: e + 1, // We'll insert e expired proposals + 1 active
			creator: caller.clone(),
			deposit: T::MultisigDeposit::get(),
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: e + 1,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Insert e expired proposals (worst case for auto-cleanup)
		let expired_block = 10u32.into();
		for i in 0..e {
			let system_call = frame_system::Call::<T>::remark { remark: vec![i as u8; 10] };
			let call = <T as Config>::RuntimeCall::from(system_call);
			let encoded_call = call.encode();
			let bounded_call: BoundedCallOf<T> = encoded_call.try_into().unwrap();
			let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

			let proposal_data = ProposalDataOf::<T> {
				proposer: caller.clone(),
				call: bounded_call,
				expiry: expired_block,
				approvals: bounded_approvals,
				deposit: 10u32.into(),
				status: ProposalStatus::Active,
			};
			Proposals::<T>::insert(&multisig_address, i, proposal_data);
		}

		// Move past expiry so proposals are expired
		frame_system::Pallet::<T>::set_block_number(100u32.into());

		// Directly insert active proposal into storage
		// Create a remark call where the remark itself is c bytes
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; c as usize] };
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
			status: ProposalStatus::Active,
		};

		let proposal_id = e; // Active proposal after expired ones
		Proposals::<T>::insert(&multisig_address, proposal_id, proposal_data);

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), proposal_id);

		// Verify proposal was removed from storage (auto-deleted after cancellation)
		assert!(!Proposals::<T>::contains_key(&multisig_address, proposal_id));

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
			proposal_nonce: 1, // We'll insert proposal with id 0
			creator: caller.clone(),
			deposit: T::MultisigDeposit::get(),
			last_activity: 1u32.into(),
			active_proposals: 1,
			proposals_per_signer: BoundedBTreeMap::new(),
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
			status: ProposalStatus::Active,
		};

		let proposal_id = 0u32;
		Proposals::<T>::insert(&multisig_address, proposal_id, proposal_data);

		// Move past expiry
		frame_system::Pallet::<T>::set_block_number(100u32.into());

		// Call as signer (caller is one of signers)
		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), proposal_id);

		// Verify proposal was removed
		assert!(!Proposals::<T>::contains_key(&multisig_address, proposal_id));

		Ok(())
	}

	#[benchmark]
	fn claim_deposits(
		p: Linear<1, { T::MaxTotalProposalsInStorage::get() }>, /* number of expired proposals
		                                                         * to cleanup */
	) -> Result<(), BenchmarkError> {
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
			proposal_nonce: p, // We'll insert p proposals with ids 0..p-1
			creator: caller.clone(),
			deposit: T::MultisigDeposit::get(),
			last_activity: 1u32.into(),
			active_proposals: p,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Create multiple expired proposals directly in storage
		let expiry = 10u32.into(); // Already expired

		for i in 0..p {
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
				status: ProposalStatus::Active,
			};

			Proposals::<T>::insert(&multisig_address, i, proposal_data);
		}

		// Move past expiry
		frame_system::Pallet::<T>::set_block_number(100u32.into());

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		// Verify all expired proposals were cleaned up
		assert_eq!(Proposals::<T>::iter_key_prefix(&multisig_address).count(), 0);

		Ok(())
	}

	#[benchmark]
	fn dissolve_multisig() -> Result<(), BenchmarkError> {
		// Setup: Create a clean multisig (no proposals, zero balance)
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let deposit = T::MultisigDeposit::get();

		// Reserve deposit from caller
		T::Currency::reserve(&caller, deposit)?;

		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			nonce: 0,
			proposal_nonce: 0,
			creator: caller.clone(),
			deposit,
			last_activity: frame_system::Pallet::<T>::block_number(),
			active_proposals: 0, // No proposals
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Ensure multisig address has zero balance (required for dissolution)
		// Don't fund it at all

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		// Verify multisig was removed
		assert!(!Multisigs::<T>::contains_key(&multisig_address));

		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
