#![cfg_attr(not(feature = "std"), no_std)]

//! # Vesting Pallet (pull-based)
//!
//! A minimal "vesting wallet": the pallet's sovereign account (the **pot**) holds the entire
//! unclaimed vesting allocation, endowed at genesis, and beneficiaries are paid by plain
//! transfers only at claim time. No locks, freezes, or holds ever touch a beneficiary account,
//! so any address — including a keyless wormhole address — can be a beneficiary.
//!
//! Each schedule has a globally unique `u64` id; an account may hold any number of schedules.
//! Vesting is linear between `start` and `end` with nothing claimable before `cliff`
//! (all timestamps are milliseconds since the unix epoch, read from `pallet_timestamp`).
//!
//! `claim` is deliberately permissionless: wormhole addresses can never sign and
//! high-security accounts are call-whitelisted, so for both a third-party "ping" is the
//! only claim path. The payout always goes to the stored beneficiary, never the caller.
//!
//! The admin origin (the treasury account, with Root as break-glass) can create schedules
//! (funded from the treasury in the same call), end them early (vested part to the
//! beneficiary, unvested remainder back to the treasury), and retarget a schedule's
//! beneficiary (lost-key remedy).

extern crate alloc;

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
	use alloc::vec::Vec;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate},
			tokens::Preservation,
			Time,
		},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use qp_wormhole::TransferProofRecorder;
	use sp_arithmetic::{helpers_128bit::multiply_by_rational_with_rounding, Rounding};
	use sp_runtime::{
		traits::{AccountIdConversion, CheckedAdd, Saturating, Zero},
		ArithmeticError, SaturatedConversion,
	};

	pub(crate) type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;
	pub type VestingScheduleOf<T> =
		VestingSchedule<<T as frame_system::Config>::AccountId, BalanceOf<T>>;

	/// Milliseconds since the unix epoch, as reported by `pallet_timestamp`.
	pub type Moment = u64;

	/// A single vesting grant. `claimed` only ever grows and never exceeds `total`.
	#[derive(Encode, Decode, MaxEncodedLen, Clone, TypeInfo, Debug, PartialEq, Eq)]
	pub struct VestingSchedule<AccountId, Balance> {
		/// Account the pot pays out to. Admin-retargetable (lost-key remedy).
		pub beneficiary: AccountId,
		/// When linear accrual starts (ms since unix epoch).
		pub start: Moment,
		/// Before this moment nothing is claimable; at it, the amount accrued since
		/// `start` unlocks at once. `start <= cliff <= end`.
		pub cliff: Moment,
		/// When the full `total` is vested. `start < end`.
		pub end: Moment,
		/// Total grant size.
		pub total: Balance,
		/// Already paid out.
		pub claimed: Balance,
	}

	/// The in-code storage version.
	///
	/// This pallet is deployed on fresh chains only: genesis endows the pot and seeds
	/// the schedule table. There is deliberately no upgrade migration — if the pallet
	/// ever were added to a live chain in place, the pot would simply start unfunded
	/// and `create_schedule` fails loudly with [`Error::PotUnderfunded`] until the
	/// treasury sends the pot its existential-deposit buffer.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The native currency. Payouts are plain transfers — never locks/holds/freezes.
		type Currency: Inspect<Self::AccountId> + Mutate<Self::AccountId>;

		/// Wall-clock source (`pallet_timestamp`), milliseconds since the unix epoch.
		type TimeProvider: Time<Moment = Moment>;

		/// Derives the pot's sovereign account.
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		/// Origin allowed to create, end, and retarget schedules
		/// (Root or signed-by-treasury in the runtime).
		type AdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// The configured treasury account: funding source for `create_schedule` and
		/// destination for unvested remainders. `None` if the chain was started without
		/// a treasury, in which case admin calls fail loudly.
		type TreasuryAccount: Get<Option<Self::AccountId>>;

		/// Asset id type forwarded to the proof recorder (payouts are always native:
		/// `None`).
		type AssetId;

		/// Records beneficiary payouts as wormhole transfer proofs (ZK-tree leaves).
		///
		/// The pallet records its payouts itself so that they are captured on **every**
		/// dispatch origin — including Root calls enacted by the scheduler, which run
		/// outside the signed-extrinsic lifecycle and are invisible to the event-scanning
		/// `WormholeProofRecorderExtension`. The extension in turn skips pot-sourced
		/// transfer events, so signed paths are not double-recorded.
		type ProofRecorder: qp_wormhole::TransferProofRecorder<
			Self::AccountId,
			Self::AssetId,
			BalanceOf<Self>,
		>;

		/// Wormhole leaf amount quantum. ZK-tree leaves commit `amount / quantum`, so a
		/// payout below one quantum would create a zero-value leaf: funds moved to a
		/// keyless beneficiary would be irrecoverable. Every schedule total must be a
		/// multiple of this, and every payout is rounded down to a multiple.
		#[pallet::constant]
		type PayoutQuantum: Get<BalanceOf<Self>>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	/// Next schedule id to assign. Ids are sequential and never reused.
	#[pallet::storage]
	pub type NextScheduleId<T: Config> = StorageValue<_, u64, ValueQuery>;

	/// All vesting schedules by id. A beneficiary may appear in any number of entries.
	#[pallet::storage]
	pub type Schedules<T: Config> = StorageMap<_, Twox64Concat, u64, VestingScheduleOf<T>>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A new schedule was created and the pot funded from the treasury.
		ScheduleCreated {
			schedule_id: u64,
			beneficiary: T::AccountId,
			start: Moment,
			cliff: Moment,
			end: Moment,
			total: BalanceOf<T>,
		},
		/// Vested funds were paid out to the beneficiary.
		Claimed { schedule_id: u64, beneficiary: T::AccountId, amount: BalanceOf<T> },
		/// A schedule was ended early: unpaid vested part to the beneficiary,
		/// unvested remainder back to the treasury.
		ScheduleEnded {
			schedule_id: u64,
			beneficiary: T::AccountId,
			vested_paid: BalanceOf<T>,
			unvested_returned: BalanceOf<T>,
		},
		/// A schedule's beneficiary was changed.
		ScheduleRetargeted {
			schedule_id: u64,
			old_beneficiary: T::AccountId,
			new_beneficiary: T::AccountId,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// No schedule exists under this id.
		NoSchedule,
		/// Schedule parameters violate `start <= cliff <= end`, `start < end`,
		/// `total >= existential deposit`, or `total` is not a multiple of the payout
		/// quantum.
		InvalidSchedule,
		/// Nothing is claimable right now (before the cliff, already fully claimed, or
		/// less than one payout quantum accrued).
		NothingToClaim,
		/// The treasury account is not configured on this chain.
		TreasuryNotConfigured,
		/// The pot does not hold its existential-deposit buffer; endow it first.
		PotUnderfunded,
		/// The beneficiary must not be the pot, and retargeting must change the account.
		InvalidBeneficiary,
	}

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		/// `(beneficiary, start_ms, cliff_ms, end_ms, total)`; ids are assigned
		/// sequentially from 0 in list order. The pot must be endowed (via the balances
		/// genesis) with exactly the sum of totals plus the existential deposit.
		pub schedules: Vec<(T::AccountId, Moment, Moment, Moment, u128)>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			if self.schedules.is_empty() {
				return;
			}
			let pot = Pallet::<T>::pot_account_id();
			let ed = T::Currency::minimum_balance();
			let mut sum: BalanceOf<T> = Zero::zero();
			for (i, (beneficiary, start, cliff, end, total)) in self.schedules.iter().enumerate() {
				let total: BalanceOf<T> = (*total)
					.try_into()
					.ok()
					.expect("vesting genesis: total does not fit the Balance type");
				assert!(
					Pallet::<T>::schedule_is_valid(*start, *cliff, *end, total),
					"vesting genesis: invalid schedule at index {i}"
				);
				assert!(
					beneficiary != &pot,
					"vesting genesis: the pot cannot be a beneficiary (index {i})"
				);
				sum = sum
					.checked_add(&total)
					.expect("vesting genesis: sum of totals overflows Balance");
				Schedules::<T>::insert(
					i as u64,
					VestingSchedule {
						beneficiary: beneficiary.clone(),
						start: *start,
						cliff: *cliff,
						end: *end,
						total,
						claimed: Zero::zero(),
					},
				);
			}
			NextScheduleId::<T>::put(self.schedules.len() as u64);
			assert!(
				T::Currency::total_balance(&pot) == sum.saturating_add(ed),
				"vesting genesis: pot balance must equal sum of schedule totals plus the \
				 existential deposit"
			);
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			assert!(
				!T::PayoutQuantum::get().is_zero(),
				"PayoutQuantum must be non-zero (it is a divisor)"
			);
		}

		#[cfg(feature = "try-runtime")]
		fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
			Self::do_try_state()
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Pay out everything currently claimable on `schedule_id` to its beneficiary,
		/// rounded down to a multiple of [`Config::PayoutQuantum`] (sub-quantum payouts
		/// would create zero-value wormhole leaves and strand funds on keyless
		/// beneficiaries).
		///
		/// Permissionless: any signed account may call this for any schedule; the payout
		/// always goes to the stored beneficiary. This is the only claim path for
		/// beneficiaries that cannot sign (wormhole addresses, high-security accounts).
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::claim())]
		pub fn claim(origin: OriginFor<T>, schedule_id: u64) -> DispatchResult {
			ensure_signed(origin)?;
			Schedules::<T>::try_mutate(schedule_id, |maybe_schedule| {
				let schedule = maybe_schedule.as_mut().ok_or(Error::<T>::NoSchedule)?;
				let vested = Self::vested_amount(schedule, T::TimeProvider::now());
				let owed = vested.saturating_sub(schedule.claimed);
				let payable = Self::quantize_down(owed);
				ensure!(!payable.is_zero(), Error::<T>::NothingToClaim);
				Self::pay_out(&Self::pot_account_id(), &schedule.beneficiary, payable)?;
				// `claimed` advances only by the transferred amount, so it stays
				// quantum-aligned; totals are quantum-aligned too, hence the final claim
				// at `end` pays out exactly and no dust is ever left behind.
				schedule.claimed = schedule.claimed.saturating_add(payable);
				Self::deposit_event(Event::Claimed {
					schedule_id,
					beneficiary: schedule.beneficiary.clone(),
					amount: payable,
				});
				Ok(())
			})
		}

		/// Create a new schedule under the next free id, moving `total` from the
		/// treasury account into the pot in the same call.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::create_schedule())]
		pub fn create_schedule(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			start: Moment,
			cliff: Moment,
			end: Moment,
			total: BalanceOf<T>,
		) -> DispatchResult {
			T::AdminOrigin::ensure_origin(origin)?;
			let treasury = T::TreasuryAccount::get().ok_or(Error::<T>::TreasuryNotConfigured)?;
			let pot = Self::pot_account_id();
			ensure!(Self::schedule_is_valid(start, cliff, end, total), Error::<T>::InvalidSchedule);
			ensure!(beneficiary != pot, Error::<T>::InvalidBeneficiary);
			// A treasury misconfigured to be the pot itself would record an obligation
			// without funding it, silently corrupting the pot's accounting invariant.
			ensure!(treasury != pot, Error::<T>::TreasuryNotConfigured);
			// The pot's ED buffer is what lets keep-alive payouts always clear; a chain
			// launched without genesis schedules must endow the pot before creating any.
			ensure!(
				T::Currency::total_balance(&pot) >= T::Currency::minimum_balance(),
				Error::<T>::PotUnderfunded
			);
			let schedule_id = NextScheduleId::<T>::get();
			let next_id = schedule_id.checked_add(1).ok_or(ArithmeticError::Overflow)?;
			T::Currency::transfer(&treasury, &pot, total, Preservation::Preserve)?;
			NextScheduleId::<T>::put(next_id);
			Schedules::<T>::insert(
				schedule_id,
				VestingSchedule {
					beneficiary: beneficiary.clone(),
					start,
					cliff,
					end,
					total,
					claimed: Zero::zero(),
				},
			);
			Self::deposit_event(Event::ScheduleCreated {
				schedule_id,
				beneficiary,
				start,
				cliff,
				end,
				total,
			});
			Ok(())
		}

		/// End a schedule early: the still-unpaid vested part (rounded down to a
		/// [`Config::PayoutQuantum`] multiple) goes to the beneficiary, everything else
		/// this schedule still holds — the unvested remainder plus any sub-quantum
		/// vested dust — returns to the treasury, and the schedule is removed. The
		/// treasury is signature-controlled and needs no wormhole leaf, so dust is safe
		/// there but would be stranded on a keyless beneficiary.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::end_schedule())]
		pub fn end_schedule(origin: OriginFor<T>, schedule_id: u64) -> DispatchResult {
			T::AdminOrigin::ensure_origin(origin)?;
			let treasury = T::TreasuryAccount::get().ok_or(Error::<T>::TreasuryNotConfigured)?;
			let schedule = Schedules::<T>::get(schedule_id).ok_or(Error::<T>::NoSchedule)?;
			let pot = Self::pot_account_id();
			let vested = Self::vested_amount(&schedule, T::TimeProvider::now());
			let vested_paid = Self::quantize_down(vested.saturating_sub(schedule.claimed));
			let unvested_returned =
				schedule.total.saturating_sub(schedule.claimed).saturating_sub(vested_paid);
			if !vested_paid.is_zero() {
				Self::pay_out(&pot, &schedule.beneficiary, vested_paid)?;
			}
			if !unvested_returned.is_zero() {
				T::Currency::transfer(&pot, &treasury, unvested_returned, Preservation::Preserve)?;
			}
			Schedules::<T>::remove(schedule_id);
			Self::deposit_event(Event::ScheduleEnded {
				schedule_id,
				beneficiary: schedule.beneficiary,
				vested_paid,
				unvested_returned,
			});
			Ok(())
		}

		/// Change a schedule's beneficiary; everything else, including `claimed`, is
		/// untouched. Remedy for a lost key or migration to a multisig.
		#[pallet::call_index(3)]
		#[pallet::weight(T::WeightInfo::retarget_schedule())]
		pub fn retarget_schedule(
			origin: OriginFor<T>,
			schedule_id: u64,
			new_beneficiary: T::AccountId,
		) -> DispatchResult {
			T::AdminOrigin::ensure_origin(origin)?;
			ensure!(new_beneficiary != Self::pot_account_id(), Error::<T>::InvalidBeneficiary);
			Schedules::<T>::try_mutate(schedule_id, |maybe_schedule| {
				let schedule = maybe_schedule.as_mut().ok_or(Error::<T>::NoSchedule)?;
				ensure!(new_beneficiary != schedule.beneficiary, Error::<T>::InvalidBeneficiary);
				let old_beneficiary =
					core::mem::replace(&mut schedule.beneficiary, new_beneficiary.clone());
				Self::deposit_event(Event::ScheduleRetargeted {
					schedule_id,
					old_beneficiary,
					new_beneficiary,
				});
				Ok(())
			})
		}
	}

	impl<T: Config> Pallet<T> {
		/// The pot: the pallet's sovereign account holding all unclaimed vesting funds.
		pub fn pot_account_id() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}

		/// Amount vested at `now`: 0 before the cliff, `total` from `end`, linear in
		/// between (floor rounding; the `end` branch guarantees exactness, the final
		/// claim absorbs rounding dust).
		pub fn vested_amount(schedule: &VestingScheduleOf<T>, now: Moment) -> BalanceOf<T> {
			if now < schedule.cliff {
				return Zero::zero();
			}
			if now >= schedule.end {
				return schedule.total;
			}
			// Here `cliff <= now < end`, and `start <= cliff`, so both differences are
			// in range and `duration > 0`.
			let elapsed = u128::from(now.saturating_sub(schedule.start));
			let duration = u128::from(schedule.end.saturating_sub(schedule.start));
			let total: u128 = schedule.total.saturated_into();
			// 256-bit internally: exact for the whole input domain. `None` only on a
			// zero divisor, which the branches above rule out.
			let vested =
				multiply_by_rational_with_rounding(total, elapsed, duration, Rounding::Down)
					.unwrap_or(total);
			vested.saturated_into()
		}

		fn schedule_is_valid(
			start: Moment,
			cliff: Moment,
			end: Moment,
			total: BalanceOf<T>,
		) -> bool {
			start <= cliff &&
				cliff <= end && start < end &&
				total >= T::Currency::minimum_balance() &&
				(total % T::PayoutQuantum::get()).is_zero()
		}

		/// Round down to a multiple of the payout quantum.
		fn quantize_down(amount: BalanceOf<T>) -> BalanceOf<T> {
			amount.saturating_sub(amount % T::PayoutQuantum::get())
		}

		/// Move a payout out of the pot AND record it as a wormhole transfer proof —
		/// fused into one function so no payout path can move funds without creating
		/// the ZK proof material a wormhole beneficiary needs to exit.
		///
		/// The recorder is the same canonical entry point every recorded transfer on
		/// this chain funnels through. It must be invoked here, with the payout, rather
		/// than left to the event-scanning transaction extension: Root calls enacted by
		/// the scheduler run outside the signed-extrinsic lifecycle and the extension
		/// never sees them. The extension in turn skips pot-sourced transfer events, so
		/// signed paths are not double-recorded.
		fn pay_out(
			pot: &T::AccountId,
			beneficiary: &T::AccountId,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			T::Currency::transfer(pot, beneficiary, amount, Preservation::Preserve)?;
			T::ProofRecorder::record_transfer_proof(None, pot.clone(), beneficiary.clone(), amount);
			Ok(())
		}

		/// Invariant: the pot covers all outstanding obligations plus its ED buffer, and
		/// every stored schedule is internally consistent.
		#[cfg(any(feature = "try-runtime", test))]
		pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
			let pot = Self::pot_account_id();
			let next_id = NextScheduleId::<T>::get();
			let mut outstanding: BalanceOf<T> = Zero::zero();
			for (id, schedule) in Schedules::<T>::iter() {
				frame_support::ensure!(
					id < next_id,
					sp_runtime::TryRuntimeError::Other("schedule id >= NextScheduleId")
				);
				frame_support::ensure!(
					Self::schedule_is_valid(
						schedule.start,
						schedule.cliff,
						schedule.end,
						schedule.total
					),
					sp_runtime::TryRuntimeError::Other("invalid stored schedule")
				);
				frame_support::ensure!(
					schedule.claimed <= schedule.total,
					sp_runtime::TryRuntimeError::Other("claimed exceeds total")
				);
				frame_support::ensure!(
					(schedule.claimed % T::PayoutQuantum::get()).is_zero(),
					sp_runtime::TryRuntimeError::Other("claimed is not quantum-aligned")
				);
				frame_support::ensure!(
					schedule.beneficiary != pot,
					sp_runtime::TryRuntimeError::Other("pot is a beneficiary")
				);
				outstanding =
					outstanding.saturating_add(schedule.total.saturating_sub(schedule.claimed));
			}
			frame_support::ensure!(
				T::Currency::total_balance(&pot) >=
					outstanding.saturating_add(T::Currency::minimum_balance()),
				sp_runtime::TryRuntimeError::Other("pot does not cover outstanding obligations")
			);
			Ok(())
		}
	}
}
