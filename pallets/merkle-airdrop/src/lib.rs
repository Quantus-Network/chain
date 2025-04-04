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

#[frame_support::pallet]
pub mod pallet {
    use frame_support::{
        pallet_prelude::*,
        traits::{Get},
    };
    use frame_system::pallet_prelude::*;
    use sp_std::prelude::*;
    use frame_support::traits::Currency;
    use super::weights::WeightInfo;
    use sp_runtime::traits::AccountIdConversion;
    use sp_runtime::traits::Saturating;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Configuration trait for the Merkle airdrop pallet.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The currency mechanism.
        type Currency: Currency<Self::AccountId>;

        /// The maximum number of airdrops that can be active at once.
        #[pallet::constant]
        type MaxAirdrops: Get<u32>;

        /// The pallet id, used for deriving its sovereign account ID.
        #[pallet::constant]
        type PalletId: Get<frame_support::PalletId>;

        /// Weight information for the extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    /// Type for storing a Merkle root hash
    pub type MerkleRoot = [u8; 32];

    /// Airdrop ID type
    pub type AirdropId = u32;

    /// Storage for Merkle roots of each airdrop
    #[pallet::storage]
    #[pallet::getter(fn airdrop_merkle_roots)]
    pub type AirdropMerkleRoots<T> = StorageMap<_, Blake2_128Concat, AirdropId, MerkleRoot>;

    /// Storage for airdrop balances
    #[pallet::storage]
    #[pallet::getter(fn airdrop_balances)]
    pub type AirdropBalances<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        AirdropId,
        <<T as Config>::Currency as Currency<T::AccountId>>::Balance
    >;

    /// Storage for claimed status
    #[pallet::storage]
    #[pallet::getter(fn is_claimed)]
    pub type Claimed<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat, AirdropId,
        Blake2_128Concat, T::AccountId,
        bool, ValueQuery
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
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
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
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The specified airdrop does not exist.
        AirdropNotFound,
        /// The airdrop with this ID already exists.
        AirdropAlreadyExists,
        /// The maximum number of airdrops has been reached.
        TooManyAirdrops,
        /// The airdrop does not have sufficient balance for this operation.
        InsufficientAirdropBalance,
        /// The user has already claimed from this airdrop.
        AlreadyClaimed,
        /// The provided Merkle proof is invalid.
        InvalidProof,
    }

    impl<T: Config> Pallet<T> {
        /// Returns the account ID of the pallet.
        ///
        /// This account is used to hold the funds for all airdrops.
        pub fn account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        /// Verifies a Merkle proof against a Merkle root.
        pub fn verify_merkle_proof(
            account: &T::AccountId,
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
            merkle_root: &[u8; 32],
            merkle_proof: &Vec<[u8; 32]>
        ) -> bool {
            // Create and hash the leaf data (account + amount)
            let account_bytes = account.encode();
            let amount_bytes = amount.encode();
            let leaf_data = [&account_bytes[..], &amount_bytes[..]].concat();
            let leaf_hash = sp_core::blake2_256(&leaf_data);

            // Start with the leaf hash
            let mut current_hash = leaf_hash;

            // Apply each proof element
            for proof_element in merkle_proof {
                // Sort the hashes to ensure consistent ordering
                // Compare arrays lexicographically
                let combined = if current_hash.as_slice() < proof_element.as_slice() {
                    [&current_hash[..], &proof_element[..]].concat()
                } else {
                    [&proof_element[..], &current_hash[..]].concat()
                };

                // Hash the combined value
                current_hash = sp_core::blake2_256(&combined);
            }

            // Compare the computed root with the stored root
            current_hash == *merkle_root
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
        ///
        /// # Errors
        ///
        /// * `TooManyAirdrops` - If the maximum number of airdrops has been reached
        /// * `AirdropAlreadyExists` - If an airdrop with this ID already exists
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::create_airdrop())]
        pub fn create_airdrop(
            origin: OriginFor<T>,
            merkle_root: MerkleRoot,
        ) -> DispatchResult {
            let _who = ensure_signed(origin)?;

            // Get the next available airdrop ID
            let airdrop_id = Self::next_airdrop_id();

            // Ensure we haven't reached the maximum number of airdrops
            ensure!(
                airdrop_id < T::MaxAirdrops::get(),
                Error::<T>::TooManyAirdrops
            );

            // Ensure this airdrop doesn't already exist (should never happen with sequential IDs)
            ensure!(
                !AirdropMerkleRoots::<T>::contains_key(airdrop_id),
                Error::<T>::AirdropAlreadyExists
            );

            // Store the Merkle root for this airdrop
            AirdropMerkleRoots::<T>::insert(airdrop_id, merkle_root);

            // Initialize the airdrop balance to zero with explicit type
            let zero_balance: <<T as Config>::Currency as Currency<T::AccountId>>::Balance = 0u32.into();
            AirdropBalances::<T>::insert(airdrop_id, zero_balance);

            // Increment the airdrop ID counter for next time
            NextAirdropId::<T>::put(airdrop_id + 1);

            // Emit an event
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
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            // Ensure the airdrop exists
            ensure!(
                AirdropMerkleRoots::<T>::contains_key(airdrop_id),
                Error::<T>::AirdropNotFound
            );

            // Transfer tokens from the caller to the pallet account
            T::Currency::transfer(
                &who,
                &Self::account_id(),
                amount,
                frame_support::traits::ExistenceRequirement::KeepAlive
            )?;

            // Update the airdrop balance
            AirdropBalances::<T>::mutate(airdrop_id, |balance| {
                if let Some(current_balance) = balance {
                    *current_balance = current_balance.saturating_add(amount);
                } else {
                    *balance = Some(amount);
                }
            });

            // Emit an event
            Self::deposit_event(Event::AirdropFunded {
                airdrop_id,
                amount,
            });

            Ok(())
        }

        /// Claim tokens from an airdrop by providing a Merkle proof.
        ///
        /// Users can claim their tokens by providing a proof of their eligibility.
        /// The proof is verified against the airdrop's Merkle root.
        ///
        /// # Parameters
        ///
        /// * `origin` - The origin of the call (must be signed)
        /// * `airdrop_id` - The ID of the airdrop to claim from
        /// * `amount` - The amount of tokens to claim
        /// * `merkle_proof` - The Merkle proof verifying eligibility
        ///
        /// # Errors
        ///
        /// * `AirdropNotFound` - If the specified airdrop does not exist
        /// * `AlreadyClaimed` - If the user has already claimed from this airdrop
        /// * `InvalidProof` - If the provided Merkle proof is invalid
        /// * `InsufficientAirdropBalance` - If the airdrop doesn't have enough tokens
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::claim())]
        pub fn claim(
            origin: OriginFor<T>,
            airdrop_id: AirdropId,
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
            merkle_proof: Vec<[u8; 32]>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            // Ensure the airdrop exists
            ensure!(
                AirdropMerkleRoots::<T>::contains_key(airdrop_id),
                Error::<T>::AirdropNotFound
            );

            // Ensure the user hasn't already claimed
            ensure!(
                !Claimed::<T>::contains_key(airdrop_id, &who),
                Error::<T>::AlreadyClaimed
            );

            // Get the Merkle root for this airdrop
            let merkle_root = AirdropMerkleRoots::<T>::get(airdrop_id)
                .ok_or(Error::<T>::AirdropNotFound)?;

            // Verify the Merkle proof
            ensure!(
                Self::verify_merkle_proof(&who, amount, &merkle_root, &merkle_proof),
                Error::<T>::InvalidProof
            );

            // Ensure the airdrop has sufficient balance
            let airdrop_balance = AirdropBalances::<T>::get(airdrop_id)
                .ok_or(Error::<T>::InsufficientAirdropBalance)?;
            ensure!(
                airdrop_balance >= amount,
                Error::<T>::InsufficientAirdropBalance
            );

            // Mark as claimed before performing the transfer to prevent reentrancy attacks
            Claimed::<T>::insert(airdrop_id, &who, true);

            // Update the airdrop balance
            AirdropBalances::<T>::mutate(airdrop_id, |balance| {
                if let Some(current_balance) = balance {
                    *current_balance = current_balance.saturating_sub(amount);
                }
            });

            // Transfer tokens from the pallet account to the user
            T::Currency::transfer(
                &Self::account_id(),
                &who,
                amount,
                frame_support::traits::ExistenceRequirement::KeepAlive
            )?;

            // Emit an event
            Self::deposit_event(Event::Claimed {
                airdrop_id,
                account: who,
                amount,
            });

            Ok(())
        }
    }
}