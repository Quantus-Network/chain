//! Benchmarking for pallet faucet
#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account as benchmark_account, v2::*, BenchmarkError};
use frame_system::RawOrigin;

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn mint_new_tokens() -> Result<(), BenchmarkError> {
        let dest: T::AccountId = benchmark_account("dest", 0, 0);
        let seed = 0_u64;
        let dest_lookup = <T as frame_system::Config>::Lookup::unlookup(dest.clone());

        #[extrinsic_call]
        _(RawOrigin::None, dest_lookup.clone(), seed);

        assert_eq!(
            T::Currency::free_balance(&dest),
            T::DefaultMintAmount::get(),
        );

        Ok(())
    }
}
