//! # Reversibility Core Pallet
//!
//! Provides the core logic for scheduling and cancelling reversible transactions.
//! It manages the state of accounts opting into reversibility and the pending
//! transactions associated with them. Transaction interception is handled
//! separately via a `SignedExtension`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::WeightInfo;

use alloc::vec::Vec;
use frame_support::pallet_prelude::*;
use frame_support::traits::tokens::{Fortitude, Restriction};
use frame_system::pallet_prelude::*;
use qp_common::scheduler::{BlockNumberOrTimestamp, DispatchTime, ScheduleNamed};
use sp_runtime::traits::StaticLookup;

/// Type alias for this config's `BlockNumberOrTimestamp`.
pub type BlockNumberOrTimestampOf<T> =
    BlockNumberOrTimestamp<BlockNumberFor<T>, <T as Config>::Moment>;

/// How to delay transactions
/// - `Explicit`: Only delay transactions explicitly using this pallet's `schedule_transfer` extrinsic.
/// - `Intercept`: Intercept and delay transactions at the `TransactionExtension` level.
///
/// For example, for a reversible account with `DelayPolicy::Intercept`, the transaction
/// will be delayed even if the user doesn't explicitly call `schedule_transfer`. And for `DelayPolicy::Explicit`,
/// the transaction will be delayed only if the user explicitly calls this pallet's `schedule_transfer` extrinsic.
#[derive(Encode, Decode, MaxEncodedLen, Clone, Default, TypeInfo, Debug, PartialEq, Eq)]
pub enum DelayPolicy {
    /// Only explicitly delay transactions using `schedule_transfer` call
    #[default]
    Explicit,
    /// Intercept and delay transactions at `TransactionExtension` level. This is not UX friendly
    /// since it will return `TransactionValidityError` to the caller, but will still manage
    /// to delay the transaction.
    ///
    /// This is an opt-in feature and will not be enabled by default.
    Intercept,
}

/// Reversible account details
#[derive(Encode, Decode, MaxEncodedLen, Clone, Default, TypeInfo, Debug, PartialEq, Eq)]
pub struct ReversibleAccountData<AccountId, Delay> {
    /// The account that can reverse the transaction. `None` means the account itself.
    pub explicit_reverser: Option<AccountId>,
    /// The delay period for the account
    pub delay: Delay,
    /// The policy for the account
    pub policy: DelayPolicy,
}

/// Pending transfer details
#[derive(Encode, Decode, MaxEncodedLen, Clone, Default, TypeInfo, Debug, PartialEq, Eq)]
pub struct PendingTransfer<AccountId, Balance, Call> {
    /// The account that scheduled the transaction
    pub who: AccountId,
    /// The call
    pub call: Call,
    /// Amount frozen for the transaction
    pub amount: Balance,
    /// Count of this pending transaction for the account. Used to track number of identical
    /// transactions scheduled by the same account.
    pub count: u32,
}

/// Balance type
type BalanceOf<T> = <T as pallet_balances::Config>::Balance;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use crate::BlockNumberOrTimestampOf;
    use frame_support::dispatch::PostDispatchInfo;
    use frame_support::traits::fungible::MutateHold;
    use frame_support::traits::tokens::Precision;
    use frame_support::traits::{Bounded, CallerTrait, QueryPreimage, StorePreimage, Time};
    use frame_support::{traits::schedule::v3::TaskName, PalletId};
    use sp_runtime::traits::{AccountIdConversion, AtLeast32Bit};
    use sp_runtime::traits::{BlockNumberProvider, Dispatchable};
    use sp_runtime::traits::{Hash, Scale};
    use sp_runtime::Saturating;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config:
        frame_system::Config<
            RuntimeCall: From<pallet_balances::Call<Self>>
                             + From<Call<Self>>
                             + Dispatchable<PostInfo = PostDispatchInfo>,
        > + pallet_balances::Config<RuntimeHoldReason = <Self as Config>::RuntimeHoldReason>
    {
        /// The overarching runtime event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Scheduler for the runtime. We use the Named scheduler for cancellability.
        type Scheduler: ScheduleNamed<
            BlockNumberFor<Self>,
            Self::Moment,
            Self::RuntimeCall,
            Self::SchedulerOrigin,
            Hasher = Self::Hashing,
        >;

        /// Scheduler origin
        type SchedulerOrigin: From<frame_system::RawOrigin<Self::AccountId>>
            + CallerTrait<Self::AccountId>
            + MaxEncodedLen;

        /// Block number provider for scheduling.
        type BlockNumberProvider: BlockNumberProvider<BlockNumber = BlockNumberFor<Self>>;

        /// Maximum pending reversible transactions allowed per account. Used for BoundedVec.
        #[pallet::constant]
        type MaxPendingPerAccount: Get<u32>;

        /// The default delay period for reversible transactions if none is specified.
        #[pallet::constant]
        type DefaultDelay: Get<BlockNumberOrTimestampOf<Self>>;

        /// The minimum delay period allowed for reversible transactions.
        #[pallet::constant]
        type MinDelayPeriod: Get<BlockNumberOrTimestampOf<Self>>;

        /// Pallet Id
        type PalletId: Get<PalletId>;

        /// The preimage provider with which we look up call hashes to get the call.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        /// A type representing the weights required by the dispatchables of this pallet.
        type WeightInfo: WeightInfo;

        /// Hold reason for the reversible transactions.
        type RuntimeHoldReason: From<HoldReason>;

        /// Moment type for scheduling.
        type Moment: Saturating
            + Copy
            + Parameter
            + AtLeast32Bit
            + Scale<BlockNumberFor<Self>, Output = Self::Moment>
            + MaxEncodedLen;

        /// Time provider for scheduling.
        type TimeProvider: Time<Moment = Self::Moment>;
    }

    /// Maps accounts to their chosen reversibility delay period (in milliseconds).
    /// Accounts present in this map have reversibility enabled.
    #[pallet::storage]
    #[pallet::getter(fn reversible_accounts)]
    pub type ReversibleAccounts<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        ReversibleAccountData<T::AccountId, BlockNumberOrTimestampOf<T>>,
        OptionQuery,
    >;

    /// Stores the details of pending transactions scheduled for delayed execution.
    /// Keyed by the unique transaction ID.
    #[pallet::storage]
    #[pallet::getter(fn pending_dispatches)]
    pub type PendingTransfers<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::Hash,
        PendingTransfer<T::AccountId, BalanceOf<T>, Bounded<T::RuntimeCall, T::Hashing>>,
        OptionQuery,
    >;

    /// Indexes pending transaction IDs per account for efficient lookup and cancellation.
    /// Also enforces the maximum pending transactions limit per account.
    #[pallet::storage]
    #[pallet::getter(fn account_pending_index)]
    pub type AccountPendingIndex<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A user has enabled or updated their reversibility settings.
        /// [who, maybe_delay: None means disabled]
        ReversibilitySet {
            who: T::AccountId,
            data: ReversibleAccountData<T::AccountId, BlockNumberOrTimestampOf<T>>,
        },
        /// A transaction has been intercepted and scheduled for delayed execution.
        /// [who, tx_id, execute_at_moment]
        TransactionScheduled {
            who: T::AccountId,
            tx_id: T::Hash,
            execute_at: DispatchTime<BlockNumberFor<T>, T::Moment>,
        },
        /// A scheduled transaction has been successfully cancelled by the owner.
        /// [who, tx_id]
        TransactionCancelled { who: T::AccountId, tx_id: T::Hash },
        /// A scheduled transaction was executed by the scheduler.
        /// [tx_id, dispatch_result]
        TransactionExecuted {
            tx_id: T::Hash,
            result: DispatchResultWithPostInfo,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The account attempting to enable reversibility is already marked as reversible.
        AccountAlreadyReversible,
        /// The account attempting the action is not marked as reversible.
        AccountNotReversible,
        /// Reverser can not be the account itself, because it is redundant.
        ExplicitReverserCanNotBeSelf,
        /// The specified pending transaction ID was not found.
        PendingTxNotFound,
        /// The caller is not the original submitter of the transaction they are trying to cancel.
        NotOwner,
        /// The account has reached the maximum number of pending reversible transactions.
        TooManyPendingTransactions,
        /// The specified delay period is below the configured minimum.
        DelayTooShort,
        /// Failed to schedule the transaction execution with the scheduler pallet.
        SchedulingFailed,
        /// Failed to cancel the scheduled task with the scheduler pallet.
        CancellationFailed,
        /// Failed to decode the OpaqueCall back into a RuntimeCall.
        CallDecodingFailed,
        /// Call is invalid.
        InvalidCall,
        /// Invalid scheduler origin
        InvalidSchedulerOrigin,
        /// Reverser is invalid
        InvalidReverser,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T>
    where
        T: pallet_balances::Config<RuntimeHoldReason = <T as Config>::RuntimeHoldReason>,
    {
        /// Enable reversibility for the calling account with a specified delay, or disable it.
        ///
        /// - `delay`: The time (in milliseconds) after submission before the transaction executes.
        ///   If `None`, reversibility is disabled for the account.
        ///   If `Some`, must be >= `MinDelayPeriod`.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::set_reversibility())]
        pub fn set_reversibility(
            origin: OriginFor<T>,
            delay: Option<BlockNumberOrTimestampOf<T>>,
            policy: DelayPolicy,
            reverser: Option<T::AccountId>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(
                reverser != Some(who.clone()),
                Error::<T>::ExplicitReverserCanNotBeSelf
            );
            ensure!(
                !ReversibleAccounts::<T>::contains_key(&who),
                Error::<T>::AccountAlreadyReversible
            );
            ensure!(
                reverser != Some(who.clone()),
                Error::<T>::ExplicitReverserCanNotBeSelf
            );

            let delay = delay.unwrap_or(T::DefaultDelay::get());

            ensure!(delay >= T::MinDelayPeriod::get(), Error::<T>::DelayTooShort);

            let reversible_account_data = ReversibleAccountData {
                explicit_reverser: reverser,
                delay,
                policy: policy.clone(),
            };

            ReversibleAccounts::<T>::insert(&who, reversible_account_data.clone());
            Self::deposit_event(Event::ReversibilitySet {
                who,
                data: reversible_account_data,
            });

            Ok(())
        }

        /// Cancel a pending reversible transaction scheduled by the caller.
        ///
        /// - `tx_id`: The unique identifier of the transaction to cancel.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel())]
        pub fn cancel(origin: OriginFor<T>, tx_id: T::Hash) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::cancel_transfer(&who, tx_id)
        }

        /// Called by the Scheduler to finalize the scheduled task/call
        ///
        /// - `tx_id`: The unique id of the transaction to finalize and dispatch.
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::execute_transfer())]
        pub fn execute_transfer(
            origin: OriginFor<T>,
            tx_id: T::Hash,
        ) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin)?;

            ensure!(
                who == Self::account_id(),
                Error::<T>::InvalidSchedulerOrigin
            );

            Self::do_execute_transfer(&tx_id)
        }

        /// Schedule a transaction for delayed execution.
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::schedule_transfer())]
        pub fn schedule_transfer(
            origin: OriginFor<T>,
            dest: <<T as frame_system::Config>::Lookup as StaticLookup>::Source,
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            Self::do_schedule_transfer(origin, dest, amount)
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            assert!(
                !T::MinDelayPeriod::get().is_zero(),
                "`T::MinDelayPeriod` must be greater than 0"
            );
            assert!(
                T::MinDelayPeriod::get() <= T::DefaultDelay::get(),
                "`T::MinDelayPeriod` must be less or equal to `T::DefaultDelay`"
            );
        }
    }

    /// A reason for holding funds.
    #[pallet::composite_enum]
    pub enum HoldReason {
        /// Scheduled transfer amount.
        #[codec(index = 0)]
        ScheduledTransfer,
    }

    impl<T: Config> Pallet<T>
    where
        T: pallet_balances::Config<RuntimeHoldReason = <T as Config>::RuntimeHoldReason>,
    {
        /// Check if an account has reversibility enabled and return its delay.
        pub fn is_reversible(
            who: &T::AccountId,
        ) -> Option<ReversibleAccountData<T::AccountId, BlockNumberOrTimestampOf<T>>> {
            ReversibleAccounts::<T>::get(who)
        }

        // Pallet account as origin
        pub fn account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        fn do_execute_transfer(tx_id: &T::Hash) -> DispatchResultWithPostInfo {
            let pending = PendingTransfers::<T>::get(tx_id).ok_or(Error::<T>::PendingTxNotFound)?;

            // get from preimages
            let (call, _) = T::Preimages::realize::<T::RuntimeCall>(&pending.call)
                .map_err(|_| Error::<T>::CallDecodingFailed)?;

            // Release the funds
            pallet_balances::Pallet::<T>::release(
                &HoldReason::ScheduledTransfer.into(),
                &pending.who,
                pending.amount,
                Precision::Exact,
            )?;

            let post_info = call
                .dispatch(frame_support::dispatch::RawOrigin::Signed(pending.who.clone()).into());

            // Remove from account index
            AccountPendingIndex::<T>::mutate(&pending.who, |current_count| {
                // Decrement the count of pending transactions for the account.
                *current_count = current_count.saturating_sub(1);
            });

            // Remove from main storage
            if pending.count > 1 {
                // If there are more than one identical transactions, decrement the count
                PendingTransfers::<T>::insert(
                    tx_id,
                    PendingTransfer {
                        who: pending.who.clone(),
                        call: pending.call,
                        amount: pending.amount,
                        count: pending.count.saturating_sub(1),
                    },
                );
            } else {
                // Otherwise, remove the transaction from storage
                PendingTransfers::<T>::remove(tx_id);
            }

            // Emit event
            Self::deposit_event(Event::TransactionExecuted {
                tx_id: *tx_id,
                result: post_info,
            });

            post_info
        }

        /// Simply converts hash output to a `TaskName`
        pub fn make_schedule_id(tx_id: &T::Hash, nonce: u32) -> Result<TaskName, DispatchError> {
            let task_name = T::Hashing::hash_of(&(tx_id, nonce).encode())
                .clone()
                .as_ref()
                .try_into()
                .map_err(|_| Error::<T>::InvalidCall)?;

            Ok(task_name)
        }

        /// Schedules a runtime call for delayed execution.
        /// This is intended to be called by the `TransactionExtension`, NOT directly by users.
        pub fn do_schedule_transfer(
            origin: T::RuntimeOrigin,
            dest: <<T as frame_system::Config>::Lookup as StaticLookup>::Source,
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let ReversibleAccountData { delay, .. } =
                Self::reversible_accounts(&who).ok_or(Error::<T>::AccountNotReversible)?;

            let transfer_call: T::RuntimeCall = pallet_balances::Call::<T>::transfer_keep_alive {
                dest: dest.clone(),
                value: amount,
            }
            .into();

            let tx_id = T::Hashing::hash_of(&(who.clone(), transfer_call.clone()).encode());

            // Check if the account can accommodate another pending transaction
            AccountPendingIndex::<T>::mutate(&who, |current_count| -> Result<(), DispatchError> {
                ensure!(
                    *current_count < T::MaxPendingPerAccount::get(),
                    Error::<T>::TooManyPendingTransactions
                );
                *current_count = current_count.saturating_add(1);
                Ok(())
            })?;

            let dispatch_time = match delay {
                BlockNumberOrTimestamp::BlockNumber(blocks) => DispatchTime::At(
                    T::BlockNumberProvider::current_block_number().saturating_add(blocks),
                ),
                BlockNumberOrTimestamp::Timestamp(millis) => {
                    DispatchTime::After(BlockNumberOrTimestamp::Timestamp(
                        T::TimeProvider::now().saturating_add(millis),
                    ))
                }
            };

            let call = T::Preimages::bound(transfer_call)?;

            // Store details before scheduling
            let new_pending = if let Some(pending) = PendingTransfers::<T>::get(tx_id) {
                PendingTransfer {
                    who: who.clone(),
                    call,
                    amount,
                    count: pending.count.saturating_add(1),
                }
            } else {
                PendingTransfer {
                    who: who.clone(),
                    call,
                    amount,
                    count: 1,
                }
            };
            let schedule_id = Self::make_schedule_id(&tx_id, new_pending.count)?;

            PendingTransfers::<T>::insert(tx_id, new_pending);

            let bounded_call = T::Preimages::bound(Call::<T>::execute_transfer { tx_id }.into())?;

            // Schedule the `do_execute` call
            T::Scheduler::schedule_named(
                schedule_id,
                dispatch_time,
                None,
                Default::default(),
                frame_support::dispatch::RawOrigin::Signed(Self::account_id()).into(),
                bounded_call,
            )
            .map_err(|e| {
                log::error!("Failed to schedule transaction: {:?}", e);
                Error::<T>::SchedulingFailed
            })?;

            // Hold the funds for the delay period
            pallet_balances::Pallet::<T>::hold(
                &HoldReason::ScheduledTransfer.into(),
                &who,
                amount,
            )?;

            Self::deposit_event(Event::TransactionScheduled {
                who,
                tx_id,
                execute_at: dispatch_time,
            });

            Ok(())
        }

        /// Cancels a previously scheduled transaction. Internal logic used by `cancel` extrinsic.
        fn cancel_transfer(who: &T::AccountId, tx_id: T::Hash) -> DispatchResult {
            // Retrieve owner from storage to verify ownership
            let pending = PendingTransfers::<T>::get(tx_id).ok_or(Error::<T>::PendingTxNotFound)?;

            let reversible_account_data = ReversibleAccounts::<T>::get(&pending.who)
                .ok_or(Error::<T>::AccountNotReversible)?;

            if let Some(explicit_reverser) = &reversible_account_data.explicit_reverser {
                // If the reverser is set, ensure the caller is the reverser
                ensure!(who == explicit_reverser, Error::<T>::InvalidReverser);
            } else {
                ensure!(&pending.who == who, Error::<T>::NotOwner);
            }

            if pending.count > 1 {
                // If there are more than one identical transactions, decrement the count
                PendingTransfers::<T>::insert(
                    &tx_id,
                    PendingTransfer {
                        who: pending.who.clone(),
                        call: pending.call,
                        amount: pending.amount,
                        count: pending.count.saturating_sub(1),
                    },
                );
            } else {
                // Otherwise, remove the transaction from storage
                PendingTransfers::<T>::remove(&tx_id);
            }

            // Decrement account index
            AccountPendingIndex::<T>::mutate(&pending.who, |current_count| {
                // Decrement the count of pending transactions for the account.
                *current_count = current_count.saturating_sub(1);
            });

            let schedule_id = Self::make_schedule_id(&tx_id, pending.count)?;

            // Cancel the scheduled task
            T::Scheduler::cancel_named(schedule_id).map_err(|_| Error::<T>::CancellationFailed)?;

            if let Some(reverser) = &reversible_account_data.explicit_reverser {
                pallet_balances::Pallet::<T>::transfer_on_hold(
                    &HoldReason::ScheduledTransfer.into(),
                    &pending.who,
                    reverser,
                    pending.amount,
                    Precision::Exact,
                    Restriction::Free,
                    Fortitude::Polite,
                )?;
            } else {
                // Release the funds
                pallet_balances::Pallet::<T>::release(
                    &HoldReason::ScheduledTransfer.into(),
                    &pending.who,
                    pending.amount,
                    Precision::Exact,
                )?;
            }
            Self::deposit_event(Event::TransactionCancelled {
                who: who.clone(),
                tx_id,
            });
            Ok(())
        }
    }

    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        /// Configure initial reversible accounts. [AccountId, Delay]
        /// NOTE: using `(bool, BlockNumberFor<T>)` where `bool` indicates if the delay is in block numbers
        pub initial_reversible_accounts: Vec<(T::AccountId, BlockNumberFor<T>)>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            for (who, delay) in &self.initial_reversible_accounts {
                // Basic validation, ensure delay is reasonable if needed
                let delay = BlockNumberOrTimestampOf::<T>::BlockNumber(*delay);

                if delay >= T::MinDelayPeriod::get() {
                    ReversibleAccounts::<T>::insert(
                        who,
                        ReversibleAccountData {
                            explicit_reverser: None,
                            delay,
                            policy: DelayPolicy::Explicit,
                        },
                    );
                } else {
                    // Optionally log a warning during genesis build
                    log::warn!(
                        "Genesis config for account {:?} has delay {:?} below MinDelayPeriod {:?}, skipping.",
                        who, delay, T::MinDelayPeriod::get()
                     );
                }
            }
        }
    }
}
