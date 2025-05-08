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
use crate::governance::{MinRankOfClassConverter, PreimageDeposit, TracksInfo};
use frame_support::traits::ConstU64;
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
use frame_system::{EnsureRoot, EnsureRootWithSuccess};
use pallet_referenda::impl_tracksinfo_get;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use pallet_ranked_collective::{self, Geometric};
use pallet_vesting::VestingPalletId;
use poseidon_resonance::PoseidonHasher;
use sp_runtime::{traits::One, Perbill};
use sp_runtime::traits::ConstU16;
use sp_version::RuntimeVersion;
use crate::fellowship::{ApproveOrigin, FastPromoteOrigin, FellowshipEvidenceSize, MaxFellowshipRank, PromoteOrigin};
// Local module imports
use super::{AccountId, Balance, Balances, Block, BlockNumber, Hash, Nonce, OriginCaller, PalletInfo, Preimage, Referenda, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason, RuntimeOrigin, RuntimeTask, Scheduler, System, TechFellowship, DAYS, EXISTENTIAL_DEPOSIT, MICRO_UNIT, UNIT, VERSION};

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
    pub const SS58Prefix: u8 = 42;
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
    type MaxConsumers = frame_support::traits::ConstU32<16>;
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
}

impl pallet_qpow::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_qpow::DefaultWeightInfo;
    type InitialDifficulty = ConstU64<5500000000>; //Min + 5
    type MinDifficulty = ConstU64<50000000000>;
    type TargetBlockTime = ConstU64<1000>;
    type AdjustmentPeriod = ConstU32<10>;
    type DampeningFactor = ConstU64<8>;
    type BlockTimeHistorySize = ConstU32<60>;
    type MaxReorgDepth = ConstU32<10>;
}

impl pallet_wormhole::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_wormhole::DefaultWeightInfo;
}

parameter_types! {
    pub const MinimumPeriod: u64 = 100;
}
impl pallet_timestamp::Config for Runtime {
    /// A timestamp: milliseconds since the unix epoch.
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = MinimumPeriod;
    type WeightInfo = ();
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
    type ExistentialDeposit = ConstU128<EXISTENTIAL_DEPOSIT>;
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
    pub const MinimumDeposit: Balance = 1 * UNIT;
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

impl pallet_core_fellowship::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_core_fellowship::weights::SubstrateWeight<Runtime>;
    type MaxRank = MaxFellowshipRank;
    type EvidenceSize = FellowshipEvidenceSize;
    type ParamsOrigin = EnsureRoot<AccountId>;
    type InductOrigin = EnsureRoot<AccountId>;
    type ApproveOrigin = ApproveOrigin;
    type PromoteOrigin = PromoteOrigin;
    type FastPromoteOrigin = FastPromoteOrigin;
    type Members = TechFellowship;
    type Balance = Balance;
}

parameter_types! {
    pub const MinRankOfClassDelta: u16 = 0;
}
impl pallet_ranked_collective::Config for Runtime {
    type WeightInfo = pallet_ranked_collective::weights::SubstrateWeight<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type AddOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>;
    type RemoveOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>;
    type PromoteOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>;
    type DemoteOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>;
    type ExchangeOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>;
    type Polls = pallet_referenda::Pallet<Runtime, TechReferendaInstance>;
    type MinRankOfClass = MinRankOfClassConverter<MinRankOfClassDelta>;
    type MemberSwappedHandler = pallet_core_fellowship::Pallet<Runtime>;
    type VoteWeight = Geometric;
    type MaxMemberCount = ();

    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkSetup = ();
}

/*impl pallet_ranked_collective::Config for Runtime {
    type WeightInfo = pallet_ranked_collective::weights::SubstrateWeight<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    // Use our custom origins
    type AddOrigin = FellowshipAdminOrigin;
    type RemoveOrigin = FellowshipAdminOrigin;
    type PromoteOrigin = PromotionOrigin;
    type DemoteOrigin = PromotionOrigin;
    type ExchangeOrigin = FellowshipAdminOrigin;
    type Polls = TechReferenda;
    type MinRankOfClass = MinRankOfClassConverter<MinRankOfClassDelta>;
    type MemberSwappedHandler = pallet_core_fellowship::Pallet<Runtime>;
    // For the vote weight, use Geometric
    type VoteWeight = Geometric;
    type MaxMemberCount = ConstU32<1000>; // Maximum number of members

    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkSetup = pallet_core_fellowship::Pallet<Runtime>;
}*/

parameter_types! {
    pub const PreimageBaseDeposit: Balance = 1 * UNIT;
    pub const PreimageByteDeposit: Balance = 1 * MICRO_UNIT;
}

impl pallet_preimage::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
    type Currency = Balances;
    type ManagerOrigin = frame_system::EnsureRoot<AccountId>;
    type Consideration = PreimageDeposit;
}

impl_tracksinfo_get!(TracksInfo, Balance, BlockNumber);

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
    type Tracks = TracksInfo;
    /// The pallet used to store preimages (detailed proposal content) for referenda.
    type Preimages = Preimage;
}

//impl_tracksinfo_get!(TracksInfo, Balance, BlockNumber);

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

// pub struct TechReferendaInstance;
// impl frame_support::traits::Instance for TechReferendaInstance {
//     const PREFIX: &'static str = "TechReferenda";
//     const INDEX: u8 = 0;
// }

pub type TechReferendaInstance = pallet_referenda::Instance1;

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
    type Tracks = TracksInfo;
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

impl pallet_vesting::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type PalletId = VestingPalletId;
    type WeightInfo = pallet_vesting::weights::SubstrateWeight<Runtime>;
}
impl pallet_utility::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = pallet_utility::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    pub const ReversibleTransfersPalletIdValue: PalletId = PalletId(*b"rtpallet");
    pub const DefaultDelay: BlockNumber = 10;
    pub const MinDelayPeriod: BlockNumber = 2;
    pub const MaxReversibleTransfers: u32 = 10;
}

impl pallet_reversible_transfers::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type SchedulerOrigin = OriginCaller;
    type Scheduler = Scheduler;
    type BlockNumberProvider = System;
    type MaxPendingPerAccount = MaxReversibleTransfers;
    type DefaultDelay = DefaultDelay;
    type MinDelayPeriod = MinDelayPeriod;
    type PalletId = ReversibleTransfersPalletIdValue;
    type Preimages = Preimage;
    type WeightInfo = pallet_reversible_transfers::weights::SubstrateWeight<Runtime>;
    type RuntimeHoldReason = RuntimeHoldReason;
}
