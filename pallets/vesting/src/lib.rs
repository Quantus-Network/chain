#![cfg_attr(not(feature = "std"), no_std)]

//! # Vesting Pallet (pull-based)
//!
//! A minimal "vesting wallet": the pallet's sovereign account (the **pot**) holds the entire
//! unclaimed vesting allocation, endowed at genesis, and beneficiaries are paid by plain
//! transfers only at claim time. No locks, freezes, or holds ever touch a beneficiary account,
//! so any address — including a keyless wormhole address — can be a beneficiary.
//!
//! Each schedule has a globally unique `u64` id; an account may hold any number of schedules.
//! Vesting is linear between `start` and `end` with nothing claimable before `cliff`.
//! Times are milliseconds since the unix epoch, read from `pallet_timestamp`. Genesis
//! schedules may instead store offsets from the first non-zero timestamp (the block-1
//! inherent); the genesis block's `Now` is 0 and is never used as TGE.
//!
//! `claim` is deliberately permissionless: wormhole addresses can never sign and
//! high-security accounts are call-whitelisted, so for both a third-party "ping" is the
//! only claim path. The payout always goes to the stored beneficiary, never the caller.
//!
//! The admin origin (the treasury account, with Root as break-glass) can create schedules
//! (funded from the treasury in the same call), end them early (vested part to the
//! beneficiary, unvested remainder back to the treasury), and retarget a schedule's
//! beneficiary. A retarget replaces the wallet of the *same* grantee — the old address is
//! lost, stolen, or abandoned — so it pays the old address nothing; everything unclaimed
//! follows the schedule to the new wallet.

extern crate alloc;

pub use pallet::*;

/// Smallest leaf-quantum count at which a 4 bps Wormhole volume fee is exact
/// (`2500 * 4 / 10_000 = 1`). Non-final claims pay a multiple of
/// `NON_FINAL_PAYOUT_QUANTA * PayoutQuantum` (25 QTC at the runtime leaf quantum).
pub const NON_FINAL_PAYOUT_QUANTA: u128 = 2_500;

/// Capacity of the genesis schedule table. Offset genesis schedules are rebased in the
/// block-1 timestamp inherent, so this fixed capacity is what keeps that hook constant-time.
pub const MAX_GENESIS_SCHEDULES: u32 = 64;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
mod weights_generated;
pub use weights::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate},
			tokens::Preservation,
			OnTimestampSet, Time,
		},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use qp_wormhole::TransferProofRecorder;
	use sp_arithmetic::{helpers_128bit::multiply_by_rational_with_rounding, Rounding};
	use sp_runtime::{
		traits::{AccountIdConversion, CheckedAdd, CheckedSub, Saturating, Zero},
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
		/// Timestamp of the last successful beneficiary payout.
		pub last_claim_at: Option<Moment>,
	}

	enum ClaimPlan<Balance> {
		Pay(Balance),
		NothingToClaim,
		TooSoon,
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

		/// Records every pallet transfer as a wormhole transfer proof (ZK-tree leaf),
		/// including treasury ↔ pot bookkeeping. The pallet records itself so every
		/// dispatch origin is captured — including Root calls enacted by the scheduler,
		/// which run outside the signed-extrinsic lifecycle and are invisible to the
		/// event-scanning `WormholeProofRecorderExtension`. The extension in turn skips
		/// pot-touching transfer events, so signed paths are not double-recorded.
		type ProofRecorder: qp_wormhole::TransferProofRecorder<
			Self::AccountId,
			Self::AssetId,
			BalanceOf<Self>,
		>;

		/// Wormhole leaf amount quantum. ZK-tree leaves commit `amount / quantum`, so a
		/// payout below one quantum would create a zero-value leaf: funds moved to a
		/// keyless beneficiary would be irrecoverable. Every schedule total must be a
		/// positive multiple of this. Intermediate claims round down further to
		/// [`NON_FINAL_PAYOUT_QUANTA`] leaf quanta; the final claim may be a single
		/// quantum.
		#[pallet::constant]
		type PayoutQuantum: Get<BalanceOf<Self>>;

		/// Minimum elapsed milliseconds between successful claims on one schedule.
		#[pallet::constant]
		type MinClaimInterval: Get<Moment>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	/// Next schedule id to assign. Ids are sequential and never reused.
	#[pallet::storage]
	pub type NextScheduleId<T: Config> = StorageValue<_, u64, ValueQuery>;

	/// All vesting schedules by id. A beneficiary may appear in any number of entries.
	#[pallet::storage]
	pub type Schedules<T: Config> = StorageMap<_, Twox64Concat, u64, VestingScheduleOf<T>>;

	/// Whether genesis schedules are anchored to the first non-zero timestamp, and — once
	/// that timestamp arrives — what it was. Absent when genesis used absolute times.
	#[derive(Encode, Decode, MaxEncodedLen, Clone, Copy, TypeInfo, Debug, PartialEq, Eq)]
	pub enum LaunchAnchor {
		/// Genesis schedules hold offsets awaiting the first non-zero timestamp.
		/// Only ids `0..NextScheduleId` can exist in this state — at most
		/// [`MAX_GENESIS_SCHEDULES`] — because the rebase runs in the block-1 timestamp
		/// inherent, before any extrinsic can create a schedule.
		Pending,
		/// Genesis offsets were rebased onto this unix-ms moment.
		Anchored(Moment),
	}

	#[pallet::storage]
	pub type Launch<T: Config> = StorageValue<_, LaunchAnchor>;

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
		/// A schedule's beneficiary was changed. Nothing was paid out: the retarget
		/// replaces the same grantee's wallet, so the accrued entitlement follows the
		/// schedule to the new address.
		ScheduleRetargeted {
			schedule_id: u64,
			old_beneficiary: T::AccountId,
			new_beneficiary: T::AccountId,
		},
		/// Offset genesis schedules were rebased onto this unix-ms timestamp. The
		/// genesis block's `Now` is 0 and is never used.
		LaunchMomentSet { at: Moment },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// No schedule exists under this id.
		NoSchedule,
		/// Schedule parameters violate `start <= cliff <= end`, `start < end`, or
		/// `total` is not a positive multiple of the payout quantum.
		InvalidSchedule,
		/// Nothing is claimable right now (before the cliff, already fully claimed, or
		/// the accrual rounds down to zero — below one quantum for a final claim,
		/// below the non-final alignment otherwise).
		NothingToClaim,
		/// This schedule has already paid out within the minimum claim interval.
		ClaimTooSoon,
		/// The treasury account is not configured or aliases the vesting pot.
		TreasuryNotConfigured,
		/// The pot does not hold its existential-deposit buffer; endow it first.
		PotUnderfunded,
		/// The beneficiary must not be the pot, and retargeting must change the account.
		InvalidBeneficiary,
		/// The proof recorder reported the transfer credit as dropped: no wormhole leaf
		/// was created, so the transfer is rolled back rather than finalized without a
		/// leaf.
		TransferProofNotRecorded,
	}

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		/// `(beneficiary, start_ms, cliff_ms, end_ms, total)`; ids are assigned
		/// sequentially from 0 in list order. Times are unix-ms unless
		/// [`Self::anchor_to_first_timestamp`] is set, in which case they are offsets
		/// from the first non-zero timestamp. The pot must be endowed (via the balances
		/// genesis) with exactly the sum of totals plus the existential deposit. Fixed
		/// capacity of [`MAX_GENESIS_SCHEDULES`]: the block-1 rebase iterates this table.
		pub schedules: BoundedVec<
			(T::AccountId, Moment, Moment, Moment, u128),
			ConstU32<MAX_GENESIS_SCHEDULES>,
		>,
		/// When true, genesis `start`/`cliff`/`end` are offsets from the first non-zero
		/// timestamp (block 1 inherent). Genesis-block `Now` is 0 and is not used.
		#[serde(default)]
		pub anchor_to_first_timestamp: bool,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			if self.anchor_to_first_timestamp {
				Launch::<T>::put(LaunchAnchor::Pending);
			}
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
						last_claim_at: None,
					},
				);
			}
			NextScheduleId::<T>::put(self.schedules.len() as u64);
			let required = sum
				.checked_add(&ed)
				.expect("vesting genesis: obligations plus existential deposit overflow Balance");
			assert!(
				T::Currency::total_balance(&pot) == required,
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
			assert!(!T::MinClaimInterval::get().is_zero(), "MinClaimInterval must be non-zero");
		}

		#[cfg(feature = "try-runtime")]
		fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
			Self::do_try_state()
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Pay the largest valid claim on `schedule_id` to its beneficiary. Non-final
		/// payouts are rounded down to [`NON_FINAL_PAYOUT_QUANTA`] leaf quanta; the
		/// leftover stays on the schedule until a later claim or the exact final
		/// payout (at least one [`Config::PayoutQuantum`]).
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
				let now = T::TimeProvider::now();
				let payable = match Self::claim_plan(schedule, now)? {
					ClaimPlan::Pay(amount) => amount,
					ClaimPlan::NothingToClaim => return Err(Error::<T>::NothingToClaim.into()),
					ClaimPlan::TooSoon => return Err(Error::<T>::ClaimTooSoon.into()),
				};
				Self::settle(schedule, payable, now)?;
				Self::deposit_event(Event::Claimed {
					schedule_id,
					beneficiary: schedule.beneficiary.clone(),
					amount: payable,
				});
				Ok(())
			})
		}

		/// Create a new schedule under the next free id, moving `total` from the
		/// treasury account into the pot in the same call (recorded as a leaf).
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
			let (treasury, pot) = Self::treasury_and_pot()?;
			ensure!(Self::schedule_is_valid(start, cliff, end, total), Error::<T>::InvalidSchedule);
			ensure!(beneficiary != pot, Error::<T>::InvalidBeneficiary);
			// The pot's ED buffer is what lets keep-alive payouts always clear; a chain
			// launched without genesis schedules must endow the pot before creating any.
			ensure!(
				T::Currency::total_balance(&pot) >= T::Currency::minimum_balance(),
				Error::<T>::PotUnderfunded
			);
			let schedule_id = NextScheduleId::<T>::get();
			let next_id = schedule_id.checked_add(1).ok_or(ArithmeticError::Overflow)?;
			Self::transfer_and_record(&treasury, &pot, total)?;
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
					last_claim_at: None,
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

		/// End a schedule early: the still-unpaid vested part (rounded to the nearest
		/// [`Config::PayoutQuantum`]) goes to the beneficiary if it is at least one
		/// quantum; otherwise that sliver is refunded with the unvested remainder.
		/// Both legs are recorded as wormhole leaves.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::end_schedule())]
		pub fn end_schedule(origin: OriginFor<T>, schedule_id: u64) -> DispatchResult {
			T::AdminOrigin::ensure_origin(origin)?;
			let (treasury, pot) = Self::treasury_and_pot()?;
			let schedule = Schedules::<T>::get(schedule_id).ok_or(Error::<T>::NoSchedule)?;
			let (remaining, owed) = Self::outstanding(&schedule, T::TimeProvider::now())?;
			let quantum = T::PayoutQuantum::get();
			// Nearest multiple: align `owed + quantum/2`. Halves round up. A
			// sub-quantum sliver cannot become a leaf, so it rides to the treasury
			// with the unvested remainder.
			let vested_payout =
				Self::align(owed.saturating_add(quantum / 2u32.saturated_into()), quantum)
					.min(remaining);
			let unvested_refund =
				remaining.checked_sub(&vested_payout).ok_or(ArithmeticError::Underflow)?;
			if !vested_payout.is_zero() {
				Self::transfer_and_record(&pot, &schedule.beneficiary, vested_payout)?;
			}
			if !unvested_refund.is_zero() {
				Self::transfer_and_record(&pot, &treasury, unvested_refund)?;
			}
			Schedules::<T>::remove(schedule_id);
			Self::deposit_event(Event::ScheduleEnded {
				schedule_id,
				beneficiary: schedule.beneficiary,
				vested_paid: vested_payout,
				unvested_returned: unvested_refund,
			});
			Ok(())
		}

		/// Change the schedule's beneficiary without paying anything out. A retarget
		/// replaces the wallet of the *same* grantee (lost-key remedy): the old address
		/// may be lost or stolen, so settling it would burn funds or pay the thief.
		/// Everything vested but unclaimed stays on the schedule and goes to the new
		/// wallet at its next claim. (A permissionless claim landing before the
		/// retarget still pays the old address, so rotate promptly.)
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

		/// Unix-ms TGE origin, once the first non-zero timestamp has rebased offset
		/// genesis schedules. `None` when genesis used absolute times, or before that
		/// inherent.
		pub fn launch_moment() -> Option<Moment> {
			match Launch::<T>::get() {
				Some(LaunchAnchor::Anchored(at)) => Some(at),
				_ => None,
			}
		}

		fn treasury_and_pot() -> Result<(T::AccountId, T::AccountId), Error<T>> {
			let treasury = T::TreasuryAccount::get().ok_or(Error::<T>::TreasuryNotConfigured)?;
			let pot = Self::pot_account_id();
			ensure!(treasury != pot, Error::<T>::TreasuryNotConfigured);
			Ok((treasury, pot))
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
					.expect("validated schedule duration is non-zero");
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
				!total.is_zero() &&
				(total % T::PayoutQuantum::get()).is_zero()
		}

		/// Remaining obligation and vested-but-unpaid amount, before alignment.
		fn outstanding(
			schedule: &VestingScheduleOf<T>,
			now: Moment,
		) -> Result<(BalanceOf<T>, BalanceOf<T>), ArithmeticError> {
			let remaining = schedule
				.total
				.checked_sub(&schedule.claimed)
				.ok_or(ArithmeticError::Underflow)?;
			let owed = Self::vested_amount(schedule, now)
				.checked_sub(&schedule.claimed)
				.ok_or(ArithmeticError::Underflow)?;
			Ok((remaining, owed))
		}

		/// Floor `amount` to a multiple of `quantum`.
		fn align(amount: BalanceOf<T>, quantum: BalanceOf<T>) -> BalanceOf<T> {
			amount.saturating_sub(amount % quantum)
		}

		fn claim_plan(
			schedule: &VestingScheduleOf<T>,
			now: Moment,
		) -> Result<ClaimPlan<BalanceOf<T>>, ArithmeticError> {
			let (remaining, owed) = Self::outstanding(schedule, now)?;
			// `owed == remaining` only when fully vested (`vested == total`), so the
			// final claim pays the exact remainder — quantum-aligned because `total`
			// and `claimed` are. Non-final claims round down to the fee-exact
			// alignment and leave at least one quantum behind by construction. A
			// fully claimed schedule lands in the final branch with zero.
			let payable = if owed == remaining {
				owed
			} else {
				Self::align(
					owed,
					T::PayoutQuantum::get()
						.saturating_mul(NON_FINAL_PAYOUT_QUANTA.saturated_into()),
				)
			};
			if payable.is_zero() {
				return Ok(ClaimPlan::NothingToClaim);
			}
			if let Some(last) = schedule.last_claim_at {
				let elapsed = now.checked_sub(last).ok_or(ArithmeticError::Underflow)?;
				if elapsed < T::MinClaimInterval::get() {
					return Ok(ClaimPlan::TooSoon);
				}
			}
			Ok(ClaimPlan::Pay(payable))
		}

		/// Pay `amount` to the schedule's beneficiary and advance the schedule to match:
		/// the single place a claimable payout is settled, so no path can drift on what
		/// a payout does to `claimed` and `last_claim_at`.
		fn settle(
			schedule: &mut VestingScheduleOf<T>,
			amount: BalanceOf<T>,
			now: Moment,
		) -> DispatchResult {
			Self::transfer_and_record(&Self::pot_account_id(), &schedule.beneficiary, amount)?;
			schedule.claimed =
				schedule.claimed.checked_add(&amount).ok_or(ArithmeticError::Overflow)?;
			schedule.last_claim_at = Some(now);
			Ok(())
		}

		/// Move `amount` from `from` to `to` AND record it as a wormhole transfer
		/// proof — fused so no pallet transfer can move funds without a ZK leaf.
		///
		/// The recorder is the same canonical entry point every recorded transfer on
		/// this chain funnels through. It must be invoked here rather than left to the
		/// event-scanning transaction extension: Root calls enacted by the scheduler
		/// run outside the signed-extrinsic lifecycle and the extension never sees
		/// them. The extension in turn skips pot-touching transfer events, so signed
		/// paths are not double-recorded.
		///
		/// The recorder contract permits `false` for a deliberately dropped credit.
		/// That must be treated as failure here: were the transfer finalized anyway,
		/// the caller would advance `claimed` (or remove the schedule), making the
		/// missing proof unrecoverable. The storage layer rolls the transfer back, so
		/// the call stays retryable. (The runtime's Wormhole recorder always records
		/// nonzero native credits, so this guards the generic recorder boundary
		/// rather than a reachable runtime path.)
		///
		/// No nested `#[transactional]`: every caller is a dispatchable whose storage
		/// layer already rolls back on `Err`, so a failed record undoes the transfer.
		fn transfer_and_record(
			from: &T::AccountId,
			to: &T::AccountId,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			T::Currency::transfer(from, to, amount, Preservation::Preserve)?;
			ensure!(
				T::ProofRecorder::record_transfer_proof(None, from.clone(), to.clone(), amount),
				Error::<T>::TransferProofNotRecorded
			);
			Ok(())
		}

		/// Invariant: when schedules exist, the pot covers all outstanding obligations
		/// plus its ED buffer, and every stored schedule is internally consistent.
		#[cfg(any(feature = "try-runtime", test))]
		pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
			let pot = Self::pot_account_id();
			// State-dependent (the treasury is storage-backed and only known after
			// genesis), so it cannot live in `integrity_test`. The admin calls also
			// re-check it at use via `treasury_and_pot`.
			if let Some(treasury) = T::TreasuryAccount::get() {
				frame_support::ensure!(
					treasury != pot,
					sp_runtime::TryRuntimeError::Other("treasury aliases the vesting pot")
				);
			}
			let next_id = NextScheduleId::<T>::get();
			let mut outstanding: BalanceOf<T> = Zero::zero();
			let mut has_schedules = false;
			for (id, schedule) in Schedules::<T>::iter() {
				has_schedules = true;
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
				let remaining = schedule
					.total
					.checked_sub(&schedule.claimed)
					.ok_or(sp_runtime::TryRuntimeError::Other("claimed exceeds total"))?;
				frame_support::ensure!(
					remaining.is_zero() || remaining >= T::PayoutQuantum::get(),
					sp_runtime::TryRuntimeError::Other(
						"remaining obligation is below one payout quantum"
					)
				);
				frame_support::ensure!(
					schedule.beneficiary != pot,
					sp_runtime::TryRuntimeError::Other("pot is a beneficiary")
				);
				outstanding = outstanding.checked_add(&remaining).ok_or(
					sp_runtime::TryRuntimeError::Other("outstanding obligations overflow"),
				)?;
			}
			let required = if has_schedules {
				outstanding
					.checked_add(&T::Currency::minimum_balance())
					.ok_or(sp_runtime::TryRuntimeError::Other("required pot balance overflows"))?
			} else {
				Zero::zero()
			};
			frame_support::ensure!(
				T::Currency::total_balance(&pot) >= required,
				sp_runtime::TryRuntimeError::Other("pot does not cover outstanding obligations")
			);
			Ok(())
		}
	}

	impl<T: Config> OnTimestampSet<Moment> for Pallet<T> {
		/// Constant-bounded, as the timestamp pallet requires: the loop covers the genesis
		/// table only (no extrinsic runs before the block-1 inherent), which holds at most
		/// [`MAX_GENESIS_SCHEDULES`] entries, and it runs exactly once, in block 1 of a chain
		/// launched with offset schedules. Its reads and writes are deliberately not itemised
		/// in the timestamp inherent's weight: nothing after genesis can grow the table or
		/// reach this path again, so there is no repeatable cost to charge for — the whole
		/// price is one bounded burst in the launch block.
		fn on_timestamp_set(now: Moment) {
			if now.is_zero() || Launch::<T>::get() != Some(LaunchAnchor::Pending) {
				return;
			}
			for id in 0..NextScheduleId::<T>::get() {
				Schedules::<T>::mutate(id, |maybe| {
					if let Some(schedule) = maybe {
						schedule.start = schedule.start.saturating_add(now);
						schedule.cliff = schedule.cliff.saturating_add(now);
						schedule.end = schedule.end.saturating_add(now);
					}
				});
			}
			Launch::<T>::put(LaunchAnchor::Anchored(now));
			Self::deposit_event(Event::LaunchMomentSet { at: now });
		}
	}
}
