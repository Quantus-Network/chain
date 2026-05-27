//! Mock runtime for testing pallet-key-association.

use crate as pallet_key_association;
use frame_support::{derive_impl, parameter_types};
use sp_runtime::BuildStorage;

type Block = frame_system::mocking::MockBlock<Test>;
pub type AccountId = sp_core::crypto::AccountId32;

/// Create an AccountId from a u64 (for test convenience).
pub fn account_id(id: u64) -> AccountId {
	let mut data = [0u8; 32];
	data[0..8].copy_from_slice(&id.to_le_bytes());
	AccountId::new(data)
}

#[frame_support::runtime]
mod runtime {
	#[runtime::runtime]
	#[runtime::derive(
		RuntimeCall,
		RuntimeEvent,
		RuntimeError,
		RuntimeOrigin,
		RuntimeFreezeReason,
		RuntimeHoldReason,
		RuntimeSlashReason,
		RuntimeLockId,
		RuntimeTask
	)]
	pub struct Test;

	#[runtime::pallet_index(0)]
	pub type System = frame_system::Pallet<Test>;

	#[runtime::pallet_index(1)]
	pub type KeyAssociation = pallet_key_association::Pallet<Test>;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountId = AccountId;
	type Lookup = sp_runtime::traits::IdentityLookup<Self::AccountId>;
}

parameter_types! {
	pub const MaxAssociationsParam: u32 = 8;
}

impl pallet_key_association::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type MaxAssociations = MaxAssociationsParam;
	type WeightInfo = ();
}

/// Build test externalities with default genesis.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let t = frame_system::GenesisConfig::<Test>::default()
		.build_storage()
		.expect("Genesis build should succeed");
	t.into()
}
