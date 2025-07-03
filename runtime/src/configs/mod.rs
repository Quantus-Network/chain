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
use crate::governance::definitions::{
    CommunityTracksInfo, GlobalMaxMembers, MinRankOfClassConverter, PreimageDeposit,
    RootOrMemberForCollectiveOrigin, RootOrMemberForTechReferendaOrigin,
    RuntimeNativeBalanceConverter, RuntimeNativePaymaster, TechCollectiveTracksInfo,
};
use crate::governance::{pallet_custom_origins, Spender};
use crate::MILLI_UNIT;
use frame_support::traits::{
    AsEnsureOriginWithArg, ConstU64, EitherOf, NeverEnsureOrigin, WithdrawReasons,
};
use frame_support::PalletId;
use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstU128, ConstU32, ConstU8, VariantCountOf},
    weights::{
        constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
        IdentityFee, Weight,
    },
};
use frame_system::limits::{BlockLength, BlockWeights};
use frame_system::{EnsureRoot, EnsureRootWithSuccess, EnsureSigned};
use pallet_ranked_collective::Linear;
use pallet_referenda::impl_tracksinfo_get;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use poseidon_resonance::PoseidonHasher;
use qp_scheduler::BlockNumberOrTimestamp;
use sp_runtime::traits::ConvertInto;
use sp_runtime::{traits::One, Perbill, Permill};
use sp_version::RuntimeVersion;

// Local module imports
use super::{
    AccountId, AssetId, Balance, Balances, Block, BlockNumber, Hash, Nonce, OriginCaller,
    PalletInfo, Preimage, Referenda, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason,
    RuntimeHoldReason, RuntimeOrigin, RuntimeTask, Scheduler, System, Timestamp, Vesting, DAYS,
    EXISTENTIAL_DEPOSIT, MICRO_UNIT, UNIT, VERSION,
};

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
    pub const BlockHashCount: BlockNumber = 2400;
    pub const Version: RuntimeVersion = VERSION;

    /// We allow for 2 seconds of compute with a 6 second average block time.
    pub RuntimeBlockWeights: BlockWeights = BlockWeights::with_sensible_defaults(
        Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
        NORMAL_DISPATCH_RATIO,
    );
    pub RuntimeBlockLength: BlockLength = BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
    pub const SS58Prefix: u8 = 189;
    pub const MerkleAirdropPalletId: PalletId = PalletId(*b"airdrop!");
    pub const MaxProofs: u32 = 100;
    pub const UnsignedClaimPriority: u32 = 100;
    pub MiningRewardsFeesToTreasury: Permill = Permill::from_percent(10);
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

impl pallet_faucet::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type MaxTokenAmount = MaxTokenAmount;
    type DefaultMintAmount = DefaultMintAmount;
}

impl pallet_mining_rewards::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_mining_rewards::weights::SubstrateWeight<Runtime>;
    type Currency = Balances;
    type BlockReward = ConstU128<1_000_000_000_000>; // 1 token
    type TreasuryPalletId = TreasuryPalletId;
    type FeesToTreasuryPermill = MiningRewardsFeesToTreasury;
}

parameter_types! {
    /// Target block time
    pub const TargetBlockTime: u64 = 10000;
}

impl pallet_qpow::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    // NOTE: InitialDistance will be shifted left by this amount: higher is easier
    type InitialDistanceThresholdExponent = ConstU32<502>;
    type DifficultyAdjustPercentClamp = ConstU8<10>;
    type TargetBlockTime = TargetBlockTime;
    type AdjustmentPeriod = ConstU32<1>;
    type BlockTimeHistorySize = ConstU32<10>;
    type MaxReorgDepth = ConstU32<10>;
    type FixedU128Scale = ConstU128<1_000_000_000_000_000_000>;
    type MaxDistanceMultiplier = ConstU32<2>;
}

parameter_types! {
     pub const WormholeMintingAccount: AccountId = AccountId::new([1u8; 32]);
}

impl pallet_wormhole::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    type MintingAccount = WormholeMintingAccount;
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
    /// The ubiquitous event type.
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
    pub const MaxVotes: u32 = 512;
    pub const MaxTurnout: Balance = 60 * UNIT;
    pub const MinimumDeposit: Balance = UNIT;
}

impl pallet_conviction_voting::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_conviction_voting::weights::SubstrateWeight<Runtime>;
    type Currency = Balances;
    type VoteLockingPeriod = VoteLockingPeriod;
    type MaxVotes = MaxVotes;
    type MaxTurnout = MaxTurnout;
    type Polls = Referenda;
}

parameter_types! {
    pub const PreimageBaseDeposit: Balance = UNIT;
    pub const PreimageByteDeposit: Balance = MICRO_UNIT;
}

impl pallet_preimage::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
    type Currency = Balances;
    type ManagerOrigin = frame_system::EnsureRoot<AccountId>;
    type Consideration = PreimageDeposit;
}

impl_tracksinfo_get!(CommunityTracksInfo, Balance, BlockNumber);

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
    /// The overarching event type for the runtime.
    type RuntimeEvent = RuntimeEvent;
    /// Provides weights for the pallet operations to properly charge transaction fees.
    type WeightInfo = pallet_referenda::weights::SubstrateWeight<Runtime>;
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
    /// The privileged origin allowed to kill a referendum that's not passing - only root can do this.
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
    type Tally = pallet_conviction_voting::Tally<Balance, MaxTurnout>;
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

impl_tracksinfo_get!(TechCollectiveTracksInfo, Balance, BlockNumber);

impl pallet_referenda::Config<TechReferendaInstance> for Runtime {
    /// The type of call dispatched by referenda upon approval and execution.
    type RuntimeCall = RuntimeCall;
    /// The overarching event type for the runtime.
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
    /// The privileged origin allowed to kill a referendum that's not passing - only root can do this.
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
    type RuntimeEvent = RuntimeEvent;
    type RuntimeOrigin = RuntimeOrigin;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = frame_system::EnsureRoot<AccountId>;
    type MaxScheduledPerBlock = MaxScheduledPerBlock;
    type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Runtime>;
    type OriginPrivilegeCmp = frame_support::traits::EqualPrivilegeOnly;
    type Preimages = Preimage;
    type TimeProvider = Timestamp;
    type Moment = u64;
    type TimestampBucketSize = ConstU64<2000>; // 2 second
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
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_sudo::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    pub const MinVestedTransfer: Balance = UNIT;
    /// Unvested funds can be transferred and reserved for any other means (reserves overlap)
    pub UnvestedFundsAllowedWithdrawReasons: WithdrawReasons =
    WithdrawReasons::except(WithdrawReasons::TRANSFER | WithdrawReasons::RESERVE);
}

impl pallet_vesting::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type WeightInfo = pallet_vesting::weights::SubstrateWeight<Runtime>;
    type MinVestedTransfer = MinVestedTransfer;
    type BlockNumberToBalance = ConvertInto;
    type UnvestedFundsAllowedWithdrawReasons = UnvestedFundsAllowedWithdrawReasons;
    type BlockNumberProvider = System;

    const MAX_VESTING_SCHEDULES: u32 = 28;
}

impl pallet_utility::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = pallet_utility::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    /// Base deposit for creating a recovery configuration
    pub const ConfigDepositBase: Balance = 10 * UNIT;
    /// Deposit required per friend
    pub const FriendDepositFactor: Balance = 1 * UNIT;
    /// Maximum number of friends allowed in a recovery configuration
    pub const MaxFriends: u32 = 9;
    /// Deposit required to initiate a recovery
    pub const RecoveryDeposit: Balance = 10 * UNIT;
}

impl pallet_recovery::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_recovery::weights::SubstrateWeight<Runtime>;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ConfigDepositBase = ConfigDepositBase;
    type FriendDepositFactor = FriendDepositFactor;
    type MaxFriends = MaxFriends;
    type RecoveryDeposit = RecoveryDeposit;
}

parameter_types! {
    pub const ReversibleTransfersPalletIdValue: PalletId = PalletId(*b"rtpallet");
    pub const DefaultDelay: BlockNumberOrTimestamp<BlockNumber, Moment> = BlockNumberOrTimestamp::BlockNumber(DAYS);
    pub const MinDelayPeriodBlocks: BlockNumber = 2;
    pub const MaxReversibleTransfers: u32 = 10;
    pub const MaxInterceptorAccounts: u32 = 32;
}

impl pallet_reversible_transfers::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
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
}

impl pallet_merkle_airdrop::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Vesting = Vesting;
    type MaxProofs = MaxProofs;
    type PalletId = MerkleAirdropPalletId;
    type WeightInfo = pallet_merkle_airdrop::weights::SubstrateWeight<Runtime>;
    type UnsignedClaimPriority = UnsignedClaimPriority;
    type BlockNumberProvider = System;
    type BlockNumberToBalance = ConvertInto;
}

parameter_types! {
    pub const TreasuryPalletId: PalletId = PalletId(*b"py/trsry");
    pub const ProposalBond: Permill = Permill::from_percent(5);
    pub const ProposalBondMinimum: Balance = 1 * UNIT;
    pub const ProposalBondMaximum: Option<Balance> = None;
    pub const SpendPeriod: BlockNumber = 2 * DAYS;
    pub const Burn: Permill = Permill::from_percent(0);
    pub const MaxApprovals: u32 = 100;
    pub const TreasuryPayoutPeriod: BlockNumber = 14 * DAYS; // Added for PayoutPeriod
}

impl pallet_treasury::Config for Runtime {
    type PalletId = TreasuryPalletId;
    type Currency = Balances;
    type RejectOrigin = EnsureRoot<AccountId>;
    type RuntimeEvent = RuntimeEvent;
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
    pub const MaxBalance: Balance = Balance::max_value();
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

impl pallet_assets::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Balance = Balance;
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
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = ();
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
