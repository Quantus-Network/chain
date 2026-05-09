#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
pub use pallet::*;
pub mod weights;
pub use weights::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
	use crate::WeightInfo;
	use alloc::vec::Vec;
	use codec::{Decode, Encode};
	use frame_support::{
		pallet_prelude::*,
		traits::{Currency, ReservableCurrency},
		transactional, BoundedVec,
	};
	use frame_system::pallet_prelude::*;
	use qp_wormhole_verifier::{
		AggregatedPublicCircuitInputs, BytesDigest, Layer1AggregatedPublicCircuitInputs,
		PublicInputsByAccount, L0_AGGREGATED_PUBLIC_INPUT_LAYOUT_VERSION,
	};
	use sp_runtime::traits::{Saturating, Zero};

	pub type CandidateId = [u8; 32];
	pub type BundleId = [u8; 32];
	pub type CircuitId = [u8; 32];
	pub type PublicInputsHash = [u8; 32];
	pub type Nullifier = [u8; 32];
	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[derive(
		Encode,
		Decode,
		DecodeWithMemTracking,
		Clone,
		PartialEq,
		Eq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	pub enum L0CandidateStatus {
		Pending,
		Claimed { bundle_id: BundleId },
		Settled { bundle_id: BundleId },
		Dropped,
		Expired,
		ChallengedInvalid,
	}

	#[derive(
		Encode,
		Decode,
		DecodeWithMemTracking,
		Clone,
		PartialEq,
		Eq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	pub enum BundleStatus {
		Claimed,
		Proving,
		Submitted,
		Settled,
		Expired,
		Challenged,
		Reassigned,
	}

	#[derive(
		Encode,
		Decode,
		DecodeWithMemTracking,
		Clone,
		PartialEq,
		Eq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	pub struct BundleGroupKey {
		pub circuit_id: CircuitId,
		pub public_input_layout_version: u32,
		pub num_leaf_proofs: u32,
		pub num_layer0_proofs: u32,
		pub asset_id: u32,
		pub volume_fee_bps: u32,
		pub block_hash: [u8; 32],
		pub block_number: u32,
	}

	#[derive(
		Encode,
		Decode,
		DecodeWithMemTracking,
		Clone,
		PartialEq,
		Eq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	pub struct ExitSlotSummary {
		pub summed_output_amount: u32,
		pub exit_account: [u8; 32],
	}

	#[derive(
		Encode,
		Decode,
		DecodeWithMemTracking,
		Clone,
		PartialEq,
		Eq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	#[scale_info(skip_type_params(MaxProofBytes, MaxNullifiers, MaxExitSlots))]
	pub struct L0Candidate<
		AccountId,
		BlockNumber,
		Balance,
		MaxProofBytes,
		MaxNullifiers,
		MaxExitSlots,
	>
	where
		MaxProofBytes: Get<u32>,
		MaxNullifiers: Get<u32>,
		MaxExitSlots: Get<u32>,
	{
		pub proof_hash: CandidateId,
		pub public_inputs_hash: PublicInputsHash,
		pub group_key: BundleGroupKey,
		pub submitter: AccountId,
		pub submitted_at: BlockNumber,
		pub expires_at: BlockNumber,
		pub proof_bytes: BoundedVec<u8, MaxProofBytes>,
		pub nullifiers: BoundedVec<Nullifier, MaxNullifiers>,
		pub exit_summary: BoundedVec<ExitSlotSummary, MaxExitSlots>,
		pub aggregation_tip: Balance,
		pub storage_bond: Balance,
		pub validity_bond: Balance,
		pub status: L0CandidateStatus,
	}

	pub type CandidateOf<T> = L0Candidate<
		<T as frame_system::Config>::AccountId,
		BlockNumberFor<T>,
		BalanceOf<T>,
		<T as Config>::MaxL0ProofBytes,
		<T as Config>::MaxNullifiersPerL0,
		<T as Config>::MaxExitSlotsPerL0,
	>;

	#[derive(
		Encode,
		Decode,
		DecodeWithMemTracking,
		Clone,
		PartialEq,
		Eq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	pub struct AggregatorInfo<BlockNumber, Balance> {
		pub registered_at: BlockNumber,
		pub reward_address: [u8; 32],
		pub bond: Balance,
		pub max_active_jobs: u32,
		pub active_jobs: u32,
	}

	#[derive(
		Encode,
		Decode,
		DecodeWithMemTracking,
		Clone,
		PartialEq,
		Eq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	#[scale_info(skip_type_params(MaxCandidates))]
	pub struct Bundle<AccountId, BlockNumber, Balance, MaxCandidates>
	where
		MaxCandidates: Get<u32>,
	{
		pub bundle_id: BundleId,
		pub group_key: BundleGroupKey,
		pub ordered_candidates: BoundedVec<CandidateId, MaxCandidates>,
		pub bundle_root: [u8; 32],
		pub public_inputs_root: [u8; 32],
		pub assigned_miner: AccountId,
		pub aggregator_address: [u8; 32],
		pub claimed_at: BlockNumber,
		pub deadline: BlockNumber,
		pub miner_bond: Balance,
		pub reward_pot: Balance,
		pub retry_count: u32,
		pub status: BundleStatus,
	}

	pub type BundleOf<T> = Bundle<
		<T as frame_system::Config>::AccountId,
		BlockNumberFor<T>,
		BalanceOf<T>,
		<T as Config>::MaxCandidatesPerQueue,
	>;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_wormhole::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type Currency: ReservableCurrency<Self::AccountId>;

		#[pallet::constant]
		type MaxL0ProofBytes: Get<u32>;

		#[pallet::constant]
		type MaxNullifiersPerL0: Get<u32>;

		#[pallet::constant]
		type MaxExitSlotsPerL0: Get<u32>;

		#[pallet::constant]
		type MaxCandidatesPerQueue: Get<u32>;

		#[pallet::constant]
		type CandidateLifetime: Get<BlockNumberFor<Self>>;

		#[pallet::constant]
		type StorageBond: Get<BalanceOf<Self>>;

		#[pallet::constant]
		type ValidityBond: Get<BalanceOf<Self>>;

		#[pallet::constant]
		type NumLayer0Proofs: Get<u32>;

		#[pallet::constant]
		type CircuitId: Get<CircuitId>;

		#[pallet::constant]
		type MaxActiveBundlesPerMiner: Get<u32>;

		#[pallet::constant]
		type BundleProvingPeriod: Get<BlockNumberFor<Self>>;

		#[pallet::constant]
		type MinMinerBond: Get<BalanceOf<Self>>;

		#[pallet::constant]
		type MaxL1ProofBytes: Get<u32>;

		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	pub type L0Candidates<T: Config> =
		StorageMap<_, Blake2_128Concat, CandidateId, CandidateOf<T>, OptionQuery>;

	#[pallet::storage]
	pub type PendingQueues<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		BundleGroupKey,
		BoundedVec<CandidateId, T::MaxCandidatesPerQueue>,
		ValueQuery,
	>;

	#[pallet::storage]
	pub type RegisteredAggregators<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		AggregatorInfo<BlockNumberFor<T>, BalanceOf<T>>,
		OptionQuery,
	>;

	#[pallet::storage]
	pub type Bundles<T: Config> =
		StorageMap<_, Blake2_128Concat, BundleId, BundleOf<T>, OptionQuery>;

	#[pallet::storage]
	pub type MinerActiveBundles<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		BoundedVec<BundleId, T::MaxActiveBundlesPerMiner>,
		ValueQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		L0CandidateSubmitted {
			candidate_id: CandidateId,
			submitter: T::AccountId,
			group_key: BundleGroupKey,
		},
		AggregatorRegistered {
			account: T::AccountId,
		},
		BundleClaimed {
			bundle_id: BundleId,
			miner: T::AccountId,
			group_key: BundleGroupKey,
		},
		BundleTimedOut {
			bundle_id: BundleId,
		},
		BundleSettled {
			bundle_id: BundleId,
			miner: T::AccountId,
		},
		L0CandidateChallengedInvalid {
			candidate_id: CandidateId,
			challenger: T::AccountId,
		},
		L0CandidateExpired {
			candidate_id: CandidateId,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		ProofTooLarge,
		MalformedPublicInputs,
		UnsupportedCircuit,
		UnsupportedLayoutVersion,
		QueueFull,
		DuplicateCandidate,
		CandidateNotFound,
		CandidateNotPending,
		StaleBlockReference,
		InvalidVolumeFeeRate,
		NonNativeAssetNotSupported,
		BondReservationFailed,
		TooManyNullifiers,
		TooManyExitSlots,
		AggregatorNotRegistered,
		TooManyActiveJobs,
		InsufficientMinerBond,
		InsufficientCandidates,
		BundleNotFound,
		BundleNotActive,
		BundleExpired,
		NotAssignedMiner,
		BundleNotExpired,
		DuplicateNullifier,
		NullifierUnavailable,
		ActiveBundleLimit,
		ProofMismatch,
		L1ProofTooLarge,
		MalformedL1Proof,
		MalformedL1PublicInputs,
		L1ProofRejected,
		RewardTransferFailed,
		CandidateValid,
		CandidateNotExpired,
		ChallengeVerificationUnavailable,
		InvalidRewardAddress,
		AggregatorAddressMismatch,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::submit_l0_candidate())]
		pub fn submit_l0_candidate(
			origin: OriginFor<T>,
			proof_bytes: Vec<u8>,
			aggregation_tip: BalanceOf<T>,
		) -> DispatchResult {
			let submitter = ensure_signed(origin)?;
			ensure!(
				proof_bytes.len() <= T::MaxL0ProofBytes::get() as usize,
				Error::<T>::ProofTooLarge
			);

			let candidate_id = sp_io::hashing::blake2_256(&proof_bytes);
			ensure!(!L0Candidates::<T>::contains_key(candidate_id), Error::<T>::DuplicateCandidate);

			let proof = pallet_wormhole::Pallet::<T>::deserialize_aggregated_proof(&proof_bytes)
				.map_err(|_| Error::<T>::MalformedPublicInputs)?;
			let inputs = pallet_wormhole::Pallet::<T>::parse_aggregated_inputs_from_proof(&proof)
				.map_err(|_| Error::<T>::MalformedPublicInputs)?;

			ensure!(inputs.asset_id == 0, Error::<T>::NonNativeAssetNotSupported);
			ensure!(
				inputs.volume_fee_bps == <T as pallet_wormhole::Config>::VolumeFeeRateBps::get(),
				Error::<T>::InvalidVolumeFeeRate
			);

			let block_number = BlockNumberFor::<T>::from(inputs.block_data.block_number);
			let block_hash = frame_system::Pallet::<T>::block_hash(block_number);
			ensure!(block_hash != T::Hash::default(), Error::<T>::StaleBlockReference);
			ensure!(
				block_hash.as_ref() == inputs.block_data.block_hash.as_ref(),
				Error::<T>::StaleBlockReference
			);

			let nullifiers = Self::bounded_nullifiers(&inputs)?;
			let exit_summary = Self::bounded_exit_summary(&inputs)?;
			let group_key = Self::group_key(&inputs)?;
			let queue = PendingQueues::<T>::get(&group_key);
			ensure!(queue.len() < T::MaxCandidatesPerQueue::get() as usize, Error::<T>::QueueFull);

			let proof_bytes: BoundedVec<u8, T::MaxL0ProofBytes> =
				proof_bytes.try_into().map_err(|_| Error::<T>::ProofTooLarge)?;
			let public_inputs_hash = Self::public_inputs_hash(&inputs)?;
			let submitted_at = frame_system::Pallet::<T>::block_number();
			let expires_at = submitted_at.saturating_add(T::CandidateLifetime::get());
			let storage_bond = T::StorageBond::get();
			let validity_bond = T::ValidityBond::get();

			Self::reserve_candidate_funds(
				&submitter,
				storage_bond,
				validity_bond,
				aggregation_tip,
			)?;

			let candidate = L0Candidate {
				proof_hash: candidate_id,
				public_inputs_hash,
				group_key: group_key.clone(),
				submitter: submitter.clone(),
				submitted_at,
				expires_at,
				proof_bytes,
				nullifiers,
				exit_summary,
				aggregation_tip,
				storage_bond,
				validity_bond,
				status: L0CandidateStatus::Pending,
			};

			L0Candidates::<T>::insert(candidate_id, candidate);
			PendingQueues::<T>::try_mutate(&group_key, |queue| {
				queue.try_push(candidate_id).map_err(|_| Error::<T>::QueueFull)
			})?;

			Self::deposit_event(Event::L0CandidateSubmitted { candidate_id, submitter, group_key });

			Ok(())
		}

		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::register_aggregator())]
		pub fn register_aggregator(
			origin: OriginFor<T>,
			reward_address: [u8; 32],
			max_active_jobs: u32,
			bond: BalanceOf<T>,
		) -> DispatchResult {
			let account = ensure_signed(origin)?;
			Self::decode_registered_reward_address(&reward_address)?;
			let registered_at = frame_system::Pallet::<T>::block_number();
			if !bond.is_zero() {
				<T as Config>::Currency::reserve(&account, bond)
					.map_err(|_| Error::<T>::BondReservationFailed)?;
			}
			RegisteredAggregators::<T>::insert(
				&account,
				AggregatorInfo {
					registered_at,
					reward_address,
					bond,
					max_active_jobs,
					active_jobs: 0,
				},
			);
			Self::deposit_event(Event::AggregatorRegistered { account });
			Ok(())
		}

		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::claim_bundle())]
		#[transactional]
		pub fn claim_bundle(
			origin: OriginFor<T>,
			group_key: BundleGroupKey,
			aggregator_address: [u8; 32],
			miner_bond: BalanceOf<T>,
		) -> DispatchResult {
			let miner = ensure_signed(origin)?;
			ensure!(miner_bond >= T::MinMinerBond::get(), Error::<T>::InsufficientMinerBond);

			let mut info = RegisteredAggregators::<T>::get(&miner)
				.ok_or(Error::<T>::AggregatorNotRegistered)?;
			ensure!(
				aggregator_address == info.reward_address,
				Error::<T>::AggregatorAddressMismatch
			);
			ensure!(info.active_jobs < info.max_active_jobs, Error::<T>::TooManyActiveJobs);
			ensure!(
				MinerActiveBundles::<T>::get(&miner).len() <
					T::MaxActiveBundlesPerMiner::get() as usize,
				Error::<T>::ActiveBundleLimit
			);
			ensure!(group_key.circuit_id == T::CircuitId::get(), Error::<T>::UnsupportedCircuit);
			ensure!(
				group_key.public_input_layout_version == L0_AGGREGATED_PUBLIC_INPUT_LAYOUT_VERSION,
				Error::<T>::UnsupportedLayoutVersion
			);
			ensure!(
				group_key.num_layer0_proofs == T::NumLayer0Proofs::get(),
				Error::<T>::UnsupportedCircuit
			);

			let selected = Self::select_bundle_candidates(&group_key)?;
			let nullifiers = Self::candidate_nullifiers(&selected)?;
			pallet_wormhole::Pallet::<T>::ensure_no_duplicate_nullifiers(&nullifiers)
				.map_err(|_| Error::<T>::DuplicateNullifier)?;

			let public_inputs_root = Self::public_inputs_root(&selected)?;
			let bundle_root = Self::bundle_root(&group_key, &selected, public_inputs_root);
			let claimed_at = frame_system::Pallet::<T>::block_number();
			let deadline = claimed_at.saturating_add(T::BundleProvingPeriod::get());
			let bundle_id = Self::bundle_id(&miner, &group_key, &selected, claimed_at);

			<T as Config>::Currency::reserve(&miner, miner_bond)
				.map_err(|_| Error::<T>::BondReservationFailed)?;
			if let Err(_err) = pallet_wormhole::Pallet::<T>::lock_nullifiers_for_bundle(
				bundle_id,
				deadline,
				&nullifiers,
			) {
				<T as Config>::Currency::unreserve(&miner, miner_bond);
				return Err(Error::<T>::NullifierUnavailable.into());
			}

			Self::remove_selected_from_queue(&group_key, &selected)?;
			for candidate_id in &selected {
				L0Candidates::<T>::try_mutate(candidate_id, |candidate| -> DispatchResult {
					let candidate = candidate.as_mut().ok_or(Error::<T>::CandidateNotFound)?;
					ensure!(
						candidate.status == L0CandidateStatus::Pending,
						Error::<T>::CandidateNotPending
					);
					candidate.status = L0CandidateStatus::Claimed { bundle_id };
					Ok(())
				})?;
			}

			let ordered_candidates: BoundedVec<CandidateId, T::MaxCandidatesPerQueue> =
				selected.clone().try_into().map_err(|_| Error::<T>::InsufficientCandidates)?;
			let bundle = Bundle {
				bundle_id,
				group_key: group_key.clone(),
				ordered_candidates,
				bundle_root,
				public_inputs_root,
				assigned_miner: miner.clone(),
				aggregator_address: info.reward_address,
				claimed_at,
				deadline,
				miner_bond,
				reward_pot: BalanceOf::<T>::zero(),
				retry_count: 0,
				status: BundleStatus::Claimed,
			};

			Bundles::<T>::insert(bundle_id, bundle);
			MinerActiveBundles::<T>::try_mutate(&miner, |active| {
				active.try_push(bundle_id).map_err(|_| Error::<T>::ActiveBundleLimit)
			})?;
			info.active_jobs = info.active_jobs.saturating_add(1);
			RegisteredAggregators::<T>::insert(&miner, info);

			Self::deposit_event(Event::BundleClaimed { bundle_id, miner, group_key });
			Ok(())
		}

		#[pallet::call_index(3)]
		#[pallet::weight(<T as Config>::WeightInfo::timeout_bundle())]
		#[transactional]
		pub fn timeout_bundle(origin: OriginFor<T>, bundle_id: BundleId) -> DispatchResult {
			let _who = ensure_signed(origin)?;
			let mut bundle = Bundles::<T>::get(bundle_id).ok_or(Error::<T>::BundleNotFound)?;
			ensure!(
				matches!(bundle.status, BundleStatus::Claimed | BundleStatus::Proving),
				Error::<T>::BundleNotActive
			);
			let now = frame_system::Pallet::<T>::block_number();
			ensure!(now > bundle.deadline, Error::<T>::BundleNotExpired);

			let candidate_ids = bundle.ordered_candidates.to_vec();
			let nullifiers = Self::candidate_nullifiers(&candidate_ids)?;
			pallet_wormhole::Pallet::<T>::unlock_nullifiers_for_bundle(bundle_id, &nullifiers)
				.map_err(|_| Error::<T>::NullifierUnavailable)?;

			for candidate_id in &candidate_ids {
				L0Candidates::<T>::try_mutate(candidate_id, |candidate| -> DispatchResult {
					let candidate = candidate.as_mut().ok_or(Error::<T>::CandidateNotFound)?;
					candidate.status = if now > candidate.expires_at {
						Self::refund_candidate_reserves(candidate);
						L0CandidateStatus::Expired
					} else {
						L0CandidateStatus::Pending
					};
					Ok(())
				})?;
			}
			Self::return_unexpired_candidates_to_queue(&bundle.group_key, &candidate_ids, now)?;
			<T as Config>::Currency::unreserve(&bundle.assigned_miner, bundle.miner_bond);
			Self::remove_active_bundle(&bundle.assigned_miner, bundle_id);
			Self::decrement_active_jobs(&bundle.assigned_miner);
			bundle.status = BundleStatus::Expired;
			Bundles::<T>::insert(bundle_id, bundle);

			Self::deposit_event(Event::BundleTimedOut { bundle_id });
			Ok(())
		}

		#[pallet::call_index(4)]
		#[pallet::weight(<T as Config>::WeightInfo::submit_l1_aggregate())]
		#[transactional]
		pub fn submit_l1_aggregate(
			origin: OriginFor<T>,
			bundle_id: BundleId,
			proof_bytes: Vec<u8>,
		) -> DispatchResult {
			let submitter = ensure_signed(origin)?;
			ensure!(
				proof_bytes.len() <= T::MaxL1ProofBytes::get() as usize,
				Error::<T>::L1ProofTooLarge
			);
			let bundle = Bundles::<T>::get(bundle_id).ok_or(Error::<T>::BundleNotFound)?;
			ensure!(submitter == bundle.assigned_miner, Error::<T>::NotAssignedMiner);
			ensure!(
				matches!(bundle.status, BundleStatus::Claimed | BundleStatus::Proving),
				Error::<T>::BundleNotActive
			);
			let now = frame_system::Pallet::<T>::block_number();
			ensure!(now <= bundle.deadline, Error::<T>::BundleExpired);

			let proof = pallet_wormhole::Pallet::<T>::deserialize_layer1_proof(&proof_bytes)
				.map_err(|_| Error::<T>::MalformedL1Proof)?;
			let inputs = pallet_wormhole::Pallet::<T>::parse_layer1_inputs_from_proof(&proof)
				.map_err(|_| Error::<T>::MalformedL1PublicInputs)?;
			Self::ensure_l1_matches_bundle(&bundle, &inputs)?;
			pallet_wormhole::Pallet::<T>::verify_layer1_proof(&proof)
				.map_err(|_| Error::<T>::L1ProofRejected)?;

			let nullifiers = Self::layer1_nullifier_bytes(&inputs)?;
			Self::settle_verified_l1_bundle(
				bundle_id,
				bundle,
				nullifiers,
				&inputs.account_data,
				inputs.volume_fee_bps,
			)
		}

		#[pallet::call_index(5)]
		#[pallet::weight(<T as Config>::WeightInfo::challenge_invalid_l0_candidate())]
		#[transactional]
		pub fn challenge_invalid_l0_candidate(
			origin: OriginFor<T>,
			candidate_id: CandidateId,
		) -> DispatchResult {
			let challenger = ensure_signed(origin)?;
			let candidate =
				L0Candidates::<T>::get(candidate_id).ok_or(Error::<T>::CandidateNotFound)?;
			ensure!(
				candidate.status == L0CandidateStatus::Pending,
				Error::<T>::CandidateNotPending
			);

			match pallet_wormhole::Pallet::<T>::verify_aggregated_proof_for_candidate(
				candidate.proof_bytes.as_slice(),
			) {
				Ok(_) => return Err(Error::<T>::CandidateValid.into()),
				Err(pallet_wormhole::Error::<T>::AggregatedVerificationFailed) => {},
				Err(_) => return Err(Error::<T>::ChallengeVerificationUnavailable.into()),
			}

			Self::remove_selected_from_queue(&candidate.group_key, &[candidate_id])?;
			Self::refund_candidate_storage_and_tip(&candidate);
			let (_slashed, _remaining) = <T as Config>::Currency::slash_reserved(
				&candidate.submitter,
				candidate.validity_bond,
			);
			L0Candidates::<T>::mutate(candidate_id, |stored| {
				if let Some(stored) = stored {
					stored.status = L0CandidateStatus::ChallengedInvalid;
				}
			});

			Self::deposit_event(Event::L0CandidateChallengedInvalid { candidate_id, challenger });
			Ok(())
		}

		#[pallet::call_index(6)]
		#[pallet::weight(<T as Config>::WeightInfo::drop_expired_candidate())]
		#[transactional]
		pub fn drop_expired_candidate(
			origin: OriginFor<T>,
			candidate_id: CandidateId,
		) -> DispatchResult {
			let _who = ensure_signed(origin)?;
			let mut candidate =
				L0Candidates::<T>::get(candidate_id).ok_or(Error::<T>::CandidateNotFound)?;
			ensure!(
				candidate.status == L0CandidateStatus::Pending,
				Error::<T>::CandidateNotPending
			);
			let now = frame_system::Pallet::<T>::block_number();
			ensure!(now > candidate.expires_at, Error::<T>::CandidateNotExpired);

			Self::remove_selected_from_queue(&candidate.group_key, &[candidate_id])?;
			Self::refund_candidate_reserves(&candidate);
			candidate.status = L0CandidateStatus::Expired;
			L0Candidates::<T>::insert(candidate_id, candidate);

			Self::deposit_event(Event::L0CandidateExpired { candidate_id });
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		pub(crate) fn settle_verified_l1_bundle(
			bundle_id: BundleId,
			mut bundle: BundleOf<T>,
			nullifiers: Vec<Nullifier>,
			account_data: &[PublicInputsByAccount],
			volume_fee_bps: u32,
		) -> DispatchResult {
			let reward_account = Self::reward_account(&bundle)?;
			let prepared = pallet_wormhole::Pallet::<T>::prepare_public_output_settlement(
				account_data,
				volume_fee_bps,
				pallet_wormhole::SettlementKind::DelegatedL1 {
					aggregation_reward_account: reward_account.clone(),
				},
			)
			.map_err(|_| Error::<T>::ProofMismatch)?;
			pallet_wormhole::Pallet::<T>::ensure_nullifiers_locked_by_bundle(
				bundle_id,
				&nullifiers,
			)
			.map_err(|_| Error::<T>::NullifierUnavailable)?;

			pallet_wormhole::Pallet::<T>::apply_public_output_settlement(
				prepared,
				Some(&reward_account),
			)
			.map_err(|_| Error::<T>::ProofMismatch)?;
			pallet_wormhole::Pallet::<T>::mark_locked_nullifiers_used(bundle_id, &nullifiers)
				.map_err(|_| Error::<T>::NullifierUnavailable)?;

			for candidate_id in bundle.ordered_candidates.iter() {
				L0Candidates::<T>::try_mutate(candidate_id, |candidate| -> DispatchResult {
					let candidate = candidate.as_mut().ok_or(Error::<T>::CandidateNotFound)?;
					Self::release_candidate_reserves(candidate, &reward_account)?;
					candidate.status = L0CandidateStatus::Settled { bundle_id };
					Ok(())
				})?;
			}

			<T as Config>::Currency::unreserve(&bundle.assigned_miner, bundle.miner_bond);
			Self::remove_active_bundle(&bundle.assigned_miner, bundle_id);
			Self::decrement_active_jobs(&bundle.assigned_miner);
			let settled_miner = bundle.assigned_miner.clone();
			bundle.status = BundleStatus::Settled;
			Bundles::<T>::insert(bundle_id, bundle);
			Self::deposit_event(Event::BundleSettled { bundle_id, miner: settled_miner });
			Ok(())
		}

		fn reserve_candidate_funds(
			submitter: &T::AccountId,
			storage_bond: BalanceOf<T>,
			validity_bond: BalanceOf<T>,
			aggregation_tip: BalanceOf<T>,
		) -> Result<(), Error<T>> {
			<T as Config>::Currency::reserve(submitter, storage_bond)
				.map_err(|_| Error::<T>::BondReservationFailed)?;
			if let Err(_err) = <T as Config>::Currency::reserve(submitter, validity_bond) {
				<T as Config>::Currency::unreserve(submitter, storage_bond);
				return Err(Error::<T>::BondReservationFailed);
			}
			if let Err(_err) = <T as Config>::Currency::reserve(submitter, aggregation_tip) {
				<T as Config>::Currency::unreserve(submitter, storage_bond);
				<T as Config>::Currency::unreserve(submitter, validity_bond);
				return Err(Error::<T>::BondReservationFailed);
			}
			Ok(())
		}

		fn release_candidate_reserves(
			candidate: &CandidateOf<T>,
			reward_account: &T::AccountId,
		) -> Result<(), Error<T>> {
			Self::refund_candidate_bonds(candidate);
			if !candidate.aggregation_tip.is_zero() {
				<T as Config>::Currency::unreserve(&candidate.submitter, candidate.aggregation_tip);
				<T as Config>::Currency::transfer(
					&candidate.submitter,
					reward_account,
					candidate.aggregation_tip,
					frame_support::traits::ExistenceRequirement::AllowDeath,
				)
				.map_err(|_| Error::<T>::RewardTransferFailed)?;
			}
			Ok(())
		}

		fn refund_candidate_reserves(candidate: &CandidateOf<T>) {
			Self::refund_candidate_bonds(candidate);
			Self::refund_candidate_tip(candidate);
		}

		fn refund_candidate_storage_and_tip(candidate: &CandidateOf<T>) {
			<T as Config>::Currency::unreserve(&candidate.submitter, candidate.storage_bond);
			Self::refund_candidate_tip(candidate);
		}

		fn refund_candidate_bonds(candidate: &CandidateOf<T>) {
			<T as Config>::Currency::unreserve(&candidate.submitter, candidate.storage_bond);
			<T as Config>::Currency::unreserve(&candidate.submitter, candidate.validity_bond);
		}

		fn refund_candidate_tip(candidate: &CandidateOf<T>) {
			if !candidate.aggregation_tip.is_zero() {
				<T as Config>::Currency::unreserve(&candidate.submitter, candidate.aggregation_tip);
			}
		}

		fn select_bundle_candidates(
			group_key: &BundleGroupKey,
		) -> Result<Vec<CandidateId>, Error<T>> {
			let now = frame_system::Pallet::<T>::block_number();
			let mut selected = Vec::new();
			for candidate_id in PendingQueues::<T>::get(group_key) {
				let Some(candidate) = L0Candidates::<T>::get(candidate_id) else {
					continue;
				};
				if candidate.status != L0CandidateStatus::Pending ||
					candidate.group_key != *group_key ||
					now > candidate.expires_at
				{
					continue;
				}
				selected.push(candidate_id);
				if selected.len() == T::NumLayer0Proofs::get() as usize {
					break;
				}
			}
			ensure!(
				selected.len() == T::NumLayer0Proofs::get() as usize,
				Error::<T>::InsufficientCandidates
			);
			Ok(selected)
		}

		fn remove_selected_from_queue(
			group_key: &BundleGroupKey,
			selected: &[CandidateId],
		) -> DispatchResult {
			let mut retained = BoundedVec::<CandidateId, T::MaxCandidatesPerQueue>::new();
			for candidate_id in PendingQueues::<T>::get(group_key) {
				if selected.contains(&candidate_id) {
					continue;
				}
				retained.try_push(candidate_id).map_err(|_| Error::<T>::QueueFull)?;
			}
			PendingQueues::<T>::insert(group_key, retained);
			Ok(())
		}

		fn return_unexpired_candidates_to_queue(
			group_key: &BundleGroupKey,
			candidate_ids: &[CandidateId],
			now: BlockNumberFor<T>,
		) -> DispatchResult {
			PendingQueues::<T>::try_mutate(group_key, |queue| -> DispatchResult {
				for candidate_id in candidate_ids {
					let Some(candidate) = L0Candidates::<T>::get(candidate_id) else {
						continue;
					};
					if now <= candidate.expires_at {
						queue.try_push(*candidate_id).map_err(|_| Error::<T>::QueueFull)?;
					}
				}
				Ok(())
			})
		}

		fn candidate_nullifiers(candidate_ids: &[CandidateId]) -> Result<Vec<Nullifier>, Error<T>> {
			let mut out = Vec::new();
			for candidate_id in candidate_ids {
				let candidate =
					L0Candidates::<T>::get(candidate_id).ok_or(Error::<T>::CandidateNotFound)?;
				out.extend(candidate.nullifiers.iter().copied());
			}
			Ok(out)
		}

		fn expected_exit_summary(
			candidate_ids: &[CandidateId],
		) -> Result<Vec<ExitSlotSummary>, Error<T>> {
			let mut out = Vec::new();
			for candidate_id in candidate_ids {
				let candidate =
					L0Candidates::<T>::get(candidate_id).ok_or(Error::<T>::CandidateNotFound)?;
				out.extend(candidate.exit_summary.iter().cloned());
			}
			Ok(out)
		}

		fn public_inputs_root(candidate_ids: &[CandidateId]) -> Result<[u8; 32], Error<T>> {
			let mut bytes = Vec::new();
			for candidate_id in candidate_ids {
				let candidate =
					L0Candidates::<T>::get(candidate_id).ok_or(Error::<T>::CandidateNotFound)?;
				bytes.extend_from_slice(&candidate.public_inputs_hash);
			}
			Ok(sp_io::hashing::blake2_256(&bytes))
		}

		fn bundle_root(
			group_key: &BundleGroupKey,
			candidate_ids: &[CandidateId],
			public_inputs_root: [u8; 32],
		) -> [u8; 32] {
			let mut bytes = b"quantus:wormhole:l1-bundle:v1".to_vec();
			bytes.extend_from_slice(&group_key.encode());
			bytes.extend_from_slice(&public_inputs_root);
			for candidate_id in candidate_ids {
				bytes.extend_from_slice(candidate_id);
			}
			sp_io::hashing::blake2_256(&bytes)
		}

		fn bundle_id(
			miner: &T::AccountId,
			group_key: &BundleGroupKey,
			candidate_ids: &[CandidateId],
			claimed_at: BlockNumberFor<T>,
		) -> BundleId {
			let mut bytes = b"quantus:wormhole:l1-bundle-id:v1".to_vec();
			bytes.extend_from_slice(&miner.encode());
			bytes.extend_from_slice(&group_key.encode());
			bytes.extend_from_slice(&claimed_at.encode());
			for candidate_id in candidate_ids {
				bytes.extend_from_slice(candidate_id);
			}
			sp_io::hashing::blake2_256(&bytes)
		}

		fn layer1_nullifier_bytes(
			inputs: &Layer1AggregatedPublicCircuitInputs,
		) -> Result<Vec<Nullifier>, Error<T>> {
			inputs
				.nullifiers
				.iter()
				.map(Self::digest_to_bytes)
				.collect::<Result<Vec<_>, _>>()
		}

		pub(crate) fn ensure_l1_matches_bundle(
			bundle: &BundleOf<T>,
			inputs: &Layer1AggregatedPublicCircuitInputs,
		) -> Result<(), Error<T>> {
			ensure!(
				Self::digest_to_bytes(&inputs.aggregator_address)? == bundle.aggregator_address,
				Error::<T>::ProofMismatch
			);
			ensure!(inputs.asset_id == bundle.group_key.asset_id, Error::<T>::ProofMismatch);
			ensure!(
				inputs.volume_fee_bps == bundle.group_key.volume_fee_bps,
				Error::<T>::ProofMismatch
			);
			ensure!(
				Self::digest_to_bytes(&inputs.block_data.block_hash)? ==
					bundle.group_key.block_hash,
				Error::<T>::ProofMismatch
			);
			ensure!(
				inputs.block_data.block_number == bundle.group_key.block_number,
				Error::<T>::ProofMismatch
			);

			let candidate_ids = bundle.ordered_candidates.to_vec();
			let mut expected_nullifiers = Self::candidate_nullifiers(&candidate_ids)?;
			let mut actual_nullifiers = Self::layer1_nullifier_bytes(inputs)?;
			pallet_wormhole::Pallet::<T>::ensure_no_duplicate_nullifiers(&actual_nullifiers)
				.map_err(|_| Error::<T>::DuplicateNullifier)?;
			expected_nullifiers.sort();
			actual_nullifiers.sort();
			ensure!(expected_nullifiers == actual_nullifiers, Error::<T>::ProofMismatch);

			let expected_exits = Self::expected_exit_summary(&candidate_ids)?;
			ensure!(expected_exits.len() == inputs.account_data.len(), Error::<T>::ProofMismatch);
			for (expected, actual) in expected_exits.iter().zip(inputs.account_data.iter()) {
				ensure!(
					expected.summed_output_amount == actual.summed_output_amount,
					Error::<T>::ProofMismatch
				);
				ensure!(
					expected.exit_account == Self::digest_to_bytes(&actual.exit_account)?,
					Error::<T>::ProofMismatch
				);
			}
			Ok(())
		}

		fn decode_registered_reward_address(
			reward_address: &[u8; 32],
		) -> Result<T::AccountId, Error<T>> {
			let account = T::AccountId::decode(&mut &reward_address[..])
				.map_err(|_| Error::<T>::InvalidRewardAddress)?;
			let encoded = account.encode();
			let encoded: [u8; 32] =
				encoded.as_slice().try_into().map_err(|_| Error::<T>::InvalidRewardAddress)?;
			ensure!(encoded == *reward_address, Error::<T>::InvalidRewardAddress);
			Ok(account)
		}

		fn reward_account(bundle: &BundleOf<T>) -> Result<T::AccountId, Error<T>> {
			Self::decode_registered_reward_address(&bundle.aggregator_address)
		}

		fn remove_active_bundle(miner: &T::AccountId, bundle_id: BundleId) {
			MinerActiveBundles::<T>::mutate(miner, |active| {
				if let Some(pos) = active.iter().position(|id| *id == bundle_id) {
					active.remove(pos);
				}
			});
		}

		fn decrement_active_jobs(miner: &T::AccountId) {
			RegisteredAggregators::<T>::mutate(miner, |info| {
				if let Some(info) = info {
					info.active_jobs = info.active_jobs.saturating_sub(1);
				}
			});
		}

		fn group_key(inputs: &AggregatedPublicCircuitInputs) -> Result<BundleGroupKey, Error<T>> {
			let block_hash = Self::digest_to_bytes(&inputs.block_data.block_hash)?;
			Ok(BundleGroupKey {
				circuit_id: T::CircuitId::get(),
				public_input_layout_version: L0_AGGREGATED_PUBLIC_INPUT_LAYOUT_VERSION,
				num_leaf_proofs: inputs.nullifiers.len() as u32,
				num_layer0_proofs: T::NumLayer0Proofs::get(),
				asset_id: inputs.asset_id,
				volume_fee_bps: inputs.volume_fee_bps,
				block_hash,
				block_number: inputs.block_data.block_number,
			})
		}

		fn bounded_nullifiers(
			inputs: &AggregatedPublicCircuitInputs,
		) -> Result<BoundedVec<Nullifier, T::MaxNullifiersPerL0>, Error<T>> {
			let mut out = BoundedVec::<Nullifier, T::MaxNullifiersPerL0>::new();
			for nullifier in &inputs.nullifiers {
				out.try_push(Self::digest_to_bytes(nullifier)?)
					.map_err(|_| Error::<T>::TooManyNullifiers)?;
			}
			Ok(out)
		}

		fn bounded_exit_summary(
			inputs: &AggregatedPublicCircuitInputs,
		) -> Result<BoundedVec<ExitSlotSummary, T::MaxExitSlotsPerL0>, Error<T>> {
			let mut out = BoundedVec::<ExitSlotSummary, T::MaxExitSlotsPerL0>::new();
			for exit in &inputs.account_data {
				out.try_push(ExitSlotSummary {
					summed_output_amount: exit.summed_output_amount,
					exit_account: Self::digest_to_bytes(&exit.exit_account)?,
				})
				.map_err(|_| Error::<T>::TooManyExitSlots)?;
			}
			Ok(out)
		}

		fn public_inputs_hash(
			inputs: &AggregatedPublicCircuitInputs,
		) -> Result<PublicInputsHash, Error<T>> {
			let mut bytes = Vec::new();
			bytes.extend_from_slice(&inputs.asset_id.to_le_bytes());
			bytes.extend_from_slice(&inputs.volume_fee_bps.to_le_bytes());
			bytes.extend_from_slice(Self::digest_to_bytes(&inputs.block_data.block_hash)?.as_ref());
			bytes.extend_from_slice(&inputs.block_data.block_number.to_le_bytes());
			for exit in &inputs.account_data {
				bytes.extend_from_slice(&exit.summed_output_amount.to_le_bytes());
				bytes.extend_from_slice(Self::digest_to_bytes(&exit.exit_account)?.as_ref());
			}
			for nullifier in &inputs.nullifiers {
				bytes.extend_from_slice(Self::digest_to_bytes(nullifier)?.as_ref());
			}
			Ok(sp_io::hashing::blake2_256(&bytes))
		}

		fn digest_to_bytes(digest: &BytesDigest) -> Result<[u8; 32], Error<T>> {
			digest.as_ref().try_into().map_err(|_| Error::<T>::MalformedPublicInputs)
		}
	}
}
