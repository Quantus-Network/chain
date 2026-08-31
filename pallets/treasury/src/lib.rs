#![cfg_attr(not(feature = "std"), no_std)]

//! # Treasury Configuration Pallet
//!
//! This pallet provides a centralized surface for treasury-related runtime parameters
//! that can be adjusted by privileged origins (currently root/governance).
//!
//! ## Purpose & Rationale
//!
//! The treasury is not paid from mining rewards. This pallet stores the account
//! that holds treasury funds (endowments and later spending).
//!
//! This architecture enables:
//!
//! - **Minimal privilege surface**: The technical collective's authority can be limited to a known
//!   set of configuration parameters rather than arbitrary runtime calls.
//! - **Auditability**: All adjustable parameters are explicitly defined in dedicated pallets,
//!   making it clear what can and cannot be changed post-genesis.
//! - **Future extensibility**: As the treasury subsystem grows (e.g., budgets, spending proposals),
//!   this pallet provides a natural home for that logic.
//!
//! ## Current Features
//!
//! - [`TreasuryAccount`]: The account that holds treasury funds.
//! - [`TreasuryProvider`] trait: Account lookup for pallets that spend from or pay to treasury.

pub mod migrations;
pub mod weights;
pub use weights::WeightInfo;

/// Trait for providing the treasury account.
pub trait TreasuryProvider {
	type AccountId;
	fn account_id() -> Self::AccountId;
}

#[frame_support::pallet]
pub mod pallet {
	use super::WeightInfo;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	/// The in-code storage version.
	///
	/// v1: `TreasuryPortion` set to 50% (50/50 treasury/miner split, see `migrations::v1`).
	/// v2: `TreasuryPortion` removed; treasury is not paid from block rewards.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(2);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type WeightInfo: crate::WeightInfo;
	}

	/// The treasury account that holds treasury funds.
	#[pallet::storage]
	#[pallet::getter(fn treasury_account)]
	pub type TreasuryAccount<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub treasury_account: Option<T::AccountId>,
	}

	impl<T: Config> Default for GenesisConfig<T> {
		/// The default deliberately configures nothing: the previous default account,
		/// `[1u8; 32]`, is the runtime's keyless minting sentinel, so a chain spec that
		/// omitted the treasury section silently sent every treasury payout to an
		/// address nobody can sign for. (FRAME requires the impl to exist and to build:
		/// `RuntimeGenesisConfig` derives `Default` from it.)
		fn default() -> Self {
			Self { treasury_account: None }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T>
	where
		T::AccountId: From<[u8; 32]> + PartialEq,
	{
		fn build(&self) {
			// The all-`None` state is the `Default`, which FRAME requires to build (see
			// the `Default` impl above). Write nothing: `account_id()` panics on first
			// use, so a spec that forgot the treasury section still fails loudly.
			let Some(account) = self.treasury_account.as_ref() else {
				return;
			};
			let zero: T::AccountId = [0u8; 32].into();
			assert!(account != &zero, "Treasury account must not be zero address");
			TreasuryAccount::<T>::put(account.clone());
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// The treasury account was updated.
		///
		/// Note: This only redirects where future treasury credits are sent. Any balance
		/// accumulated in the old account remains there and is NOT automatically migrated.
		/// Use a separate balance transfer if funds need to be moved.
		TreasuryAccountUpdated {
			/// The previous treasury account (None if this is the first time setting it).
			old_account: Option<T::AccountId>,
			/// The new treasury account that will receive future credits.
			new_account: T::AccountId,
		},
	}

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		T::AccountId: From<[u8; 32]> + PartialEq,
	{
		/// Set the treasury account. Root only. Zero address is rejected (funds would be locked).
		///
		/// **Important**: This only changes where *future* treasury credits are sent. Any balance
		/// that has already accumulated in the current treasury account is NOT automatically
		/// migrated to the new account. If you need to move existing funds, perform a separate
		/// balance transfer (e.g., via governance proposal) after updating the account.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::set_treasury_account())]
		pub fn set_treasury_account(origin: OriginFor<T>, account: T::AccountId) -> DispatchResult {
			ensure_root(origin)?;
			let zero: T::AccountId = [0u8; 32].into();
			ensure!(account != zero, Error::<T>::InvalidTreasuryAccount);
			let old_account = TreasuryAccount::<T>::get();
			TreasuryAccount::<T>::put(&account);
			Self::deposit_event(Event::TreasuryAccountUpdated {
				old_account,
				new_account: account,
			});
			Ok(())
		}
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Treasury account cannot be zero address (funds would be permanently locked).
		InvalidTreasuryAccount,
	}

	impl<T: Config> Pallet<T> {
		/// Get the treasury account. Panics if not configured (chain misconfigured).
		/// Zero-address check is done in genesis build and set_treasury_account only.
		pub fn account_id() -> T::AccountId {
			TreasuryAccount::<T>::get()
				.expect("Treasury account must be set in genesis; chain is misconfigured")
		}
	}

	/// Implements `Get<AccountId>` for use as runtime config parameter.
	pub struct TreasuryAccountGetter<T>(core::marker::PhantomData<T>);
	impl<T: Config> frame_support::traits::Get<T::AccountId> for TreasuryAccountGetter<T> {
		fn get() -> T::AccountId {
			Pallet::<T>::account_id()
		}
	}
}

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub use pallet::*;

impl<T: pallet::Config> TreasuryProvider for pallet::Pallet<T> {
	type AccountId = T::AccountId;
	fn account_id() -> Self::AccountId {
		pallet::Pallet::<T>::account_id()
	}
}
