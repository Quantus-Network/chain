use crate as pallet_wormhole;
use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU32, Everything},
	weights::IdentityFee,
};
use frame_system::mocking::MockUncheckedExtrinsic;
use qp_poseidon::PoseidonHasher;
use sp_core::H256;
use sp_runtime::{traits::IdentityLookup, BuildStorage};
// --- MOCK RUNTIME ---

construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
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

// --- FRAME SYSTEM ---

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

// --- PALLET BALANCES ---

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
}

// --- PALLET WORMHOLE ---

parameter_types! {
	pub const MintingAccount: AccountId = AccountId::new([
		231, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
		0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
	]);
}

impl pallet_wormhole::Config for Test {
	type WeightInfo = crate::weights::SubstrateWeight<Test>;
	type WeightToFee = IdentityFee<Balance>;
	type Currency = Balances;
	type MintingAccount = MintingAccount;
}

// Helper function to build a genesis configuration
pub fn new_test_ext() -> sp_state_machine::TestExternalities<PoseidonHasher> {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	let endowment = 1e18 as Balance;
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(account_id(1), endowment), (account_id(2), endowment)],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
