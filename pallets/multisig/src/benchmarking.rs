//! Benchmarking setup for pallet-multisig

use super::*;
use crate::{
	BoundedApprovalsOf, BoundedCallOf, BoundedSignersOf, DissolveApprovals, MultisigDataOf,
	Multisigs, Pallet as Multisig, ProposalDataOf, ProposalStatus, Proposals,
};
use alloc::vec;
use frame_benchmarking::v2::*;
use frame_support::{traits::fungible::Mutate, BoundedBTreeMap};

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
	use frame_support::traits::ReservableCurrency;
	use frame_system::RawOrigin;

	/// Benchmark `create_multisig` extrinsic.
	/// Parameter: s = signers count
	#[benchmark]
	fn create_multisig(s: Linear<2, { T::MaxSigners::get() }>) -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();

		// Fund the caller with enough balance for deposit
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		// Create signers (including caller)
		let mut signers = vec![caller.clone()];
		for n in 0..s.saturating_sub(1) {
			let signer: T::AccountId = account("signer", n, SEED);
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

	/// Benchmark `propose` extrinsic (non-HS path).
	/// Parameter: c = call size
	#[benchmark]
	fn propose(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(100000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(100000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;
		signers.sort();

		// Create multisig directly in storage (empty, no existing proposals)
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			creator: caller.clone(),
			signers: bounded_signers,
			threshold,
			proposal_nonce: 0,
			deposit: T::MultisigDeposit::get(),
			active_proposals: 0,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		frame_system::Pallet::<T>::set_block_number(100u32.into());

		let new_call = frame_system::Call::<T>::remark { remark: vec![99u8; c as usize] };
		let encoded_call = <T as Config>::RuntimeCall::from(new_call).encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), encoded_call, expiry);

		// Verify proposal was created
		let multisig = Multisigs::<T>::get(&multisig_address).unwrap();
		assert_eq!(multisig.active_proposals, 1);

		Ok(())
	}

	/// Benchmark `propose` for high-security multisigs (includes decode + whitelist check).
	/// More expensive than normal propose due to:
	/// 1. is_high_security() check (1 DB read from ReversibleTransfers::HighSecurityAccounts)
	/// 2. RuntimeCall decode (O(c) overhead - scales with call size)
	/// 3. is_whitelisted() pattern matching
	/// Parameter: c = call size
	#[benchmark]
	fn propose_high_security(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(100000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(100000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;
		signers.sort();

		// Create multisig directly in storage (empty, no existing proposals)
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			creator: caller.clone(),
			signers: bounded_signers,
			threshold,
			proposal_nonce: 0,
			deposit: T::MultisigDeposit::get(),
			active_proposals: 0,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Set this multisig as high-security for benchmarking
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
			insert_hs_account_for_benchmark::<T>(multisig_address.clone(), hs_data);
		}

		frame_system::Pallet::<T>::set_block_number(100u32.into());

		// Whitelisted call with variable size to measure decode cost O(c)
		// NOTE: system::remark is whitelisted ONLY in runtime-benchmarks mode
		let new_call = frame_system::Call::<T>::remark { remark: vec![99u8; c as usize] };
		let encoded_call = <T as Config>::RuntimeCall::from(new_call).encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();

		#[extrinsic_call]
		propose(RawOrigin::Signed(caller.clone()), multisig_address.clone(), encoded_call, expiry);

		// Verify proposal was created
		let multisig = Multisigs::<T>::get(&multisig_address).unwrap();
		assert_eq!(multisig.active_proposals, 1);

		Ok(())
	}

	/// Benchmark `approve` extrinsic (without execution).
	/// Parameter: c = call size (stored proposal call)
	#[benchmark]
	fn approve(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		// NOTE: approve() does NOT do auto-cleanup (removed for predictable gas costs)
		// So we don't need to test with expired proposals

		// Setup: Create multisig and proposal directly in storage
		// Threshold is 3, so adding one more approval won't trigger execution
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);
		let signer3: T::AccountId = account("signer3", 2, SEED);
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
			creator: caller.clone(),
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

	/// Benchmark `execute` extrinsic (dispatches an Approved proposal).
	/// Parameter: c = call size
	#[benchmark]
	fn execute(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(10000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(10000u128));

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;
		signers.sort();

		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			creator: caller.clone(),
			signers: bounded_signers,
			threshold,
			proposal_nonce: 1,
			deposit: T::MultisigDeposit::get(),
			active_proposals: 1,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Insert an Approved proposal (threshold already reached)
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; c as usize] };
		let call = <T as Config>::RuntimeCall::from(system_call);
		let encoded_call = call.encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		let bounded_call: BoundedCallOf<T> = encoded_call.try_into().unwrap();
		let bounded_approvals: BoundedApprovalsOf<T> =
			vec![caller.clone(), signer1.clone()].try_into().unwrap();

		let proposal_data = ProposalDataOf::<T> {
			proposer: caller.clone(),
			call: bounded_call,
			expiry,
			approvals: bounded_approvals,
			deposit: 10u32.into(),
			status: ProposalStatus::Approved,
		};

		let proposal_id = 0u32;
		Proposals::<T>::insert(&multisig_address, proposal_id, proposal_data);

		#[extrinsic_call]
		_(RawOrigin::Signed(signer2.clone()), multisig_address.clone(), proposal_id);

		// Verify proposal was removed from storage after execution
		assert!(!Proposals::<T>::contains_key(&multisig_address, proposal_id));

		Ok(())
	}

	#[benchmark]
	fn cancel() -> Result<(), BenchmarkError> {
		// Setup: Create multisig and proposal directly in storage
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);

		let mut signers = vec![caller.clone(), signer1.clone(), signer2.clone()];
		let threshold = 2u32;

		// Sort signers to match create_multisig behavior
		signers.sort();

		// Directly insert multisig into storage
		let multisig_address = Multisig::<T>::derive_multisig_address(&signers, threshold, 0);
		let bounded_signers: BoundedSignersOf<T> = signers.clone().try_into().unwrap();
		let multisig_data = MultisigDataOf::<T> {
			creator: caller.clone(),
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

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);
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
			creator: caller.clone(),
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

	/// Benchmark `claim_deposits` extrinsic.
	/// Parameters: i = iterated proposals, r = removed (cleaned) proposals
	#[benchmark]
	fn claim_deposits(
		i: Linear<1, { T::MaxTotalProposalsInStorage::get() }>,
		r: Linear<1, { T::MaxTotalProposalsInStorage::get() }>,
	) -> Result<(), BenchmarkError> {
		// cleaned_target = min(r, i): can't clean more proposals than we iterate
		let cleaned_target = (r as u32).min(i);

		// Total proposals = i (maps directly to iteration parameter)
		// No edge case needed here: claim_deposits doesn't create a new proposal,
		// so there's no `total < Max` check to worry about.
		let total_proposals = i;

		// Setup: Create multisig with 3 signers and multiple proposals
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100000u128));

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);
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
			creator: caller.clone(),
			signers: bounded_signers,
			threshold,
			proposal_nonce: total_proposals,
			deposit: T::MultisigDeposit::get(),
			active_proposals: total_proposals,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, multisig_data);

		// Build proposal template once - only expiry varies per proposal
		let template_call: BoundedCallOf<T> = {
			let system_call = frame_system::Call::<T>::remark { remark: vec![0u8; 32] };
			<T as Config>::RuntimeCall::from(system_call).encode().try_into().unwrap()
		};
		let template_approvals: BoundedApprovalsOf<T> = vec![caller.clone()].try_into().unwrap();

		// Insert proposals: first `cleaned_target` are expired, rest are non-expired.
		// This separates iteration cost (read all total_proposals) from cleanup cost
		// (delete cleaned_target).
		let expired_block = 10u32.into();
		let future_block = 999999u32.into();
		for idx in 0..total_proposals {
			let expiry = if idx < cleaned_target { expired_block } else { future_block };
			Proposals::<T>::insert(
				&multisig_address,
				idx,
				ProposalDataOf::<T> {
					proposer: caller.clone(),
					call: template_call.clone(),
					expiry,
					approvals: template_approvals.clone(),
					deposit: 10u32.into(),
					status: ProposalStatus::Active,
				},
			);
		}

		// Move past expired_block but before future_block
		frame_system::Pallet::<T>::set_block_number(100u32.into());

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		// Verify: only non-expired proposals remain
		let remaining = Proposals::<T>::iter_key_prefix(&multisig_address).count() as u32;
		assert_eq!(remaining, total_proposals - cleaned_target);

		Ok(())
	}

	#[benchmark]
	fn dissolve_multisig() -> Result<(), BenchmarkError> {
		// Setup: Create a clean multisig (no proposals, zero balance)
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(10000u128));

		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);

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
			creator: caller.clone(),
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
