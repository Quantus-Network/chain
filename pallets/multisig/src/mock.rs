//! Mock runtime for testing pallet-multisig

use crate as pallet_multisig;
use frame_support::{
	parameter_types,
	traits::{ConstU32, Everything},
	PalletId,
};
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};

type Block = frame_system::mocking::MockBlock<Test>;
type Balance = u128;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		Balances: pallet_balances,
		Multisig: pallet_multisig,
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
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
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<Balance>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
	type RuntimeTask = ();
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
	type ExtensionsWeightInfo = ();
}

parameter_types! {
	pub const ExistentialDeposit: Balance = 1;
	pub const MaxLocks: u32 = 50;
	pub const MaxReserves: u32 = 50;
	pub const MaxFreezes: u32 = 50;
}

impl pallet_balances::Config for Test {
	type WeightInfo = ();
	type Balance = Balance;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type MaxLocks = MaxLocks;
	type MaxReserves = MaxReserves;
	type ReserveIdentifier = [u8; 8];
	type RuntimeHoldReason = RuntimeHoldReason;
	type RuntimeFreezeReason = RuntimeFreezeReason;
	type FreezeIdentifier = ();
	type MaxFreezes = MaxFreezes;
	type DoneSlashHandler = ();
}

parameter_types! {
	pub const MultisigPalletId: PalletId = PalletId(*b"py/mltsg");
	pub const MaxSignersParam: u32 = 10;
	pub const MaxActiveProposalsParam: u32 = 10; // For testing
	pub const MaxCallSizeParam: u32 = 1024;
	pub const MultisigDepositParam: Balance = 100;
	pub const MultisigFeeParam: Balance = 50; // Non-refundable fee
	pub const ProposalDepositParam: Balance = 10;
	pub const ProposalFeeParam: Balance = 5; // Non-refundable fee
	pub const GracePeriodParam: u64 = 100; // 100 blocks for testing
	pub const MaxExecutedProposalsQueryParam: u32 = 100; // Max results per query
}

impl pallet_multisig::Config for Test {
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type MaxSigners = MaxSignersParam;
	type MaxActiveProposals = MaxActiveProposalsParam;
	type MaxCallSize = MaxCallSizeParam;
	type MultisigDeposit = MultisigDepositParam;
	type MultisigFee = MultisigFeeParam;
	type ProposalDeposit = ProposalDepositParam;
	type ProposalFee = ProposalFeeParam;
	type GracePeriod = GracePeriodParam;
	type MaxExecutedProposalsQuery = MaxExecutedProposalsQueryParam;
	type PalletId = MultisigPalletId;
	type WeightInfo = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_balances::GenesisConfig::<Test> {
		balances: vec![
			(1, 1000), // Alice
			(2, 2000), // Bob
			(3, 3000), // Charlie
			(4, 4000), // Dave
			(5, 5000), // Eve
		],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
