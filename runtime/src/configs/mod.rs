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
		GlobalMaxMembers, MinRankOfClassConverter, PreimageDeposit,
		RootOrMemberForTechReferendaOrigin, TechCollectiveTracksInfo,
	},
	MILLI_UNIT,
};
use frame_support::{
	derive_impl, parameter_types,
	traits::{
		ConstU128, ConstU16, ConstU32, ConstU8, EitherOfDiverse, EnsureOrigin, Get,
		NeverEnsureOrigin, VariantCountOf,
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
	EnsureRoot, EnsureRootWithSuccess,
};
use pallet_ranked_collective::Linear;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use smallvec::smallvec;

use qp_scheduler::BlockNumberOrTimestamp;
use sp_runtime::{
	traits::{BlakeTwo256, One},
	AccountId32, Perbill, Permill,
};
use sp_version::RuntimeVersion;

// Local module imports
use super::{
	AccountId, AssetId, Balance, Balances, Block, BlockNumber, Hash, Nonce, OriginCaller,
	PalletInfo, Preimage, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason,
	RuntimeHoldReason, RuntimeOrigin, RuntimeTask, Scheduler, System, Timestamp, Wormhole, ZkTree,
	DAYS, EXISTENTIAL_DEPOSIT, MAX_SUPPLY, MICRO_UNIT, MILLIS_PER_DAY, TARGET_BLOCK_TIME_MS, UNIT,
	VERSION,
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
	type MaxSupply = ConstU128<{ MAX_SUPPLY }>;
	type EmissionDivisor = ConstU128<15_163_560>; // Divide remaining supply by this amount
	type Treasury = pallet_treasury::Pallet<Runtime>;
	type MintingAccount = MintingAccount;
	type Unit = MiningUnit;
}

parameter_types! {
	/// Target block time ms
	pub const TargetBlockTime: u64 = TARGET_BLOCK_TIME_MS;
	pub const TimestampBucketSize: u64 = 2 * TARGET_BLOCK_TIME_MS; // Nyquist frequency
	/// Initial mining difficulty
	pub const QPoWInitialDifficulty: U512 = U512([4_000_000, 0, 0, 0, 0, 0, 0, 0]);
}

impl pallet_qpow::Config for Runtime {
	type InitialDifficulty = QPoWInitialDifficulty;
	type TargetBlockTime = TargetBlockTime;
	type MaxReorgDepth = ConstU32<100>;
	type WeightInfo = ();
}

parameter_types! {
	/// Canonical minting account for native token operations (mining rewards, wormhole exits).
	/// Used as the `from` address in TransferProofs when native tokens are minted.
	/// This is a well-known sentinel address, not a real account.
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
	// Undeciding timeout (45 days): a submitted referendum that is NOT in the track queue —
	// e.g. one that never received a decision deposit — is rejected as TimedOut after this
	// long. Referenda that ARE queued for deciding are exempt: the timeout check
	// (pallets/referenda/src/lib.rs, `service_referendum`) is gated on `!status.in_queue`,
	// so a queued referendum that simply never gets a free deciding slot is NOT timed out.
	pub const UndecidingTimeout: BlockNumber = 45 * DAYS;
	pub const AlarmInterval: BlockNumber = 1;
}

parameter_types! {
	pub const MinRankOfClassDelta: u16 = 0;
	pub const MaxMemberCount: u32 = 13;
}
impl pallet_ranked_collective::Config for Runtime {
	type WeightInfo = pallet_ranked_collective::weights::SubstrateWeight<Runtime>;
	type RuntimeEvent = RuntimeEvent;
	// #91267: membership changes go through Root only (i.e. a passed TechReferenda vote), so no
	// single member can unilaterally add/remove others or stuff the collective. Root operates at
	// rank 0, matching the flat collective.
	type AddOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>;
	type RemoveOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>;
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
// 1. **No relay chain constraints:** Unlike parachains, solo chains have no external entity
//    imposing PoV size limits. Validators have full state access.
//
// 2. **Full nodes validate blocks:** All validators maintain complete state, so they don't need
//    witnesses to re-execute transactions.
//
// 3. **ref_time provides sufficient protection:** Compute-bound benchmarking (ref_time) naturally
//    correlates with state access patterns. Heavy state reads/writes increase ref_time, providing
//    indirect protection against state-heavy transactions.
//
// 4. **Block length limits storage abuse:** The 5 MB block size limit caps the amount of data that
//    can be included per block, preventing bandwidth-based attacks.
//
// **When to revisit this decision:**
//
// This design should be reconsidered if the chain adopts features where proof/witness
// size becomes a meaningful resource constraint:
//
// - **Light client support:** Light clients verify blocks using state proofs. Large witnesses
//   increase sync times and bandwidth requirements for light clients.
//
// - **Cross-chain bridges:** Bridge protocols often require merkle proofs of state. Unbounded proof
//   sizes could make bridge operations expensive or impractical.
//
// - **Stateless validation:** If the chain moves toward stateless block validation (validators
//   don't keep full state), witness size becomes a critical resource.
//
// - **ZK proof generation:** If state proofs are used as inputs to ZK circuits, proof size directly
//   impacts prover time and memory requirements.
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
	type HighSecurity = HighSecurityConfig;
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
	type HighSecurity = HighSecurityConfig;
}

parameter_types! {
	pub const ReversibleTransfersPalletIdValue: PalletId = PalletId(*b"rtpallet");
	pub const DefaultDelay: BlockNumberOrTimestamp<BlockNumber, Moment> = BlockNumberOrTimestamp::BlockNumber(DAYS);
	pub const MinDelayPeriodBlocks: BlockNumber = 2;
	pub const MaxGuardianAccounts: u32 = 32;
	pub const MaxPendingPerAccount: u32 = 16;
	/// Volume fee for reversed transactions from high-security accounts only (1% fee is burned)
	pub const HighSecurityVolumeFee: Permill = Permill::from_percent(1);
}

impl pallet_reversible_transfers::Config for Runtime {
	type AssetId = AssetId;
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
	type MaxGuardianAccounts = MaxGuardianAccounts;
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
	pub const VestingPalletId: PalletId = PalletId(*b"qvesting");
	/// Vesting payouts are rounded down to multiples of the wormhole leaf quantum
	/// (`SCALE_DOWN_FACTOR`): a sub-quantum transfer would be committed as a
	/// zero-value leaf, stranding funds paid to keyless beneficiaries.
	pub const VestingPayoutQuantum: Balance = pallet_wormhole::SCALE_DOWN_FACTOR;
	/// One QUAN keeps every payout above the existential deposit and the Wormhole
	/// circuit's fee-consuming minimum.
	pub const VestingMinimumPayout: Balance = UNIT;
	pub const VestingMinClaimInterval: u64 = MILLIS_PER_DAY;
}

/// The quantum above is anchored to the wormhole pallet's constant, but the value that
/// actually decides whether a leaf is non-zero is the ZK tree's. They are the same
/// number today; if they ever diverge, sub-quantum payouts would round to zero-value
/// leaves and strand funds on keyless beneficiaries.
const _: () = assert!(
	pallet_wormhole::SCALE_DOWN_FACTOR == pallet_zk_tree::tree::AMOUNT_SCALE_DOWN_FACTOR,
	"vesting payout quantum must match the ZK tree's leaf amount scale factor"
);

/// A ZK leaf commits `amount / AMOUNT_SCALE_DOWN_FACTOR` as a `u32`, saturating at
/// `u32::MAX`. A payout past that ceiling would move real funds while committing a
/// clamped leaf, leaving the excess unexitable for a keyless beneficiary. Nothing in
/// the runtime bounds a single vesting payout below the ceiling — total issuance does:
/// no payout can exceed the maximum supply.
const _: () = assert!(
	MAX_SUPPLY < (u32::MAX as Balance) * pallet_zk_tree::tree::AMOUNT_SCALE_DOWN_FACTOR,
	"a single payout could exceed the ZK leaf's u32 amount ceiling"
);

/// The configured treasury account as an `Option` — unlike
/// `pallet_treasury::Pallet::account_id()`, this never panics on a chain whose
/// genesis omitted the treasury; vesting admin calls fail with an explicit error instead.
pub struct TreasuryAccountOption;
impl Get<Option<AccountId>> for TreasuryAccountOption {
	fn get() -> Option<AccountId> {
		pallet_treasury::Pallet::<Runtime>::treasury_account()
	}
}

/// `Signed(who)` where `who` is the configured treasury account.
///
/// The treasury is a multisig in real deployments; the multisig pallet dispatches
/// approved proposals as `RawOrigin::Signed(multisig_address)`, so a plain
/// signed-origin check covers it.
pub struct EnsureTreasury;
impl EnsureOrigin<RuntimeOrigin> for EnsureTreasury {
	type Success = AccountId;
	fn try_origin(o: RuntimeOrigin) -> Result<Self::Success, RuntimeOrigin> {
		match (o.clone().into(), pallet_treasury::Pallet::<Runtime>::treasury_account()) {
			(Ok(frame_system::RawOrigin::Signed(who)), Some(treasury)) if who == treasury =>
				Ok(who),
			_ => Err(o),
		}
	}
	#[cfg(feature = "runtime-benchmarks")]
	fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
		pallet_treasury::Pallet::<Runtime>::treasury_account()
			.map(RuntimeOrigin::signed)
			.ok_or(())
	}
}

impl pallet_vesting::Config for Runtime {
	type Currency = Balances;
	type TimeProvider = Timestamp;
	type PalletId = VestingPalletId;
	type AdminOrigin = EitherOfDiverse<EnsureRoot<AccountId>, EnsureTreasury>;
	type TreasuryAccount = TreasuryAccountOption;
	type AssetId = AssetId;
	// The pallet records its payouts itself so Root calls enacted by the scheduler
	// (invisible to the event-scanning extension) still create ZK-tree leaves; the
	// extension skips pot-sourced events to avoid double-recording signed paths.
	type ProofRecorder = Wormhole;
	type PayoutQuantum = VestingPayoutQuantum;
	type MinimumPayout = VestingMinimumPayout;
	type MinClaimInterval = VestingMinClaimInterval;
	type WeightInfo = pallet_vesting::weights::SubstrateWeight<Runtime>;
}

// Multisig configuration
parameter_types! {
	pub const MultisigPalletId: PalletId = PalletId(*b"py/mltsg");
	pub const MaxSigners: u32 = 100;
	pub const MaxTotalProposalsInStorage: u32 = 200; // Max Active + Approved proposals per multisig
	pub const MaxCallSize: u32 = 10240; // 10KB
	pub const MultisigFee: Balance = 600 * MILLI_UNIT; // 0.6 UNIT (non-refundable, burned)
	pub const ProposalDeposit: Balance = 1000 * MILLI_UNIT; // 1 UNIT (locked until cleanup)
	pub const ProposalFee: Balance = 1000 * MILLI_UNIT; // 1 UNIT (non-refundable)
	pub const SignerStepFactorParam: Permill = Permill::from_percent(1);
	pub const MaxExpiryDuration: BlockNumber = 100_800; // ~2 weeks at 12s blocks (14 days * 24h * 60m * 60s / 12s)
	// Maximum weight for inner calls executed via multisig.
	// Set to ~50% of max block weight to allow for multisig bookkeeping overhead.
	// ref_time: 1 second (half of 2s max), proof_size: 2.5 MB (half of 5 MB max)
	pub MaxInnerCallWeight: Weight = Weight::from_parts(1_000_000_000_000, 2_621_440);
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
/// Whitelist includes only delayed, reversible operations plus vesting claims:
/// - `schedule_transfer`: Schedule delayed native token transfer
/// - `cancel`: Cancel pending delayed transfer
/// - `recover_funds`: Guardian-initiated recovery
/// - `Vesting::claim`: safe because the payout goes to the schedule's stored beneficiary, never to
///   the caller
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
			) | RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel { .. }) |
				RuntimeCall::ReversibleTransfers(
					pallet_reversible_transfers::Call::recover_funds { .. }
				) | RuntimeCall::Vesting(pallet_vesting::Call::claim { .. })
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
	type ProposalDeposit = ProposalDeposit;
	type ProposalFee = ProposalFee;
	type SignerStepFactor = SignerStepFactorParam;
	type MaxExpiryDuration = MaxExpiryDuration;
	type MaxInnerCallWeight = MaxInnerCallWeight;
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

parameter_types! {
	/// Volume fee rate in basis points (4 bps = 0.04%).
	/// The circuit already enforces a one-quantum (0.01 QUAN) minimum fee via ceil
	/// rounding of `(out₁ + out₂) × 10000 ≤ input × (10000 − bps)`, so small exits
	/// pay a flat 0.01 QUAN and large exits pay the headline rate. There is no
	/// separate on-chain minimum exit amount.
	pub const VolumeFeeRateBps: u32 = 4;
	/// Proportion of volume fees to burn (50% burned, 50% to miner)
	pub const VolumeFeesBurnRate: Permill = Permill::from_percent(50);
	/// Half of the burn bucket on public-batch exits goes to the aggregator instead.
	pub const VolumeFeesAggregatorRate: Permill = Permill::from_percent(50);
}

impl pallet_wormhole::Config for Runtime {
	type NativeBalance = Balance;
	type Currency = Balances;
	type AssetId = AssetId;
	type AssetBalance = Balance;
	type TransferCount = u64;
	/// Use the same MintingAccount as mining-rewards for consistency.
	/// Both pallets mint native tokens and should use the same sentinel "from" address.
	type MintingAccount = MintingAccount;
	type VolumeFeeRateBps = VolumeFeeRateBps;
	type VolumeFeesBurnRate = VolumeFeesBurnRate;
	type VolumeFeesAggregatorRate = VolumeFeesAggregatorRate;
	type WormholeAccountId = AccountId32;
	type WeightInfo = pallet_wormhole::weights::SubstrateWeight<Runtime>;
	type ZkTree = ZkTree;
}

impl pallet_zk_tree::Config for Runtime {
	type AssetId = AssetId;
	type Balance = Balance;
}
