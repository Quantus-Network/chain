//! Benchmarking setup for pallet-mining-rewards

use super::*;
use crate::Pallet as MiningRewards;
use frame_support::traits::Currency;
use sp_consensus_pow::POW_ENGINE_ID;
use sp_runtime::generic::DigestItem;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarks {
    use super::*;
    use codec::Encode;
    use frame_benchmarking::{benchmarks, impl_benchmark_test_suite, whitelisted_caller};
    use frame_support::traits::{Get, OnFinalize, OnInitialize};
    use sp_runtime::Saturating;

    type CurrencyOf<T> = <T as Config>::Currency;
    type BalanceOf<T> =
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    benchmarks! {
        on_initialize {
        }: {
            MiningRewards::<T>::on_initialize(frame_system::Pallet::<T>::block_number())
        }

        collect_transaction_fees {
            let fee_amount: BalanceOf<T> = 100u32.into();
        }: {
            MiningRewards::<T>::collect_transaction_fees(fee_amount)
        } verify {
            assert_eq!(MiningRewards::<T>::collected_fees(), fee_amount);
        }

        on_finalize {
            let miner: T::AccountId = whitelisted_caller();
            let miner_encoded = miner.encode();

            let digest_item = DigestItem::PreRuntime(POW_ENGINE_ID, miner_encoded);
            frame_system::Pallet::<T>::deposit_log(digest_item);

            let fee_amount: BalanceOf<T> = 100u32.into();
            MiningRewards::<T>::collect_transaction_fees(fee_amount);

            let initial_balance = CurrencyOf::<T>::free_balance(&miner);
            let block_number = frame_system::Pallet::<T>::block_number();
        }: {
            MiningRewards::<T>::on_finalize(block_number)
        } verify {
            let expected_reward = T::BlockReward::get().saturating_add(fee_amount);
            let final_balance = CurrencyOf::<T>::free_balance(&miner);
            assert!(final_balance > initial_balance);
            assert_eq!(final_balance, initial_balance.saturating_add(expected_reward));
        }
    }

    impl_benchmark_test_suite!(
        MiningRewards,
        crate::mock::new_test_ext(),
        crate::mock::Test
    );
}
