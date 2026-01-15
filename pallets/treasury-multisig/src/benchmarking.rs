// Benchmarking setup
#![allow(clippy::unwrap_used)] // Benchmarks can panic on setup failures

use super::*;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;

#[benchmarks]
mod benchmarks {
	use super::*;
	use alloc::vec::Vec;
	use frame_support::BoundedVec;

	#[benchmark]
	fn set_treasury_signatories(s: Linear<1, 100>, // Number of signatories
	) {
		// Setup: Create signatories
		let signatories: Vec<T::AccountId> =
			(0..s).map(|i| frame_benchmarking::account("signatory", i, 0)).collect();

		let threshold = (s / 2 + 1) as u16; // Majority threshold

		// Initialize with some signatories first
		let initial_signatories: Vec<T::AccountId> =
			(0..3).map(|i| frame_benchmarking::account("initial", i, 0)).collect();
		Signatories::<T>::put(BoundedVec::try_from(initial_signatories).unwrap());
		Threshold::<T>::put(2);

		#[extrinsic_call]
		_(RawOrigin::Root, signatories, threshold);

		// Verify
		assert_eq!(Threshold::<T>::get(), threshold);
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
