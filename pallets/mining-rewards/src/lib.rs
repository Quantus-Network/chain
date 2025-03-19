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
	// Import various useful types required by all FRAME pallets.
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use sp_runtime::generic::DigestItem;
	use sp_consensus_pow::POW_ENGINE_ID;
	use codec::Decode;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type WeightInfo: WeightInfo;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A miner has been identified for a block
		MinerRewarded {
			/// Block number
			block: BlockNumberFor<T>,
			/// Miner account
			miner: T::AccountId,
			// TODO add revard details
		},
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(block_number: BlockNumberFor<T>) -> Weight {
			// Extract miner ID from the pre-runtime digest
			if let Some(miner) = Self::extract_miner_from_digest() {

				// Emit an event
				Self::deposit_event(Event::MinerRewarded {
					block: block_number,
					miner: miner.clone()
				});

				log::info!(
                    target: "mining-rewards",
                    "Miner identified for block {:?}: {:?}",
                    block_number,
                    miner
                );
			} else {
				log::warn!(
                    target: "mining-rewards",
                    "Failed to identify miner for block {:?}",
                    block_number
                );
			}

			// Return weight consumed
			Weight::from_parts(10_000, 0)
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		// You can add extrinsics here if needed
	}

	impl<T: Config> Pallet<T> {
		/// Extract miner account ID from the pre-runtime digest
		fn extract_miner_from_digest() -> Option<T::AccountId> {
			// Get the digest from the current block
			let digest = <frame_system::Pallet<T>>::digest();

			// Look for pre-runtime digest with POW_ENGINE_ID
			for log in digest.logs.iter() {
				if let DigestItem::PreRuntime(engine_id, data) = log {
					if engine_id == &POW_ENGINE_ID {
						// Try to decode the miner account ID
						if let Ok(miner) = T::AccountId::decode(&mut &data[..]) {
							return Some(miner);
						}
					}
				}
			}
			None
		}
	}
}