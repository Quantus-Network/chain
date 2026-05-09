use core::marker::PhantomData;
use frame_support::weights::Weight;

pub trait WeightInfo {
	fn submit_l0_candidate() -> Weight;
	fn register_aggregator() -> Weight;
	fn update_aggregator() -> Weight;
	fn unregister_aggregator() -> Weight;
	fn claim_bundle() -> Weight;
	fn timeout_bundle() -> Weight;
	fn submit_l1_aggregate() -> Weight;
	fn challenge_invalid_l0_candidate() -> Weight;
	fn drop_expired_candidate() -> Weight;
}

pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	fn submit_l0_candidate() -> Weight {
		Weight::from_parts(100_000_000, 0)
	}

	fn register_aggregator() -> Weight {
		Weight::from_parts(10_000_000, 0)
	}

	fn update_aggregator() -> Weight {
		Weight::from_parts(10_000_000, 0)
	}

	fn unregister_aggregator() -> Weight {
		Weight::from_parts(10_000_000, 0)
	}

	fn claim_bundle() -> Weight {
		Weight::from_parts(100_000_000, 0)
	}

	fn timeout_bundle() -> Weight {
		Weight::from_parts(50_000_000, 0)
	}

	fn submit_l1_aggregate() -> Weight {
		Weight::from_parts(500_000_000, 0)
	}

	fn challenge_invalid_l0_candidate() -> Weight {
		Weight::from_parts(500_000_000, 0)
	}

	fn drop_expired_candidate() -> Weight {
		Weight::from_parts(25_000_000, 0)
	}
}

impl WeightInfo for () {
	fn submit_l0_candidate() -> Weight {
		Weight::from_parts(100_000_000, 0)
	}

	fn register_aggregator() -> Weight {
		Weight::from_parts(10_000_000, 0)
	}

	fn update_aggregator() -> Weight {
		Weight::from_parts(10_000_000, 0)
	}

	fn unregister_aggregator() -> Weight {
		Weight::from_parts(10_000_000, 0)
	}

	fn claim_bundle() -> Weight {
		Weight::from_parts(100_000_000, 0)
	}

	fn timeout_bundle() -> Weight {
		Weight::from_parts(50_000_000, 0)
	}

	fn submit_l1_aggregate() -> Weight {
		Weight::from_parts(500_000_000, 0)
	}

	fn challenge_invalid_l0_candidate() -> Weight {
		Weight::from_parts(500_000_000, 0)
	}

	fn drop_expired_candidate() -> Weight {
		Weight::from_parts(25_000_000, 0)
	}
}
