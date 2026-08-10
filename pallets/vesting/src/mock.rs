use crate as pallet_vesting;

use core::cell::{Cell, RefCell};
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

/// Defaults of the `static` config values below. Named so the per-test reset in
/// [`reset_thread_local_state`] restores exactly what a fresh thread would start with.
const DEFAULT_EXISTENTIAL_DEPOSIT: Balance = 1_000;
const DEFAULT_PAYOUT_QUANTUM: Balance = 1_000;
const DEFAULT_MINIMUM_PAYOUT: Balance = 10_000;
const DEFAULT_MIN_CLAIM_INTERVAL: u64 = 100_000;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	/// `static` so individual tests can vary it via `ExistentialDeposit::set`.
	pub static ExistentialDeposit: Balance = DEFAULT_EXISTENTIAL_DEPOSIT;
	pub const VestingPalletId: PalletId = PalletId(*b"qvesting");
	/// `static` so tests can unset it to exercise `TreasuryNotConfigured`.
	pub static TreasuryAccount: Option<AccountId32> = Some(TREASURY);
	/// `static` so tests can vary the wormhole leaf quantum (e.g. make it coarser than
	/// the ED to exercise sub-quantum rounding, or finer to exercise below-ED payouts).
	pub static PayoutQuantum: Balance = DEFAULT_PAYOUT_QUANTUM;
	pub static MinimumPayout: Balance = DEFAULT_MINIMUM_PAYOUT;
	pub static MinClaimInterval: u64 = DEFAULT_MIN_CLAIM_INTERVAL;
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

/// One recorded payout proof: `(from, to, amount)`.
pub type RecordedProof = (AccountId32, AccountId32, Balance);

thread_local! {
	static RECORDED_PROOFS: RefCell<Vec<RecordedProof>> = const { RefCell::new(Vec::new()) };
	static DROP_CREDITS: Cell<bool> = const { Cell::new(false) };
}

/// Captures every payout the pallet records, for test assertions.
pub struct MockProofRecorder;

impl MockProofRecorder {
	pub fn recorded() -> Vec<RecordedProof> {
		RECORDED_PROOFS.with(|proofs| proofs.borrow().clone())
	}

	/// Make the recorder report every subsequent credit as deliberately dropped
	/// (the `false` branch the `TransferProofRecorder` contract permits).
	pub fn set_drop_credits(drop: bool) {
		DROP_CREDITS.with(|flag| flag.set(drop));
	}

	fn clear() {
		RECORDED_PROOFS.with(|proofs| proofs.borrow_mut().clear());
		DROP_CREDITS.with(|flag| flag.set(false));
	}
}

impl qp_wormhole::TransferProofRecorder<AccountId32, u32, Balance> for MockProofRecorder {
	fn record_transfer_proof(
		asset_id: Option<u32>,
		from: AccountId32,
		to: AccountId32,
		amount: Balance,
	) -> bool {
		assert!(asset_id.is_none(), "vesting payouts are always native");
		if DROP_CREDITS.with(|flag| flag.get()) {
			return false;
		}
		RECORDED_PROOFS.with(|proofs| proofs.borrow_mut().push((from, to, amount)));
		true
	}
}

impl pallet_vesting::Config for Test {
	type Currency = Balances;
	type TimeProvider = Timestamp;
	type PalletId = VestingPalletId;
	type AdminOrigin = EitherOfDiverse<EnsureRoot<AccountId32>, EnsureTreasury>;
	type TreasuryAccount = TreasuryAccount;
	type AssetId = u32;
	type ProofRecorder = MockProofRecorder;
	type PayoutQuantum = PayoutQuantum;
	type MinimumPayout = MinimumPayout;
	type MinClaimInterval = MinClaimInterval;
	type WeightInfo = ();
}

pub fn pot() -> AccountId32 {
	Vesting::pot_account_id()
}

pub fn set_time(now_ms: u64) {
	pallet_timestamp::Now::<Test>::put(now_ms);
}

pub type ScheduleTuple = (AccountId32, u64, u64, u64, u128);

/// Pot endowed with exactly `sum(totals) + ED` — the valid genesis shape. The ED comes
/// from the const rather than the `static`: the builder resets the statics anyway, so
/// reading the live value here would only be a chance to read a leaked one.
pub fn new_test_ext(schedules: Vec<ScheduleTuple>) -> sp_io::TestExternalities {
	let sum: u128 = schedules.iter().map(|(_, _, _, _, total)| total).sum();
	new_test_ext_with_pot_balance(schedules, sum + DEFAULT_EXISTENTIAL_DEPOSIT)
}

/// The `pub static` config values, `RECORDED_PROOFS`, and the drop-credits switch live
/// in thread-local storage, and the test harness reuses worker threads across tests:
/// without this, a value a test sets (`PayoutQuantum::set(3_000)`,
/// `TreasuryAccount::set(None)`, …) leaks into whichever test the same worker runs
/// next, and recorded-proof assertions see earlier tests' entries. Every test builds
/// its externalities through here, so resetting at build time makes each one start
/// from the documented defaults.
fn reset_thread_local_state() {
	ExistentialDeposit::set(DEFAULT_EXISTENTIAL_DEPOSIT);
	TreasuryAccount::set(Some(TREASURY));
	PayoutQuantum::set(DEFAULT_PAYOUT_QUANTUM);
	MinimumPayout::set(DEFAULT_MINIMUM_PAYOUT);
	MinClaimInterval::set(DEFAULT_MIN_CLAIM_INTERVAL);
	MockProofRecorder::clear();
}

pub fn new_test_ext_with_pot_balance(
	schedules: Vec<ScheduleTuple>,
	pot_balance: Balance,
) -> sp_io::TestExternalities {
	reset_thread_local_state();
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
	ext.execute_with(|| {
		System::set_block_number(1);
		// Stamp in-code storage versions like the real runtime genesis build does
		// (this mock assembles storage from individual pallet configs, which skips it).
		<AllPalletsWithSystem as frame_support::traits::OnGenesis>::on_genesis();
	});
	ext
}
