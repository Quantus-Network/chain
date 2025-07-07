//! Benchmarking setup for pallet-mining-rewards

extern crate alloc;

use super::*;
use crate::Pallet as MiningRewards;
use frame_benchmarking::{account, v2::*, BenchmarkError};
use frame_support::traits::fungible::{Inspect, Mutate};
use frame_system::pallet_prelude::BlockNumberFor;
use frame_system::Pallet as SystemPallet;
use sp_consensus_pow::POW_ENGINE_ID;
use sp_runtime::generic::{Digest, DigestItem};
use sp_runtime::traits::AccountIdConversion;

#[benchmarks]
mod benchmarks {
    use super::*;
    use codec::Encode;
    use frame_support::traits::{Get, OnFinalize};
    use sp_runtime::Saturating;

    #[benchmark]
    fn on_finalize_rewarded_miner() -> Result<(), BenchmarkError> {
        let block_number: BlockNumberFor<T> = 1u32.into();
        let miner: T::AccountId = account("miner", 0, 0);
        let fees_collected: BalanceOf<T> = 1000u32.into();

        CollectedFees::<T>::put(fees_collected);

        let miner_digest_item = DigestItem::PreRuntime(POW_ENGINE_ID, miner.encode());

        SystemPallet::<T>::initialize(
            &block_number,
            &SystemPallet::<T>::parent_hash(),
            &Digest {
                logs: alloc::vec![miner_digest_item],
            },
        );

        // Pre-fund Treasury account to ensure it exists
        let treasury_account = T::TreasuryPalletId::get().into_account_truncating();
        let ed = T::Currency::minimum_balance();
        let _ = T::Currency::mint_into(&treasury_account, ed.saturating_mul(1000u32.into()));
        let _ = T::Currency::mint_into(&miner, ed.saturating_mul(1000u32.into()));

        #[block]
        {
            MiningRewards::<T>::on_finalize(block_number);
        }
        Ok(())
    }
}
