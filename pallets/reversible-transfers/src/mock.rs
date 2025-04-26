use crate as pallet_reversible_transfers;
use frame_support::{
    derive_impl, ord_parameter_types, parameter_types,
    traits::{EitherOfDiverse, EqualPrivilegeOnly},
    PalletId,
};
use frame_system::{limits::BlockWeights, EnsureRoot, EnsureSignedBy};
use sp_core::{ConstU128, ConstU32};
use sp_runtime::{BuildStorage, Perbill, Weight};

type Block = frame_system::mocking::MockBlock<Test>;
pub type Balance = u128;
pub type AccountId = u64;

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
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId;
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

parameter_types! {
    pub const ReversibleTransfersPalletIdValue: PalletId = PalletId(*b"rtpallet");
    pub const BlockHashCount: u32 = 250;
    pub const DefaultDelay: u64 = 10;
    pub const MinDelayPeriod: u64 = 2;
    pub const MaxReversibleTransfers: u32 = 100;
}

impl pallet_reversible_transfers::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type SchedulerOrigin = OriginCaller;
    type RuntimeHoldReason = RuntimeHoldReason;
    type Scheduler = Scheduler;
    type BlockNumberProvider = System;
    type MaxPendingPerAccount = MaxReversibleTransfers;
    type DefaultDelay = DefaultDelay;
    type MinDelayPeriod = MinDelayPeriod;
    type PalletId = ReversibleTransfersPalletIdValue;
    type Preimages = Preimage;
    type WeightInfo = ();
}

impl pallet_preimage::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    type Currency = ();
    type ManagerOrigin = EnsureRoot<u64>;
    type Consideration = ();
}

parameter_types! {
    pub storage MaximumSchedulerWeight: Weight = Perbill::from_percent(80) *
        BlockWeights::default().max_block;
}
ord_parameter_types! {
    pub const One: u64 = 1;
}

impl pallet_scheduler::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeOrigin = RuntimeOrigin;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = EitherOfDiverse<EnsureRoot<u64>, EnsureSignedBy<One, u64>>;
    type OriginPrivilegeCmp = EqualPrivilegeOnly;
    type MaxScheduledPerBlock = ConstU32<10>;
    type WeightInfo = ();
    type Preimages = Preimage;
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
    let mut t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(1, 1_000_000_000_000_000), (2, 2)],
    }
    .assimilate_storage(&mut t)
    .unwrap();

    pallet_reversible_transfers::GenesisConfig::<Test> {
        initial_reversible_accounts: vec![(1, 10)],
    }
    .assimilate_storage(&mut t)
    .unwrap();

    t.into()
}
