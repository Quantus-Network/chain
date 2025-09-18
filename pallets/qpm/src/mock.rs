use core::marker::PhantomData;

use crate as pallet_qpm;
use alloc::collections::BTreeMap;
use frame_support::derive_impl;
use pallet_balances::AccountData;
use sp_consensus_qpow::BlockInfo;
use sp_core::{parameter_types, ConstU128};
use sp_runtime::BuildStorage;

type Block = frame_system::mocking::MockBlock<Test>;

#[frame_support::runtime]
mod runtime {
	// The main runtime
	#[runtime::runtime]
	// Runtime Types to be generated
	#[runtime::derive(
		RuntimeCall,
		RuntimeEvent,
		RuntimeError,
		RuntimeOrigin,
		RuntimeFreezeReason,
		RuntimeHoldReason,
		RuntimeSlashReason,
		RuntimeLockId,
		RuntimeTask,
		RuntimeViewFunction
	)]
	pub struct Test;

	#[runtime::pallet_index(0)]
	pub type System = frame_system::Pallet<Test>;

	#[runtime::pallet_index(1)]
	pub type QPM = pallet_qpm::Pallet<Test>;

	#[runtime::pallet_index(2)]
	pub type Balances = pallet_balances::Pallet<Test>;
}

pub type AccountId = u64;
type Balance = u128;

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountId = AccountId;
	type AccountData = AccountData<Balance>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type Balance = Balance;
	type DustRemoval = ();
	type ExistentialDeposit = ConstU128<0>;
	type AccountStore = frame_system::Pallet<Test>;
	type WeightInfo = ();
	type RuntimeHoldReason = RuntimeHoldReason;
}

// Thread-local storage for block times
thread_local! {
	pub static MOCK_BLOCK_TIMES: core::cell::RefCell<BTreeMap<u64, u64>> = core::cell::RefCell::new(BTreeMap::new());
}

pub fn set_mock_block_time(block: u64, moment: u64) {
	MOCK_BLOCK_TIMES.with(|m| {
		m.borrow_mut().insert(block, moment);
	});
}

pub struct MockBlockTimes<B, M>(PhantomData<(B, M)>);

impl<
		Moment: Copy + Default + Ord + From<u64> + Into<u64>,
		BlockNumber: Copy + Default + Ord + From<u64> + Into<u64>,
	> BlockInfo<BlockNumber, Moment> for MockBlockTimes<BlockNumber, Moment>
{
	fn average_block_time() -> BlockNumber {
		// For mock, just return a constant
		10.into()
	}

	fn block_time(block_number: BlockNumber) -> Moment {
		// Use a static instance for tests
		MOCK_BLOCK_TIMES
			.with(|m| m.borrow_mut().get(&block_number.into()).copied().unwrap_or_default())
			.into()
	}

	fn last_block_time() -> Moment {
		MOCK_BLOCK_TIMES.with(|m| {
			m.borrow_mut()
				.iter()
				.next_back()
				.map(|(_, &moment)| moment)
				.unwrap_or_default()
				.into()
		})
	}
}

parameter_types! {
	pub const PredictionDepositAmount: u128 = 100;
	pub const BlockBufferTime: u32 = 5;
	pub PoolAddress: AccountId = 123456;
	pub const MaxPredictions: u32 = 256;
}

impl pallet_qpm::Config for Test {
	type BlockNumberProvider = System;
	type Moment = u64;
	type BlockTimeInfo = MockBlockTimes<u64, u64>;
	type Currency = Balances;
	type PredictionDepositAmount = PredictionDepositAmount;
	type BlockBufferTime = BlockBufferTime;
	type PoolAddress = PoolAddress;
	type MaxPredictions = MaxPredictions;
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	frame_system::GenesisConfig::<Test>::default().build_storage().unwrap().into()
}
