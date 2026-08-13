#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use lazy_static::lazy_static;
pub use pallet::*;
use qp_plonky2_verifier::util::serialization::DefaultGateSerializer;
use qp_wormhole_verifier::{
	CircuitConfig, CommonCircuitData, VerifierCircuitData, VerifierOnlyCircuitData,
	WormholeVerifier, D, F, MIN_LEAF_SECURITY_BITS, PUBLIC_INPUTS_FELTS_LEN,
};

/// Header felts of the private-batch PI layout:
/// `num_unique_exits(1) + asset_id(1) + volume_fee_bps(1) + block_hash(4) + block_number(1)`.
/// The payload is padded to one `PUBLIC_INPUTS_FELTS_LEN` block per leaf proof
/// (see `PrivateBatchPublicInputs::try_from_u64_slice` in `qp-wormhole-inputs`).
const PRIVATE_BATCH_PI_HEADER_FELTS: usize = 8;

/// Fixed transaction-pool priority for unsigned wormhole exit submissions.
///
/// Must not be derived from public-input amounts: pool admission
/// (`validate_unsigned`) only runs cheap pre-validation, so those amounts are
/// attacker-controlled. Combined with the nullifier-based `provides` tag, an
/// amount-derived priority would let junk with inflated PIs usurp a victim's
/// same-tag exit (the pool replaces on strictly higher priority). A constant
/// makes first-seen win; amount-based ordering is not needed for `Pays::No`
/// exit traffic.
pub const UNSIGNED_EXIT_PRIORITY: u64 = 1;

/// Hard upper bound on the serialized size of a settlement proof (the `proof_bytes`
/// argument of `verify_private_batch` / `verify_public_batch`), enforced before the
/// blob is copied or parsed.
///
/// Settlement extrinsics are unsigned and fee-free, and pre-validation runs for every
/// gossiped pool candidate, so without this gate the only bound on the bytes an
/// attacker can make every node copy (`to_vec`) and feed through the plonky2 parser
/// is the block-length limit — megabytes above any real proof. Proof sizes are fixed
/// by the compiled circuit dimensions: the current fixtures serialize to ~151 KB
/// (private batch) and ~224 KB (public batch), so 512 KiB leaves ample headroom for
/// circuit-knob growth (proof size scales only mildly with batch counts) while
/// keeping worst-case admission work near real-proof cost. If a circuit upgrade ever
/// pushes a real proof past this cap, `pre_validation_rejects_oversized_proof_bytes`
/// and every fixture-based settlement test will fail loudly at the same time.
pub const MAX_PROOF_BYTES: usize = 512 * 1024;

/// Expected public-input count of the private-batch circuit compiled into this runtime.
fn private_batch_expected_public_inputs() -> usize {
	PRIVATE_BATCH_PI_HEADER_FELTS + circuit_config::NUM_LEAF_PROOFS * PUBLIC_INPUTS_FELTS_LEN
}

/// Expected public-input count of the public-batch circuit compiled into this runtime.
fn public_batch_expected_public_inputs() -> usize {
	qp_wormhole_inputs::public_batch_pi::pi_len(
		circuit_config::NUM_PRIVATE_BATCH_PROOFS,
		circuit_config::NUM_LEAF_PROOFS,
	)
}

/// Canonical circuit config of the private-batch aggregation circuit.
///
/// Must stay identical to `qp_zk_circuits_common::circuit::wormhole_private_batch_circuit_config`,
/// which the build-time circuit generation uses. It is replicated here because
/// `qp-zk-circuits-common` force-enables `qp-plonky2/std` and therefore cannot be a
/// runtime dependency; the `batch_configs_match_circuit_crate` test asserts parity.
fn private_batch_expected_config() -> CircuitConfig {
	CircuitConfig {
		num_wires: 135,
		num_routed_wires: 60,
		..CircuitConfig::standard_recursion_zk_config()
	}
}

/// Canonical circuit config of the public-batch aggregation circuit.
///
/// Must stay identical to `qp_zk_circuits_common::circuit::wormhole_public_batch_circuit_config`
/// (the standard non-ZK recursion config); see [`private_batch_expected_config`] for why it is
/// replicated here.
fn public_batch_expected_config() -> CircuitConfig {
	CircuitConfig::standard_recursion_config()
}

/// Defense-in-depth profile check for batch verifier artifacts, mirroring the
/// audit-hardened `WormholeVerifier::new_from_bytes` canonical-leaf checks.
///
/// The batch artifacts are generated at build time from the pinned circuit crates and
/// their bytes vary with the `QP_NUM_*` sizing env vars, so unlike the leaf path there
/// is no fixed keccak256 commitment to pin them to. The decoded profile is still
/// validated: the circuit config must equal the canonical config for that batch
/// circuit, meet the minimum security-bits floor, and carry exactly the public-input
/// count implied by the compiled batch dimensions.
fn ensure_batch_verifier_profile(
	common: &CommonCircuitData<F, D>,
	expected_config: &CircuitConfig,
	expected_public_inputs: usize,
) -> Result<(), &'static str> {
	if common.config != *expected_config {
		return Err("circuit config does not match the canonical batch circuit config");
	}
	if common.config.security_bits < MIN_LEAF_SECURITY_BITS {
		return Err("circuit config is below the minimum security-bits floor");
	}
	if common.num_public_inputs != expected_public_inputs {
		return Err("public-input count does not match the compiled batch dimensions");
	}
	Ok(())
}

/// Load a batch verifier from pre-serialized verifier-only and common circuit bytes.
///
/// Unlike [`WormholeVerifier::new_from_bytes`], this accepts batch-circuit artifacts
/// (private/public batch) rather than only the canonical leaf circuit pins, so the
/// keccak256 commitment check does not apply; [`ensure_batch_verifier_profile`] is
/// enforced instead.
fn load_batch_verifier_from_bytes(
	verifier_bytes: &[u8],
	common_bytes: &[u8],
	expected_config: &CircuitConfig,
	expected_public_inputs: usize,
	name: &'static str,
) -> Option<WormholeVerifier> {
	let verifier_only = match VerifierOnlyCircuitData::from_bytes(verifier_bytes.to_vec()) {
		Ok(data) => data,
		Err(e) => {
			#[cfg(feature = "std")]
			log::error!("Failed to deserialize {name} verifier-only data: {e}");
			return None;
		},
	};

	let common = match CommonCircuitData::from_bytes(common_bytes.to_vec(), &DefaultGateSerializer)
	{
		Ok(data) => data,
		Err(e) => {
			#[cfg(feature = "std")]
			log::error!("Failed to deserialize {name} common circuit data: {e}");
			return None;
		},
	};

	if let Err(_reason) =
		ensure_batch_verifier_profile(&common, expected_config, expected_public_inputs)
	{
		#[cfg(feature = "std")]
		log::error!(
			"{name} verifier artifact rejected: {_reason} \
			 (security_bits={}, num_public_inputs={}, expected_public_inputs={})",
			common.config.security_bits,
			common.num_public_inputs,
			expected_public_inputs
		);
		return None;
	}

	Some(WormholeVerifier { circuit_data: VerifierCircuitData { verifier_only, common } })
}

#[cfg(any(test, feature = "runtime-benchmarks"))]
mod bench_fixtures;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod migrations;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;
pub use weights::*;

lazy_static! {
	static ref PRIVATE_BATCH_VERIFIER: Option<WormholeVerifier> = {
		let verifier_bytes =
			include_bytes!(concat!(env!("OUT_DIR"), "/private_batch_verifier.bin"));
		let common_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/private_batch_common.bin"));
		load_batch_verifier_from_bytes(
			verifier_bytes,
			common_bytes,
			&private_batch_expected_config(),
			private_batch_expected_public_inputs(),
			"private batch",
		)
	};
	static ref PUBLIC_BATCH_VERIFIER: Option<WormholeVerifier> = {
		let verifier_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/public_batch_verifier.bin"));
		let common_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/public_batch_common.bin"));
		load_batch_verifier_from_bytes(
			verifier_bytes,
			common_bytes,
			&public_batch_expected_config(),
			public_batch_expected_public_inputs(),
			"public batch",
		)
	};
}

/// Circuit sizing constants (generated by `build.rs` from `QP_NUM_*` env vars).
pub mod circuit_config {
	include!(concat!(env!("OUT_DIR"), "/wormhole_circuit_config.rs"));
}

/// Getter for the private-batch proof verifier
pub fn get_private_batch_verifier() -> Result<&'static WormholeVerifier, &'static str> {
	PRIVATE_BATCH_VERIFIER.as_ref().ok_or("Private-batch verifier not available")
}

/// Getter for the public-batch proof verifier
pub fn get_public_batch_verifier() -> Result<&'static WormholeVerifier, &'static str> {
	PUBLIC_BATCH_VERIFIER.as_ref().ok_or("Public-batch verifier not available")
}

/// Scale factor for quantizing amounts from 12 to 2 decimal places (10^10).
/// Amounts in the circuit are stored as u32 with 2 decimal places of precision.
/// On-chain amounts use 12 decimal places, so we multiply by this factor when
/// converting from circuit amounts to on-chain amounts.
pub const SCALE_DOWN_FACTOR: u128 = 10_000_000_000;

#[frame_support::pallet]
pub mod pallet {
	use crate::WeightInfo;
	use alloc::vec::Vec;
	use codec::Decode;
	use frame_support::{
		dispatch::{DispatchErrorWithPostInfo, DispatchResultWithPostInfo, PostDispatchInfo},
		pallet_prelude::*,
		traits::{
			fungible::{Inspect as FungibleInspect, Mutate, Unbalanced},
			Currency,
		},
	};
	use frame_system::pallet_prelude::*;
	use pallet_zk_tree::ZkTreeRecorder;
	use qp_wormhole_verifier::{
		parse_private_batch_public_inputs, parse_public_batch_public_inputs,
		PrivateBatchPublicInputs, ProofWithPublicInputs, PublicBatchPublicInputs, C, D, F,
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

	/// Current storage version of the pallet.
	///
	/// - v1 introduced the (since removed) wormhole soundness counters (`PotentialWormholeBalance`
	///   and `TotalWormholeExits`).
	/// - v2 removes them again: the mechanism only bounded the *rate* of a soundness attacker
	///   rather than stopping one, at considerable complexity. The v1 -> v2 migration deletes the
	///   two counters from storage (see `migrations::v2`).
	///
	/// `TransferCount` is keyed on the Goldilocks-canonical recipient form, but that is enforced
	/// in `record_transfer` at write time (see `canonical_leaf_recipient` and the test
	/// `deposits_to_non_canonical_alias_do_not_collide_with_canonical_leaf`), not by a storage
	/// migration — the layout is unchanged — so it does not carry its own storage version.
	pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(2);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Native balance type for transfer proofs.
		type NativeBalance: Parameter
			+ Member
			+ Default
			+ Copy
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

		/// Asset ID type for transfer proofs.
		type AssetId: Parameter + Member + Default + From<u32> + Clone + MaxEncodedLen;

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
			+ Into<u64>;

		/// Account ID used as the "from" account when creating transfer proofs for minted tokens
		#[pallet::constant]
		type MintingAccount: Get<<Self as frame_system::Config>::AccountId>;

		/// Volume fee rate in basis points (1 basis point = 0.01%).
		/// This must match the fee rate used in proof generation.
		#[pallet::constant]
		type VolumeFeeRateBps: Get<u32>;

		/// Proportion of volume fees to burn (not mint). The remainder goes to the block author.
		/// Example: Permill::from_percent(50) means 50% burned, 50% to miner.
		#[pallet::constant]
		type VolumeFeesBurnRate: Get<Permill>;

		/// For public-batch proofs, the proportion of the burn bucket redirected to the
		/// aggregator instead of being destroyed. The miner's share is unchanged.
		/// Example: Permill::from_percent(50) means half the burn portion goes to the aggregator.
		#[pallet::constant]
		type VolumeFeesAggregatorRate: Get<Permill>;

		/// Weight information for pallet operations.
		type WeightInfo: WeightInfo;

		/// Override system AccountId for wormhole operations
		///
		/// The `AsRef<[u8]>`/`From<[u8; 32]>` bounds allow `record_transfer` to
		/// canonicalize recipients (reduce each 8-byte limb mod the Goldilocks prime)
		/// before keying `TransferCount` and inserting ZK-tree leaves.
		type WormholeAccountId: Parameter
			+ Member
			+ MaybeSerializeDeserialize
			+ core::fmt::Debug
			+ MaybeDisplay
			+ Ord
			+ MaxEncodedLen
			+ AsRef<[u8]>
			+ From<[u8; 32]>
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

	#[pallet::storage]
	#[pallet::getter(fn used_nullifiers)]
	pub(super) type UsedNullifiers<T: Config> =
		StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

	/// Transfer count per recipient - used to generate unique leaf indices in the ZK trie.
	///
	/// Keyed on the *canonical* recipient (each 8-byte limb reduced mod the Goldilocks
	/// prime, matching the ZK leaf encoding — see `canonical_leaf_recipient`), so that a
	/// recipient and its non-canonical byte aliases share one count sequence and two
	/// distinct deposits can never commit to identical leaves.
	#[pallet::storage]
	#[pallet::getter(fn transfer_count)]
	pub type TransferCount<T: Config> =
		StorageMap<_, Blake2_128Concat, T::WormholeAccountId, T::TransferCount, ValueQuery>;

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
		/// The block author's share of the wormhole exit volume fee was minted.
		///
		/// NOTE: keep this as the last variant — indexers decode events by their
		/// position in this enum, so existing variants must never be reordered.
		MinerVolumeFeePaid {
			miner: <T as frame_system::Config>::AccountId,
			amount: BalanceOf<T>,
		},
		/// Some segments of an exit bundle were denied (their nullifiers were already
		/// used, e.g. because the underlying private batch landed on-chain separately).
		/// The remaining segments were processed normally.
		SegmentsDenied {
			indices: Vec<u32>,
		},
		/// An exit slot could not be minted (e.g. a below-existential-deposit credit to
		/// a fresh account) and was skipped so the rest of the bundle still processed.
		/// The skipped exit's nullifier stays marked, so this exit cannot be retried.
		///
		/// NOTE: keep new variants appended at the end — indexers decode events by their
		/// position in this enum, so existing variants must never be reordered.
		ExitMintFailed {
			account: <T as frame_system::Config>::AccountId,
			amount: BalanceOf<T>,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidPublicInputs,
		/// No segment of the bundle is spendable: every non-dummy segment contains a
		/// nullifier that is already used (or the single segment of a private-batch
		/// proof does).
		NullifierAlreadyUsed,
		/// The bundle contains only dummy (all-zero) padding segments, so there is
		/// nothing to exit. Distinct from [`Error::NullifierAlreadyUsed`], which is a
		/// replay of real segments.
		NoValidSegments,
		BlockNotFound,
		VerifierNotAvailable,
		ProofDeserializationFailed,
		/// The submitted proof blob exceeds [`crate::MAX_PROOF_BYTES`]. Rejected before
		/// any copy or parsing so oversized unsigned spam costs only a length check.
		ProofTooLarge,
		/// The proof bytes are not the canonical serialization of the decoded proof
		/// (e.g. a valid proof with trailing bytes, which the plonky2 parser would
		/// silently ignore). Every proof has exactly one accepted byte encoding.
		NonCanonicalProofEncoding,
		ProofVerificationFailed,
		InvalidProofPublicInputs,
		/// The volume fee rate in the proof doesn't match the configured rate
		InvalidVolumeFeeRate,
		/// Only native asset (asset_id = 0) is supported in this version
		NonNativeAssetNotSupported,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// On block 1, record a transfer proof for every account that exists with a
		/// balance — i.e. exactly the genesis balances.
		///
		/// The genesis state is the single source of truth: proofs are *derived* from the
		/// balances actually issued (there is no separate endowment list that could
		/// disagree with them), so an exitable leaf that isn't backed by real issuance is
		/// unrepresentable. This runs before any extrinsic has ever executed, so the
		/// account set observed here is precisely the genesis set.
		///
		/// We do this at block 1 rather than in a genesis build because events emitted
		/// during genesis are not persisted (Substrate limitation); recording here emits
		/// `NativeTransferred` events that indexers like Subsquid can track.
		fn on_initialize(n: BlockNumberFor<T>) -> Weight {
			// Only process on block 1
			if n != One::one() {
				return Weight::zero();
			}

			let minting_account: T::WormholeAccountId = T::MintingAccount::get().into();
			let mut accounts_seen = 0u64;
			let mut recorded = 0u64;

			for who in frame_system::Account::<T>::iter_keys() {
				accounts_seen = accounts_seen.saturating_add(1);
				let amount = <T::Currency as Currency<_>>::total_balance(&who);
				if amount.is_zero() {
					continue;
				}
				let to: T::WormholeAccountId = who.into();
				// Record transfer proof and emit event
				Self::record_transfer(T::AssetId::default(), &minting_account, &to, amount);
				recorded = recorded.saturating_add(1);
			}

			// Weight: 1 read per iterated account + N * (2 reads + 2 writes + 1 event)
			// per recorded proof
			T::DbWeight::get().reads_writes(
				accounts_seen.saturating_add(recorded.saturating_mul(2)),
				recorded.saturating_mul(2),
			)
		}
	}

	/// One deniable unit of an exit bundle: the exits and nullifiers contributed by a
	/// single private-batch proof.
	///
	/// Denial granularity is the segment because the private-batch circuit's exit
	/// grouping sums amounts across leaves — a single nullifier's contribution cannot
	/// be attributed to specific exit slots, so a double-spent nullifier invalidates
	/// its whole segment. Since a private batch is aggregated client-side, a segment
	/// corresponds to one client, and denial never affects other users' exits.
	pub struct ExitSegment {
		pub account_data: Vec<qp_wormhole_verifier::PublicInputsByAccount>,
		pub nullifiers: Vec<qp_wormhole_verifier::BytesDigest>,
	}

	/// Normalized on-chain view of an exit proof, independent of aggregation depth.
	///
	/// A private-batch proof parses into a bundle with exactly one segment. A future
	/// public-batch proof parses into one segment per inner private batch (the
	/// public-batch circuit forwards exits and nullifiers in order, preserving the
	/// per-segment attribution this type relies on).
	pub struct ExitBundle {
		pub asset_id: u32,
		pub volume_fee_bps: u32,
		pub block_data: qp_wormhole_verifier::BlockData,
		/// Set for public-batch proofs; receives a rebate from the burn bucket.
		pub aggregator_address: Option<qp_wormhole_verifier::BytesDigest>,
		pub segments: Vec<ExitSegment>,
	}

	impl From<PrivateBatchPublicInputs> for ExitBundle {
		fn from(inputs: PrivateBatchPublicInputs) -> Self {
			ExitBundle {
				asset_id: inputs.asset_id,
				volume_fee_bps: inputs.volume_fee_bps,
				block_data: inputs.block_data,
				aggregator_address: None,
				segments: alloc::vec![ExitSegment {
					account_data: inputs.account_data,
					nullifiers: inputs.nullifiers,
				}],
			}
		}
	}

	impl ExitBundle {
		/// Build an exit bundle from parsed public-batch public inputs, splitting the
		/// flattened exit slots and nullifiers into one segment per inner private batch.
		pub fn from_public_batch(
			inputs: PublicBatchPublicInputs,
			num_leaf_proofs: usize,
			num_private_batch_proofs: usize,
		) -> Self {
			let slots_per_segment = num_leaf_proofs * 2;
			let nullifiers_per_segment = num_leaf_proofs;

			let mut segments = Vec::with_capacity(num_private_batch_proofs);
			for i in 0..num_private_batch_proofs {
				let account_start = i * slots_per_segment;
				let account_end = account_start + slots_per_segment;
				let null_start = i * nullifiers_per_segment;
				let null_end = null_start + nullifiers_per_segment;

				segments.push(ExitSegment {
					account_data: inputs.account_data[account_start..account_end].to_vec(),
					nullifiers: inputs.nullifiers[null_start..null_end].to_vec(),
				});
			}

			ExitBundle {
				asset_id: inputs.asset_id,
				volume_fee_bps: inputs.volume_fee_bps,
				block_data: inputs.block_data,
				aggregator_address: Some(inputs.aggregator_address),
				segments,
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Verify a private-batch wormhole proof and process all exits in the batch.
		///
		/// Returns `DispatchResultWithPostInfo` to allow weight correction on early failures.
		/// If validation fails before ZK verification, we return minimal weight.
		/// If ZK verification fails, we return full weight since the work was done.
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::verify_private_batch())]
		pub fn verify_private_batch(
			origin: OriginFor<T>,
			proof_bytes: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			ensure_none(origin)?;

			// The ZK verification is the block-inclusion gate in `ValidateUnsigned::pre_dispatch`,
			// which always runs before this dispatch for a bare (unsigned) extrinsic and rejects
			// any proof that fails to verify. This body therefore only re-runs the cheap
			// pre-validation to recover the bundle before processing — re-verifying here would
			// double the ZK-verify cost while the declared weight only accounts for one verify.
			let (_, _, bundle, _) = match Self::pre_validate_private_batch_proof(&proof_bytes) {
				Ok(result) => result,
				Err(e) => {
					// Only cheap pre-validation runs here, so any failure is pre-verify work.
					return Err(DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo {
							actual_weight: Some(<T as Config>::WeightInfo::pre_validate_proof()),
							pays_fee: Pays::No,
						},
						error: e.into(),
					});
				},
			};

			Self::process_exit_bundle(bundle)
		}

		/// Verify a public-batch wormhole proof and process all valid exit segments.
		///
		/// Invalid segments (already-spent nullifiers) are denied individually; dummy-padded
		/// segments (all-zero nullifiers) are skipped silently. A portion of the burn bucket
		/// is minted to the proof's `aggregator_address`; if that mint fails (e.g. the
		/// account doesn't exist and the rebate is below the existential deposit) the
		/// rebate is burned instead of failing the users' exits.
		#[pallet::call_index(3)]
		#[pallet::weight(<T as Config>::WeightInfo::verify_public_batch())]
		pub fn verify_public_batch(
			origin: OriginFor<T>,
			proof_bytes: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			ensure_none(origin)?;

			// ZK verification is the block-inclusion gate in `pre_dispatch` (see
			// `verify_private_batch`); this body only re-runs the cheap pre-validation.
			let (_, _, bundle, _) = match Self::pre_validate_public_batch_proof(&proof_bytes) {
				Ok(result) => result,
				Err(e) => {
					return Err(DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo {
							actual_weight: Some(
								<T as Config>::WeightInfo::pre_validate_public_batch_proof(),
							),
							pays_fee: Pays::No,
						},
						error: e.into(),
					});
				},
			};

			Self::process_exit_bundle(bundle)
		}
	}

	impl<T: Config> Pallet<T> {
		/// A dummy-padded segment: the public-batch circuit zeroes all nullifiers (and
		/// exit slots) for all-dummy inner private batches.
		fn segment_is_inert(segment: &ExitSegment) -> bool {
			segment.nullifiers.iter().all(|n| n.as_ref() == &[0u8; 32])
		}

		/// Compute per-segment validity for an exit bundle.
		///
		/// A segment is valid iff none of its real nullifiers is already used on-chain,
		/// none appeared in an earlier valid segment of the same bundle, and none is
		/// repeated *within* the segment itself. Intra-segment duplicates are rejected
		/// because the private-batch circuit's exit grouping sums a replayed leaf's amount
		/// into a single inflated exit while only one shared nullifier would be marked
		/// spent — accepting the duplicate would mint value backed by a single spend. A
		/// well-formed batch never repeats a real nullifier, so this only rejects replays.
		/// (The circuit now also enforces this, but the check is cheap and kept here as an
		/// in-consensus defense that does not trust the aggregation circuit's constraints.)
		/// Zero nullifiers (from dummy leaf padding inside a real private batch) mint
		/// nothing and are exempt from the collision checks entirely.
		pub(crate) fn segment_validity(bundle: &ExitBundle) -> Result<Vec<bool>, Error<T>> {
			let mut claimed = alloc::collections::BTreeSet::<[u8; 32]>::new();
			let mut validity = Vec::with_capacity(bundle.segments.len());

			for segment in &bundle.segments {
				if Self::segment_is_inert(segment) {
					validity.push(false);
					continue;
				}

				let mut nullifier_bytes = Vec::with_capacity(segment.nullifiers.len());
				for nullifier in &segment.nullifiers {
					let bytes: [u8; 32] = (*nullifier)
						.as_ref()
						.try_into()
						.map_err(|_| Error::<T>::InvalidProofPublicInputs)?;
					if bytes == [0u8; 32] {
						continue;
					}
					nullifier_bytes.push(bytes);
				}

				// A real nullifier repeated within this segment can only be a leaf proof
				// replayed across slots; reject the whole segment before it can mint the
				// summed, inflated exit. `insert` returns false the first time a value
				// repeats, so `all` is false iff any duplicate exists.
				let mut seen = alloc::collections::BTreeSet::<[u8; 32]>::new();
				let intra_segment_unique = nullifier_bytes.iter().all(|bytes| seen.insert(*bytes));

				let valid = intra_segment_unique &&
					nullifier_bytes.iter().all(|bytes| {
						!UsedNullifiers::<T>::contains_key(bytes) && !claimed.contains(bytes)
					});

				if valid {
					claimed.extend(nullifier_bytes);
				}
				validity.push(valid);
			}

			Ok(validity)
		}

		/// Reject a bundle in which no segment is valid, distinguishing an all-dummy
		/// bundle (nothing to exit) from a replay of real segments.
		fn ensure_any_segment_valid(
			bundle: &ExitBundle,
			validity: &[bool],
		) -> Result<(), Error<T>> {
			if validity.iter().any(|v| *v) {
				return Ok(());
			}
			if bundle.segments.iter().all(Self::segment_is_inert) {
				Err(Error::<T>::NoValidSegments)
			} else {
				Err(Error::<T>::NullifierAlreadyUsed)
			}
		}

		/// Process a validated exit bundle: mark nullifiers, mint exits, distribute fees.
		///
		/// Invalid segments (a nullifier already used on-chain, or colliding with an earlier
		/// valid segment of this bundle) are denied as a whole — none of their exits are
		/// minted and none of their nullifiers are marked — while the remaining segments
		/// are processed normally. The bundle is rejected outright if no segment is valid.
		///
		/// Validity is recomputed here rather than reused from `validate_proof` because
		/// chain state may have changed between pool validation and block inclusion.
		pub(crate) fn process_exit_bundle(bundle: ExitBundle) -> DispatchResultWithPostInfo {
			let validity = Self::segment_validity(&bundle)?;
			Self::ensure_any_segment_valid(&bundle, &validity)?;

			// Get the minting account for recording transfer proofs
			let mint_account = T::MintingAccount::get();

			let mut nullifier_list = Vec::<[u8; 32]>::new();
			let mut denied_segments = Vec::<u32>::new();
			let mut processed_accounts: Vec<(
				<T as frame_system::Config>::AccountId,
				BalanceOf<T>,
			)> = Vec::new();

			for (seg_idx, (segment, valid)) in
				bundle.segments.iter().zip(validity.iter()).enumerate()
			{
				if Self::segment_is_inert(segment) {
					continue;
				}

				if !*valid {
					denied_segments.push(seg_idx as u32);
					continue;
				}

				// Mark nullifiers as used (validate_proof only checks existence)
				for nullifier in &segment.nullifiers {
					let nullifier_bytes: [u8; 32] = (*nullifier)
						.as_ref()
						.try_into()
						.map_err(|_| Error::<T>::InvalidProofPublicInputs)?;
					if nullifier_bytes == [0u8; 32] {
						continue;
					}
					UsedNullifiers::<T>::insert(nullifier_bytes, true);
					nullifier_list.push(nullifier_bytes);
				}

				// Compute exit amounts and prepare account data
				for (idx, account_data) in segment.account_data.iter().enumerate() {
					// Skip dummy account slots (exit_account == 0 with zero amount)
					// Dummy proofs from aggregation padding have all-zero exit accounts
					// Also skip deduplicated slots (the circuit zeros out duplicate exit accounts)
					let exit_account_bytes: [u8; 32] =
						(*account_data.exit_account).as_ref().try_into().map_err(|e| {
							log::error!("Failed to convert exit_account at idx {}: {:?}", idx, e);
							Error::<T>::InvalidProofPublicInputs
						})?;

					if exit_account_bytes == [0u8; 32] || account_data.summed_output_amount == 0 {
						continue;
					}

					// Convert output amount to Balance type (scale up from quantized value)
					let exit_balance_u128 = (account_data.summed_output_amount as u128)
						.saturating_mul(crate::SCALE_DOWN_FACTOR);
					let exit_balance: BalanceOf<T> =
						exit_balance_u128.try_into().map_err(|_| {
							log::error!("Failed to convert exit_balance at idx {}", idx);
							Error::<T>::InvalidProofPublicInputs
						})?;

					// Decode exit account from public inputs
					let exit_account = <T as frame_system::Config>::AccountId::decode(
						&mut &exit_account_bytes[..],
					)
					.map_err(|_| Error::<T>::InvalidProofPublicInputs)?;

					processed_accounts.push((exit_account, exit_balance));
				}
			}

			// Surface denied segments so aggregators can observe races and clients can
			// detect that their private batch was consumed by someone else's bundle.
			if !denied_segments.is_empty() {
				Self::deposit_event(Event::SegmentsDenied { indices: denied_segments });
			}

			// Mint exits first; fees and ProofVerified use only successfully minted amounts.
			let mut minted_exit_amount: BalanceOf<T> = Zero::zero();
			for (exit_account, exit_balance) in &processed_accounts {
				// Skip failed credits (e.g. below ED); nullifier already marked, value
				// excluded from fee settlement / event.
				match Self::credit_and_record(exit_account, *exit_balance, &mint_account) {
					Ok(_) => {},
					Err(e) => {
						log::warn!(
							"Exit mint of {:?} to {:?} failed ({:?}); skipping this exit",
							*exit_balance,
							exit_account,
							e
						);
						Self::deposit_event(Event::ExitMintFailed {
							account: exit_account.clone(),
							amount: *exit_balance,
						});
						continue;
					},
				}

				minted_exit_amount = minted_exit_amount.saturating_add(*exit_balance);
			}

			Self::deposit_event(Event::ProofVerified {
				exit_amount: minted_exit_amount,
				nullifiers: nullifier_list,
			});

			let total_exit_u128: u128 = minted_exit_amount.try_into().map_err(|_| {
				log::error!("Failed to convert minted_exit_amount to u128");
				Error::<T>::InvalidProofPublicInputs
			})?;
			let total_fee_u128 =
				Self::volume_fee_for_exit(total_exit_u128, T::VolumeFeeRateBps::get());

			// Fee distribution: configurable portion burned, remainder to miner
			//
			// Original deposit locked `input_amount` in an unspendable account (tokens still
			// exist). On exit we mint `output_amount` to user, where: input >= output + fee
			//
			// Fee split (controlled by VolumeFeesBurnRate):
			//   - burn_amount = fee * burn_rate  (reduces total issuance via Currency::burn)
			//   - miner_fee = fee - burn_amount  (credited to block author with a zk-tree leaf)
			//
			// Supply accounting:
			//   - Minting exit amounts: increases balances but NOT issuance by sum(output_amounts)
			//   - Minting miner fee: increases balance but NOT issuance (increase_balance)
			//   - Burning: decreases total issuance by burn_amount
			//   - Net change: +sum(output_amounts) - burn_amount
			let burn_rate = T::VolumeFeesBurnRate::get();
			let mut burn_amount_u128 = burn_rate * total_fee_u128;
			let miner_fee_u128 = total_fee_u128.saturating_sub(burn_amount_u128);

			// Public-batch aggregator rebate: redirect part of the burn bucket to the
			// aggregator. The miner's share is unchanged.
			if let Some(aggregator_address) = &bundle.aggregator_address {
				let aggregator_rate = T::VolumeFeesAggregatorRate::get();
				let aggregator_fee_u128 = aggregator_rate * burn_amount_u128;
				burn_amount_u128 = burn_amount_u128.saturating_sub(aggregator_fee_u128);

				if aggregator_fee_u128 > 0 {
					let aggregator_fee: BalanceOf<T> =
						aggregator_fee_u128.try_into().map_err(|_| {
							log::error!("Failed to convert aggregator_fee_u128 to BalanceOf");
							Error::<T>::InvalidProofPublicInputs
						})?;

					let aggregator_bytes: [u8; 32] = (*aggregator_address)
						.as_ref()
						.try_into()
						.map_err(|_| Error::<T>::InvalidProofPublicInputs)?;
					let aggregator_account =
						<T as frame_system::Config>::AccountId::decode(&mut &aggregator_bytes[..])
							.map_err(|_| Error::<T>::InvalidProofPublicInputs)?;

					// A failed rebate mint (e.g. the aggregator account doesn't exist
					// and the rebate is below the existential deposit) must not revert
					// the whole bundle - that would drag users' exits down with a
					// problem the aggregator inflicted on itself. Burn the rebate
					// instead and process the exits normally.
					match Self::credit_and_record(
						&aggregator_account,
						aggregator_fee,
						&mint_account,
					) {
						Ok(_) => {},
						Err(e) => {
							log::warn!(
								"Aggregator rebate of {:?} could not be minted ({:?}); burning it instead",
								aggregator_fee,
								e
							);
							burn_amount_u128 = burn_amount_u128.saturating_add(aggregator_fee_u128);
						},
					}
				}
			}

			let miner_fee: BalanceOf<T> = miner_fee_u128.try_into().map_err(|_| {
				log::error!("Failed to convert miner_fee_u128 to BalanceOf");
				Error::<T>::InvalidProofPublicInputs
			})?;

			// Mint miner's portion of volume fee to block author
			// If no author is found, add to burn amount instead of silently losing it
			if !miner_fee.is_zero() {
				let digest = frame_system::Pallet::<T>::digest();
				if let Some(author) = qp_wormhole::extract_author_from_digest::<
					<T as frame_system::Config>::AccountId,
					_,
				>(digest.logs.iter())
				{
					// A failed miner-fee mint (e.g. the author account doesn't exist and
					// the fee is below the existential deposit) must not revert the whole
					// bundle and drag users' exits down with it. Burn it instead, exactly
					// like the no-author branch below and the aggregator-rebate fallback.
					match Self::credit_and_record(&author, miner_fee, &mint_account) {
						Ok(_) => {
							Self::deposit_event(Event::MinerVolumeFeePaid {
								miner: author,
								amount: miner_fee,
							});
						},
						Err(e) => {
							log::warn!(
								"Miner fee of {:?} could not be minted ({:?}); burning it instead",
								miner_fee,
								e
							);
							burn_amount_u128 = burn_amount_u128.saturating_add(miner_fee_u128);
						},
					}
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
				Error::<T>::InvalidProofPublicInputs
			})?;
			if !burn_amount.is_zero() {
				let current = <T::Currency as FungibleInspect<_>>::total_issuance();
				<T::Currency as Unbalanced<_>>::set_total_issuance(
					current.saturating_sub(burn_amount),
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
			// Pool admission runs on every gossiped transaction, so it deliberately performs
			// only the cheap pre-validation (deserialize, parse, bundle checks) and NOT the
			// expensive ZK verification. The full verify is the block-inclusion gate in
			// `pre_dispatch`; deferring it here prevents unsigned, fee-free traffic from
			// forcing unbounded verification work per gossiped byte-variant of a proof.
			match call {
				Call::verify_private_batch { proof_bytes } => {
					// `validity` is computed for its side-effect of rejecting
					// wholly-unspendable / all-dummy bundles via the cheap checks;
					// priority must not read amounts from it (see UNSIGNED_EXIT_PRIORITY).
					let (_, _, bundle, _validity) =
						Self::pre_validate_private_batch_proof(proof_bytes)
							.map_err(|_| InvalidTransaction::Call)?;

					ValidTransaction::with_tag_prefix("WormholePrivateBatch")
						.and_provides(Self::exit_bundle_provides_tag(&bundle))
						.priority(crate::UNSIGNED_EXIT_PRIORITY)
						.longevity(5)
						.propagate(true)
						.build()
				},
				Call::verify_public_batch { proof_bytes } => {
					// Same as the private-batch path: cheap checks only, fixed priority.
					let (_, _, bundle, _validity) =
						Self::pre_validate_public_batch_proof(proof_bytes)
							.map_err(|_| InvalidTransaction::Call)?;

					ValidTransaction::with_tag_prefix("WormholePublicBatch")
						.and_provides(Self::exit_bundle_provides_tag(&bundle))
						.priority(crate::UNSIGNED_EXIT_PRIORITY)
						.longevity(5)
						.propagate(true)
						.build()
				},
				_ => InvalidTransaction::Call.into(),
			}
		}

		fn pre_dispatch(call: &Self::Call) -> Result<(), TransactionValidityError> {
			// Block-inclusion gate. Unlike `validate_unsigned` (pool admission), this runs
			// the FULL validation including ZK verification, and returning `Err` here
			// excludes the transaction from the block being built (and makes a block that
			// includes an unverifiable proof invalid on import). This is what keeps
			// unverified/junk proofs out of blocks now that pool admission is verify-free.
			match call {
				Call::verify_private_batch { proof_bytes } =>
					Self::validate_private_batch_proof(proof_bytes)
						.map(|_| ())
						.map_err(|_| InvalidTransaction::Call.into()),
				Call::verify_public_batch { proof_bytes } =>
					Self::validate_public_batch_proof(proof_bytes)
						.map(|_| ())
						.map_err(|_| InvalidTransaction::Call.into()),
				_ => Err(InvalidTransaction::Call.into()),
			}
		}
	}

	impl<T: Config> Pallet<T> {
		/// Volume fee owed on `minted_exit` base units at `fee_bps`, in base units.
		///
		/// Mirrors the circuit's integer fee relation over QUANTIZED amounts
		/// (`out · 10000 ≤ input · (10000 − bps)`, see `docs/wormhole-zk.md`):
		/// `fee_quanta = ceil(exit_quanta · bps / (10000 − bps))`, which is exactly
		/// the minimum fee gap the proofs lock. In particular the smallest valid
		/// exit (one quantum out forces `input ≥ 2` quanta) settles a full
		/// one-quantum fee, where truncating base-unit division would settle only
		/// `bps / (10000 − bps)` of a quantum. Computed over the bundle's minted
		/// total, the result never exceeds what the individual proofs locked
		/// (ceil of a sum ≤ sum of ceils).
		///
		/// `minted_exit` is always a whole number of quanta: every exit balance is
		/// a quantized `u32` output times [`SCALE_DOWN_FACTOR`], so the quantum
		/// division below is exact. A degenerate `fee_bps >= 10000` yields a zero
		/// fee, matching the previous `checked_div(..).unwrap_or(0)` behaviour.
		pub(crate) fn volume_fee_for_exit(minted_exit: u128, fee_bps: u32) -> u128 {
			let fee_bps = fee_bps as u128;
			let denominator = 10_000u128.saturating_sub(fee_bps);
			if denominator == 0 {
				return 0;
			}
			(minted_exit / crate::SCALE_DOWN_FACTOR)
				.saturating_mul(fee_bps)
				.div_ceil(denominator)
				.saturating_mul(crate::SCALE_DOWN_FACTOR)
		}

		/// Shared cheap checks for any exit bundle (asset, fee, block, segment validity).
		pub(crate) fn validate_exit_bundle_common(
			bundle: &ExitBundle,
		) -> Result<Vec<bool>, Error<T>> {
			ensure!(bundle.asset_id == 0, Error::<T>::NonNativeAssetNotSupported);
			ensure!(
				bundle.volume_fee_bps == T::VolumeFeeRateBps::get(),
				Error::<T>::InvalidVolumeFeeRate
			);
			let block_number = BlockNumberFor::<T>::from(bundle.block_data.block_number);
			let block_hash = frame_system::Pallet::<T>::block_hash(block_number);
			ensure!(block_hash != T::Hash::default(), Error::<T>::BlockNotFound);
			ensure!(
				block_hash.as_ref() == bundle.block_data.block_hash.as_ref(),
				Error::<T>::InvalidPublicInputs
			);

			let validity = Self::segment_validity(bundle)?;
			Self::ensure_any_segment_valid(bundle, &validity)?;
			Ok(validity)
		}

		/// Cheap pre-validation of a private-batch proof: deserialize, parse public
		/// inputs, and run the cheap bundle checks — but **not** the expensive ZK
		/// verification. Returns the verifier and deserialized proof (so a caller can
		/// optionally run the ZK verify), plus the parsed bundle and per-segment validity.
		///
		/// This is the work performed on the transaction-pool admission path
		/// (`validate_unsigned`), where running the full ZK verify would let unsigned,
		/// fee-free traffic force unbounded verification work per gossiped proof. The
		/// full verification is deferred to the block-inclusion gate (`pre_dispatch`).
		pub(crate) fn pre_validate_private_batch_proof(
			proof_bytes: &[u8],
		) -> Result<
			(
				&'static crate::WormholeVerifier,
				ProofWithPublicInputs<F, C, D>,
				ExitBundle,
				Vec<bool>,
			),
			Error<T>,
		> {
			// Length gate FIRST: `proof_bytes` is attacker-controlled, unsigned and
			// fee-free, and everything below copies (`to_vec`) and parses the whole
			// blob. Without this bound the only limit is the block-length cap,
			// megabytes above any real proof.
			ensure!(proof_bytes.len() <= crate::MAX_PROOF_BYTES, Error::<T>::ProofTooLarge);
			let verifier = crate::get_private_batch_verifier()
				.map_err(|_| Error::<T>::VerifierNotAvailable)?;
			let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.to_vec(),
				&verifier.circuit_data.common,
			)
			.map_err(|_| Error::<T>::ProofDeserializationFailed)?;
			// Exact-framing check: `from_bytes` reads the proof off the front of the
			// buffer and silently ignores trailing bytes, so without this a valid
			// proof would have unboundedly many accepted byte representations — each
			// a distinct tx hash whose copy+parse the pool re-pays at admission.
			// Round-tripping pins one canonical encoding per proof (and also rejects
			// non-canonical field encodings).
			ensure!(
				proof.to_bytes().as_slice() == proof_bytes,
				Error::<T>::NonCanonicalProofEncoding
			);
			let inputs = parse_private_batch_public_inputs(&proof)
				.map_err(|_| Error::<T>::InvalidProofPublicInputs)?;
			let bundle: ExitBundle = inputs.into();

			let validity = Self::validate_exit_bundle_common(&bundle)?;

			Ok((verifier, proof, bundle, validity))
		}

		/// Validate a private-batch proof (cheap checks + full ZK verification).
		fn validate_private_batch_proof(
			proof_bytes: &[u8],
		) -> Result<(ExitBundle, Vec<bool>), Error<T>> {
			let (verifier, proof, bundle, validity) =
				Self::pre_validate_private_batch_proof(proof_bytes)?;

			verifier.verify(proof).map_err(|e| {
				log::error!("Private-batch proof verification failed: {:?}", e);
				Error::<T>::ProofVerificationFailed
			})?;

			Ok((bundle, validity))
		}

		/// Cheap pre-validation of a public-batch proof (no ZK verify). See
		/// [`Self::pre_validate_private_batch_proof`].
		pub(crate) fn pre_validate_public_batch_proof(
			proof_bytes: &[u8],
		) -> Result<
			(
				&'static crate::WormholeVerifier,
				ProofWithPublicInputs<F, C, D>,
				ExitBundle,
				Vec<bool>,
			),
			Error<T>,
		> {
			// Same gates as `pre_validate_private_batch_proof`: length bound before
			// any copy/parse, then exact canonical framing after deserialization.
			ensure!(proof_bytes.len() <= crate::MAX_PROOF_BYTES, Error::<T>::ProofTooLarge);
			let verifier =
				crate::get_public_batch_verifier().map_err(|_| Error::<T>::VerifierNotAvailable)?;
			let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
				proof_bytes.to_vec(),
				&verifier.circuit_data.common,
			)
			.map_err(|_| Error::<T>::ProofDeserializationFailed)?;
			ensure!(
				proof.to_bytes().as_slice() == proof_bytes,
				Error::<T>::NonCanonicalProofEncoding
			);
			let inputs = parse_public_batch_public_inputs(
				&proof,
				crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
				crate::circuit_config::NUM_LEAF_PROOFS,
			)
			.map_err(|_| Error::<T>::InvalidProofPublicInputs)?;
			let bundle = ExitBundle::from_public_batch(
				inputs,
				crate::circuit_config::NUM_LEAF_PROOFS,
				crate::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
			);

			let validity = Self::validate_exit_bundle_common(&bundle)?;

			Ok((verifier, proof, bundle, validity))
		}

		/// Validate a public-batch proof (cheap checks + full ZK verification).
		fn validate_public_batch_proof(
			proof_bytes: &[u8],
		) -> Result<(ExitBundle, Vec<bool>), Error<T>> {
			let (verifier, proof, bundle, validity) =
				Self::pre_validate_public_batch_proof(proof_bytes)?;

			verifier.verify(proof).map_err(|e| {
				log::error!("Public-batch proof verification failed: {:?}", e);
				Error::<T>::ProofVerificationFailed
			})?;

			Ok((bundle, validity))
		}

		/// Stable, semantic transaction-pool dedup tag for an exit bundle: a hash of the
		/// bundle's nullifiers. Two encodings of the same logical exit (e.g. a proof
		/// resubmitted with a mutated non-PI byte) share nullifiers and therefore collide
		/// on this tag, so the pool holds one entry per logical exit instead of one per
		/// distinct byte string (which `blake2_256(proof_bytes)` allowed an attacker to
		/// bypass with a single-byte change).
		fn exit_bundle_provides_tag(bundle: &ExitBundle) -> [u8; 32] {
			let mut preimage = Vec::new();
			for segment in &bundle.segments {
				for nullifier in &segment.nullifiers {
					preimage.extend_from_slice(nullifier.as_ref());
				}
			}
			sp_io::hashing::blake2_256(&preimage)
		}

		/// Canonical form of a recipient for ZK-leaf keying.
		///
		/// Reduces each 8-byte limb of the 32-byte account mod the Goldilocks prime —
		/// the exact reduction the leaf hash's lossy encoding applies — so that
		/// `TransferCount` and the stored leaf recipient are keyed per leaf-encoding
		/// class rather than per raw byte string. The withdrawal circuit binds the
		/// leaf's recipient felts to a canonical Poseidon output, so canonicalizing
		/// never changes who can exit a leaf. Non-32-byte accounts are returned
		/// unchanged (the leaf hash requires 32-byte accounts anyway).
		fn canonical_leaf_recipient(to: &T::WormholeAccountId) -> T::WormholeAccountId {
			match <[u8; 32]>::try_from(to.as_ref()) {
				Ok(bytes) => pallet_zk_tree::tree::canonicalize_account_bytes(bytes).into(),
				Err(_) => to.clone(),
			}
		}

		/// Credit `amount` to `to` via event-free `increase_balance` and insert a
		/// zk-tree leaf so the credit is exitable.
		///
		/// Must stay paired: wormhole-derived accounts have no signing key, so a
		/// balance without a leaf is permanently frozen. Must stay
		/// `Unbalanced::increase_balance` (not `mint_into`): the runtime's
		/// `WormholeProofRecorderExtension` would otherwise double-record from the
		/// `Minted` event. Pinned by `exit_credits_emit_no_scannable_transfer_events`
		/// and the miner-fee / aggregator-rebate leaf tests.
		fn credit_and_record(
			to: &<T as frame_system::Config>::AccountId,
			amount: BalanceOf<T>,
			mint_account: &<T as frame_system::Config>::AccountId,
		) -> Result<BalanceOf<T>, DispatchError> {
			let credited = <T::Currency as Unbalanced<_>>::increase_balance(
				to,
				amount,
				frame_support::traits::tokens::Precision::Exact,
			)?;
			let from_account: <T as Config>::WormholeAccountId = mint_account.clone().into();
			let to_account: <T as Config>::WormholeAccountId = to.clone().into();
			Self::record_transfer(T::AssetId::default(), &from_account, &to_account, amount);
			Ok(credited)
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
			// The ZK leaf commits to the recipient through a lossy encoding that reduces
			// each 8-byte limb mod the Goldilocks prime, so a recipient and its
			// non-canonical byte aliases encode identically. Key the transfer count and
			// the stored leaf on the canonical form so every deposit gets a unique
			// (recipient-class, count) pair — otherwise a deposit to an alias would start
			// its own count at 0 and could commit to the same leaf (and thus the same
			// `H(secret, transfer_count)` nullifier) as a canonical deposit, leaving one
			// of the two permanently unexitable.
			let leaf_to = Self::canonical_leaf_recipient(to);
			let current_count = TransferCount::<T>::get(&leaf_to);

			// Increment transfer count for this recipient
			TransferCount::<T>::insert(
				&leaf_to,
				current_count.saturating_add(T::TransferCount::one()),
			);

			// Insert into ZK tree for Merkle proof generation
			// Returns the leaf index for clients to use when fetching proofs
			let leaf_index = T::ZkTree::record_transfer(
				leaf_to.into(),
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
		) -> bool {
			// A zero-amount credit moves no value, so a leaf for it is pure state growth:
			// it would advance the recipient's transfer count, enlarge the ZK tree, and
			// emit a transfer event for nothing. Zero-value `Balances::Transfer` events
			// are reachable from permissionless surfaces (plain `transfer_keep_alive(0)`,
			// zero-value scheduled transfers, ...), so drop the credit here — the single
			// chokepoint every event-scan / call-site recorder goes through — and report
			// it as not recorded so weight reconciliation does not count a leaf insert.
			if amount.is_zero() {
				return false;
			}
			// The wormhole tags native leaves with `asset_id == 0`, but `pallet_assets` uses
			// id 0 for an unrelated, independently-mintable token. Genuine native reaches us as
			// `None` (from `Balances` events); a `pallet_assets` asset-0 credit reaches us as
			// `Some(0)` (from `Assets::Issued`). These must not be conflated: a `Some(0)` credit
			// is backed by no native, so recording it as a native deposit would inflate the
			// native potential-balance pool and insert a natively-exitable leaf, letting an
			// asset-0 issuer mint unbacked native out of the wormhole.
			match asset_id {
				// Native token.
				None => {
					Self::record_transfer(T::AssetId::default(), &from, &to, amount);
					true
				},
				// A `pallet_assets` asset whose id collides with the reserved native id. The
				// wormhole only supports native exits, and this is not a native deposit, so it
				// must not touch native accounting — drop it.
				Some(id) if id == T::AssetId::default() => {
					log::warn!(
						target: "runtime::wormhole",
						"Dropping pallet_assets asset-0 credit (not native): from={:?} to={:?} amount={:?}",
						from,
						to,
						amount,
					);
					false
				},
				// A genuine non-native asset: recorded as an inert (non-native, never-exitable)
				// leaf, preserving the existing behaviour for future asset-wormhole support.
				Some(id) => {
					Self::record_transfer(id, &from, &to, amount);
					true
				},
			}
		}
	}
}
