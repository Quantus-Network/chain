#![cfg_attr(not(feature = "std"), no_std)]
#![deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

extern crate alloc;
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

pub mod apis;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarks;
pub mod configs;

pub use qp_dilithium_crypto::{
	Dilithium65Pair, Dilithium65Public, Dilithium65Signature, Dilithium65SignatureWithPublic,
	Dilithium87Public, Dilithium87Signature, DilithiumSignatureScheme,
};

use alloc::vec::Vec;
use sp_core::U512;
use sp_runtime::{
	generic, impl_opaque_keys,
	traits::{BlakeTwo256, IdentifyAccount, Verify},
	MultiAddress,
};
use sp_version::RuntimeVersion;

pub use frame_system::Call as SystemCall;
pub use pallet_balances::Call as BalancesCall;
pub use pallet_reversible_transfers as ReversibleTransfersCall;
pub use pallet_timestamp::Call as TimestampCall;

#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;

pub mod genesis_config_presets;
pub mod governance;
pub mod transaction_extensions;

pub use governance::origins::pallet_custom_origins;

/// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
/// the specifics of the runtime. They can then be made to be agnostic over specific formats
/// of data like extrinsics, allowing for them to continue syncing the network through upgrades
/// to even the core data structures.
pub mod opaque {
	use super::*;
	use sp_runtime::generic;

	pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

	// Block header with Poseidon block hash and BlakeTwo256 for state trie
	pub type Header = qp_header::Header<BlockNumber, BlakeTwo256>;

	// Opaque block type.
	pub type Block = generic::Block<Header, UncheckedExtrinsic>;
	// Opaque block identifier type.
	pub type BlockId = generic::BlockId<Block>;

	// Opaque block hash type (H256).
	pub type Hash = sp_core::H256;
}

impl_opaque_keys! {
	pub struct SessionKeys {
		// pub a*ura: A*ura,
		// pub g*randpa: G*randpa,
	}
}

// Runtime versioning: https://docs.substrate.io/main-docs/build/upgrade#runtime-versioning
//
// Do not increment `spec_version` (or `runtime/Cargo.toml`) in feature PRs.
// The Quantus - Release Proposal workflow does that when `is_runtime_upgrade`
// is set, and ships the matching node binary + wasm together. Native execution
// substitutes for on-chain Wasm only when spec_name, spec_version, and
// authoring_version all match, so leaving this number alone on main does not
// activate a new verifier on a live chain. See docs/RUNTIME_UPDATE.md.
//
// Bump `transaction_version` only when the signed extrinsic encoding changes
// (TxExtension set or payload layout) — not for verifier-rule changes.
#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
	spec_name: alloc::borrow::Cow::Borrowed("quantus-runtime"),
	impl_name: alloc::borrow::Cow::Borrowed("quantus-runtime"),
	authoring_version: 1,
	spec_version: 147,
	impl_version: 1,
	apis: apis::RUNTIME_API_VERSIONS,
	transaction_version: 6,
	system_version: 1,
};

// Time is measured by number of blocks.
pub const TARGET_BLOCK_TIME_MS: u64 = 12_000;

/// Derived time units expressed in number of blocks (e.g. 60s / 12s = 5 blocks per minute)
pub const MINUTES: BlockNumber = (60_000u64 / TARGET_BLOCK_TIME_MS) as BlockNumber;
pub const HOURS: BlockNumber = MINUTES * 60;
pub const DAYS: BlockNumber = HOURS * 24;

pub const BLOCK_HASH_COUNT: BlockNumber = 2400;

// Unit = the base number of indivisible units for balances
pub const UNIT: Balance = 1_000_000_000_000;
pub const MILLI_UNIT: Balance = 1_000_000_000;
pub const MICRO_UNIT: Balance = 1_000_000;

/// Existential deposit.
pub const EXISTENTIAL_DEPOSIT: Balance = MILLI_UNIT;

/// Hard cap on total issuance; mining emissions stop here.
pub const MAX_SUPPLY: Balance = 21_000_000 * UNIT;

/// Central fee dial. Every absolute-QUAN price in the runtime — weight/base and
/// length fees, multisig fees and deposit, preimage and referendum deposits, the
/// high-security inclusion-fee cap — is derived through [`scale_fee`], so editing
/// this one ratio (plus a runtime upgrade) repositions the whole price level.
/// Percentage rates (wormhole bps, reversal/step factors), the existential
/// deposit, and the 0.01-QUAN circuit quanta are deliberately not scaled.
pub const FEE_SCALE_NUM: Balance = 1;
pub const FEE_SCALE_DEN: Balance = 1;

pub const fn scale_fee(base: Balance) -> Balance {
	base * FEE_SCALE_NUM / FEE_SCALE_DEN
}

/// Wall-clock day in milliseconds — the unit vesting schedules and claim cadence are
/// expressed in (`pallet_timestamp` moments, not block numbers).
pub const MILLIS_PER_DAY: u64 = 24 * 60 * 60 * 1000;

/// Alias to 512-bit hash when used in the context of a transaction signature on the chain.
// pub type Signature = MultiSignature;
pub type Signature = DilithiumSignatureScheme;

/// Some way of identifying an account on the chain. We intentionally make it equivalent
/// to the public key of our transaction signing scheme.
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

/// Balance of an account.
pub type Balance = u128;

/// Id type for assets
pub type AssetId = u32;

/// Index of a transaction in the chain.
pub type Nonce = u32;

/// A hash of some data used by the chain.
pub type Hash = sp_core::H256;

/// An index to a block.
pub type BlockNumber = u32;

/// The address format for describing accounts.
pub type Address = MultiAddress<AccountId, ()>;

/// Block header type as expected by this runtime.
/// Uses Poseidon for block hash and BlakeTwo256 for state trie / extrinsics root.
pub type Header = qp_header::Header<BlockNumber, BlakeTwo256>;

/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

/// A Block signed with a Justification
pub type SignedBlock = generic::SignedBlock<Block>;

/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;

/// Type of the difficulty
pub type Difficulty = U512;

/// The SignedExtension to the basic transaction logic.
pub type TxExtension = (
	frame_system::CheckNonZeroSender<Runtime>,
	frame_system::CheckSpecVersion<Runtime>,
	frame_system::CheckTxVersion<Runtime>,
	frame_system::CheckGenesis<Runtime>,
	frame_system::CheckEra<Runtime>,
	frame_system::CheckNonce<Runtime>,
	frame_system::CheckWeight<Runtime>,
	transaction_extensions::ReversibleTransactionExtension<Runtime>,
	// Must run before `ChargeTransactionPayment`: post-dispatch hooks execute
	// left-to-right, and payment finalizes the fee from `PostDispatchInfo` at its
	// turn — the wormhole recorder's refund of statically over-charged per-transfer
	// weight only reaches the payer's fee if it lands first.
	transaction_extensions::WormholeProofRecorderExtension<Runtime>,
	// The high-security zero-tip policy is NOT enforced here: it lives in
	// `transaction_extensions::HighSecurityFungibleAdapter` (the configured
	// `OnChargeTransaction`), which every fee path of this extension goes
	// through, so no refactor of this tuple can silently reopen the tip channel.
	pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
	frame_metadata_hash_extension::CheckMetadataHash<Runtime>,
	// Must stay last: re-runs the block-weight reclaim so that refunds made by the
	// extensions above (e.g. the wormhole recorder returning statically over-charged
	// per-transfer weight) are returned to block capacity. `CheckWeight`'s own reclaim
	// runs before those refunds exist; `reclaim_weight` is idempotent via
	// `ExtrinsicWeightReclaimed`, so running it twice never double-counts.
	frame_system::WeightReclaim<Runtime>,
);

/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
	generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;

/// The payload being signed in transactions.
pub type SignedPayload = generic::SignedPayload<RuntimeCall, TxExtension>;

/// All storage migrations to run on runtime upgrade.
pub type Migrations = (
	// v1 -> v2: delete the removed wormhole soundness counters.
	pallet_wormhole::migrations::MigrateV1ToV2<Runtime>,
	// v0 -> v1: no-op version bump (TreasuryPortion is no longer written).
	pallet_treasury::migrations::MigrateV0ToV1<Runtime>,
	// v1 -> v2: kill leftover TreasuryPortion; treasury is not paid from emission.
	pallet_treasury::migrations::MigrateV1ToV2<Runtime>,
);

/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
	Runtime,
	Block,
	frame_system::ChainContext<Runtime>,
	Runtime,
	AllPalletsWithSystem,
	Migrations,
>;

// Create the runtime by composing the FRAME pallets that were previously configured.
#[frame_support::runtime]
mod runtime {
	#[runtime::runtime]
	#[runtime::derive(
		RuntimeCall,
		RuntimeEvent,
		RuntimeError,
		RuntimeOrigin,
		RuntimeFreezeReason,
		RuntimeHoldReason,
		RuntimeSlashReason,
		RuntimeLockId,
		RuntimeTask
	)]
	pub struct Runtime;

	#[runtime::pallet_index(0)]
	pub type System = frame_system;

	#[runtime::pallet_index(1)]
	pub type Timestamp = pallet_timestamp;

	#[runtime::pallet_index(2)]
	pub type Balances = pallet_balances;

	#[runtime::pallet_index(3)]
	pub type TransactionPayment = pallet_transaction_payment;

	// Index 4 was `pallet_sudo` (removed). Kept vacant so downstream pallet indices stay stable.

	#[runtime::pallet_index(5)]
	pub type QPoW = pallet_qpow;

	#[runtime::pallet_index(6)]
	pub type MiningRewards = pallet_mining_rewards;

	#[runtime::pallet_index(7)]
	pub type Preimage = pallet_preimage;

	// The scheduler is used internally for reversible transfers and governance via the
	// `Scheduler`/`ScheduleNamed` trait. Its extrinsics are disabled so users cannot place
	// arbitrary transactions onto the scheduler.
	#[runtime::pallet_index(8)]
	#[runtime::disable_call]
	pub type Scheduler = pallet_scheduler;

	#[runtime::pallet_index(9)]
	pub type Utility = pallet_utility;

	// Index 10 was the community `Referenda` instance (removed with the public/token-weighted
	// governance lane). Kept vacant so downstream pallet indices stay stable.

	#[runtime::pallet_index(11)]
	pub type ReversibleTransfers = pallet_reversible_transfers;

	// Index 12 was `ConvictionVoting` (removed with the community lane). Kept vacant.

	#[runtime::pallet_index(13)]
	pub type TechCollective = pallet_ranked_collective;

	#[runtime::pallet_index(14)]
	pub type TechReferenda = pallet_referenda::Pallet<Runtime, Instance1>;

	#[runtime::pallet_index(15)]
	pub type TreasuryPallet = pallet_treasury;

	// Index 16 was `pallet_recovery` (removed). Kept vacant so downstream pallet indices stay
	// stable.

	// Index 17 was `pallet_assets` (removed). Kept vacant so downstream pallet indices stay stable.

	// Index 18 was `pallet_assets_holder` (removed with assets). Kept vacant.

	#[runtime::pallet_index(19)]
	pub type Multisig = pallet_multisig;

	#[runtime::pallet_index(20)]
	pub type Wormhole = pallet_wormhole;

	#[runtime::pallet_index(21)]
	pub type ZkTree = pallet_zk_tree;

	#[runtime::pallet_index(22)]
	pub type Vesting = pallet_vesting;

	// Custom governance origins (no calls, no storage): dispatch origins for the
	// non-Root tech-referenda tracks, e.g. `FastUpgrade`.
	#[runtime::pallet_index(23)]
	pub type Origins = pallet_custom_origins;
}
