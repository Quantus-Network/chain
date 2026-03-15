//! Benchmarking for pallet_treasury

use super::*;
use frame_benchmarking::v2::*;

#[benchmarks(
	where
	T: Config,
	T::AccountId: From<[u8; 32]> + PartialEq,
)]
mod benchmarks {
	use super::*;
	use frame_system::RawOrigin;

	#[benchmark]
	fn set_treasury_account() -> Result<(), BenchmarkError> {
		let account: T::AccountId = account("caller", 0, 0);
		let root: <T as frame_system::Config>::RuntimeOrigin = RawOrigin::Root.into();

		#[extrinsic_call]
		_(root, account);

		Ok(())
	}

	#[benchmark]
	fn set_treasury_portion() -> Result<(), BenchmarkError> {
		let portion = sp_runtime::Permill::from_percent(50);
		let root: <T as frame_system::Config>::RuntimeOrigin = RawOrigin::Root.into();

		#[extrinsic_call]
		_(root, portion);

		Ok(())
	}
}
