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

#[cfg(wormhole_layer1_verifier)]
lazy_static! {
	static ref LAYER1_VERIFIER: Option<WormholeVerifier> = {
		let verifier_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/layer1_verifier.bin"));
		let common_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/layer1_common.bin"));
		WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes).ok()
	};
}

/// Getter for the aggregated proof verifier
pub fn get_aggregated_verifier() -> Result<&'static WormholeVerifier, &'static str> {
	AGGREGATED_VERIFIER.as_ref().ok_or("Aggregated verifier not available")
}

/// Getter for the layer-1 aggregated proof verifier.
#[cfg(wormhole_layer1_verifier)]
pub fn get_layer1_verifier() -> Result<&'static WormholeVerifier, &'static str> {
	LAYER1_VERIFIER.as_ref().ok_or("Layer1 verifier not available")
}

/// Getter for the layer-1 aggregated proof verifier when L1 artifacts were not generated.
#[cfg(not(wormhole_layer1_verifier))]
pub fn get_layer1_verifier() -> Result<&'static WormholeVerifier, &'static str> {
	Err("Layer1 verifier not available")
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
	use codec::Decode;
	use frame_support::{
		dispatch::{DispatchErrorWithPostInfo, DispatchResultWithPostInfo, PostDispatchInfo},
		pallet_prelude::*,
		traits::{
			fungible::{Inspect as FungibleInspect, Mutate, Unbalanced},
			fungibles::{self},
			BuildGenesisConfig, Currency,
		},
		transactional,
	};
	use frame_system::pallet_prelude::*;
	use pallet_zk_tree::ZkTreeRecorder;
	use qp_wormhole_verifier::{
		parse_aggregated_public_inputs, parse_layer1_aggregated_public_inputs,
		AggregatedPublicCircuitInputs, Layer1AggregatedPublicCircuitInputs, ProofWithPublicInputs,
		PublicInputsByAccount, C, D, F,
	};
	use sp_runtime::{
		traits::{CheckedAdd, MaybeDisplay, One, Saturating, Zero},
		transaction_validity::{
			InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
		},
		Permill,
	};

	pub type BalanceOf<T> = <T as Config>::NativeBalance;
	pub type AssetBalanceOf<T> = <T as Config>::AssetBalance;

	#[derive(Clone, PartialEq, Eq, RuntimeDebug)]
	pub enum SettlementKind<AccountId> {
		DirectL0,
		DelegatedL1 { aggregation_reward_account: AccountId },
	}

	#[derive(Clone, PartialEq, Eq, RuntimeDebug)]
	pub struct PreparedPublicOutputSettlement<AccountId, Balance> {
		pub transfers: Vec<(AccountId, Balance)>,
		pub total_exit_amount: Balance,
		pub total_fee: Balance,
		pub burn_amount: Balance,
		pub block_author_fee: Balance,
		pub aggregation_prover_fee: Balance,
		pub block_author: Option<AccountId>,
		pub aggregation_reward_account: Option<AccountId>,
	}

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
			+ Into<u64>
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

		/// Proportion of volume fees to burn (not mint). For direct L0 settlements, the remainder
		/// goes to the block author when one is available. For delegated L1 settlements, the
		/// non-burned remainder is split between the aggregation prover and block author.
		#[pallet::constant]
		type VolumeFeesBurnRate: Get<Permill>;

		/// Proportion of the non-burned delegated L1 fee paid to the aggregation prover.
		/// Direct L0 settlement ignores this value to preserve existing fee behavior.
		#[pallet::constant]
		type AggregationProverFeeShare: Get<Permill>;

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

		/// ZK Tree recorder for inserting transfer leaves into the Merkle tree.
		/// Set to `()` to disable ZK tree recording.
		type ZkTree: pallet_zk_tree::ZkTreeRecorder<
			<Self as frame_system::Config>::AccountId,
			Self::AssetId,
			Self::NativeBalance,
		>;
	}

	#[derive(Encode, Decode, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
	pub struct NullifierLock<BlockNumber> {
		pub bundle_id: [u8; 32],
		pub expires_at: BlockNumber,
	}

	#[pallet::storage]
	#[pallet::getter(fn used_nullifiers)]
	pub(super) type UsedNullifiers<T: Config> =
		StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn locked_nullifiers)]
	pub type LockedNullifiers<T: Config> =
		StorageMap<_, Blake2_128Concat, [u8; 32], NullifierLock<BlockNumberFor<T>>, OptionQuery>;

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
		/// A native token transfer was recorded.
		///
		/// The `leaf_index` can be used to fetch Merkle proofs via the
		/// `zkTrie_getMerkleProof` RPC for ZK circuit verification.
		NativeTransferred {
			from: <T as frame_system::Config>::AccountId,
			to: <T as frame_system::Config>::AccountId,
			amount: BalanceOf<T>,
			transfer_count: T::TransferCount,
			/// Index of this transfer in the ZK trie (for Merkle proof lookup)
			leaf_index: u64,
		},
		/// A non-native asset transfer was recorded.
		///
		/// The `leaf_index` can be used to fetch Merkle proofs via the
		/// `zkTrie_getMerkleProof` RPC for ZK circuit verification.
		AssetTransferred {
			asset_id: T::AssetId,
			from: <T as frame_system::Config>::AccountId,
			to: <T as frame_system::Config>::AccountId,
			amount: AssetBalanceOf<T>,
			transfer_count: T::TransferCount,
			/// Index of this transfer in the ZK trie (for Merkle proof lookup)
			leaf_index: u64,
		},
		ProofVerified {
			exit_amount: BalanceOf<T>,
			nullifiers: Vec<[u8; 32]>,
		},
		WormholeFeeSettled {
			total_fee: BalanceOf<T>,
			burn_amount: BalanceOf<T>,
			block_author_fee: BalanceOf<T>,
			aggregation_prover_fee: BalanceOf<T>,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidPublicInputs,
		NullifierAlreadyUsed,
		NullifierLocked,
		DuplicateNullifier,
		NullifierNotLocked,
		NullifierLockMismatch,
		BlockNotFound,
		AggregatedVerifierNotAvailable,
		AggregatedProofDeserializationFailed,
		AggregatedVerificationFailed,
		InvalidAggregatedPublicInputs,
		Layer1VerifierNotAvailable,
		Layer1ProofDeserializationFailed,
		InvalidLayer1PublicInputs,
		Layer1VerificationFailed,
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
		#[transactional]
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

			let nullifier_list = Self::collect_aggregated_nullifier_bytes(&aggregated_inputs)?;
			let prepared = Self::prepare_public_output_settlement(
				&aggregated_inputs.account_data,
				aggregated_inputs.volume_fee_bps,
				SettlementKind::DirectL0,
			)?;

			// Mark nullifiers as used (validate_proof only checks availability)
			Self::mark_nullifiers_used(&nullifier_list)?;

			// Emit event for each exit account
			Self::deposit_event(Event::ProofVerified {
				exit_amount: prepared.total_exit_amount,
				nullifiers: nullifier_list,
			});
			Self::apply_public_output_settlement(prepared)?;

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
		pub fn is_nullifier_used(nullifier: &[u8; 32]) -> bool {
			UsedNullifiers::<T>::contains_key(*nullifier)
		}

		pub fn is_nullifier_locked(nullifier: &[u8; 32]) -> bool {
			LockedNullifiers::<T>::contains_key(*nullifier)
		}

		pub fn ensure_no_duplicate_nullifiers(nullifiers: &[[u8; 32]]) -> Result<(), Error<T>> {
			let mut sorted = nullifiers.to_vec();
			sorted.sort();
			for pair in sorted.windows(2) {
				if pair[0] == pair[1] {
					return Err(Error::<T>::DuplicateNullifier);
				}
			}
			Ok(())
		}

		pub fn ensure_nullifiers_available_for_direct_settlement(
			nullifiers: &[[u8; 32]],
		) -> Result<(), Error<T>> {
			Self::ensure_no_duplicate_nullifiers(nullifiers)?;
			for nullifier in nullifiers {
				ensure!(!Self::is_nullifier_used(nullifier), Error::<T>::NullifierAlreadyUsed);
				ensure!(!Self::is_nullifier_locked(nullifier), Error::<T>::NullifierLocked);
			}
			Ok(())
		}

		pub fn lock_nullifiers_for_bundle(
			bundle_id: [u8; 32],
			expires_at: BlockNumberFor<T>,
			nullifiers: &[[u8; 32]],
		) -> Result<(), Error<T>> {
			Self::ensure_nullifiers_available_for_direct_settlement(nullifiers)?;

			for nullifier in nullifiers {
				LockedNullifiers::<T>::insert(*nullifier, NullifierLock { bundle_id, expires_at });
			}

			Ok(())
		}

		pub fn unlock_nullifiers_for_bundle(
			bundle_id: [u8; 32],
			nullifiers: &[[u8; 32]],
		) -> Result<(), Error<T>> {
			Self::ensure_no_duplicate_nullifiers(nullifiers)?;

			for nullifier in nullifiers {
				let lock =
					LockedNullifiers::<T>::get(*nullifier).ok_or(Error::<T>::NullifierNotLocked)?;
				ensure!(lock.bundle_id == bundle_id, Error::<T>::NullifierLockMismatch);
			}

			for nullifier in nullifiers {
				LockedNullifiers::<T>::remove(*nullifier);
			}

			Ok(())
		}

		pub fn mark_nullifiers_used(nullifiers: &[[u8; 32]]) -> Result<(), Error<T>> {
			Self::ensure_no_duplicate_nullifiers(nullifiers)?;

			for nullifier in nullifiers {
				UsedNullifiers::<T>::insert(*nullifier, true);
			}

			Ok(())
		}

		pub fn mark_locked_nullifiers_used(
			bundle_id: [u8; 32],
			nullifiers: &[[u8; 32]],
		) -> Result<(), Error<T>> {
			Self::ensure_nullifiers_locked_by_bundle(bundle_id, nullifiers)?;

			for nullifier in nullifiers {
				UsedNullifiers::<T>::insert(*nullifier, true);
				LockedNullifiers::<T>::remove(*nullifier);
			}

			Ok(())
		}

		pub fn ensure_nullifiers_locked_by_bundle(
			bundle_id: [u8; 32],
			nullifiers: &[[u8; 32]],
		) -> Result<(), Error<T>> {
			Self::ensure_no_duplicate_nullifiers(nullifiers)?;

			for nullifier in nullifiers {
				let lock =
					LockedNullifiers::<T>::get(*nullifier).ok_or(Error::<T>::NullifierNotLocked)?;
				ensure!(lock.bundle_id == bundle_id, Error::<T>::NullifierLockMismatch);
			}

			Ok(())
		}

		fn collect_aggregated_nullifier_bytes(
			inputs: &AggregatedPublicCircuitInputs,
		) -> Result<Vec<[u8; 32]>, Error<T>> {
			inputs
				.nullifiers
				.iter()
				.map(|nullifier| {
					(*nullifier)
						.as_ref()
						.try_into()
						.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)
				})
				.collect()
		}

		pub fn deserialize_aggregated_proof(
			proof_bytes: &[u8],
		) -> Result<ProofWithPublicInputs<F, C, D>, Error<T>> {
			let verifier = crate::get_aggregated_verifier()
				.map_err(|_| Error::<T>::AggregatedVerifierNotAvailable)?;
			ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.to_vec(),
				&verifier.circuit_data.common,
			)
			.map_err(|_| Error::<T>::AggregatedProofDeserializationFailed)
		}

		pub fn parse_aggregated_inputs_from_proof(
			proof: &ProofWithPublicInputs<F, C, D>,
		) -> Result<AggregatedPublicCircuitInputs, Error<T>> {
			parse_aggregated_public_inputs(proof)
				.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)
		}

		pub fn verify_aggregated_proof_for_candidate(
			proof_bytes: &[u8],
		) -> Result<AggregatedPublicCircuitInputs, Error<T>> {
			let verifier = crate::get_aggregated_verifier()
				.map_err(|_| Error::<T>::AggregatedVerifierNotAvailable)?;
			let proof = Self::deserialize_aggregated_proof(proof_bytes)?;
			let inputs = Self::parse_aggregated_inputs_from_proof(&proof)?;
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

			verifier.verify(proof).map_err(|e| {
				log::error!("Candidate aggregated proof verification failed: {:?}", e);
				Error::<T>::AggregatedVerificationFailed
			})?;

			Ok(inputs)
		}

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
			let proof = Self::deserialize_aggregated_proof(proof_bytes)?;
			let inputs = Self::parse_aggregated_inputs_from_proof(&proof)?;
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
			let nullifiers = Self::collect_aggregated_nullifier_bytes(&inputs)?;
			Self::ensure_nullifiers_available_for_direct_settlement(&nullifiers)?;

			// Full ZK verification - if this fails, full verification weight was consumed
			verifier.verify(proof.clone()).map_err(|e| {
				log::error!("Aggregated proof verification failed: {:?}", e);
				Error::<T>::AggregatedVerificationFailed
			})?;

			Ok((proof, inputs))
		}

		/// Deserialize a layer-1 aggregated proof using the layer-1 common circuit data.
		///
		/// This intentionally does not verify the proof. Callers can parse public inputs and run
		/// cheap bundle/effects checks before paying the full ZK verification cost.
		pub fn deserialize_layer1_proof(
			proof_bytes: &[u8],
		) -> Result<ProofWithPublicInputs<F, C, D>, Error<T>> {
			let verifier =
				crate::get_layer1_verifier().map_err(|_| Error::<T>::Layer1VerifierNotAvailable)?;
			ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.to_vec(),
				&verifier.circuit_data.common,
			)
			.map_err(|_| Error::<T>::Layer1ProofDeserializationFailed)
		}

		/// Parse layer-1 public inputs from an already-deserialized proof.
		pub fn parse_layer1_inputs_from_proof(
			proof: &ProofWithPublicInputs<F, C, D>,
		) -> Result<Layer1AggregatedPublicCircuitInputs, Error<T>> {
			parse_layer1_aggregated_public_inputs(proof)
				.map_err(|_| Error::<T>::InvalidLayer1PublicInputs)
		}

		/// Verify an already-deserialized layer-1 aggregated proof.
		pub fn verify_layer1_proof(proof: &ProofWithPublicInputs<F, C, D>) -> Result<(), Error<T>> {
			let verifier =
				crate::get_layer1_verifier().map_err(|_| Error::<T>::Layer1VerifierNotAvailable)?;
			verifier.verify_ref(proof).map_err(|e| {
				log::error!("Layer-1 aggregated proof verification failed: {:?}", e);
				Error::<T>::Layer1VerificationFailed
			})
		}

		pub fn settle_public_outputs(
			account_data: &[PublicInputsByAccount],
			volume_fee_bps: u32,
		) -> Result<BalanceOf<T>, Error<T>> {
			let prepared = Self::prepare_public_output_settlement(
				account_data,
				volume_fee_bps,
				SettlementKind::DirectL0,
			)?;
			Self::apply_public_output_settlement(prepared)
		}

		pub fn prepare_public_output_settlement(
			account_data: &[PublicInputsByAccount],
			volume_fee_bps: u32,
			settlement_kind: SettlementKind<<T as frame_system::Config>::AccountId>,
		) -> Result<
			PreparedPublicOutputSettlement<<T as frame_system::Config>::AccountId, BalanceOf<T>>,
			Error<T>,
		> {
			let mut total_exit_amount: BalanceOf<T> = Zero::zero();
			let mut transfers: Vec<(<T as frame_system::Config>::AccountId, BalanceOf<T>)> =
				Vec::with_capacity(account_data.len());

			for (idx, account_data) in account_data.iter().enumerate() {
				let exit_account_bytes: [u8; 32] =
					(*account_data.exit_account).as_ref().try_into().map_err(|e| {
						log::error!("Failed to convert exit_account at idx {}: {:?}", idx, e);
						Error::<T>::InvalidAggregatedPublicInputs
					})?;

				if exit_account_bytes == [0u8; 32] || account_data.summed_output_amount == 0 {
					continue;
				}

				let exit_balance_u128 = (account_data.summed_output_amount as u128)
					.saturating_mul(crate::SCALE_DOWN_FACTOR);
				let exit_balance: BalanceOf<T> = exit_balance_u128.try_into().map_err(|_| {
					log::error!("Failed to convert exit_balance at idx {}", idx);
					Error::<T>::InvalidAggregatedPublicInputs
				})?;

				let exit_account =
					<T as frame_system::Config>::AccountId::decode(&mut &exit_account_bytes[..])
						.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;

				total_exit_amount =
					total_exit_amount.checked_add(&exit_balance).ok_or_else(|| {
						log::error!("Failed to add exit_balance at idx {}", idx);
						Error::<T>::InvalidAggregatedPublicInputs
					})?;
				transfers.push((exit_account, exit_balance));
			}

			ensure!(
				total_exit_amount >= T::MinimumTransferAmount::get(),
				Error::<T>::TransferAmountBelowMinimum
			);

			let fee_bps = volume_fee_bps as u128;
			ensure!(fee_bps < 10_000, Error::<T>::InvalidAggregatedPublicInputs);
			let total_exit_u128: u128 = total_exit_amount.try_into().map_err(|_| {
				log::error!("Failed to convert total_exit_amount to u128");
				Error::<T>::InvalidAggregatedPublicInputs
			})?;
			let total_fee_u128 = total_exit_u128
				.saturating_mul(fee_bps)
				.checked_div(10000u128.saturating_sub(fee_bps))
				.unwrap_or(0);

			let burn_rate = T::VolumeFeesBurnRate::get();
			let mut burn_amount_u128 = burn_rate * total_fee_u128;
			let non_burned_fee_u128 = total_fee_u128.saturating_sub(burn_amount_u128);
			let (aggregation_reward_account, aggregation_prover_fee_u128) = match settlement_kind {
				SettlementKind::DirectL0 => (None, 0),
				SettlementKind::DelegatedL1 { aggregation_reward_account } => (
					Some(aggregation_reward_account),
					T::AggregationProverFeeShare::get() * non_burned_fee_u128,
				),
			};
			let block_author_fee_u128 =
				non_burned_fee_u128.saturating_sub(aggregation_prover_fee_u128);
			let block_author = if block_author_fee_u128 == 0 {
				None
			} else {
				let digest = frame_system::Pallet::<T>::digest();
				qp_wormhole::extract_author_from_digest::<<T as frame_system::Config>::AccountId, _>(
					digest.logs.iter().cloned(),
				)
			};
			if block_author.is_none() {
				burn_amount_u128 = burn_amount_u128.saturating_add(block_author_fee_u128);
			}

			let total_fee: BalanceOf<T> = total_fee_u128.try_into().map_err(|_| {
				log::error!("Failed to convert total_fee_u128 to BalanceOf");
				Error::<T>::InvalidAggregatedPublicInputs
			})?;
			let burn_amount: BalanceOf<T> = burn_amount_u128.try_into().map_err(|_| {
				log::error!("Failed to convert burn_amount_u128 to BalanceOf");
				Error::<T>::InvalidAggregatedPublicInputs
			})?;
			let block_author_fee: BalanceOf<T> =
				if block_author.is_some() { block_author_fee_u128 } else { 0 }
					.try_into()
					.map_err(|_| {
						log::error!("Failed to convert block_author_fee_u128 to BalanceOf");
						Error::<T>::InvalidAggregatedPublicInputs
					})?;
			let aggregation_prover_fee: BalanceOf<T> =
				aggregation_prover_fee_u128.try_into().map_err(|_| {
					log::error!("Failed to convert aggregation_prover_fee_u128 to BalanceOf");
					Error::<T>::InvalidAggregatedPublicInputs
				})?;

			Ok(PreparedPublicOutputSettlement {
				transfers,
				total_exit_amount,
				total_fee,
				burn_amount,
				block_author_fee,
				aggregation_prover_fee,
				block_author,
				aggregation_reward_account,
			})
		}

		pub fn apply_public_output_settlement(
			prepared: PreparedPublicOutputSettlement<
				<T as frame_system::Config>::AccountId,
				BalanceOf<T>,
			>,
		) -> Result<BalanceOf<T>, Error<T>> {
			let mint_account = T::MintingAccount::get();

			if !prepared.block_author_fee.is_zero() {
				let author = prepared
					.block_author
					.as_ref()
					.ok_or(Error::<T>::InvalidAggregatedPublicInputs)?;
				<T::Currency as Unbalanced<_>>::increase_balance(
					author,
					prepared.block_author_fee,
					frame_support::traits::tokens::Precision::Exact,
				)
				.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;
			}

			if !prepared.aggregation_prover_fee.is_zero() {
				let reward_target = prepared
					.aggregation_reward_account
					.as_ref()
					.ok_or(Error::<T>::InvalidAggregatedPublicInputs)?;
				<T::Currency as Unbalanced<_>>::increase_balance(
					reward_target,
					prepared.aggregation_prover_fee,
					frame_support::traits::tokens::Precision::Exact,
				)
				.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;
			}

			if !prepared.burn_amount.is_zero() {
				let current = <T::Currency as FungibleInspect<_>>::total_issuance();
				<T::Currency as Unbalanced<_>>::set_total_issuance(
					current.saturating_sub(prepared.burn_amount),
				);
			}

			for (exit_account, exit_balance) in &prepared.transfers {
				<T::Currency as Unbalanced<_>>::increase_balance(
					exit_account,
					*exit_balance,
					frame_support::traits::tokens::Precision::Exact,
				)
				.map_err(|_| Error::<T>::InvalidAggregatedPublicInputs)?;

				let from_account: <T as Config>::WormholeAccountId = mint_account.clone().into();
				let to_account: <T as Config>::WormholeAccountId = exit_account.clone().into();
				Self::record_transfer(
					T::AssetId::default(),
					&from_account,
					&to_account,
					*exit_balance,
				);
			}

			Self::deposit_event(Event::WormholeFeeSettled {
				total_fee: prepared.total_fee,
				burn_amount: prepared.burn_amount,
				block_author_fee: prepared.block_author_fee,
				aggregation_prover_fee: prepared.aggregation_prover_fee,
			});

			Ok(prepared.total_exit_amount)
		}

		/// Record a transfer in the ZK tree and emit events.
		///
		/// This inserts the transfer data into the 4-ary Poseidon Merkle tree
		/// managed by pallet-zk-tree, which provides Merkle proofs for ZK circuits.
		///
		/// The emitted event includes `leaf_index` which clients can use to fetch
		/// Merkle proofs via `zkTree_getMerkleProof(leaf_index)` RPC.
		pub fn record_transfer(
			asset_id: T::AssetId,
			from: &<T as Config>::WormholeAccountId,
			to: &<T as Config>::WormholeAccountId,
			amount: BalanceOf<T>,
		) {
			let current_count = TransferCount::<T>::get(to);

			// Increment transfer count for this recipient
			TransferCount::<T>::insert(to, current_count.saturating_add(T::TransferCount::one()));

			// Insert into ZK tree for Merkle proof generation
			// Returns the leaf index for clients to use when fetching proofs
			let leaf_index = T::ZkTree::record_transfer(
				to.clone().into(),
				current_count.into(),
				asset_id.clone(),
				amount,
			);

			if asset_id == T::AssetId::default() {
				Self::deposit_event(Event::<T>::NativeTransferred {
					from: from.clone().into(),
					to: to.clone().into(),
					amount,
					transfer_count: current_count,
					leaf_index,
				});
			} else {
				Self::deposit_event(Event::<T>::AssetTransferred {
					from: from.clone().into(),
					to: to.clone().into(),
					asset_id,
					amount: amount.into(),
					transfer_count: current_count,
					leaf_index,
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
