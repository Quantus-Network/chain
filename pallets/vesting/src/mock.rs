// pallets/vesting/src/mock.rs
use core::convert::{TryFrom, TryInto};
use frame_support::{
	parameter_types,
	traits::{ConstU32, Hooks},
	PalletId,
	__private::sp_io,
};
use sp_runtime::{
	testing::H256,
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};

use crate as pallet_vesting; // Your pallet

// Define the test runtime
frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		Timestamp: pallet_timestamp,
		Vesting: pallet_vesting
	}
);

pub type Balance = u128;

pub type Block = frame_system::mocking::MockBlock<Test>;

// System config
parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

impl frame_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type BaseCallFilter = frame_support::traits::Everything;
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
	type MaxConsumers = ConstU32<16>;
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
}

// Balances config
parameter_types! {
	pub const ExistentialDeposit: u128 = 1;
	pub const MaxLocks: u32 = 50;
	pub const MaxReserves: u32 = 50;
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

// Timestamp config
parameter_types! {
	pub const MinimumPeriod: u64 = 1;
}

impl pallet_timestamp::Config for Test {
	type Moment = u64; // Milliseconds
	type OnTimestampSet = ();
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

// Vesting config
parameter_types! {
	pub const VestingPalletId: PalletId = PalletId(*b"vestpal_");
	pub const MaxSchedules: u32 = 100;
}

impl pallet_vesting::Config for Test {
	type PalletId = VestingPalletId;
	type WeightInfo = ();
}

// Helper to build genesis storage
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(1, 100000), (2, 2000)], // Accounts 1 and 2 with funds
	}
	.assimilate_storage(&mut t)
	.unwrap();

	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| System::set_block_number(1)); // Start at block 1
	ext.execute_with(|| {
		pallet_timestamp::Pallet::<Test>::set(RuntimeOrigin::none(), 5)
			.expect("Cannot set time to now")
	}); // Start at block 1

	ext
}

pub fn run_to_block(n: u64, timestamp: u64) {
	while System::block_number() < n {
		let block_number = System::block_number();

		// Run on_finalize for the current block
		System::on_finalize(block_number);
		// pallet_timestamp::Pallet::<Test>::on_finalize(block_number);

		// Increment block number
		// println!("setting block number to {}", block_number);
		System::set_block_number(block_number + 1);

		System::on_initialize(block_number + 1);
	}
	pallet_timestamp::Pallet::<Test>::on_finalize(n);
	// println!("setting timestamp to {}", timestamp);
	pallet_timestamp::Pallet::<Test>::set(RuntimeOrigin::none(), timestamp)
		.expect("Cannot set time");
}
