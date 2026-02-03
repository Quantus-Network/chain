use crate::{self as pallet_wormhole, SCALE_DOWN_FACTOR};
use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU128, ConstU32, Everything},
};
use frame_system::mocking::MockUncheckedExtrinsic;
use qp_poseidon::PoseidonHasher;
use sp_core::H256;
use sp_runtime::{traits::IdentityLookup, BuildStorage, Permill};

construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		Assets: pallet_assets,
		Wormhole: pallet_wormhole,
	}
);

pub type Balance = u128;
pub type AccountId = sp_core::crypto::AccountId32;
pub type Block<T> = sp_runtime::generic::Block<
	qp_header::Header<u64, PoseidonHasher>,
	MockUncheckedExtrinsic<T, qp_dilithium_crypto::DilithiumSignatureScheme>,
>;

/// Helper function to convert a u64 to an AccountId32
pub fn account_id(id: u64) -> AccountId {
	let mut bytes = [0u8; 32];
	bytes[0..8].copy_from_slice(&id.to_le_bytes());
	AccountId::new(bytes)
}

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
	type Hashing = PoseidonHasher;
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
}

parameter_types! {
	pub const MintingAccount: AccountId = AccountId::new([
		231, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
		0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
	]);
	/// Minimum transfer amount: 1 token (100 quantized units × SCALE_DOWN_FACTOR)
	pub const MinimumTransferAmount: Balance = 100 * SCALE_DOWN_FACTOR;
	/// Volume fee rate in basis points (10 bps = 0.1%)
	pub const VolumeFeeRateBps: u32 = 10;
	/// Proportion of volume fees to burn (50% burned, 50% to miner)
	pub const VolumeFeesBurnRate: Permill = Permill::from_percent(50);
}

impl pallet_wormhole::Config for Test {
	type WeightInfo = crate::weights::SubstrateWeight<Test>;
	type Currency = Balances;
	type Assets = Assets;
	type TransferCount = u64;
	type MintingAccount = MintingAccount;
	type MinimumTransferAmount = MinimumTransferAmount;
	type VolumeFeeRateBps = VolumeFeeRateBps;
	type VolumeFeesBurnRate = VolumeFeesBurnRate;
	type WormholeAccountId = AccountId;
}

// Helper function to build a genesis configuration
pub fn new_test_ext() -> sp_state_machine::TestExternalities<PoseidonHasher> {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	t.into()
}
