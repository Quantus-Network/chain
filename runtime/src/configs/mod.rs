// This is free and unencumbered software released into the public domain.
//
// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.
//
// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.
//
// For more information, please refer to <http://unlicense.org>

// Substrate and Polkadot dependencies
use crate::{
	governance::definitions::{
		CommunityTracksInfo, GlobalMaxMembers, MinRankOfClassConverter, PreimageDeposit,
		RootOrMemberForCollectiveOrigin, RootOrMemberForTechReferendaOrigin,
		TechCollectiveTracksInfo,
	},
	MILLI_UNIT,
};
use frame_support::{
	derive_impl, parameter_types,
	traits::{
		AsEnsureOriginWithArg, ConstU128, ConstU32, ConstU8, Get, NeverEnsureOrigin, VariantCountOf,
	},
	weights::{
		constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
		IdentityFee, Weight, WeightToFeeCoefficient, WeightToFeeCoefficients,
		WeightToFeePolynomial,
	},
	PalletId,
};
use frame_system::{
	limits::{BlockLength, BlockWeights},
	EnsureRoot, EnsureSigned,
};
use pallet_ranked_collective::Linear;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use smallvec::smallvec;

use qp_scheduler::BlockNumberOrTimestamp;
use sp_runtime::{
	traits::{BlakeTwo256, One},
	AccountId32, FixedU128, Perbill, Permill,
};
use sp_version::RuntimeVersion;

// Local module imports
use super::{
	AccountId, Assets, Balance, Balances, Block, BlockNumber, Hash, Nonce, OriginCaller,
	PalletInfo, Preimage, Referenda, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason,
	RuntimeHoldReason, RuntimeOrigin, RuntimeTask, Scheduler, System, Timestamp, Wormhole, ZkTree,
	DAYS, EXISTENTIAL_DEPOSIT, MICRO_UNIT, TARGET_BLOCK_TIME_MS, UNIT, VERSION,
};
use sp_core::U512;

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
	pub const BlockHashCount: BlockNumber = 4096;
	pub const Version: RuntimeVersion = VERSION;

	/// Block weight limits for the runtime.
	///
	/// - `ref_time`: 6 seconds of compute (with 12 second block time, this leaves headroom)
	/// - `proof_size`: Set to u64::MAX (uncapped) - this is intentional for a solo PoW chain
	///   where stateless validation and PoV limits don't apply.
	///
	/// See "Proof Size Design Rationale" in the Transaction Fee Structure section below
	/// for detailed explanation of why proof_size is uncapped and when to revisit this.
	pub RuntimeBlockWeights: BlockWeights = BlockWeights::with_sensible_defaults(
		Weight::from_parts(6u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
		NORMAL_DISPATCH_RATIO,
	);
	/// Maximum block length (5 MB).
	///
	/// Estimated network transfer times:
	/// - Download: 100 Mbps link ~600ms, 1 Gbps link ~200ms
	/// - Upload: 10 Mbps link ~4.1s, 100 Mbps link ~500ms
	pub RuntimeBlockLength: BlockLength = BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
	pub const SS58Prefix: u8 = 189;
}

/// The default types are being injected by [`derive_impl`](`frame_support::derive_impl`) from
/// [`SoloChainDefaultConfig`](`struct@frame_system::config_preludes::SolochainDefaultConfig`),
/// but overridden as needed.
#[derive_impl(frame_system::config_preludes::SolochainDefaultConfig)]
impl frame_system::Config for Runtime {
	/// The block type for the runtime.
	type Block = Block;
	/// Block & extrinsics weights: base values and limits.
	type BlockWeights = RuntimeBlockWeights;
	/// The maximum length of a block (in bytes).
	type BlockLength = RuntimeBlockLength;
	/// The identifier used to distinguish between accounts.
	type AccountId = AccountId;

	type Lookup = sp_runtime::traits::AccountIdLookup<Self::AccountId, ()>;
	/// The type for storing how many extrinsics an account has signed.
	type Nonce = Nonce;
	/// The type for hashing blocks and tries.
	type Hash = Hash;
	/// The hashing algorithm used for state trie and extrinsics root.
	/// This matches the `StateHash` parameter in qp_header::Header.
	type Hashing = BlakeTwo256;
	/// Maximum number of block number to block hash mappings to keep (oldest pruned first).
	type BlockHashCount = BlockHashCount;
	/// The weight of database operations that the runtime can invoke.
	type DbWeight = RocksDbWeight;
	/// Version of the runtime.
	type Version = Version;
	/// The data to be stored in an account.
	type AccountData = pallet_balances::AccountData<Balance>;
	/// This is used as an identifier of the chain. 42 is the generic substrate prefix.
	type SS58Prefix = SS58Prefix;
	type MaxConsumers = ConstU32<16>;
}

parameter_types! {
	pub const MiningUnit: Balance = UNIT;
}

impl pallet_mining_rewards::Config for Runtime {
	type Currency = Balances;
	type AssetId = AssetId;
	type ProofRecorder = Wormhole;
	type WeightInfo = pallet_mining_rewards::weights::SubstrateWeight<Runtime>;
	type MaxSupply = ConstU128<{ 21_000_000 * UNIT }>; // 21 million tokens
	type EmissionDivisor = ConstU128<26_280_000>; // Divide remaining supply by this amount
	type Treasury = pallet_treasury::Pallet<Runtime>;
	type MintingAccount = MintingAccount;
	type Unit = MiningUnit;
}

parameter_types! {
	/// Target block time ms
	pub const TargetBlockTime: u64 = TARGET_BLOCK_TIME_MS;
	pub const TimestampBucketSize: u64 = 2 * TARGET_BLOCK_TIME_MS; // Nyquist frequency
	/// Initial mining difficulty - low value for development
	pub const QPoWInitialDifficulty: U512 = U512([1189189, 0, 0, 0, 0, 0, 0, 0]);
	/// Difficulty adjustment percent clamp
	pub const DifficultyAdjustPercentClamp: FixedU128 = FixedU128::from_rational(10, 100);
}

impl pallet_qpow::Config for Runtime {
	// Starting difficulty - should be challenging enough to require some work but not too high
	type InitialDifficulty = QPoWInitialDifficulty;
	type DifficultyAdjustPercentClamp = DifficultyAdjustPercentClamp;
	type TargetBlockTime = TargetBlockTime;
	type MaxReorgDepth = ConstU32<180>;

	type WeightInfo = ();
	type EmaAlpha = ConstU32<100>; // out of 1000, last_block_time * alpha + (previous_ema * (1 - alpha)) on moving average
}

parameter_types! {
	/// Canonical minting account for native token operations (mining rewards, wormhole exits).
	/// Used as the `from` address in TransferProofs when native tokens are minted.
	/// This is a well-known sentinel address, not a real account.
	pub const MintingAccount: AccountId = AccountId::new([1u8; 32]);
	/// Canonical minting account for pallet_assets mint operations.
	/// Used as the `from` address in TransferProofs when assets are minted.
	/// This is a well-known sentinel address, not a real account.
	pub const AssetMintingAccount: AccountId = AccountId::new([2u8; 32]);
}

type Moment = u64;

parameter_types! {
	pub const MinimumPeriod: u64 = 100;
}

impl pallet_timestamp::Config for Runtime {
	/// A timestamp: milliseconds since the unix epoch.
	type Moment = Moment;
	type OnTimestampSet = ();
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

parameter_types! {
	pub const ExistentialDeposit: Balance = EXISTENTIAL_DEPOSIT;
}

impl pallet_balances::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
	type RuntimeFreezeReason = RuntimeFreezeReason;
	type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
	/// The type for recording an account's balance.
	type Balance = Balance;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type ReserveIdentifier = [u8; 8];
	type FreezeIdentifier = RuntimeFreezeReason;
	type MaxLocks = ConstU32<50>;
	type MaxReserves = ();
	type MaxFreezes = VariantCountOf<RuntimeFreezeReason>;
	type DoneSlashHandler = ();
}

parameter_types! {
	pub const VoteLockingPeriod: BlockNumber = 7 * DAYS;
	pub const MaxVotes: u32 = 4096;
}

/// Dynamic MaxTurnout that uses the current total issuance of tokens
/// This makes governance support thresholds automatically scale with token supply
pub struct DynamicMaxTurnout;

impl Get<Balance> for DynamicMaxTurnout {
	fn get() -> Balance {
		// Use current total issuance as MaxTurnout
		// This ensures support thresholds scale with actual token supply
		Balances::total_issuance()
	}
}

impl pallet_conviction_voting::Config for Runtime {
	type WeightInfo = pallet_conviction_voting::weights::SubstrateWeight<Runtime>;
	type Currency = Balances;
	type RuntimeEvent = RuntimeEvent;
	type VoteLockingPeriod = VoteLockingPeriod;
	type MaxVotes = MaxVotes;
	type MaxTurnout = DynamicMaxTurnout;
	type Polls = Referenda;
	type BlockNumberProvider = System;
	type VotingHooks = ();
}

parameter_types! {
	pub const PreimageBaseDeposit: Balance = UNIT;
	pub const PreimageByteDeposit: Balance = MICRO_UNIT;
}

impl pallet_preimage::Config for Runtime {
	type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type ManagerOrigin = EnsureRoot<AccountId>;
	type Consideration = PreimageDeposit;
}

parameter_types! {
	// Default voting period (28 days)
	pub const ReferendumDefaultVotingPeriod: BlockNumber = 28 * DAYS;
	// Minimum time before a successful referendum can be enacted (4 days)
	pub const ReferendumMinEnactmentPeriod: BlockNumber = 4 * DAYS;
	// Maximum number of active referenda
	pub const ReferendumMaxProposals: u32 = 100;
	// Submission deposit for referenda
	pub const ReferendumSubmissionDeposit: Balance = 100 * UNIT;
	// Undeciding timeout (90 days)
	pub const UndecidingTimeout: BlockNumber = 45 * DAYS;
	pub const AlarmInterval: BlockNumber = 1;
}

impl pallet_referenda::Config for Runtime {
	/// Provides weights for the pallet operations to properly charge transaction fees.
	type WeightInfo = pallet_referenda::weights::SubstrateWeight<Runtime>;
	type RuntimeEvent = RuntimeEvent;
	/// The type of call dispatched by referenda upon approval and execution.
	type RuntimeCall = RuntimeCall;
	/// The scheduler pallet used to delay execution of successful referenda.
	type Scheduler = Scheduler;
	/// The currency mechanism used for handling deposits and voting.
	type Currency = Balances;
	/// The origin allowed to submit referenda - in this case any signed account.
	type SubmitOrigin = frame_system::EnsureSigned<AccountId>;
	/// The privileged origin allowed to cancel an ongoing referendum - only root can do this.
	type CancelOrigin = EnsureRoot<AccountId>;
	/// The privileged origin allowed to kill a referendum that's not passing - only root can do
	/// this.
	type KillOrigin = EnsureRoot<AccountId>;
	/// Destination for slashed deposits when a referendum is cancelled or killed.
	/// Leaving () here, will burn all slashed deposits. It's possible to use here the same idea
	/// as we have for TransactionFees (OnUnbalanced) - with this it should be possible to
	/// do something more sophisticated with this.
	type Slash = (); // Will discard any slashed deposits
	/// The voting mechanism used to collect votes and determine how they're counted.
	/// Connected to the conviction voting pallet to allow conviction-weighted votes.
	type Votes = pallet_conviction_voting::VotesOf<Runtime>;
	/// The method to tally votes and determine referendum outcome.
	/// Uses conviction voting's tally system with a maximum turnout threshold.
	type Tally = pallet_conviction_voting::Tally<Balance, DynamicMaxTurnout>;
	/// The deposit required to submit a referendum proposal.
	type SubmissionDeposit = ReferendumSubmissionDeposit;
	/// Maximum number of referenda that can be in the deciding phase simultaneously.
	type MaxQueued = ReferendumMaxProposals;
	/// Time period after which an undecided referendum will be automatically rejected.
	type UndecidingTimeout = UndecidingTimeout;
	/// The frequency at which the pallet checks for expired or ready-to-timeout referenda.
	type AlarmInterval = AlarmInterval;
	/// Defines the different referendum tracks (categories with distinct parameters).
	type Tracks = CommunityTracksInfo;
	/// The pallet used to store preimages (detailed proposal content) for referenda.
	type Preimages = Preimage;
	/// Blocknumber provider
	type BlockNumberProvider = System;
}

parameter_types! {
	pub const MinRankOfClassDelta: u16 = 0;
	pub const MaxMemberCount: u32 = 13;
}
impl pallet_ranked_collective::Config for Runtime {
	type WeightInfo = pallet_ranked_collective::weights::SubstrateWeight<Runtime>;
	type RuntimeEvent = RuntimeEvent;
	type AddOrigin = RootOrMemberForCollectiveOrigin;
	type RemoveOrigin = RootOrMemberForCollectiveOrigin;
	type PromoteOrigin = NeverEnsureOrigin<u16>;
	type DemoteOrigin = NeverEnsureOrigin<u16>;
	type ExchangeOrigin = NeverEnsureOrigin<u16>;
	type Polls = pallet_referenda::Pallet<Runtime, TechReferendaInstance>;
	type MinRankOfClass = MinRankOfClassConverter<MinRankOfClassDelta>;
	type MemberSwappedHandler = ();
	type VoteWeight = Linear;
	type MaxMemberCount = GlobalMaxMembers<MaxMemberCount>;

	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkSetup = ();
}

pub type TechReferendaInstance = pallet_referenda::Instance1;

impl pallet_referenda::Config<TechReferendaInstance> for Runtime {
	/// The type of call dispatched by referenda upon approval and execution.
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	/// Provides weights for the pallet operations to properly charge transaction fees.
	type WeightInfo = pallet_referenda::weights::SubstrateWeight<Runtime>;
	/// The scheduler pallet used to delay execution of successful referenda.
	type Scheduler = Scheduler;
	/// The currency mechanism used for handling deposits and voting.
	type Currency = Balances;
	/// The origin allowed to submit referenda - in this case any signed account.
	type SubmitOrigin = RootOrMemberForTechReferendaOrigin;
	/// The privileged origin allowed to cancel an ongoing referendum - only root can do this.
	type CancelOrigin = EnsureRoot<AccountId>;
	/// The privileged origin allowed to kill a referendum that's not passing - only root can do
	/// this.
	type KillOrigin = EnsureRoot<AccountId>;
	/// Destination for slashed deposits when a referendum is cancelled or killed.
	/// Leaving () here, will burn all slashed deposits. It's possible to use here the same idea
	/// as we have for TransactionFees (OnUnbalanced) - with this it should be possible to
	/// do something more sophisticated with this.
	type Slash = (); // Will discard any slashed deposits
	/// The voting mechanism used to collect votes and determine how they're counted.
	/// Connected to the conviction voting pallet to allow conviction-weighted votes.
	type Votes = pallet_ranked_collective::Votes;
	/// The method to tally votes and determine referendum outcome.
	/// Uses conviction voting's tally system with a maximum turnout threshold.
	type Tally = pallet_ranked_collective::TallyOf<Runtime>;
	/// The deposit required to submit a referendum proposal.
	type SubmissionDeposit = ReferendumSubmissionDeposit;
	/// Maximum number of referenda that can be in the deciding phase simultaneously.
	type MaxQueued = ReferendumMaxProposals;
	/// Time period after which an undecided referendum will be automatically rejected.
	type UndecidingTimeout = UndecidingTimeout;
	/// The frequency at which the pallet checks for expired or ready-to-timeout referenda.
	type AlarmInterval = AlarmInterval;
	/// Defines the different referendum tracks (categories with distinct parameters).
	type Tracks = TechCollectiveTracksInfo;
	/// The pallet used to store preimages (detailed proposal content) for referenda.
	type Preimages = Preimage;
	/// Blocknumber provider
	type BlockNumberProvider = System;
}

parameter_types! {
	// Maximum weight for scheduled calls (80% of the block's maximum weight)
	pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) * RuntimeBlockWeights::get().max_block;
	// Maximum number of scheduled calls per block
	pub const MaxScheduledPerBlock: u32 = 50;
}

impl pallet_scheduler::Config for Runtime {
	type RuntimeOrigin = RuntimeOrigin;
	type PalletsOrigin = OriginCaller;
	type RuntimeCall = RuntimeCall;
	type MaximumWeight = MaximumSchedulerWeight;
	type ScheduleOrigin = EnsureRoot<AccountId>;
	type MaxScheduledPerBlock = MaxScheduledPerBlock;
	type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Runtime>;
	type OriginPrivilegeCmp = frame_support::traits::EqualPrivilegeOnly;
	type Preimages = Preimage;
	type TimeProvider = Timestamp;
	type Moment = u64;
	type TimestampBucketSize = TimestampBucketSize;
}

// ============================================================================
// Transaction Fee Structure
// ============================================================================
//
// This is a solo Proof of Work chain (not a parachain), so Proof of Validity (PoV)
// size limits do not apply - we don't submit proofs to any relay chain.
//
// Fee Structure:
// - **Compute (ref_time):** 1 balance unit per unit of ref_time
//   - 1 second of compute ≈ 1 UNIT (since WEIGHT_REF_TIME_PER_SECOND = 10^12)
//   - Uses `IdentityFee<Balance>` for direct 1:1 mapping
//
// - **Extrinsic Length:** 1 UNIT per megabyte (LENGTH_FEE_MULTIPLIER = 10^6)
//   - This brings storage/bandwidth costs in line with compute costs
//   - A 5 MB block (max size) costs ~5 UNIT in length fees
//   - A typical 500-byte transfer costs ~0.0005 UNIT in length fees
//   - Uses `LengthToFeeMultiplier` with 10^6 coefficient
//
// - **Proof Size:** Not enforced or charged
//   - Block weight limit uses u64::MAX for proof_size component
//   - WeightToFee only considers ref_time, not proof_size
//
// Fee Destination:
// - 100% of transaction fees go to the block miner
// - Block rewards are split: 70% miner, 30% treasury
//
// Spam Prevention:
// - Existential deposit: 0.001 UNIT
// - Various pallet-specific deposits (multisig, governance, recovery, etc.)
// - Miners can reject transactions below their minimum fee threshold
//
// ============================================================================
// Proof Size Design Rationale
// ============================================================================
//
// **Why proof_size is set to u64::MAX and not priced:**
//
// In Substrate's two-dimensional weight system, `proof_size` represents the amount of
// state witness data required for stateless validation. This is critical for parachains
// where validators must re-execute blocks using only the PoV (Proof of Validity) blob,
// which has strict size limits imposed by the relay chain.
//
// For this solo PoW chain, proof_size constraints are intentionally disabled because:
//
// 1. **No relay chain constraints:** Unlike parachains, solo chains have no external
//    entity imposing PoV size limits. Validators have full state access.
//
// 2. **Full nodes validate blocks:** All validators maintain complete state, so they
//    don't need witnesses to re-execute transactions.
//
// 3. **ref_time provides sufficient protection:** Compute-bound benchmarking (ref_time)
//    naturally correlates with state access patterns. Heavy state reads/writes increase
//    ref_time, providing indirect protection against state-heavy transactions.
//
// 4. **Block length limits storage abuse:** The 5 MB block size limit caps the amount
//    of data that can be included per block, preventing bandwidth-based attacks.
//
// **When to revisit this decision:**
//
// This design should be reconsidered if the chain adopts features where proof/witness
// size becomes a meaningful resource constraint:
//
// - **Light client support:** Light clients verify blocks using state proofs. Large
//   witnesses increase sync times and bandwidth requirements for light clients.
//
// - **Cross-chain bridges:** Bridge protocols often require merkle proofs of state.
//   Unbounded proof sizes could make bridge operations expensive or impractical.
//
// - **Stateless validation:** If the chain moves toward stateless block validation
//   (validators don't keep full state), witness size becomes a critical resource.
//
// - **ZK proof generation:** If state proofs are used as inputs to ZK circuits,
//   proof size directly impacts prover time and memory requirements.
//
// To enable proof_size enforcement in the future:
// 1. Set a concrete proof_size limit in RuntimeBlockWeights (instead of u64::MAX)
// 2. Update WeightToFee to price both ref_time and proof_size dimensions
// 3. Ensure all pallet benchmarks accurately measure proof_size

/// Multiplier for converting extrinsic length (bytes) to fee.
/// At 10^6, this means 1 MB of data costs approximately 1 UNIT in fees,
/// bringing storage costs roughly in line with compute costs.
pub const LENGTH_FEE_MULTIPLIER: Balance = 1_000_000;

/// Converts extrinsic length to fee with a multiplier.
///
/// This implementation applies [`LENGTH_FEE_MULTIPLIER`] to the extrinsic length,
/// making 1 MB of extrinsic data cost approximately 1 UNIT in fees.
///
/// Fee comparison at different transaction sizes:
/// - 500 bytes (simple transfer): ~0.0005 UNIT
/// - 10 KB (complex call): ~0.01 UNIT
/// - 100 KB (batch operation): ~0.1 UNIT
/// - 1 MB (large payload): ~1 UNIT
/// - 5 MB (full block): ~5 UNIT
pub struct LengthToFeeMultiplier;

impl WeightToFeePolynomial for LengthToFeeMultiplier {
	type Balance = Balance;

	fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
		smallvec![WeightToFeeCoefficient {
			degree: 1,
			negative: false,
			coeff_frac: Perbill::zero(),
			coeff_integer: LENGTH_FEE_MULTIPLIER,
		}]
	}
}

parameter_types! {
	pub FeeMultiplier: Multiplier = Multiplier::one();
}

impl pallet_transaction_payment::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type OnChargeTransaction =
		FungibleAdapter<Balances, pallet_mining_rewards::TransactionFeesCollector<Runtime>>;
	/// Converts compute weight (ref_time) to fee. Uses identity (1:1) mapping,
	/// so 1 second of compute costs approximately 1 UNIT.
	type WeightToFee = IdentityFee<Balance>;
	/// Converts extrinsic length to fee. Uses 10^6 multiplier so 1 MB costs ~1 UNIT,
	/// bringing storage/bandwidth costs in line with compute costs.
	type LengthToFee = LengthToFeeMultiplier;
	type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightInfo = pallet_transaction_payment::weights::SubstrateWeight<Runtime>;
}

impl pallet_utility::Config for Runtime {
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type PalletsOrigin = OriginCaller;
	type WeightInfo = pallet_utility::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	/// Base deposit for creating a recovery configuration
	pub const ConfigDepositBase: Balance = 10 * UNIT;
	/// Deposit required per friend
	pub const FriendDepositFactor: Balance = UNIT;
	/// Maximum number of friends allowed in a recovery configuration
	pub const MaxFriends: u32 = 9;
	/// Deposit required to initiate a recovery
	pub const RecoveryDeposit: Balance = 10 * UNIT;
}

impl pallet_recovery::Config for Runtime {
	type WeightInfo = pallet_recovery::weights::SubstrateWeight<Runtime>;
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type ConfigDepositBase = ConfigDepositBase;
	type FriendDepositFactor = FriendDepositFactor;
	type MaxFriends = MaxFriends;
	type RecoveryDeposit = RecoveryDeposit;
	type BlockNumberProvider = System;
}

parameter_types! {
	pub const ReversibleTransfersPalletIdValue: PalletId = PalletId(*b"rtpallet");
	pub const DefaultDelay: BlockNumberOrTimestamp<BlockNumber, Moment> = BlockNumberOrTimestamp::BlockNumber(DAYS);
	pub const MinDelayPeriodBlocks: BlockNumber = 2;
	pub const MaxInterceptorAccounts: u32 = 32;
	pub const MaxPendingPerAccount: u32 = 16;
	/// Volume fee for reversed transactions from high-security accounts only (1% fee is burned)
	pub const HighSecurityVolumeFee: Permill = Permill::from_percent(1);
}

impl pallet_reversible_transfers::Config for Runtime {
	type SchedulerOrigin = OriginCaller;
	type Scheduler = Scheduler;
	type BlockNumberProvider = System;
	type DefaultDelay = DefaultDelay;
	type MinDelayPeriodBlocks = MinDelayPeriodBlocks;
	type MinDelayPeriodMoment = TargetBlockTime;
	type PalletId = ReversibleTransfersPalletIdValue;
	type Preimages = Preimage;
	type WeightInfo = pallet_reversible_transfers::weights::SubstrateWeight<Runtime>;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Moment = Moment;
	type TimeProvider = Timestamp;
	type MaxInterceptorAccounts = MaxInterceptorAccounts;
	type MaxPendingPerAccount = MaxPendingPerAccount;
	type VolumeFee = HighSecurityVolumeFee;
	type ProofRecorder = Wormhole;
}

parameter_types! {
	pub const TreasuryPalletId: PalletId = PalletId(*b"py/trsry");
}

impl pallet_treasury::Config for Runtime {
	type WeightInfo = pallet_treasury::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	pub const AssetDeposit: Balance = MILLI_UNIT;
	pub const AssetAccountDeposit: Balance = MILLI_UNIT;
	pub const AssetsStringLimit: u32 = 50;
	pub const MetadataDepositBase: Balance = MILLI_UNIT;
	pub const MetadataDepositPerByte: Balance = MILLI_UNIT;
}

/// We allow root to execute privileged asset operations.
pub type AssetsForceOrigin = EnsureRoot<AccountId>;
type AssetId = u32;

impl pallet_assets::Config for Runtime {
	type Balance = Balance;
	type RuntimeEvent = RuntimeEvent;
	type AssetId = AssetId;
	type AssetIdParameter = codec::Compact<AssetId>;
	type Currency = Balances;
	type CreateOrigin = AsEnsureOriginWithArg<EnsureSigned<AccountId>>;
	type ForceOrigin = AssetsForceOrigin;
	type AssetDeposit = AssetDeposit;
	type MetadataDepositBase = MetadataDepositBase;
	type MetadataDepositPerByte = MetadataDepositPerByte;
	type ApprovalDeposit = ExistentialDeposit;
	type StringLimit = AssetsStringLimit;
	type Freezer = ();
	type Extra = ();
	type WeightInfo = pallet_assets::weights::SubstrateWeight<Runtime>;
	type CallbackHandle = pallet_assets::AutoIncAssetId<Runtime, ()>;
	type AssetAccountDeposit = AssetAccountDeposit;
	type RemoveItemsLimit = frame_support::traits::ConstU32<1000>;
	type Holder = pallet_assets_holder::Pallet<Runtime>;
	type ReserveData = ();
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ();
}

impl pallet_assets_holder::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
}

// Multisig configuration
parameter_types! {
	pub const MultisigPalletId: PalletId = PalletId(*b"py/mltsg");
	pub const MaxSigners: u32 = 100;
	pub const MaxTotalProposalsInStorage: u32 = 200; // Max total in storage (Active + Executed + Cancelled)
	pub const MaxCallSize: u32 = 10240; // 10KB
	pub const MultisigFee: Balance = 100 * MILLI_UNIT; // 0.1 UNIT (non-refundable)
	pub const MultisigDeposit: Balance = 500 * MILLI_UNIT; // 0.5 UNIT (storage rent, non-refundable)
	pub const ProposalDeposit: Balance = 1000 * MILLI_UNIT; // 1 UNIT (locked until cleanup)
	pub const ProposalFee: Balance = 1000 * MILLI_UNIT; // 1 UNIT (non-refundable)
	pub const SignerStepFactorParam: Permill = Permill::from_percent(1);
	pub const MaxExpiryDuration: BlockNumber = 100_800; // ~2 weeks at 12s blocks (14 days * 24h * 60m * 60s / 12s)
}

/// High-Security configuration wrapper for Runtime
///
/// This type alias delegates to `ReversibleTransfers` pallet for high-security checks
/// and adds RuntimeCall-specific whitelist validation.
///
/// Used by:
/// - Multisig pallet: validates calls in `propose()` extrinsic
/// - Transaction extensions: validates calls for high-security EOAs
///
/// Whitelist includes only delayed, reversible operations:
/// - `schedule_transfer`: Schedule delayed native token transfer
/// - `schedule_asset_transfer`: Schedule delayed asset transfer
/// - `cancel`: Cancel pending delayed transfer
pub struct HighSecurityConfig;

impl qp_high_security::HighSecurityInspector<AccountId, RuntimeCall> for HighSecurityConfig {
	fn is_high_security(who: &AccountId) -> bool {
		// Delegate to reversible-transfers pallet
		pallet_reversible_transfers::Pallet::<Runtime>::is_high_security_account(who)
	}

	fn is_whitelisted(call: &RuntimeCall) -> bool {
		matches!(
			call,
			RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer { .. }
			) | RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_asset_transfer { .. }
			) | RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel { .. }) |
				RuntimeCall::ReversibleTransfers(
					pallet_reversible_transfers::Call::recover_funds { .. }
				)
		)
	}

	fn guardian(who: &AccountId) -> Option<AccountId> {
		// Delegate to reversible-transfers pallet
		pallet_reversible_transfers::Pallet::<Runtime>::get_guardian(who)
	}
}

impl pallet_multisig::Config for Runtime {
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type MaxSigners = MaxSigners;
	type MaxTotalProposalsInStorage = MaxTotalProposalsInStorage;
	type MaxCallSize = MaxCallSize;
	type MultisigFee = MultisigFee;
	type MultisigDeposit = MultisigDeposit;
	type ProposalDeposit = ProposalDeposit;
	type ProposalFee = ProposalFee;
	type SignerStepFactor = SignerStepFactorParam;
	type MaxExpiryDuration = MaxExpiryDuration;
	type PalletId = MultisigPalletId;
	type WeightInfo = pallet_multisig::weights::SubstrateWeight<Runtime>;
	type HighSecurity = HighSecurityConfig;
}

impl TryFrom<RuntimeCall> for pallet_balances::Call<Runtime> {
	type Error = ();
	fn try_from(call: RuntimeCall) -> Result<Self, Self::Error> {
		match call {
			RuntimeCall::Balances(c) => Ok(c),
			_ => Err(()),
		}
	}
}

impl TryFrom<RuntimeCall> for pallet_assets::Call<Runtime> {
	type Error = ();
	fn try_from(call: RuntimeCall) -> Result<Self, Self::Error> {
		match call {
			RuntimeCall::Assets(c) => Ok(c),
			_ => Err(()),
		}
	}
}

parameter_types! {
	/// Minimum transfer amount for wormhole (10 QUAN = 10 * 10^12)
	pub const WormholeMinimumTransferAmount: Balance = UNIT / 10;
	/// Volume fee rate in basis points (10 bps = 0.1%)
	pub const VolumeFeeRateBps: u32 = 10;
	/// Proportion of volume fees to burn (50% burned, 50% to miner)
	pub const VolumeFeesBurnRate: Permill = Permill::from_percent(50);
}

impl pallet_wormhole::Config for Runtime {
	type NativeBalance = Balance;
	type Currency = Balances;
	type Assets = Assets;
	type AssetId = AssetId;
	type AssetBalance = Balance;
	type TransferCount = u64;
	/// Use the same MintingAccount as mining-rewards for consistency.
	/// Both pallets mint native tokens and should use the same sentinel "from" address.
	type MintingAccount = MintingAccount;
	type MinimumTransferAmount = WormholeMinimumTransferAmount;
	type VolumeFeeRateBps = VolumeFeeRateBps;
	type VolumeFeesBurnRate = VolumeFeesBurnRate;
	type WormholeAccountId = AccountId32;
	type WeightInfo = pallet_wormhole::weights::SubstrateWeight<Runtime>;
	type ZkTree = ZkTree;
}

impl pallet_zk_tree::Config for Runtime {
	type AssetId = AssetId;
	type Balance = Balance;
}
