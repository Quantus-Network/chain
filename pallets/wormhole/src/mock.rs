use crate::{self as pallet_wormhole};
use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU128, ConstU32, Everything},
};
use frame_system::mocking::MockUncheckedExtrinsic;
use qp_poseidon::PoseidonHasher;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage, Permill,
};

// Re-export shared test helpers from qp_wormhole
pub use qp_wormhole::{account_id, MINTING_ACCOUNT};

construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		Assets: pallet_assets,
		Wormhole: pallet_wormhole,
	}
);

pub type Balance = u128;
/// 1 QUAN = 10^12 (12 decimal places)
pub const UNIT: Balance = 1_000_000_000_000;
pub type AccountId = sp_core::crypto::AccountId32;
pub type Block<T> = sp_runtime::generic::Block<
	qp_header::Header<u64, PoseidonHasher, sp_runtime::traits::BlakeTwo256>,
	MockUncheckedExtrinsic<T, qp_dilithium_crypto::DilithiumSignatureScheme>,
>;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
}

impl frame_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type BaseCallFilter = Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type RuntimeTask = ();
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block<Self>;
	type BlockHashCount = BlockHashCount;
	type DbWeight = ();
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<Balance>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type ExtensionsWeightInfo = ();
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
}

parameter_types! {
	pub const ExistentialDeposit: Balance = 1;
}

impl pallet_balances::Config for Test {
	type RuntimeHoldReason = ();
	type RuntimeFreezeReason = ();
	type WeightInfo = ();
	type Balance = Balance;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type ReserveIdentifier = [u8; 8];
	type FreezeIdentifier = ();
	type MaxLocks = ConstU32<50>;
	type MaxReserves = ();
	type MaxFreezes = ();
	type DoneSlashHandler = ();
	type RuntimeEvent = RuntimeEvent;
}

impl pallet_assets::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type AssetId = u32;
	type AssetIdParameter = u32;
	type Currency = Balances;
	type CreateOrigin =
		frame_support::traits::AsEnsureOriginWithArg<frame_system::EnsureSigned<AccountId>>;
	type ForceOrigin = frame_system::EnsureRoot<AccountId>;
	type AssetDeposit = ConstU128<1>;
	type AssetAccountDeposit = ConstU128<1>;
	type MetadataDepositBase = ConstU128<1>;
	type MetadataDepositPerByte = ConstU128<1>;
	type ApprovalDeposit = ConstU128<1>;
	type StringLimit = ConstU32<50>;
	type Freezer = ();
	type Extra = ();
	type WeightInfo = ();
	type RemoveItemsLimit = ConstU32<1000>;
	type CallbackHandle = ();
	type Holder = ();
	type ReserveData = ();
}

parameter_types! {
	/// The "from" account used when recording transfer proofs for minted tokens.
	/// Uses the shared MINTING_ACCOUNT constant from qp_wormhole.
	pub const MintingAccount: AccountId = MINTING_ACCOUNT;
	/// Minimum transfer amount (10 QUAN)
	pub const MinimumTransferAmount: Balance = 10 * UNIT;
	/// Volume fee rate in basis points (10 bps = 0.1%)
	pub const VolumeFeeRateBps: u32 = 10;
	/// Proportion of volume fees to burn (50% burned, 50% to miner)
	pub const VolumeFeesBurnRate: Permill = Permill::from_percent(50);
}

impl pallet_wormhole::Config for Test {
	type NativeBalance = Balance;
	type Currency = Balances;
	type Assets = Assets;
	type AssetId = u32;
	type AssetBalance = Balance;
	type TransferCount = u64;
	type MintingAccount = MintingAccount;
	type MinimumTransferAmount = MinimumTransferAmount;
	type VolumeFeeRateBps = VolumeFeeRateBps;
	type VolumeFeesBurnRate = VolumeFeesBurnRate;
	type WormholeAccountId = AccountId;
	type WeightInfo = crate::weights::SubstrateWeight<Test>;
	type ZkTree = (); // Disabled in tests - use () no-op implementation
}

// Helper function to build a genesis configuration
pub fn new_test_ext() -> sp_state_machine::TestExternalities<PoseidonHasher> {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	t.into()
}

/// Build test externalities with genesis endowments.
/// Each endowment is (address, amount) and will have both balance and TransferProof recorded
/// (after block 1 initialization), enabling the address to spend via ZK proofs.
///
/// Note: This sets up the genesis state, but TransferProofs are recorded in on_initialize
/// at block 1. Tests should call `System::set_block_number(1)` and then trigger
/// `Wormhole::on_initialize(1)` to process the endowments.
pub fn new_test_ext_with_endowments(
	endowments: Vec<(AccountId, Balance)>,
) -> sp_state_machine::TestExternalities<PoseidonHasher> {
	use sp_runtime::BuildStorage;

	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	// Set up balances for the endowed accounts
	pallet_balances::GenesisConfig::<Test> {
		balances: endowments.to_vec(),
		dev_accounts: None,
	}
	.assimilate_storage(&mut t)
	.unwrap();

	// Set up endowments to be processed at block 1
	pallet_wormhole::GenesisConfig::<Test> {
		endowed_addresses: endowments,
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
