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
	governance::{
		definitions::{
			CommunityTracksInfo, GlobalMaxMembers, MinRankOfClassConverter, PreimageDeposit,
			RootOrMemberForCollectiveOrigin, RootOrMemberForTechReferendaOrigin,
			RuntimeNativeBalanceConverter, RuntimeNativePaymaster, TechCollectiveTracksInfo,
		},
		pallet_custom_origins, Spender,
	},
	MILLI_UNIT,
};
use frame_support::{
	derive_impl, parameter_types,
	traits::{
		AsEnsureOriginWithArg, ConstU128, ConstU32, ConstU8, EitherOf, Get, NeverEnsureOrigin,
		VariantCountOf,
	},
	weights::{
		constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
		IdentityFee, Weight,
	},
	PalletId,
};
use frame_system::{
	limits::{BlockLength, BlockWeights},
	EnsureRoot, EnsureRootWithSuccess, EnsureSigned,
};
use pallet_ranked_collective::Linear;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use qp_poseidon::PoseidonHasher;
use qp_scheduler::BlockNumberOrTimestamp;
use sp_runtime::{traits::One, FixedU128, Perbill, Permill};
use sp_version::RuntimeVersion;

// Local module imports
use super::{
	AccountId, Balance, Balances, Block, BlockNumber, Hash, Nonce, OriginCaller, PalletInfo,
	Preimage, Referenda, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason,
	RuntimeHoldReason, RuntimeOrigin, RuntimeTask, Scheduler, System, Timestamp, DAYS,
	EXISTENTIAL_DEPOSIT, MICRO_UNIT, TARGET_BLOCK_TIME_MS, UNIT, VERSION,
};
use sp_core::U512;

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
	pub const BlockHashCount: BlockNumber = 4096;
	pub const Version: RuntimeVersion = VERSION;

	/// We allow for 6 seconds of compute with a 12 second average block time.
	pub RuntimeBlockWeights: BlockWeights = BlockWeights::with_sensible_defaults(
		Weight::from_parts(6u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
		NORMAL_DISPATCH_RATIO,
	);
	// We estimate to download 5MB blocks it takes a 100Mbs link 600ms and 200ms for 1Gbs link
	// To upload, 10Mbs link takes 4.1s and 100Mbs takes 500ms
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
	/// The type for hash function that computes extrinsic root
	type Hashing = PoseidonHasher;
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
	pub const MaxTokenAmount: Balance = 1000 * UNIT;
	pub const DefaultMintAmount: Balance = 10 * UNIT;
}

impl pallet_mining_rewards::Config for Runtime {
	type Currency = Balances;
	type WeightInfo = pallet_mining_rewards::weights::SubstrateWeight<Runtime>;
	type MinerBlockReward = ConstU128<{ 10 * UNIT }>; // 10 tokens
	type TreasuryBlockReward = ConstU128<0>; // 0 tokens
	type TreasuryPalletId = TreasuryPalletId;
	type MintingAccount = MintingAccount;
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
	type FixedU128Scale = ConstU128<1_000_000_000_000_000_000>;
	type WeightInfo = ();
	type EmaAlpha = ConstU32<100>; // out of 1000, last_block_time * alpha + (previous_ema * (1 - alpha)) on moving average
}

parameter_types! {
	 pub const MintingAccount: AccountId = AccountId::new([1u8; 32]);
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
	pub const MinimumDeposit: Balance = UNIT;
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

parameter_types! {
	// Default voting period (28 days)
	pub const TechReferendumDefaultVotingPeriod: BlockNumber = 28 * DAYS;
	// Minimum time before a successful referendum can be enacted (4 days)
	pub const TechReferendumMinEnactmentPeriod: BlockNumber = 4 * DAYS;
	// Maximum number of active referenda
	pub const TechReferendumMaxProposals: u32 = 100;
	// Submission deposit for referenda
	pub const TechReferendumSubmissionDeposit: Balance = 100 * UNIT;
	// Undeciding timeout (90 days)
	pub const TechUndecidingTimeout: BlockNumber = 45 * DAYS;
	pub const TechAlarmInterval: BlockNumber = 1;
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
	// Optional postponement for calls without preimage
	pub const NoPreimagePostponement: Option<u32> = Some(10);
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

parameter_types! {
	pub FeeMultiplier: Multiplier = Multiplier::one();
}

impl pallet_transaction_payment::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type OnChargeTransaction =
		FungibleAdapter<Balances, pallet_mining_rewards::TransactionFeesCollector<Runtime>>;
	type WeightToFee = IdentityFee<Balance>;
	type LengthToFee = IdentityFee<Balance>;
	type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightInfo = pallet_transaction_payment::weights::SubstrateWeight<Runtime>;
}

impl pallet_sudo::Config for Runtime {
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = pallet_sudo::weights::SubstrateWeight<Runtime>;
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
	pub const MaxReversibleTransfers: u32 = 10;
	pub const MaxInterceptorAccounts: u32 = 32;
	/// Volume fee for reversed transactions from high-security accounts only (1% fee is burned)
	pub const HighSecurityVolumeFee: Permill = Permill::from_percent(1);
}

impl pallet_reversible_transfers::Config for Runtime {
	type SchedulerOrigin = OriginCaller;
	type Scheduler = Scheduler;
	type BlockNumberProvider = System;
	type MaxPendingPerAccount = MaxReversibleTransfers;
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
	type VolumeFee = HighSecurityVolumeFee;
}

parameter_types! {
	pub const TreasuryPalletId: PalletId = PalletId(*b"py/trsry");
	pub const ProposalBond: Permill = Permill::from_percent(5);
	pub const ProposalBondMinimum: Balance = UNIT;
	pub const ProposalBondMaximum: Option<Balance> = None;
	pub const SpendPeriod: BlockNumber = 2 * DAYS;
	pub const Burn: Permill = Permill::from_percent(0);
	pub const MaxApprovals: u32 = 100;
	pub const TreasuryPayoutPeriod: BlockNumber = 14 * DAYS; // Added for PayoutPeriod
}

impl pallet_treasury::Config for Runtime {
	type PalletId = TreasuryPalletId;
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type RejectOrigin = EnsureRoot<AccountId>;
	type SpendPeriod = SpendPeriod;
	type Burn = Burn;
	type BurnDestination = (); // Treasury funds will be burnt without a specific destination
	type SpendFunds = (); // No external pallets spending treasury funds directly through this hook
	type MaxApprovals = MaxApprovals; // For deprecated spend_local flow
	type WeightInfo = pallet_treasury::weights::SubstrateWeight<Runtime>;
	type SpendOrigin = TreasurySpender; // Changed to use the custom EnsureOrigin
	type AssetKind = (); // Using () to represent native currency for simplicity
	type Beneficiary = AccountId; // Spends are paid to AccountId
	type BeneficiaryLookup = sp_runtime::traits::AccountIdLookup<AccountId, ()>; // Standard lookup for AccountId
	type Paymaster = RuntimeNativePaymaster; // Custom paymaster for native currency
	type BalanceConverter = RuntimeNativeBalanceConverter; // Custom converter for native currency
	type PayoutPeriod = TreasuryPayoutPeriod; // How long a spend is valid for claiming
	type BlockNumberProvider = System;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = (); // System pallet provides block number
}

parameter_types! {
	pub const MaxBalance: Balance = Balance::MAX;
}

pub type TreasurySpender = EitherOf<EnsureRootWithSuccess<AccountId, MaxBalance>, Spender>;

impl pallet_custom_origins::Config for Runtime {}

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
	pub const MaxActiveProposals: u32 = 100; // Max active proposals per multisig
	pub const MaxTotalProposalsInStorage: u32 = 200; // Max total in storage (Active + Executed + Cancelled)
	pub const MaxCallSize: u32 = 10240; // 10KB
	pub const MultisigFee: Balance = 100 * MILLI_UNIT; // 0.1 UNIT (non-refundable)
	pub const MultisigDeposit: Balance = 500 * MILLI_UNIT; // 0.5 UNIT (refundable)
	pub const ProposalDeposit: Balance = 1000 * MILLI_UNIT; // 1 UNIT (locked until cleanup)
	pub const ProposalFee: Balance = 1000 * MILLI_UNIT; // 1 UNIT (non-refundable)
	pub const SignerStepFactorParam: Permill = Permill::from_percent(1);
	pub const MaxExpiryDuration: BlockNumber = 100_800; // ~2 weeks at 12s blocks (14 days * 24h * 60m * 60s / 12s)
}

/// Whitelist for calls that can be proposed in multisigs
impl pallet_multisig::Config for Runtime {
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type MaxSigners = MaxSigners;
	type MaxActiveProposals = MaxActiveProposals;
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
