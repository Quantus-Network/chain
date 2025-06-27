//! > Made with *Substrate*, for *Polkadot*.
//!
//! [![github]](https://github.com/paritytech/polkadot-sdk/tree/master/substrate/frame/scheduler) -
//! [![polkadot]](https://polkadot.network)
//!
//! [polkadot]: https://img.shields.io/badge/polkadot-E6007A?style=for-the-badge&logo=polkadot&logoColor=white
//! [github]: https://img.shields.io/badge/github-8da0cb?style=for-the-badge&labelColor=555555&logo=github
//!
//! # Scheduler Pallet
//!
//! A Pallet for scheduling runtime calls.
//!
//! ## Overview
//!
//! This Pallet exposes capabilities for scheduling runtime calls to occur at a specified block
//! number or at a specified period. These scheduled runtime calls may be named or anonymous and may
//! be canceled.
//!
//! __NOTE:__ Instead of using the filter contained in the origin to call `fn schedule`, scheduled
//! runtime calls will be dispatched with the default filter for the origin: namely
//! `frame_system::Config::BaseCallFilter` for all origin types (except root which will get no
//! filter).
//!
//! If a call is scheduled using proxy or whatever mechanism which adds filter, then those filter
//! will not be used when dispatching the schedule runtime call.
//!
//! ### Examples
//!
//! 1. Scheduling a runtime call at a specific block.
#![doc = docify::embed!("src/tests.rs", basic_scheduling_works)]
//!
//! 2. Scheduling a preimage hash of a runtime call at a specific block
#![doc = docify::embed!("src/tests.rs", scheduling_with_preimages_works)]

//!
//! ## Pallet API
//!
//! See the [`pallet`] module for more information about the interfaces this pallet exposes,
//! including its configuration trait, dispatchables, storage items, events and errors.
//!
//! ## Warning
//!
//! This Pallet executes all scheduled runtime calls in the [`on_initialize`] hook. Do not execute
//! any runtime calls which should not be considered mandatory.
//!
//! Please be aware that any scheduled runtime calls executed in a future block may __fail__ or may
//! result in __undefined behavior__ since the runtime could have upgraded between the time of
//! scheduling and execution. For example, the runtime upgrade could have:
//!
//! * Modified the implementation of the runtime call (runtime specification upgrade).
//!     * Could lead to undefined behavior.
//! * Removed or changed the ordering/index of the runtime call.
//!     * Could fail due to the runtime call index not being part of the `Call`.
//!     * Could lead to undefined behavior, such as executing another runtime call with the same
//!       index.
//!
//! [`on_initialize`]: frame_support::traits::Hooks::on_initialize

// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod impls;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod traits;
pub mod weights;

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use codec::{Decode, Encode, MaxEncodedLen};
use core::{borrow::Borrow, cmp::Ordering, marker::PhantomData};
use frame_support::{
    dispatch::{DispatchResult, GetDispatchInfo, Parameter, RawOrigin},
    ensure,
    traits::{
        schedule::{self, DispatchTime as DispatchBlock, MaybeHashed},
        Bounded, CallerTrait, EnsureOrigin, Get, IsType, OriginTrait, PrivilegeCmp, QueryPreimage,
        StorageVersion, StorePreimage, Time,
    },
    weights::{Weight, WeightMeter},
};
use frame_system::{
    pallet_prelude::BlockNumberFor,
    {self as system},
};
use qp_scheduler::{BlockNumberOrTimestamp, DispatchTime, Period, ScheduleNamed};
use scale_info::TypeInfo;
use sp_runtime::{
    traits::{BadOrigin, Dispatchable, One, Saturating, Zero},
    BoundedVec, DispatchError, RuntimeDebug,
};

pub use pallet::*;
pub use weights::WeightInfo;

/// Just a simple index for naming period tasks.
pub type PeriodicIndex = u32;
/// The location of a scheduled task that can be used to remove it.
pub type TaskAddress<BlockNumber, Moment> = (BlockNumberOrTimestamp<BlockNumber, Moment>, u32);
/// Task address of Config.
pub type TaskAddressOf<T> = TaskAddress<BlockNumberFor<T>, <T as Config>::Moment>;

pub type CallOrHashOf<T> =
    MaybeHashed<<T as Config>::RuntimeCall, <T as frame_system::Config>::Hash>;

pub type BoundedCallOf<T> =
    Bounded<<T as Config>::RuntimeCall, <T as frame_system::Config>::Hashing>;

pub type BlockNumberOrTimestampOf<T> =
    BlockNumberOrTimestamp<BlockNumberFor<T>, <T as Config>::Moment>;

/// The configuration of the retry mechanism for a given task along with its current state.
#[derive(Clone, Copy, RuntimeDebug, PartialEq, Eq, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct RetryConfig<Period> {
    /// Initial amount of retries allowed.
    total_retries: u8,
    /// Amount of retries left.
    remaining: u8,
    /// Period of time between retry attempts.
    period: Period,
}

/// Information regarding an item to be executed in the future.
#[cfg_attr(any(feature = "std", test), derive(PartialEq, Eq))]
#[derive(Clone, RuntimeDebug, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct Scheduled<Name, Call, BlockNumber, PalletsOrigin, AccountId, Moment> {
    /// The unique identity for this task, if there is one.
    maybe_id: Option<Name>,
    /// This task's priority.
    priority: schedule::Priority,
    /// The call to be dispatched.
    call: Call,
    /// If the call is periodic, then this points to the information concerning that.
    maybe_periodic: Option<Period<BlockNumber, Moment>>,
    /// The origin with which to dispatch the call.
    origin: PalletsOrigin,
    _phantom: PhantomData<AccountId>,
}

impl<Name, Call, BlockNumber, PalletsOrigin, AccountId, Moment>
    Scheduled<Name, Call, BlockNumber, PalletsOrigin, AccountId, Moment>
where
    Call: Clone,
    PalletsOrigin: Clone,
{
    /// Create a new task to be used for retry attempts of the original one. The cloned task will
    /// have the same `priority`, `call` and `origin`, but will always be non-periodic and unnamed.
    pub fn as_retry(&self) -> Self {
        Self {
            maybe_id: None,
            priority: self.priority,
            call: self.call.clone(),
            maybe_periodic: None,
            origin: self.origin.clone(),
            _phantom: Default::default(),
        }
    }
}

pub type ScheduledOf<T> = Scheduled<
    TaskName,
    BoundedCallOf<T>,
    BlockNumberFor<T>,
    <T as Config>::PalletsOrigin,
    <T as frame_system::Config>::AccountId,
    <T as Config>::Moment,
>;

pub(crate) trait MarginalWeightInfo: WeightInfo {
    fn service_task(maybe_lookup_len: Option<usize>, named: bool, periodic: bool) -> Weight {
        let base = Self::service_task_base();
        let mut total = match maybe_lookup_len {
            None => base,
            Some(l) => Self::service_task_fetched(l as u32),
        };
        if named {
            total.saturating_accrue(Self::service_task_named().saturating_sub(base));
        }
        if periodic {
            total.saturating_accrue(Self::service_task_periodic().saturating_sub(base));
        }
        total
    }
}
impl<T: WeightInfo> MarginalWeightInfo for T {}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{dispatch::PostDispatchInfo, pallet_prelude::*};
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::{AtLeast32Bit, Scale};

    /// The in-code storage version.
    const STORAGE_VERSION: StorageVersion = StorageVersion::new(4);

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    /// `system::Config` should always be included in our implied traits.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The aggregated origin which the dispatch will take.
        type RuntimeOrigin: OriginTrait<PalletsOrigin = Self::PalletsOrigin>
            + From<Self::PalletsOrigin>
            + IsType<<Self as system::Config>::RuntimeOrigin>;

        /// The caller origin, overarching type of all pallets origins.
        type PalletsOrigin: From<system::RawOrigin<Self::AccountId>>
            + CallerTrait<Self::AccountId>
            + MaxEncodedLen;

        /// The aggregated call type.
        type RuntimeCall: Parameter
            + Dispatchable<
                RuntimeOrigin = <Self as Config>::RuntimeOrigin,
                PostInfo = PostDispatchInfo,
            > + GetDispatchInfo
            + From<system::Call<Self>>;

        /// The maximum weight that may be scheduled per block for any dispatchables.
        #[pallet::constant]
        type MaximumWeight: Get<Weight>;

        /// Required origin to schedule or cancel calls.
        type ScheduleOrigin: EnsureOrigin<<Self as system::Config>::RuntimeOrigin>;

        /// Compare the privileges of origins.
        ///
        /// This will be used when canceling a task, to ensure that the origin that tries
        /// to cancel has greater or equal privileges as the origin that created the scheduled task.
        ///
        /// For simplicity the [`EqualPrivilegeOnly`](frame_support::traits::EqualPrivilegeOnly) can
        /// be used. This will only check if two given origins are equal.
        type OriginPrivilegeCmp: PrivilegeCmp<Self::PalletsOrigin>;

        /// The maximum number of scheduled calls in the queue for a single block.
        ///
        /// NOTE:
        /// + Dependent pallets' benchmarks might require a higher limit for the setting. Set a
        /// higher limit under `runtime-benchmarks` feature.
        #[pallet::constant]
        type MaxScheduledPerBlock: Get<u32>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        /// The preimage provider with which we look up call hashes to get the call.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        /// Moment type
        type Moment: Saturating
            + Copy
            + Parameter
            + AtLeast32Bit
            + Scale<BlockNumberFor<Self>, Output = Self::Moment>
            + MaxEncodedLen
            + Default
            + sp_runtime::traits::Zero;

        /// Time provider, usually timestamp pallet.
        type TimeProvider: Time<Moment = Self::Moment>;

        /// Precision of the timestamp buckets.
        ///
        /// Timestamp based dispatches are rounded to the nearest bucket of this precision.
        #[pallet::constant]
        type TimestampBucketSize: Get<Self::Moment>;
    }

    /// Tracks incomplete block-based agendas that need to be processed in a later block.
    #[pallet::storage]
    pub type IncompleteBlockSince<T: Config> = StorageValue<_, BlockNumberFor<T>>;

    /// Tracks incomplete timestamp-based agendas that need to be processed in a later block.
    #[pallet::storage]
    pub type IncompleteTimestampSince<T: Config> = StorageValue<_, T::Moment>;

    /// Tracks the last timestamp bucket that was fully processed.
    /// Used to avoid reprocessing all buckets from 0 on every run.
    #[pallet::storage]
    pub type LastProcessedTimestamp<T: Config> = StorageValue<_, T::Moment>;

    /// Items to be executed, indexed by the block number that they should be executed on.
    #[pallet::storage]
    pub type Agenda<T: Config> = StorageMap<
        _,
        Twox64Concat,
        BlockNumberOrTimestampOf<T>,
        BoundedVec<Option<ScheduledOf<T>>, T::MaxScheduledPerBlock>,
        ValueQuery,
    >;

    /// Retry configurations for items to be executed, indexed by task address.
    #[pallet::storage]
    pub type Retries<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        TaskAddressOf<T>,
        RetryConfig<BlockNumberOrTimestampOf<T>>,
        OptionQuery,
    >;

    /// Lookup from a name to the block number and index of the task.
    ///
    /// For v3 -> v4 the previously unbounded identities are Blake2-256 hashed to form the v4
    /// identities.
    #[pallet::storage]
    pub(crate) type Lookup<T: Config> = StorageMap<_, Twox64Concat, TaskName, TaskAddressOf<T>>;

    /// Events type.
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Scheduled some task.
        Scheduled {
            when: BlockNumberOrTimestampOf<T>,
            index: u32,
        },
        /// Canceled some task.
        Canceled {
            when: BlockNumberOrTimestampOf<T>,
            index: u32,
        },
        /// Dispatched some task.
        Dispatched {
            task: TaskAddressOf<T>,
            id: Option<TaskName>,
            result: DispatchResult,
        },
        /// Set a retry configuration for some task.
        RetrySet {
            task: TaskAddressOf<T>,
            id: Option<TaskName>,
            period: BlockNumberOrTimestampOf<T>,
            retries: u8,
        },
        /// Cancel a retry configuration for some task.
        RetryCancelled {
            task: TaskAddressOf<T>,
            id: Option<TaskName>,
        },
        /// The call for the provided hash was not found so the task has been aborted.
        CallUnavailable {
            task: TaskAddressOf<T>,
            id: Option<TaskName>,
        },
        /// The given task was unable to be renewed since the agenda is full at that block.
        PeriodicFailed {
            task: TaskAddressOf<T>,
            id: Option<TaskName>,
        },
        /// The given task was unable to be retried since the agenda is full at that block or there
        /// was not enough weight to reschedule it.
        RetryFailed {
            task: TaskAddressOf<T>,
            id: Option<TaskName>,
        },
        /// The given task can never be executed since it is overweight.
        PermanentlyOverweight {
            task: TaskAddressOf<T>,
            id: Option<TaskName>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Failed to schedule a call
        FailedToSchedule,
        /// Cannot find the scheduled call.
        NotFound,
        /// Given target block number is in the past.
        TargetBlockNumberInPast,
        /// Given target timestamp is in the past.
        TargetTimestampInPast,
        /// Reschedule failed because it does not change scheduled time.
        RescheduleNoChange,
        /// Attempt to use a non-named function on a named task.
        Named,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Execute the scheduled calls
        fn on_initialize(now: BlockNumberFor<T>) -> Weight {
            let mut weight_counter = WeightMeter::with_limit(T::MaximumWeight::get());

            // Process block-based agendas
            Self::service_agendas(
                &mut weight_counter,
                BlockNumberOrTimestamp::BlockNumber(now),
                u32::max_value(),
            );

            // Process timestamp-based agendas using current system time
            // This ensures no buckets are skipped if block times are longer than bucket intervals
            let current_timestamp = T::TimeProvider::now();
            if current_timestamp > T::Moment::zero() {
                Self::service_agendas(
                    &mut weight_counter,
                    BlockNumberOrTimestamp::Timestamp(current_timestamp),
                    u32::max_value(),
                );
            }

            weight_counter.consumed()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Anonymously schedule a task.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::schedule(T::MaxScheduledPerBlock::get()))]
        pub fn schedule(
            origin: OriginFor<T>,
            when: BlockNumberFor<T>,
            maybe_periodic: Option<Period<BlockNumberFor<T>, T::Moment>>,
            priority: schedule::Priority,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            Self::do_schedule(
                DispatchTime::At(when),
                maybe_periodic,
                priority,
                origin.caller().clone(),
                T::Preimages::bound(*call)?,
            )?;
            Ok(())
        }

        /// Cancel an anonymously scheduled task.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel(T::MaxScheduledPerBlock::get()))]
        pub fn cancel(
            origin: OriginFor<T>,
            when: BlockNumberOrTimestampOf<T>,
            index: u32,
        ) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            Self::do_cancel(Some(origin.caller().clone()), (when, index))?;
            Ok(())
        }

        /// Schedule a named task.
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::schedule_named(T::MaxScheduledPerBlock::get()))]
        pub fn schedule_named(
            origin: OriginFor<T>,
            id: TaskName,
            when: BlockNumberFor<T>,
            maybe_periodic: Option<Period<BlockNumberFor<T>, T::Moment>>,
            priority: schedule::Priority,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            Self::do_schedule_named(
                id,
                DispatchTime::At(when),
                maybe_periodic,
                priority,
                origin.caller().clone(),
                T::Preimages::bound(*call)?,
            )?;
            Ok(())
        }

        /// Cancel a named scheduled task.
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel_named(T::MaxScheduledPerBlock::get()))]
        pub fn cancel_named(origin: OriginFor<T>, id: TaskName) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            Self::do_cancel_named(Some(origin.caller().clone()), id)?;
            Ok(())
        }

        /// Anonymously schedule a task after a delay.
        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::schedule(T::MaxScheduledPerBlock::get()))]
        pub fn schedule_after(
            origin: OriginFor<T>,
            after: BlockNumberOrTimestamp<BlockNumberFor<T>, T::Moment>,
            maybe_periodic: Option<Period<BlockNumberFor<T>, T::Moment>>,
            priority: schedule::Priority,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            Self::do_schedule(
                DispatchTime::After(after),
                maybe_periodic,
                priority,
                origin.caller().clone(),
                T::Preimages::bound(*call)?,
            )?;
            Ok(())
        }

        /// Schedule a named task after a delay.
        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::schedule_named(T::MaxScheduledPerBlock::get()))]
        pub fn schedule_named_after(
            origin: OriginFor<T>,
            id: TaskName,
            after: BlockNumberOrTimestamp<BlockNumberFor<T>, T::Moment>,
            maybe_periodic: Option<Period<BlockNumberFor<T>, T::Moment>>,
            priority: schedule::Priority,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            Self::do_schedule_named(
                id,
                DispatchTime::After(after),
                maybe_periodic,
                priority,
                origin.caller().clone(),
                T::Preimages::bound(*call)?,
            )?;
            Ok(())
        }

        /// Set a retry configuration for a task so that, in case its scheduled run fails, it will
        /// be retried after `period` blocks, for a total amount of `retries` retries or until it
        /// succeeds.
        ///
        /// Tasks which need to be scheduled for a retry are still subject to weight metering and
        /// agenda space, same as a regular task. If a periodic task fails, it will be scheduled
        /// normally while the task is retrying.
        ///
        /// Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic
        /// clones of the original task. Their retry configuration will be derived from the
        /// original task's configuration, but will have a lower value for `remaining` than the
        /// original `total_retries`.
        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::set_retry())]
        pub fn set_retry(
            origin: OriginFor<T>,
            task: TaskAddressOf<T>,
            retries: u8,
            period: BlockNumberOrTimestampOf<T>,
        ) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            let (when, index) = task;
            let agenda = Agenda::<T>::get(when);
            let scheduled = agenda
                .get(index as usize)
                .and_then(Option::as_ref)
                .ok_or(Error::<T>::NotFound)?;
            Self::ensure_privilege(origin.caller(), &scheduled.origin)?;
            Retries::<T>::insert(
                (when, index),
                RetryConfig {
                    total_retries: retries,
                    remaining: retries,
                    period,
                },
            );
            Self::deposit_event(Event::RetrySet {
                task,
                id: None,
                period,
                retries,
            });
            Ok(())
        }

        /// Set a retry configuration for a named task so that, in case its scheduled run fails, it
        /// will be retried after `period` blocks, for a total amount of `retries` retries or until
        /// it succeeds.
        ///
        /// Tasks which need to be scheduled for a retry are still subject to weight metering and
        /// agenda space, same as a regular task. If a periodic task fails, it will be scheduled
        /// normally while the task is retrying.
        ///
        /// Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic
        /// clones of the original task. Their retry configuration will be derived from the
        /// original task's configuration, but will have a lower value for `remaining` than the
        /// original `total_retries`.
        #[pallet::call_index(7)]
        #[pallet::weight(<T as Config>::WeightInfo::set_retry_named())]
        pub fn set_retry_named(
            origin: OriginFor<T>,
            id: TaskName,
            retries: u8,
            period: BlockNumberOrTimestampOf<T>,
        ) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            let (when, agenda_index) = Lookup::<T>::get(&id).ok_or(Error::<T>::NotFound)?;
            let agenda = Agenda::<T>::get(when);
            let scheduled = agenda
                .get(agenda_index as usize)
                .and_then(Option::as_ref)
                .ok_or(Error::<T>::NotFound)?;
            Self::ensure_privilege(origin.caller(), &scheduled.origin)?;
            Retries::<T>::insert(
                (when, agenda_index),
                RetryConfig {
                    total_retries: retries,
                    remaining: retries,
                    period,
                },
            );
            Self::deposit_event(Event::RetrySet {
                task: (when, agenda_index),
                id: Some(id),
                period,
                retries,
            });
            Ok(())
        }

        /// Removes the retry configuration of a task.
        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel_retry())]
        pub fn cancel_retry(origin: OriginFor<T>, task: TaskAddressOf<T>) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            Self::do_cancel_retry(origin.caller(), task)?;
            Self::deposit_event(Event::RetryCancelled { task, id: None });
            Ok(())
        }

        /// Cancel the retry configuration of a named task.
        #[pallet::call_index(9)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel_retry_named())]
        pub fn cancel_retry_named(origin: OriginFor<T>, id: TaskName) -> DispatchResult {
            T::ScheduleOrigin::ensure_origin(origin.clone())?;
            let origin = <T as Config>::RuntimeOrigin::from(origin);
            let task = Lookup::<T>::get(&id).ok_or(Error::<T>::NotFound)?;
            Self::do_cancel_retry(origin.caller(), task)?;
            Self::deposit_event(Event::RetryCancelled { task, id: Some(id) });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    fn resolve_time(
        when: DispatchTime<BlockNumberFor<T>, T::Moment>,
    ) -> Result<BlockNumberOrTimestampOf<T>, DispatchError> {
        let current_block = frame_system::Pallet::<T>::block_number();
        let now = T::TimeProvider::now();

        let when = match when {
            DispatchTime::At(x) => BlockNumberOrTimestamp::BlockNumber(x),
            // The current block has already completed it's scheduled tasks, so
            // Schedule the task at lest one block after this current block.
            DispatchTime::After(x) => {
                // get the median block time
                let res = match x {
                    BlockNumberOrTimestamp::BlockNumber(x) => BlockNumberOrTimestamp::BlockNumber(
                        current_block.saturating_add(x).saturating_add(One::one()),
                    ),
                    BlockNumberOrTimestamp::Timestamp(_) => {
                        x.normalize(T::TimestampBucketSize::get())
                    }
                };
                res
            }
        };

        match when {
            BlockNumberOrTimestamp::BlockNumber(x) => {
                ensure!(x > current_block, Error::<T>::TargetBlockNumberInPast);
            }
            BlockNumberOrTimestamp::Timestamp(x) => {
                // Ensure that the timestamp is in the future.
                ensure!(x > now, Error::<T>::TargetTimestampInPast);
            }
        };

        Ok(when)
    }

    fn place_task(
        when: BlockNumberOrTimestampOf<T>,
        what: ScheduledOf<T>,
    ) -> Result<TaskAddressOf<T>, (DispatchError, ScheduledOf<T>)> {
        let maybe_name = what.maybe_id;
        let index = Self::push_to_agenda(when, what)?;
        let address = (when, index);
        if let Some(name) = maybe_name {
            Lookup::<T>::insert(name, address)
        }
        Self::deposit_event(Event::Scheduled {
            when: address.0,
            index: address.1,
        });
        Ok(address)
    }

    fn push_to_agenda(
        when: BlockNumberOrTimestampOf<T>,
        what: ScheduledOf<T>,
    ) -> Result<u32, (DispatchError, ScheduledOf<T>)> {
        let mut agenda = Agenda::<T>::get(when);
        let index = if (agenda.len() as u32) < T::MaxScheduledPerBlock::get() {
            // will always succeed due to the above check.
            let _ = agenda.try_push(Some(what));
            agenda.len() as u32 - 1
        } else {
            if let Some(hole_index) = agenda.iter().position(|i| i.is_none()) {
                agenda[hole_index] = Some(what);
                hole_index as u32
            } else {
                return Err((DispatchError::Exhausted, what));
            }
        };
        Agenda::<T>::insert(when, agenda);
        Ok(index)
    }

    /// Remove trailing `None` items of an agenda at `when`. If all items are `None` remove the
    /// agenda record entirely.
    fn cleanup_agenda(when: BlockNumberOrTimestampOf<T>) {
        let mut agenda = Agenda::<T>::get(when);
        match agenda.iter().rposition(|i| i.is_some()) {
            Some(i) if agenda.len() > i + 1 => {
                agenda.truncate(i + 1);
                Agenda::<T>::insert(when, agenda);
            }
            Some(_) => {}
            None => {
                Agenda::<T>::remove(when);
            }
        }
    }

    fn do_schedule(
        when: DispatchTime<BlockNumberFor<T>, T::Moment>,
        maybe_periodic: Option<Period<BlockNumberFor<T>, T::Moment>>,
        priority: schedule::Priority,
        origin: T::PalletsOrigin,
        call: BoundedCallOf<T>,
    ) -> Result<TaskAddressOf<T>, DispatchError> {
        let when = Self::resolve_time(when)?;

        let lookup_hash = call.lookup_hash();

        // sanitize maybe_periodic
        let maybe_periodic = maybe_periodic
            .filter(|p| p.1 > 1 && !p.0.is_zero())
            // Remove one from the number of repetitions since we will schedule one now.
            .map(|(p, c)| (p, c - 1));
        let task = Scheduled {
            maybe_id: None,
            priority,
            call,
            maybe_periodic,
            origin,
            _phantom: PhantomData,
        };
        let res = Self::place_task(when, task).map_err(|x| x.0)?;

        if let Some(hash) = lookup_hash {
            // Request the call to be made available.
            T::Preimages::request(&hash);
        }

        Ok(res)
    }

    fn do_cancel(
        origin: Option<T::PalletsOrigin>,
        (when, index): TaskAddressOf<T>,
    ) -> Result<(), DispatchError> {
        let scheduled = Agenda::<T>::try_mutate(when, |agenda| {
            agenda.get_mut(index as usize).map_or(
                Ok(None),
                |s| -> Result<Option<Scheduled<_, _, _, _, _, _>>, DispatchError> {
                    if let (Some(ref o), Some(ref s)) = (origin, s.borrow()) {
                        Self::ensure_privilege(o, &s.origin)?;
                    };
                    Ok(s.take())
                },
            )
        })?;
        if let Some(s) = scheduled {
            T::Preimages::drop(&s.call);
            if let Some(id) = s.maybe_id {
                Lookup::<T>::remove(id);
            }
            Retries::<T>::remove((when, index));
            Self::cleanup_agenda(when);
            Self::deposit_event(Event::Canceled { when, index });
            Ok(())
        } else {
            return Err(Error::<T>::NotFound.into());
        }
    }

    fn do_reschedule(
        (when, index): TaskAddressOf<T>,
        new_time: DispatchTime<BlockNumberFor<T>, T::Moment>,
    ) -> Result<TaskAddressOf<T>, DispatchError> {
        let new_time = Self::resolve_time(new_time)?;

        if new_time == when {
            return Err(Error::<T>::RescheduleNoChange.into());
        }

        let task = Agenda::<T>::try_mutate(when, |agenda| {
            let task = agenda.get_mut(index as usize).ok_or(Error::<T>::NotFound)?;
            ensure!(
                !matches!(
                    task,
                    Some(Scheduled {
                        maybe_id: Some(_),
                        ..
                    })
                ),
                Error::<T>::Named
            );
            task.take().ok_or(Error::<T>::NotFound)
        })?;
        Self::cleanup_agenda(when);
        Self::deposit_event(Event::Canceled { when, index });

        Self::place_task(new_time, task).map_err(|x| x.0)
    }

    fn do_schedule_named(
        id: TaskName,
        when: DispatchTime<BlockNumberFor<T>, T::Moment>,
        maybe_periodic: Option<Period<BlockNumberFor<T>, T::Moment>>,
        priority: schedule::Priority,
        origin: T::PalletsOrigin,
        call: BoundedCallOf<T>,
    ) -> Result<TaskAddressOf<T>, DispatchError> {
        // ensure id it is unique
        if Lookup::<T>::contains_key(&id) {
            return Err(Error::<T>::FailedToSchedule.into());
        }

        let when = Self::resolve_time(when)?;

        let lookup_hash = call.lookup_hash();

        // sanitize maybe_periodic
        let maybe_periodic = maybe_periodic
            .filter(|p| p.1 > 1 && !p.0.is_zero())
            // Remove one from the number of repetitions since we will schedule one now.
            .map(|(p, c)| (p, c - 1));

        let task = Scheduled {
            maybe_id: Some(id),
            priority,
            call,
            maybe_periodic,
            origin,
            _phantom: Default::default(),
        };
        let res = Self::place_task(when, task).map_err(|x| x.0)?;

        if let Some(hash) = lookup_hash {
            // Request the call to be made available.
            T::Preimages::request(&hash);
        }

        Ok(res)
    }

    fn do_cancel_named(origin: Option<T::PalletsOrigin>, id: TaskName) -> DispatchResult {
        Lookup::<T>::try_mutate_exists(id, |lookup| -> DispatchResult {
            if let Some((when, index)) = lookup.take() {
                let i = index as usize;
                Agenda::<T>::try_mutate(when, |agenda| -> DispatchResult {
                    if let Some(s) = agenda.get_mut(i) {
                        if let (Some(ref o), Some(ref s)) = (origin, s.borrow()) {
                            Self::ensure_privilege(o, &s.origin)?;
                            Retries::<T>::remove((when, index));
                            T::Preimages::drop(&s.call);
                        }
                        *s = None;
                    }
                    Ok(())
                })?;
                Self::cleanup_agenda(when);
                Self::deposit_event(Event::Canceled { when, index });
                Ok(())
            } else {
                return Err(Error::<T>::NotFound.into());
            }
        })
    }

    fn do_reschedule_named(
        id: TaskName,
        new_time: DispatchTime<BlockNumberFor<T>, T::Moment>,
    ) -> Result<TaskAddressOf<T>, DispatchError> {
        let new_time = Self::resolve_time(new_time)?;

        let lookup = Lookup::<T>::get(id);
        let (when, index) = lookup.ok_or(Error::<T>::NotFound)?;

        if new_time == when {
            return Err(Error::<T>::RescheduleNoChange.into());
        }

        let task = Agenda::<T>::try_mutate(when, |agenda| {
            let task = agenda.get_mut(index as usize).ok_or(Error::<T>::NotFound)?;
            task.take().ok_or(Error::<T>::NotFound)
        })?;
        Self::cleanup_agenda(when);
        Self::deposit_event(Event::Canceled { when, index });
        Self::place_task(new_time, task).map_err(|x| x.0)
    }

    fn do_cancel_retry(
        origin: &T::PalletsOrigin,
        (when, index): TaskAddressOf<T>,
    ) -> Result<(), DispatchError> {
        let agenda = Agenda::<T>::get(when);
        let scheduled = agenda
            .get(index as usize)
            .and_then(Option::as_ref)
            .ok_or(Error::<T>::NotFound)?;
        Self::ensure_privilege(origin, &scheduled.origin)?;
        Retries::<T>::remove((when, index));
        Ok(())
    }
}

enum ServiceTaskError {
    /// Could not be executed due to missing preimage.
    Unavailable,
    /// Could not be executed due to weight limitations.
    Overweight,
}
use ServiceTaskError::*;

impl<T: Config> Pallet<T> {
    /// Service up to `max` agendas queue starting from earliest incompletely executed agenda.
    fn service_agendas(weight: &mut WeightMeter, now: BlockNumberOrTimestampOf<T>, max: u32) {
        if weight
            .try_consume(T::WeightInfo::service_agendas_base())
            .is_err()
        {
            return;
        }

        let normalized_now = now.normalize(T::TimestampBucketSize::get());

        // Use separate incomplete tracking based on agenda type
        let (mut when, mut incomplete_since) = match normalized_now {
            BlockNumberOrTimestamp::BlockNumber(current_block) => {
                let next_block = current_block.saturating_add(One::one());
                let start_block = IncompleteBlockSince::<T>::take().unwrap_or(current_block);
                (
                    BlockNumberOrTimestamp::BlockNumber(start_block),
                    BlockNumberOrTimestamp::BlockNumber(next_block),
                )
            }
            BlockNumberOrTimestamp::Timestamp(current_time) => {
                let next_bucket = current_time.saturating_add(T::TimestampBucketSize::get());

                // Start from incomplete timestamp if exists, otherwise from last processed timestamp
                let start_time = if let Some(incomplete) = IncompleteTimestampSince::<T>::take() {
                    incomplete
                } else {
                    // Use last processed timestamp, but for safety, always check from the start
                    // on the very first processing cycle to ensure no tasks are missed
                    LastProcessedTimestamp::<T>::get().unwrap_or(T::Moment::zero())
                };

                (
                    BlockNumberOrTimestamp::Timestamp(start_time),
                    BlockNumberOrTimestamp::Timestamp(next_bucket),
                )
            }
        };

        let mut executed = 0;

        let max_items = T::MaxScheduledPerBlock::get();
        let mut count_down = max;
        let service_agenda_base_weight = T::WeightInfo::service_agenda_base(max_items);
        while count_down > 0
            && when <= normalized_now
            && weight.can_consume(service_agenda_base_weight)
        {
            if !Self::service_agenda(
                weight,
                &mut executed,
                normalized_now,
                when,
                u32::max_value(),
            ) {
                incomplete_since = incomplete_since.min(when);
            }
            match when {
                BlockNumberOrTimestamp::BlockNumber(x) => {
                    when = BlockNumberOrTimestamp::BlockNumber(x.saturating_add(One::one()));
                }
                BlockNumberOrTimestamp::Timestamp(x) => {
                    when = BlockNumberOrTimestamp::Timestamp(
                        x.saturating_add(T::TimestampBucketSize::get()),
                    );
                }
            };

            count_down.saturating_dec();
        }

        // Store incomplete since in the appropriate storage
        incomplete_since = incomplete_since.min(when);

        if incomplete_since <= normalized_now {
            match incomplete_since {
                BlockNumberOrTimestamp::BlockNumber(block) => {
                    IncompleteBlockSince::<T>::put(block);
                }
                BlockNumberOrTimestamp::Timestamp(time) => {
                    IncompleteTimestampSince::<T>::put(time);
                }
            }
        } else {
            // If we completed all processing up to normalized_now, update last processed timestamp
            match normalized_now {
                BlockNumberOrTimestamp::Timestamp(time) => {
                    LastProcessedTimestamp::<T>::put(time);
                }
                _ => {} // Block processing doesn't need this tracking
            }
        }
    }

    /// Returns `true` if the agenda was fully completed, `false` if it should be revisited at a
    /// later block.
    fn service_agenda(
        weight: &mut WeightMeter,
        executed: &mut u32,
        now: BlockNumberOrTimestampOf<T>,
        when: BlockNumberOrTimestampOf<T>,
        max: u32,
    ) -> bool {
        let mut agenda = Agenda::<T>::get(when);
        let mut ordered = agenda
            .iter()
            .enumerate()
            .filter_map(|(index, maybe_item)| {
                maybe_item
                    .as_ref()
                    .map(|item| (index as u32, item.priority))
            })
            .collect::<Vec<_>>();
        ordered.sort_by_key(|k| k.1);
        let within_limit = weight
            .try_consume(T::WeightInfo::service_agenda_base(ordered.len() as u32))
            .is_ok();
        debug_assert!(
            within_limit,
            "weight limit should have been checked in advance"
        );

        // Items which we know can be executed and have postponed for execution in a later block.
        let mut postponed = (ordered.len() as u32).saturating_sub(max);
        // Items which we don't know can ever be executed.
        let mut dropped = 0;

        for (agenda_index, _) in ordered.into_iter().take(max as usize) {
            let task = match agenda[agenda_index as usize].take() {
                None => continue,
                Some(t) => t,
            };
            let base_weight = T::WeightInfo::service_task(
                task.call.lookup_len().map(|x| x as usize),
                task.maybe_id.is_some(),
                task.maybe_periodic.is_some(),
            );
            if !weight.can_consume(base_weight) {
                postponed += 1;
                break;
            }
            let result = Self::service_task(weight, now, when, agenda_index, *executed == 0, task);
            agenda[agenda_index as usize] = match result {
                Err((Unavailable, slot)) => {
                    dropped += 1;
                    slot
                }
                Err((Overweight, slot)) => {
                    postponed += 1;
                    slot
                }
                Ok(()) => {
                    *executed += 1;
                    None
                }
            };
        }
        if postponed > 0 || dropped > 0 {
            Agenda::<T>::insert(when, agenda);
        } else {
            Agenda::<T>::remove(when);
        }

        postponed == 0
    }

    /// Service (i.e. execute) the given task, being careful not to overflow the `weight` counter.
    ///
    /// This involves:
    /// - removing and potentially replacing the `Lookup` entry for the task.
    /// - realizing the task's call which can include a preimage lookup.
    /// - Rescheduling the task for execution in a later agenda if periodic.
    fn service_task(
        weight: &mut WeightMeter,
        now: BlockNumberOrTimestampOf<T>,
        when: BlockNumberOrTimestampOf<T>,
        agenda_index: u32,
        is_first: bool,
        mut task: ScheduledOf<T>,
    ) -> Result<(), (ServiceTaskError, Option<ScheduledOf<T>>)> {
        if let Some(ref id) = task.maybe_id {
            Lookup::<T>::remove(id);
        }

        let (call, lookup_len) = match T::Preimages::peek(&task.call) {
            Ok(c) => c,
            Err(_) => {
                Self::deposit_event(Event::CallUnavailable {
                    task: (when, agenda_index),
                    id: task.maybe_id,
                });

                // It was not available when we needed it, so we don't need to have requested it
                // anymore.
                T::Preimages::drop(&task.call);

                // We don't know why `peek` failed, thus we most account here for the "full weight".
                let _ = weight.try_consume(T::WeightInfo::service_task(
                    task.call.lookup_len().map(|x| x as usize),
                    task.maybe_id.is_some(),
                    task.maybe_periodic.is_some(),
                ));

                return Err((Unavailable, Some(task)));
            }
        };

        let _ = weight.try_consume(T::WeightInfo::service_task(
            lookup_len.map(|x| x as usize),
            task.maybe_id.is_some(),
            task.maybe_periodic.is_some(),
        ));

        match Self::execute_dispatch(weight, task.origin.clone(), call) {
            Err(()) if is_first => {
                T::Preimages::drop(&task.call);
                Self::deposit_event(Event::PermanentlyOverweight {
                    task: (when, agenda_index),
                    id: task.maybe_id,
                });
                Err((Unavailable, Some(task)))
            }
            Err(()) => Err((Overweight, Some(task))),
            Ok(result) => {
                let failed = result.is_err();
                let maybe_retry_config = Retries::<T>::take((when, agenda_index));
                Self::deposit_event(Event::Dispatched {
                    task: (when, agenda_index),
                    id: task.maybe_id,
                    result,
                });

                match maybe_retry_config {
                    Some(retry_config) if failed => {
                        Self::schedule_retry(weight, now, when, agenda_index, &task, retry_config);
                    }
                    _ => {}
                }

                if let &Some((period, count)) = &task.maybe_periodic {
                    if count > 1 {
                        task.maybe_periodic = Some((period, count - 1));
                    } else {
                        task.maybe_periodic = None;
                    }
                    let wake = now.saturating_add(&period);
                    if let Ok(wake) = wake {
                        match Self::place_task(wake, task) {
                            Ok(new_address) => {
                                if let Some(retry_config) = maybe_retry_config {
                                    Retries::<T>::insert(new_address, retry_config);
                                }
                            }
                            Err((_, task)) => {
                                // TODO: Leave task in storage somewhere for it to be rescheduled
                                // manually.
                                T::Preimages::drop(&task.call);
                                Self::deposit_event(Event::PeriodicFailed {
                                    task: (when, agenda_index),
                                    id: task.maybe_id,
                                });
                            }
                        }
                    }
                } else {
                    T::Preimages::drop(&task.call);
                }
                Ok(())
            }
        }
    }

    /// Make a dispatch to the given `call` from the given `origin`, ensuring that the `weight`
    /// counter does not exceed its limit and that it is counted accurately (e.g. accounted using
    /// post info if available).
    ///
    /// NOTE: Only the weight for this function will be counted (origin lookup, dispatch and the
    /// call itself).
    ///
    /// Returns an error if the call is overweight.
    fn execute_dispatch(
        weight: &mut WeightMeter,
        origin: T::PalletsOrigin,
        call: <T as Config>::RuntimeCall,
    ) -> Result<DispatchResult, ()> {
        let base_weight = match origin.as_system_ref() {
            Some(&RawOrigin::Signed(_)) => T::WeightInfo::execute_dispatch_signed(),
            _ => T::WeightInfo::execute_dispatch_unsigned(),
        };
        let call_weight = call.get_dispatch_info().call_weight;
        // We only allow a scheduled call if it cannot push the weight past the limit.
        let max_weight = base_weight.saturating_add(call_weight);

        if !weight.can_consume(max_weight) {
            return Err(());
        }

        let dispatch_origin = origin.into();
        let (maybe_actual_call_weight, result) = match call.dispatch(dispatch_origin) {
            Ok(post_info) => (post_info.actual_weight, Ok(())),
            Err(error_and_info) => (
                error_and_info.post_info.actual_weight,
                Err(error_and_info.error),
            ),
        };
        let call_weight = maybe_actual_call_weight.unwrap_or(call_weight);
        let _ = weight.try_consume(base_weight);
        let _ = weight.try_consume(call_weight);
        Ok(result)
    }

    /// Check if a task has a retry configuration in place and, if so, try to reschedule it.
    ///
    /// Possible causes for failure to schedule a retry for a task:
    /// - there wasn't enough weight to run the task reschedule logic
    /// - there was no retry configuration in place
    /// - there were no more retry attempts left
    /// - the agenda was full.
    fn schedule_retry(
        weight: &mut WeightMeter,
        now: BlockNumberOrTimestampOf<T>,
        when: BlockNumberOrTimestampOf<T>,
        agenda_index: u32,
        task: &ScheduledOf<T>,
        retry_config: RetryConfig<BlockNumberOrTimestampOf<T>>,
    ) {
        if weight
            .try_consume(T::WeightInfo::schedule_retry(T::MaxScheduledPerBlock::get()))
            .is_err()
        {
            Self::deposit_event(Event::RetryFailed {
                task: (when, agenda_index),
                id: task.maybe_id,
            });
            return;
        }

        let RetryConfig {
            total_retries,
            mut remaining,
            period,
        } = retry_config;
        remaining = match remaining.checked_sub(1) {
            Some(n) => n,
            None => return,
        };
        let wake = now.saturating_add(&period);
        if let Ok(wake) = wake {
            match Self::place_task(wake, task.as_retry()) {
                Ok(address) => {
                    // Reinsert the retry config to the new address of the task after it was
                    // placed.
                    Retries::<T>::insert(
                        address,
                        RetryConfig {
                            total_retries,
                            remaining,
                            period,
                        },
                    );
                }
                Err((_, task)) => {
                    // TODO: Leave task in storage somewhere for it to be
                    // rescheduled manually.
                    T::Preimages::drop(&task.call);
                    Self::deposit_event(Event::RetryFailed {
                        task: (when, agenda_index),
                        id: task.maybe_id,
                    });
                }
            }
        }
    }

    /// Ensure that `left` has at least the same level of privilege or higher than `right`.
    ///
    /// Returns an error if `left` has a lower level of privilege or the two cannot be compared.
    fn ensure_privilege(
        left: &<T as Config>::PalletsOrigin,
        right: &<T as Config>::PalletsOrigin,
    ) -> Result<(), DispatchError> {
        if matches!(
            T::OriginPrivilegeCmp::cmp_privilege(left, right),
            Some(Ordering::Less) | None
        ) {
            return Err(BadOrigin.into());
        }
        Ok(())
    }
}

impl<T: Config> schedule::v3::Anon<BlockNumberFor<T>, <T as Config>::RuntimeCall, T::PalletsOrigin>
    for Pallet<T>
{
    type Address = TaskAddressOf<T>;
    type Hasher = T::Hashing;

    fn schedule(
        when: DispatchBlock<BlockNumberFor<T>>,
        maybe_periodic: Option<schedule::Period<BlockNumberFor<T>>>,
        priority: schedule::Priority,
        origin: T::PalletsOrigin,
        call: BoundedCallOf<T>,
    ) -> Result<Self::Address, DispatchError> {
        let periodic = if let Some(periodic) = maybe_periodic {
            Some((BlockNumberOrTimestamp::BlockNumber(periodic.0), periodic.1))
        } else {
            None
        };

        Self::do_schedule(when.into(), periodic, priority, origin, call)
    }

    fn cancel((when, index): Self::Address) -> Result<(), DispatchError> {
        Self::do_cancel(None, (when, index)).map_err(map_err_to_v3_err::<T>)
    }

    fn reschedule(
        address: Self::Address,
        when: DispatchBlock<BlockNumberFor<T>>,
    ) -> Result<Self::Address, DispatchError> {
        Self::do_reschedule(address, when.into()).map_err(map_err_to_v3_err::<T>)
    }

    fn next_dispatch_time(
        (when, index): Self::Address,
    ) -> Result<BlockNumberFor<T>, DispatchError> {
        Agenda::<T>::get(when)
            .get(index as usize)
            .ok_or(DispatchError::Unavailable)
            .map(|_| when.as_block_number())
            .and_then(|x| x.ok_or(DispatchError::Unavailable))
    }
}

use schedule::v3::TaskName;

impl<T: Config> schedule::v3::Named<BlockNumberFor<T>, <T as Config>::RuntimeCall, T::PalletsOrigin>
    for Pallet<T>
{
    type Address = TaskAddressOf<T>;
    type Hasher = T::Hashing;

    fn schedule_named(
        id: TaskName,
        when: DispatchBlock<BlockNumberFor<T>>,
        maybe_periodic: Option<schedule::Period<BlockNumberFor<T>>>,
        priority: schedule::Priority,
        origin: T::PalletsOrigin,
        call: BoundedCallOf<T>,
    ) -> Result<Self::Address, DispatchError> {
        let periodic = if let Some(periodic) = maybe_periodic {
            Some((BlockNumberOrTimestamp::BlockNumber(periodic.0), periodic.1))
        } else {
            None
        };
        Self::do_schedule_named(id, when.into(), periodic, priority, origin, call)
    }

    fn cancel_named(id: TaskName) -> Result<(), DispatchError> {
        Self::do_cancel_named(None, id).map_err(map_err_to_v3_err::<T>)
    }

    fn reschedule_named(
        id: TaskName,
        when: DispatchBlock<BlockNumberFor<T>>,
    ) -> Result<Self::Address, DispatchError> {
        Self::do_reschedule_named(id, when.into()).map_err(map_err_to_v3_err::<T>)
    }

    fn next_dispatch_time(id: TaskName) -> Result<BlockNumberFor<T>, DispatchError> {
        Lookup::<T>::get(id)
            .and_then(|(when, index)| {
                Agenda::<T>::get(when)
                    .get(index as usize)
                    .map(|_| when.as_block_number())
            })
            .ok_or(DispatchError::Unavailable)
            .and_then(|x| x.ok_or(DispatchError::Unavailable))
    }
}

impl<T: Config>
    ScheduleNamed<
        BlockNumberFor<T>,
        <T as Config>::Moment,
        <T as Config>::RuntimeCall,
        T::PalletsOrigin,
    > for Pallet<T>
{
    type Address = TaskAddressOf<T>;
    type Hasher = T::Hashing;
    fn schedule_named(
        id: TaskName,
        when: DispatchTime<BlockNumberFor<T>, T::Moment>,
        maybe_periodic: Option<Period<BlockNumberFor<T>, T::Moment>>,
        priority: schedule::Priority,
        origin: T::PalletsOrigin,
        call: BoundedCallOf<T>,
    ) -> Result<Self::Address, DispatchError> {
        Self::do_schedule_named(id, when, maybe_periodic, priority, origin, call)
    }

    fn cancel_named(id: TaskName) -> Result<(), DispatchError> {
        Self::do_cancel_named(None, id).map_err(map_err_to_v3_err::<T>)
    }

    fn reschedule_named(
        id: TaskName,
        when: DispatchTime<BlockNumberFor<T>, T::Moment>,
    ) -> Result<Self::Address, DispatchError> {
        Self::do_reschedule_named(id, when).map_err(map_err_to_v3_err::<T>)
    }

    fn next_dispatch_time(id: TaskName) -> Result<BlockNumberFor<T>, DispatchError> {
        Lookup::<T>::get(id)
            .ok_or(DispatchError::Unavailable)
            .and_then(|(when, index)| {
                Agenda::<T>::get(when)
                    .get(index as usize)
                    .ok_or(DispatchError::Unavailable)
                    .and_then(|_| when.as_block_number().ok_or(DispatchError::Unavailable))
            })
    }
}

/// Maps a pallet error to an `schedule::v3` error.
fn map_err_to_v3_err<T: Config>(err: DispatchError) -> DispatchError {
    if err == DispatchError::from(Error::<T>::NotFound) {
        DispatchError::Unavailable
    } else {
        err
    }
}
