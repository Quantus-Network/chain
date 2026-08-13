use crate as pallet_pubkey;

use frame_support::traits::Everything;
use sp_runtime::{
	testing::H256,
	traits::{BlakeTwo256, IdentityLookup},
	AccountId32, BuildStorage,
};

frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Pubkey: pallet_pubkey,
	}
);

pub type Block = frame_system::mocking::MockBlock<Test>;

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
	type AccountId = AccountId32;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block;
	type BlockHashCount = frame_support::traits::ConstU64<250>;
	// Nonzero so tests can observe the weight `on_killed_account` registers.
	type DbWeight = frame_support::weights::constants::RocksDbWeight;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = Pubkey;
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

impl pallet_pubkey::Config for Test {}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	sp_io::TestExternalities::new(t)
}
