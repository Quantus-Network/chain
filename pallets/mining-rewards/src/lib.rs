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
	use qp_wormhole::TransferProofRecorder;
	use sp_runtime::traits::Saturating;

	pub(crate) type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Fees and failed-mint retries awaiting distribution by `on_finalize`.
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
			/// Quantized reward (block reward + fees, aligned to the wormhole quantum)
			reward: BalanceOf<T>,
		},
		/// Transaction fees were collected for later distribution
		FeesCollected {
			/// The amount collected
			amount: BalanceOf<T>,
			/// Total fees waiting for distribution
			total: BalanceOf<T>,
		},
		/// No miner in the digest; the credit stays in `CollectedFees` for the next block.
		PayoutDeferred {
			/// Amount held for the next miner
			amount: BalanceOf<T>,
		},
		/// Miner mint failed; the credit stays in `CollectedFees` for retry.
		MinerMintFailed {
			/// The miner who should have received the reward
			miner: T::AccountId,
			/// The reward amount retained
			reward: BalanceOf<T>,
		},
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			assert!(!T::EmissionDivisor::get().is_zero(), "EmissionDivisor must be non-zero");
			assert!(!T::MaxSupply::get().is_zero(), "MaxSupply must be non-zero");
			assert!(
				Self::leaf_quantum() > BalanceOf::<T>::zero(),
				"ZK-tree amount scale factor must fit in Balance and be non-zero"
			);
		}

		fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
			// Return weight consumed for on finalize hook
			<T as crate::pallet::Config>::WeightInfo::on_finalize_rewarded_miner()
		}

		fn on_finalize(_block_number: BlockNumberFor<T>) {
			// Take collected fees first - needed for accurate supply calculation below.
			// This also picks up any reward whose mint failed on an earlier finalize:
			let tx_fees = <CollectedFees<T>>::take();

			// Calculate dynamic block reward based on remaining supply.
			// Note: Transaction fees were burned when the NegativeImbalance was dropped
			// (during transaction execution), so we add them back to get the true
			// current supply before re-minting them to the miner. This prevents the
			// block reward calculation from being slightly inflated by the burned fees.
			// Retried unminted rewards inside `tx_fees` are likewise already-scheduled
			// supply (they were budgeted from the remaining supply of their own block),
			// so this line covers them too.
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

			// Extract miner ID from the pre-runtime digest
			let miner = Self::extract_miner_from_digest();

			// Fees and the block reward are one miner credit. Combining before
			// quantizing can recover a quantum that two independent floors would drop,
			// and the miner spends one leaf / nullifier rather than two.
			let miner_gross = tx_fees.saturating_add(total_reward);

			// Log readable amounts (convert to tokens by dividing by unit)
			if let (Ok(total), Ok(gross), Ok(current), Ok(fees), Ok(unit)) = (
				TryInto::<u128>::try_into(total_reward),
				TryInto::<u128>::try_into(miner_gross),
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
					"💰 Rewards: block={:.6}, fees={:.6}, miner_gross={:.6}, supply={:.2}, remaining={:.2}",
					total as f64 / unit_f64,
					fees as f64 / unit_f64,
					gross as f64 / unit_f64,
					current as f64 / unit_f64,
					remaining as f64 / unit_f64
				);
			}

			let Some(miner) = miner else {
				// A valid QPoW block always carries an author preimage, but extraction
				// is fallible (malformed digest). Do not panic in a hook and do not
				// divert miner credits to treasury: hold them for the next author.
				Self::retain_unminted(miner_gross);
				if !miner_gross.is_zero() {
					Self::deposit_event(Event::PayoutDeferred { amount: miner_gross });
				}
				return;
			};

			let (quantized, dust) = Self::quantize(miner_gross);
			Self::mint_reward(&miner, quantized);
			// Remainder stays unminted so a later block can form a full quantum.
			Self::retain_unminted(dust);
		}
	}

	impl<T: Config> Pallet<T> {
		/// Extract miner wormhole address by hashing the preimage from pre-runtime digest
		fn extract_miner_from_digest() -> Option<T::AccountId> {
			let digest = <frame_system::Pallet<T>>::digest();
			qp_wormhole::extract_author_from_digest(digest.logs.iter())
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

		/// The amount quantum the ZK leaf actually commits: `hash_leaf` stores
		/// `amount / AMOUNT_SCALE_DOWN_FACTOR`. Miner credits must be multiples of this
		/// or the remainder is unexitable.
		fn leaf_quantum() -> BalanceOf<T> {
			pallet_zk_tree::tree::AMOUNT_SCALE_DOWN_FACTOR
				.try_into()
				.unwrap_or_else(|_| BalanceOf::<T>::zero())
		}

		/// Round down to a multiple of the leaf quantum. Returns `(aligned, remainder)`.
		fn quantize(amount: BalanceOf<T>) -> (BalanceOf<T>, BalanceOf<T>) {
			let remainder = amount % Self::leaf_quantum();
			(amount.saturating_sub(remainder), remainder)
		}

		fn mint_reward(miner: &T::AccountId, reward: BalanceOf<T>) {
			if reward.is_zero() {
				return;
			}

			debug_assert!(
				(reward % Self::leaf_quantum()).is_zero(),
				"miner credits must be leaf-quantum aligned"
			);

			match T::Currency::mint_into(miner, reward) {
				Ok(_) => {
					T::ProofRecorder::record_transfer_proof(
						None, // Native token
						T::MintingAccount::get(),
						miner.clone(),
						reward,
					);
					Self::deposit_event(Event::MinerRewarded { miner: miner.clone(), reward });
				},
				Err(e) => {
					log::warn!(
						target: "mining-rewards",
						"Failed to mint {:?} to miner {:?}: {:?}, retaining for retry",
						reward, miner, e
					);
					Self::retain_unminted(reward);
					Self::deposit_event(Event::MinerMintFailed { miner: miner.clone(), reward });
				},
			}
		}

		/// Roll a failed mint back into `CollectedFees` for the next finalize.
		fn retain_unminted(reward: BalanceOf<T>) {
			<CollectedFees<T>>::mutate(|pending| {
				*pending = pending.saturating_add(reward);
			});
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
