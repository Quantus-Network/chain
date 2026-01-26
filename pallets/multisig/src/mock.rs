//! Mock runtime for testing pallet-multisig

use crate as pallet_multisig;
use frame_support::{
	parameter_types,
	traits::{ConstU32, Everything},
	PalletId,
};
use sp_core::{crypto::AccountId32, H256};
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage, Permill,
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
	type AccountId = AccountId32;
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
	pub const MintingAccount: AccountId32 = AccountId32::new([99u8; 32]);
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
	type MintingAccount = MintingAccount;
}

parameter_types! {
	pub const MultisigPalletId: PalletId = PalletId(*b"py/mltsg");
	pub const MaxSignersParam: u32 = 10;
	pub const MaxActiveProposalsParam: u32 = 50; // For testing
	pub const MaxTotalProposalsInStorageParam: u32 = 20; // 2x MaxActiveProposals
	pub const MaxCallSizeParam: u32 = 1024;
	pub const MultisigFeeParam: Balance = 1000; // Non-refundable fee
	pub const MultisigDepositParam: Balance = 500; // Refundable deposit
	pub const ProposalDepositParam: Balance = 100;
	pub const ProposalFeeParam: Balance = 1000; // Non-refundable fee
	pub const SignerStepFactorParam: Permill = Permill::from_parts(10_000); // 1%
	pub const MaxExpiryDurationParam: u64 = 10000; // 10000 blocks for testing (enough for all test scenarios)
}

impl pallet_multisig::Config for Test {
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type MaxSigners = MaxSignersParam;
	type MaxActiveProposals = MaxActiveProposalsParam;
	type MaxTotalProposalsInStorage = MaxTotalProposalsInStorageParam;
	type MaxCallSize = MaxCallSizeParam;
	type MultisigFee = MultisigFeeParam;
	type MultisigDeposit = MultisigDepositParam;
	type ProposalDeposit = ProposalDepositParam;
	type ProposalFee = ProposalFeeParam;
	type SignerStepFactor = SignerStepFactorParam;
	type MaxExpiryDuration = MaxExpiryDurationParam;
	type PalletId = MultisigPalletId;
	type WeightInfo = ();
}

// Helper to create AccountId32 from u64
pub fn account_id(id: u64) -> AccountId32 {
	let mut data = [0u8; 32];
	data[0..8].copy_from_slice(&id.to_le_bytes());
	AccountId32::new(data)
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_balances::GenesisConfig::<Test> {
		balances: vec![
			(account_id(1), 100000), // Alice
			(account_id(2), 200000), // Bob
			(account_id(3), 300000), // Charlie
			(account_id(4), 400000), // Dave
			(account_id(5), 500000), // Eve
		],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
