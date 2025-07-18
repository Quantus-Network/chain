#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

pub mod apis;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarks;
pub mod configs;

pub use dilithium_crypto::ResonancePublic;
pub use dilithium_crypto::ResonanceSignature;
pub use dilithium_crypto::ResonanceSignatureScheme;

use alloc::vec::Vec;
use sp_runtime::{
    generic, impl_opaque_keys,
    traits::{IdentifyAccount, Verify},
    MultiAddress,
};
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

pub use frame_system::Call as SystemCall;
pub use pallet_balances::Call as BalancesCall;
pub use pallet_merkle_airdrop;
pub use pallet_reversible_transfers as ReversibleTransfersCall;
pub use pallet_timestamp::Call as TimestampCall;

#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;

pub mod genesis_config_presets;
pub mod transaction_extensions;

pub mod governance;

use crate::governance::pallet_custom_origins;
use poseidon_resonance::PoseidonHasher;

/// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
/// the specifics of the runtime. They can then be made to be agnostic over specific formats
/// of data like extrinsics, allowing for them to continue syncing the network through upgrades
/// to even the core data structures.
pub mod opaque {
    use super::*;
    use sp_runtime::{generic, traits::Hash as HashT};

    pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

    // For whatever reason, changing this causes the block hash and
    // the storage root to be computed with poseidon, but not the extrinsics root.
    // For the wormhole proofs, we only need the storage root to be calculated with poseidon.
    // However, some internal checks in dev build expect extrinsics_root to be computed with same
    // Hash function, so we change the configs/mod.rs Hashing type as well
    // Opaque block header type.
    pub type Header = generic::Header<BlockNumber, PoseidonHasher>;

    // Opaque block type.
    pub type Block = generic::Block<Header, UncheckedExtrinsic>;
    // Opaque block identifier type.
    pub type BlockId = generic::BlockId<Block>;

    // Opaque block hash type.
    pub type Hash = <PoseidonHasher as HashT>::Output;
}

impl_opaque_keys! {
    pub struct SessionKeys {
        // pub a*ura: A*ura,
        // pub g*randpa: G*randpa,
    }
}

// To learn more about runtime versioning, see:
// https://docs.substrate.io/main-docs/build/upgrade#runtime-versioning
#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
    spec_name: alloc::borrow::Cow::Borrowed("quantus-runtime"),
    impl_name: alloc::borrow::Cow::Borrowed("quantus-runtime"),
    authoring_version: 1,
    // The version of the runtime specification. A full node will not attempt to use its native
    //   runtime in substitute for the on-chain Wasm runtime unless all of `spec_name`,
    //   `spec_version`, and `authoring_version` are the same between Wasm and native.
    // This value is set to 100 to notify Polkadot-JS App (https://polkadot.js.org/apps) to use
    //   the compatible custom types.
    spec_version: 104,
    impl_version: 1,
    apis: apis::RUNTIME_API_VERSIONS,
    transaction_version: 1,
    system_version: 1,
};

// Time is measured by number of blocks.
pub const MINUTES: BlockNumber = 60;
pub const HOURS: BlockNumber = MINUTES * 60;
pub const DAYS: BlockNumber = HOURS * 24;

pub const BLOCK_HASH_COUNT: BlockNumber = 2400;

// Unit = the base number of indivisible units for balances
pub const UNIT: Balance = 1_000_000_000_000;
pub const MILLI_UNIT: Balance = 1_000_000_000;
pub const MICRO_UNIT: Balance = 1_000_000;

/// Existential deposit.
pub const EXISTENTIAL_DEPOSIT: Balance = MILLI_UNIT;

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
    NativeVersion {
        runtime_version: VERSION,
        can_author_with: Default::default(),
    }
}

/// Alias to 512-bit hash when used in the context of a transaction signature on the chain.
// pub type Signature = MultiSignature;
pub type Signature = ResonanceSignatureScheme;

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
pub type Header = generic::Header<BlockNumber, PoseidonHasher>;

/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

/// A Block signed with a Justification
pub type SignedBlock = generic::SignedBlock<Block>;

/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;

/// The SignedExtension to the basic transaction logic.
pub type TxExtension = (
    frame_system::CheckNonZeroSender<Runtime>,
    frame_system::CheckSpecVersion<Runtime>,
    frame_system::CheckTxVersion<Runtime>,
    frame_system::CheckGenesis<Runtime>,
    frame_system::CheckEra<Runtime>,
    frame_system::CheckNonce<Runtime>,
    frame_system::CheckWeight<Runtime>,
    pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
    frame_metadata_hash_extension::CheckMetadataHash<Runtime>,
    transaction_extensions::ReversibleTransactionExtension<Runtime>,
);

/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
    generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;

/// The payload being signed in transactions.
pub type SignedPayload = generic::SignedPayload<RuntimeCall, TxExtension>;

/// All migrations of the runtime, aside from the ones declared in the pallets.
///
/// This can be a tuple of types, each implementing `OnRuntimeUpgrade`.
#[allow(unused_parens)]
type Migrations = ();

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

    #[runtime::pallet_index(4)]
    pub type Sudo = pallet_sudo;

    #[runtime::pallet_index(5)]
    pub type QPoW = pallet_qpow;

    #[runtime::pallet_index(6)]
    pub type Wormhole = pallet_wormhole;

    #[runtime::pallet_index(7)]
    pub type MiningRewards = pallet_mining_rewards;

    #[runtime::pallet_index(8)]
    pub type Vesting = pallet_vesting;

    #[runtime::pallet_index(9)]
    pub type Preimage = pallet_preimage;

    #[runtime::pallet_index(10)]
    pub type Scheduler = pallet_scheduler;

    #[runtime::pallet_index(11)]
    pub type Utility = pallet_utility;

    #[runtime::pallet_index(12)]
    pub type Referenda = pallet_referenda;

    #[runtime::pallet_index(13)]
    pub type ReversibleTransfers = pallet_reversible_transfers;

    #[runtime::pallet_index(14)]
    pub type ConvictionVoting = pallet_conviction_voting;

    #[runtime::pallet_index(15)]
    pub type TechCollective = pallet_ranked_collective;

    #[runtime::pallet_index(16)]
    pub type TechReferenda = pallet_referenda::Pallet<Runtime, Instance1>;

    #[runtime::pallet_index(17)]
    pub type MerkleAirdrop = pallet_merkle_airdrop;

    #[runtime::pallet_index(18)]
    pub type TreasuryPallet = pallet_treasury;

    #[runtime::pallet_index(19)]
    pub type Origins = pallet_custom_origins;

    #[runtime::pallet_index(20)]
    pub type Recovery = pallet_recovery;

    #[runtime::pallet_index(21)]
    pub type Assets = pallet_assets;
}
