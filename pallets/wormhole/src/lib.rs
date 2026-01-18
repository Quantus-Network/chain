#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use core::marker::PhantomData;

use codec::{Decode, MaxEncodedLen};
use frame_support::StorageHasher;
use lazy_static::lazy_static;
pub use pallet::*;
pub use qp_poseidon::{PoseidonHasher as PoseidonCore, ToFelts};
use qp_wormhole_verifier::WormholeVerifier;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;
use sp_metadata_ir::StorageHasherIR;
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

// We use a generic struct so we can pass the specific Key type to the hasher
pub struct PoseidonStorageHasher<Key>(PhantomData<Key>);

impl<Key: Decode + ToFelts + 'static> StorageHasher for PoseidonStorageHasher<Key> {
	// We are lying here, but maybe it's ok because it's just metadata
	const METADATA: StorageHasherIR = StorageHasherIR::Identity;
	type Output = [u8; 32];

	fn hash(x: &[u8]) -> Self::Output {
		PoseidonCore::hash_storage::<Key>(x)
	}

	fn max_len<K: MaxEncodedLen>() -> usize {
		32
	}
}

#[frame_support::pallet]
pub mod pallet {
	use crate::{PoseidonStorageHasher, ToFelts, WeightInfo};
	use alloc::vec::Vec;
	use codec::Decode;
	use frame_support::{
		dispatch::DispatchResult,
		pallet_prelude::*,
		traits::{
			fungible::{Mutate, Unbalanced},
			fungibles::{self, Inspect as FungiblesInspect, Mutate as FungiblesMutate},
			tokens::Preservation,
			Currency, ExistenceRequirement, WithdrawReasons,
		},
		weights::WeightToFee,
	};
	use frame_system::pallet_prelude::*;
	use qp_wormhole_circuit::inputs::PublicCircuitInputs;
	use qp_wormhole_verifier::ProofWithPublicInputs;
	use qp_zk_circuits_common::circuit::{C, D, F};
	use sp_runtime::{
		traits::{MaybeDisplay, Saturating, StaticLookup, Zero},
		Perbill,
	};

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
	pub type AssetIdOf<T> = <<T as Config>::Assets as fungibles::Inspect<
		<T as frame_system::Config>::AccountId,
	>>::AssetId;
	pub type AssetBalanceOf<T> = <<T as Config>::Assets as fungibles::Inspect<
		<T as frame_system::Config>::AccountId,
	>>::Balance;
	pub type AccountIdLookupOf<T> = <<T as frame_system::Config>::Lookup as StaticLookup>::Source;

	pub type TransferProofKey<T> = (
		AssetIdOf<T>,
		<T as Config>::TransferCount,
		<T as Config>::WormholeAccountId,
		<T as Config>::WormholeAccountId,
		BalanceOf<T>,
	);

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config
	where
		AssetIdOf<Self>: Default + From<u32> + Clone + ToFelts,
		BalanceOf<Self>: Default + ToFelts,
		AssetBalanceOf<Self>: Into<BalanceOf<Self>> + From<BalanceOf<Self>>,
	{
		/// Currency type used for native token transfers and minting
		type Currency: Mutate<<Self as frame_system::Config>::AccountId, Balance = BalanceOf<Self>>
			+ Unbalanced<<Self as frame_system::Config>::AccountId>
			+ Currency<<Self as frame_system::Config>::AccountId>;

		/// Assets type used for managing fungible assets
		type Assets: fungibles::Inspect<<Self as frame_system::Config>::AccountId>
			+ fungibles::Mutate<<Self as frame_system::Config>::AccountId>
			+ fungibles::Create<<Self as frame_system::Config>::AccountId>;

		/// Transfer count type used in storage
		type TransferCount: Parameter
			+ MaxEncodedLen
			+ Default
			+ Saturating
			+ Copy
			+ sp_runtime::traits::One
			+ ToFelts;

		/// Account ID used as the "from" account when creating transfer proofs for minted tokens
		#[pallet::constant]
		type MintingAccount: Get<<Self as frame_system::Config>::AccountId>;

		/// Weight information for pallet operations.
		type WeightInfo: WeightInfo;

		type WeightToFee: WeightToFee<Balance = BalanceOf<Self>>;

		/// Override system AccountId to make it felts encodable
		type WormholeAccountId: Parameter
			+ Member
			+ MaybeSerializeDeserialize
			+ core::fmt::Debug
			+ MaybeDisplay
			+ Ord
			+ MaxEncodedLen
			+ ToFelts
			+ Into<<Self as frame_system::Config>::AccountId>
			+ From<<Self as frame_system::Config>::AccountId>;
	}

	#[pallet::storage]
	#[pallet::getter(fn used_nullifiers)]
	pub(super) type UsedNullifiers<T: Config> =
		StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

	/// Transfer proofs for wormhole transfers (both native and assets)
	#[pallet::storage]
	#[pallet::getter(fn transfer_proof)]
	pub type TransferProof<T: Config> = StorageMap<
		_,
		PoseidonStorageHasher<TransferProofKey<T>>,
		TransferProofKey<T>,
		(),
		OptionQuery,
	>;

	/// Transfer count for all wormhole transfers
	#[pallet::storage]
	#[pallet::getter(fn transfer_count)]
	pub type TransferCount<T: Config> = StorageValue<_, T::TransferCount, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		NativeTransferred {
			from: <T as frame_system::Config>::AccountId,
			to: <T as frame_system::Config>::AccountId,
			amount: BalanceOf<T>,
			transfer_count: T::TransferCount,
		},
		AssetTransferred {
			asset_id: AssetIdOf<T>,
			from: <T as frame_system::Config>::AccountId,
			to: <T as frame_system::Config>::AccountId,
			amount: AssetBalanceOf<T>,
			transfer_count: T::TransferCount,
		},
		ProofVerified {
			exit_amount: BalanceOf<T>,
		},
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
		AssetNotFound,
		SelfTransfer,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::verify_wormhole_proof())]
		pub fn verify_wormhole_proof(origin: OriginFor<T>, proof_bytes: Vec<u8>) -> DispatchResult {
			ensure_none(origin)?;
			// Note: The funding_amount in public inputs is expected to be quantized (i.e., scaled
			// down from 12 to 2 decimals points of precision) so we need to scale it back up
			// here to get the actual amount the chain expects with 12 decimal places of precision.
			const SCALE_DOWN_FACTOR: u128 = 10_000_000_000; // 10^10;

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

			// Extract the block number from public inputs
			let block_number = BlockNumberFor::<T>::try_from(public_inputs.block_number)
				.map_err(|_| Error::<T>::InvalidPublicInputs)?;

			// Get the block hash for the specified block number
			let block_hash = frame_system::Pallet::<T>::block_hash(block_number);

			// Check if block number is not in the future
			let current_block = frame_system::Pallet::<T>::block_number();
			ensure!(block_number <= current_block, Error::<T>::InvalidBlockNumber);

			// Validate that the block exists by checking if it's not the default hash
			// The default hash (all zeros) indicates the block doesn't exist
			let default_hash = T::Hash::default();
			ensure!(block_hash != default_hash, Error::<T>::BlockNotFound);

			// Ensure that the block hash from storage matches the one in public inputs
			ensure!(
				block_hash.as_ref() == public_inputs.block_hash.as_ref(),
				Error::<T>::InvalidPublicInputs
			);

			verifier.verify(proof.clone()).map_err(|_| Error::<T>::VerificationFailed)?;

			// Mark nullifier as used
			UsedNullifiers::<T>::insert(nullifier_bytes, true);

			let exit_balance_u128 =
				(public_inputs.funding_amount as u128).saturating_mul(SCALE_DOWN_FACTOR);

			// Convert to Balance type
			let exit_balance: BalanceOf<T> =
				exit_balance_u128.try_into().map_err(|_| Error::<T>::InvalidPublicInputs)?;

			// Decode exit account from public inputs
			let exit_account_bytes = *public_inputs.exit_account;
			let exit_account =
				<T as frame_system::Config>::AccountId::decode(&mut &exit_account_bytes[..])
					.map_err(|_| Error::<T>::InvalidPublicInputs)?;

			// Extract asset_id from public inputs
			let asset_id_u32 = public_inputs.asset_id;
			let asset_id: AssetIdOf<T> = asset_id_u32.into();

			// Calculate fees first
			let weight = <T as Config>::WeightInfo::verify_wormhole_proof();
			let weight_fee = T::WeightToFee::weight_to_fee(&weight);
			let volume_fee_perbill = Perbill::from_rational(1u32, 1000u32);
			let volume_fee = volume_fee_perbill * exit_balance;
			let total_fee = weight_fee.saturating_add(volume_fee);

			// Handle native (asset_id = 0) or asset transfers
			if asset_id == AssetIdOf::<T>::default() {
				// Native token transfer
				// Mint tokens to the exit account
				// This does not affect total issuance and does not create an imbalance
				<T::Currency as Unbalanced<_>>::increase_balance(
					&exit_account,
					exit_balance,
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
			} else {
				// Asset transfer
				let asset_balance: AssetBalanceOf<T> = exit_balance.into();
				<T::Assets as FungiblesMutate<_>>::mint_into(
					asset_id.clone(),
					&exit_account,
					asset_balance,
				)?;

				// For assets, we still need to charge fees in native currency
				// The exit account must have enough native balance to pay fees
				if !total_fee.is_zero() {
					let _fee_imbalance = T::Currency::withdraw(
						&exit_account,
						total_fee,
						WithdrawReasons::TRANSACTION_PAYMENT,
						ExistenceRequirement::AllowDeath,
					)?;
				}
			}

			// Create a transfer proof for the minted tokens
			let mint_account = T::MintingAccount::get();
			Self::record_transfer(
				asset_id,
				mint_account.into(),
				exit_account.into(),
				exit_balance,
			)?;

			// Emit event
			Self::deposit_event(Event::ProofVerified { exit_amount: exit_balance });

			Ok(())
		}

		/// Transfer native tokens and store proof for wormhole
		#[pallet::call_index(1)]
		#[pallet::weight(T::DbWeight::get().reads_writes(1, 2))]
		pub fn transfer_native(
			origin: OriginFor<T>,
			dest: AccountIdLookupOf<T>,
			#[pallet::compact] amount: BalanceOf<T>,
		) -> DispatchResult {
			let source = ensure_signed(origin)?;
			let dest = T::Lookup::lookup(dest)?;

			// Prevent self-transfers
			ensure!(source != dest, Error::<T>::SelfTransfer);

			// Perform the transfer
			<T::Currency as Mutate<_>>::transfer(&source, &dest, amount, Preservation::Expendable)?;

			// Store proof with asset_id = Default (0 for native)
			Self::record_transfer(AssetIdOf::<T>::default(), source.into(), dest.into(), amount)?;

			Ok(())
		}

		/// Transfer asset tokens and store proof for wormhole
		#[pallet::call_index(2)]
		#[pallet::weight(T::DbWeight::get().reads_writes(2, 2))]
		pub fn transfer_asset(
			origin: OriginFor<T>,
			asset_id: AssetIdOf<T>,
			dest: AccountIdLookupOf<T>,
			#[pallet::compact] amount: AssetBalanceOf<T>,
		) -> DispatchResult {
			let source = ensure_signed(origin)?;
			let dest = T::Lookup::lookup(dest)?;

			// Prevent self-transfers
			ensure!(source != dest, Error::<T>::SelfTransfer);

			// Check if asset exists
			ensure!(
				<T::Assets as FungiblesInspect<_>>::asset_exists(asset_id.clone()),
				Error::<T>::AssetNotFound
			);

			// Perform the transfer
			<T::Assets as fungibles::Mutate<_>>::transfer(
				asset_id.clone(),
				&source,
				&dest,
				amount,
				Preservation::Expendable,
			)?;

			// Store proof
			Self::record_transfer(asset_id, source.into(), dest.into(), amount.into())?;

			Ok(())
		}
	}

	// Helper functions for recording transfer proofs
	impl<T: Config> Pallet<T> {
		/// Record a transfer proof
		/// This should be called by transaction extensions or other runtime components
		pub fn record_transfer(
			asset_id: AssetIdOf<T>,
			from: <T as Config>::WormholeAccountId,
			to: <T as Config>::WormholeAccountId,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			let current_count = TransferCount::<T>::get();
			TransferProof::<T>::insert(
				(asset_id.clone(), current_count, from.clone(), to.clone(), amount),
				(),
			);
			TransferCount::<T>::put(current_count.saturating_add(T::TransferCount::one()));

			if asset_id == AssetIdOf::<T>::default() {
				Self::deposit_event(Event::<T>::NativeTransferred {
					from: from.into(),
					to: to.into(),
					amount,
					transfer_count: current_count,
				});
			} else {
				Self::deposit_event(Event::<T>::AssetTransferred {
					from: from.into(),
					to: to.into(),
					asset_id,
					amount: amount.into(),
					transfer_count: current_count,
				});
			}

			Ok(())
		}
	}

	// Implement the TransferProofRecorder trait for other pallets to use
	impl<T: Config>
		qp_wormhole::TransferProofRecorder<
			<T as Config>::WormholeAccountId,
			AssetIdOf<T>,
			BalanceOf<T>,
		> for Pallet<T>
	{
		type Error = DispatchError;

		fn record_transfer_proof(
			asset_id: Option<AssetIdOf<T>>,
			from: <T as Config>::WormholeAccountId,
			to: <T as Config>::WormholeAccountId,
			amount: BalanceOf<T>,
		) -> Result<(), Self::Error> {
			let asset_id_value = asset_id.unwrap_or_default();
			Self::record_transfer(asset_id_value, from, to, amount)
		}
	}
}
