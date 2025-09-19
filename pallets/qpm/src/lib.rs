#![cfg_attr(not(feature = "std"), no_std)]

//! Quantus Prediction Markets (QPM) pallet
//! This pallet provides the functionality for creating and managing prediction markets.
//! It allows users to create markets, place bets, and resolve markets based on certain conditions.
//! The pallet also provides a mechanism for reporting and disputing market outcomes.

// Re-export all pallet parts, this is needed to properly import the pallet into the runtime.
extern crate alloc;

use codec::{Decode, Encode};
use frame_support::{
	pallet_prelude::*,
	traits::{
		fungible::{Inspect, Mutate},
		Get,
	},
};
use frame_system::pallet_prelude::*;
use sp_consensus_qpow::BlockInfo;
use sp_runtime::traits::UniqueSaturatedInto;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

/// A single prediction
#[derive(
	Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub struct CompactPrediction<Moment: Ord + PartialOrd, AccountId> {
	/// Prediction moment
	moment: Moment,
	/// Prediction account
	account: AccountId,
}

/// Bounded sorted vector - single storage entry
type PredictionList<AccountId, Moment, MaxPredictions> =
	BoundedVec<CompactPrediction<Moment, AccountId>, MaxPredictions>;
/// Balance type
type BalanceOf<T> =
	<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use sp_runtime::traits::{AtLeast32Bit, BlockNumberProvider, Saturating, Scale};

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Block number provider
		type BlockNumberProvider: BlockNumberProvider<BlockNumber = BlockNumberFor<Self>>;

		/// Get block information
		type BlockTimeInfo: sp_consensus_qpow::BlockInfo<BlockNumberFor<Self>, Self::Moment>;

		/// Currency type
		type Currency: Mutate<Self::AccountId>;

		/// Type that represents the moment in time
		type Moment: Parameter
			+ Default
			+ AtLeast32Bit
			+ Scale<BlockNumberFor<Self>, Output = Self::Moment>
			+ Copy
			+ MaxEncodedLen
			+ scale_info::StaticTypeInfo;

		/// Prediction deposit amount, flat
		#[pallet::constant]
		type PredictionDepositAmount: Get<BalanceOf<Self>>;

		/// Block buffer time. How many blocks in the future can predictions be made for?
		///
		/// This value determines the minimum number of blocks in the future for which predictions
		/// can be made.
		#[pallet::constant]
		type BlockBufferTime: Get<BlockNumberFor<Self>>;

		/// Constant address for pool
		#[pallet::constant]
		type PoolAddress: Get<Self::AccountId>;

		/// Max predictions for a block
		#[pallet::constant]
		type MaxPredictions: Get<u32>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Predictions for the block
	#[pallet::storage]
	pub type Predictions<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		BlockNumberFor<T>,
		PredictionList<T::AccountId, T::Moment, T::MaxPredictions>,
		ValueQuery,
	>;

	#[pallet::error]
	pub enum Error<T> {
		/// Prediction too early
		PredictionTooEarly,
		/// Exceeded max predictions for a block
		TooManyPredictions,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Prediction made
		PredictionMade {
			block_number: BlockNumberFor<T>,
			prediction: CompactPrediction<T::Moment, T::AccountId>,
		},
		/// Prediction resolved
		PredictionResolved {
			block_number: BlockNumberFor<T>,
			prediction: CompactPrediction<T::Moment, T::AccountId>,
		},
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight(10_000)]
		pub fn predict(
			origin: OriginFor<T>,
			block_number: BlockNumberFor<T>,
			timestamp: T::Moment,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// ensure block number is in the future
			let current_block = T::BlockNumberProvider::current_block_number();

			ensure!(
				current_block.saturating_add(T::BlockBufferTime::get()) <= block_number,
				Error::<T>::PredictionTooEarly
			);

			// Transfer funds from the user to the pool address
			T::Currency::transfer(
				&who,
				&T::PoolAddress::get(),
				T::PredictionDepositAmount::get(),
				frame_support::traits::tokens::Preservation::Preserve,
			)?;

			Predictions::<T>::try_mutate(block_number, |predictions| -> DispatchResult {
				predictions
					.try_push(CompactPrediction { moment: timestamp, account: who })
					.map_err(|_| Error::<T>::TooManyPredictions)?;

				Ok(())
			})?;

			Ok(())
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_now: BlockNumberFor<T>) -> Weight {
			Weight::from_parts(10_000, 0)
		}

		fn on_finalize(now: BlockNumberFor<T>) {
			if let Some((prediction, total_predictions)) = Self::resolve_predictions(now) {
				if let Ok(_) = T::Currency::transfer(
					&T::PoolAddress::get(),
					&prediction.account,
					T::PredictionDepositAmount::get().saturating_mul(
						total_predictions.saturating_sub(One::one()).unique_saturated_into(),
					),
					frame_support::traits::tokens::Preservation::Preserve,
				) {
					Pallet::<T>::deposit_event(Event::PredictionResolved {
						block_number: now,
						prediction,
					});
				} else {
					log::error!(target: "qpm", "Failed to transfer prediction deposit");
				}
			}
		}
	}
}

impl<T: Config> Pallet<T> {
	fn resolve_predictions(
		block_number: BlockNumberFor<T>,
	) -> Option<(CompactPrediction<T::Moment, T::AccountId>, u32)> {
		let predictions = Predictions::<T>::get(block_number);

		if predictions.is_empty() {
			return None;
		}

		let total_predictions = predictions.len() as u32;
		let actual_block_time = T::BlockTimeInfo::block_time(block_number);

		let insert_index = predictions.binary_search_by_key(&actual_block_time, |pred| pred.moment);

		match insert_index {
			Ok(exact_index) => Some((predictions[exact_index].clone(), total_predictions)),
			Err(insert_index) => {
				// get two neighbors and compare
				let prev_prediction = predictions.get(insert_index.checked_sub(1)?).cloned();
				let next_prediction = predictions.get(insert_index).cloned();

				match (prev_prediction, next_prediction) {
					(Some(prev), Some(next)) => {
						let prev_moment: u128 = prev.moment.unique_saturated_into();
						let next_moment: u128 = next.moment.unique_saturated_into();
						let prev_distance =
							prev_moment.abs_diff(actual_block_time.unique_saturated_into());
						let next_distance =
							next_moment.abs_diff(actual_block_time.unique_saturated_into());

						if prev_distance < next_distance {
							Some((prev, total_predictions))
						} else {
							Some((next, total_predictions))
						}
					},
					(Some(prev), None) => Some((prev, total_predictions)),
					(None, Some(next)) => Some((next, total_predictions)),
					(None, None) => None,
				}
			},
		}
	}
}
