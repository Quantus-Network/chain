//! # ZK-Vote Pallet
//!
//! A single-round, privacy-preserving voting mechanism for blockchain validators
//! that allows anonymous voting while preventing double voting.
//!
//! ## Overview
//!
//! This pallet provides functionality for:
//! - Registering proposals for private voting with a Merkle root of eligible voters.
//! - Submitting anonymous votes using a zero-knowledge proof.
//! - Tallying votes (Yes/No).
//! - Preventing double voting using nullifiers.
//!
//! ### ZK Circuit Logic (as designed)
//!
//! - **Public Inputs**:
//!   - `Proposal ID`
//!   - `Merkle root` of eligible addresses
//!   - `Vote` (yes/no)
//!   - `Nullifier` = `hash(hash(private_key) || proposal ID)`
//! - **Private Inputs**:
//!   - Address `private key`
//!   - `Merkle proof` of address inclusion in the eligible set
//!
//! The circuit verifies the Merkle proof, derives the public key to check against the one in the proof,
//! and ensures the nullifier is correctly computed.
//!
//! The pallet receives the public inputs and the ZK proof, verifies the proof,
//! checks for nullifier reuse, and records the vote.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::pallet_prelude::*;
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_core::RuntimeDebug;
use sp_runtime::transaction_validity::{
    InvalidTransaction, TransactionLongevity, TransactionSource, TransactionValidity,
    ValidTransaction,
};

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::WeightInfo;

// --- Types ---

/// The unique identifier for a proposal.
pub type ProposalId = u32;

/// The Merkle root of eligible voters.
pub type MerkleRoot = [u8; 32];

/// A unique value to prevent double voting, specific to a proposal and a voter.
pub type Nullifier = [u8; 32];

/// The vote decision.
#[derive(Encode, Decode, Copy, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub enum Vote {
    Yes,
    No,
}

/// A struct to hold information about a registered proposal for ZK voting.
#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct Proposal<AccountId> {
    /// The creator of the proposal.
    pub creator: AccountId,
    /// The Merkle root of the set of eligible voters.
    pub merkle_root: MerkleRoot,
    /// The current tally of 'Yes' votes.
    pub yes_votes: u64,
    /// The current tally of 'No' votes.
    pub no_votes: u64,
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The origin that is allowed to register new proposals.
        type RegisterOrigin: EnsureOrigin<Self::RuntimeOrigin, Success = Self::AccountId>;

        /// The maximum length of a ZK proof.
        #[pallet::constant]
        type MaxProofLength: Get<u32>;

        /// Priority for unsigned vote transactions.
        #[pallet::constant]
        type UnsignedVotePriority: Get<TransactionPriority>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    /// Stores the details of each registered proposal.
    #[pallet::storage]
    #[pallet::getter(fn proposals)]
    pub type Proposals<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, Proposal<T::AccountId>>;

    /// Tracks used nullifiers for each proposal to prevent double voting.
    /// The key is a hash of the nullifier to prevent DB preimage attacks.
    #[pallet::storage]
    #[pallet::getter(fn used_nullifiers)]
    pub type UsedNullifiers<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        ProposalId,
        Blake2_128Concat,
        Nullifier,
        (),
        ValueQuery,
    >;

    /// Counter for the next proposal ID.
    #[pallet::storage]
    #[pallet::getter(fn next_proposal_id)]
    pub type NextProposalId<T> = StorageValue<_, ProposalId, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new proposal was successfully registered for ZK-voting.
        ProposalRegistered {
            proposal_id: ProposalId,
            creator: T::AccountId,
            merkle_root: MerkleRoot,
        },
        /// A vote was successfully cast.
        Voted { proposal_id: ProposalId, vote: Vote },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The specified proposal was not found.
        ProposalNotFound,
        /// The nullifier has already been used for this proposal.
        NullifierAlreadyUsed,
        /// The provided ZK proof is invalid.
        InvalidProof,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Registers a new proposal for ZK-based voting.
        ///
        /// This extrinsic creates a new proposal entry, storing the Merkle root of
        /// eligible voters. Only an authorized origin can perform this action.
        ///
        /// # Parameters
        ///
        /// * `origin`: The privileged origin for registering proposals.
        /// * `merkle_root`: The Merkle root hash of the list of eligible voters.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::register_proposal())]
        pub fn register_proposal(origin: OriginFor<T>, merkle_root: MerkleRoot) -> DispatchResult {
            let creator = T::RegisterOrigin::ensure_origin(origin)?;

            let proposal_id = Self::next_proposal_id();
            let new_proposal = Proposal {
                creator: creator.clone(),
                merkle_root,
                yes_votes: 0,
                no_votes: 0,
            };

            Proposals::<T>::insert(proposal_id, new_proposal);
            NextProposalId::<T>::mutate(|id| *id += 1);

            Self::deposit_event(Event::ProposalRegistered {
                proposal_id,
                creator,
                merkle_root,
            });

            Ok(())
        }

        /// Submits an anonymous vote for a given proposal.
        ///
        /// This is an unsigned extrinsic, ensuring voter anonymity. The voter's
        /// eligibility and the prevention of double-voting are enforced by a
        /// zero-knowledge proof and a nullifier.
        ///
        /// # Parameters
        ///
        /// * `origin`: Must be `None`.
        /// * `proposal_id`: The ID of the proposal to vote on.
        /// * `vote`: The voter's choice (`Yes` or `No`).
        /// * `nullifier`: A unique value to prevent double voting.
        /// * `proof`: The ZK proof of eligibility and valid vote construction.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::vote())]
        pub fn vote(
            origin: OriginFor<T>,
            proposal_id: ProposalId,
            vote: Vote,
            nullifier: Nullifier,
            proof: BoundedVec<u8, T::MaxProofLength>,
        ) -> DispatchResult {
            ensure_none(origin)?;

            // 1. Check if nullifier is already used
            ensure!(
                !UsedNullifiers::<T>::contains_key(proposal_id, nullifier),
                Error::<T>::NullifierAlreadyUsed
            );

            // 2. Get proposal
            let mut proposal =
                Proposals::<T>::get(proposal_id).ok_or(Error::<T>::ProposalNotFound)?;

            // 3. Verify ZK proof
            let is_valid = Self::verify_proof(&proposal.merkle_root, &vote, &nullifier, &proof);
            ensure!(is_valid, Error::<T>::InvalidProof);

            // 4. Record nullifier
            UsedNullifiers::<T>::insert(proposal_id, nullifier, ());

            // 5. Update tally
            match vote {
                Vote::Yes => proposal.yes_votes = proposal.yes_votes.saturating_add(1),
                Vote::No => proposal.no_votes = proposal.no_votes.saturating_add(1),
            }
            Proposals::<T>::insert(proposal_id, proposal);

            Self::deposit_event(Event::Voted { proposal_id, vote });

            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            if let Call::vote {
                proposal_id,
                vote,
                nullifier,
                proof,
            } = call
            {
                // --- Lightweight checks ---

                // 1. Check if proposal exists
                let proposal = Proposals::<T>::get(proposal_id).ok_or(InvalidTransaction::Stale)?;

                // 2. Check if nullifier is already used
                if UsedNullifiers::<T>::contains_key(*proposal_id, *nullifier) {
                    return InvalidTransaction::Stale.into();
                }

                // --- Heavier check (proof verification) ---
                if !Self::verify_proof(&proposal.merkle_root, vote, nullifier, proof) {
                    return InvalidTransaction::BadProof.into();
                }

                ValidTransaction::with_tag_prefix("ZKVote")
                    .priority(T::UnsignedVotePriority::get())
                    .and_provides((proposal_id, nullifier.encode()))
                    .longevity(TransactionLongevity::MAX)
                    .propagate(true)
                    .build()
            } else {
                InvalidTransaction::Call.into()
            }
        }
    }

    impl<T: Config> Pallet<T> {
        /// TODO: not yet implemented.
        pub fn verify_proof(
            _merkle_root: &MerkleRoot,
            _vote: &Vote,
            _nullifier: &Nullifier,
            _proof: &[u8],
        ) -> bool {
            true
        }
    }
}
