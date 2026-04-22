//! # Quantus Multisig Pallet
//!
//! This pallet provides multisignature functionality for managing shared accounts
//! that require multiple approvals before executing transactions.
//!
//! ## Features
//!
//! - Create multisig addresses with deterministic generation (signers + threshold + user-provided
//!   nonce)
//! - Propose transactions for multisig approval
//! - Approve proposed transactions
//! - Execute transactions once threshold is reached (automatic)
//! - Cleanup of expired proposals via claim_deposits() and remove_expired()
//! - Per-signer proposal limits for filibuster protection
//!
//! ## Design Notes
//!
//! Multisigs are permanent once created. There is no dissolution mechanism by design:
//! - Avoids complexity around native/non-native asset handling during dissolution
//! - Prevents griefing attacks (e.g., sending dust to block dissolution)
//! - The multisig deposit acts as storage rent rather than a refundable deposit
//! - Users who want to "close" a multisig simply stop using it
//!
//! ## Data Structures
//!
//! - **MultisigData**: Contains signers, threshold, proposal counter, deposit, and per-signer
//!   tracking
//! - **ProposalData**: Contains transaction data, proposer, expiry, approvals, deposit, and status

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
use frame_support::{traits::Get, BoundedBTreeMap, BoundedVec};
use scale_info::TypeInfo;
use sp_runtime::RuntimeDebug;

/// Multisig account data
#[derive(Encode, Decode, MaxEncodedLen, Clone, TypeInfo, RuntimeDebug, PartialEq, Eq)]
pub struct MultisigData<AccountId, BoundedSigners, Balance, BoundedProposalsPerSigner> {
	/// Account that created this multisig
	pub creator: AccountId,
	/// List of signers who can approve transactions
	pub signers: BoundedSigners,
	/// Number of approvals required to execute a transaction
	pub threshold: u32,
	/// Proposal counter for unique proposal IDs
	pub proposal_nonce: u32,
	/// Deposit reserved by the creator (for storage rent)
	pub deposit: Balance,
	/// Number of active proposals (for global limit checking)
	pub active_proposals: u32,
	/// Per-signer proposal count (for filibuster protection)
	/// Maps AccountId -> number of active proposals
	pub proposals_per_signer: BoundedProposalsPerSigner,
}

impl<
		AccountId: Default,
		BoundedSigners: Default,
		Balance: Default,
		BoundedProposalsPerSigner: Default,
	> Default for MultisigData<AccountId, BoundedSigners, Balance, BoundedProposalsPerSigner>
{
	fn default() -> Self {
		Self {
			creator: Default::default(),
			signers: Default::default(),
			threshold: 1,
			proposal_nonce: 0,
			deposit: Default::default(),
			active_proposals: 0,
			proposals_per_signer: Default::default(),
		}
	}
}

/// Proposal status
#[derive(Encode, Decode, MaxEncodedLen, Clone, TypeInfo, RuntimeDebug, PartialEq, Eq)]
pub enum ProposalStatus {
	/// Proposal is active and awaiting approvals
	Active,
	/// Proposal has reached threshold and is ready to execute
	Approved,
	/// Proposal was executed successfully
	Executed,
	/// Proposal was cancelled by proposer
	Cancelled,
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
	/// Deposit held for this proposal (returned only when proposal is removed)
	pub deposit: Balance,
	/// Current status of the proposal
	pub status: ProposalStatus,
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
		dispatch::{
			DispatchErrorWithPostInfo, DispatchResult, DispatchResultWithPostInfo, GetDispatchInfo,
			Pays, PostDispatchInfo,
		},
		pallet_prelude::*,
		traits::{Currency, ReservableCurrency},
		weights::Weight,
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use qp_high_security::HighSecurityInspector;
	use sp_arithmetic::traits::Saturating;
	use sp_runtime::{
		traits::{Dispatchable, Hash, TrailingZeroInput},
		Permill,
	};

	/// The in-code storage version.
	///
	/// This establishes an explicit baseline for future storage migrations.
	/// Increment this and add a migration hook when storage layout changes.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
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

		/// Maximum total number of proposals in storage per multisig (Active + Executed +
		/// Cancelled) This prevents unbounded storage growth and incentivizes cleanup
		#[pallet::constant]
		type MaxTotalProposalsInStorage: Get<u32>;

		/// Maximum size of an encoded call
		#[pallet::constant]
		type MaxCallSize: Get<u32>;

		/// Fee charged for creating a multisig (non-refundable, burned)
		#[pallet::constant]
		type MultisigFee: Get<BalanceOf<Self>>;

		/// Deposit reserved for creating a multisig (storage rent, non-refundable).
		/// This prevents spam creation of multisig accounts.
		#[pallet::constant]
		type MultisigDeposit: Get<BalanceOf<Self>>;

		/// Deposit required per proposal (returned on execute or cancel)
		#[pallet::constant]
		type ProposalDeposit: Get<BalanceOf<Self>>;

		/// Fee charged for creating a proposal (non-refundable, paid always)
		#[pallet::constant]
		type ProposalFee: Get<BalanceOf<Self>>;

		/// Percentage increase in ProposalFee for each signer in the multisig.
		///
		/// Formula: `FinalFee = ProposalFee + (ProposalFee * SignerCount * SignerStepFactor)`
		/// Example: If Fee=100, Signers=5, Factor=1%, then Extra = 100 * 5 * 0.01 = 5. Total = 105.
		#[pallet::constant]
		type SignerStepFactor: Get<Permill>;

		/// Pallet ID for generating multisig addresses
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		/// Maximum duration (in blocks) that a proposal can be set to expire in the future.
		/// This prevents proposals from being created with extremely far expiry dates
		/// that would lock deposits and bloat storage for extended periods.
		///
		/// Example: If set to 100_000 blocks (~2 weeks at 12s blocks),
		/// a proposal created at block 1000 cannot have expiry > 101_000.
		#[pallet::constant]
		type MaxExpiryDuration: Get<BlockNumberFor<Self>>;

		/// Weight information for extrinsics
		type WeightInfo: WeightInfo;

		/// Interface to check if an account is in high-security mode
		type HighSecurity: qp_high_security::HighSecurityInspector<
			Self::AccountId,
			<Self as pallet::Config>::RuntimeCall,
		>;
	}

	/// Type alias for bounded signers vector
	pub type BoundedSignersOf<T> =
		BoundedVec<<T as frame_system::Config>::AccountId, <T as Config>::MaxSigners>;

	/// Type alias for bounded approvals vector
	pub type BoundedApprovalsOf<T> =
		BoundedVec<<T as frame_system::Config>::AccountId, <T as Config>::MaxSigners>;

	/// Type alias for bounded call data
	pub type BoundedCallOf<T> = BoundedVec<u8, <T as Config>::MaxCallSize>;

	/// Type alias for per-signer proposal counts
	pub type BoundedProposalsPerSignerOf<T> =
		BoundedBTreeMap<<T as frame_system::Config>::AccountId, u32, <T as Config>::MaxSigners>;

	/// Type alias for MultisigData with proper bounds
	pub type MultisigDataOf<T> = MultisigData<
		<T as frame_system::Config>::AccountId,
		BoundedSignersOf<T>,
		BalanceOf<T>,
		BoundedProposalsPerSignerOf<T>,
	>;

	/// Type alias for ProposalData with proper bounds
	pub type ProposalDataOf<T> = ProposalData<
		<T as frame_system::Config>::AccountId,
		BalanceOf<T>,
		BlockNumberFor<T>,
		BoundedCallOf<T>,
		BoundedApprovalsOf<T>,
	>;

	/// Multisigs stored by their deterministic address
	#[pallet::storage]
	#[pallet::getter(fn multisigs)]
	pub type Multisigs<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, MultisigDataOf<T>, OptionQuery>;

	/// Proposals indexed by (multisig_address, proposal_nonce)
	#[pallet::storage]
	#[pallet::getter(fn proposals)]
	pub type Proposals<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Twox64Concat,
		u32,
		ProposalDataOf<T>,
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
		/// A proposal has been created
		ProposalCreated { multisig_address: T::AccountId, proposer: T::AccountId, proposal_id: u32 },
		/// A proposal has been approved by a signer
		ProposalApproved {
			multisig_address: T::AccountId,
			approver: T::AccountId,
			proposal_id: u32,
			approvals_count: u32,
		},
		/// A proposal has reached threshold and is ready to execute
		ProposalReadyToExecute {
			multisig_address: T::AccountId,
			proposal_id: u32,
			approvals_count: u32,
		},
		/// A proposal has been executed
		/// Contains all data needed for indexing by SubSquid
		ProposalExecuted {
			multisig_address: T::AccountId,
			proposal_id: u32,
			proposer: T::AccountId,
			call: Vec<u8>,
			approvers: Vec<T::AccountId>,
			result: DispatchResult,
		},
		/// A proposal has been cancelled by the proposer
		ProposalCancelled {
			multisig_address: T::AccountId,
			proposer: T::AccountId,
			proposal_id: u32,
		},
		/// Expired proposal was removed from storage
		ProposalRemoved {
			multisig_address: T::AccountId,
			proposal_id: u32,
			proposer: T::AccountId,
			removed_by: T::AccountId,
		},
		/// Batch deposits claimed
		DepositsClaimed {
			multisig_address: T::AccountId,
			claimer: T::AccountId,
			total_returned: BalanceOf<T>,
			proposals_removed: u32,
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
		/// Proposal expiry is in the past
		ExpiryInPast,
		/// Proposal expiry is too far in the future (exceeds MaxExpiryDuration)
		ExpiryTooFar,
		/// Proposal has expired
		ProposalExpired,
		/// Call data too large
		CallTooLarge,
		/// Failed to decode call data
		InvalidCall,
		/// Too many total proposals in storage for this multisig (cleanup required)
		TooManyProposalsInStorage,
		/// This signer has too many proposals in storage (filibuster protection)
		TooManyProposalsPerSigner,
		/// Insufficient balance for deposit
		InsufficientBalance,
		/// Proposal has active deposit
		ProposalHasDeposit,
		/// Proposal has not expired yet
		ProposalNotExpired,
		/// Proposal is not active (already executed or cancelled)
		ProposalNotActive,
		/// Proposal has not been approved yet (threshold not reached)
		ProposalNotApproved,
		/// Call is not allowed for high-security multisig
		CallNotAllowedForHighSecurityMultisig,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create a new multisig account with deterministic address
		///
		/// Parameters:
		/// - `signers`: List of accounts that can sign for this multisig
		/// - `threshold`: Number of approvals required to execute transactions
		/// - `nonce`: User-provided nonce for address uniqueness
		///
		/// The multisig address is deterministically derived from:
		/// hash(pallet_id || sorted_signers || threshold || nonce)
		///
		/// Signers are automatically sorted before hashing, so order doesn't matter.
		///
		/// Economic costs:
		/// - MultisigFee: burned immediately (spam prevention)
		/// - MultisigDeposit: reserved until dissolution, then returned to creator (storage bond)
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::create_multisig(signers.len() as u32))]
		pub fn create_multisig(
			origin: OriginFor<T>,
			signers: Vec<T::AccountId>,
			threshold: u32,
			nonce: u64,
		) -> DispatchResult {
			let creator = ensure_signed(origin)?;

			// Validate inputs
			ensure!(threshold > 0, Error::<T>::ThresholdZero);
			ensure!(!signers.is_empty(), Error::<T>::NotEnoughSigners);
			ensure!(threshold <= signers.len() as u32, Error::<T>::ThresholdTooHigh);
			ensure!(signers.len() <= T::MaxSigners::get() as usize, Error::<T>::TooManySigners);

			// Sort signers for duplicate check and storage
			let mut sorted_signers = signers.clone();
			sorted_signers.sort();

			// Check for duplicate signers
			for i in 1..sorted_signers.len() {
				ensure!(sorted_signers[i] != sorted_signers[i - 1], Error::<T>::DuplicateSigner);
			}

			// Generate deterministic multisig address
			// Note: derive_multisig_address() will sort internally, but we already have sorted
			// for duplicate check, so we pass sorted to avoid double sorting
			let multisig_address = Self::derive_multisig_address(&sorted_signers, threshold, nonce);

			// Ensure multisig doesn't already exist
			ensure!(
				!Multisigs::<T>::contains_key(&multisig_address),
				Error::<T>::MultisigAlreadyExists
			);

			// Charge non-refundable fee (burned)
			let fee = T::MultisigFee::get();
			let _ = T::Currency::withdraw(
				&creator,
				fee,
				frame_support::traits::WithdrawReasons::FEE,
				frame_support::traits::ExistenceRequirement::KeepAlive,
			)
			.map_err(|_| Error::<T>::InsufficientBalance)?;

			// Reserve deposit from creator (will be returned on dissolve)
			let deposit = T::MultisigDeposit::get();
			T::Currency::reserve(&creator, deposit).map_err(|_| Error::<T>::InsufficientBalance)?;

			// Convert sorted signers to bounded vec
			let bounded_signers: BoundedSignersOf<T> =
				sorted_signers.try_into().map_err(|_| Error::<T>::TooManySigners)?;

			// Store multisig data
			Multisigs::<T>::insert(
				&multisig_address,
				MultisigDataOf::<T> {
					creator: creator.clone(),
					signers: bounded_signers.clone(),
					threshold,
					proposal_nonce: 0,
					deposit,
					active_proposals: 0,
					proposals_per_signer: BoundedProposalsPerSignerOf::<T>::default(),
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
		/// - A deposit (refundable - returned immediately on execution/cancellation)
		/// - A fee (non-refundable, burned immediately)
		///
		/// **For threshold=1:** The proposal is created with `Approved` status immediately
		/// and can be executed via `execute()` without additional approvals.
		///
		/// **Weight:** Charged upfront for worst-case (high-security path with decode).
		/// Refunded to actual cost on success based on whether HS path was taken.
		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::propose_high_security(call.len() as u32))]
		#[allow(clippy::useless_conversion)]
		pub fn propose(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			call: Vec<u8>,
			expiry: BlockNumberFor<T>,
		) -> DispatchResultWithPostInfo {
			let proposer = ensure_signed(origin)?;

			// CRITICAL: Check call size FIRST, before any heavy operations (especially decode)
			// This prevents DoS via oversized payloads that would be decoded before size validation
			let call_size = call.len() as u32;
			if call_size > T::MaxCallSize::get() {
				return Self::err_with_weight(Error::<T>::CallTooLarge, 0);
			}

			// Check if proposer is a signer (1 read: Multisigs)
			let multisig_data = Multisigs::<T>::get(&multisig_address).ok_or_else(|| {
				DispatchErrorWithPostInfo {
					post_info: PostDispatchInfo {
						actual_weight: Some(T::DbWeight::get().reads(1)),
						pays_fee: Pays::Yes,
					},
					error: Error::<T>::MultisigNotFound.into(),
				}
			})?;
			if !multisig_data.signers.contains(&proposer) {
				return Self::err_with_weight(Error::<T>::NotASigner, 1);
			}

			// High-security check: if multisig is high-security, only whitelisted calls allowed
			// Size already validated above, so decode is now safe
			// (2 reads: Multisigs + HighSecurityAccounts)
			let is_high_security = T::HighSecurity::is_high_security(&multisig_address);
			if is_high_security {
				let decoded_call =
					<T as Config>::RuntimeCall::decode(&mut &call[..]).map_err(|_| {
						DispatchErrorWithPostInfo {
							post_info: PostDispatchInfo {
								actual_weight: Some(T::DbWeight::get().reads(2)),
								pays_fee: Pays::Yes,
							},
							error: Error::<T>::InvalidCall.into(),
						}
					})?;
				if !T::HighSecurity::is_whitelisted(&decoded_call) {
					return Self::err_with_weight(
						Error::<T>::CallNotAllowedForHighSecurityMultisig,
						2,
					);
				}
			}

			let current_block = frame_system::Pallet::<T>::block_number();

			// Get signers count (used for multiple checks below)
			let signers_count = multisig_data.signers.len() as u32;

			ensure!(
				multisig_data.active_proposals < T::MaxTotalProposalsInStorage::get(),
				Error::<T>::TooManyProposalsInStorage
			);

			// Check per-signer proposal limit (filibuster protection)
			// Each signer can have max (TotalLimit / SignersCount) proposals
			let max_proposals_per_signer =
				T::MaxTotalProposalsInStorage::get().saturating_div(signers_count);
			let proposer_current_count =
				multisig_data.proposals_per_signer.get(&proposer).copied().unwrap_or(0);
			ensure!(
				proposer_current_count < max_proposals_per_signer,
				Error::<T>::TooManyProposalsPerSigner
			);

			// Validate expiry is in the future
			ensure!(expiry > current_block, Error::<T>::ExpiryInPast);

			// Validate expiry is not too far in the future
			let max_expiry = current_block.saturating_add(T::MaxExpiryDuration::get());
			ensure!(expiry <= max_expiry, Error::<T>::ExpiryTooFar);

			// Calculate dynamic fee based on number of signers
			// Fee = Base + floor(StepFactor * Base * SignerCount)
			let base_fee = T::ProposalFee::get();
			let step_factor = T::SignerStepFactor::get();

			// Multiply base by signer count first, then apply step factor percentage.
			// This avoids early floor truncation that would zero out small percentages.
			// Example: base=99, factor=1%, signers=100 -> floor(1% * 9900) = 99
			let multiplier = base_fee.saturating_mul(signers_count.into());
			let total_increase = step_factor.mul_floor(multiplier);
			let fee = base_fee.saturating_add(total_increase);

			// Charge non-refundable fee (burned)
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

			// Convert to bounded vec (call_size already computed and validated above)
			let bounded_call: BoundedCallOf<T> =
				call.try_into().map_err(|_| Error::<T>::CallTooLarge)?;

			let threshold_met = 1 >= multisig_data.threshold;

			let proposal_id = Multisigs::<T>::try_mutate(
				&multisig_address,
				|maybe_multisig| -> Result<u32, DispatchError> {
					let multisig = maybe_multisig.as_mut().ok_or(Error::<T>::MultisigNotFound)?;
					let nonce = multisig.proposal_nonce;
					multisig.proposal_nonce = nonce.saturating_add(1);
					multisig.active_proposals = multisig.active_proposals.saturating_add(1);
					let count = multisig.proposals_per_signer.get(&proposer).copied().unwrap_or(0);
					multisig
						.proposals_per_signer
						.try_insert(proposer.clone(), count.saturating_add(1))
						.map_err(|_| Error::<T>::TooManySigners)?;
					Ok(nonce)
				},
			)?;

			let mut approvals = BoundedApprovalsOf::<T>::default();
			let _ = approvals.try_push(proposer.clone());

			Proposals::<T>::insert(
				&multisig_address,
				proposal_id,
				ProposalData {
					proposer: proposer.clone(),
					call: bounded_call,
					expiry,
					approvals,
					deposit,
					status: if threshold_met {
						ProposalStatus::Approved
					} else {
						ProposalStatus::Active
					},
				},
			);

			Self::deposit_event(Event::ProposalCreated {
				multisig_address: multisig_address.clone(),
				proposer,
				proposal_id,
			});

			if threshold_met {
				Self::deposit_event(Event::ProposalReadyToExecute {
					multisig_address: multisig_address.clone(),
					proposal_id,
					approvals_count: 1,
				});
			}

			// Refund weight: HS path was charged upfront, refund if non-HS
			let actual_weight = if is_high_security {
				<T as Config>::WeightInfo::propose_high_security(call_size)
			} else {
				<T as Config>::WeightInfo::propose(call_size)
			};

			Ok(PostDispatchInfo { actual_weight: Some(actual_weight), pays_fee: Pays::Yes })
		}

		/// Approve a proposed transaction
		///
		/// If this approval brings the total approvals to or above the threshold,
		/// the proposal status changes to `Approved` and can be executed via `execute()`.
		///
		/// Parameters:
		/// - `multisig_address`: The multisig account
		/// - `proposal_id`: ID (nonce) of the proposal to approve
		///
		/// Weight: Charges for MAX call size, refunds based on actual
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::approve(T::MaxCallSize::get()))]
		#[allow(clippy::useless_conversion)]
		pub fn approve(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			proposal_id: u32,
		) -> DispatchResultWithPostInfo {
			let approver = ensure_signed(origin)?;

			// Check if approver is a signer (1 read: Multisigs)
			let multisig_data = Multisigs::<T>::get(&multisig_address).ok_or_else(|| {
				DispatchErrorWithPostInfo {
					post_info: PostDispatchInfo {
						actual_weight: Some(T::DbWeight::get().reads(1)),
						pays_fee: Pays::Yes,
					},
					error: Error::<T>::MultisigNotFound.into(),
				}
			})?;
			if !multisig_data.signers.contains(&approver) {
				return Self::err_with_weight(Error::<T>::NotASigner, 1);
			}

			// Get proposal (2 reads: Multisigs + Proposals)
			let mut proposal =
				Proposals::<T>::get(&multisig_address, proposal_id).ok_or_else(|| {
					DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo {
							actual_weight: Some(T::DbWeight::get().reads(2)),
							pays_fee: Pays::Yes,
						},
						error: Error::<T>::ProposalNotFound.into(),
					}
				})?;

			// Calculate actual weight based on real call size
			let actual_call_size = proposal.call.len() as u32;
			let actual_weight = <T as Config>::WeightInfo::approve(actual_call_size);

			let current_block = frame_system::Pallet::<T>::block_number();
			if current_block > proposal.expiry {
				return Self::err_with_weight(Error::<T>::ProposalExpired, 2);
			}

			if proposal.approvals.contains(&approver) {
				return Self::err_with_weight(Error::<T>::AlreadyApproved, 2);
			}

			// Add approval
			proposal
				.approvals
				.try_push(approver.clone())
				.map_err(|_| Error::<T>::TooManySigners)?;

			let approvals_count = proposal.approvals.len() as u32;

			// Check if threshold is reached - if so, mark as Approved
			if approvals_count >= multisig_data.threshold {
				proposal.status = ProposalStatus::Approved;
			}

			// Save proposal
			Proposals::<T>::insert(&multisig_address, proposal_id, &proposal);

			// Emit approval event
			Self::deposit_event(Event::ProposalApproved {
				multisig_address: multisig_address.clone(),
				approver,
				proposal_id,
				approvals_count,
			});

			// Emit ready-to-execute event if threshold just reached
			if proposal.status == ProposalStatus::Approved {
				Self::deposit_event(Event::ProposalReadyToExecute {
					multisig_address,
					proposal_id,
					approvals_count,
				});
			}

			// Return actual weight (refund overpayment)
			Ok(PostDispatchInfo { actual_weight: Some(actual_weight), pays_fee: Pays::Yes })
		}

		/// Cancel a proposed transaction (only by proposer)
		///
		/// Parameters:
		/// - `multisig_address`: The multisig account
		/// - `proposal_id`: ID (nonce) of the proposal to cancel
		#[pallet::call_index(3)]
		#[pallet::weight(<T as Config>::WeightInfo::cancel(T::MaxCallSize::get()))]
		#[allow(clippy::useless_conversion)]
		pub fn cancel(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			proposal_id: u32,
		) -> DispatchResultWithPostInfo {
			let canceller = ensure_signed(origin)?;

			// Get proposal (1 read: Proposals)
			let proposal =
				Proposals::<T>::get(&multisig_address, proposal_id).ok_or_else(|| {
					DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo {
							actual_weight: Some(T::DbWeight::get().reads(1)),
							pays_fee: Pays::Yes,
						},
						error: Error::<T>::ProposalNotFound.into(),
					}
				})?;

			// Check if caller is the proposer (1 read already performed)
			if canceller != proposal.proposer {
				return Self::err_with_weight(Error::<T>::NotProposer, 1);
			}

			// Check if proposal is cancellable (Active or Approved)
			if proposal.status != ProposalStatus::Active &&
				proposal.status != ProposalStatus::Approved
			{
				return Self::err_with_weight(Error::<T>::ProposalNotActive, 1);
			}

			let call_size = proposal.call.len() as u32;

			// Remove proposal from storage and return deposit immediately
			Self::remove_proposal_and_return_deposit(
				&multisig_address,
				proposal_id,
				&proposal.proposer,
				proposal.deposit,
			);

			// Emit event
			Self::deposit_event(Event::ProposalCancelled {
				multisig_address,
				proposer: canceller,
				proposal_id,
			});

			let actual_weight = <T as Config>::WeightInfo::cancel(call_size);
			Ok(PostDispatchInfo { actual_weight: Some(actual_weight), pays_fee: Pays::Yes })
		}

		/// Remove expired proposals and return deposits to proposers
		///
		/// Can only be called by signers of the multisig.
		/// Removes Active or Approved proposals that have expired (past expiry block).
		/// Executed and Cancelled proposals are automatically cleaned up immediately.
		///
		/// Approved+expired proposals can become stuck if proposer is unavailable (e.g. lost
		/// keys, compromise). Allowing any signer to remove them prevents permanent deposit
		/// lockup and enables multisig dissolution.
		///
		/// The deposit is always returned to the original proposer, not the caller.
		#[pallet::call_index(4)]
		#[pallet::weight(<T as Config>::WeightInfo::remove_expired(T::MaxCallSize::get()))]
		pub fn remove_expired(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			proposal_id: u32,
		) -> DispatchResultWithPostInfo {
			let caller = ensure_signed(origin)?;

			// Verify caller is a signer (1 read: Multisigs)
			let multisig_data = Multisigs::<T>::get(&multisig_address).ok_or_else(|| {
				DispatchErrorWithPostInfo {
					post_info: PostDispatchInfo {
						actual_weight: Some(T::DbWeight::get().reads(1)),
						pays_fee: Pays::Yes,
					},
					error: Error::<T>::MultisigNotFound.into(),
				}
			})?;
			if !multisig_data.signers.contains(&caller) {
				return Self::err_with_weight(Error::<T>::NotASigner, 1);
			}

			// Get proposal (2 reads: Multisigs + Proposals)
			let proposal =
				Proposals::<T>::get(&multisig_address, proposal_id).ok_or_else(|| {
					DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo {
							actual_weight: Some(T::DbWeight::get().reads(2)),
							pays_fee: Pays::Yes,
						},
						error: Error::<T>::ProposalNotFound.into(),
					}
				})?;

			// Active or Approved proposals can be removed when expired (Executed/Cancelled
			// are auto-removed). Approved+expired would otherwise be stuck if proposer
			// unavailable.
			if proposal.status != ProposalStatus::Active &&
				proposal.status != ProposalStatus::Approved
			{
				return Self::err_with_weight(Error::<T>::ProposalNotActive, 2);
			}

			// Check if expired
			let current_block = frame_system::Pallet::<T>::block_number();
			if current_block <= proposal.expiry {
				return Self::err_with_weight(Error::<T>::ProposalNotExpired, 2);
			}

			// Remove proposal from storage and return deposit
			Self::remove_proposal_and_return_deposit(
				&multisig_address,
				proposal_id,
				&proposal.proposer,
				proposal.deposit,
			);

			// Emit event
			Self::deposit_event(Event::ProposalRemoved {
				multisig_address,
				proposal_id,
				proposer: proposal.proposer.clone(),
				removed_by: caller,
			});

			// Return actual weight based on proposal call size
			let actual_weight =
				<T as Config>::WeightInfo::remove_expired(proposal.call.len() as u32);
			Ok(PostDispatchInfo { actual_weight: Some(actual_weight), pays_fee: Pays::Yes })
		}

		/// Claim all deposits from expired proposals
		///
		/// This is a batch operation that removes all expired proposals where:
		/// - Caller is the proposer
		/// - Proposal is Active or Approved and past expiry block
		///
		/// Note: Executed and Cancelled proposals are automatically cleaned up immediately,
		/// so only Active+Expired and Approved+Expired proposals need manual cleanup.
		///
		/// Returns all proposal deposits to the proposer in a single transaction.
		#[pallet::call_index(5)]
		#[pallet::weight(<T as Config>::WeightInfo::claim_deposits(
		T::MaxTotalProposalsInStorage::get(),  // Worst-case iterated
		T::MaxTotalProposalsInStorage::get(),  // Worst-case cleaned
		T::MaxCallSize::get()  // Worst-case avg call size
	))]
		#[allow(clippy::useless_conversion)]
		pub fn claim_deposits(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
		) -> DispatchResultWithPostInfo {
			let caller = ensure_signed(origin)?;

			// Verify caller is a signer (1 read: Multisigs)
			let multisig_data = Multisigs::<T>::get(&multisig_address).ok_or_else(|| {
				DispatchErrorWithPostInfo {
					post_info: PostDispatchInfo {
						actual_weight: Some(T::DbWeight::get().reads(1)),
						pays_fee: Pays::Yes,
					},
					error: Error::<T>::MultisigNotFound.into(),
				}
			})?;
			if !multisig_data.signers.contains(&caller) {
				return Self::err_with_weight(Error::<T>::NotASigner, 1);
			}

			let (cleaned, total_proposals_iterated, total_call_bytes, total_returned) =
				Self::cleanup_proposer_expired(&multisig_address, &caller, &caller);

			// Emit summary event (total_returned is the actual sum of stored deposits unreserved)
			Self::deposit_event(Event::DepositsClaimed {
				multisig_address: multisig_address.clone(),
				claimer: caller,
				total_returned,
				proposals_removed: cleaned,
			});

			// Average call size over iterated proposals (for weight)
			let avg_call_size = if total_proposals_iterated > 0 {
				total_call_bytes / total_proposals_iterated
			} else {
				0
			};

			let actual_weight = <T as Config>::WeightInfo::claim_deposits(
				total_proposals_iterated,
				cleaned,
				avg_call_size,
			);
			Ok(PostDispatchInfo { actual_weight: Some(actual_weight), pays_fee: Pays::Yes })
		}

		/// Execute an approved proposal
		///
		/// Can be called by any signer of the multisig once the proposal has reached
		/// the approval threshold (status = Approved). The proposal must not be expired.
		///
		/// On execution:
		/// - The call is decoded and dispatched as the multisig account
		/// - Proposal is removed from storage
		/// - Deposit is returned to the proposer
		///
		/// Parameters:
		/// - `multisig_address`: The multisig account
		/// - `proposal_id`: ID (nonce) of the proposal to execute
		///
		/// Note: The weight charged includes both multisig bookkeeping and the inner call's
		/// declared weight. Actual weight is refunded based on post-dispatch info.
		#[pallet::call_index(7)]
		#[pallet::weight({
			// Worst case: max bookkeeping + max possible call weight (from benchmarks)
			// The actual weight will be refunded based on the real call's weight
			<T as Config>::WeightInfo::execute(T::MaxCallSize::get())
		})]
		#[allow(clippy::useless_conversion)]
		pub fn execute(
			origin: OriginFor<T>,
			multisig_address: T::AccountId,
			proposal_id: u32,
		) -> DispatchResultWithPostInfo {
			let executor = ensure_signed(origin)?;

			// Check if executor is a signer (1 read: Multisigs)
			let multisig_data = Multisigs::<T>::get(&multisig_address).ok_or_else(|| {
				DispatchErrorWithPostInfo {
					post_info: PostDispatchInfo {
						actual_weight: Some(T::DbWeight::get().reads(1)),
						pays_fee: Pays::Yes,
					},
					error: Error::<T>::MultisigNotFound.into(),
				}
			})?;
			if !multisig_data.signers.contains(&executor) {
				return Self::err_with_weight(Error::<T>::NotASigner, 1);
			}

			// Get proposal (2 reads: Multisigs + Proposals)
			let proposal =
				Proposals::<T>::get(&multisig_address, proposal_id).ok_or_else(|| {
					DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo {
							actual_weight: Some(T::DbWeight::get().reads(2)),
							pays_fee: Pays::Yes,
						},
						error: Error::<T>::ProposalNotFound.into(),
					}
				})?;

			// Must be Approved status
			if proposal.status != ProposalStatus::Approved {
				return Self::err_with_weight(Error::<T>::ProposalNotApproved, 2);
			}

			// Must not be expired
			let current_block = frame_system::Pallet::<T>::block_number();
			if current_block > proposal.expiry {
				return Self::err_with_weight(Error::<T>::ProposalExpired, 2);
			}

			// Decode the call
			let call = <T as Config>::RuntimeCall::decode(&mut &proposal.call[..])
				.map_err(|_| Self::err_with_weight_raw(Error::<T>::InvalidCall, 2))?;

			// Get weight info for accounting
			let call_weight = call.get_dispatch_info().call_weight;
			let bookkeeping_weight = Self::bookkeeping_weight(proposal.call.len() as u32);

			// EFFECTS: Remove proposal and return deposit BEFORE dispatch (reentrancy protection)
			Self::remove_proposal_and_return_deposit(
				&multisig_address,
				proposal_id,
				&proposal.proposer,
				proposal.deposit,
			);

			// INTERACTIONS: Dispatch the call as the multisig account
			let result =
				call.dispatch(frame_system::RawOrigin::Signed(multisig_address.clone()).into());

			// Emit event with execution details
			Self::deposit_event(Event::ProposalExecuted {
				multisig_address,
				proposal_id,
				proposer: proposal.proposer,
				call: proposal.call.to_vec(),
				approvers: proposal.approvals.to_vec(),
				result: result.as_ref().map(|_| ()).map_err(|e| e.error),
			});

			// Calculate actual weight: bookkeeping + inner call's actual weight
			let actual_call_weight = match &result {
				Ok(info) | Err(DispatchErrorWithPostInfo { post_info: info, .. }) => {
					info.actual_weight.unwrap_or(call_weight)
				},
			};
			let total_weight = bookkeeping_weight.saturating_add(actual_call_weight);

			// Return result with proper weight accounting
			result
				.map(|_| PostDispatchInfo { actual_weight: Some(total_weight), pays_fee: Pays::Yes })
				.map_err(|e| DispatchErrorWithPostInfo {
					post_info: PostDispatchInfo {
						actual_weight: Some(total_weight),
						pays_fee: Pays::Yes,
					},
					error: e.error,
				})
		}

	}

	impl<T: Config> Pallet<T> {
		/// Return an error with actual weight consumed instead of charging full upfront weight.
		/// Use for early exits where minimal work was performed.
		fn err_with_weight(error: Error<T>, reads: u64) -> DispatchResultWithPostInfo {
			Err(DispatchErrorWithPostInfo {
				post_info: PostDispatchInfo {
					actual_weight: Some(T::DbWeight::get().reads(reads)),
					pays_fee: Pays::Yes,
				},
				error: error.into(),
			})
		}

		/// Return a raw DispatchErrorWithPostInfo (not wrapped in Result).
		/// Use when you need to map_err with a custom error.
		fn err_with_weight_raw(error: Error<T>, reads: u64) -> DispatchErrorWithPostInfo {
			DispatchErrorWithPostInfo {
				post_info: PostDispatchInfo {
					actual_weight: Some(T::DbWeight::get().reads(reads)),
					pays_fee: Pays::Yes,
				},
				error: error.into(),
			}
		}

		/// Returns the multisig bookkeeping weight for execute (excludes inner call weight).
		fn bookkeeping_weight(call_size: u32) -> Weight {
			<T as Config>::WeightInfo::execute(call_size)
		}

		/// Derive a deterministic multisig address from signers, threshold, and nonce
		///
		/// The address is computed as: hash(pallet_id || sorted_signers || threshold || nonce)
		/// Signers are automatically sorted internally for deterministic results.
		/// This allows users to pre-compute the address before creating the multisig.
		pub fn derive_multisig_address(
			signers: &[T::AccountId],
			threshold: u32,
			nonce: u64,
		) -> T::AccountId {
			// Sort signers for deterministic address generation
			// User doesn't need to worry about order
			let mut sorted_signers = signers.to_vec();
			sorted_signers.sort();

			// Create a unique identifier from pallet id + sorted signers + threshold + nonce.
			//
			// IMPORTANT:
			// - Do NOT `Decode` directly from a finite byte-slice and then "fallback" to a constant
			//   address on error: that can cause address collisions / DoS.
			// - Using `TrailingZeroInput` makes decoding deterministic and infallible by providing
			//   an infinite stream (hash bytes padded with zeros).
			let pallet_id = T::PalletId::get();
			let mut data = Vec::new();
			data.extend_from_slice(&pallet_id.0);
			data.extend_from_slice(&sorted_signers.encode());
			data.extend_from_slice(&threshold.encode());
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

		/// Cleanup ALL expired proposals for a specific proposer
		///
		/// Iterates through all proposals in the multisig and removes expired ones
		/// belonging to the specified proposer.
		///
		/// Returns: (cleaned_count, total_proposals_iterated, total_call_bytes, total_deposits)
		/// - cleaned_count: number of proposals actually removed
		/// - total_proposals_iterated: total proposals that existed before cleanup (for weight
		///   calculation)
		/// - total_call_bytes: sum of proposal.call.len() over iterated proposals (for weight)
		/// - total_deposits: sum of actual deposits unreserved (from stored proposal data)
		fn cleanup_proposer_expired(
			multisig_address: &T::AccountId,
			proposer: &T::AccountId,
			caller: &T::AccountId,
		) -> (u32, u32, u32, BalanceOf<T>) {
			let current_block = frame_system::Pallet::<T>::block_number();
			let mut total_iterated = 0u32;
			let mut total_call_bytes = 0u32;
			let mut total_deposits = BalanceOf::<T>::zero();

			// Collect expired proposals to remove
			// IMPORTANT: We count ALL proposals during iteration (for weight calculation)
			let expired_proposals: Vec<(u32, T::AccountId, BalanceOf<T>)> =
				Proposals::<T>::iter_prefix(multisig_address)
					.filter_map(|(proposal_id, proposal)| {
						total_iterated += 1; // Count every proposal we iterate through
						total_call_bytes += proposal.call.len() as u32;

						// Only proposer's expired proposals (Active or Approved)
						if proposal.proposer == *proposer &&
							(proposal.status == ProposalStatus::Active ||
								proposal.status == ProposalStatus::Approved) &&
							current_block > proposal.expiry
						{
							Some((proposal_id, proposal.proposer, proposal.deposit))
						} else {
							None
						}
					})
					.collect();

			let cleaned = expired_proposals.len() as u32;

			// Remove proposals and emit events
			for (proposal_id, expired_proposer, deposit) in expired_proposals {
				total_deposits = total_deposits.saturating_add(deposit);

				Self::remove_proposal_and_return_deposit(
					multisig_address,
					proposal_id,
					&expired_proposer,
					deposit,
				);

				Self::deposit_event(Event::ProposalRemoved {
					multisig_address: multisig_address.clone(),
					proposal_id,
					proposer: expired_proposer,
					removed_by: caller.clone(),
				});
			}

			(cleaned, total_iterated, total_call_bytes, total_deposits)
		}

		/// Remove a proposal from storage and return deposit to proposer
		/// Used for cleanup operations
		fn remove_proposal_and_return_deposit(
			multisig_address: &T::AccountId,
			proposal_id: u32,
			proposer: &T::AccountId,
			deposit: BalanceOf<T>,
		) {
			// Remove from storage
			Proposals::<T>::remove(multisig_address, proposal_id);

			// Decrement proposal counters
			Multisigs::<T>::mutate(multisig_address, |maybe_data| {
				if let Some(ref mut data) = maybe_data {
					data.active_proposals = data.active_proposals.saturating_sub(1);
					if let Some(count) = data.proposals_per_signer.get_mut(proposer) {
						*count = count.saturating_sub(1);
						// Remove entry if count reaches 0 to save storage
						if *count == 0 {
							data.proposals_per_signer.remove(proposer);
						}
					}
				}
			});

			// Return deposit to proposer
			T::Currency::unreserve(proposer, deposit);
		}

	}
}
