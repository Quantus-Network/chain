use crate as pallet_mining_rewards;

use frame_support::{
	parameter_types,
	traits::{ConstU32, Everything, Hooks},
	PalletId,
};
use qp_poseidon::PoseidonHasher;
use sp_consensus_pow::POW_ENGINE_ID;
use sp_runtime::{
	app_crypto::sp_core,
	testing::H256,
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage, Digest, DigestItem,
};

// Configure a mock runtime to test the pallet
frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		MiningRewards: pallet_mining_rewards,
	}
);

pub type Balance = u128;
pub type Block = frame_system::mocking::MockBlock<Test>;
const UNIT: u128 = 1_000_000_000_000u128;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 189;
	pub const MaxSupply: u128 = 21_000_000 * UNIT;
	pub const EmissionDivisor: u128 = 26_280_000;
	pub const ExistentialDeposit: Balance = 1;
	pub const TreasuryPalletId: PalletId = PalletId(*b"py/trsry");
}

impl frame_system::Config for Test {
	type BaseCallFilter = Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type RuntimeTask = ();
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = sp_core::crypto::AccountId32;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block;
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
	type MaxConsumers = frame_support::traits::ConstU32<16>;
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
	type RuntimeEvent = RuntimeEvent;
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
	type MaxFreezes = ConstU32<0>;
	type DoneSlashHandler = ();
}

parameter_types! {
	pub const TreasuryPortion: u8 = 50; // 50% goes to treasury in tests (matching runtime)
	pub const MintingAccount: sp_core::crypto::AccountId32 = sp_core::crypto::AccountId32::new([99u8; 32]);
}

impl pallet_mining_rewards::Config for Test {
	type Currency = Balances;
	type WeightInfo = ();
	type MaxSupply = MaxSupply;
	type EmissionDivisor = EmissionDivisor;
	type TreasuryPortion = TreasuryPortion;
	type TreasuryPalletId = TreasuryPalletId;
	type MintingAccount = MintingAccount;
}

/// Helper function to convert a u8 to a preimage
pub fn miner_preimage(id: u8) -> [u8; 32] {
	[id; 32]
}

/// Helper function to derive wormhole address from preimage
pub fn wormhole_address_from_preimage(preimage: [u8; 32]) -> sp_core::crypto::AccountId32 {
	let hash = PoseidonHasher::hash_padded(&preimage);
	sp_core::crypto::AccountId32::from(hash)
}

// Configure default miner preimages and addresses for tests
pub fn miner_preimage_1() -> [u8; 32] {
	miner_preimage(1)
}

pub fn miner_preimage_2() -> [u8; 32] {
	miner_preimage(2)
}

pub fn miner() -> sp_core::crypto::AccountId32 {
	wormhole_address_from_preimage(miner_preimage_1())
}

pub fn miner2() -> sp_core::crypto::AccountId32 {
	wormhole_address_from_preimage(miner_preimage_2())
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(miner(), ExistentialDeposit::get()), (miner2(), ExistentialDeposit::get())],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| System::set_block_number(1)); // Start at block 1
	ext
}

// Helper function to create a block digest with a miner preimage
pub fn set_miner_digest(miner_account: sp_core::crypto::AccountId32) {
	// Find the preimage that corresponds to this miner address
	let preimage = if miner_account == miner() {
		miner_preimage_1()
	} else if miner_account == miner2() {
		miner_preimage_2()
	} else {
		// For other miners, use their raw bytes as preimage for testing
		let mut preimage = [0u8; 32];
		preimage.copy_from_slice(miner_account.as_ref());
		preimage
	};

	set_miner_preimage_digest(preimage);
}

// Helper function to create a block digest with a specific preimage
pub fn set_miner_preimage_digest(preimage: [u8; 32]) {
	let pre_digest = DigestItem::PreRuntime(POW_ENGINE_ID, preimage.to_vec());
	let digest = Digest { logs: vec![pre_digest] };

	// Set the digest in the system
	System::reset_events();
	System::initialize(&1, &sp_core::H256::default(), &digest);
}

// Helper function to run a block
pub fn run_to_block(n: u64) {
	while System::block_number() < n {
		let block_number = System::block_number();

		// Run on_finalize for the current block
		MiningRewards::on_finalize(block_number);
		System::on_finalize(block_number);

		// Increment block number
		System::set_block_number(block_number + 1);

		// Run on_initialize for the next block
		System::on_initialize(block_number + 1);
		MiningRewards::on_initialize(block_number + 1);
	}
}
