//! # Merkle Airdrop Pallet
//!
//! A pallet for distributing tokens via Merkle proofs, allowing efficient token airdrops
//! where recipients can claim their tokens by providing cryptographic proofs of eligibility.
//!
//! ## Overview
//!
//! This pallet provides functionality for:
//! - Creating airdrops with a Merkle root representing all valid claims
//! - Funding airdrops with tokens to be distributed
//! - Allowing users to claim tokens by providing Merkle proofs
//! - Allowing creators to delete airdrops and reclaim any unclaimed tokens
//!
//! The use of Merkle trees allows for gas-efficient verification of eligibility without
//! storing the complete list of recipients on-chain.
//!
//! ## Interface
//!
//! ### Dispatchable Functions
//!
//! * `create_airdrop` - Create a new airdrop with a Merkle root
//! * `fund_airdrop` - Fund an existing airdrop with tokens
//! * `claim` - Claim tokens from an airdrop by providing a Merkle proof
//! * `delete_airdrop` - Delete an airdrop and reclaim any remaining tokens (creator only)

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::*;

use frame_support::traits::fungible::Inspect;

type BalanceOf<T> =
    <<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

/// Type for storing a Merkle root hash
pub type MerkleRoot = [u8; 32];

/// Type for Merkle hash values
pub type MerkleHash = [u8; 32];

/// Airdrop ID type
pub type AirdropId = u32;

#[frame_support::pallet]
pub mod pallet {
    use crate::{AirdropId, BalanceOf, MerkleHash, MerkleRoot};

    use super::weights::WeightInfo;
    use frame_support::{
        pallet_prelude::*,
        traits::{
            fungible::{Inspect, Mutate},
            Get,
        },
    };
    use frame_system::pallet_prelude::*;
    use sp_io::hashing::blake2_256;
    use sp_runtime::traits::AccountIdConversion;
    use sp_runtime::traits::Saturating;
    extern crate alloc;
    use alloc::vec;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Configuration trait for the Merkle airdrop pallet.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The currency mechanism.
        type Currency: Inspect<Self::AccountId> + Mutate<Self::AccountId>;

        /// The maximum number of proof elements allowed in a Merkle proof.
        #[pallet::constant]
        type MaxProofs: Get<u32>;

        /// The pallet id, used for deriving its sovereign account ID.
        #[pallet::constant]
        type PalletId: Get<frame_support::PalletId>;

        /// Weight information for the extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    /// Storage for Merkle roots of each airdrop
    #[pallet::storage]
    #[pallet::getter(fn airdrop_merkle_roots)]
    pub type AirdropMerkleRoots<T> = StorageMap<_, Blake2_128Concat, AirdropId, MerkleRoot>;

    /// Storage for airdrop creators
    #[pallet::storage]
    #[pallet::getter(fn airdrop_creators)]
    pub type AirdropCreators<T: Config> = StorageMap<_, Blake2_128Concat, AirdropId, T::AccountId>;

    /// Storage for airdrop balances
    #[pallet::storage]
    #[pallet::getter(fn airdrop_balances)]
    pub type AirdropBalances<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        AirdropId,
        <<T as Config>::Currency as Inspect<T::AccountId>>::Balance,
        ValueQuery,
    >;

    /// Storage for claimed status
    #[pallet::storage]
    #[pallet::getter(fn is_claimed)]
    pub type Claimed<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        AirdropId,
        Blake2_128Concat,
        T::AccountId,
        (),
        ValueQuery,
    >;

    /// Counter for airdrop IDs
    #[pallet::storage]
    #[pallet::getter(fn next_airdrop_id)]
    pub type NextAirdropId<T> = StorageValue<_, AirdropId, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new airdrop has been created.
        ///
        /// Parameters: [airdrop_id, merkle_root]
        AirdropCreated {
            /// The ID of the created airdrop
            airdrop_id: AirdropId,
            /// The Merkle root of the airdrop
            merkle_root: MerkleRoot,
        },
        /// An airdrop has been funded with tokens.
        ///
        /// Parameters: [airdrop_id, amount]
        AirdropFunded {
            /// The ID of the funded airdrop
            airdrop_id: AirdropId,
            /// The amount of tokens added to the airdrop
            amount: <<T as Config>::Currency as Inspect<T::AccountId>>::Balance,
        },
        /// A user has claimed tokens from an airdrop.
        ///
        /// Parameters: [airdrop_id, account, amount]
        Claimed {
            /// The ID of the airdrop claimed from
            airdrop_id: AirdropId,
            /// The account that claimed the tokens
            account: T::AccountId,
            /// The amount of tokens claimed
            amount: <<T as Config>::Currency as Inspect<T::AccountId>>::Balance,
        },
        /// An airdrop has been deleted.
        ///
        /// Parameters: [airdrop_id]
        AirdropDeleted {
            /// The ID of the deleted airdrop
            airdrop_id: AirdropId,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The specified airdrop does not exist.
        AirdropNotFound,
        /// The airdrop does not have sufficient balance for this operation.
        InsufficientAirdropBalance,
        /// The user has already claimed from this airdrop.
        AlreadyClaimed,
        /// The provided Merkle proof is invalid.
        InvalidProof,
        /// Only the creator of an airdrop can delete it.
        NotAirdropCreator,
    }

    impl<T: Config> Pallet<T> {
        /// Returns the account ID of the pallet.
        ///
        /// This account is used to hold the funds for all airdrops.
        pub fn account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        /// Verifies a Merkle proof against a Merkle root using Blake2 hash.
        ///
        /// This function checks if an account is eligible to claim a specific amount from an airdrop
        /// by verifying a Merkle proof against the stored Merkle root.
        ///
        /// # Parameters
        ///
        /// * `account` - The account ID claiming tokens
        /// * `amount` - The amount of tokens being claimed
        /// * `merkle_root` - The Merkle root to verify against
        /// * `merkle_proof` - The proof path from the leaf to the root
        ///
        /// # Returns
        ///
        /// `true` if the proof is valid, `false` otherwise
        pub fn verify_merkle_proof(
            account: &T::AccountId,
            amount: BalanceOf<T>,
            merkle_root: &MerkleRoot,
            merkle_proof: &[MerkleHash],
        ) -> bool {
            let leaf = Self::calculate_leaf_hash_blake2(account, amount);

            // Verify the proof by walking up the tree
            let mut computed_hash = leaf;
            for proof_element in merkle_proof.iter() {
                computed_hash = if computed_hash < *proof_element {
                    Self::calculate_parent_hash_blake2(&computed_hash, proof_element)
                } else {
                    Self::calculate_parent_hash_blake2(proof_element, &computed_hash)
                };
            }
            computed_hash == *merkle_root
        }

        /// Calculates the leaf hash for a Merkle tree using Blake2.
        ///
        /// This function creates a leaf node hash from an account and amount using the
        /// Blake2 hash function, which is optimized for zero-knowledge proofs.
        ///
        /// # Parameters
        ///
        /// * `account` - The account ID to include in the leaf
        /// * `amount` - The token amount to include in the leaf
        ///
        /// # Returns
        ///
        /// A 32-byte array containing the Blake2 hash of the account and amount
        pub fn calculate_leaf_hash_blake2(
            account: &T::AccountId,
            amount: BalanceOf<T>,
        ) -> MerkleHash {
            let bytes = (account, amount).encode();
            blake2_256(&bytes)
        }

        /// Calculates the parent hash in a Merkle tree using Blake2.
        ///
        /// This function combines two child hashes to create their parent hash in the Merkle tree.
        /// The children are ordered lexicographically before hashing to ensure consistency.
        ///
        /// # Parameters
        ///
        /// * `left` - The first child hash
        /// * `right` - The second child hash
        ///
        /// # Returns
        ///
        /// A 32-byte array containing the Blake2 hash of the combined children
        pub fn calculate_parent_hash_blake2(left: &MerkleHash, right: &MerkleHash) -> MerkleHash {
            // Ensure consistent ordering of inputs (important for verification)
            let combined = if left < right {
                [left.as_slice(), right.as_slice()].concat()
            } else {
                [right.as_slice(), left.as_slice()].concat()
            };

            blake2_256(&combined)
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Create a new airdrop with a Merkle root.
        ///
        /// The Merkle root is a cryptographic hash that represents all valid claims
        /// for this airdrop. Users will later provide Merkle proofs to verify their
        /// eligibility to claim tokens.
        ///
        /// # Parameters
        ///
        /// * `origin` - The origin of the call (must be signed)
        /// * `merkle_root` - The Merkle root hash representing all valid claims
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::create_airdrop())]
        pub fn create_airdrop(origin: OriginFor<T>, merkle_root: MerkleRoot) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let airdrop_id = Self::next_airdrop_id();

            AirdropMerkleRoots::<T>::insert(airdrop_id, merkle_root);
            AirdropCreators::<T>::insert(airdrop_id, who.clone());

            NextAirdropId::<T>::put(airdrop_id.saturating_add(1));

            Self::deposit_event(Event::AirdropCreated {
                airdrop_id,
                merkle_root,
            });

            Ok(())
        }

        /// Fund an existing airdrop with tokens.
        ///
        /// This function transfers tokens from the caller to the airdrop's account,
        /// making them available for users to claim.
        ///
        /// # Parameters
        ///
        /// * `origin` - The origin of the call (must be signed)
        /// * `airdrop_id` - The ID of the airdrop to fund
        /// * `amount` - The amount of tokens to add to the airdrop
        ///
        /// # Errors
        ///
        /// * `AirdropNotFound` - If the specified airdrop does not exist
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::fund_airdrop())]
        pub fn fund_airdrop(
            origin: OriginFor<T>,
            airdrop_id: AirdropId,
            amount: <<T as Config>::Currency as Inspect<T::AccountId>>::Balance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(
                AirdropMerkleRoots::<T>::contains_key(airdrop_id),
                Error::<T>::AirdropNotFound
            );

            <T::Currency as Mutate<T::AccountId>>::transfer(
                &who,
                &Self::account_id(),
                amount,
                frame_support::traits::tokens::Preservation::Preserve,
            )?;

            AirdropBalances::<T>::mutate(airdrop_id, |balance| {
                *balance = balance.saturating_add(amount);
            });

            Self::deposit_event(Event::AirdropFunded { airdrop_id, amount });

            Ok(())
        }

        /// Claim tokens from an airdrop by providing a Merkle proof.
        ///
        /// Users can claim their tokens by providing a proof of their eligibility.
        /// The proof is verified against the airdrop's Merkle root.
        /// Anyone can trigger a claim for any eligible recipient.
        ///
        /// # Parameters
        ///
        /// * `origin` - The origin of the call
        /// * `airdrop_id` - The ID of the airdrop to claim from
        /// * `amount` - The amount of tokens to claim
        /// * `merkle_proof` - The Merkle proof verifying eligibility
        ///
        /// # Errors
        ///
        /// * `AirdropNotFound` - If the specified airdrop does not exist
        /// * `AlreadyClaimed` - If the recipient has already claimed from this airdrop
        /// * `InvalidProof` - If the provided Merkle proof is invalid
        /// * `InsufficientAirdropBalance` - If the airdrop doesn't have enough tokens
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::claim())]
        pub fn claim(
            origin: OriginFor<T>,
            airdrop_id: AirdropId,
            recipient: T::AccountId,
            amount: <<T as Config>::Currency as Inspect<T::AccountId>>::Balance,
            merkle_proof: BoundedVec<MerkleHash, T::MaxProofs>,
        ) -> DispatchResult {
            let _ = ensure_signed(origin)?;

            ensure!(
                AirdropMerkleRoots::<T>::contains_key(airdrop_id),
                Error::<T>::AirdropNotFound
            );

            ensure!(
                !Claimed::<T>::contains_key(airdrop_id, &recipient),
                Error::<T>::AlreadyClaimed
            );

            let merkle_root =
                AirdropMerkleRoots::<T>::get(airdrop_id).ok_or(Error::<T>::AirdropNotFound)?;

            ensure!(
                Self::verify_merkle_proof(&recipient, amount, &merkle_root, &merkle_proof),
                Error::<T>::InvalidProof
            );

            let airdrop_balance = AirdropBalances::<T>::get(airdrop_id);
            ensure!(
                airdrop_balance >= amount,
                Error::<T>::InsufficientAirdropBalance
            );

            // Mark as claimed before performing the transfer
            Claimed::<T>::insert(airdrop_id, &recipient, ());

            AirdropBalances::<T>::mutate(airdrop_id, |balance| {
                *balance = balance.saturating_sub(amount);
            });

            <T::Currency as Mutate<T::AccountId>>::transfer(
                &Self::account_id(),
                &recipient,
                amount,
                frame_support::traits::tokens::Preservation::Preserve,
            )?;

            Self::deposit_event(Event::Claimed {
                airdrop_id,
                account: recipient,
                amount,
            });

            Ok(())
        }

        /// Delete an airdrop and reclaim any remaining funds.
        ///
        /// This function allows the creator of an airdrop to delete it and reclaim
        /// any remaining tokens that haven't been claimed.
        ///
        /// # Parameters
        ///
        /// * `origin` - The origin of the call (must be the airdrop creator)
        /// * `airdrop_id` - The ID of the airdrop to delete
        ///
        /// # Errors
        ///
        /// * `AirdropNotFound` - If the specified airdrop does not exist
        /// * `NotAirdropCreator` - If the caller is not the creator of the airdrop
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::delete_airdrop())]
        pub fn delete_airdrop(origin: OriginFor<T>, airdrop_id: AirdropId) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(
                AirdropMerkleRoots::<T>::contains_key(airdrop_id),
                Error::<T>::AirdropNotFound
            );

            let creator =
                AirdropCreators::<T>::get(airdrop_id).ok_or(Error::<T>::AirdropNotFound)?;
            ensure!(who == creator, Error::<T>::NotAirdropCreator);

            let balance = AirdropBalances::<T>::get(airdrop_id);

            if !balance.is_zero() {
                <T::Currency as Mutate<T::AccountId>>::transfer(
                    &Self::account_id(),
                    &creator,
                    balance,
                    frame_support::traits::tokens::Preservation::Preserve,
                )?;
            }

            // Remove the airdrop data from storage
            AirdropMerkleRoots::<T>::remove(airdrop_id);
            AirdropBalances::<T>::remove(airdrop_id);
            AirdropCreators::<T>::remove(airdrop_id);

            Self::deposit_event(Event::AirdropDeleted { airdrop_id });

            Ok(())
        }
    }
}
