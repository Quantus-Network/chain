#![cfg_attr(not(feature = "std"), no_std)]

//! Treasury configuration pallet.
//!
//! Provides TreasuryProvider trait for mining-rewards integration.

pub mod weights;
pub use weights::WeightInfo;

/// Trait for providing treasury account and portion to mining-rewards.
pub trait TreasuryProvider {
	type AccountId;
	fn account_id() -> Self::AccountId;
	fn portion() -> u8;
}

#[frame_support::pallet]
pub mod pallet {
	use super::WeightInfo;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

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

	/// The portion of mining rewards that goes to treasury (0-100).
	#[pallet::storage]
	#[pallet::getter(fn treasury_portion)]
	pub type TreasuryPortion<T: Config> = StorageValue<_, u8, ValueQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub treasury_account: T::AccountId,
		pub treasury_portion: u8,
	}

	impl<T: Config> Default for GenesisConfig<T>
	where
		T::AccountId: From<[u8; 32]>,
	{
		fn default() -> Self {
			Self { treasury_account: [0u8; 32].into(), treasury_portion: 50 }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			assert!(self.treasury_portion <= 100, "Treasury portion must be 0-100");
			TreasuryAccount::<T>::put(self.treasury_account.clone());
			TreasuryPortion::<T>::put(self.treasury_portion);
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		TreasuryAccountUpdated { new_account: T::AccountId },
		TreasuryPortionUpdated { new_portion: u8 },
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Set the treasury account. Root only.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::set_treasury_account())]
		pub fn set_treasury_account(origin: OriginFor<T>, account: T::AccountId) -> DispatchResult {
			ensure_root(origin)?;
			TreasuryAccount::<T>::put(&account);
			Self::deposit_event(Event::TreasuryAccountUpdated { new_account: account });
			Ok(())
		}

		/// Set the treasury portion (0-100). Root only.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::set_treasury_portion())]
		pub fn set_treasury_portion(origin: OriginFor<T>, portion: u8) -> DispatchResult {
			ensure_root(origin)?;
			ensure!(portion <= 100, Error::<T>::InvalidPortion);
			TreasuryPortion::<T>::put(portion);
			Self::deposit_event(Event::TreasuryPortionUpdated { new_portion: portion });
			Ok(())
		}
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidPortion,
	}

	impl<T: Config> Pallet<T> {
		/// Get the treasury account. Returns zero account if not configured.
		pub fn account_id() -> T::AccountId {
			TreasuryAccount::<T>::get().unwrap_or_else(|| {
				T::AccountId::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
					.unwrap_or_else(|_| {
						// Fallback: zero account
						T::AccountId::decode(&mut &[0u8; 32][..])
							.unwrap_or_else(|_| panic!("Cannot create fallback AccountId"))
					})
			})
		}

		/// Get the treasury portion (0-100).
		pub fn portion() -> u8 {
			TreasuryPortion::<T>::get()
		}
	}

	/// Implements `Get<AccountId>` for use as runtime config parameter.
	pub struct TreasuryAccountGetter<T>(core::marker::PhantomData<T>);
	impl<T: Config> frame_support::traits::Get<T::AccountId> for TreasuryAccountGetter<T> {
		fn get() -> T::AccountId {
			Pallet::<T>::account_id()
		}
	}

	/// Implements `Get<u8>` for use as runtime config parameter.
	pub struct TreasuryPortionGetter<T>(core::marker::PhantomData<T>);
	impl<T: Config> frame_support::traits::Get<u8> for TreasuryPortionGetter<T> {
		fn get() -> u8 {
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
	fn portion() -> u8 {
		pallet::Pallet::<T>::portion()
	}
}
