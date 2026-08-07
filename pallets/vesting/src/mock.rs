use crate as pallet_vesting;

use frame_support::{
	parameter_types,
	traits::{ConstU32, ConstU64, EitherOfDiverse, EnsureOrigin, Everything},
	PalletId,
};
use frame_system::EnsureRoot;
use sp_core::crypto::AccountId32;
use sp_runtime::{
	testing::H256,
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};

frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Timestamp: pallet_timestamp,
		Balances: pallet_balances,
		Vesting: pallet_vesting,
	}
);

pub type Balance = u128;
pub type Block = frame_system::mocking::MockBlock<Test>;
pub const UNIT: Balance = 1_000_000_000_000;

pub const ALICE: AccountId32 = AccountId32::new([1u8; 32]);
pub const BOB: AccountId32 = AccountId32::new([2u8; 32]);
pub const CHARLIE: AccountId32 = AccountId32::new([3u8; 32]);
pub const PINGER: AccountId32 = AccountId32::new([8u8; 32]);
pub const TREASURY: AccountId32 = AccountId32::new([9u8; 32]);
pub const TREASURY_FUNDS: Balance = 1_000_000 * UNIT;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	/// `static` so individual tests can vary it via `ExistentialDeposit::set`.
	pub static ExistentialDeposit: Balance = 1_000;
	pub const VestingPalletId: PalletId = PalletId(*b"qvesting");
	/// `static` so tests can unset it to exercise `TreasuryNotConfigured`.
	pub static TreasuryAccount: Option<AccountId32> = Some(TREASURY);
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
	type AccountId = AccountId32;
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
	type RuntimeEvent = RuntimeEvent;
}

impl pallet_timestamp::Config for Test {
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = ConstU64<1>;
	type WeightInfo = ();
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

/// Same shape as the runtime's `EnsureTreasury`: `Signed(who)` where `who` is the
/// configured treasury account.
pub struct EnsureTreasury;
impl EnsureOrigin<RuntimeOrigin> for EnsureTreasury {
	type Success = AccountId32;
	fn try_origin(o: RuntimeOrigin) -> Result<Self::Success, RuntimeOrigin> {
		match (o.clone().into(), TreasuryAccount::get()) {
			(Ok(frame_system::RawOrigin::Signed(who)), Some(treasury)) if who == treasury =>
				Ok(who),
			_ => Err(o),
		}
	}
	#[cfg(feature = "runtime-benchmarks")]
	fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
		TreasuryAccount::get().map(RuntimeOrigin::signed).ok_or(())
	}
}

impl pallet_vesting::Config for Test {
	type Currency = Balances;
	type TimeProvider = Timestamp;
	type PalletId = VestingPalletId;
	type AdminOrigin = EitherOfDiverse<EnsureRoot<AccountId32>, EnsureTreasury>;
	type TreasuryAccount = TreasuryAccount;
	type WeightInfo = ();
}

pub fn pot() -> AccountId32 {
	Vesting::pot_account_id()
}

pub fn set_time(now_ms: u64) {
	pallet_timestamp::Now::<Test>::put(now_ms);
}

pub type ScheduleTuple = (AccountId32, u64, u64, u64, u128);

/// Pot endowed with exactly `sum(totals) + ED` — the valid genesis shape.
pub fn new_test_ext(schedules: Vec<ScheduleTuple>) -> sp_io::TestExternalities {
	let sum: u128 = schedules.iter().map(|(_, _, _, _, total)| total).sum();
	new_test_ext_with_pot_balance(schedules, sum + ExistentialDeposit::get())
}

pub fn new_test_ext_with_pot_balance(
	schedules: Vec<ScheduleTuple>,
	pot_balance: Balance,
) -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	let mut balances = vec![(TREASURY, TREASURY_FUNDS), (PINGER, UNIT)];
	if pot_balance > 0 {
		balances.push((pot(), pot_balance));
	}
	pallet_balances::GenesisConfig::<Test> { balances, dev_accounts: None }
		.assimilate_storage(&mut t)
		.unwrap();

	pallet_vesting::GenesisConfig::<Test> { schedules }
		.assimilate_storage(&mut t)
		.unwrap();

	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| System::set_block_number(1));
	ext
}
