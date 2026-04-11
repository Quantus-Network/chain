#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use lazy_static::lazy_static;
pub use pallet::*;
pub use qp_poseidon::ToFelts;
use qp_wormhole_verifier::WormholeVerifier;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
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
	use crate::{ToFelts, WeightInfo};
	use alloc::vec::Vec;
	use codec::{Decode, Encode};
	use frame_support::{
		dispatch::{DispatchErrorWithPostInfo, DispatchResultWithPostInfo, PostDispatchInfo},
		pallet_prelude::*,
		traits::{
			fungible::{Inspect as FungibleInspect, Mutate, Unbalanced},
			fungibles::{self},
			BuildGenesisConfig, Currency,
		},
	};
	use frame_system::pallet_prelude::*;
	use pallet_zk_trie::ZkTrieRecorder;
	use qp_wormhole_verifier::{
		parse_aggregated_public_inputs, AggregatedPublicCircuitInputs, ProofWithPublicInputs, C, D,
		F,
	};
	use sp_runtime::{
		traits::{MaybeDisplay, One, Saturating, Zero},
		transaction_validity::{
			InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
		},
		Permill,
	};

	pub type BalanceOf<T> = <T as Config>::NativeBalance;
	pub type AssetBalanceOf<T> = <T as Config>::AssetBalance;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Genesis configuration for recording transfer proofs.
	///
	/// This allows addresses to be endowed at genesis with funds that can be spent
	/// using ZK proofs. The endowments are stored during genesis and processed in
	/// `on_initialize` at block 1, which calls `record_transfer` for each address.
	/// This records both the TransferProof in storage AND emits NativeTransferred events.
	///
	/// We defer to block 1 because events emitted during genesis_build are not
	/// persisted (Substrate limitation). By processing at block 1, indexers like
	/// Subsquid can track these transfers.
	///
	/// The chain does not distinguish between "wormhole addresses" and regular addresses -
	/// any address can have transfer proofs recorded and spend via ZK proofs.
	///
	/// Note: The actual balance must also be set via BalancesConfig separately.
	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		/// Addresses to record transfer proofs for at genesis: (address, amount).
		/// A TransferProof will be recorded for each, enabling ZK spending.
		/// Uses u128 for serde compatibility; converted to BalanceOf<T> at build time.
		pub endowed_addresses: Vec<(T::WormholeAccountId, u128)>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			// Store endowments to be processed in on_initialize at block 1.
			// We can't call record_transfer here because events emitted during
			// genesis_build are not persisted (Substrate limitation).
			// By deferring to block 1, both storage and events are handled correctly.
			let pending: Vec<(T::WormholeAccountId, BalanceOf<T>)> = self
				.endowed_addresses
				.iter()
				.map(|(to, amount)| {
					let balance: BalanceOf<T> = (*amount).try_into().unwrap_or_else(|_| {
						panic!("Genesis endowment amount {} exceeds Balance capacity", amount)
					});
					(to.clone(), balance)
				})
				.collect();

			if !pending.is_empty() {
				GenesisEndowmentsPending::<T>::put(pending);
			}
		}
	}

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Native balance type with ToFelts bound for Poseidon hashing in transfer proofs.
		type NativeBalance: Parameter
			+ Member
			+ Default
			+ Copy
			+ ToFelts
			+ MaxEncodedLen
			+ sp_runtime::traits::AtLeast32BitUnsigned
			+ sp_runtime::traits::CheckedAdd
			+ sp_runtime::traits::CheckedSub
			+ sp_runtime::traits::Zero
			+ sp_runtime::traits::Saturating;

		/// Currency type used for native token transfers and minting.
		type Currency: Mutate<<Self as frame_system::Config>::AccountId, Balance = Self::NativeBalance>
			+ Unbalanced<<Self as frame_system::Config>::AccountId>
			+ Currency<<Self as frame_system::Config>::AccountId, Balance = Self::NativeBalance>;

		/// Assets type used for managing fungible assets.
		/// The AssetId must match Self::AssetId for consistency.
		type Assets: fungibles::Inspect<
				<Self as frame_system::Config>::AccountId,
				AssetId = Self::AssetId,
				Balance = Self::AssetBalance,
			> + fungibles::Mutate<<Self as frame_system::Config>::AccountId>
			+ fungibles::Create<<Self as frame_system::Config>::AccountId>;

		/// Asset ID type with bounds needed for Poseidon hashing in transfer proofs.
		type AssetId: Parameter + Member + Default + From<u32> + Clone + ToFelts + MaxEncodedLen;

		/// Asset balance type that can convert to/from native balance.
		type AssetBalance: Parameter
			+ Member
			+ Into<Self::NativeBalance>
			+ From<Self::NativeBalance>
			+ MaxEncodedLen;

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

		/// ZK Trie recorder for inserting transfer leaves into the Merkle tree.
		/// Set to `()` to disable ZK trie recording.
		type ZkTrie: pallet_zk_trie::ZkTrieRecorder<
			<Self as frame_system::Config>::AccountId,
			Self::AssetId,
			Self::NativeBalance,
		>;
	}

	#[pallet::storage]
	#[pallet::getter(fn used_nullifiers)]
	pub(super) type UsedNullifiers<T: Config> =
		StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

	/// Transfer count per recipient - used to generate unique leaf indices in the ZK trie.
	#[pallet::storage]
	#[pallet::getter(fn transfer_count)]
	pub type TransferCount<T: Config> =
		StorageMap<_, Blake2_128Concat, T::WormholeAccountId, T::TransferCount, ValueQuery>;

	/// Genesis endowments pending event emission.
	/// Stores (to_address, amount) for each genesis endowment.
	/// These are processed in on_initialize at block 1 to emit NativeTransferred events,
	/// then cleared. This ensures indexers like Subsquid can track genesis transfers.
	///
	/// Unbounded because it's only populated at genesis and cleared on block 1.
	#[pallet::storage]
	#[pallet::unbounded]
	pub type GenesisEndowmentsPending<T: Config> =
		StorageValue<_, Vec<(T::WormholeAccountId, BalanceOf<T>)>, ValueQuery>;

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
			asset_id: T::AssetId,
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

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// On block 1, process all genesis endowments by calling record_transfer.
		/// This records transfer proofs and emits NativeTransferred events.
		/// We defer this from genesis_build because events emitted during genesis
		/// are not persisted (Substrate limitation).
		fn on_initialize(n: BlockNumberFor<T>) -> Weight {
			// Only process on block 1
			if n != One::one() {
				return Weight::zero();
			}

			let pending = GenesisEndowmentsPending::<T>::take();
			if pending.is_empty() {
				return Weight::zero();
			}

			let minting_account: T::WormholeAccountId = T::MintingAccount::get().into();
			let num_endowments = pending.len() as u64;

			for (to, amount) in pending {
				// Record transfer proof and emit event
				Self::record_transfer(T::AssetId::default(), &minting_account, &to, amount);
			}

			// Weight: 1 read (take pending) + N * (2 reads + 2 writes + 1 event) per endowment
			T::DbWeight::get().reads_writes(1 + num_endowments * 2, num_endowments * 2)
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Verify an aggregated wormhole proof and process all transfers in the batch.
		///
		/// Returns `DispatchResultWithPostInfo` to allow weight correction on early failures.
		/// If validation fails before ZK verification, we return minimal weight.
		/// If ZK verification fails, we return full weight since the work was done.
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::verify_aggregated_proof())]
		pub fn verify_aggregated_proof(
			origin: OriginFor<T>,
			proof_bytes: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			ensure_none(origin)?;

			// Full validation including ZK verification (defense-in-depth, also done in
			// validate_unsigned). Weight returned depends on which stage failed.
			let (_proof, aggregated_inputs) = match Self::validate_proof(&proof_bytes) {
				Ok(result) => result,
				Err(e) => {
					// Determine weight based on which stage failed
					let actual_weight = match e {
						// ZK verification was attempted - full weight consumed
						Error::<T>::AggregatedVerificationFailed =>
							Some(<T as Config>::WeightInfo::verify_aggregated_proof()),
						// Failed before ZK verification - minimal weight
						_ => Some(<T as Config>::WeightInfo::pre_validate_proof()),
					};
					return Err(DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo { actual_weight, pays_fee: Pays::No },
						error: e.into(),
					});
				},
			};

			// Mark nullifiers as used (validate_proof only checks existence)
			let mut nullifier_list = Vec::<[u8; 32]>::new();
			for nullifier in &aggregated_inputs.nullifiers {
				let nullifier_bytes: [u8; 32] = (*nullifier)
					.as_ref()
					.try_into()
					.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;
				UsedNullifiers::<T>::insert(nullifier_bytes, true);
				nullifier_list.push(nullifier_bytes);
			}

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
				let from_account: <T as Config>::WormholeAccountId = mint_account.clone().into();
				let to_account: <T as Config>::WormholeAccountId = exit_account.clone().into();
				Self::record_transfer(
					T::AssetId::default(),
					&from_account,
					&to_account,
					*exit_balance,
				);
			}

			// Success - use declared weight (actual_weight: None means use declared weight)
			Ok(PostDispatchInfo { actual_weight: None, pays_fee: Pays::No })
		}
	}

	#[pallet::validate_unsigned]
	impl<T: Config> ValidateUnsigned for Pallet<T> {
		type Call = Call<T>;

		fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			match call {
				Call::verify_aggregated_proof { proof_bytes } => {
					// Full validation including ZK verification - prevents invalid proofs
					// with high amounts from entering the pool and crowding out valid txs
					let (_proof, inputs) =
						Self::validate_proof(proof_bytes).map_err(|_| InvalidTransaction::Call)?;

					// Priority based on total transfer volume - higher value transfers get
					// priority. This prevents DoS since attackers must transfer real value
					// (and valid proofs) to get high priority.
					let total_amount: u64 =
						inputs.account_data.iter().map(|a| a.summed_output_amount as u64).sum();

					ValidTransaction::with_tag_prefix("WormholeAggregatedVerify")
						.and_provides(sp_io::hashing::blake2_256(proof_bytes))
						.priority(total_amount)
						.longevity(5)
						.propagate(true)
						.build()
				},
				_ => InvalidTransaction::Call.into(),
			}
		}

		fn pre_dispatch(call: &Self::Call) -> Result<(), TransactionValidityError> {
			// Skip re-validation - validate_unsigned already did full verification,
			// and dispatch will verify again as defense-in-depth
			match call {
				Call::verify_aggregated_proof { .. } => Ok(()),
				_ => Err(InvalidTransaction::Call.into()),
			}
		}
	}

	impl<T: Config> Pallet<T> {
		/// Validate an aggregated proof (cheap checks + full ZK verification).
		/// Called by both validate_unsigned (pool gating) and dispatch (defense-in-depth).
		///
		/// Errors before ZK verification (deserialization, nullifier checks, etc.) allow
		/// dispatch to return minimal weight. `AggregatedVerificationFailed` indicates
		/// full ZK verification was attempted.
		fn validate_proof(
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

			// Full ZK verification - if this fails, full verification weight was consumed
			verifier.verify(proof.clone()).map_err(|e| {
				log::error!("Aggregated proof verification failed: {:?}", e);
				Error::<T>::AggregatedVerificationFailed
			})?;

			Ok((proof, inputs))
		}

		/// Record a transfer in the ZK trie and emit events.
		///
		/// This inserts the transfer data into the 4-ary Poseidon Merkle tree
		/// managed by pallet-zk-trie, which provides Merkle proofs for ZK circuits.
		pub fn record_transfer(
			asset_id: T::AssetId,
			from: &<T as Config>::WormholeAccountId,
			to: &<T as Config>::WormholeAccountId,
			amount: BalanceOf<T>,
		) {
			let current_count = TransferCount::<T>::get(to);

			// Increment transfer count for this recipient
			TransferCount::<T>::insert(to, current_count.saturating_add(T::TransferCount::one()));

			// Insert into ZK trie for Merkle proof generation
			// Convert transfer_count to u64 for the trie
			let transfer_count_u64: u64 = {
				let encoded = current_count.encode();
				let mut bytes = [0u8; 8];
				let len = encoded.len().min(8);
				bytes[..len].copy_from_slice(&encoded[..len]);
				u64::from_le_bytes(bytes)
			};
			T::ZkTrie::record_transfer(
				to.clone().into(),
				transfer_count_u64,
				asset_id.clone(),
				amount,
			);

			if asset_id == T::AssetId::default() {
				Self::deposit_event(Event::<T>::NativeTransferred {
					from: from.clone().into(),
					to: to.clone().into(),
					amount,
					transfer_count: current_count,
				});
			} else {
				Self::deposit_event(Event::<T>::AssetTransferred {
					from: from.clone().into(),
					to: to.clone().into(),
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
			<T as Config>::AssetId,
			BalanceOf<T>,
		> for Pallet<T>
	{
		fn record_transfer_proof(
			asset_id: Option<<T as Config>::AssetId>,
			from: <T as Config>::WormholeAccountId,
			to: <T as Config>::WormholeAccountId,
			amount: BalanceOf<T>,
		) {
			let asset_id_value = asset_id.unwrap_or_default();
			Self::record_transfer(asset_id_value, &from, &to, amount);
		}
	}
}
