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
	use core::marker::PhantomData;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate},
			Defensive, Get, Imbalance, OnUnbalanced,
		},
	};
	use frame_system::pallet_prelude::*;
	use qp_wormhole::TransferProofs;
	use sp_consensus_pow::POW_ENGINE_ID;
	use sp_runtime::{
		generic::DigestItem,
		traits::{AccountIdConversion, Saturating},
	};

	pub(crate) type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	#[pallet::getter(fn collected_fees)]
	pub(super) type CollectedFees<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Currency type that also stores zk proofs
		type Currency: Mutate<Self::AccountId>
			+ qp_wormhole::TransferProofs<BalanceOf<Self>, Self::AccountId>;

		/// The base block reward given to miners
		#[pallet::constant]
		type MinerBlockReward: Get<BalanceOf<Self>>;

		/// The base block reward given to treasury
		#[pallet::constant]
		type TreasuryBlockReward: Get<BalanceOf<Self>>;

		/// The treasury pallet ID
		#[pallet::constant]
		type TreasuryPalletId: Get<frame_support::PalletId>;

		/// Account ID used as the "from" account when creating transfer proofs for minted tokens
		#[pallet::constant]
		type MintingAccount: Get<Self::AccountId>;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A miner has been identified for a block
		MinerRewarded {
			/// Miner account
			miner: T::AccountId,
			/// Total reward (base + fees)
			reward: BalanceOf<T>,
		},
		/// Transaction fees were collected for later distribution
		FeesCollected {
			/// The amount collected
			amount: BalanceOf<T>,
			/// Total fees waiting for distribution
			total: BalanceOf<T>,
		},
		/// Rewards were sent to Treasury when no miner was specified
		TreasuryRewarded {
			/// Total reward (base + fees)
			reward: BalanceOf<T>,
		},
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
			// Return weight consumed for on finalize hook
			<T as crate::pallet::Config>::WeightInfo::on_finalize_rewarded_miner()
		}

		fn on_finalize(_block_number: BlockNumberFor<T>) {
			// Get the block rewards
			let miner_reward = T::MinerBlockReward::get();
			let treasury_reward = T::TreasuryBlockReward::get();
			let tx_fees = <CollectedFees<T>>::take();

			// Extract miner ID from the pre-runtime digest
			let miner = Self::extract_miner_from_digest();

			log::debug!(target: "mining-rewards", "💰 Base reward: {:?}", miner_reward);
			log::debug!(target: "mining-rewards", "💰 Original Tx_fees: {:?}", tx_fees);

			// Send fees to miner if any
			if tx_fees > Zero::zero() {
				Self::mint_reward(miner.clone(), tx_fees);
			}

			// Send rewards separately for accounting
			Self::mint_reward(miner, miner_reward);

			// Send treasury reward
			Self::mint_reward(None, treasury_reward);
		}
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
						// Try to decode the accountId
						// TODO: to enforce miner wormholes, decode inner hash here
						if let Ok(miner) = T::AccountId::decode(&mut &data[..]) {
							return Some(miner);
						}
					}
				}
			}
			None
		}

		pub fn collect_transaction_fees(fees: BalanceOf<T>) {
			<CollectedFees<T>>::mutate(|total_fees| {
				*total_fees = total_fees.saturating_add(fees);
			});
			Self::deposit_event(Event::FeesCollected {
				amount: fees,
				total: <CollectedFees<T>>::get(),
			});
		}

		fn mint_reward(maybe_miner: Option<T::AccountId>, reward: BalanceOf<T>) {
			let mint_account = T::MintingAccount::get();

			match maybe_miner {
				Some(miner) => {
					let _ = T::Currency::mint_into(&miner, reward).defensive();

					T::Currency::store_transfer_proof(&mint_account, &miner, reward);

					Self::deposit_event(Event::MinerRewarded { miner: miner.clone(), reward });

					log::debug!(
						target: "mining-rewards",
						"💰 Rewards sent to miner: {:?} {:?}",
						reward,
						miner
					);
				},
				None => {
					let treasury = T::TreasuryPalletId::get().into_account_truncating();
					let _ = T::Currency::mint_into(&treasury, reward).defensive();

					T::Currency::store_transfer_proof(&mint_account, &treasury, reward);

					Self::deposit_event(Event::TreasuryRewarded { reward });

					log::debug!(
						target: "mining-rewards",
						"💰 Rewards sent to Treasury: {:?}",
						reward
					);
				},
			};
		}
	}

	pub struct TransactionFeesCollector<T>(PhantomData<T>);

	impl<T, I> OnUnbalanced<I> for TransactionFeesCollector<T>
	where
		T: Config,
		I: Imbalance<BalanceOf<T>>,
	{
		fn on_nonzero_unbalanced(amount: I) {
			Pallet::<T>::collect_transaction_fees(amount.peek());
		}
	}
}
