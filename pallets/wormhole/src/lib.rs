#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use lazy_static::lazy_static;
pub use pallet::*;
pub use qp_poseidon::{PoseidonHasher as PoseidonCore, ToFelts};
use qp_wormhole_verifier::WormholeVerifier;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;
pub use weights::*;

lazy_static! {
	static ref AGGREGATED_VERIFIER: Option<WormholeVerifier> = {
		let verifier_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/aggregated_verifier.bin"));
		let common_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/aggregated_common.bin"));
		WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes).ok()
	};
}

/// Getter for the aggregated proof verifier
pub fn get_aggregated_verifier() -> Result<&'static WormholeVerifier, &'static str> {
	AGGREGATED_VERIFIER.as_ref().ok_or("Aggregated verifier not available")
}

/// Scale factor for quantizing amounts from 12 to 2 decimal places (10^10).
/// Amounts in the circuit are stored as u32 with 2 decimal places of precision.
/// On-chain amounts use 12 decimal places, so we multiply by this factor when
/// converting from circuit amounts to on-chain amounts.
pub const SCALE_DOWN_FACTOR: u128 = 10_000_000_000;

#[frame_support::pallet]
pub mod pallet {
	use crate::{PoseidonCore, ToFelts, WeightInfo};
	use alloc::vec::Vec;
	use codec::{Decode, Encode};
	use frame_support::{
		dispatch::DispatchResult,
		pallet_prelude::*,
		traits::{
			fungible::{Inspect as FungibleInspect, Mutate, Unbalanced},
			fungibles::{self},
			Currency,
		},
	};
	use frame_system::pallet_prelude::*;
	use qp_wormhole_verifier::{
		parse_aggregated_public_inputs, AggregatedPublicCircuitInputs, ProofWithPublicInputs, C, D,
		F,
	};
	use sp_runtime::{
		traits::{MaybeDisplay, Saturating, Zero},
		transaction_validity::{
			InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
			ValidTransaction,
		},
		Permill,
	};

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
	pub type AssetIdOf<T> = <<T as Config>::Assets as fungibles::Inspect<
		<T as frame_system::Config>::AccountId,
	>>::AssetId;
	pub type AssetBalanceOf<T> = <<T as Config>::Assets as fungibles::Inspect<
		<T as frame_system::Config>::AccountId,
	>>::Balance;

	/// Key for TransferProof storage - uniquely identifies a transfer.
	/// Uses (to, transfer_count) since transfer_count is atomic per recipient.
	/// This is hashed with Blake2_256 to form the storage key suffix.
	pub type TransferProofKey<T> = (<T as Config>::WormholeAccountId, <T as Config>::TransferCount);

	/// Full transfer data including amount - used to compute the leaf_inputs_hash via Poseidon2.
	/// This is what the ZK circuit verifies.
	pub type TransferProofData<T> = (
		AssetIdOf<T>,
		<T as Config>::TransferCount,
		<T as Config>::WormholeAccountId,
		<T as Config>::WormholeAccountId,
		BalanceOf<T>,
	);

	/// The leaf_inputs_hash stored as the value in TransferProof storage.
	/// This is the Poseidon2 hash of TransferProofData, verified by the ZK circuit.
	pub type LeafInputsHash = [u8; 32];

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

		/// Minimum transfer amount required for wormhole transfers.
		/// This prevents dust transfers that waste storage.
		#[pallet::constant]
		type MinimumTransferAmount: Get<BalanceOf<Self>>;

		/// Volume fee rate in basis points (1 basis point = 0.01%).
		/// This must match the fee rate used in proof generation.
		#[pallet::constant]
		type VolumeFeeRateBps: Get<u32>;

		/// Proportion of volume fees to burn (not mint). The remainder goes to the block author.
		/// Example: Permill::from_percent(50) means 50% burned, 50% to miner.
		#[pallet::constant]
		type VolumeFeesBurnRate: Get<Permill>;

		/// Weight information for pallet operations.
		type WeightInfo: WeightInfo;

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

	/// Transfer proofs for wormhole transfers (both native and assets).
	///
	/// Storage key: Twox128("Wormhole") || Twox128("TransferProof") || Blake2_256(to,
	/// transfer_count) Storage value: leaf_inputs_hash (Poseidon2 hash of full transfer data)
	///
	/// The key uses only (to, transfer_count) since transfer_count is atomic per recipient.
	/// The ZK circuit verifies that the leaf_inputs_hash in the value section matches
	/// the Poseidon2 hash of all transfer details (asset_id, count, from, to, amount),
	/// providing full 256-bit security.
	#[pallet::storage]
	#[pallet::getter(fn transfer_proof)]
	pub type TransferProof<T: Config> =
		StorageMap<_, Blake2_256, TransferProofKey<T>, LeafInputsHash, OptionQuery>;

	/// Transfer count for all wormhole transfers
	#[pallet::storage]
	#[pallet::getter(fn transfer_count)]
	pub type TransferCount<T: Config> =
		StorageMap<_, Blake2_128Concat, T::WormholeAccountId, T::TransferCount, ValueQuery>;

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
			nullifiers: Vec<[u8; 32]>,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidPublicInputs,
		NullifierAlreadyUsed,
		VerifierNotAvailable,
		BlockNotFound,
		AggregatedVerifierNotAvailable,
		AggregatedProofDeserializationFailed,
		AggregatedVerificationFailed,
		InvalidAggregatedPublicInputs,
		/// The volume fee rate in the proof doesn't match the configured rate
		InvalidVolumeFeeRate,
		/// Transfer amount is below the minimum required
		TransferAmountBelowMinimum,
		/// Only native asset (asset_id = 0) is supported in this version
		NonNativeAssetNotSupported,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Verify an aggregated wormhole proof and process all transfers in the batch
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::verify_aggregated_proof())]
		pub fn verify_aggregated_proof(
			origin: OriginFor<T>,
			proof_bytes: Vec<u8>,
		) -> DispatchResult {
			ensure_none(origin)?;

			let (proof, aggregated_inputs) = Self::pre_validate_proof(&proof_bytes)?;

			// Mark nullifiers as used (pre_validate_proof only checks existence)
			let mut nullifier_list = Vec::<[u8; 32]>::new();
			for nullifier in &aggregated_inputs.nullifiers {
				let nullifier_bytes: [u8; 32] = (*nullifier)
					.as_ref()
					.try_into()
					.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;
				UsedNullifiers::<T>::insert(nullifier_bytes, true);
				nullifier_list.push(nullifier_bytes);
			}

			let verifier = crate::get_aggregated_verifier()
				.map_err(|_| Error::<T>::AggregatedVerifierNotAvailable)?;
			verifier.verify(proof.clone()).map_err(|e| {
				log::error!("Aggregated proof verification failed: {:?}", e);
				Error::<T>::AggregatedVerificationFailed
			})?;

			// Get the minting account for recording transfer proofs
			let mint_account = T::MintingAccount::get();

			// First pass: compute total exit amount and prepare account data
			let mut total_exit_amount: BalanceOf<T> = Zero::zero();
			let mut processed_accounts: Vec<(
				<T as frame_system::Config>::AccountId,
				BalanceOf<T>,
			)> = Vec::with_capacity(aggregated_inputs.account_data.len());

			for (idx, account_data) in aggregated_inputs.account_data.iter().enumerate() {
				// Skip dummy account slots (exit_account == 0 with zero amount)
				// Dummy proofs from aggregation padding have all-zero exit accounts
				// Also skip deduplicated slots (the circuit zeros out duplicate exit accounts)
				let exit_account_bytes: [u8; 32] =
					(*account_data.exit_account).as_ref().try_into().map_err(|e| {
						log::error!("Failed to convert exit_account at idx {}: {:?}", idx, e);
						Error::<T>::InvalidAggregatedPublicInputs
					})?;

				if exit_account_bytes == [0u8; 32] || account_data.summed_output_amount == 0 {
					continue;
				}

				// Convert output amount to Balance type (scale up from quantized value)
				let exit_balance_u128 = (account_data.summed_output_amount as u128)
					.saturating_mul(crate::SCALE_DOWN_FACTOR);
				let exit_balance: BalanceOf<T> = exit_balance_u128.try_into().map_err(|_| {
					log::error!("Failed to convert exit_balance at idx {}", idx);
					Error::<T>::InvalidAggregatedPublicInputs
				})?;

				// Decode exit account from public inputs
				let exit_account =
					<T as frame_system::Config>::AccountId::decode(&mut &exit_account_bytes[..])
						.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;

				total_exit_amount = total_exit_amount.saturating_add(exit_balance);
				processed_accounts.push((exit_account, exit_balance));
			}

			// Ensure total exit amount meets the minimum transfer requirement
			ensure!(
				total_exit_amount >= T::MinimumTransferAmount::get(),
				Error::<T>::TransferAmountBelowMinimum
			);

			// Emit event for each exit account
			Self::deposit_event(Event::ProofVerified {
				exit_amount: total_exit_amount,
				nullifiers: nullifier_list,
			});

			// Compute the total fee from the input amounts
			// fee = total_output_amount * volume_fee_bps / (10000 - volume_fee_bps)
			// This is the fee that was deducted from input to get output.
			let fee_bps = T::VolumeFeeRateBps::get() as u128;
			let total_exit_u128: u128 = total_exit_amount.try_into().map_err(|_| {
				log::error!("Failed to convert total_exit_amount to u128");
				Error::<T>::InvalidAggregatedPublicInputs
			})?;
			let total_fee_u128 = total_exit_u128
				.saturating_mul(fee_bps)
				.checked_div(10000u128.saturating_sub(fee_bps))
				.unwrap_or(0);

			// Fee distribution: configurable portion burned, remainder to miner
			//
			// Original deposit locked `input_amount` in an unspendable account (tokens still
			// exist). On exit we mint `output_amount` to user, where: input >= output + fee
			//
			// Fee split (controlled by VolumeFeesBurnRate):
			//   - burn_amount = fee * burn_rate  (reduces total issuance via Currency::burn)
			//   - miner_fee = fee - burn_amount  (minted to block author via increase_balance)
			//
			// Supply accounting:
			//   - Minting exit amounts: increases balances but NOT issuance by sum(output_amounts)
			//   - Minting miner fee: increases balance but NOT issuance (increase_balance)
			//   - Burning: decreases total issuance by burn_amount
			//   - Net change: +sum(output_amounts) - burn_amount
			let burn_rate = T::VolumeFeesBurnRate::get();
			let mut burn_amount_u128 = burn_rate * total_fee_u128;
			let miner_fee_u128 = total_fee_u128.saturating_sub(burn_amount_u128);
			let miner_fee: BalanceOf<T> = miner_fee_u128.try_into().map_err(|_| {
				log::error!("Failed to convert miner_fee_u128 to BalanceOf");
				Error::<T>::InvalidAggregatedPublicInputs
			})?;

			// Mint miner's portion of volume fee to block author
			// If no author is found, add to burn amount instead of silently losing it
			if !miner_fee.is_zero() {
				let digest = frame_system::Pallet::<T>::digest();
				if let Some(author) = qp_wormhole::extract_author_from_digest::<
					<T as frame_system::Config>::AccountId,
					_,
				>(digest.logs.iter().cloned())
				{
					<T::Currency as Unbalanced<_>>::increase_balance(
						&author,
						miner_fee,
						frame_support::traits::tokens::Precision::Exact,
					)?;
				} else {
					// No block author found - add miner fee to burn amount
					log::warn!(
						"No block author found, burning miner fee of {:?} instead",
						miner_fee
					);
					burn_amount_u128 = burn_amount_u128.saturating_add(miner_fee_u128);
				}
			}

			// Burn the total burn amount (base burn + any orphaned miner fee)
			let burn_amount: BalanceOf<T> = burn_amount_u128.try_into().map_err(|_| {
				log::error!("Failed to convert burn_amount_u128 to BalanceOf");
				Error::<T>::InvalidAggregatedPublicInputs
			})?;
			if !burn_amount.is_zero() {
				let current = <T::Currency as FungibleInspect<_>>::total_issuance();
				<T::Currency as Unbalanced<_>>::set_total_issuance(
					current.saturating_sub(burn_amount),
				);
			}

			// Process transfers and record proofs
			for (exit_account, exit_balance) in &processed_accounts {
				// Native token transfer - mint tokens to the exit account
				<T::Currency as Unbalanced<_>>::increase_balance(
					exit_account,
					*exit_balance,
					frame_support::traits::tokens::Precision::Exact,
				)?;

				// Record transfer proof for the minted tokens
				Self::record_transfer(
					AssetIdOf::<T>::default(),
					mint_account.clone().into(),
					exit_account.clone().into(),
					*exit_balance,
				);
			}

			Ok(())
		}
	}

	#[pallet::validate_unsigned]
	impl<T: Config> ValidateUnsigned for Pallet<T>
	where
		BalanceOf<T>: Default + ToFelts,
		AssetBalanceOf<T>: Into<BalanceOf<T>> + From<BalanceOf<T>>,
	{
		type Call = Call<T>;

		fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			match call {
				Call::verify_aggregated_proof { proof_bytes } => {
					Self::pre_validate_proof(proof_bytes).map_err(|_| InvalidTransaction::Call)?;
					ValidTransaction::with_tag_prefix("WormholeAggregatedVerify")
						.and_provides(sp_io::hashing::blake2_256(proof_bytes))
						.priority(TransactionPriority::MAX / 2)
						.longevity(5)
						.propagate(true)
						.build()
				},
				_ => InvalidTransaction::Call.into(),
			}
		}
	}

	impl<T: Config> Pallet<T> {
		/// Pre-validate an aggregated proof (all cheap checks, no ZK verification).
		/// Called by both validate_unsigned (pool gating) and dispatch (defense-in-depth).
		fn pre_validate_proof(
			proof_bytes: &[u8],
		) -> Result<(ProofWithPublicInputs<F, C, D>, AggregatedPublicCircuitInputs), Error<T>> {
			let verifier = crate::get_aggregated_verifier()
				.map_err(|_| Error::<T>::AggregatedVerifierNotAvailable)?;
			let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.to_vec(),
				&verifier.circuit_data.common,
			)
			.map_err(|_| Error::<T>::AggregatedProofDeserializationFailed)?;
			let inputs = parse_aggregated_public_inputs(&proof)
				.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;
			ensure!(inputs.asset_id == 0, Error::<T>::NonNativeAssetNotSupported);
			ensure!(
				inputs.volume_fee_bps == T::VolumeFeeRateBps::get(),
				Error::<T>::InvalidVolumeFeeRate
			);
			let block_number = BlockNumberFor::<T>::from(inputs.block_data.block_number);
			let block_hash = frame_system::Pallet::<T>::block_hash(block_number);
			ensure!(block_hash != T::Hash::default(), Error::<T>::BlockNotFound);
			ensure!(
				block_hash.as_ref() == inputs.block_data.block_hash.as_ref(),
				Error::<T>::InvalidPublicInputs
			);
			for nullifier in &inputs.nullifiers {
				let bytes: [u8; 32] = (*nullifier)
					.as_ref()
					.try_into()
					.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;
				ensure!(
					!UsedNullifiers::<T>::contains_key(bytes),
					Error::<T>::NullifierAlreadyUsed
				);
			}
			Ok((proof, inputs))
		}

		pub fn record_transfer(
			asset_id: AssetIdOf<T>,
			from: <T as Config>::WormholeAccountId,
			to: <T as Config>::WormholeAccountId,
			amount: BalanceOf<T>,
		) {
			let current_count = TransferCount::<T>::get(&to);

			// Storage key uses Blake2_256 hash of (to, transfer_count)
			// This is unique since transfer_count is atomic per recipient
			let key: TransferProofKey<T> = (to.clone(), current_count);

			// Storage value is the Poseidon2 hash of the full transfer data (leaf_inputs_hash)
			// This matches what the ZK circuit computes and verifies
			let full_data: TransferProofData<T> =
				(asset_id.clone(), current_count, from.clone(), to.clone(), amount);
			let encoded_data = full_data.encode();
			let leaf_inputs_hash =
				PoseidonCore::hash_storage::<TransferProofData<T>>(&encoded_data);

			TransferProof::<T>::insert(key, leaf_inputs_hash);
			TransferCount::<T>::insert(&to, current_count.saturating_add(T::TransferCount::one()));

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
		fn record_transfer_proof(
			asset_id: Option<AssetIdOf<T>>,
			from: <T as Config>::WormholeAccountId,
			to: <T as Config>::WormholeAccountId,
			amount: BalanceOf<T>,
		) {
			let asset_id_value = asset_id.unwrap_or_default();
			Self::record_transfer(asset_id_value, from, to, amount);
		}
	}
}
