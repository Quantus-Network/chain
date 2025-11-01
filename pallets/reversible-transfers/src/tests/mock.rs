use core::{cell::RefCell, marker::PhantomData};

use crate as pallet_reversible_transfers;
use frame_support::{
	derive_impl, ord_parameter_types, parameter_types,
	traits::{EitherOfDiverse, EqualPrivilegeOnly, Time},
	PalletId,
};
use frame_system::{limits::BlockWeights, EnsureRoot, EnsureSignedBy};
use qp_scheduler::BlockNumberOrTimestamp;
use sp_core::{ConstU128, ConstU32};
use sp_runtime::{BuildStorage, Perbill, Permill, Weight};

type Block = frame_system::mocking::MockBlock<Test>;
pub type Balance = u128;
pub type AccountId = sp_core::crypto::AccountId32;

pub(crate) type ReversibleTransfersCall = pallet_reversible_transfers::Call<Test>;

/// Helper function to convert a u8 to an AccountId32
pub fn account_id(id: u8) -> AccountId {
	AccountId::new([id; 32])
}

/// Helper function for account 256 (which can't be represented as a single u8)
pub fn account_256() -> AccountId {
	let mut bytes = [0u8; 32];
	bytes[0] = 0;
	bytes[1] = 1;
	AccountId::new(bytes)
}

/// Helper functions for commonly used test account IDs
pub fn alice() -> AccountId {
	account_id(1)
}
pub fn bob() -> AccountId {
	account_id(2)
}
pub fn charlie() -> AccountId {
	account_id(3)
}
pub fn dave() -> AccountId {
	account_id(4)
}
pub fn eve() -> AccountId {
	account_id(5)
}
pub fn ferdie() -> AccountId {
	account_id(255)
}
pub fn treasury() -> AccountId {
	account_id(99)
}

/// Helper function for interceptor account (avoiding + 100 calculations)
pub fn interceptor_1() -> AccountId {
	account_id(101)
}

/// Helper function for interceptor account 2
pub fn interceptor_255() -> AccountId {
	let mut bytes = [255u8; 32];
	bytes[0] = 100; // Make it different from ferdie
	AccountId::new(bytes)
}

#[frame_support::runtime]
mod runtime {
	// The main runtime
	#[runtime::runtime]
	// Runtime Types to be generated
	#[runtime::derive(
		RuntimeCall,
		RuntimeEvent,
		RuntimeError,
		RuntimeOrigin,
		RuntimeFreezeReason,
		RuntimeHoldReason,
		RuntimeSlashReason,
		RuntimeLockId,
		RuntimeTask
	)]
	pub struct Test;

	#[runtime::pallet_index(0)]
	pub type System = frame_system::Pallet<Test>;

	#[runtime::pallet_index(1)]
	pub type ReversibleTransfers = pallet_reversible_transfers::Pallet<Test>;

	#[runtime::pallet_index(2)]
	pub type Preimage = pallet_preimage::Pallet<Test>;

	#[runtime::pallet_index(3)]
	pub type Scheduler = pallet_scheduler::Pallet<Test>;

	#[runtime::pallet_index(4)]
	pub type Balances = pallet_balances::Pallet<Test>;

	#[runtime::pallet_index(5)]
	pub type Recovery = pallet_recovery::Pallet<Test>;

	#[runtime::pallet_index(6)]
	pub type Utility = pallet_utility::Pallet<Test>;

	#[runtime::pallet_index(7)]
	pub type Assets = pallet_assets::Pallet<Test>;

	#[runtime::pallet_index(8)]
	pub type AssetsHolder = pallet_assets_holder::Pallet<Test>;
}

impl TryFrom<RuntimeCall> for pallet_balances::Call<Test> {
	type Error = ();
	fn try_from(call: RuntimeCall) -> Result<Self, Self::Error> {
		match call {
			RuntimeCall::Balances(c) => Ok(c),
			_ => Err(()),
		}
	}
}

impl TryFrom<RuntimeCall> for pallet_assets::Call<Test> {
	type Error = ();
	fn try_from(call: RuntimeCall) -> Result<Self, Self::Error> {
		match call {
			RuntimeCall::Assets(c) => Ok(c),
			_ => Err(()),
		}
	}
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountId = AccountId;
	type Lookup = sp_runtime::traits::IdentityLookup<Self::AccountId>;
	type AccountData = pallet_balances::AccountData<Balance>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type Balance = Balance;
	type DustRemoval = ();
	type ExistentialDeposit = ConstU128<1>;
	type AccountStore = frame_system::Pallet<Test>;
	type WeightInfo = ();
	type RuntimeHoldReason = RuntimeHoldReason;
	type MaxFreezes = MaxReversibleTransfers;
}

// In memory storage
thread_local! {
	static MOCKED_TIME: RefCell<Moment> = RefCell::new(69420);
}

type Moment = u64;

/// A mock `TimeProvider` that allows setting the current time for tests.
pub struct MockTimestamp<T>(PhantomData<T>);

impl<T: pallet_scheduler::Config> MockTimestamp<T>
where
	T::Moment: From<Moment>,
{
	/// Sets the current time for the `MockTimestamp` provider.
	pub fn set_timestamp(now: Moment) {
		MOCKED_TIME.with(|v| {
			*v.borrow_mut() = now;
		});
	}

	/// Resets the timestamp to a default value (e.g., 0 or a specific starting time).
	/// Good to call at the beginning of tests or `execute_with` blocks if needed.
	pub fn reset_timestamp() {
		MOCKED_TIME.with(|v| {
			*v.borrow_mut() = 69420;
		});
	}
}

impl<T> Time for MockTimestamp<T> {
	type Moment = Moment;
	fn now() -> Self::Moment {
		MOCKED_TIME.with(|v| *v.borrow())
	}
}

parameter_types! {
	pub const ReversibleTransfersPalletIdValue: PalletId = PalletId(*b"rtpallet");
	pub const DefaultDelay: BlockNumberOrTimestamp<u64, u64> = BlockNumberOrTimestamp::BlockNumber(10);
	pub const MinDelayPeriodBlocks: u64 = 2;
	pub const MinDelayPeriodMoment: u64 = 2000;
	pub const MaxReversibleTransfers: u32 = 100;
	pub const MaxInterceptorAccounts: u32 = 10;
	pub const HighSecurityVolumeFee: Permill = Permill::from_percent(1);
	/// Mock treasury account ID for tests
	pub const TreasuryAccount: AccountId = AccountId::new([99u8; 32]);
}

impl pallet_reversible_transfers::Config for Test {
	type SchedulerOrigin = OriginCaller;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Scheduler = Scheduler;
	type BlockNumberProvider = System;
	type MaxPendingPerAccount = MaxReversibleTransfers;
	type DefaultDelay = DefaultDelay;
	type MinDelayPeriodBlocks = MinDelayPeriodBlocks;
	type MinDelayPeriodMoment = MinDelayPeriodMoment;
	type PalletId = ReversibleTransfersPalletIdValue;
	type Preimages = Preimage;
	type WeightInfo = ();
	type Moment = Moment;
	type TimeProvider = MockTimestamp<Test>;
	type MaxInterceptorAccounts = MaxInterceptorAccounts;
	type VolumeFee = HighSecurityVolumeFee;
	type TreasuryAccountId = TreasuryAccount;
}

parameter_types! {
	pub const AssetDeposit: Balance = 0;
	pub const AssetAccountDeposit: Balance = 0;
	pub const AssetsStringLimit: u32 = 50;
	pub const MetadataDepositBase: Balance = 0;
	pub const MetadataDepositPerByte: Balance = 0;
}

impl pallet_assets::Config for Test {
	type Balance = Balance;
	type RuntimeEvent = RuntimeEvent;
	type AssetId = u32;
	type AssetIdParameter = codec::Compact<u32>;
	type Currency = Balances;
	type CreateOrigin =
		frame_support::traits::AsEnsureOriginWithArg<frame_system::EnsureSigned<AccountId>>;
	type ForceOrigin = frame_system::EnsureRoot<AccountId>;
	type AssetDeposit = AssetDeposit;
	type MetadataDepositBase = MetadataDepositBase;
	type MetadataDepositPerByte = MetadataDepositPerByte;
	type ApprovalDeposit = sp_core::ConstU128<0>;
	type StringLimit = AssetsStringLimit;
	type Freezer = ();
	type Extra = ();
	type WeightInfo = ();
	type CallbackHandle = pallet_assets::AutoIncAssetId<Test, ()>;
	type AssetAccountDeposit = AssetAccountDeposit;
	type RemoveItemsLimit = frame_support::traits::ConstU32<1000>;
	type Holder = pallet_assets_holder::Pallet<Test>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ();
}

impl pallet_assets_holder::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
}

parameter_types! {
	pub const ConfigDepositBase: Balance = 1;
	pub const FriendDepositFactor: Balance = 1;
	pub const MaxFriends: u32 = 9;
	pub const RecoveryDeposit: Balance = 1;
}

impl pallet_recovery::Config for Test {
	type WeightInfo = ();
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type ConfigDepositBase = ConfigDepositBase;
	type FriendDepositFactor = FriendDepositFactor;
	type MaxFriends = MaxFriends;
	type RecoveryDeposit = RecoveryDeposit;
	type BlockNumberProvider = System;
}

impl pallet_preimage::Config for Test {
	type WeightInfo = ();
	type Currency = ();
	type ManagerOrigin = EnsureRoot<AccountId>;
	type Consideration = ();
	type RuntimeEvent = RuntimeEvent;
}

parameter_types! {
	pub storage MaximumSchedulerWeight: Weight = Perbill::from_percent(80) *
		BlockWeights::default().max_block;

	pub const TimestampBucketSize: u64 = 1000;
}

ord_parameter_types! {
	pub const One: AccountId = AccountId::new([1u8; 32]);
}

impl pallet_scheduler::Config for Test {
	type RuntimeOrigin = RuntimeOrigin;
	type PalletsOrigin = OriginCaller;
	type RuntimeCall = RuntimeCall;
	type MaximumWeight = MaximumSchedulerWeight;
	type ScheduleOrigin = EitherOfDiverse<EnsureRoot<AccountId>, EnsureSignedBy<One, AccountId>>;
	type OriginPrivilegeCmp = EqualPrivilegeOnly;
	type MaxScheduledPerBlock = ConstU32<10>;
	type WeightInfo = ();
	type Preimages = Preimage;
	type Moment = Moment;
	type TimeProvider = MockTimestamp<Test>;
	type TimestampBucketSize = TimestampBucketSize;
}

impl pallet_utility::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type PalletsOrigin = OriginCaller;
	type WeightInfo = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_balances::GenesisConfig::<Test> {
		balances: vec![
			(account_id(1), 1_000_000_000_000_000),
			(account_id(2), 2),
			(account_id(3), 100_000_000_000),
			(account_id(4), 100_000_000_000),
			(account_id(5), 100_000_000_000),
			(account_id(6), 100_000_000_000),
			(account_id(7), 1_000_000_000_000),
			(account_id(8), 100_000_000_000),
			(account_id(9), 100_000_000_000),
			(account_id(255), 100_000_000_000),
			(account_256(), 100_000_000_000), // 256
			// Test accounts for interceptor tests
			(account_id(100), 100_000_000_000),
			(account_id(101), 100_000_000_000),
			(account_id(102), 100_000_000_000),
			(account_id(103), 100_000_000_000),
			(account_id(104), 100_000_000_000),
			(account_id(105), 100_000_000_000),
			(account_id(106), 100_000_000_000),
			(account_id(107), 100_000_000_000),
			(account_id(108), 100_000_000_000),
			(account_id(109), 100_000_000_000),
			(account_id(110), 100_000_000_000),
			(account_id(111), 100_000_000_000),
			// Treasury account for fee collection tests (must meet existential deposit)
			(account_id(99), 1),
		],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	pallet_reversible_transfers::GenesisConfig::<Test> {
		initial_high_security_accounts: vec![(account_id(1), account_id(2), 10)],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
