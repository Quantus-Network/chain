//! Benchmarking setup for pallet-mining-rewards

extern crate alloc;

use super::*;
use crate::Pallet as MiningRewards;
use frame_benchmarking::v2::*;
use frame_support::traits::fungible::{Inspect, Mutate};
use frame_system::{pallet_prelude::BlockNumberFor, Pallet as SystemPallet};
use sp_consensus_qpow::POW_ENGINE_ID;
use sp_runtime::generic::{Digest, DigestItem};

#[benchmarks]
mod benchmarks {
	use super::*;
	use codec::Decode;
	use frame_support::traits::OnFinalize;
	use pallet_treasury::TreasuryProvider;
	use sp_runtime::Saturating;

	#[benchmark]
	fn on_finalize_rewarded_miner() -> Result<(), BenchmarkError> {
		let block_number: BlockNumberFor<T> = 1u32.into();
		let fees_collected: BalanceOf<T> = 1000u32.into();

		CollectedFees::<T>::put(fees_collected);

		// The digest contains a 32-byte preimage, which is hashed via Poseidon
		// to derive the actual miner wormhole address. We use a fixed preimage
		// and derive the corresponding address for pre-funding.
		let miner_preimage: [u8; 32] = [42u8; 32];
		let miner_address = qp_wormhole::derive_wormhole_address(miner_preimage);
		let miner = T::AccountId::decode(&mut &miner_address[..])
			.expect("AccountId should decode from 32 bytes");

		let miner_digest_item = DigestItem::PreRuntime(POW_ENGINE_ID, miner_preimage.to_vec());

		SystemPallet::<T>::initialize(
			&block_number,
			&SystemPallet::<T>::parent_hash(),
			&Digest { logs: alloc::vec![miner_digest_item] },
		);

		// Pre-fund Treasury and miner accounts to ensure they exist
		let treasury_account = T::Treasury::account_id();
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
