//! Mock runtime for pallet-voting.

use crate as pallet_voting;
use frame_support::{derive_impl, parameter_types};
use frame_system::EnsureSigned;
use sp_core::H256;
use sp_runtime::{
    traits::{BlakeTwo256, IdentityLookup},
    BuildStorage,
};

type Block = frame_system::mocking::MockBlock<Test>;
type AccountId = u64;

frame_support::construct_runtime!(
    pub enum Test
    {
        System: frame_system,
        Voting: pallet_voting,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId;
    type Nonce = u64;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type RuntimeEvent = RuntimeEvent;
}

parameter_types! {
    pub const MaxProofLength: u32 = 256;
    pub const UnsignedVotePriority: u64 = 100;
}

impl pallet_voting::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type RegisterOrigin = EnsureSigned<AccountId>;
    type MaxProofLength = MaxProofLength;
    type UnsignedVotePriority = UnsignedVotePriority;
    type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}
