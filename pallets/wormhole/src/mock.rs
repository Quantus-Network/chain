// use crate as pallet_wormhole;
// use frame_support::{
//     construct_runtime, parameter_types,
//     traits::{ConstU32, ConstU64, Everything},
// };
// use sp_core::H256;
// use sp_runtime::{
//     traits::{BlakeTwo256, IdentityLookup},
//     BuildStorage,
// };
// use sp_io;
// use frame_system::mocking::{MockBlock};
// use pallet_balances;
// use pallet_balances::AccountData;


// type Block = MockBlock<Test>;
// type Balance = u64;

// parameter_types! {
//     pub const ExistentialDeposit: Balance = 1;
//     pub const MaxLocks: u32 = 50;
//     pub const MaxReserves: u32 = 50;
//     pub const MaxFreezes: u32 = 50;
// }
// impl frame_system::Config for Test {
//     type RuntimeEvent = RuntimeEvent;
//     type BaseCallFilter = Everything;
//     type BlockWeights = ();
//     type BlockLength = ();
//     type RuntimeOrigin = RuntimeOrigin;
//     type RuntimeCall = RuntimeCall;
//     type RuntimeTask = ();
//     type Nonce = u64;
//     type Hash = H256;
//     type Hashing = BlakeTwo256;
//     type AccountId = u64;
//     type Lookup = IdentityLookup<Self::AccountId>;
//     type Block = Block;
//     type BlockHashCount = ConstU64<250>;
//     type DbWeight = ();
//     type Version = ();
//     type PalletInfo = PalletInfo;
//     type AccountData = AccountData<Balance>;
//     type OnNewAccount = ();
//     type OnKilledAccount = ();
//     type SystemWeightInfo = ();
//     type SS58Prefix = ();
//     type OnSetCode = ();
//     type MaxConsumers = ConstU32<16>;
//     type SingleBlockMigrations = ();
//     type MultiBlockMigrator = ();
//     type PreInherents = ();
//     type PostInherents = ();
//     type PostTransactions = ();
// }

// impl pallet_balances::Config for Test {
//     type Balance = Balance;
//     type DustRemoval = ();
//     type RuntimeEvent = RuntimeEvent;
//     type ExistentialDeposit = ExistentialDeposit;
//     type AccountStore = System;
//     type WeightInfo = ();
//     type MaxLocks = MaxLocks;
//     type MaxReserves = MaxReserves;
//     type ReserveIdentifier = [u8; 8];
//     type RuntimeHoldReason = ();
//     type RuntimeFreezeReason = ();
//     type FreezeIdentifier = ();
//     type MaxFreezes = MaxFreezes;
// }

// // Configure a mock runtime to test the pallet.
// construct_runtime!(
//     pub enum Test where
//         Block = Block,
//         NodeBlock = Block,
//         UncheckedExtrinsic = UncheckedExtrinsic,
//     {
//         System: frame_system,
//         Balances: pallet_balances,
//         Wormhole: pallet_wormhole,
//     }
// );


// impl pallet_wormhole::Config for Test {
//     type RuntimeEvent = RuntimeEvent;
// }

// // Helper function to build a genesis configuration
// pub fn new_test_ext() -> sp_io::TestExternalities {
//     frame_system::GenesisConfig::<Test>::default()
//         .build_storage()
//         .unwrap()
//         .into()
// }
