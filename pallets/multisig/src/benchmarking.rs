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

/// Multisig address used by the `propose_high_security` benchmark (signer1+signer2+caller).
/// Exposed so the mock `HighSecurity` can treat it as HS in unit tests.
#[cfg(any(test, feature = "runtime-benchmarks"))]
#[allow(dead_code)]
pub fn propose_high_security_benchmark_multisig_address<T>() -> T::AccountId
where
	T: Config + pallet_balances::Config,
	BalanceOf2<T>: From<u128>,
{
	use frame_benchmarking::v2::{account, whitelisted_caller};
	let caller: T::AccountId = whitelisted_caller();
	let signer1: T::AccountId = account("signer1", 0, SEED);
	let signer2: T::AccountId = account("signer2", 1, SEED);
	let mut signers = vec![caller, signer1, signer2];
	signers.sort();
	Multisig::<T>::derive_multisig_address(&signers, 2, 0)
}

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
	use frame_support::traits::ReservableCurrency;
	use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
	use qp_high_security::HighSecurityInspector;

	// ---------- Reusable setup helpers (keep benchmark bodies focused on what we measure)
	// ----------

	/// Funded caller + signers (sorted). Caller is first in the list.
	fn setup_funded_signer_set<T: Config + pallet_balances::Config>(
		signer_count: u32,
	) -> (T::AccountId, Vec<T::AccountId>)
	where
		BalanceOf2<T>: From<u128>,
	{
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100_000u128));
		let mut signers = vec![caller.clone()];
		for i in 0..signer_count.saturating_sub(1) {
			let s: T::AccountId = account("signer", i, SEED);
			fund_account::<T>(&s, BalanceOf2::<T>::from(100_000u128));
			signers.push(s);
		}
		signers.sort();
		(caller, signers)
	}

	/// Funded caller + signers matching genesis (signer1, signer2). Multisig address is in
	/// ReversibleTransfers::initial_high_security_accounts when runtime-benchmarks.
	fn setup_funded_signer_set_hs<T: Config + pallet_balances::Config>(
	) -> (T::AccountId, Vec<T::AccountId>)
	where
		BalanceOf2<T>: From<u128>,
	{
		let caller: T::AccountId = whitelisted_caller();
		let signer1: T::AccountId = account("signer1", 0, SEED);
		let signer2: T::AccountId = account("signer2", 1, SEED);
		fund_account::<T>(&caller, BalanceOf2::<T>::from(100_000u128));
		fund_account::<T>(&signer1, BalanceOf2::<T>::from(100_000u128));
		fund_account::<T>(&signer2, BalanceOf2::<T>::from(100_000u128));
		let mut signers = vec![caller.clone(), signer1, signer2];
		signers.sort();
		(caller, signers)
	}

	/// Insert multisig into storage (bypasses create_multisig). Returns multisig address.
	fn insert_multisig<T: Config>(
		caller: &T::AccountId,
		signers: &[T::AccountId],
		threshold: u32,
		nonce: u64,
		proposal_nonce: u32,
		active_proposals: u32,
	) -> T::AccountId {
		let multisig_address = Multisig::<T>::derive_multisig_address(signers, threshold, nonce);
		let bounded_signers: BoundedSignersOf<T> = signers.to_vec().try_into().unwrap();
		let data = MultisigDataOf::<T> {
			creator: caller.clone(),
			signers: bounded_signers,
			threshold,
			proposal_nonce,
			deposit: T::MultisigDeposit::get(),
			active_proposals,
			proposals_per_signer: BoundedBTreeMap::new(),
		};
		Multisigs::<T>::insert(&multisig_address, data);
		multisig_address
	}

	fn set_block<T: frame_system::Config>(n: u32)
	where
		BlockNumberFor<T>: From<u32>,
	{
		frame_system::Pallet::<T>::set_block_number(n.into());
	}

	/// Returns a Vec of MaxSigners account IDs for worst-case approvals decode cost.
	fn approvals_max<T: Config>() -> Vec<T::AccountId> {
		(0..T::MaxSigners::get()).map(|i| account("approval", i, SEED)).collect()
	}

	/// Insert a single proposal into storage. `approvals` = list of account ids that have approved.
	#[allow(clippy::too_many_arguments)]
	fn insert_proposal<T: Config>(
		multisig_address: &T::AccountId,
		proposal_id: u32,
		proposer: &T::AccountId,
		call_size: u32,
		expiry: BlockNumberFor<T>,
		approvals: &[T::AccountId],
		status: ProposalStatus,
		deposit: crate::BalanceOf<T>,
	) {
		let system_call = frame_system::Call::<T>::remark { remark: vec![1u8; call_size as usize] };
		let encoded = <T as Config>::RuntimeCall::from(system_call).encode();
		let bounded_call: BoundedCallOf<T> = encoded.try_into().unwrap();
		let bounded_approvals: BoundedApprovalsOf<T> = approvals.to_vec().try_into().unwrap();
		let proposal_data = ProposalDataOf::<T> {
			proposer: proposer.clone(),
			call: bounded_call,
			expiry,
			approvals: bounded_approvals,
			deposit,
			status,
		};
		Proposals::<T>::insert(multisig_address, proposal_id, proposal_data);
	}

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
	/// Uses different signers than propose_high_security so the multisig address is NOT in
	/// HighSecurityAccounts (dev genesis records whitelisted_caller+signer1+signer2). No decode, no
	/// whitelist. Parameter: c = call size
	#[benchmark]
	fn propose(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		// Uses account("signer", 0/1) so multisig address differs from genesis (signer1/signer2).
		let (caller, signers) = setup_funded_signer_set::<T>(3);
		let threshold = 2u32;
		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 0, 0);
		assert!(
			!T::HighSecurity::is_high_security(&multisig_address),
			"propose must hit non-HS path"
		);
		set_block::<T>(100);

		let new_call = frame_system::Call::<T>::remark { remark: vec![99u8; c as usize] };
		let encoded_call = <T as Config>::RuntimeCall::from(new_call).encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), encoded_call, expiry);

		let multisig = Multisigs::<T>::get(&multisig_address).unwrap();
		assert_eq!(multisig.active_proposals, 1);
		Ok(())
	}

	/// Benchmark `propose` for high-security multisigs.
	/// Uses signer1/signer2 so multisig address matches genesis (ReversibleTransfers::
	/// initial_high_security_accounts) or mock's HighSecurity (unit tests).
	/// Uses whitelisted call (remark "safe") so HS path accepts it.
	#[benchmark]
	fn propose_high_security(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let _ = c;
		let (caller, signers) = setup_funded_signer_set_hs::<T>();
		let threshold = 2u32;
		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 0, 0);
		assert!(
			T::HighSecurity::is_high_security(&multisig_address),
			"propose_high_security must hit HS path"
		);
		set_block::<T>(100);

		let new_call = frame_system::Call::<T>::remark { remark: b"safe".to_vec() };
		let encoded_call = <T as Config>::RuntimeCall::from(new_call).encode();
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();

		#[extrinsic_call]
		propose(RawOrigin::Signed(caller.clone()), multisig_address.clone(), encoded_call, expiry);

		let multisig = Multisigs::<T>::get(&multisig_address).unwrap();
		assert_eq!(multisig.active_proposals, 1);
		Ok(())
	}

	/// Benchmark `approve` extrinsic (without execution). Uses MaxSigners for worst-case approvals
	/// decode. Threshold = MaxSigners, 99 approvals pre-stored, approver adds 100th.
	/// Parameter: c = call size (stored proposal call)
	#[benchmark]
	fn approve(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let max_s = T::MaxSigners::get();
		let (caller, signers) = setup_funded_signer_set::<T>(max_s);
		let threshold = max_s;
		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 1, 1);
		set_block::<T>(100);
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		// Worst-case approvals decode: threshold-1 approvals (99 for MaxSigners=100)
		let approvals: Vec<_> = signers[0..threshold as usize - 1].to_vec();
		insert_proposal::<T>(
			&multisig_address,
			0,
			&caller,
			c,
			expiry,
			&approvals,
			ProposalStatus::Active,
			10u32.into(),
		);
		let approver = signers[threshold as usize - 1].clone();

		#[extrinsic_call]
		_(RawOrigin::Signed(approver), multisig_address.clone(), 0u32);

		let proposal = Proposals::<T>::get(&multisig_address, 0).unwrap();
		assert_eq!(proposal.approvals.len(), threshold as usize);
		Ok(())
	}

	/// Benchmark `execute` extrinsic (dispatches an Approved proposal).
	/// Uses MaxSigners approvals for worst-case decode. Parameter: c = call size
	#[benchmark]
	fn execute(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let max_s = T::MaxSigners::get();
		let (caller, signers) = setup_funded_signer_set::<T>(max_s);
		let threshold = max_s;
		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 1, 1);
		set_block::<T>(100);
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		// Worst-case approvals decode: MaxSigners approvals (Approved)
		insert_proposal::<T>(
			&multisig_address,
			0,
			&caller,
			c,
			expiry,
			&signers,
			ProposalStatus::Approved,
			10u32.into(),
		);
		let executor = signers[0].clone();

		#[extrinsic_call]
		_(RawOrigin::Signed(executor), multisig_address.clone(), 0u32);

		assert!(!Proposals::<T>::contains_key(&multisig_address, 0));
		Ok(())
	}

	/// Benchmark `cancel` extrinsic. Uses MaxSigners approvals for worst-case decode.
	/// Parameter: c = stored proposal call size
	#[benchmark]
	fn cancel(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let (caller, signers) = setup_funded_signer_set::<T>(3);
		let threshold = 2u32;
		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 1, 1);
		set_block::<T>(100);
		let expiry = frame_system::Pallet::<T>::block_number() + 1000u32.into();
		let approvals = approvals_max::<T>();
		insert_proposal::<T>(
			&multisig_address,
			0,
			&caller,
			c,
			expiry,
			&approvals,
			ProposalStatus::Active,
			T::ProposalDeposit::get(),
		);
		<T as crate::Config>::Currency::reserve(&caller, T::ProposalDeposit::get()).unwrap();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), 0u32);

		assert!(!Proposals::<T>::contains_key(&multisig_address, 0));
		Ok(())
	}

	/// Benchmark `remove_expired` extrinsic. Uses MaxSigners approvals for worst-case decode.
	/// Parameter: c = stored proposal call size
	#[benchmark]
	fn remove_expired(
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let (caller, signers) = setup_funded_signer_set::<T>(3);
		let threshold = 2u32;
		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 1, 1);
		let expiry = 10u32.into();
		let approvals = approvals_max::<T>();
		insert_proposal::<T>(
			&multisig_address,
			0,
			&caller,
			c,
			expiry,
			&approvals,
			ProposalStatus::Active,
			10u32.into(),
		);
		set_block::<T>(100);

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone(), 0u32);

		assert!(!Proposals::<T>::contains_key(&multisig_address, 0));
		Ok(())
	}

	/// Benchmark `claim_deposits` extrinsic. Uses MaxSigners approvals per proposal for worst-case
	/// decode. Parameters: i = iterated proposals, r = removed (cleaned) proposals,
	/// c = average stored call size (affects iteration cost)
	#[benchmark]
	fn claim_deposits(
		i: Linear<1, { T::MaxTotalProposalsInStorage::get() }>,
		r: Linear<1, { T::MaxTotalProposalsInStorage::get() }>,
		c: Linear<0, { T::MaxCallSize::get().saturating_sub(100) }>,
	) -> Result<(), BenchmarkError> {
		let cleaned_target = (r as u32).min(i);
		let total_proposals = i;

		let (caller, signers) = setup_funded_signer_set::<T>(3);
		let threshold = 2u32;
		let multisig_address =
			insert_multisig::<T>(&caller, &signers, threshold, 0, total_proposals, total_proposals);

		let approvals = approvals_max::<T>();
		let expired_block = 10u32.into();
		let future_block = 999999u32.into();
		for idx in 0..total_proposals {
			let expiry = if idx < cleaned_target { expired_block } else { future_block };
			insert_proposal::<T>(
				&multisig_address,
				idx,
				&caller,
				c,
				expiry,
				&approvals,
				ProposalStatus::Active,
				10u32.into(),
			);
		}

		set_block::<T>(100);

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		let remaining = Proposals::<T>::iter_key_prefix(&multisig_address).count() as u32;
		assert_eq!(remaining, total_proposals - cleaned_target);
		Ok(())
	}

	/// Benchmark `approve_dissolve` when threshold is NOT reached.
	/// Just adds an approval to DissolveApprovals (cheap path).
	#[benchmark]
	fn approve_dissolve() -> Result<(), BenchmarkError> {
		let (caller, signers) = setup_funded_signer_set::<T>(3);
		let threshold = 3u32; // Need 3 approvals, we add 1st
		let deposit = T::MultisigDeposit::get();
		<T as crate::Config>::Currency::reserve(&caller, deposit)?;

		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 0, 0);
		// No pre-inserted approvals - caller adds first approval (threshold not reached)

		#[extrinsic_call]
		approve_dissolve(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		assert!(Multisigs::<T>::contains_key(&multisig_address));
		assert!(DissolveApprovals::<T>::get(&multisig_address).unwrap().len() == 1);
		Ok(())
	}

	/// Benchmark `approve_dissolve` when threshold IS reached (dissolves multisig).
	#[benchmark]
	fn approve_dissolve_threshold_reached() -> Result<(), BenchmarkError> {
		let (caller, signers) = setup_funded_signer_set::<T>(3);
		let threshold = 2u32;
		let deposit = T::MultisigDeposit::get();
		<T as crate::Config>::Currency::reserve(&caller, deposit)?;

		let multisig_address = insert_multisig::<T>(&caller, &signers, threshold, 0, 0, 0);
		// Pre-insert one approval from a signer that is NOT the caller (avoid AlreadyApproved).
		let first_approval = signers.iter().find(|s| *s != &caller).unwrap().clone();
		let mut approvals = BoundedApprovalsOf::<T>::default();
		approvals.try_push(first_approval).unwrap();
		DissolveApprovals::<T>::insert(&multisig_address, approvals);

		#[extrinsic_call]
		approve_dissolve(RawOrigin::Signed(caller.clone()), multisig_address.clone());

		assert!(!Multisigs::<T>::contains_key(&multisig_address));
		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
