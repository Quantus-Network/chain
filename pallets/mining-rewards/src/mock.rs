use crate as pallet_mining_rewards;
use codec::Encode;
use frame_support::{
	__private::sp_io,
	parameter_types,
	traits::{ConstU32, Everything, Hooks},
	PalletId,
};
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

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 189;
	pub const BlockReward: u128 = 50;
	pub const ExistentialDeposit: Balance = 1;
	pub const TreasuryPalletId: PalletId = PalletId(*b"py/trsry");
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
	type AccountId = u64;
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
}

impl pallet_balances::Config for Test {
	type RuntimeEvent = RuntimeEvent;
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
	pub const TreasuryBlockReward: u128 = 50;
	pub const MintingAccount: u64 = 999;
}

impl pallet_mining_rewards::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type WeightInfo = ();
	type MinerBlockReward = BlockReward;
	type TreasuryBlockReward = TreasuryBlockReward;
	type TreasuryPalletId = TreasuryPalletId;
	type MintingAccount = MintingAccount;
}

// Configure a default miner account for tests
pub const MINER: u64 = 1;
pub const MINER2: u64 = 2;

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(MINER, ExistentialDeposit::get()), (MINER2, ExistentialDeposit::get())],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| System::set_block_number(1)); // Start at block 1
	ext
}

// Helper function to create a block digest with a miner pre-runtime digest
pub fn set_miner_digest(miner: u64) {
	let miner_bytes = miner.encode();
	let pre_digest = DigestItem::PreRuntime(POW_ENGINE_ID, miner_bytes);
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
