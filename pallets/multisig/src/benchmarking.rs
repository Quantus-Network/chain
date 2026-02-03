//! Benchmarking setup for pallet-multisig

use super::*;
use crate::{
	BoundedApprovalsOf, BoundedCallOf, BoundedSignersOf, DissolveApprovals, MultisigDataOf,
	Multisigs, Pallet as Multisig, ProposalDataOf, ProposalStatus, Proposals,
};
use alloc::vec;
use frame_benchmarking::{account as benchmark_account, v2::*, BenchmarkError};
use frame_support::{
	traits::{fungible::Mutate, ReservableCurrency},
	BoundedBTreeMap,
};
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
	T: Config + pallet_balances::Config + pallet_reversible_transfers::Config,
	BalanceOf2<T>: From<u128>,
)]
mod benchmarks {
	use super::*;
	use codec::Encode;

	#[benchmark]
	fn create_multisig(
		s: Linear<2, { T::MaxSigners::get() }>, // number of signers
	) -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();

		// Fund the caller with enough balance for deposit
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		// Create signers (including caller)
		let mut signers = vec![caller.clone()];
		for i in 0..s.saturating_sub(1) {
			let signer: T::AccountId = benchmark_account("signer", i, SEED);
			signers.push(signer);
		}
		let threshold = 2u32;
		let nonce = 0u64;

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), signers.clone(), threshold, nonce);

		// Verify the multisig was created
		// Note: signers are sorted internally, so we must sort for address derivation
		let mut sorted_signers = signers.clone();
		sorted_signers.sort();
		let multisig_address =
			Multisig::<T>::derive_multisig_address(&sorted_signers, threshold, nonce);
		assert!(Multisigs::<T>::contains_key(multisig_address));

		Ok(())
	}

	#[benchmark]
	fn propose(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
		i: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, /* proposals iterated */
		r: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, /* proposals removed (cleaned) */
	) -> Result<(), BenchmarkError> {
		// NOTE: In benchmark we set i == r (worst-case: all expired)
		let e = i.max(r); // Use max for setup to ensure enough expired proposals
					// Setup: Create a multisig with 3 signers (standard test case)
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
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			proposal_nonce: e, // We'll insert e expired proposals
			deposit: T::MultisigDeposit::get(),
			active_proposals: e,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Insert e expired proposals (measures iteration cost, not cleanup cost)
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
		i: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, /* proposals iterated */
		r: Linear<0, { T::MaxTotalProposalsInStorage::get() }>, /* proposals removed (cleaned) */
	) -> Result<(), BenchmarkError> {
		// NOTE: In benchmark we set i == r (worst-case: all expired)
		let e = i.max(r);
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

		// Setup: Create a high-security multisig with 3 signers (standard test case)
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
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			proposal_nonce: e,
			deposit: T::MultisigDeposit::get(),
			active_proposals: e,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// IMPORTANT: Set this multisig as high-security for benchmarking
		// This ensures we measure the actual HS code path
		#[cfg(feature = "runtime-benchmarks")]
		{
			use pallet_reversible_transfers::{
				benchmarking::insert_hs_account_for_benchmark, HighSecurityAccountData,
			};
			use qp_scheduler::BlockNumberOrTimestamp;

			let hs_data = HighSecurityAccountData {
				interceptor: multisig_address.clone(),
				delay: BlockNumberOrTimestamp::BlockNumber(100u32.into()),
			};
			// Use helper that accepts T: pallet_reversible_transfers::Config
			insert_hs_account_for_benchmark::<T>(multisig_address.clone(), hs_data);
		}

		// Insert e expired proposals (measures iteration cost, not cleanup cost)
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

		// Create a whitelisted call for HS multisig
		// Using system::remark with variable size to measure decode cost O(c)
		// NOTE: system::remark is whitelisted ONLY in runtime-benchmarks mode
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
	) -> Result<(), BenchmarkError> {
		// NOTE: approve() does NOT do auto-cleanup (removed for predictable gas costs)
		// So we don't need to test with expired proposals (e parameter removed)

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
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			proposal_nonce: 1, // One active proposal
			deposit: T::MultisigDeposit::get(),
			active_proposals: 1,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Set current block to avoid expiry issues
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

		let proposal_id = 0; // Single active proposal
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
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			proposal_nonce: 1, // We'll insert proposal with id 0
			deposit: T::MultisigDeposit::get(),
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
	fn cancel() -> Result<(), BenchmarkError> {
		// Setup: Create multisig and proposal directly in storage
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = benchmark_account("signer1", 0, SEED);
		let signer2: T::AccountId = benchmark_account("signer2", 1, SEED);

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			proposal_nonce: 1,
			deposit: T::MultisigDeposit::get(),
			active_proposals: 1,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Directly insert active proposal into storage
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; 10] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		let bounded_call: BoundedCallOf<T> = encoded_call.try_into().unwrap();
		let bounded_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

		let proposal_data = ProposalDataOf::<T> {
			proposer: caller.clone(),
			call: bounded_call,
			expiry,
			approvals: bounded_approvals,
			deposit: T::ProposalDeposit::get(),
			status: ProposalStatus::Active,
		};

		let proposal_id = 0;
		Proposals::<T>::insert(&multisig_address, proposal_id, proposal_data);

		// Reserve deposit for proposer
		<T as crate::Config>::Currency::reserve(&caller, T::ProposalDeposit::get()).unwrap();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), proposal_id);

		// Verify proposal was removed from storage
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
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			proposal_nonce: 1, // We'll insert proposal with id 0
			deposit: T::MultisigDeposit::get(),
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
		i: Linear<1, { T::MaxTotalProposalsInStorage::get() }>, /* proposals iterated */
		r: Linear<1, { T::MaxTotalProposalsInStorage::get() }>, /* proposals removed (cleaned) */
	) -> Result<(), BenchmarkError> {
		// NOTE: In benchmark we set i == r (worst-case: all expired)
		let p = i.max(r);

		// Setup: Create multisig with 3 signers and multiple expired proposals
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
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers,
			threshold,
			proposal_nonce: p, // We'll insert p proposals with ids 0..p-1
			deposit: T::MultisigDeposit::get(),
			active_proposals: p,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Create multiple expired proposals directly in storage
		// NOTE: All proposals are expired and belong to caller, so:
		//   - total_iterated = p (what we measure)
		//   - cleaned = p (side effect)
		// We charge for iteration cost, not cleanup count!
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
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let deposit = T::MultisigDeposit::get();

		// Reserve deposit from caller
		<T as crate::Config>::Currency::reserve(&caller, deposit)?;

		let multisig_data = MultisigDataOf::<T> {
			signers: bounded_signers.clone(),
			threshold,
			proposal_nonce: 0,
			deposit,
			active_proposals: 0, // No proposals
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Add first approval (signer1)
		let mut approvals = BoundedApprovalsOf::<T>::default();
		approvals.try_push(signer1.clone()).unwrap();
		DissolveApprovals::<T>::insert(&multisig_address, approvals);

		// Ensure multisig address has zero balance (required for dissolution)
		// Don't fund it at all

		// Benchmark the final approval that triggers dissolution
		#[extrinsic_call]
		approve_dissolve(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		// Verify multisig was removed
		assert!(!Multisigs::<T>::contains_key(&multisig_address));

		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
