use crate::{self as pallet_wormhole};
use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU32, Everything},
};
use frame_system::mocking::MockUncheckedExtrinsic;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage, Permill,
};

// Re-export shared test helpers from qp_wormhole
pub use qp_wormhole::{account_id, MINTING_ACCOUNT};

construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		ZkTree: pallet_zk_tree,
		Wormhole: pallet_wormhole,
	}
);

pub type Balance = u128;
/// 1 QUAN = 10^12 (12 decimal places)
pub const UNIT: Balance = 1_000_000_000_000;
pub type AccountId = sp_core::crypto::AccountId32;
pub type Block<T> = sp_runtime::generic::Block<
	qp_header::Header<u64, BlakeTwo256>,
	MockUncheckedExtrinsic<T, qp_dilithium_crypto::DilithiumSignatureScheme>,
>;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
}

impl frame_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type BaseCallFilter = Everything;
	type AuthorizeUpgradeOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type BlockWeights = ();
	type BlockLength = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type RuntimeTask = ();
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block<Self>;
	type BlockHashCount = BlockHashCount;
	type DbWeight = ();
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<Balance>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type ExtensionsWeightInfo = ();
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
}

parameter_types! {
	/// `static` so individual tests can raise it (e.g. to exercise the
	/// below-ED aggregator rebate fallback) via `ExistentialDeposit::set`.
	pub static ExistentialDeposit: Balance = 1;
}

impl pallet_balances::Config for Test {
	type RuntimeHoldReason = ();
	type RuntimeFreezeReason = ();
	type WeightInfo = ();
	type Balance = Balance;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type ReserveIdentifier = [u8; 8];
	type FreezeIdentifier = ();
	type MaxLocks = ConstU32<50>;
	type MaxReserves = ();
	type MaxFreezes = ();
	type DoneSlashHandler = ();
	type RuntimeEvent = RuntimeEvent;
}

parameter_types! {
	/// The "from" account used when recording transfer proofs for minted tokens.
	/// Uses the shared MINTING_ACCOUNT constant from qp_wormhole.
	pub const MintingAccount: AccountId = MINTING_ACCOUNT;
	/// Volume fee rate in basis points.
	/// Matches the live runtime and the committed private/public batch hex fixtures
	/// (`test-data/*.hex`).
	pub const VolumeFeeRateBps: u32 = 4;
	/// Proportion of volume fees to burn (50% burned, 50% to miner)
	pub const VolumeFeesBurnRate: Permill = Permill::from_percent(50);
	/// Half of the burn bucket on public-batch exits goes to the aggregator.
	pub const VolumeFeesAggregatorRate: Permill = Permill::from_percent(50);
}

impl pallet_zk_tree::Config for Test {
	type AssetId = u32;
	type Balance = Balance;
}

impl pallet_wormhole::Config for Test {
	type NativeBalance = Balance;
	type Currency = Balances;
	type AssetId = u32;
	type AssetBalance = Balance;
	type TransferCount = u64;
	type MintingAccount = MintingAccount;
	type VolumeFeeRateBps = VolumeFeeRateBps;
	type VolumeFeesBurnRate = VolumeFeesBurnRate;
	type VolumeFeesAggregatorRate = VolumeFeesAggregatorRate;
	type WormholeAccountId = AccountId;
	type WeightInfo = crate::weights::SubstrateWeight<Test>;
	// Real ZK tree so tests exercise the actual leaf hashing (e.g. the
	// recipient-canonicalization invariant), not the no-op recorder.
	type ZkTree = ZkTree;
}

// Helper function to build a genesis configuration
pub fn new_test_ext() -> sp_state_machine::TestExternalities<BlakeTwo256> {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	t.into()
}

/// Build test externalities with genesis balance endowments.
///
/// TransferProofs are *derived* from these free balances in `on_initialize` at block 1
/// (the wormhole pallet records a proof for every account with a free balance), enabling
/// each address to spend via ZK proofs. Tests should call `System::set_block_number(1)`
/// and then trigger `Wormhole::on_initialize(1)` to process them.
pub fn new_test_ext_with_endowments(
	endowments: Vec<(AccountId, Balance)>,
) -> sp_state_machine::TestExternalities<BlakeTwo256> {
	use sp_runtime::BuildStorage;

	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

	// Set up balances for the endowed accounts; wormhole proofs derive from these.
	pallet_balances::GenesisConfig::<Test> { balances: endowments, dev_accounts: None }
		.assimilate_storage(&mut t)
		.unwrap();

	t.into()
}

/// Set a miner preimage in the pre-runtime digest, simulating QPoW block authorship
/// so that `qp_wormhole::extract_author_from_digest` finds a block author.
pub fn set_miner_preimage_digest(preimage: [u8; 32]) {
	let pre_digest =
		sp_runtime::DigestItem::PreRuntime(qp_wormhole::POW_ENGINE_ID, preimage.to_vec());
	System::deposit_log(pre_digest);
}

/// Assert that `to` received an exitable native zk-tree leaf of `amount`.
///
/// Wormhole-derived accounts have no signing key; the only spend path is a
/// zk-tree leaf. A credit without one is permanently frozen.
///
/// `amount` must be a positive multiple of [`crate::SCALE_DOWN_FACTOR`]:
/// `hash_leaf` commits `amount / 10^10`, so a sub-quantum credit would store a
/// nonzero balance whose circuit amount is zero and cannot be withdrawn.
pub fn assert_exitable_native_leaf(to: &AccountId, amount: Balance) {
	assert!(
		amount > 0 && amount % crate::SCALE_DOWN_FACTOR == 0,
		"credited amount {amount} must be a positive whole number of quanta"
	);
	let matching: Vec<u64> = System::events()
		.into_iter()
		.filter_map(|r| match r.event {
			RuntimeEvent::Wormhole(crate::Event::<Test>::NativeTransferred {
				to: event_to,
				amount: event_amount,
				leaf_index,
				..
			}) if event_to == *to && event_amount == amount => Some(leaf_index),
			_ => None,
		})
		.collect();
	assert_eq!(
		matching.len(),
		1,
		"expected exactly one NativeTransferred of {amount} to {to:?} (got {})",
		matching.len()
	);
	let leaf = ZkTree::leaf(matching[0]).expect("recorded leaf_index must exist in the zk-tree");
	assert_eq!(leaf.amount, amount, "leaf amount must match the credited balance");
	assert_eq!(leaf.asset_id, 0, "fee credits are native");
	let zeroed = pallet_zk_tree::ZkLeaf {
		to: leaf.to.clone(),
		transfer_count: leaf.transfer_count,
		asset_id: leaf.asset_id,
		amount: 0,
	};
	assert_ne!(
		pallet_zk_tree::tree::hash_leaf::<Test>(&leaf),
		pallet_zk_tree::tree::hash_leaf::<Test>(&zeroed),
		"committed circuit amount must be nonzero (hash_leaf divides by 10^10)"
	);
}
