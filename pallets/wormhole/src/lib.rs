#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use lazy_static::lazy_static;
pub use pallet::*;
use qp_wormhole_verifier::WormholeVerifier;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;
pub use weights::*;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

lazy_static! {
	static ref WORMHOLE_VERIFIER: Option<WormholeVerifier> = {
		let verifier_bytes = include_bytes!("../verifier.bin");
		let common_bytes = include_bytes!("../common.bin");
		WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes).ok()
	};
}

// Add a safe getter function
pub fn get_wormhole_verifier() -> Result<&'static WormholeVerifier, &'static str> {
	WORMHOLE_VERIFIER.as_ref().ok_or("Wormhole verifier not available")
}

#[frame_support::pallet]
pub mod pallet {
	use crate::WeightInfo;
	use alloc::vec::Vec;
	use codec::Decode;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Mutate, Unbalanced},
			Currency, ExistenceRequirement, WithdrawReasons,
		},
		weights::WeightToFee,
	};
	use frame_system::pallet_prelude::*;
	use qp_wormhole::TransferProofs;
	use qp_wormhole_circuit::inputs::PublicCircuitInputs;
	use qp_wormhole_verifier::ProofWithPublicInputs;
	use qp_zk_circuits_common::circuit::{C, D, F};
	use sp_runtime::{
		traits::{Hash as HashT, Header, Saturating, Zero},
		Perbill,
	};

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Currency type used for minting tokens and handling wormhole transfers
		type Currency: Mutate<Self::AccountId, Balance = BalanceOf<Self>>
			+ TransferProofs<BalanceOf<Self>, Self::AccountId>
			+ Unbalanced<Self::AccountId>
			+ Currency<Self::AccountId>;

		/// Account ID used as the "from" account when creating transfer proofs for minted tokens
		#[pallet::constant]
		type MintingAccount: Get<Self::AccountId>;

		/// Weight information for pallet operations.
		type WeightInfo: WeightInfo;

		type WeightToFee: WeightToFee<Balance = BalanceOf<Self>>;
	}

	#[pallet::storage]
	#[pallet::getter(fn used_nullifiers)]
	pub(super) type UsedNullifiers<T: Config> =
		StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		ProofVerified { exit_amount: BalanceOf<T> },
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidProof,
		ProofDeserializationFailed,
		VerificationFailed,
		InvalidPublicInputs,
		NullifierAlreadyUsed,
		VerifierNotAvailable,
		InvalidStorageRoot,
		StorageRootMismatch,
		BlockNotFound,
		InvalidBlockNumber,
		HeaderDecodingFailed,
		HeaderNumberMismatch,
		HeaderHashMismatch,
	}

	impl<T: Config> Pallet<T> {
		fn apply_post_verification(public_inputs: &PublicCircuitInputs) -> DispatchResult {
			// Mark nullifier as used
			let nullifier_bytes = *public_inputs.nullifier;
			UsedNullifiers::<T>::insert(nullifier_bytes, true);

			let exit_balance_u128 = public_inputs.funding_amount;

			// Convert to Balance type
			let exit_balance: BalanceOf<T> =
				exit_balance_u128.try_into().map_err(|_| Error::<T>::InvalidPublicInputs)?;

			// Decode exit account from public inputs
			let exit_account_bytes = *public_inputs.exit_account;
			let exit_account = T::AccountId::decode(&mut &exit_account_bytes[..])
				.map_err(|_| Error::<T>::InvalidPublicInputs)?;

			// Calculate fees first
			let weight = <T as Config>::WeightInfo::verify_wormhole_proof();
			let weight_fee = T::WeightToFee::weight_to_fee(&weight);
			let volume_fee_perbill = Perbill::from_rational(1u32, 1000u32);
			let volume_fee = volume_fee_perbill * exit_balance;
			let total_fee = weight_fee.saturating_add(volume_fee);

			// Mint tokens to the exit account
			// This does not affect total issuance and does not create an imbalance
			<T::Currency as Unbalanced<_>>::increase_balance(
				&exit_account,
				exit_balance.into(),
				frame_support::traits::tokens::Precision::Exact,
			)?;

			// Withdraw fee from exit account if fees are non-zero
			// This creates a negative imbalance that will be handled by the transaction payment
			// pallet
			if !total_fee.is_zero() {
				let _fee_imbalance = T::Currency::withdraw(
					&exit_account,
					total_fee,
					WithdrawReasons::TRANSACTION_PAYMENT,
					ExistenceRequirement::KeepAlive,
				)?;
			}

			// Create a transfer proof for the minted tokens
			let mint_account = T::MintingAccount::get();
			T::Currency::store_transfer_proof(&mint_account, &exit_account, exit_balance);

			// Emit event
			Self::deposit_event(Event::ProofVerified { exit_amount: exit_balance });

			Ok(())
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::verify_wormhole_proof())]
		pub fn verify_wormhole_proof(
			origin: OriginFor<T>,
			proof_bytes: Vec<u8>,
			block_number: BlockNumberFor<T>,
		) -> DispatchResult {
			ensure_none(origin)?;

			let verifier =
				crate::get_wormhole_verifier().map_err(|_| Error::<T>::VerifierNotAvailable)?;

			let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes,
				&verifier.circuit_data.common,
			)
			.map_err(|_| Error::<T>::ProofDeserializationFailed)?;

			// Parse public inputs using the existing parser
			let public_inputs = PublicCircuitInputs::try_from(&proof)
				.map_err(|_| Error::<T>::InvalidPublicInputs)?;

			let nullifier_bytes = *public_inputs.nullifier;

			// Verify nullifier hasn't been used
			ensure!(
				!UsedNullifiers::<T>::contains_key(nullifier_bytes),
				Error::<T>::NullifierAlreadyUsed
			);

			// Get the block hash for the specified block number
			let block_hash = frame_system::Pallet::<T>::block_hash(block_number);

			// Check if block number is not in the future
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(block_number <= current_block, Error::<T>::InvalidBlockNumber);

			// Validate that the block exists by checking if it's not the default hash
			// The default hash (all zeros) indicates the block doesn't exist
			let default_hash = T::Hash::default();
			ensure!(block_hash != default_hash, Error::<T>::BlockNotFound);

			// Get the storage root for the current state (legacy behavior)
			let storage_root = sp_io::storage::root(sp_runtime::StateVersion::V1);

			let root_hash = public_inputs.root_hash;
			let storage_root_bytes = storage_root.as_slice();

			// Compare the root_hash from the proof with the current storage root
			if root_hash.as_ref() != storage_root_bytes {
				log::warn!(
					target: "wormhole",
					"Storage root mismatch for block {:?}: expected {:?}, got {:?}",
					block_number,
					root_hash.as_ref(),
					storage_root_bytes
				);
				return Err(Error::<T>::StorageRootMismatch.into());
			}

			#[cfg(any(test, feature = "runtime-benchmarks"))]
			{
				let _root_hash = root_hash;
				let _storage_root_bytes = storage_root_bytes;
				log::debug!(
					target: "wormhole",
					"Skipping storage root validation in test/benchmark environment"
				);
			}

			verifier.verify(proof.clone()).map_err(|_| Error::<T>::VerificationFailed)?;

			Self::apply_post_verification(&public_inputs)
		}

		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::verify_wormhole_proof())]
		pub fn verify_wormhole_proof_with_header(
			origin: OriginFor<T>,
			proof_bytes: Vec<u8>,
			block_number: BlockNumberFor<T>,
			header_bytes: Vec<u8>,
		) -> DispatchResult {
			ensure_none(origin)?;

			let verifier =
				crate::get_wormhole_verifier().map_err(|_| Error::<T>::VerifierNotAvailable)?;

			// Decode the supplied header
			let header: <<T as frame_system::Config>::Block as sp_runtime::traits::Block>::Header =
				Decode::decode(&mut &header_bytes[..]).map_err(|_| Error::<T>::HeaderDecodingFailed)?;

			// Check if block number is not in the future
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(block_number <= current_block, Error::<T>::InvalidBlockNumber);

			// Validate that the block exists by checking stored block hash ring buffer
			let expected_hash = frame_system::Pallet::<T>::block_hash(block_number);
			let default_hash = T::Hash::default();
			ensure!(expected_hash != default_hash, Error::<T>::BlockNotFound);

			// Cross-check header number
			ensure!(*header.number() == block_number, Error::<T>::HeaderNumberMismatch);

			// Cross-check header hash equals stored hash
			let provided_hash = header.hash();
			ensure!(provided_hash == expected_hash, Error::<T>::HeaderHashMismatch);

			let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes,
				&verifier.circuit_data.common,
			)
			.map_err(|_| Error::<T>::ProofDeserializationFailed)?;

			// Parse public inputs using the existing parser
			let public_inputs = PublicCircuitInputs::try_from(&proof)
				.map_err(|_| Error::<T>::InvalidPublicInputs)?;

			// Verify nullifier hasn't been used
			let nullifier_bytes = *public_inputs.nullifier;
			ensure!(
				!UsedNullifiers::<T>::contains_key(nullifier_bytes),
				Error::<T>::NullifierAlreadyUsed
			);

			// Compare root from public inputs with the authenticated header state root
			let root_hash = public_inputs.root_hash;
			let header_root_bytes = header.state_root().as_ref();
			if root_hash.as_ref() != header_root_bytes {
				log::warn!(
					target: "wormhole",
					"Storage root mismatch for block {:?}: expected {:?}, got {:?}",
					block_number,
					header_root_bytes,
					root_hash.as_ref()
				);
				return Err(Error::<T>::StorageRootMismatch.into());
			}

			verifier.verify(proof.clone()).map_err(|_| Error::<T>::VerificationFailed)?;

			Self::apply_post_verification(&public_inputs)
		}
	}
}
