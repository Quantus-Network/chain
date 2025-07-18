// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # Scheduler test environment.

use super::*;

use crate as scheduler;
use core::cell::RefCell;
use frame_support::{
    derive_impl, ord_parameter_types, parameter_types,
    traits::{ConstU32, Contains, EitherOfDiverse, EqualPrivilegeOnly, OnFinalize, OnInitialize},
};
use frame_system::{EnsureRoot, EnsureSignedBy};
use sp_runtime::{BuildStorage, Perbill};

// Logger module to track execution.
#[frame_support::pallet]
pub mod logger {
    use super::{OriginCaller, OriginTrait};
    use frame_support::{pallet_prelude::*, parameter_types};
    use frame_system::pallet_prelude::*;

    parameter_types! {
        static Log: Vec<(OriginCaller, u32)> = Vec::new();
    }
    pub fn log() -> Vec<(OriginCaller, u32)> {
        Log::get().clone()
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::storage]
    pub type Threshold<T: Config> = StorageValue<_, (BlockNumberFor<T>, BlockNumberFor<T>)>;

    #[pallet::error]
    pub enum Error<T> {
        /// Under the threshold.
        TooEarly,
        /// Over the threshold.
        TooLate,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        Logged(u32, Weight),
    }

    #[pallet::call]
    impl<T: Config> Pallet<T>
    where
        <T as frame_system::Config>::RuntimeOrigin: OriginTrait<PalletsOrigin = OriginCaller>,
    {
        #[pallet::call_index(0)]
        #[pallet::weight(*weight)]
        pub fn log(origin: OriginFor<T>, i: u32, weight: Weight) -> DispatchResult {
            Self::deposit_event(Event::Logged(i, weight));
            Log::mutate(|log| {
                log.push((origin.caller().clone(), i));
            });
            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(*weight)]
        pub fn log_without_filter(origin: OriginFor<T>, i: u32, weight: Weight) -> DispatchResult {
            Self::deposit_event(Event::Logged(i, weight));
            Log::mutate(|log| {
                log.push((origin.caller().clone(), i));
            });
            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(*weight)]
        pub fn timed_log(origin: OriginFor<T>, i: u32, weight: Weight) -> DispatchResult {
            let now = frame_system::Pallet::<T>::block_number();
            let (start, end) = Threshold::<T>::get().unwrap_or((0u32.into(), u32::MAX.into()));
            ensure!(now >= start, Error::<T>::TooEarly);
            ensure!(now <= end, Error::<T>::TooLate);
            Self::deposit_event(Event::Logged(i, weight));
            Log::mutate(|log| {
                log.push((origin.caller().clone(), i));
            });
            Ok(())
        }
    }
}

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test
    {
        System: frame_system,
        Logger: logger,
        Scheduler: scheduler,
        Preimage: pallet_preimage,
    }
);

// Scheduler must dispatch with root and no filter, this tests base filter is indeed not used.
pub struct BaseFilter;
impl Contains<RuntimeCall> for BaseFilter {
    fn contains(call: &RuntimeCall) -> bool {
        !matches!(call, RuntimeCall::Logger(LoggerCall::log { .. }))
    }
}

parameter_types! {
    pub BlockWeights: frame_system::limits::BlockWeights =
        frame_system::limits::BlockWeights::simple_max(
            Weight::from_parts(2_000_000_000_000, u64::MAX),
        );
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for Test {
    type BaseCallFilter = BaseFilter;
    type Block = Block;
}
impl logger::Config for Test {
    type RuntimeEvent = RuntimeEvent;
}
ord_parameter_types! {
    pub const One: u64 = 1;
}

impl pallet_preimage::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    type Currency = ();
    type ManagerOrigin = EnsureRoot<u64>;
    type Consideration = ();
}

pub struct TestWeightInfo;
impl WeightInfo for TestWeightInfo {
    fn service_agendas_base() -> Weight {
        Weight::from_parts(0b0000_0001, 0)
    }
    fn service_agenda_base(i: u32) -> Weight {
        Weight::from_parts((i << 8) as u64 + 0b0000_0010, 0)
    }
    fn service_task_base() -> Weight {
        Weight::from_parts(0b0000_0100, 0)
    }
    fn service_task_periodic() -> Weight {
        Weight::from_parts(0b0000_1100, 0)
    }
    fn service_task_named() -> Weight {
        Weight::from_parts(0b0001_0100, 0)
    }
    fn service_task_fetched(s: u32) -> Weight {
        Weight::from_parts((s << 8) as u64 + 0b0010_0100, 0)
    }
    fn execute_dispatch_signed() -> Weight {
        Weight::from_parts(0b0100_0000, 0)
    }
    fn execute_dispatch_unsigned() -> Weight {
        Weight::from_parts(0b1000_0000, 0)
    }
    fn schedule(_s: u32) -> Weight {
        Weight::from_parts(50, 0)
    }
    fn cancel(_s: u32) -> Weight {
        Weight::from_parts(50, 0)
    }
    fn schedule_named(_s: u32) -> Weight {
        Weight::from_parts(50, 0)
    }
    fn cancel_named(_s: u32) -> Weight {
        Weight::from_parts(50, 0)
    }
    fn schedule_retry(_s: u32) -> Weight {
        Weight::from_parts(100000, 0)
    }
    fn set_retry() -> Weight {
        Weight::from_parts(50, 0)
    }
    fn set_retry_named() -> Weight {
        Weight::from_parts(50, 0)
    }
    fn cancel_retry() -> Weight {
        Weight::from_parts(50, 0)
    }
    fn cancel_retry_named() -> Weight {
        Weight::from_parts(50, 0)
    }
}
parameter_types! {
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) *
        BlockWeights::get().max_block;
}

parameter_types! {
    pub const MaxTimestampBucketSize: u64 = 10_000;
}

pub type Moment = u64;

// In memory storage
thread_local! {
    static MOCKED_TIME: RefCell<Moment> = RefCell::new(0);
}

/// A mock `TimeProvider` that allows setting the current time for tests.
pub struct MockTimestamp;

impl MockTimestamp {
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
            *v.borrow_mut() = 0; // Or any default you prefer
        });
    }
}

impl Time for MockTimestamp {
    type Moment = Moment;
    fn now() -> Self::Moment {
        MOCKED_TIME.with(|v| *v.borrow())
    }
}

impl Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeOrigin = RuntimeOrigin;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = EitherOfDiverse<EnsureRoot<u64>, EnsureSignedBy<One, u64>>;
    type MaxScheduledPerBlock = ConstU32<10>;
    type WeightInfo = TestWeightInfo;
    type OriginPrivilegeCmp = EqualPrivilegeOnly;
    type Preimages = Preimage;
    type Moment = Moment;
    type TimeProvider = MockTimestamp;
    type TimestampBucketSize = MaxTimestampBucketSize;
}

pub type LoggerCall = logger::Call<Test>;

pub fn new_test_ext() -> sp_io::TestExternalities {
    let t = system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    t.into()
}

pub fn run_to_block(n: u64) {
    while System::block_number() < n {
        Scheduler::on_finalize(System::block_number());
        System::set_block_number(System::block_number() + 1);
        Scheduler::on_initialize(System::block_number());
    }
}

pub fn root() -> OriginCaller {
    system::RawOrigin::Root.into()
}
