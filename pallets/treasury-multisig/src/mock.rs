use crate as pallet_treasury_multisig;
use frame_support::{derive_impl, parameter_types};
use sp_runtime::{traits::IdentityLookup, BuildStorage};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		TreasuryMultisig: pallet_treasury_multisig,
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

impl crate::Config for Test {
	type MaxSignatories = MaxSignatories;
	type WeightInfo = ();
}

// Mock implementation of WeightInfo for tests
impl crate::weights::WeightInfo for () {
	fn set_treasury_signatories(_s: u32) -> frame_support::weights::Weight {
		frame_support::weights::Weight::zero()
	}
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	pallet_treasury_multisig::GenesisConfig::<Test> {
		signatories: vec![1, 2, 3, 4, 5],
		threshold: 3,
	}
	.assimilate_storage(&mut t)
	.unwrap();

	t.into()
}
