use crate as pallet_treasury_config;
use frame_support::{derive_impl, parameter_types};
use sp_runtime::{traits::IdentityLookup, BuildStorage};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		TreasuryConfig: pallet_treasury_config,
	}
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountId = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
}

parameter_types! {
	pub const MaxSignatories: u32 = 100;
}

impl pallet_treasury_config::Config for Test {
	type MaxSignatories = MaxSignatories;
	type WeightInfo = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_treasury_config::GenesisConfig::<Test> {
		signatories: vec![1, 2, 3, 4, 5],
		threshold: 3,
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
