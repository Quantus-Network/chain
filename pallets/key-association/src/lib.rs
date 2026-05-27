//! # Key Association Pallet
//!
//! This pallet allows users to associate classical cryptographic keys (ECDSA secp256k1, Ed25519)
//! with their post-quantum ML-DSA-87 accounts on Quantus.
//!
//! ## Purpose
//!
//! Forward migration: Users with existing classical key wallets (Ethereum, Polkadot, etc.)
//! can cryptographically prove ownership and link those identities to their Quantus account.
//!
//! ## Features
//!
//! - Associate multiple classical keys with a single ML-DSA-87 account
//! - On-chain signature verification using sp-core primitives
//! - Block-hash-based replay protection (unpredictable challenge)
//! - Reverse index for looking up which ML-DSA account owns a classical key
//!
//! ## Design Notes
//!
//! - Associations are permanent (no disassociation by design)
//! - Each classical key can only be associated with one ML-DSA account
//! - Signature validity window derived from `frame_system::BlockHashCount`

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod types;
pub mod weights;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub use pallet::*;
pub use types::*;
pub use weights::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use sp_io::hashing::blake2_128;
	use sp_runtime::{traits::Verify, Saturating};

	/// The in-code storage version.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Maximum number of classical keys that can be associated with a single ML-DSA account.
		#[pallet::constant]
		type MaxAssociations: Get<u32>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	/// ML-DSA account -> list of associated classical keys with metadata.
	#[pallet::storage]
	#[pallet::getter(fn associations)]
	pub type Associations<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		BoundedVec<(ClassicalKey, AssociationRecord<BlockNumberFor<T>>), T::MaxAssociations>,
		ValueQuery,
	>;

	/// Reverse index: Blake2-128 hash of ClassicalKey -> ML-DSA account.
	///
	/// This enables efficient lookups of which ML-DSA account owns a given classical key.
	#[pallet::storage]
	#[pallet::getter(fn key_index)]
	pub type KeyIndex<T: Config> = StorageMap<_, Blake2_128Concat, [u8; 16], T::AccountId, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A classical key was associated with an ML-DSA-87 account.
		KeyAssociated {
			/// The ML-DSA-87 account that now owns this classical key association.
			account: T::AccountId,
			/// The type of classical key (ECDSA or Ed25519).
			key_type: KeyType,
			/// Blake2-128 hash of the classical key (for indexing).
			key_hash: [u8; 16],
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// The signature type does not match the key type.
		SignatureKeyMismatch,
		/// Signature verification failed.
		InvalidSignature,
		/// The block hash does not match the hash at the given block number.
		BlockHashMismatch,
		/// The signed block is too old (outside validity window).
		SignatureExpired,
		/// This classical key is already associated with an account.
		KeyAlreadyAssociated,
		/// The account has reached the maximum number of key associations.
		TooManyAssociations,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Associate a classical key with the caller's ML-DSA-87 account.
		///
		/// The caller must provide:
		/// - `classical_key`: The public key to associate
		/// - `signature`: A signature from that key over the challenge message
		/// - `signed_block_number`: The block number whose hash was signed
		/// - `signed_block_hash`: The block hash that was included in the signed message
		///
		/// ## Challenge Message Format
		///
		/// The message that must be signed is:
		/// ```text
		/// Quantus Key Association
		/// Account: <scale-encoded account>
		/// Key: <scale-encoded classical key>
		/// Block: <block hash bytes>
		/// ```
		///
		/// ## Replay Protection
		///
		/// The signed block must be within `BlockHashCount` blocks of the current block.
		/// The provided block hash must match the on-chain hash at that block number.
		///
		/// ## Errors
		///
		/// - `BlockHashMismatch`: The hash doesn't match the on-chain hash at that block
		/// - `SignatureExpired`: The block is older than the validity window
		/// - `SignatureKeyMismatch`: Signature type doesn't match key type
		/// - `InvalidSignature`: Signature verification failed
		/// - `KeyAlreadyAssociated`: This classical key is already linked to an account
		/// - `TooManyAssociations`: Account has reached `MaxAssociations` limit
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::associate())]
		pub fn associate(
			origin: OriginFor<T>,
			classical_key: ClassicalKey,
			signature: ClassicalSignature,
			signed_block_number: BlockNumberFor<T>,
			signed_block_hash: T::Hash,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// 1. Verify block hash matches the on-chain hash at the given block number
			let actual_hash = frame_system::Pallet::<T>::block_hash(signed_block_number);
			ensure!(actual_hash == signed_block_hash, Error::<T>::BlockHashMismatch);

			// 2. Verify block is within validity window (derived from BlockHashCount)
			let current_block = frame_system::Pallet::<T>::block_number();
			let validity_window = T::BlockHashCount::get();

			ensure!(
				current_block.saturating_sub(signed_block_number) < validity_window,
				Error::<T>::SignatureExpired
			);

			// 3. Build challenge message
			let message = Self::challenge_message(&who, &classical_key, &signed_block_hash);

			// 4. Verify signature
			Self::verify_signature(&classical_key, &signature, &message)?;

			// 5. Check key is not already associated
			let key_hash = Self::hash_key(&classical_key);
			ensure!(!KeyIndex::<T>::contains_key(key_hash), Error::<T>::KeyAlreadyAssociated);

			// 6. Add to associations (enforces MaxAssociations bound)
			Associations::<T>::try_mutate(&who, |associations| {
				associations
					.try_push((
						classical_key.clone(),
						AssociationRecord { created_at: current_block },
					))
					.map_err(|_| Error::<T>::TooManyAssociations)
			})?;

			// 7. Add reverse index
			KeyIndex::<T>::insert(key_hash, &who);

			// 8. Emit event
			Self::deposit_event(Event::KeyAssociated {
				account: who,
				key_type: classical_key.key_type(),
				key_hash,
			});

			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// Build the challenge message that must be signed by the classical key.
		///
		/// Format is human-readable for hardware wallet compatibility:
		/// ```text
		/// Quantus Key Association
		/// Account: <scale-encoded account>
		/// Key: <scale-encoded classical key>
		/// Block: <block hash bytes>
		/// ```
		fn challenge_message(
			account: &T::AccountId,
			classical_key: &ClassicalKey,
			block_hash: &T::Hash,
		) -> alloc::vec::Vec<u8> {
			use codec::Encode;

			let mut msg = b"Quantus Key Association\n".to_vec();
			msg.extend_from_slice(b"Account: ");
			msg.extend_from_slice(&account.encode());
			msg.extend_from_slice(b"\nKey: ");
			msg.extend_from_slice(&classical_key.encode());
			msg.extend_from_slice(b"\nBlock: ");
			msg.extend_from_slice(block_hash.as_ref());
			msg
		}

		/// Verify a classical signature over a message.
		fn verify_signature(
			key: &ClassicalKey,
			signature: &ClassicalSignature,
			message: &[u8],
		) -> Result<(), Error<T>> {
			match (key, signature) {
				(ClassicalKey::Ecdsa(pub_key), ClassicalSignature::Ecdsa(sig)) => {
					ensure!(sig.verify(message, pub_key), Error::<T>::InvalidSignature);
				}
				(ClassicalKey::Ed25519(pub_key), ClassicalSignature::Ed25519(sig)) => {
					ensure!(sig.verify(message, pub_key), Error::<T>::InvalidSignature);
				}
				_ => return Err(Error::<T>::SignatureKeyMismatch),
			}
			Ok(())
		}

		/// Compute Blake2-128 hash of a classical key for indexing.
		fn hash_key(key: &ClassicalKey) -> [u8; 16] {
			use codec::Encode;
			blake2_128(&key.encode())
		}

		// ==================== Public Read APIs ====================

		/// Get all classical keys associated with an ML-DSA account.
		pub fn associations_for(
			account: &T::AccountId,
		) -> alloc::vec::Vec<(ClassicalKey, AssociationRecord<BlockNumberFor<T>>)> {
			Associations::<T>::get(account).into_inner()
		}

		/// Look up which ML-DSA account owns a classical key.
		pub fn account_for_key(key: &ClassicalKey) -> Option<T::AccountId> {
			let key_hash = Self::hash_key(key);
			KeyIndex::<T>::get(key_hash)
		}

		/// Check if a classical key is already associated with any account.
		pub fn is_key_associated(key: &ClassicalKey) -> bool {
			let key_hash = Self::hash_key(key);
			KeyIndex::<T>::contains_key(key_hash)
		}

		/// Compute the key hash for a given classical key (useful for clients).
		pub fn compute_key_hash(key: &ClassicalKey) -> [u8; 16] {
			Self::hash_key(key)
		}
	}
}
