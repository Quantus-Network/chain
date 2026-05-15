use crate as pallet_miner_aggregation;
use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU128, ConstU32, Everything},
};
use frame_system::mocking::MockUncheckedExtrinsic;
use qp_poseidon::PoseidonHasher;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage, Permill,
};

pub use qp_wormhole::{account_id, MINTING_ACCOUNT};

construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		Assets: pallet_assets,
		Wormhole: pallet_wormhole,
		MinerAggregation: pallet_miner_aggregation,
	}
);

pub type Balance = u128;
pub const UNIT: Balance = 1_000_000_000_000;
pub type AccountId = sp_core::crypto::AccountId32;
pub type Block<T> = sp_runtime::generic::Block<
	qp_header::Header<u64, PoseidonHasher, sp_runtime::traits::BlakeTwo256>,
	MockUncheckedExtrinsic<T, qp_dilithium_crypto::DilithiumSignatureScheme>,
>;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
}

impl frame_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type BaseCallFilter = Everything;
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
	pub const ExistentialDeposit: Balance = 1;
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

impl pallet_assets::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type AssetId = u32;
	type AssetIdParameter = u32;
	type Currency = Balances;
	type CreateOrigin =
		frame_support::traits::AsEnsureOriginWithArg<frame_system::EnsureSigned<AccountId>>;
	type ForceOrigin = frame_system::EnsureRoot<AccountId>;
	type AssetDeposit = ConstU128<1>;
	type AssetAccountDeposit = ConstU128<1>;
	type MetadataDepositBase = ConstU128<1>;
	type MetadataDepositPerByte = ConstU128<1>;
	type ApprovalDeposit = ConstU128<1>;
	type StringLimit = ConstU32<50>;
	type Freezer = ();
	type Extra = ();
	type WeightInfo = ();
	type RemoveItemsLimit = ConstU32<1000>;
	type CallbackHandle = ();
	type Holder = ();
	type ReserveData = ();
}

parameter_types! {
	pub const MintingAccount: AccountId = MINTING_ACCOUNT;
	pub const MinimumTransferAmount: Balance = 10 * UNIT;
	pub const VolumeFeeRateBps: u32 = 10;
	pub const VolumeFeesBurnRate: Permill = Permill::from_percent(50);
	pub const AggregationProverFeeShare: Permill = Permill::from_percent(25);
}

impl pallet_wormhole::Config for Test {
	type NativeBalance = Balance;
	type Currency = Balances;
	type Assets = Assets;
	type AssetId = u32;
	type AssetBalance = Balance;
	type TransferCount = u64;
	type MintingAccount = MintingAccount;
	type MinimumTransferAmount = MinimumTransferAmount;
	type VolumeFeeRateBps = VolumeFeeRateBps;
	type VolumeFeesBurnRate = VolumeFeesBurnRate;
	type AggregationProverFeeShare = AggregationProverFeeShare;
	type WormholeAccountId = AccountId;
	type WeightInfo = pallet_wormhole::weights::SubstrateWeight<Test>;
	type ZkTree = ();
}

parameter_types! {
	pub const MaxL0ProofBytes: u32 = 256 * 1024;
	pub const MaxNullifiersPerL0: u32 = 32;
	pub const MaxExitSlotsPerL0: u32 = 64;
	pub const MaxCandidatesPerQueue: u32 = 4;
	pub const CandidateLifetime: u64 = 100;
	pub const StorageBond: Balance = 10;
	pub const ValidityBond: Balance = 20;
	pub const NumLayer0Proofs: u32 = 1;
	pub const CircuitId: [u8; 32] = [42u8; 32];
	pub const MaxActiveBundlesPerMiner: u32 = 4;
	pub const BundleProvingPeriod: u64 = 10;
	pub const MinMinerBond: Balance = 50;
	pub const MaxL1ProofBytes: u32 = 512 * 1024;
	pub const MinerTimeoutSlash: Permill = Permill::from_percent(20);
	pub const InvalidL1ProofSlash: Permill = Permill::from_percent(10);
	pub const InvalidClaimSlash: Permill = Permill::from_percent(40);
	pub const InvalidCandidateChallengeReward: Permill = Permill::from_percent(50);
}

impl pallet_miner_aggregation::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type MaxL0ProofBytes = MaxL0ProofBytes;
	type MaxNullifiersPerL0 = MaxNullifiersPerL0;
	type MaxExitSlotsPerL0 = MaxExitSlotsPerL0;
	type MaxCandidatesPerQueue = MaxCandidatesPerQueue;
	type CandidateLifetime = CandidateLifetime;
	type StorageBond = StorageBond;
	type ValidityBond = ValidityBond;
	type NumLayer0Proofs = NumLayer0Proofs;
	type CircuitId = CircuitId;
	type MaxActiveBundlesPerMiner = MaxActiveBundlesPerMiner;
	type BundleProvingPeriod = BundleProvingPeriod;
	type MinMinerBond = MinMinerBond;
	type MaxL1ProofBytes = MaxL1ProofBytes;
	type MinerTimeoutSlash = MinerTimeoutSlash;
	type InvalidL1ProofSlash = InvalidL1ProofSlash;
	type InvalidClaimSlash = InvalidClaimSlash;
	type InvalidCandidateChallengeReward = InvalidCandidateChallengeReward;
	type WeightInfo = ();
}

pub fn new_test_ext() -> sp_state_machine::TestExternalities<PoseidonHasher> {
	let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![
			(account_id(1), 1_000 * UNIT),
			(account_id(2), 1_000 * UNIT),
			(account_id(3), 1_000 * UNIT),
		],
		dev_accounts: None,
	}
	.assimilate_storage(&mut t)
	.unwrap();
	t.into()
}
