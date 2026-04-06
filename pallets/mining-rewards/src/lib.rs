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
	use core::marker::PhantomData;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate},
			Get, Imbalance, OnUnbalanced,
		},
	};
	use frame_system::pallet_prelude::*;
	use pallet_treasury::TreasuryProvider;
	use qp_wormhole::TransferProofRecorder;
	use sp_runtime::traits::Saturating;

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

		/// Currency type for minting rewards
		type Currency: Mutate<Self::AccountId>;

		/// Asset ID type for the proof recorder
		type AssetId: Default;

		/// Proof recorder for storing wormhole transfer proofs
		type ProofRecorder: qp_wormhole::TransferProofRecorder<
			Self::AccountId,
			Self::AssetId,
			BalanceOf<Self>,
		>;

		/// The maximum total supply of tokens
		#[pallet::constant]
		type MaxSupply: Get<BalanceOf<Self>>;

		/// The divisor used to calculate block rewards from remaining supply
		#[pallet::constant]
		type EmissionDivisor: Get<BalanceOf<Self>>;

		/// Provides treasury account and portion (from treasury pallet).
		type Treasury: TreasuryProvider<AccountId = Self::AccountId>;

		/// The base unit for token amounts (e.g., 1e12 for 12 decimals)
		#[pallet::constant]
		type Unit: Get<BalanceOf<Self>>;

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
		/// Miner reward was redirected to treasury due to mint failure
		MinerRewardRedirected {
			/// The miner who should have received the reward
			miner: T::AccountId,
			/// The reward amount redirected to treasury
			reward: BalanceOf<T>,
		},
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			assert!(!T::EmissionDivisor::get().is_zero(), "EmissionDivisor must be non-zero");
			assert!(!T::MaxSupply::get().is_zero(), "MaxSupply must be non-zero");
		}

		fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
			// Return weight consumed for on finalize hook
			<T as crate::pallet::Config>::WeightInfo::on_finalize_rewarded_miner()
		}

		fn on_finalize(_block_number: BlockNumberFor<T>) {
			// Take collected fees first - needed for accurate supply calculation below.
			let tx_fees = <CollectedFees<T>>::take();

			// Calculate dynamic block reward based on remaining supply.
			// Note: Transaction fees were burned when the NegativeImbalance was dropped
			// (during transaction execution), so we add them back to get the true
			// current supply before re-minting them to the miner. This prevents the
			// block reward calculation from being slightly inflated by the burned fees.
			let max_supply = T::MaxSupply::get();
			let current_supply = T::Currency::total_issuance().saturating_add(tx_fees);
			let emission_divisor = T::EmissionDivisor::get();

			let remaining_supply = max_supply.saturating_sub(current_supply);

			if remaining_supply == BalanceOf::<T>::zero() {
				log::warn!(
					"💰 Emission completed: current supply has reached the configured maximum, \
					 no further block rewards will be minted."
				);
			}

			let total_reward = remaining_supply
				.checked_div(&emission_divisor)
				.unwrap_or_else(BalanceOf::<T>::zero);

			// Split the reward between treasury and miner
			let treasury_portion = T::Treasury::portion();
			let treasury_reward = treasury_portion.mul_floor(total_reward);
			let miner_reward = total_reward.saturating_sub(treasury_reward);

			// Extract miner ID from the pre-runtime digest
			let miner = Self::extract_miner_from_digest();

			// Log readable amounts (convert to tokens by dividing by unit)
			if let (Ok(total), Ok(treasury), Ok(miner_amt), Ok(current), Ok(fees), Ok(unit)) = (
				TryInto::<u128>::try_into(total_reward),
				TryInto::<u128>::try_into(treasury_reward),
				TryInto::<u128>::try_into(miner_reward),
				TryInto::<u128>::try_into(current_supply),
				TryInto::<u128>::try_into(tx_fees),
				TryInto::<u128>::try_into(T::Unit::get()),
			) {
				let remaining: u128 =
					TryInto::<u128>::try_into(max_supply.saturating_sub(current_supply))
						.unwrap_or(0);
				let unit_f64 = unit as f64;
				log::debug!(
					target: "mining-rewards",
					"💰 Rewards: total={:.6}, treasury={:.6}, miner={:.6}, fees={:.6}, supply={:.2}, remaining={:.2}",
					total as f64 / unit_f64,
					treasury as f64 / unit_f64,
					miner_amt as f64 / unit_f64,
					fees as f64 / unit_f64,
					current as f64 / unit_f64,
					remaining as f64 / unit_f64
				);
			}

			// Send fees to miner if any
			Self::mint_reward(miner.as_ref(), tx_fees);

			// Send block rewards to miner
			Self::mint_reward(miner.as_ref(), miner_reward);

			// Send treasury portion to treasury
			Self::mint_reward(None, treasury_reward);
		}
	}

	impl<T: Config> Pallet<T> {
		/// Extract miner wormhole address by hashing the preimage from pre-runtime digest
		fn extract_miner_from_digest() -> Option<T::AccountId> {
			let digest = <frame_system::Pallet<T>>::digest();
			qp_wormhole::extract_author_from_digest(digest.logs.iter().cloned())
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

		fn mint_reward(maybe_miner: Option<&T::AccountId>, reward: BalanceOf<T>) {
			if reward.is_zero() {
				return;
			}

			let mint_account = T::MintingAccount::get();
			let treasury = T::Treasury::account_id();

			match maybe_miner {
				Some(miner) => {
					match T::Currency::mint_into(miner, reward) {
						Ok(_) => {
							T::ProofRecorder::record_transfer_proof(
								None, // Native token
								mint_account,
								miner.clone(),
								reward,
							);
							Self::deposit_event(Event::MinerRewarded {
								miner: miner.clone(),
								reward,
							});
						},
						Err(e) => {
							log::warn!(
								target: "mining-rewards",
								"Failed to mint {:?} to miner {:?}: {:?}, redirecting to treasury",
								reward, miner, e
							);
							// Fallback: redirect to treasury
							match T::Currency::mint_into(&treasury, reward) {
								Ok(_) => {
									T::ProofRecorder::record_transfer_proof(
										None, // Native token
										mint_account,
										treasury,
										reward,
									);
									Self::deposit_event(Event::MinerRewardRedirected {
										miner: miner.clone(),
										reward,
									});
								},
								Err(e2) => {
									log::error!(
										target: "mining-rewards",
										"Failed to redirect {:?} to treasury: {:?}",
										reward, e2
									);
								},
							}
						},
					}
				},
				None => {
					match T::Currency::mint_into(&treasury, reward) {
						Ok(_) => {
							T::ProofRecorder::record_transfer_proof(
								None, // Native token
								mint_account,
								treasury,
								reward,
							);
							Self::deposit_event(Event::TreasuryRewarded { reward });
						},
						Err(e) => {
							log::error!(
								target: "mining-rewards",
								"Failed to mint {:?} to treasury: {:?}",
								reward, e
							);
						},
					}
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
