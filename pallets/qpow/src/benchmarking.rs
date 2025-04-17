//! Benchmarking setup for pallet_pow

use super::*;
use frame_benchmarking::v2::benchmarks;
use frame_benchmarking::BenchmarkError;

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn submit_nonce() -> Result<(), BenchmarkError> {
        let valid_nonce = [0u8; 64];

        #[block]
        {
            let header = [1u8; 32];

            let _ = crate::Pallet::<T>::submit_nonce(header, valid_nonce.clone());
        }

        assert_eq!(LatestNonce::<T>::get(), Some(valid_nonce));

        Ok(())
    }
}
