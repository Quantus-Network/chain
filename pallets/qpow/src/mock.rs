use crate as pallet_qpow;
use frame_support::{
	pallet_prelude::{ConstU32, TypedGet},
	parameter_types,
	traits::{ConstU128, ConstU64, ConstU8, Everything},
};
use primitive_types::U512;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};
use std::ops::Shl;

type Block = frame_system::mocking::MockBlock<Test>;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const MinimumPeriod: u64 = 100; // 100ms
}

impl frame_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type BaseCallFilter = Everything;
	type Block = Block;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	// Change Index to Nonce
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
	type RuntimeTask = ();
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
	type ExtensionsWeightInfo = ();
}

frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		Timestamp: pallet_timestamp,
		QPow: pallet_qpow,
	}
);

impl pallet_timestamp::Config for Test {
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

parameter_types! {
	pub const TestInitialDifficulty: U512 = U512([0, 0, 0, 0, 0, 0, 0, 1000000]);
}

impl pallet_qpow::Config for Test {
	type WeightInfo = ();
	type EmaAlpha = ConstU32<500>;
	type InitialDifficulty = TestInitialDifficulty;
	type DifficultyAdjustPercentClamp = ConstU8<10>;
	type TargetBlockTime = ConstU64<1000>;
	type MaxReorgDepth = ConstU32<10>;
	type FixedU128Scale = ConstU128<1_000_000_000_000_000_000>;
}

// Build genesis storage according to the mock runtime
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	// Add QPow genesis configuration
	pallet_qpow::GenesisConfig::<Test> { initial_difficulty: None, _phantom: Default::default() }
		.assimilate_storage(&mut t)
		.unwrap();

	t.into()
}
