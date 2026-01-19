//! # Quantus Multisig Pallet
//!
//! This pallet provides multisignature functionality for managing shared accounts
//! that require multiple approvals before executing transactions.
//!
//! ## Features
//!
//! - Create multisig addresses with configurable thresholds
//! - Propose transactions for multisig approval
//! - Approve proposed transactions
//! - Execute transactions once threshold is reached
//!
//! ## Data Structures
//!
//! - **Multisig**: Contains signers, threshold, and global nonce
//! - **Proposal**: Contains transaction data, proposer, expiry, and approvals

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::vec::Vec;
pub use pallet::*;
pub use weights::*;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub mod weights;

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{traits::Get, BoundedVec};
use scale_info::TypeInfo;
use sp_runtime::RuntimeDebug;

/// Multisig account data
#[derive(Encode, Decode, MaxEncodedLen, Clone, TypeInfo, RuntimeDebug, PartialEq, Eq)]
pub struct MultisigData<Balance, BlockNumber, AccountId, BoundedSigners> {
	/// List of signers who can approve transactions
	pub signers: BoundedSigners,
	/// Number of approvals required to execute a transaction
	pub threshold: u32,
	/// Global unique identifier for this multisig
	pub nonce: u64,
	/// Deposit required for storage (refundable after grace period)
	pub deposit: Balance,
	/// Account that created this multisig (receives deposit back)
	pub creator: AccountId,
	/// Last block when this multisig was used (for grace period calculation)
	pub last_activity: BlockNumber,
	/// Number of currently active (non-executed/non-cancelled) proposals
	pub active_proposals: u32,
}

impl<Balance: Default, BlockNumber: Default, AccountId: Default, BoundedSigners: Default> Default
	for MultisigData<Balance, BlockNumber, AccountId, BoundedSigners>
{
	fn default() -> Self {
		Self {
			signers: Default::default(),
			threshold: 1,
			nonce: 0,
			deposit: Default::default(),
			creator: Default::default(),
			last_activity: Default::default(),
			active_proposals: 0,
		}
	}
}

/// Proposal data
#[derive(Encode, Decode, MaxEncodedLen, Clone, TypeInfo, RuntimeDebug, PartialEq, Eq)]
pub struct ProposalData<AccountId, Balance, BlockNumber, BoundedCall, BoundedApprovals> {
	/// Account that proposed this transaction
	pub proposer: AccountId,
	/// The encoded call to be executed
	pub call: BoundedCall,
	/// Expiry block number
	pub expiry: BlockNumber,
	/// List of accounts that have approved this proposal
	pub approvals: BoundedApprovals,
	/// Deposit held for this proposal (returned on execute or cancel)
	pub deposit: Balance,
}

/// Executed proposal archive (only successfully executed proposals)
#[derive(Encode, Decode, MaxEncodedLen, Clone, TypeInfo, RuntimeDebug, PartialEq, Eq)]
pub struct ExecutedProposalData<AccountId, BlockNumber, BoundedCall, BoundedApprovals> {
	/// Account that proposed this transaction
	pub proposer: AccountId,
	/// The encoded call that was executed
	pub call: BoundedCall,
	/// List of accounts that approved this proposal
	pub approvers: BoundedApprovals,
	/// Block number when it was executed
	pub executed_at: BlockNumber,
	/// Whether the execution succeeded
	pub execution_succeeded: bool,
}

/// Balance type
type BalanceOf<T> = <<T as Config>::Currency as frame_support::traits::Currency<
	<T as frame_system::Config>::AccountId,
>>::Balance;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use codec::Encode;
	use frame_support::{
		dispatch::{DispatchResult, GetDispatchInfo, PostDispatchInfo},
		pallet_prelude::*,
		traits::{Currency, ReservableCurrency},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use sp_arithmetic::traits::Saturating;
	use sp_runtime::traits::{Dispatchable, Hash, TrailingZeroInput};

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config<RuntimeEvent: From<Event<Self>>> {
		/// The overarching call type
		type RuntimeCall: Parameter
			+ Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
			+ GetDispatchInfo
			+ From<frame_system::Call<Self>>
			+ codec::Decode;

		/// Currency type for handling deposits
		type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;

		/// Maximum number of signers allowed in a multisig
		#[pallet::constant]
		type MaxSigners: Get<u32>;

		/// Maximum number of active (open) proposals per multisig at any given time
		#[pallet::constant]
		type MaxActiveProposals: Get<u32>;

		/// Maximum size of an encoded call
		#[pallet::constant]
		type MaxCallSize: Get<u32>;

		/// Deposit required per multisig account (refundable after grace period)
		#[pallet::constant]
		type MultisigDeposit: Get<BalanceOf<Self>>;

		/// Fee charged for creating a multisig (non-refundable, paid always)
		#[pallet::constant]
		type MultisigFee: Get<BalanceOf<Self>>;

		/// Deposit required per proposal (returned on execute or cancel)
		#[pallet::constant]
		type ProposalDeposit: Get<BalanceOf<Self>>;

		/// Fee charged for creating a proposal (non-refundable, paid always)
		#[pallet::constant]
		type ProposalFee: Get<BalanceOf<Self>>;

		/// Grace period after expiry when proposer can still recover deposit
		/// After this period, anyone can remove the proposal and deposit is returned to proposer
		#[pallet::constant]
		type GracePeriod: Get<BlockNumberFor<Self>>;

		/// Maximum number of executed proposals to return in a single query
		/// This prevents DoS attacks via large RPC queries
		#[pallet::constant]
		type MaxExecutedProposalsQuery: Get<u32>;

		/// Pallet ID for generating multisig addresses
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		/// Weight information for extrinsics
		type WeightInfo: WeightInfo;
	}

	/// Type alias for bounded signers vector
	pub type BoundedSignersOf<T> =
		BoundedVec<<T as frame_system::Config>::AccountId, <T as Config>::MaxSigners>;

	/// Type alias for bounded approvals vector
	pub type BoundedApprovalsOf<T> =
		BoundedVec<<T as frame_system::Config>::AccountId, <T as Config>::MaxSigners>;

	/// Type alias for bounded call data
	pub type BoundedCallOf<T> = BoundedVec<u8, <T as Config>::MaxCallSize>;

	/// Type alias for MultisigData with proper bounds
	pub type MultisigDataOf<T> = MultisigData<
		BalanceOf<T>,
		BlockNumberFor<T>,
		<T as frame_system::Config>::AccountId,
		BoundedSignersOf<T>,
	>;

	/// Type alias for ProposalData with proper bounds
	pub type ProposalDataOf<T> = ProposalData<
		<T as frame_system::Config>::AccountId,
		BalanceOf<T>,
		BlockNumberFor<T>,
		BoundedCallOf<T>,
		BoundedApprovalsOf<T>,
	>;

	/// Type alias for paginated query results
	pub type PaginatedProposalsOf<T> = (
		Vec<(<T as frame_system::Config>::Hash, ExecutedProposalDataOf<T>)>,
		Option<<T as frame_system::Config>::Hash>,
	);

	/// Type alias for ExecutedProposalData with proper bounds
	pub type ExecutedProposalDataOf<T> = ExecutedProposalData<
		<T as frame_system::Config>::AccountId,
		BlockNumberFor<T>,
		BoundedCallOf<T>,
		BoundedApprovalsOf<T>,
	>;

	/// Global nonce for generating unique multisig addresses
	#[pallet::storage]
	pub type GlobalNonce<T: Config> = StorageValue<_, u64, ValueQuery>;

	/// Multisigs stored by their generated address
	#[pallet::storage]
	#[pallet::getter(fn multisigs)]
	pub type Multisigs<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, MultisigDataOf<T>, OptionQuery>;

	/// Proposals indexed by (multisig_address, proposal_hash)
	#[pallet::storage]
	#[pallet::getter(fn proposals)]
	pub type Proposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Blake2_128Concat,
		T::Hash,
		ProposalDataOf<T>,
		OptionQuery,
	>;

	/// Archive of successfully executed proposals
	/// Only proposals that were executed successfully are stored here
	/// Cancelled or expired proposals are NOT archived
	///
	/// Use `get_executed_proposals_paginated()` to query with pagination
	#[pallet::storage]
	pub type ExecutedProposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Blake2_128Concat,
		T::Hash,
		ExecutedProposalDataOf<T>,
		OptionQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A new multisig account was created
		/// [creator, multisig_address, signers, threshold, nonce]
		MultisigCreated {
			creator: T::AccountId,
			multisig_address: T::AccountId,
			signers: Vec<T::AccountId>,
			threshold: u32,
			nonce: u64,
		},
		/// A transaction has been proposed
		/// [multisig_address, proposer, proposal_hash]
		TransactionProposed {
			multisig_address: T::AccountId,
			proposer: T::AccountId,
			proposal_hash: T::Hash,
		},
		/// A transaction has been approved
		/// [multisig_address, approver, proposal_hash, approvals_count]
		TransactionApproved {
			multisig_address: T::AccountId,
			approver: T::AccountId,
			proposal_hash: T::Hash,
			approvals_count: u32,
		},
		/// A transaction has been executed
		/// [multisig_address, proposal_hash, result]
		TransactionExecuted {
			multisig_address: T::AccountId,
			proposal_hash: T::Hash,
			result: DispatchResult,
		},
		/// A transaction has been cancelled
		/// [multisig_address, proposer, proposal_hash]
		TransactionCancelled {
			multisig_address: T::AccountId,
			proposer: T::AccountId,
			proposal_hash: T::Hash,
		},
		/// Expired proposal was removed from storage
		ProposalRemoved {
			multisig_address: T::AccountId,
			proposal_hash: T::Hash,
			proposer: T::AccountId,
			removed_by: T::AccountId,
			in_grace_period: bool,
		},
		/// Batch deposits claimed
		DepositsClaimed {
			multisig_address: T::AccountId,
			claimer: T::AccountId,
			total_returned: BalanceOf<T>,
			proposals_removed: u32,
			multisig_removed: bool,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Not enough signers provided
		NotEnoughSigners,
		/// Threshold must be greater than zero
		ThresholdZero,
		/// Threshold exceeds number of signers
		ThresholdTooHigh,
		/// Too many signers
		TooManySigners,
		/// Duplicate signer in list
		DuplicateSigner,
		/// Multisig already exists
		MultisigAlreadyExists,
		/// Multisig not found
		MultisigNotFound,
		/// Caller is not a signer of this multisig
		NotASigner,
		/// Proposal not found
		ProposalNotFound,
		/// Caller is not the proposer
		NotProposer,
		/// Already approved by this signer
		AlreadyApproved,
		/// Not enough approvals to execute
		NotEnoughApprovals,
		/// Proposal has expired
		ProposalExpired,
		/// Call data too large
		CallTooLarge,
		/// Failed to decode call data
		InvalidCall,
		/// Too many active proposals for this multisig
		TooManyActiveProposals,
		/// Insufficient balance for deposit
		InsufficientBalance,
		/// Proposal has active deposit
		ProposalHasDeposit,
		/// Proposal has not expired yet
		ProposalNotExpired,
		/// Grace period has not elapsed yet
		GracePeriodNotElapsed,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create a new multisig account
		///
		/// Parameters:
		/// - `signers`: List of accounts that can sign for this multisig
		/// - `threshold`: Number of approvals required to execute transactions
		///
		/// The multisig address is derived from a hash of all signers + global nonce.
		/// The creator must pay:
		/// - A fee (non-refundable, burned)
		/// - A deposit (refundable after grace period of inactivity)
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::create_multisig())]
		pub fn create_multisig(
			origin: OriginFor<T>,
			signers: Vec<T::AccountId>,
			threshold: u32,
		) -> DispatchResult {
			let creator = ensure_signed(origin)?;

			// Validate inputs
			ensure!(threshold > 0, Error::<T>::ThresholdZero);
			ensure!(!signers.is_empty(), Error::<T>::NotEnoughSigners);
			ensure!(threshold <= signers.len() as u32, Error::<T>::ThresholdTooHigh);
			ensure!(signers.len() <= T::MaxSigners::get() as usize, Error::<T>::TooManySigners);

			// Sort signers for deterministic address generation
			// (order shouldn't matter - nonce provides uniqueness)
			let mut sorted_signers = signers.clone();
			sorted_signers.sort();

			// Check for duplicate signers
			for i in 1..sorted_signers.len() {
				ensure!(sorted_signers[i] != sorted_signers[i - 1], Error::<T>::DuplicateSigner);
			}

			// Get and increment global nonce
			let nonce = GlobalNonce::<T>::get();
			GlobalNonce::<T>::put(nonce.saturating_add(1));

			// Generate multisig address from hash of (sorted_signers, nonce)
			let multisig_address = Self::derive_multisig_address(&sorted_signers, nonce);

			// Ensure multisig doesn't already exist
			ensure!(
				!Multisigs::<T>::contains_key(&multisig_address),
				Error::<T>::MultisigAlreadyExists
			);

			// Charge non-refundable fee (burned immediately)
			let fee = T::MultisigFee::get();
			let _ = T::Currency::withdraw(
				&creator,
				fee,
				frame_support::traits::WithdrawReasons::FEE,
				frame_support::traits::ExistenceRequirement::KeepAlive,
			)
			.map_err(|_| Error::<T>::InsufficientBalance)?;

			// Reserve deposit from creator (will be returned after grace period)
			let deposit = T::MultisigDeposit::get();
			T::Currency::reserve(&creator, deposit).map_err(|_| Error::<T>::InsufficientBalance)?;

			// Convert sorted signers to bounded vec
			let bounded_signers: BoundedSignersOf<T> =
				sorted_signers.try_into().map_err(|_| Error::<T>::TooManySigners)?;

			// Get current block for last_activity
			let current_block = frame_system::Pallet::<T>::block_number();

			// Store multisig data
			Multisigs::<T>::insert(
				&multisig_address,
				MultisigDataOf::<T> {
					signers: bounded_signers.clone(),
					threshold,
					nonce,
					deposit,
					creator: creator.clone(),
					last_activity: current_block,
					active_proposals: 0,
				},
			);

			// Emit event with sorted signers
			Self::deposit_event(Event::MultisigCreated {
				creator,
				multisig_address,
				signers: bounded_signers.to_vec(),
				threshold,
				nonce,
			});

			Ok(())
		}

		/// Propose a transaction to be executed by the multisig
		///
		/// Parameters:
		/// - `multisig_address`: The multisig account that will execute the call
		/// - `call`: The encoded call to execute
		/// - `expiry`: Block number when this proposal expires
		///
		/// The proposer must be a signer and must pay:
		/// - A deposit (returned on execute or cancel)
		/// - A fee (non-refundable, burned immediately)
		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::propose())]
		pub fn propose(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			call: Vec<u8>,
			expiry: BlockNumberFor<T>,
		) -> DispatchResult {
			let proposer = ensure_signed(origin)?;

			// Check if proposer is a signer and active proposals limit
			let multisig_data =
				Multisigs::<T>::get(&multisig_address).ok_or(Error::<T>::MultisigNotFound)?;
			ensure!(multisig_data.signers.contains(&proposer), Error::<T>::NotASigner);

			// Check active proposals limit
			ensure!(
				multisig_data.active_proposals < T::MaxActiveProposals::get(),
				Error::<T>::TooManyActiveProposals
			);

			// Check call size
			ensure!(call.len() as u32 <= T::MaxCallSize::get(), Error::<T>::CallTooLarge);

			// Charge non-refundable fee (burned immediately)
			let fee = T::ProposalFee::get();
			let _ = T::Currency::withdraw(
				&proposer,
				fee,
				frame_support::traits::WithdrawReasons::FEE,
				frame_support::traits::ExistenceRequirement::KeepAlive,
			)
			.map_err(|_| Error::<T>::InsufficientBalance)?;

			// Reserve deposit from proposer (will be returned)
			let deposit = T::ProposalDeposit::get();
			T::Currency::reserve(&proposer, deposit)
				.map_err(|_| Error::<T>::InsufficientBalance)?;

			// Update multisig last_activity
			Multisigs::<T>::mutate(&multisig_address, |maybe_multisig| {
				if let Some(multisig) = maybe_multisig {
					multisig.last_activity = frame_system::Pallet::<T>::block_number();
				}
			});

			// Convert to bounded vec
			let bounded_call: BoundedCallOf<T> =
				call.try_into().map_err(|_| Error::<T>::CallTooLarge)?;

			// Calculate proposal hash
			let proposal_hash = T::Hashing::hash_of(&bounded_call);

			// Check if proposal already exists
			ensure!(
				!Proposals::<T>::contains_key(&multisig_address, proposal_hash),
				Error::<T>::ProposalHasDeposit
			);

			// Create proposal with proposer as first approval
			let mut approvals = BoundedApprovalsOf::<T>::default();
			let _ = approvals.try_push(proposer.clone());

			let proposal = ProposalData {
				proposer: proposer.clone(),
				call: bounded_call,
				expiry,
				approvals,
				deposit,
			};

			// Store proposal
			Proposals::<T>::insert(&multisig_address, proposal_hash, proposal);

			// Increment active proposals counter
			Multisigs::<T>::mutate(&multisig_address, |maybe_multisig| {
				if let Some(multisig) = maybe_multisig {
					multisig.active_proposals = multisig.active_proposals.saturating_add(1);
				}
			});

			// Emit event
			Self::deposit_event(Event::TransactionProposed {
				multisig_address,
				proposer,
				proposal_hash,
			});

			Ok(())
		}

		/// Approve a proposed transaction
		///
		/// If this approval brings the total approvals to or above the threshold,
		/// the transaction will be automatically executed.
		///
		/// Parameters:
		/// - `multisig_address`: The multisig account
		/// - `proposal_hash`: Hash of the proposal to approve
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::approve())]
		pub fn approve(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			proposal_hash: T::Hash,
		) -> DispatchResult {
			let approver = ensure_signed(origin)?;

			// Check if approver is a signer
			let multisig_data =
				Multisigs::<T>::get(&multisig_address).ok_or(Error::<T>::MultisigNotFound)?;
			ensure!(multisig_data.signers.contains(&approver), Error::<T>::NotASigner);

			// Get proposal
			let mut proposal = Proposals::<T>::get(&multisig_address, proposal_hash)
				.ok_or(Error::<T>::ProposalNotFound)?;

			// Check if not expired
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(current_block <= proposal.expiry, Error::<T>::ProposalExpired);

			// Check if already approved
			ensure!(!proposal.approvals.contains(&approver), Error::<T>::AlreadyApproved);

			// Add approval
			proposal
				.approvals
				.try_push(approver.clone())
				.map_err(|_| Error::<T>::TooManySigners)?;

			let approvals_count = proposal.approvals.len() as u32;

			// Emit approval event
			Self::deposit_event(Event::TransactionApproved {
				multisig_address: multisig_address.clone(),
				approver,
				proposal_hash,
				approvals_count,
			});

			// Check if threshold is reached - if so, execute immediately
			if approvals_count >= multisig_data.threshold {
				// Execute the transaction
				Self::do_execute(multisig_address, proposal_hash, proposal)?;
			} else {
				// Not ready yet, just save the proposal
				Proposals::<T>::insert(&multisig_address, proposal_hash, proposal);

				// Update multisig last_activity
				Multisigs::<T>::mutate(&multisig_address, |maybe_multisig| {
					if let Some(multisig) = maybe_multisig {
						multisig.last_activity = frame_system::Pallet::<T>::block_number();
					}
				});
			}

			Ok(())
		}

		/// Cancel a proposed transaction (only by proposer)
		///
		/// Parameters:
		/// - `multisig_address`: The multisig account
		/// - `proposal_hash`: Hash of the proposal to cancel
		#[pallet::call_index(3)]
		#[pallet::weight(<T as Config>::WeightInfo::cancel())]
		pub fn cancel(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			proposal_hash: T::Hash,
		) -> DispatchResult {
			let canceller = ensure_signed(origin)?;

			// Get proposal
			let proposal = Proposals::<T>::get(&multisig_address, proposal_hash)
				.ok_or(Error::<T>::ProposalNotFound)?;

			// Check if caller is the proposer
			ensure!(canceller == proposal.proposer, Error::<T>::NotProposer);

			// Return deposit to proposer
			T::Currency::unreserve(&proposal.proposer, proposal.deposit);

			// Remove proposal
			Proposals::<T>::remove(&multisig_address, proposal_hash);

			// Decrement active proposals counter
			Multisigs::<T>::mutate(&multisig_address, |maybe_multisig| {
				if let Some(multisig) = maybe_multisig {
					multisig.active_proposals = multisig.active_proposals.saturating_sub(1);
				}
			});

			// Emit event
			Self::deposit_event(Event::TransactionCancelled {
				multisig_address,
				proposer: canceller,
				proposal_hash,
			});

			Ok(())
		}

		/// Remove an expired proposal and return deposit to proposer
		///
		/// Can be called by anyone after the proposal has expired.
		/// - Within grace period: only proposer can remove, deposit returned
		/// - After grace period: anyone can remove, deposit returned to proposer
		///
		/// This ensures storage cleanup while giving proposers time to act.
		#[pallet::call_index(4)]
		#[pallet::weight(<T as Config>::WeightInfo::remove_expired())]
		pub fn remove_expired(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			proposal_hash: T::Hash,
		) -> DispatchResult {
			let caller = ensure_signed(origin)?;

			// Get proposal
			let proposal = Proposals::<T>::get(&multisig_address, proposal_hash)
				.ok_or(Error::<T>::ProposalNotFound)?;

			// Check if expired
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(current_block > proposal.expiry, Error::<T>::ProposalNotExpired);

			// Calculate grace period end
			let grace_period_end = proposal.expiry.saturating_add(T::GracePeriod::get());
			let is_in_grace = current_block <= grace_period_end;
			let is_proposer = caller == proposal.proposer;

			// Within grace period: only proposer can remove
			if is_in_grace {
				ensure!(is_proposer, Error::<T>::NotProposer);
			}
			// After grace period: anyone can remove

			// Return deposit to proposer
			T::Currency::unreserve(&proposal.proposer, proposal.deposit);

			// Remove proposal from storage
			Proposals::<T>::remove(&multisig_address, proposal_hash);

			// Decrement active proposals counter
			Multisigs::<T>::mutate(&multisig_address, |maybe_multisig| {
				if let Some(multisig) = maybe_multisig {
					multisig.active_proposals = multisig.active_proposals.saturating_sub(1);
				}
			});

			// Emit event
			Self::deposit_event(Event::ProposalRemoved {
				multisig_address,
				proposal_hash,
				proposer: proposal.proposer.clone(),
				removed_by: caller,
				in_grace_period: is_in_grace,
			});

			Ok(())
		}

		/// Claim all deposits from cancelled and expired proposals, and inactive multisigs
		///
		/// This is a batch operation that:
		/// - Returns all proposal deposits where caller is proposer
		/// - Returns multisig deposit if caller is creator and grace period elapsed
		/// - Only works after grace period has elapsed
		/// - Removes all cancelled and expired proposals from storage
		/// - Removes multisig if inactive past grace period
		/// - Single transaction to clean up all user's old deposits
		///
		/// Use this after grace period to recover all your deposits at once.
		#[pallet::call_index(5)]
		#[pallet::weight(<T as Config>::WeightInfo::claim_deposits())]
		pub fn claim_deposits(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
		) -> DispatchResult {
			let caller = ensure_signed(origin)?;

			let current_block = frame_system::Pallet::<T>::block_number();
			let grace_period = T::GracePeriod::get();

			let mut total_returned = BalanceOf::<T>::zero();
			let mut removed_count = 0u32;

			// Iterate through all proposals for this multisig
			let proposals_to_remove: Vec<(T::Hash, ProposalDataOf<T>)> =
				Proposals::<T>::iter_prefix(&multisig_address)
					.filter(|(_, proposal)| {
						// Only proposals where caller is proposer
						if proposal.proposer != caller {
							return false;
						}

						// Calculate grace period end
						let grace_period_end = proposal.expiry.saturating_add(grace_period);

						// Only process if grace period has elapsed
						current_block > grace_period_end
					})
					.collect();

			// Remove proposals and return deposits
			for (hash, proposal) in proposals_to_remove {
				// Return deposit
				T::Currency::unreserve(&proposal.proposer, proposal.deposit);
				total_returned = total_returned.saturating_add(proposal.deposit);

				// Remove from storage
				Proposals::<T>::remove(&multisig_address, hash);
				removed_count = removed_count.saturating_add(1);

				// Decrement active proposals counter
				Multisigs::<T>::mutate(&multisig_address, |maybe_multisig| {
					if let Some(multisig) = maybe_multisig {
						multisig.active_proposals = multisig.active_proposals.saturating_sub(1);
					}
				});

				// Emit event for each removed proposal
				Self::deposit_event(Event::ProposalRemoved {
					multisig_address: multisig_address.clone(),
					proposal_hash: hash,
					proposer: caller.clone(),
					removed_by: caller.clone(),
					in_grace_period: false,
				});
			}

			// Check if multisig itself can be removed
			let mut multisig_removed = false;
			if let Some(multisig_data) = Multisigs::<T>::get(&multisig_address) {
				// Calculate grace period end for multisig
				let grace_period_end = multisig_data.last_activity.saturating_add(grace_period);

				// Check if grace period elapsed and no more proposals
				let has_proposals = Proposals::<T>::iter_prefix(&multisig_address).next().is_some();

				if current_block > grace_period_end && !has_proposals {
					// Check if caller is creator
					if caller == multisig_data.creator {
						// Return multisig deposit to creator
						T::Currency::unreserve(&multisig_data.creator, multisig_data.deposit);
						total_returned = total_returned.saturating_add(multisig_data.deposit);

						// Remove multisig from storage
						Multisigs::<T>::remove(&multisig_address);

						multisig_removed = true;
					}
				}
			}

			// Emit summary event
			Self::deposit_event(Event::DepositsClaimed {
				multisig_address: multisig_address.clone(),
				claimer: caller,
				total_returned,
				proposals_removed: removed_count,
				multisig_removed,
			});

			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// Derive a multisig address from signers and nonce
		pub fn derive_multisig_address(signers: &[T::AccountId], nonce: u64) -> T::AccountId {
			// Create a unique identifier from pallet id + signers + nonce.
			//
			// IMPORTANT:
			// - Do NOT `Decode` directly from a finite byte-slice and then "fallback" to a constant
			//   address on error: that can cause address collisions / DoS.
			// - Using `TrailingZeroInput` makes decoding deterministic and infallible by providing
			//   an infinite stream (hash bytes padded with zeros).
			let pallet_id = T::PalletId::get();
			let mut data = Vec::new();
			data.extend_from_slice(&pallet_id.0);
			data.extend_from_slice(&signers.encode());
			data.extend_from_slice(&nonce.encode());

			// Hash the data and map it deterministically into an AccountId.
			let hash = T::Hashing::hash(&data);
			T::AccountId::decode(&mut TrailingZeroInput::new(hash.as_ref()))
				.expect("TrailingZeroInput provides sufficient bytes; qed")
		}

		/// Check if an account is a signer for a given multisig
		pub fn is_signer(multisig_address: &T::AccountId, account: &T::AccountId) -> bool {
			if let Some(multisig_data) = Multisigs::<T>::get(multisig_address) {
				multisig_data.signers.contains(account)
			} else {
				false
			}
		}

		/// Get a single executed proposal from archive
		/// Returns None if proposal was never executed or was cancelled/expired
		pub fn get_executed_proposal(
			multisig_address: &T::AccountId,
			proposal_hash: &T::Hash,
		) -> Option<ExecutedProposalDataOf<T>> {
			ExecutedProposals::<T>::get(multisig_address, proposal_hash)
		}

		/// Get executed proposals for a multisig with pagination
		///
		/// # Arguments
		/// * `multisig_address` - The multisig account to query
		/// * `start_after` - Optional hash to start after (for pagination)
		/// * `limit` - Maximum number of results to return (capped at MaxExecutedProposalsQuery)
		///
		/// # Returns
		/// * `Vec<(T::Hash, ExecutedProposalDataOf<T>)>` - List of (proposal_hash, data) pairs
		/// * `Option<T::Hash>` - Next cursor for pagination (Some if more results exist, None if
		///   end)
		///
		/// # Example
		/// ```ignore
		/// // First page
		/// let (proposals, next_cursor) = Multisig::get_executed_proposals_paginated(&multisig, None, 100);
		///
		/// // Next page (if next_cursor is Some)
		/// if let Some(cursor) = next_cursor {
		///     let (more_proposals, next_cursor) = Multisig::get_executed_proposals_paginated(&multisig, Some(cursor), 100);
		/// }
		/// ```
		pub fn get_executed_proposals_paginated(
			multisig_address: &T::AccountId,
			start_after: Option<T::Hash>,
			limit: u32,
		) -> PaginatedProposalsOf<T> {
			// Cap limit at configured maximum
			let max_limit = T::MaxExecutedProposalsQuery::get().min(limit);

			let iter = ExecutedProposals::<T>::iter_prefix(multisig_address);

			let mut results = Vec::new();
			let mut next_cursor = None;

			// If start_after is provided, we need to skip until we find it
			let mut found_start = start_after.is_none(); // If no start_after, we're already "found"

			for (hash, data) in iter {
				// Skip until we pass start_after
				if !found_start {
					if Some(&hash) == start_after.as_ref() {
						found_start = true; // Mark as found
					}
					continue; // Skip this element (including start_after itself)
				}

				// Now we're past start_after (or there was no start_after)
				// Collect results up to max_limit
				if results.len() < max_limit as usize {
					results.push((hash, data));
				} else {
					// We have one more result beyond the limit, so there's a next page
					// Use the last element we collected as the cursor for next page
					next_cursor = Some(results.last().unwrap().0);
					break;
				}
			}

			(results, next_cursor)
		}

		/// Internal function to execute a proposal
		/// Called automatically from `approve()` when threshold is reached
		///
		/// This function is private and cannot be called from outside the pallet
		fn do_execute(
			multisig_address: T::AccountId,
			proposal_hash: T::Hash,
			proposal: ProposalDataOf<T>,
		) -> DispatchResult {
			// Decode the call before modifying storage
			let call = <T as Config>::RuntimeCall::decode(&mut &proposal.call[..])
				.map_err(|_| Error::<T>::InvalidCall)?;

			// Return deposit to proposer
			T::Currency::unreserve(&proposal.proposer, proposal.deposit);

			// Execute the call as the multisig account
			let result =
				call.dispatch(frame_system::RawOrigin::Signed(multisig_address.clone()).into());

			let execution_succeeded = result.is_ok();

			// Archive the executed proposal (for successful executions only in terms of storage,
			// but we store all executed proposals with their result)
			let executed_proposal = ExecutedProposalDataOf::<T> {
				proposer: proposal.proposer.clone(),
				call: proposal.call.clone(),
				approvers: proposal.approvals.clone(),
				executed_at: frame_system::Pallet::<T>::block_number(),
				execution_succeeded,
			};
			ExecutedProposals::<T>::insert(&multisig_address, proposal_hash, executed_proposal);

			// Remove proposal from active storage
			Proposals::<T>::remove(&multisig_address, proposal_hash);

			// Update multisig: decrement counter and update last_activity
			Multisigs::<T>::mutate(&multisig_address, |maybe_multisig| {
				if let Some(multisig) = maybe_multisig {
					multisig.last_activity = frame_system::Pallet::<T>::block_number();
					multisig.active_proposals = multisig.active_proposals.saturating_sub(1);
				}
			});

			// Emit event with execution result
			Self::deposit_event(Event::TransactionExecuted {
				multisig_address,
				proposal_hash,
				result: result.map(|_| ()).map_err(|e| e.error),
			});

			Ok(())
		}
	}
}
