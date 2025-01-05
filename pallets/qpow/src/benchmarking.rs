//! Benchmarking setup for pallet_pow

use super::*;
use frame_benchmarking::{benchmarks, whitelisted_caller};
use frame_system::RawOrigin;

benchmarks! {
    submit_solution {
        let caller: T::AccountId = whitelisted_caller();
        let nonce: u32 = 42;
    }: _(RawOrigin::Signed(caller), nonce)

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}