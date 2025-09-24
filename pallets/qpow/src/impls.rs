//! impls for pallet_qpow

use crate::{Config, Pallet};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_consensus_qpow::BlockInfo;

impl<T: Config> BlockInfo<BlockNumberFor<T>, T::Moment> for Pallet<T> {
	fn average_block_time() -> T::Moment {
		Pallet::<T>::median_block_time()
	}

	fn block_time(block_number: BlockNumberFor<T>) -> T::Moment {
		Pallet::<T>::block
	}

	fn last_block_time() -> T::Moment {}
}
