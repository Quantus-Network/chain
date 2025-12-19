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
			fungible::hold::Mutate as HoldMutate, tokens::Precision, Currency,
			ExistenceRequirement::AllowDeath, Get,
		},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::{
		traits::{AccountIdConversion, Saturating},
		ArithmeticError,
	};

	#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	pub enum VestingType<Moment> {
		/// Linear vesting - tokens unlock proportionally over time
		Linear,
		/// Linear vesting with cliff - nothing unlocks until cliff, then linear
		LinearWithCliff { cliff: Moment },
		/// Stepped vesting - tokens unlock in equal portions at regular intervals
		Stepped { step_duration: Moment },
	}

	#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	pub struct VestingSchedule<AccountId, Balance, Moment> {
		pub id: u64,                           // Unique id
		pub creator: AccountId,                // Who created the schedule
		pub beneficiary: AccountId,            // Who gets the tokens
		pub amount: Balance,                   // Total tokens to vest
		pub start: Moment,                     // When vesting begins
		pub end: Moment,                       // When vesting fully unlocks
		pub vesting_type: VestingType<Moment>, // Type of vesting
		pub claimed: Balance,                  // Tokens already claimed
		pub funding_account: AccountId,        // Account from which tokens are claimed (lazy funding)
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

	/// Number of vesting schedules per beneficiary
	#[pallet::storage]
	pub type BeneficiaryScheduleCount<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId, // beneficiary
		u32,          // count of active schedules
		ValueQuery,
	>;

	/// Total pending obligations per funding account
	/// This tracks how much each account has promised to vest (not yet fully claimed)
	#[pallet::storage]
	pub type PendingObligations<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId, // funding_account
		T::Balance,   // total pending amount
		ValueQuery,
	>;

	/// Amount currently frozen per funding account
	/// This tracks how much is actually frozen (may be less than obligations if insufficient funds)
	#[pallet::storage]
	pub type FrozenBalance<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId, // funding_account
		T::Balance,   // frozen amount
		ValueQuery,
	>;

	/// Reason for holding funds in this pallet
	#[pallet::composite_enum]
	pub enum HoldReason {
		/// Funds are held for vesting obligations
		VestingObligation,
	}

	#[pallet::config]
	pub trait Config:
		frame_system::Config<RuntimeEvent: From<Event<Self>>>
		+ pallet_balances::Config<RuntimeHoldReason: From<HoldReason>>
		+ pallet_timestamp::Config
	{
		type PalletId: Get<PalletId>;
		type WeightInfo: WeightInfo;

		/// Maximum number of vesting schedules per beneficiary
		#[pallet::constant]
		type MaxSchedulesPerBeneficiary: Get<u32>;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Vesting schedule created [beneficiary, amount, start, end, schedule_id, funding_account]
		VestingScheduleCreated {
			beneficiary: T::AccountId,
			amount: T::Balance,
			start: T::Moment,
			end: T::Moment,
			schedule_id: u64,
			funding_account: T::AccountId,
		},
		/// Tokens claimed [beneficiary, amount, schedule_id]
		TokensClaimed { beneficiary: T::AccountId, amount: T::Balance, schedule_id: u64 },
		/// Partial claim (insufficient funds available) [beneficiary, claimed, remaining_unlocked, schedule_id]
		PartialClaim {
			beneficiary: T::AccountId,
			claimed: T::Balance,
			remaining_unlocked: T::Balance,
			schedule_id: u64,
		},
		/// Vesting schedule cancelled [creator, schedule_id]
		VestingScheduleCancelled { creator: T::AccountId, schedule_id: u64 },
		/// Funds frozen for vesting obligations [account, amount, total_frozen]
		FundsFrozen { account: T::AccountId, amount: T::Balance, total_frozen: T::Balance },
		/// Funds unfrozen (obligations decreased) [account, amount, total_frozen]
		FundsUnfrozen { account: T::AccountId, amount: T::Balance, total_frozen: T::Balance },
	}

	#[pallet::error]
	pub enum Error<T> {
		NoVestingSchedule,   // No schedule exists for the caller
		InvalidSchedule,     // Start block >= end block
		TooManySchedules,    // Exceeded maximum number of schedules
		NotCreator,          // Caller isn't the creator
		ScheduleNotFound,    // No schedule with that ID
		NothingToClaim,      // No unlocked tokens available to claim
		NoFundsAvailable,    // Funding account has no funds
		ArithmeticError,     // Arithmetic overflow/underflow
		InsufficientFrozen,  // Not enough frozen balance for operation
		InvalidStepDuration, // Step duration must be > 0
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// Auto-freeze funds for pending vesting obligations
		///
		/// This runs every block to automatically freeze funds in accounts that have
		/// pending vesting obligations. Since mining rewards come every block,
		/// we check each account with obligations to see if we can freeze more.
		///
		/// Note: Skips pallet account - tokens already in pallet are secured and
		/// don't need freeze (they came via upfront transfer from vested_transfer).
		fn on_initialize(_n: BlockNumberFor<T>) -> Weight {
			let mut weight = Weight::zero();
			let pallet_account = Self::account_id();

			// Iterate through all accounts with pending obligations
			for (account, obligations) in PendingObligations::<T>::iter() {
				if obligations == T::Balance::zero() {
					// Skip empty obligations (shouldn't happen, but defensive)
					weight = weight.saturating_add(T::DbWeight::get().reads(1));
					continue;
				}

				// Skip pallet account - tokens already in pallet are secured
				// They came via upfront transfer (vested_transfer trait) and don't need freeze
				if account == pallet_account {
					weight = weight.saturating_add(T::DbWeight::get().reads(1));
					continue;
				}

				let frozen = FrozenBalance::<T>::get(&account);
				weight = weight.saturating_add(T::DbWeight::get().reads(2));

				// Skip if already fully funded
				if frozen >= obligations {
					continue;
				}

				// Unfunded obligations exist - try to freeze more
				if Self::try_freeze_funds(&account).is_ok() {
					// Successful freeze attempt
					weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 2));
				} else {
					// Error (e.g., account doesn't exist) - only reads
					weight = weight.saturating_add(T::DbWeight::get().reads(2));
				}
			}

			weight
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create a linear vesting schedule with lazy funding
		///
		/// # Arguments
		/// * `origin` - The creator of the vesting schedule
		/// * `beneficiary` - Who will receive the vested tokens
		/// * `amount` - Total amount to vest
		/// * `start` - When vesting begins
		/// * `end` - When vesting completes
		/// * `funding_account` - Account from which tokens will be claimed (lazy funding)
		///
		/// Tokens are NOT transferred upfront. They remain in funding_account and are
		/// automatically frozen to prevent spending. Tokens are transferred only when claimed.
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::create_vesting_schedule())]
		pub fn create_vesting_schedule(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			amount: T::Balance,
			start: T::Moment,
			end: T::Moment,
			funding_account: T::AccountId,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			ensure!(start < end, Error::<T>::InvalidSchedule);
			ensure!(amount > T::Balance::zero(), Error::<T>::InvalidSchedule);

			// Check if beneficiary has reached the maximum number of schedules
			let current_count = BeneficiaryScheduleCount::<T>::get(&beneficiary);
			ensure!(
				current_count < T::MaxSchedulesPerBeneficiary::get(),
				Error::<T>::TooManySchedules
			);

			// NO UPFRONT TRANSFER! Tokens stay in funding_account.
			// Increase pending obligations
			PendingObligations::<T>::mutate(&funding_account, |total| {
				*total = total.saturating_add(amount);
			});

			// Try to freeze funds immediately if available
			let _ = Self::try_freeze_funds(&funding_account);

			// Generate unique ID
			let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
			ScheduleCounter::<T>::put(schedule_id);

			// Add the schedule to storage
			let schedule = VestingSchedule {
				creator: who.clone(),
				beneficiary: beneficiary.clone(),
				amount,
				start,
				end,
				vesting_type: VestingType::Linear,
				claimed: T::Balance::zero(),
				id: schedule_id,
				funding_account: funding_account.clone(),
			};
			VestingSchedules::<T>::insert(schedule_id, schedule);

			// Increment beneficiary schedule count
			BeneficiaryScheduleCount::<T>::mutate(&beneficiary, |count| {
				*count = count.saturating_add(1);
			});

			Self::deposit_event(Event::VestingScheduleCreated {
				beneficiary,
				amount,
				start,
				end,
				schedule_id,
				funding_account,
			});
			Ok(())
		}

		/// Claim vested tokens (lazy transfer from funding_account)
		///
		/// Tokens are transferred from funding_account to beneficiary.
		/// If funding_account has insufficient balance, a partial claim is executed.
		/// Frozen funds are automatically released during claim.
		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::claim())]
		pub fn claim(_origin: OriginFor<T>, schedule_id: u64) -> DispatchResult {
			let mut schedule =
				VestingSchedules::<T>::get(schedule_id).ok_or(Error::<T>::NoVestingSchedule)?;

			// Calculate how much is unlocked and available to claim
			let vested = Self::vested_amount(&schedule)?;
			let claimable = vested.saturating_sub(schedule.claimed);

			ensure!(claimable > T::Balance::zero(), Error::<T>::NothingToClaim);

			let funding_account = schedule.funding_account.clone();
			let beneficiary = schedule.beneficiary.clone();

			// Amount to actually transfer (limited by available funds)
			let mut actual_transfer_amount = T::Balance::zero();

			// Try to use frozen funds first
			let frozen_for_account = FrozenBalance::<T>::get(&funding_account);
			let amount_from_frozen = claimable.min(frozen_for_account);

			if amount_from_frozen > T::Balance::zero() {
				pallet_balances::Pallet::<T>::release(
					&HoldReason::VestingObligation.into(),
					&funding_account,
					amount_from_frozen,
					Precision::Exact,
				)
				.map_err(|_| Error::<T>::InsufficientFrozen)?;

				FrozenBalance::<T>::mutate(&funding_account, |frozen| {
					*frozen = frozen.saturating_sub(amount_from_frozen);
				});
				actual_transfer_amount = actual_transfer_amount.saturating_add(amount_from_frozen);
			}

			// If more is needed, try to use free balance
			let remaining_to_claim = claimable.saturating_sub(actual_transfer_amount);
			if remaining_to_claim > T::Balance::zero() {
				let free_balance = pallet_balances::Pallet::<T>::free_balance(&funding_account);
				let amount_from_free = remaining_to_claim.min(free_balance);

				if amount_from_free > T::Balance::zero() {
					actual_transfer_amount =
						actual_transfer_amount.saturating_add(amount_from_free);
				}
			}

			ensure!(actual_transfer_amount > T::Balance::zero(), Error::<T>::NoFundsAvailable);

			// Transfer from funding_account to beneficiary
			pallet_balances::Pallet::<T>::transfer(
				&funding_account,
				&beneficiary,
				actual_transfer_amount,
				AllowDeath,
			)?;

			// Update claimed amount
			schedule.claimed = schedule.claimed.saturating_add(actual_transfer_amount);
			VestingSchedules::<T>::insert(schedule_id, &schedule);

			// Decrease pending obligations
			PendingObligations::<T>::mutate(&funding_account, |total| {
				*total = total.saturating_sub(actual_transfer_amount);
			});

			// Emit appropriate event
			if actual_transfer_amount < claimable {
				// Partial claim
				Self::deposit_event(Event::PartialClaim {
					beneficiary,
					claimed: actual_transfer_amount,
					remaining_unlocked: claimable.saturating_sub(actual_transfer_amount),
					schedule_id,
				});
			} else {
				// Full claim
				Self::deposit_event(Event::TokensClaimed {
					beneficiary,
					amount: actual_transfer_amount,
					schedule_id,
				});
			}

			Ok(())
		}

		/// Cancel a vesting schedule
		///
		/// Claims any unlocked tokens for the beneficiary, then cancels the schedule.
		/// Remaining obligations are removed and frozen funds are released.
		/// No refund needed - funds never left funding_account.
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::cancel_vesting_schedule())]
		pub fn cancel_vesting_schedule(origin: OriginFor<T>, schedule_id: u64) -> DispatchResult {
			let who = ensure_signed(origin.clone())?;

			let schedule =
				VestingSchedules::<T>::get(schedule_id).ok_or(Error::<T>::ScheduleNotFound)?;
			ensure!(schedule.creator == who, Error::<T>::NotCreator);

			// Try to claim for beneficiary whatever they are currently owed
			// Ignore errors (e.g., nothing to claim or no funds available)
			let _ = Self::claim(origin, schedule_id);

			// Re-fetch schedule after potential claim
			let schedule =
				VestingSchedules::<T>::get(schedule_id).ok_or(Error::<T>::ScheduleNotFound)?;

			// Calculate remaining obligation (not yet unlocked)
			let vested = Self::vested_amount(&schedule).unwrap_or(T::Balance::zero());
			let remaining_obligation = schedule.amount.saturating_sub(vested);

			// Decrease pending obligations by remaining amount
			PendingObligations::<T>::mutate(&schedule.funding_account, |total| {
				*total = total.saturating_sub(remaining_obligation);
			});

			// Try to update freeze (will release if possible)
			let _ = Self::try_freeze_funds(&schedule.funding_account);

			// Store beneficiary before removing schedule
			let beneficiary = schedule.beneficiary.clone();

			// Remove schedule
			VestingSchedules::<T>::remove(schedule_id);

			// Decrement beneficiary schedule count
			BeneficiaryScheduleCount::<T>::mutate(&beneficiary, |count| {
				*count = count.saturating_sub(1);
			});

			// Emit event
			Self::deposit_event(Event::VestingScheduleCancelled { creator: who, schedule_id });
			Ok(())
		}

		/// Create a vesting schedule with cliff (lazy funding)
		///
		/// Nothing unlocks until cliff, then linear vesting begins.
		#[pallet::call_index(3)]
		#[pallet::weight(<T as Config>::WeightInfo::create_vesting_schedule_with_cliff())]
		pub fn create_vesting_schedule_with_cliff(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			amount: T::Balance,
			cliff: T::Moment,
			end: T::Moment,
			funding_account: T::AccountId,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			ensure!(cliff < end, Error::<T>::InvalidSchedule);
			ensure!(amount > T::Balance::zero(), Error::<T>::InvalidSchedule);

			// Check if beneficiary has reached the maximum number of schedules
			let current_count = BeneficiaryScheduleCount::<T>::get(&beneficiary);
			ensure!(
				current_count < T::MaxSchedulesPerBeneficiary::get(),
				Error::<T>::TooManySchedules
			);

			// NO UPFRONT TRANSFER! Increase pending obligations
			PendingObligations::<T>::mutate(&funding_account, |total| {
				*total = total.saturating_add(amount);
			});

			// Try to freeze funds immediately if available
			let _ = Self::try_freeze_funds(&funding_account);

			// Generate unique ID
			let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
			ScheduleCounter::<T>::put(schedule_id);

			// Add the schedule to storage
			let schedule = VestingSchedule {
				creator: who.clone(),
				beneficiary: beneficiary.clone(),
				amount,
				start: cliff, // Start is set to cliff for calculations
				end,
				vesting_type: VestingType::LinearWithCliff { cliff },
				claimed: T::Balance::zero(),
				id: schedule_id,
				funding_account: funding_account.clone(),
			};
			VestingSchedules::<T>::insert(schedule_id, schedule);

			// Increment beneficiary schedule count
			BeneficiaryScheduleCount::<T>::mutate(&beneficiary, |count| {
				*count = count.saturating_add(1);
			});

			Self::deposit_event(Event::VestingScheduleCreated {
				beneficiary,
				amount,
				start: cliff,
				end,
				schedule_id,
				funding_account,
			});
			Ok(())
		}

		/// Create a stepped vesting schedule (lazy funding)
		///
		/// Tokens unlock in discrete steps at regular intervals.
		#[pallet::call_index(4)]
		#[pallet::weight(<T as Config>::WeightInfo::create_stepped_vesting_schedule())]
		pub fn create_stepped_vesting_schedule(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			amount: T::Balance,
			start: T::Moment,
			end: T::Moment,
			step_duration: T::Moment,
			funding_account: T::AccountId,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			ensure!(start < end, Error::<T>::InvalidSchedule);
			ensure!(amount > T::Balance::zero(), Error::<T>::InvalidSchedule);
			ensure!(step_duration > T::Moment::zero(), Error::<T>::InvalidStepDuration);

			let duration = end.saturating_sub(start);
			ensure!(duration >= step_duration, Error::<T>::InvalidSchedule);

			// Check if beneficiary has reached the maximum number of schedules
			let current_count = BeneficiaryScheduleCount::<T>::get(&beneficiary);
			ensure!(
				current_count < T::MaxSchedulesPerBeneficiary::get(),
				Error::<T>::TooManySchedules
			);

			// NO UPFRONT TRANSFER! Increase pending obligations
			PendingObligations::<T>::mutate(&funding_account, |total| {
				*total = total.saturating_add(amount);
			});

			// Try to freeze funds immediately if available
			let _ = Self::try_freeze_funds(&funding_account);

			// Generate unique ID
			let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
			ScheduleCounter::<T>::put(schedule_id);

			// Add the schedule to storage
			let schedule = VestingSchedule {
				creator: who.clone(),
				beneficiary: beneficiary.clone(),
				amount,
				start,
				end,
				vesting_type: VestingType::Stepped { step_duration },
				claimed: T::Balance::zero(),
				id: schedule_id,
				funding_account: funding_account.clone(),
			};
			VestingSchedules::<T>::insert(schedule_id, schedule);

			// Increment beneficiary schedule count
			BeneficiaryScheduleCount::<T>::mutate(&beneficiary, |count| {
				*count = count.saturating_add(1);
			});

			Self::deposit_event(Event::VestingScheduleCreated {
				beneficiary,
				amount,
				start,
				end,
				schedule_id,
				funding_account,
			});
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		// Helper to calculate vested amount
		pub fn vested_amount(
			schedule: &VestingSchedule<T::AccountId, T::Balance, T::Moment>,
		) -> Result<T::Balance, DispatchError> {
			let now = <pallet_timestamp::Pallet<T>>::get();

			match &schedule.vesting_type {
				VestingType::Linear => Self::calculate_linear_vested(
					now,
					schedule.start,
					schedule.end,
					schedule.amount,
				),
				VestingType::LinearWithCliff { cliff } => {
					if now < *cliff {
						Ok(T::Balance::zero())
					} else if now >= schedule.end {
						Ok(schedule.amount)
					} else {
						// Linear vesting from cliff to end
						Self::calculate_linear_vested(now, *cliff, schedule.end, schedule.amount)
					}
				},
				VestingType::Stepped { step_duration } => Self::calculate_stepped_vested(
					now,
					schedule.start,
					schedule.end,
					schedule.amount,
					*step_duration,
				),
			}
		}

		// Calculate linear vesting
		fn calculate_linear_vested(
			now: T::Moment,
			start: T::Moment,
			end: T::Moment,
			amount: T::Balance,
		) -> Result<T::Balance, DispatchError> {
			if now < start {
				return Ok(T::Balance::zero());
			}
			if now >= end {
				return Ok(amount);
			}

			let elapsed = now.saturating_sub(start);
			let duration = end.saturating_sub(start);

			// Convert to u64 for calculation
			let amount_u64: u64 = amount
				.try_into()
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;
			let elapsed_u64: u64 = elapsed
				.try_into()
				.map_err(|_| DispatchError::Other("Moment conversion failed"))?;
			let duration_u64: u64 = duration
				.try_into()
				.map_err(|_| DispatchError::Other("Moment conversion failed"))?;
			let duration_safe: u64 = duration_u64.max(1);

			let vested_u64: u64 = amount_u64
				.saturating_mul(elapsed_u64)
				.checked_div(duration_safe)
				.ok_or(DispatchError::Arithmetic(ArithmeticError::Underflow))?;

			let vested = T::Balance::try_from(vested_u64)
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;

			Ok(vested)
		}

		// Calculate stepped vesting
		fn calculate_stepped_vested(
			now: T::Moment,
			start: T::Moment,
			end: T::Moment,
			amount: T::Balance,
			step_duration: T::Moment,
		) -> Result<T::Balance, DispatchError> {
			if now < start {
				return Ok(T::Balance::zero());
			}
			if now >= end {
				return Ok(amount);
			}

			let elapsed = now.saturating_sub(start);
			let total_duration = end.saturating_sub(start);

			// Convert to u64 for calculation
			let elapsed_u64: u64 = elapsed
				.try_into()
				.map_err(|_| DispatchError::Other("Moment conversion failed"))?;
			let step_duration_u64: u64 = step_duration
				.try_into()
				.map_err(|_| DispatchError::Other("Moment conversion failed"))?;
			let total_duration_u64: u64 = total_duration
				.try_into()
				.map_err(|_| DispatchError::Other("Moment conversion failed"))?;
			let amount_u64: u64 = amount
				.try_into()
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;

			// Calculate number of completed steps
			let steps_passed = elapsed_u64 / step_duration_u64;
			let total_steps = total_duration_u64.div_ceil(step_duration_u64);

			// Calculate vested amount based on completed steps
			let vested_u64 = amount_u64.saturating_mul(steps_passed) / total_steps.max(1);

			let vested = T::Balance::try_from(vested_u64)
				.map_err(|_| DispatchError::Other("Balance conversion failed"))?;

			Ok(vested)
		}

		// Pallet account to "hold" tokens
		pub fn account_id() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}

		/// Try to freeze funds in an account for pending vesting obligations
		///
		/// This function checks how much should be frozen (pending obligations)
		/// and attempts to freeze that amount. If the account has insufficient
		/// balance, it freezes as much as possible.
		///
		/// Called automatically in on_initialize and when creating/canceling schedules.
		pub fn try_freeze_funds(account: &T::AccountId) -> DispatchResult {
			let pending_obligations = PendingObligations::<T>::get(account);
			let currently_frozen = FrozenBalance::<T>::get(account);

			// How much SHOULD be frozen?
			let should_be_frozen = pending_obligations;

			// How much CAN be frozen? (free balance minus ED)
			use frame_support::traits::Currency as CurrencyTrait;
			let free_balance =
				<pallet_balances::Pallet<T> as CurrencyTrait<T::AccountId>>::free_balance(account);
			let min_balance =
				<pallet_balances::Pallet<T> as CurrencyTrait<T::AccountId>>::minimum_balance();
			let freezable = free_balance.saturating_sub(min_balance);

			// Target freeze = min(should, can)
			let target_freeze = should_be_frozen.min(freezable);

			if target_freeze > currently_frozen {
				// Need to freeze MORE
				let to_freeze = target_freeze.saturating_sub(currently_frozen);

				// Use hold/freeze API
				let hold_reason: <T as pallet_balances::Config>::RuntimeHoldReason =
					HoldReason::VestingObligation.into();
				pallet_balances::Pallet::<T>::hold(&hold_reason, account, to_freeze)?;

				FrozenBalance::<T>::insert(account, target_freeze);

				Self::deposit_event(Event::FundsFrozen {
					account: account.clone(),
					amount: to_freeze,
					total_frozen: target_freeze,
				});
			} else if target_freeze < currently_frozen {
				// Need to freeze LESS (obligations decreased)
				let to_unfreeze = currently_frozen.saturating_sub(target_freeze);

				let hold_reason: <T as pallet_balances::Config>::RuntimeHoldReason =
					HoldReason::VestingObligation.into();
				pallet_balances::Pallet::<T>::release(
					&hold_reason,
					account,
					to_unfreeze,
					Precision::BestEffort,
				)?;

				FrozenBalance::<T>::insert(account, target_freeze);

				Self::deposit_event(Event::FundsUnfrozen {
					account: account.clone(),
					amount: to_unfreeze,
					total_frozen: target_freeze,
				});
			}
			// else: target == current, nothing to do

			Ok(())
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

			// UPFRONT TRANSFER for backward compatibility with merkle-airdrop and similar pallets
			// Transfer tokens from source to pallet account
			pallet_balances::Pallet::<T>::transfer(
				source,
				&Self::account_id(),
				amount,
				AllowDeath,
			)?;

			// Generate unique ID
			let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
			ScheduleCounter::<T>::put(schedule_id);

			// Create vesting schedule with pallet account as funding_account
			// No freeze needed - tokens are already securely in pallet account
			let vesting_schedule = VestingSchedule {
				creator: source.clone(),
				beneficiary: dest.clone(),
				amount,
				start: T::Moment::from(start_ms),
				end: T::Moment::from(end_ms),
				vesting_type: VestingType::Linear,
				claimed: T::Balance::zero(),
				id: schedule_id,
				funding_account: Self::account_id(), // Pallet account (upfront funded)
			};
			VestingSchedules::<T>::insert(schedule_id, vesting_schedule);

			// Increment beneficiary schedule count
			BeneficiaryScheduleCount::<T>::mutate(dest, |count| {
				*count = count.saturating_add(1);
			});

			Self::deposit_event(Event::VestingScheduleCreated {
				beneficiary: dest.clone(),
				amount,
				start: T::Moment::from(start_ms),
				end: T::Moment::from(end_ms),
				schedule_id,
				funding_account: Self::account_id(),
			});

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

			// Use 'who' as both beneficiary and funding_account for trait compatibility
			// Increase pending obligations
			PendingObligations::<T>::mutate(who, |total| {
				*total = total.saturating_add(locked);
			});

			// Try to freeze funds immediately if available
			let _ = Self::try_freeze_funds(who);

			// Create vesting schedule with lazy funding
			let vesting_schedule = VestingSchedule {
				creator: who.clone(),
				beneficiary: who.clone(),
				amount: locked,
				start: T::Moment::from(start_ms),
				end: T::Moment::from(end_ms),
				vesting_type: VestingType::Linear,
				claimed: T::Balance::zero(),
				id: schedule_id,
				funding_account: who.clone(), // 'who' is funding themselves
			};
			VestingSchedules::<T>::insert(schedule_id, vesting_schedule);

			// Increment beneficiary schedule count
			BeneficiaryScheduleCount::<T>::mutate(who, |count| {
				*count = count.saturating_add(1);
			});

			Self::deposit_event(Event::VestingScheduleCreated {
				beneficiary: who.clone(),
				amount: locked,
				start: T::Moment::from(start_ms),
				end: T::Moment::from(end_ms),
				schedule_id,
				funding_account: who.clone(),
			});

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
