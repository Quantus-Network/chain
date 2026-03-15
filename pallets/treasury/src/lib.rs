#![cfg_attr(not(feature = "std"), no_std)]

//! Treasury configuration pallet.
//!
//! Provides TreasuryProvider trait for mining-rewards integration.

pub mod weights;
pub use weights::WeightInfo;

use sp_runtime::Permill;

/// Trait for providing treasury account and portion to mining-rewards.
pub trait TreasuryProvider {
	type AccountId;
	fn account_id() -> Self::AccountId;
	fn portion() -> Permill;
}

#[frame_support::pallet]
pub mod pallet {
	use super::WeightInfo;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use sp_runtime::Permill;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type WeightInfo: crate::WeightInfo;
	}

	/// The treasury account that receives mining rewards.
	#[pallet::storage]
	#[pallet::getter(fn treasury_account)]
	pub type TreasuryAccount<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

	/// The portion of mining rewards that goes to treasury (Permill, 0–100%).
	/// Uses OptionQuery so genesis is required. Permill allows fine granularity (e.g. 33.3%).
	#[pallet::storage]
	#[pallet::getter(fn treasury_portion)]
	pub type TreasuryPortion<T: Config> = StorageValue<_, Permill, OptionQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub treasury_account: Option<T::AccountId>,
		pub treasury_portion: Option<Permill>,
	}

	impl<T: Config> Default for GenesisConfig<T>
	where
		T::AccountId: From<[u8; 32]>,
	{
		/// Default for test runtimes and chain specs that omit treasury.
		/// Production uses genesis_config_presets which set treasury explicitly.
		fn default() -> Self {
			Self {
				treasury_account: Some([1u8; 32].into()),
				treasury_portion: Some(Permill::from_percent(50)),
			}
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T>
	where
		T::AccountId: From<[u8; 32]> + PartialEq,
	{
		fn build(&self) {
			let account = self
				.treasury_account
				.as_ref()
				.expect("Treasury account must be set in genesis; chain is misconfigured");
			let portion = self
				.treasury_portion
				.as_ref()
				.expect("Treasury portion must be set in genesis; chain is misconfigured");
			assert!(*portion <= Permill::one(), "Treasury portion must be <= 100%");
			let zero: T::AccountId = [0u8; 32].into();
			assert!(account != &zero, "Treasury account must not be zero address");
			TreasuryAccount::<T>::put(account.clone());
			TreasuryPortion::<T>::put(*portion);
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		TreasuryAccountUpdated { new_account: T::AccountId },
		TreasuryPortionUpdated { new_portion: Permill },
	}

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		T::AccountId: From<[u8; 32]> + PartialEq,
	{
		/// Set the treasury account. Root only. Zero address is rejected (funds would be locked).
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::set_treasury_account())]
		pub fn set_treasury_account(origin: OriginFor<T>, account: T::AccountId) -> DispatchResult {
			ensure_root(origin)?;
			let zero: T::AccountId = [0u8; 32].into();
			ensure!(account != zero, Error::<T>::InvalidTreasuryAccount);
			TreasuryAccount::<T>::put(&account);
			Self::deposit_event(Event::TreasuryAccountUpdated { new_account: account });
			Ok(())
		}

		/// Set the treasury portion (Permill, 0–100%). Root only.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::set_treasury_portion())]
		pub fn set_treasury_portion(origin: OriginFor<T>, portion: Permill) -> DispatchResult {
			ensure_root(origin)?;
			ensure!(portion <= Permill::one(), Error::<T>::InvalidPortion);
			TreasuryPortion::<T>::put(portion);
			Self::deposit_event(Event::TreasuryPortionUpdated { new_portion: portion });
			Ok(())
		}
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidPortion,
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

		/// Get the treasury portion (Permill). Panics if not configured (chain misconfigured).
		pub fn portion() -> Permill {
			TreasuryPortion::<T>::get()
				.expect("Treasury portion must be set in genesis; chain is misconfigured")
		}
	}

	/// Implements `Get<AccountId>` for use as runtime config parameter.
	pub struct TreasuryAccountGetter<T>(core::marker::PhantomData<T>);
	impl<T: Config> frame_support::traits::Get<T::AccountId> for TreasuryAccountGetter<T> {
		fn get() -> T::AccountId {
			Pallet::<T>::account_id()
		}
	}

	/// Implements `Get<Permill>` for use as runtime config parameter.
	pub struct TreasuryPortionGetter<T>(core::marker::PhantomData<T>);
	impl<T: Config> frame_support::traits::Get<Permill> for TreasuryPortionGetter<T> {
		fn get() -> Permill {
			Pallet::<T>::portion()
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
	fn portion() -> Permill {
		pallet::Pallet::<T>::portion()
	}
}
