//! # Reversibility Core Pallet
//!
//! Provides the core logic for scheduling and cancelling reversible transactions.
//! It manages the state of accounts opting into reversibility and the pending
//! transactions associated with them. Transaction interception is handled
//! separately via a `SignedExtension`.
//!
//! ## Volume Fee for High-Security Accounts
//!
//! When high-security accounts reverse transactions, a configurable volume fee
//! (expressed as a Permill) is deducted from the transaction amount and sent
//! to the treasury. Regular accounts do not incur any fees when reversing transactions.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
pub use pallet::*;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::WeightInfo;

use alloc::vec::Vec;
use frame_support::{
	pallet_prelude::*,
	traits::{
		tokens::{
			fungibles::MutateHold as AssetsHold, ExistenceRequirement, Fortitude, Restriction,
		},
		Currency,
	},
};
use frame_system::pallet_prelude::*;
use qp_scheduler::{BlockNumberOrTimestamp, DispatchTime, ScheduleNamed};
use sp_arithmetic::Permill;
use sp_runtime::traits::StaticLookup;

/// Type alias for this config's `BlockNumberOrTimestamp`.
pub type BlockNumberOrTimestampOf<T> =
	BlockNumberOrTimestamp<BlockNumberFor<T>, <T as Config>::Moment>;

/// Type alias for the Recovery pallet's expected block number type
pub type RecoveryBlockNumberOf<T> =
	<<T as pallet_recovery::Config>::BlockNumberProvider as sp_runtime::traits::BlockNumberProvider>::BlockNumber;

/// High security account details
#[derive(Encode, Decode, MaxEncodedLen, Clone, Default, TypeInfo, Debug, PartialEq, Eq)]
pub struct HighSecurityAccountData<AccountId, Delay> {
	/// The account that can reverse the transaction
	pub interceptor: AccountId,
	/// The delay period for the account
	pub delay: Delay,
}

/// Pending transfer details
#[derive(Encode, Decode, MaxEncodedLen, Clone, Default, TypeInfo, Debug, PartialEq, Eq)]
pub struct PendingTransfer<AccountId, Balance, Call> {
	/// The account that scheduled the transaction
	pub from: AccountId,
	/// The account that the transfer is to
	pub to: AccountId,
	/// The account that can intercept the transaction
	pub interceptor: AccountId,
	/// The call
	pub call: Call,
	/// Amount frozen for the transaction
	pub amount: Balance,
}

/// Balance type
type BalanceOf<T> = <T as pallet_balances::Config>::Balance;

/// AssetId type
type AssetIdOf<T> = <T as pallet_assets::Config>::AssetId;

/// Canonical RuntimeCall for this pallet (disambiguates multiple `RuntimeCall` providers)
type RuntimeCallOf<T> = <T as frame_system::Config>::RuntimeCall;

/// Type aliases for asset holder pallet
type AssetsHoldReasonOf<T> = <T as pallet_assets_holder::Config>::RuntimeHoldReason;
type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
type AssetsHolderOf<T> = pallet_assets_holder::Pallet<T>;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use crate::BlockNumberOrTimestampOf;
	use frame_support::{
		dispatch::PostDispatchInfo,
		traits::{
			fungible::MutateHold, schedule::v3::TaskName, tokens::Precision, Bounded, CallerTrait,
			QueryPreimage, StorePreimage, Time,
		},
		PalletId,
	};
	use sp_runtime::{
		traits::{
			AccountIdConversion, AtLeast32Bit, BlockNumberProvider, Dispatchable, Hash, Scale, Zero,
		},
		Saturating,
	};

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config:
		frame_system::Config<
			RuntimeCall: From<pallet_balances::Call<Self>>
			                 + From<pallet_assets::Call<Self>>
			                 + From<Call<Self>>
			                 + Dispatchable<PostInfo = PostDispatchInfo>
			                 + TryInto<pallet_balances::Call<Self>>
			                 + TryInto<pallet_assets::Call<Self>>,
		> + pallet_balances::Config<RuntimeHoldReason = <Self as Config>::RuntimeHoldReason>
		+ pallet_assets::Config<Balance = <Self as pallet_balances::Config>::Balance>
		+ pallet_assets_holder::Config<RuntimeHoldReason = <Self as Config>::RuntimeHoldReason>
		+ pallet_recovery::Config
	{
		/// Scheduler for the runtime. We use the Named scheduler for cancellability.
		type Scheduler: ScheduleNamed<
			BlockNumberFor<Self>,
			Self::Moment,
			<Self as frame_system::Config>::RuntimeCall,
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

		/// Maximum number of accounts an interceptor can intercept for. Used for BoundedVec.
		#[pallet::constant]
		type MaxInterceptorAccounts: Get<u32>;

		/// The default delay period for reversible transactions if none is specified.
		///
		/// NOTE: default delay is always in blocks.
		#[pallet::constant]
		type DefaultDelay: Get<BlockNumberOrTimestampOf<Self>>;

		/// The minimum delay period allowed for reversible transactions, in blocks.
		#[pallet::constant]
		type MinDelayPeriodBlocks: Get<BlockNumberFor<Self>>;

		/// The minimum delay period allowed for reversible transactions, in milliseconds.
		#[pallet::constant]
		type MinDelayPeriodMoment: Get<Self::Moment>;

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

		/// Volume fee taken from reversed transactions for high-security accounts only,
		/// expressed as a Permill (e.g., Permill::from_percent(1) = 1%). Regular accounts incur no
		/// fees.
		#[pallet::constant]
		type VolumeFee: Get<Permill>;

		/// Treasury account ID where volume fees are sent.
		type TreasuryAccountId: Get<Self::AccountId>;
	}

	/// Maps accounts to their chosen reversibility delay period (in milliseconds).
	/// Accounts present in this map have reversibility enabled.
	#[pallet::storage]
	#[pallet::getter(fn high_security_accounts)]
	pub type HighSecurityAccounts<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		HighSecurityAccountData<T::AccountId, BlockNumberOrTimestampOf<T>>,
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
		PendingTransfer<T::AccountId, BalanceOf<T>, Bounded<RuntimeCallOf<T>, T::Hashing>>,
		OptionQuery,
	>;

	/// Indexes pending transaction IDs per account for efficient lookup and cancellation.
	/// Also enforces the maximum pending transactions limit per account.
	#[pallet::storage]
	#[pallet::getter(fn account_pending_index)]
	pub type AccountPendingIndex<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

	/// Maps sender accounts to their list of pending transaction IDs.
	/// This allows users to query all their outgoing pending transfers.
	#[pallet::storage]
	#[pallet::getter(fn pending_transfers_by_sender)]
	pub type PendingTransfersBySender<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		BoundedVec<T::Hash, T::MaxPendingPerAccount>,
		ValueQuery,
	>;

	/// Maps recipient accounts to their list of pending incoming transaction IDs.
	/// This allows users to query all their incoming pending transfers.
	#[pallet::storage]
	#[pallet::getter(fn pending_transfers_by_recipient)]
	pub type PendingTransfersByRecipient<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		BoundedVec<T::Hash, T::MaxPendingPerAccount>,
		ValueQuery,
	>;

	/// Maps interceptor accounts to the list of accounts they can intercept for.
	/// This allows the UI to efficiently query all accounts for which a given account is an
	/// interceptor.
	#[pallet::storage]
	#[pallet::getter(fn interceptor_index)]
	pub type InterceptorIndex<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		BoundedVec<T::AccountId, T::MaxInterceptorAccounts>,
		ValueQuery,
	>;

	/// Global nonce for generating unique transaction IDs.
	#[pallet::storage]
	#[pallet::getter(fn global_nonce)]
	pub type GlobalNonce<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A user has enabled their high-security settings.
		/// [who, interceptor, recoverer, delay]
		HighSecuritySet {
			who: T::AccountId,
			interceptor: T::AccountId,
			delay: BlockNumberOrTimestampOf<T>,
		},
		/// A transaction has been intercepted and scheduled for delayed execution.
		/// [from, to, interceptor, amount, tx_id, execute_at_moment]
		TransactionScheduled {
			from: T::AccountId,
			to: T::AccountId,
			interceptor: T::AccountId,
			asset_id: Option<AssetIdOf<T>>,
			amount: BalanceOf<T>,
			tx_id: T::Hash,
			execute_at: DispatchTime<BlockNumberFor<T>, T::Moment>,
		},
		/// A scheduled transaction has been successfully cancelled by the owner.
		/// [who, tx_id]
		TransactionCancelled { who: T::AccountId, tx_id: T::Hash },
		/// A scheduled transaction was executed by the scheduler.
		/// [tx_id, dispatch_result]
		TransactionExecuted { tx_id: T::Hash, result: DispatchResultWithPostInfo },

		/// Volume fee was collected from a reversed transaction. [tx_id, fee_amount]
		VolumeFeeCollected { tx_id: T::Hash, fee_amount: BalanceOf<T> },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// The account attempting to enable reversibility is already marked as reversible.
		AccountAlreadyHighSecurity,
		/// The account attempting the action is not marked as high security.
		AccountNotHighSecurity,
		/// Interceptor can not be the account itself, because it is redundant.
		InterceptorCannotBeSelf,
		/// Recoverer cannot be the account itself, because it is redundant.
		RecovererCannotBeSelf,
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
		/// Cannot schedule one time reversible transaction when account is reversible (theft
		/// deterrence)
		AccountAlreadyReversibleCannotScheduleOneTime,
		/// The interceptor has reached the maximum number of accounts they can intercept for.
		TooManyInterceptorAccounts,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		T: pallet_balances::Config<RuntimeHoldReason = <T as Config>::RuntimeHoldReason>
			+ pallet_assets_holder::Config<RuntimeHoldReason = <T as Config>::RuntimeHoldReason>,
	{
		/// Enable high-security for the calling account with a specified
		/// reversibility delay.
		///
		/// Recoverer and interceptor (aka guardian) could be the same account or
		/// different accounts.
		///
		/// Once an account is set as high security it can only make reversible
		/// transfers. It is not allowed any other calls.
		///
		/// - `delay`: The reversibility time for any transfer made by the high
		/// security account.
		/// - interceptor: The account that can intercept transctions from the
		/// high security account.
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::set_high_security())]
		pub fn set_high_security(
			origin: OriginFor<T>,
			delay: BlockNumberOrTimestampOf<T>,
			interceptor: T::AccountId,
		) -> DispatchResult {
			let who = ensure_signed(origin.clone())?;

			ensure!(interceptor != who.clone(), Error::<T>::InterceptorCannotBeSelf);
			ensure!(
				!HighSecurityAccounts::<T>::contains_key(&who),
				Error::<T>::AccountAlreadyHighSecurity
			);

			Self::validate_delay(&delay)?;

			// Set up zero delay recovery for interceptor
			// The interceptor then simply needs to claim the recovery in order to be able
			// to make calls on behalf of the high security account.
			let recovery_delay_blocks: RecoveryBlockNumberOf<T> = Zero::zero();
			pallet_recovery::Pallet::<T>::create_recovery(
				origin,
				alloc::vec![interceptor.clone()],
				One::one(),
				recovery_delay_blocks,
			)?;

			let high_security_account_data =
				HighSecurityAccountData { interceptor: interceptor.clone(), delay };

			InterceptorIndex::<T>::try_mutate(interceptor.clone(), |accounts| {
				if !accounts.contains(&who) {
					accounts
						.try_push(who.clone())
						.map_err(|_| Error::<T>::TooManyInterceptorAccounts)
				} else {
					Ok(())
				}
			})?;

			HighSecurityAccounts::<T>::insert(who.clone(), &high_security_account_data);
			Self::deposit_event(Event::HighSecuritySet { who, interceptor, delay });

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

			ensure!(who == Self::account_id(), Error::<T>::InvalidSchedulerOrigin);

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

		/// Schedule a transaction for delayed execution with a custom, one-time delay.
		///
		/// This can only be used by accounts that have *not* set up a persistent
		/// reversibility configuration with `set_high_security`.
		///
		/// - `delay`: The time (in blocks or milliseconds) before the transaction executes.
		#[pallet::call_index(4)]
		#[pallet::weight(<T as Config>::WeightInfo::schedule_transfer())]
		pub fn schedule_transfer_with_delay(
			origin: OriginFor<T>,
			dest: <<T as frame_system::Config>::Lookup as StaticLookup>::Source,
			amount: BalanceOf<T>,
			delay: BlockNumberOrTimestampOf<T>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			log::debug!(target: "reversible-transfers", "schedule_transfer_with_delay with delay: {delay:?}");

			// Accounts with pre-configured reversibility cannot use this extrinsic.
			ensure!(
				!HighSecurityAccounts::<T>::contains_key(&who),
				Error::<T>::AccountAlreadyReversibleCannotScheduleOneTime
			);

			// Validate the provided delay.
			Self::validate_delay(&delay)?;

			Self::do_schedule_transfer_inner(who.clone(), dest, who, amount, delay, None)
		}

		/// Schedule an asset transfer (pallet-assets) for delayed execution using the configured
		/// delay.
		#[pallet::call_index(5)]
		#[pallet::weight(<T as Config>::WeightInfo::schedule_transfer())]
		pub fn schedule_asset_transfer(
			origin: OriginFor<T>,
			asset_id: AssetIdOf<T>,
			dest: <<T as frame_system::Config>::Lookup as StaticLookup>::Source,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let HighSecurityAccountData { delay, interceptor, .. } =
				Self::high_security_accounts(&who).ok_or(Error::<T>::AccountNotHighSecurity)?;

			Self::do_schedule_transfer_inner(who, dest, interceptor, amount, delay, Some(asset_id))
		}

		/// Schedule an asset transfer (pallet-assets) with a custom one-time delay.
		#[pallet::call_index(6)]
		#[pallet::weight(<T as Config>::WeightInfo::schedule_transfer())]
		pub fn schedule_asset_transfer_with_delay(
			origin: OriginFor<T>,
			asset_id: AssetIdOf<T>,
			dest: <<T as frame_system::Config>::Lookup as StaticLookup>::Source,
			amount: BalanceOf<T>,
			delay: BlockNumberOrTimestampOf<T>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// High security accounts cannot use this extrinsic.
			ensure!(
				!HighSecurityAccounts::<T>::contains_key(&who),
				Error::<T>::AccountAlreadyReversibleCannotScheduleOneTime
			);

			// Validate the provided delay.
			Self::validate_delay(&delay)?;

			Self::do_schedule_transfer_inner(who.clone(), dest, who, amount, delay, Some(asset_id))
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			assert!(
				!T::MinDelayPeriodBlocks::get().is_zero() &&
					!T::MinDelayPeriodMoment::get().is_zero(),
				"Minimum delay periods must be greater than 0"
			);

			// NOTE: default delay is always in blocks
			assert!(
				BlockNumberOrTimestampOf::<T>::BlockNumber(T::MinDelayPeriodBlocks::get()) <=
					T::DefaultDelay::get(),
				"Minimum delay periods must be less or equal to `T::DefaultDelay`"
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
		T: pallet_balances::Config<RuntimeHoldReason = <T as Config>::RuntimeHoldReason>
			+ pallet_assets_holder::Config<RuntimeHoldReason = <T as Config>::RuntimeHoldReason>,
	{
		#[inline]
		fn asset_hold_reason() -> AssetsHoldReasonOf<T> {
			HoldReason::ScheduledTransfer.into()
		}
		/// Check if an account has reversibility enabled and return its delay.
		pub fn is_high_security(
			who: &T::AccountId,
		) -> Option<HighSecurityAccountData<T::AccountId, BlockNumberOrTimestampOf<T>>> {
			HighSecurityAccounts::<T>::get(who)
		}

		/// Get full details of a pending transfer by its ID
		pub fn get_pending_transfer_details(
			tx_id: &T::Hash,
		) -> Option<
			PendingTransfer<T::AccountId, BalanceOf<T>, Bounded<RuntimeCallOf<T>, T::Hashing>>,
		> {
			PendingTransfers::<T>::get(tx_id)
		}

		// Pallet account as origin
		pub fn account_id() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}

		fn validate_delay(delay: &BlockNumberOrTimestampOf<T>) -> DispatchResult {
			match delay {
				BlockNumberOrTimestamp::BlockNumber(x) => {
					ensure!(*x > T::MinDelayPeriodBlocks::get(), Error::<T>::DelayTooShort)
				},
				BlockNumberOrTimestamp::Timestamp(t) => {
					ensure!(*t > T::MinDelayPeriodMoment::get(), Error::<T>::DelayTooShort)
				},
			}
			Ok(())
		}

		fn do_execute_transfer(tx_id: &T::Hash) -> DispatchResultWithPostInfo {
			let pending = PendingTransfers::<T>::get(tx_id).ok_or(Error::<T>::PendingTxNotFound)?;

			// get from preimages
			let (call, _) = T::Preimages::realize::<RuntimeCallOf<T>>(&pending.call)
				.map_err(|_| Error::<T>::CallDecodingFailed)?;

			// If this is an assets transfer, release the held amount before dispatch
			if let Ok(assets_call) = call.clone().try_into() {
				if let pallet_assets::Call::transfer_keep_alive { id, .. } = assets_call {
					let reason = Self::asset_hold_reason();
					let _ = <AssetsHolderOf<T> as AssetsHold<AccountIdOf<T>>>::release(
						id.into(),
						&reason,
						&pending.from,
						pending.amount,
						Precision::Exact,
					);
				}
			}

			// Release the funds only for native balances holds
			if let Ok(balance_call) = call.clone().try_into() {
				if let pallet_balances::Call::transfer_keep_alive { .. } = balance_call {
					pallet_balances::Pallet::<T>::release(
						&HoldReason::ScheduledTransfer.into(),
						&pending.from,
						pending.amount,
						Precision::Exact,
					)?;
				}
			}

			// Remove transfer from all storage (handles indexes, account count, etc.)
			Self::transfer_removed(&pending.from, *tx_id, &pending);

			let post_info = call
				.dispatch(frame_support::dispatch::RawOrigin::Signed(pending.from.clone()).into());

			// Emit event
			Self::deposit_event(Event::TransactionExecuted { tx_id: *tx_id, result: post_info });

			post_info
		}

		/// Simply converts hash output value to a `TaskName`
		pub fn make_schedule_id(tx_id: &T::Hash) -> Result<TaskName, DispatchError> {
			let task_name =
				tx_id.clone().as_ref().try_into().map_err(|_| Error::<T>::InvalidCall)?;

			Ok(task_name)
		}

		/// Called when a new transfer is added - updates all storage indexes
		fn transfer_added(
			sender: &T::AccountId,
			recipient: &T::AccountId,
			tx_id: T::Hash,
			pending_transfer: PendingTransfer<
				T::AccountId,
				BalanceOf<T>,
				Bounded<RuntimeCallOf<T>, T::Hashing>,
			>,
		) -> DispatchResult {
			// Store the pending transfer
			PendingTransfers::<T>::insert(tx_id, pending_transfer);

			// Update account pending count
			AccountPendingIndex::<T>::mutate(sender, |count| {
				*count = count.saturating_add(1);
			});

			// Add to sender's pending list
			PendingTransfersBySender::<T>::try_mutate(sender, |list| {
				list.try_push(tx_id).map_err(|_| Error::<T>::TooManyPendingTransactions)
			})?;

			// Add to recipient's pending list
			PendingTransfersByRecipient::<T>::try_mutate(recipient, |list| {
				list.try_push(tx_id).map_err(|_| Error::<T>::TooManyPendingTransactions)
			})?;

			Ok(())
		}

		/// Called when a transfer is removed - cleans up all storage indexes
		fn transfer_removed(
			sender: &T::AccountId,
			tx_id: T::Hash,
			pending_transfer: &PendingTransfer<
				T::AccountId,
				BalanceOf<T>,
				Bounded<RuntimeCallOf<T>, T::Hashing>,
			>,
		) {
			// Update account pending count (always decrement for each removed instance)
			AccountPendingIndex::<T>::mutate(sender, |count| {
				*count = count.saturating_sub(1);
			});

			PendingTransfers::<T>::remove(tx_id);

			// Clean up sender index
			PendingTransfersBySender::<T>::mutate(sender, |list| {
				list.retain(|&x| x != tx_id);
			});

			// Clean up recipient index efficiently using stored recipient
			PendingTransfersByRecipient::<T>::mutate(&pending_transfer.to, |list| {
				list.retain(|&x| x != tx_id);
			});
		}

		/// Internal logic to schedule a transfer with a given delay.
		fn do_schedule_transfer_inner(
			from: T::AccountId,
			to: <<T as frame_system::Config>::Lookup as StaticLookup>::Source,
			interceptor: T::AccountId,
			amount: BalanceOf<T>,
			delay: BlockNumberOrTimestampOf<T>,
			asset_id: Option<AssetIdOf<T>>,
		) -> DispatchResult {
			let recipient = T::Lookup::lookup(to.clone())?;
			let transfer_call: RuntimeCallOf<T> = match asset_id {
				Some(ref id) => pallet_assets::Call::<T>::transfer_keep_alive {
					id: id.clone().into(),
					target: to.clone(),
					amount,
				}
				.into(),
				None => pallet_balances::Call::<T>::transfer_keep_alive {
					dest: to.clone(),
					value: amount,
				}
				.into(),
			};

			let tx_id = T::Hashing::hash_of(
				&(from.clone(), transfer_call.clone(), GlobalNonce::<T>::get()).encode(),
			);

			log::debug!(target: "reversible-transfers", "Reversible transfer scheduled with delay: {delay:?}");
			log::debug!(target: "reversible-transfers", "Reversible transfer tx_id: {tx_id:?}");

			// Check if the account can accommodate another pending transaction
			let current_count = AccountPendingIndex::<T>::get(&from);
			ensure!(
				current_count < T::MaxPendingPerAccount::get(),
				Error::<T>::TooManyPendingTransactions
			);

			let dispatch_time = match delay {
				BlockNumberOrTimestamp::BlockNumber(blocks) => DispatchTime::At(
					<T as pallet::Config>::BlockNumberProvider::current_block_number()
						.saturating_add(blocks),
				),
				BlockNumberOrTimestamp::Timestamp(millis) =>
					DispatchTime::After(BlockNumberOrTimestamp::Timestamp(
						T::TimeProvider::now().saturating_add(millis),
					)),
			};
			log::debug!(target: "reversible-transfers", "Now time: {:?}", T::TimeProvider::now());
			log::debug!(target: "reversible-transfers", "dispatch_time: {dispatch_time:?}");

			let call = T::Preimages::bound(transfer_call)?;

			// Store details before scheduling

			let new_pending = PendingTransfer {
				from: from.clone(),
				to: recipient.clone(),
				interceptor: interceptor.clone(),
				call,
				amount,
			};

			let schedule_id = Self::make_schedule_id(&tx_id)?;

			// Add transfer to all storage (handles indexes, account count, etc.)
			Self::transfer_added(&from, &recipient, tx_id, new_pending)?;

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
				log::error!("Failed to schedule transaction: {e:?}");
				Error::<T>::SchedulingFailed
			})?;

			// For assets, hold the funds using assets-holder; for native balances, hold the funds
			if let Some(ref id) = asset_id {
				let reason = Self::asset_hold_reason();
				<AssetsHolderOf<T> as AssetsHold<AccountIdOf<T>>>::hold(
					id.clone(),
					&reason,
					&from,
					amount,
				)?;
			} else {
				pallet_balances::Pallet::<T>::hold(
					&HoldReason::ScheduledTransfer.into(),
					&from,
					amount,
				)?;
			}

			GlobalNonce::<T>::mutate(|nonce| nonce.saturating_inc());

			Self::deposit_event(Event::TransactionScheduled {
				from,
				to: recipient,
				interceptor,
				asset_id,
				tx_id,
				execute_at: dispatch_time,
				amount,
			});

			Ok(())
		}

		/// Schedules a runtime call for delayed execution using the pre-configured delay.
		/// This is intended to be called by the `TransactionExtension`, NOT directly by users.
		pub fn do_schedule_transfer(
			origin: T::RuntimeOrigin,
			dest: <<T as frame_system::Config>::Lookup as StaticLookup>::Source,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let HighSecurityAccountData { delay, interceptor, .. } =
				Self::high_security_accounts(&who).ok_or(Error::<T>::AccountNotHighSecurity)?;

			Self::do_schedule_transfer_inner(who, dest, interceptor, amount, delay, None)
		}

		/// Cancels a previously scheduled transaction. Internal logic used by `cancel` extrinsic.
		fn cancel_transfer(who: &T::AccountId, tx_id: T::Hash) -> DispatchResult {
			// Retrieve owner from storage to verify ownership
			let pending = PendingTransfers::<T>::get(tx_id).ok_or(Error::<T>::PendingTxNotFound)?;

			let high_security_account_data = HighSecurityAccounts::<T>::get(&pending.from);

			// if high-security account, interceptor is third party, else it is owner
			let interceptor = if let Some(ref data) = high_security_account_data {
				ensure!(who == &data.interceptor, Error::<T>::InvalidReverser);
				data.interceptor.clone()
			} else {
				ensure!(who == &pending.from, Error::<T>::NotOwner);
				pending.from.clone()
			};

			// Remove transfer from all storage (handles indexes, account count, etc.)
			Self::transfer_removed(&pending.from, tx_id, &pending);

			let schedule_id = Self::make_schedule_id(&tx_id)?;

			// Cancel the scheduled task
			T::Scheduler::cancel_named(schedule_id).map_err(|_| Error::<T>::CancellationFailed)?;

			// Calculate volume fee only for high-security accounts
			let (fee_amount, remaining_amount) = if high_security_account_data.is_some() {
				let volume_fee = T::VolumeFee::get();
				let fee = volume_fee * pending.amount;
				let remaining = pending.amount.saturating_sub(fee);
				(fee, remaining)
			} else {
				// No fee for regular accounts
				(Zero::zero(), pending.amount)
			};
			let treasury_account = T::TreasuryAccountId::get();

			// For assets, transfer held funds to treasury (fee) and interceptor (remaining)
			// For native balances, transfer held funds to treasury (fee) and interceptor
			// (remaining)
			if let Ok((call, _)) = T::Preimages::peek::<RuntimeCallOf<T>>(&pending.call) {
				if let Ok(assets_call) = call.clone().try_into() {
					if let pallet_assets::Call::transfer_keep_alive { id, .. } = assets_call {
						let reason = Self::asset_hold_reason();
						let asset_id = id.into();

						// Transfer fee to treasury if fee_amount > 0
						if !fee_amount.is_zero() {
							let _ =
								<AssetsHolderOf<T> as AssetsHold<AccountIdOf<T>>>::transfer_on_hold(
									asset_id.clone(),
									&reason,
									&pending.from,
									&treasury_account,
									fee_amount,
									Precision::Exact,
									Restriction::Free,
									Fortitude::Polite,
								);
						}

						// Transfer remaining amount to interceptor
						if !remaining_amount.is_zero() {
							let _ =
								<AssetsHolderOf<T> as AssetsHold<AccountIdOf<T>>>::transfer_on_hold(
									asset_id,
									&reason,
									&pending.from,
									&interceptor,
									remaining_amount,
									Precision::Exact,
									Restriction::Free,
									Fortitude::Polite,
								);
						}
					}
				}
				if let Ok(balance_call) = call.clone().try_into() {
					if let pallet_balances::Call::transfer_keep_alive { .. } = balance_call {
						// Transfer fee to treasury
						pallet_balances::Pallet::<T>::transfer(
							&pending.from,
							&treasury_account,
							fee_amount,
							ExistenceRequirement::KeepAlive, // keep the source account alive
						)?;

						// Transfer remaining amount to interceptor
						pallet_balances::Pallet::<T>::transfer(
							&pending.from,
							&interceptor,
							remaining_amount,
							ExistenceRequirement::KeepAlive, // keep the source account alive
						)?;
					}
				}
			}

			Self::deposit_event(Event::TransactionCancelled { who: who.clone(), tx_id });
			Ok(())
		}
	}

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		/// Configure initial reversible accounts. [AccountId, Delay]
		/// NOTE: using `(bool, BlockNumberFor<T>)` where `bool` indicates if the delay is in block
		/// numbers
		pub initial_high_security_accounts: Vec<(T::AccountId, T::AccountId, BlockNumberFor<T>)>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			for (who, interceptor, delay) in &self.initial_high_security_accounts {
				// Basic validation, ensure delay is reasonable if needed
				let wrapped_delay = BlockNumberOrTimestampOf::<T>::BlockNumber(*delay);

				if *delay >= T::MinDelayPeriodBlocks::get() {
					HighSecurityAccounts::<T>::insert(
						who,
						HighSecurityAccountData {
							interceptor: interceptor.clone(),
							delay: wrapped_delay,
						},
					);
				} else {
					// Optionally log a warning during genesis build
					log::warn!(
                        "Genesis config for account {:?} has delay {:?} below MinDelayPeriodBlocks {:?}, skipping.",
                        who, wrapped_delay, T::MinDelayPeriodBlocks::get()
                     );
				}
			}
		}
	}
}
