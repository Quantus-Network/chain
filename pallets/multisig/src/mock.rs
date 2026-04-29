//! Mock runtime for testing pallet-multisig.
//! Single mock used for both unit tests and benchmark tests; implements
//! `pallet_reversible_transfers::Config` so that benchmark test suite compiles and runs.

use core::{cell::RefCell, marker::PhantomData};

use crate as pallet_multisig;
use frame_support::{
	derive_impl, ord_parameter_types, parameter_types,
	traits::{ConstU32, EitherOfDiverse, EqualPrivilegeOnly, Time},
	PalletId,
};
use frame_system::{limits::BlockWeights, EnsureRoot, EnsureSignedBy};
use qp_scheduler::BlockNumberOrTimestamp;
use sp_core::ConstU128;
use sp_runtime::{BuildStorage, Perbill, Permill, Weight};

type Block = frame_system::mocking::MockBlock<Test>;
pub type Balance = u128;
pub type AccountId = sp_core::crypto::AccountId32;

// account_id from u64 (first 8 bytes = id.to_le_bytes()) — same as in tests
pub fn account_id(id: u64) -> AccountId {
	let mut data = [0u8; 32];
	data[0..8].copy_from_slice(&id.to_le_bytes());
	AccountId::new(data)
}

#[frame_support::pallet]
pub mod mock_heavy_call {
	use frame_support::{dispatch::DispatchResult, weights::Weight};
	use frame_system::pallet_prelude::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight(Weight::from_parts(2_000_000_000, 2_097_152))]
		pub fn too_heavy(origin: OriginFor<T>) -> DispatchResult {
			ensure_signed(origin)?;
			Ok(())
		}
	}
}

#[frame_support::runtime]
mod runtime {
	use super::*;

	#[runtime::runtime]
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
	pub type Balances = pallet_balances::Pallet<Test>;

	#[runtime::pallet_index(2)]
	pub type Multisig = pallet_multisig::Pallet<Test>;

	#[runtime::pallet_index(3)]
	pub type Preimage = pallet_preimage::Pallet<Test>;

	#[runtime::pallet_index(4)]
	pub type Scheduler = pallet_scheduler::Pallet<Test>;

	#[runtime::pallet_index(5)]
	pub type Recovery = pallet_recovery::Pallet<Test>;

	#[runtime::pallet_index(6)]
	pub type Utility = pallet_utility::Pallet<Test>;

	#[runtime::pallet_index(7)]
	pub type Assets = pallet_assets::Pallet<Test>;

	#[runtime::pallet_index(8)]
	pub type AssetsHolder = pallet_assets_holder::Pallet<Test>;

	#[runtime::pallet_index(9)]
	pub type ReversibleTransfers = pallet_reversible_transfers::Pallet<Test>;

	#[runtime::pallet_index(10)]
	pub type HeavyCall = mock_heavy_call::Pallet<Test>;
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
	type RuntimeFreezeReason = RuntimeFreezeReason;
	type FreezeIdentifier = ();
	type MaxFreezes = ();
	type DoneSlashHandler = ();
}

parameter_types! {
	pub const MultisigPalletId: PalletId = PalletId(*b"py/mltsg");
	pub const MaxSignersParam: u32 = 10;
	pub const MaxTotalProposalsInStorageParam: u32 = 20;
	pub const MaxCallSizeParam: u32 = 1024;
	pub const MultisigFeeParam: Balance = 1000;
	pub const ProposalDepositParam: Balance = 100;
	// Use 999 instead of 1000 to catch early floor truncation bugs.
	// With step_factor=1%, per-signer increase = floor(999 * 1%) = 9 (truncated from 9.99)
	// The correct formula multiplies first: floor(999 * signers * 1%) to preserve precision.
	pub const ProposalFeeParam: Balance = 999;
	pub const SignerStepFactorParam: Permill = Permill::from_parts(10_000);
	pub const MaxExpiryDurationParam: u64 = 10000;
	// 1 billion ref_time, 1 MB proof_size - generous limit for testing
	pub const MaxInnerCallWeightParam: Weight = Weight::from_parts(1_000_000_000, 1_048_576);
}

impl pallet_multisig::Config for Test {
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type MaxSigners = MaxSignersParam;
	type MaxTotalProposalsInStorage = MaxTotalProposalsInStorageParam;
	type MaxCallSize = MaxCallSizeParam;
	type MultisigFee = MultisigFeeParam;
	type ProposalDeposit = ProposalDepositParam;
	type ProposalFee = ProposalFeeParam;
	type SignerStepFactor = SignerStepFactorParam;
	type MaxExpiryDuration = MaxExpiryDurationParam;
	type MaxInnerCallWeight = MaxInnerCallWeightParam;
	type PalletId = MultisigPalletId;
	type WeightInfo = ();
	type HighSecurity = crate::tests::MockHighSecurity;
}

impl mock_heavy_call::Config for Test {}

type Moment = u64;

thread_local! {
	static MOCKED_TIME: RefCell<Moment> = const { RefCell::new(69420) };
}

pub struct MockTimestamp<T>(PhantomData<T>);

impl<T: pallet_scheduler::Config> MockTimestamp<T>
where
	T::Moment: From<Moment>,
{
	pub fn set_timestamp(now: Moment) {
		MOCKED_TIME.with(|v| *v.borrow_mut() = now);
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
	pub const MaxPendingPerAccount: u32 = 16;
	pub const HighSecurityVolumeFee: Permill = Permill::from_percent(1);
}

/// Mock proof recorder that does nothing (for tests)
pub struct MockProofRecorder;

impl qp_wormhole::TransferProofRecorder<AccountId, u32, Balance> for MockProofRecorder {
	fn record_transfer_proof(
		_asset_id: Option<u32>,
		_from: AccountId,
		_to: AccountId,
		_amount: Balance,
	) {
	}
}

impl pallet_reversible_transfers::Config for Test {
	type SchedulerOrigin = OriginCaller;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Scheduler = Scheduler;
	type BlockNumberProvider = System;
	type DefaultDelay = DefaultDelay;
	type MinDelayPeriodBlocks = MinDelayPeriodBlocks;
	type MinDelayPeriodMoment = MinDelayPeriodMoment;
	type PalletId = ReversibleTransfersPalletIdValue;
	type Preimages = Preimage;
	type WeightInfo = ();
	type Moment = Moment;
	type TimeProvider = MockTimestamp<Test>;
	type MaxInterceptorAccounts = MaxInterceptorAccounts;
	type MaxPendingPerAccount = MaxPendingPerAccount;
	type VolumeFee = HighSecurityVolumeFee;
	type ProofRecorder = MockProofRecorder;
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
	type ReserveData = ();
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
	pub storage MaximumSchedulerWeight: Weight =
		Perbill::from_percent(80) * BlockWeights::default().max_block;
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

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_balances::GenesisConfig::<Test> {
		balances: vec![
			(account_id(1), 100_000),
			(account_id(2), 200_000),
			(account_id(3), 300_000),
			(account_id(4), 400_000),
			(account_id(5), 500_000),
		],
		dev_accounts: None,
	}
	.assimilate_storage(&mut t)
	.unwrap();

	pallet_reversible_transfers::GenesisConfig::<Test> { initial_high_security_accounts: vec![] }
		.assimilate_storage(&mut t)
		.unwrap();

	t.into()
}
