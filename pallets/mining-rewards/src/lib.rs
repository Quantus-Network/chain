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
    use frame_support::pallet_prelude::*;
    use frame_support::traits::fungible::{
        DecreaseIssuance, IncreaseIssuance, Inspect, Mutate, Unbalanced,
    };
    use frame_support::traits::Defensive;
    use frame_support::traits::{Get, Imbalance, OnUnbalanced};
    use frame_system::pallet_prelude::*;
    use sp_consensus_pow::POW_ENGINE_ID;
    use sp_runtime::generic::DigestItem;
    use sp_runtime::traits::{AccountIdConversion, Saturating};
    use sp_runtime::Permill;

    type BalanceOf<T> =
        <<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

    type NegativeImbalanceOf<T> = frame_support::traits::fungible::Imbalance<
        BalanceOf<T>,
        DecreaseIssuance<<T as frame_system::Config>::AccountId, <T as Config>::Currency>,
        IncreaseIssuance<<T as frame_system::Config>::AccountId, <T as Config>::Currency>,
    >;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::storage]
    #[pallet::getter(fn collected_fees)]
    pub(super) type CollectedFees<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        /// The currency in which fees are paid and rewards are issued
        type Currency: Mutate<Self::AccountId> + Unbalanced<Self::AccountId>;

        /// The base block reward given to miners
        #[pallet::constant]
        type BlockReward: Get<BalanceOf<Self>>;

        /// The treasury pallet ID
        #[pallet::constant]
        type TreasuryPalletId: Get<frame_support::PalletId>;

        /// The percentage of transaction fees that should go to the Treasury.
        #[pallet::constant]
        type FeesToTreasuryPermill: Get<Permill>;
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
        /// A portion of transaction fees was redirected to the Treasury.
        FeesRedirectedToTreasury {
            /// The amount of fees sent to the Treasury
            amount: BalanceOf<T>,
        },
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
            // Return weight consumed
            Weight::from_parts(10_000, 0)
        }

        fn on_finalize(_block_number: BlockNumberFor<T>) {
            // Get Treasury account
            let treasury_account = T::TreasuryPalletId::get().into_account_truncating();

            // Extract miner ID from the pre-runtime digest
            if let Some(miner) = Self::extract_miner_from_digest() {
                // Get the block reward
                let base_reward = T::BlockReward::get();
                let mut tx_fees = <CollectedFees<T>>::take();

                log::trace!(target: "mining-rewards", "💰 Base reward: {:?}", base_reward);
                log::trace!(target: "mining-rewards", "💰 Original Tx_fees: {:?}", tx_fees);

                // Calculate fees for Treasury
                let fees_to_treasury_percentage = T::FeesToTreasuryPermill::get();
                let fees_for_treasury = fees_to_treasury_percentage.mul_floor(tx_fees);

                // Send fees to Treasury if any
                if fees_for_treasury > Zero::zero() {
                    let _ =
                        T::Currency::mint_into(&treasury_account, fees_for_treasury).defensive();

                    Self::deposit_event(Event::FeesRedirectedToTreasury {
                        amount: fees_for_treasury,
                    });
                    // Subtract fees sent to treasury from the total tx_fees
                    tx_fees = tx_fees.saturating_sub(fees_for_treasury);
                }

                let reward_for_miner = base_reward.saturating_add(tx_fees);

                // Create imbalance for miner's reward
                if reward_for_miner > Zero::zero() {
                    let _ = T::Currency::mint_into(&miner, reward_for_miner).defensive();

                    // Emit an event for miner's reward
                    Self::deposit_event(Event::MinerRewarded {
                        miner: miner.clone(),
                        reward: reward_for_miner, // Actual reward for miner
                    });
                }
            } else {
                // No miner specified, send all rewards (base + all fees) to Treasury
                let base_reward = T::BlockReward::get();
                let tx_fees = <CollectedFees<T>>::take();
                let total_reward_for_treasury = base_reward.saturating_add(tx_fees);

                if total_reward_for_treasury > BalanceOf::<T>::from(0u32) {
                    let _ = T::Currency::mint_into(&treasury_account, total_reward_for_treasury)
                        .defensive();

                    // Emit an event
                    Self::deposit_event(Event::TreasuryRewarded {
                        reward: total_reward_for_treasury,
                    });

                    log::trace!(
                        target: "mining-rewards",
                        "💰 No miner specified, all rewards sent to Treasury: {:?}",
                        total_reward_for_treasury
                    );
                }
            }
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
                        // Try to decode the miner account ID
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
    }

    pub struct TransactionFeesCollector<T>(PhantomData<T>);

    impl<T> OnUnbalanced<NegativeImbalanceOf<T>> for TransactionFeesCollector<T>
    where
        T: Config + pallet_balances::Config<Balance = u128>,
        BalanceOf<T>: From<u128>,
    {
        fn on_nonzero_unbalanced(amount: NegativeImbalanceOf<T>) {
            Pallet::<T>::collect_transaction_fees(amount.peek());
        }
    }
}
