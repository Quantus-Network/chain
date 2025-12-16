#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use codec::Decode;
	use core::convert::TryInto;
	use frame_support::{
		pallet_prelude::*,
		parameter_types,
		traits::{
			Currency,
			ExistenceRequirement::{AllowDeath, KeepAlive},
			Get,
		},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::{
		traits::{AccountIdConversion, Saturating},
		ArithmeticError,
	};

	#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	pub struct VestingSchedule<AccountId, Balance, Moment> {
		pub id: u64,                // Unique id
		pub creator: AccountId,     // Who created the scehdule
		pub beneficiary: AccountId, // Who gets the tokens
		pub amount: Balance,        // Total tokens to vest
		pub start: Moment,          // When vesting begins
		pub end: Moment,            // When vesting fully unlocks
		pub claimed: Balance,       // Tokens already claimed
	}

	#[pallet::storage]
	pub type VestingSchedules<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		u64, // Key: schedule_id
		VestingSchedule<T::AccountId, T::Balance, T::Moment>,
		OptionQuery,
	>;

	#[pallet::storage]
	pub type ScheduleCounter<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::config]
	pub trait Config:
		frame_system::Config<RuntimeEvent: From<Event<Self>>>
		+ pallet_balances::Config
		+ pallet_timestamp::Config
	{
		type PalletId: Get<PalletId>;
		type WeightInfo: WeightInfo;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		VestingScheduleCreated(T::AccountId, T::Balance, T::Moment, T::Moment, u64),
		TokensClaimed(T::AccountId, T::Balance, u64),
		VestingScheduleCancelled(T::AccountId, u64), // Creator, Schedule ID
	}

	#[pallet::error]
	pub enum Error<T> {
		NoVestingSchedule, // No schedule exists for the caller
		InvalidSchedule,   // Start block >= end block
		TooManySchedules,  // Exceeded maximum number of schedules
		NotCreator,        // Caller isn’t the creator
		ScheduleNotFound,  // No schedule with that ID
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		// Create a vesting schedule
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::create_vesting_schedule())]
		pub fn create_vesting_schedule(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			amount: T::Balance,
			start: T::Moment,
			end: T::Moment,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			ensure!(start < end, Error::<T>::InvalidSchedule);
			ensure!(amount > T::Balance::zero(), Error::<T>::InvalidSchedule);

			// Transfer tokens from caller to pallet and lock them
			pallet_balances::Pallet::<T>::transfer(&who, &Self::account_id(), amount, KeepAlive)?;

			// Generate unique ID
			let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
			ScheduleCounter::<T>::put(schedule_id);

			// Add the schedule to storage
			let schedule = VestingSchedule {
				creator: who,
				beneficiary: beneficiary.clone(),
				amount,
				start,
				end,
				claimed: T::Balance::zero(),
				id: schedule_id,
			};
			VestingSchedules::<T>::insert(schedule_id, schedule);

			Self::deposit_event(Event::VestingScheduleCreated(
				beneficiary,
				amount,
				start,
				end,
				schedule_id,
			));
			Ok(())
		}

		// Claim vested tokens
		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::claim())]
		pub fn claim(_origin: OriginFor<T>, schedule_id: u64) -> DispatchResult {
			let mut schedule =
				VestingSchedules::<T>::get(schedule_id).ok_or(Error::<T>::NoVestingSchedule)?;
			let vested = Self::vested_amount(&schedule)?;
			let claimable = vested.saturating_sub(schedule.claimed);

			if claimable > T::Balance::zero() {
				schedule.claimed += claimable;
				VestingSchedules::<T>::insert(schedule_id, &schedule);

				// Transfer claimable tokens
				pallet_balances::Pallet::<T>::transfer(
					&Self::account_id(),
					&schedule.beneficiary,
					claimable,
					AllowDeath,
				)?;

				Self::deposit_event(Event::TokensClaimed(
					schedule.beneficiary,
					claimable,
					schedule_id,
				));
			}

			Ok(())
		}

		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::cancel_vesting_schedule())]
		pub fn cancel_vesting_schedule(origin: OriginFor<T>, schedule_id: u64) -> DispatchResult {
			let who = ensure_signed(origin.clone())?;

			// Claim for beneficiary whatever they are currently owed
			Self::claim(origin, schedule_id)?;

			let schedule =
				VestingSchedules::<T>::get(schedule_id).ok_or(Error::<T>::ScheduleNotFound)?;
			ensure!(schedule.creator == who, Error::<T>::NotCreator);

			// Refund unclaimed amount to the creator
			let unclaimed = schedule.amount.saturating_sub(schedule.claimed);
			if unclaimed > T::Balance::zero() {
				pallet_balances::Pallet::<T>::transfer(
					&Self::account_id(),
					&who,
					unclaimed,
					AllowDeath,
				)?;
			}

			VestingSchedules::<T>::remove(schedule_id);

			// Emit event
			Self::deposit_event(Event::VestingScheduleCancelled(who, schedule_id));
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		// Helper to calculate vested amount
		pub fn vested_amount(
			schedule: &VestingSchedule<T::AccountId, T::Balance, T::Moment>,
		) -> Result<T::Balance, DispatchError> {
			let now = <pallet_timestamp::Pallet<T>>::get();
			// No need to convert now/start/end to u64 explicitly if T::Moment is u64-like
			if now < schedule.start {
				Ok(T::Balance::zero())
			} else if now >= schedule.end {
				Ok(schedule.amount)
			} else {
				let elapsed = now.saturating_sub(schedule.start);
				let duration = schedule.end.saturating_sub(schedule.start);

				// Convert amount to u64 for intermediate calculation
				let amount_u64: u64 = schedule
					.amount
					.try_into()
					.map_err(|_| DispatchError::Other("Balance to u64 conversion failed"))?;

				// Perform calculation in u64 (T::Moment-like)
				let elapsed_u64: u64 = elapsed
					.try_into()
					.map_err(|_| DispatchError::Other("Moment to u64 conversion failed"))?;
				let duration_u64: u64 = duration
					.try_into()
					.map_err(|_| DispatchError::Other("Moment to u64 conversion failed"))?;
				let duration_safe: u64 = duration_u64.max(1);

				let vested_u64: u64 = amount_u64
					.saturating_mul(elapsed_u64)
					.checked_div(duration_safe)
					.ok_or(DispatchError::Arithmetic(ArithmeticError::Underflow))?;

				// Convert back to T::Balance
				let vested = T::Balance::try_from(vested_u64)
					.map_err(|_| DispatchError::Other("u64 to Balance conversion failed"))?;

				Ok(vested)
			}
		}

		// Pallet account to "hold" tokens
		pub fn account_id() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}
	}

	parameter_types! {
		pub const VestingPalletId: PalletId = PalletId(*b"vestingp");
	}

	// Implement VestedTransfer trait for compatibility with merkle-airdrop
	use frame_support::traits::VestedTransfer;
	use frame_system::pallet_prelude::BlockNumberFor;

	impl<T: Config> VestedTransfer<T::AccountId> for Pallet<T>
	where
		T::Balance: From<BlockNumberFor<T>> + TryInto<u64>,
		T::Moment: From<u64>,
	{
		type Currency = pallet_balances::Pallet<T>;
		type Moment = BlockNumberFor<T>;

		fn vested_transfer(
			source: &T::AccountId,
			dest: &T::AccountId,
			amount: T::Balance,
			per_block: T::Balance,
			starting_block: BlockNumberFor<T>,
		) -> DispatchResult {
			// Convert block number to timestamp (milliseconds)
			// Assuming 12 second blocks: block_number * 12000ms
			const BLOCK_TIME_MS: u64 = 12000;

			let start_block: u64 = starting_block
				.try_into()
				.map_err(|_| DispatchError::Other("Block number conversion failed"))?;
			let per_block_u64: u64 = per_block
				.try_into()
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;
			let locked: u64 = amount
				.try_into()
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;

			let start_ms = start_block.saturating_mul(BLOCK_TIME_MS);

			// Calculate duration: total_amount / per_block = number of blocks
			let duration_blocks = if per_block_u64 > 0 {
				locked.saturating_div(per_block_u64)
			} else {
				return Err(Error::<T>::InvalidSchedule.into());
			};
			let duration_ms = duration_blocks.saturating_mul(BLOCK_TIME_MS);
			let end_ms = start_ms.saturating_add(duration_ms);

			// Transfer from source to pallet account
			pallet_balances::Pallet::<T>::transfer(
				source,
				&Self::account_id(),
				amount,
				frame_support::traits::ExistenceRequirement::KeepAlive,
			)?;

			// Generate unique ID
			let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
			ScheduleCounter::<T>::put(schedule_id);

			// Create vesting schedule
			let vesting_schedule = VestingSchedule {
				creator: source.clone(),
				beneficiary: dest.clone(),
				amount,
				start: T::Moment::from(start_ms),
				end: T::Moment::from(end_ms),
				claimed: T::Balance::zero(),
				id: schedule_id,
			};
			VestingSchedules::<T>::insert(schedule_id, vesting_schedule);

			Self::deposit_event(Event::VestingScheduleCreated(
				dest.clone(),
				amount,
				T::Moment::from(start_ms),
				T::Moment::from(end_ms),
				schedule_id,
			));

			Ok(())
		}
	}

	// Implement VestingSchedule trait for compatibility with merkle-airdrop
	use frame_support::traits::VestingSchedule as VestingScheduleTrait;

	impl<T: Config> VestingScheduleTrait<T::AccountId> for Pallet<T>
	where
		T::Balance: From<BlockNumberFor<T>> + TryInto<u64>,
		T::Moment: From<u64>,
	{
		type Currency = pallet_balances::Pallet<T>;
		type Moment = BlockNumberFor<T>;

		fn vesting_balance(
			who: &T::AccountId,
		) -> Option<<Self::Currency as Currency<T::AccountId>>::Balance> {
			// Sum up all pending vested amounts for this account
			let mut total_vesting = T::Balance::zero();

			// Iterate through all schedules (this is not efficient but works)
			let counter = ScheduleCounter::<T>::get();
			for schedule_id in 1..=counter {
				if let Some(schedule) = VestingSchedules::<T>::get(schedule_id) {
					if schedule.beneficiary == *who {
						let remaining = schedule.amount.saturating_sub(schedule.claimed);
						total_vesting = total_vesting.saturating_add(remaining);
					}
				}
			}

			if total_vesting > T::Balance::zero() {
				Some(total_vesting)
			} else {
				None
			}
		}

		fn add_vesting_schedule(
			who: &T::AccountId,
			locked: <Self::Currency as Currency<T::AccountId>>::Balance,
			per_block: <Self::Currency as Currency<T::AccountId>>::Balance,
			starting_block: BlockNumberFor<T>,
		) -> DispatchResult {
			// Convert block number to timestamp (milliseconds)
			const BLOCK_TIME_MS: u64 = 12000;

			let start_block: u64 = starting_block
				.try_into()
				.map_err(|_| DispatchError::Other("Block number conversion failed"))?;
			let per_block_u64: u64 = per_block
				.try_into()
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;
			let locked_u64: u64 = locked
				.try_into()
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;

			let start_ms = start_block.saturating_mul(BLOCK_TIME_MS);

			// Calculate duration: total_amount / per_block = number of blocks
			let duration_blocks = if per_block_u64 > 0 {
				locked_u64.saturating_div(per_block_u64)
			} else {
				return Err(Error::<T>::InvalidSchedule.into());
			};
			let duration_ms = duration_blocks.saturating_mul(BLOCK_TIME_MS);
			let end_ms = start_ms.saturating_add(duration_ms);

			// Generate unique ID
			let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
			ScheduleCounter::<T>::put(schedule_id);

			// Create vesting schedule - tokens should already be in pallet account
			let vesting_schedule = VestingSchedule {
				creator: who.clone(),
				beneficiary: who.clone(),
				amount: locked,
				start: T::Moment::from(start_ms),
				end: T::Moment::from(end_ms),
				claimed: T::Balance::zero(),
				id: schedule_id,
			};
			VestingSchedules::<T>::insert(schedule_id, vesting_schedule);

			Self::deposit_event(Event::VestingScheduleCreated(
				who.clone(),
				locked,
				T::Moment::from(start_ms),
				T::Moment::from(end_ms),
				schedule_id,
			));

			Ok(())
		}

		fn can_add_vesting_schedule(
			_who: &T::AccountId,
			_locked: <Self::Currency as Currency<T::AccountId>>::Balance,
			_per_block: <Self::Currency as Currency<T::AccountId>>::Balance,
			_starting_block: BlockNumberFor<T>,
		) -> DispatchResult {
			// Our custom vesting doesn't have a limit on number of schedules
			Ok(())
		}

		fn remove_vesting_schedule(_who: &T::AccountId, _schedule_index: u32) -> DispatchResult {
			// This is not supported in our custom implementation
			// merkle-airdrop doesn't use this method
			Err(DispatchError::Other("remove_vesting_schedule not supported"))
		}
	}
}
