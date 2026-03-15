use crate as pallet_treasury;
use frame_support::{
	parameter_types,
	traits::{ConstU32, Everything},
};
use sp_runtime::{testing::H256, traits::IdentityLookup, BuildStorage};

frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Treasury: pallet_treasury,
	}
);

pub type Block = frame_system::mocking::MockBlock<Test>;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 189;
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
	type Hashing = sp_runtime::traits::BlakeTwo256;
	type AccountId = sp_core::crypto::AccountId32;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block;
	type BlockHashCount = BlockHashCount;
	type DbWeight = ();
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type ExtensionsWeightInfo = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
	type RuntimeEvent = RuntimeEvent;
}

impl pallet_treasury::Config for Test {
	type WeightInfo = pallet_treasury::weights::SubstrateWeight<Test>;
}

pub fn account_id(id: u8) -> sp_core::crypto::AccountId32 {
	sp_core::crypto::AccountId32::from([id; 32])
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut ext = new_test_ext_with_treasury(account_id(1), sp_runtime::Permill::from_percent(50));
	ext.execute_with(|| frame_system::Pallet::<Test>::set_block_number(1));
	ext
}

/// Build storage without treasury genesis. Used to test panic when TreasuryAccount/Portion is None.
pub fn new_test_ext_without_treasury() -> sp_io::TestExternalities {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| frame_system::Pallet::<Test>::set_block_number(1));
	ext
}

pub fn new_test_ext_with_treasury(
	treasury_account: sp_core::crypto::AccountId32,
	treasury_portion: sp_runtime::Permill,
) -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_treasury::GenesisConfig::<Test> { treasury_account, treasury_portion }
		.assimilate_storage(&mut t)
		.unwrap();

	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| frame_system::Pallet::<Test>::set_block_number(1));
	ext
}
