use crate as pallet_merkle_airdrop;
use frame_support::{
	parameter_types,
	traits::{ConstU32, Everything, WithdrawReasons},
	PalletId,
};
use frame_system::{self as system};
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, ConvertInto, IdentityLookup},
	BuildStorage,
};

type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Vesting: pallet_vesting,
		Balances: pallet_balances,
		MerkleAirdrop: pallet_merkle_airdrop,
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 189;
}

impl system::Config for Test {
	type BaseCallFilter = Everything;
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
	type Block = Block;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<u64>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
	type RuntimeTask = ();
	type ExtensionsWeightInfo = ();
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
	type RuntimeEvent = RuntimeEvent;
}

parameter_types! {
	pub const ExistentialDeposit: u64 = 1;
	pub const MaxLocks: u32 = 50;
	pub const MintingAccount: u64 = 1;
}

impl pallet_balances::Config for Test {
	type Balance = u64;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = ();
	type MaxLocks = MaxLocks;
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	type RuntimeHoldReason = ();
	type FreezeIdentifier = ();
	type MaxFreezes = ();
	type RuntimeFreezeReason = ();
	type DoneSlashHandler = ();
	type MintingAccount = MintingAccount;
}

parameter_types! {
	pub const MinVestedTransfer: u64 = 1;
	pub UnvestedFundsAllowedWithdrawReasons: WithdrawReasons =
	WithdrawReasons::except(WithdrawReasons::TRANSFER | WithdrawReasons::RESERVE);
}

impl pallet_vesting::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type WeightInfo = ();
	type BlockNumberProvider = System;
	type MinVestedTransfer = MinVestedTransfer;
	type BlockNumberToBalance = ConvertInto;
	type UnvestedFundsAllowedWithdrawReasons = UnvestedFundsAllowedWithdrawReasons;

	const MAX_VESTING_SCHEDULES: u32 = 3;
}

parameter_types! {
	pub const MaxProofs: u32 = 100;
	pub const MerkleAirdropPalletId: PalletId = PalletId(*b"airdrop!");
	pub const UnsignedClaimPriority: u64 = 100;
}

impl pallet_merkle_airdrop::Config for Test {
	type Vesting = Vesting;
	type MaxProofs = MaxProofs;
	type PalletId = MerkleAirdropPalletId;
	type UnsignedClaimPriority = UnsignedClaimPriority;
	type WeightInfo = ();
	type BlockNumberProvider = System;
	type BlockNumberToBalance = ConvertInto;
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(1, 10_000_000), (MerkleAirdrop::account_id(), 1)],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
