//! # Merkle Airdrop Pallet
//!
//! A pallet for distributing tokens via Merkle proofs, allowing efficient token airdrops
//! where recipients can claim their tokens by providing cryptographic proofs of eligibility.
//!
//! ## Overview
//!
//! This pallet provides functionality for:
//! - Creating airdrops with a Merkle root representing all valid claims, and optional vesting parameters
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
//! * `create_airdrop` - Create a new airdrop with a Merkle root and vesting parameters
//! * `fund_airdrop` - Fund an existing airdrop with tokens
//! * `claim` - Claim tokens from an airdrop by providing a Merkle proof
//! * `delete_airdrop` - Delete an airdrop and reclaim any remaining tokens (creator only)

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode, MaxEncodedLen};
use frame_system::pallet_prelude::BlockNumberFor;
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
use scale_info::TypeInfo;
use sp_core::RuntimeDebug;
pub use weights::*;

use frame_support::traits::{Currency, VestedTransfer};

/// NOTE: Vesting traits still use deprecated `Currency` trait.
type CurrencyOf<T> =
    <<T as Config>::Vesting as VestedTransfer<<T as frame_system::Config>::AccountId>>::Currency;

/// NOTE: Vesting traits still use deprecated `Currency` trait.
type BalanceOf<T> = <CurrencyOf<T> as Currency<<T as frame_system::Config>::AccountId>>::Balance;

/// Type alias for airdrop info for this pallet
type AirdropMetadataFor<T> =
    AirdropMetadata<BlockNumberFor<T>, BalanceOf<T>, <T as frame_system::Config>::AccountId>;

/// Type for storing a Merkle root hash
pub type MerkleRoot = [u8; 32];

/// Type for Merkle hash values
pub type MerkleHash = [u8; 32];

/// Airdrop ID type
pub type AirdropId = u32;

#[derive(Encode, Decode, PartialEq, Eq, Clone, TypeInfo, RuntimeDebug, MaxEncodedLen)]
pub struct AirdropMetadata<BlockNumber, Balance, AccountId> {
    /// Merkle root of the airdrop
    pub merkle_root: MerkleHash,
    /// Creator of the airdrop
    pub creator: AccountId,
    /// Current airdrop balance
    pub balance: Balance,
    /// Vesting period for the airdrop. `None` for immediate release.
    pub vesting_period: Option<BlockNumber>,
    /// Vesting start delay. `None` for immediate start
    pub vesting_delay: Option<BlockNumber>,
}

#[frame_support::pallet]
pub mod pallet {
    use crate::{
        AirdropId, AirdropMetadata, AirdropMetadataFor, BalanceOf, CurrencyOf, MerkleHash,
        MerkleRoot,
    };

    use super::weights::WeightInfo;
    use frame_support::{
        pallet_prelude::*,
        traits::{Currency, Get, VestedTransfer, VestingSchedule},
    };
    use frame_system::pallet_prelude::{BlockNumberFor, *};
    use sp_io::hashing::blake2_256;
    use sp_runtime::traits::AccountIdConversion;
    use sp_runtime::traits::{BlockNumberProvider, Convert, Saturating};
    use sp_runtime::transaction_validity::{
        InvalidTransaction, TransactionLongevity, TransactionSource, TransactionValidity,
        ValidTransaction,
    };
    extern crate alloc;
    use alloc::vec;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Configuration trait for the Merkle airdrop pallet.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The vesting mechanism.
        type Vesting: VestedTransfer<Self::AccountId, Moment = BlockNumberFor<Self>>
            + VestingSchedule<Self::AccountId, Moment = BlockNumberFor<Self>>;

        /// Convert the block number into a balance.
        type BlockNumberToBalance: Convert<BlockNumberFor<Self>, BalanceOf<Self>>;

        /// The maximum number of proof elements allowed in a Merkle proof.
        #[pallet::constant]
        type MaxProofs: Get<u32>;

        /// The pallet id, used for deriving its sovereign account ID.
        #[pallet::constant]
        type PalletId: Get<frame_support::PalletId>;

        /// Priority for unsigned claim transactions.
        #[pallet::constant]
        type UnsignedClaimPriority: Get<u64>;

        /// Weight information for the extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        /// Block number provider.
        type BlockNumberProvider: BlockNumberProvider<BlockNumber = BlockNumberFor<Self>>;
    }

    /// Stores general info about an airdrop
    #[pallet::storage]
    #[pallet::getter(fn airdrop_info)]
    pub type AirdropInfo<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        AirdropId,
        AirdropMetadata<BlockNumberFor<T>, BalanceOf<T>, T::AccountId>,
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
            /// Airdrop metadata
            airdrop_metadata: AirdropMetadataFor<T>,
        },
        /// An airdrop has been funded with tokens.
        ///
        /// Parameters: [airdrop_id, amount]
        AirdropFunded {
            /// The ID of the funded airdrop
            airdrop_id: AirdropId,
            /// The amount of tokens added to the airdrop
            amount: BalanceOf<T>,
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
            amount: BalanceOf<T>,
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
    #[repr(u8)]
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

    impl<T> Error<T> {
        /// Convert the error to its underlying code
        pub fn to_code(&self) -> u8 {
            match self {
                Error::<T>::AirdropNotFound => 1,
                Error::<T>::InsufficientAirdropBalance => 2,
                Error::<T>::AlreadyClaimed => 3,
                Error::<T>::InvalidProof => 4,
                Error::<T>::NotAirdropCreator => 5,
                _ => 0,
            }
        }
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
        /// * `vesting_period` - Optional vesting period for the airdrop
        /// * `vesting_delay` - Optional delay before vesting starts
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::create_airdrop())]
        pub fn create_airdrop(
            origin: OriginFor<T>,
            merkle_root: MerkleRoot,
            vesting_period: Option<BlockNumberFor<T>>,
            vesting_delay: Option<BlockNumberFor<T>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let airdrop_id = Self::next_airdrop_id();

            let airdrop_metadata = AirdropMetadata {
                merkle_root,
                creator: who.clone(),
                balance: Zero::zero(),
                vesting_period,
                vesting_delay,
            };

            AirdropInfo::<T>::insert(airdrop_id, &airdrop_metadata);
            NextAirdropId::<T>::put(airdrop_id.saturating_add(1));

            Self::deposit_event(Event::AirdropCreated {
                airdrop_id,
                airdrop_metadata,
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
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(
                AirdropInfo::<T>::contains_key(airdrop_id),
                Error::<T>::AirdropNotFound
            );

            CurrencyOf::<T>::transfer(
                &who,
                &Self::account_id(),
                amount,
                frame_support::traits::ExistenceRequirement::KeepAlive,
            )?;

            AirdropInfo::<T>::mutate(airdrop_id, |maybe_metadata| {
                if let Some(metadata) = maybe_metadata {
                    metadata.balance = metadata.balance.saturating_add(amount);
                }
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
        #[pallet::weight(T::WeightInfo::claim(merkle_proof.len() as u32))]
        pub fn claim(
            origin: OriginFor<T>,
            airdrop_id: AirdropId,
            recipient: T::AccountId,
            amount: BalanceOf<T>,
            merkle_proof: BoundedVec<MerkleHash, T::MaxProofs>,
        ) -> DispatchResult {
            ensure_none(origin)?;

            ensure!(
                !Claimed::<T>::contains_key(airdrop_id, &recipient),
                Error::<T>::AlreadyClaimed
            );

            let airdrop_metadata =
                AirdropInfo::<T>::get(airdrop_id).ok_or(Error::<T>::AirdropNotFound)?;

            ensure!(
                Self::verify_merkle_proof(
                    &recipient,
                    amount,
                    &airdrop_metadata.merkle_root,
                    &merkle_proof
                ),
                Error::<T>::InvalidProof
            );

            ensure!(
                airdrop_metadata.balance >= amount,
                Error::<T>::InsufficientAirdropBalance
            );

            // Mark as claimed before performing the transfer
            Claimed::<T>::insert(airdrop_id, &recipient, ());

            AirdropInfo::<T>::mutate(airdrop_id, |maybe_metadata| {
                if let Some(metadata) = maybe_metadata {
                    metadata.balance = metadata.balance.saturating_sub(amount);
                }
            });

            let per_block = if let Some(vesting_period) = airdrop_metadata.vesting_period {
                amount
                    .checked_div(&T::BlockNumberToBalance::convert(vesting_period))
                    .ok_or_else(|| Error::<T>::InsufficientAirdropBalance)?
            } else {
                amount
            };

            let current_block = T::BlockNumberProvider::current_block_number();
            let vesting_start =
                current_block.saturating_add(airdrop_metadata.vesting_delay.unwrap_or_default());

            T::Vesting::vested_transfer(
                &Self::account_id(),
                &recipient,
                amount,
                per_block,
                vesting_start,
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

            let airdrop_metadata =
                AirdropInfo::<T>::take(airdrop_id).ok_or(Error::<T>::AirdropNotFound)?;

            ensure!(
                airdrop_metadata.creator == who,
                Error::<T>::NotAirdropCreator
            );

            CurrencyOf::<T>::transfer(
                &Self::account_id(),
                &airdrop_metadata.creator,
                airdrop_metadata.balance,
                frame_support::traits::ExistenceRequirement::KeepAlive,
            )?;

            Self::deposit_event(Event::AirdropDeleted { airdrop_id });

            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            if let Call::claim {
                airdrop_id,
                recipient,
                amount,
                merkle_proof,
            } = call
            {
                // 1. Check if airdrop exists
                let airdrop_metadata = AirdropInfo::<T>::get(airdrop_id).ok_or_else(|| {
                    let error = Error::<T>::AirdropNotFound;
                    InvalidTransaction::Custom(error.to_code())
                })?;

                // 2. Check if already claimed
                if Claimed::<T>::contains_key(airdrop_id, recipient) {
                    let error = Error::<T>::AlreadyClaimed;
                    return InvalidTransaction::Custom(error.to_code()).into();
                }

                // 3. Verify Merkle Proof
                if !Self::verify_merkle_proof(
                    recipient,
                    *amount,
                    &airdrop_metadata.merkle_root,
                    merkle_proof,
                ) {
                    let error = Error::<T>::InvalidProof;
                    return InvalidTransaction::Custom(error.to_code()).into();
                }

                Ok(ValidTransaction {
                    priority: T::UnsignedClaimPriority::get(),
                    requires: vec![],
                    provides: vec![(airdrop_id, recipient, amount).encode()],
                    longevity: TransactionLongevity::MAX,
                    propagate: true,
                })
            } else {
                log::error!(target: "merkle-airdrop", "ValidateUnsigned: Received non-claim transaction or unexpected call structure");
                InvalidTransaction::Call.into()
            }
        }
    }
}
