use codec::Encode;
use frame_support::traits::{Currency, OnFinalize, OnInitialize};
use qp_dilithium_crypto::Dilithium65Pair;
use quantus_runtime::{
	configs::TreasuryPalletId,
	transaction_extensions::{ReversibleTransactionExtension, WormholeProofRecorderExtension},
	Balances, Runtime, RuntimeCall, Signature, SignedPayload, System, TxExtension,
	UncheckedExtrinsic, UNIT, VERSION,
};
use sp_core::{crypto::AccountId32, Pair};
use sp_runtime::{generic::Era, traits::AccountIdConversion, BuildStorage, MultiAddress};

pub struct TestCommons;

impl TestCommons {
	pub fn account_id(id: u8) -> AccountId32 {
		let mut bytes = [0u8; 32];
		bytes[0] = id;
		AccountId32::new(bytes)
	}

	/// Get the treasury account derived from the runtime's TreasuryPalletId.
	pub fn treasury_account() -> AccountId32 {
		TreasuryPalletId::get().into_account_truncating()
	}

	/// Create a test externality with properly initialized pallets.
	///
	/// This initializes:
	/// - Test accounts 1-4 with 1000 UNIT each
	/// - Treasury pallet storage (account)
	/// - Treasury account balance
	pub fn new_test_ext() -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();

		// Initialize treasury pallet storage properly
		let treasury_account = Self::treasury_account();
		pallet_treasury::GenesisConfig::<Runtime> {
			treasury_account: Some(treasury_account.clone()),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);

		// Add balances after storage is built
		ext.execute_with(|| {
			Balances::make_free_balance_be(&Self::account_id(1), 1000 * UNIT);
			Balances::make_free_balance_be(&Self::account_id(2), 1000 * UNIT);
			Balances::make_free_balance_be(&Self::account_id(3), 1000 * UNIT);
			Balances::make_free_balance_be(&Self::account_id(4), 1000 * UNIT);
			// Fund the treasury account
			Balances::make_free_balance_be(&treasury_account, 1000 * UNIT);
		});

		ext
	}

	/// Create a test externality. Governance track timing is selected at
	/// compile time via the `quantus-runtime/fast-governance` feature:
	/// - feature ON:  all referenda windows collapse to 2 blocks (fast tests)
	/// - feature OFF: production timing (hours/days) — slow but mainnet-accurate
	pub fn new_fast_governance_test_ext() -> sp_io::TestExternalities {
		#[cfg(feature = "fast-governance")]
		println!("Fast governance: all referenda windows = 2 blocks (compile-time).");
		#[cfg(not(feature = "fast-governance"))]
		println!("Production governance: real mainnet timing (hours/days).");
		Self::new_test_ext()
	}

	// Helper function to run blocks
	pub fn run_to_block(n: u32) {
		while System::block_number() < n {
			let b = System::block_number();
			// Call on_finalize for pallets that need it
			quantus_runtime::Scheduler::on_finalize(b);
			System::on_finalize(b);

			// Move to next block
			System::set_block_number(b + 1);

			// Call on_initialize for pallets that need it
			System::on_initialize(b + 1);
			quantus_runtime::Scheduler::on_initialize(b + 1);
		}
	}

	/// Build a fully signed extrinsic through the production `TxExtension`
	/// pipeline: the immortal-era extension tuple, the matching implicit
	/// tuple, and an ML-DSA-65 (Dilithium) signature over the signed payload,
	/// claiming `sender` as the transaction origin.
	///
	/// This is the ONLY test-side copy of the extension tuple — keep it in
	/// lockstep with `TxExtension` in `runtime/src/lib.rs`.
	/// (`node/src/benchmarking.rs` keeps its own copy because the node crate
	/// cannot depend on runtime test code.)
	pub fn signed_extrinsic(
		pair: &Dilithium65Pair,
		sender: AccountId32,
		call: RuntimeCall,
		nonce: u32,
		tip: u128,
	) -> UncheckedExtrinsic {
		let genesis_hash = System::block_hash(0);
		let tx_ext: TxExtension = (
			frame_system::CheckNonZeroSender::<Runtime>::new(),
			frame_system::CheckSpecVersion::<Runtime>::new(),
			frame_system::CheckTxVersion::<Runtime>::new(),
			frame_system::CheckGenesis::<Runtime>::new(),
			frame_system::CheckEra::<Runtime>::from(Era::immortal()),
			frame_system::CheckNonce::<Runtime>::from(nonce),
			frame_system::CheckWeight::<Runtime>::new(),
			ReversibleTransactionExtension::<Runtime>::new(),
			WormholeProofRecorderExtension::<Runtime>::new(),
			pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(tip),
			frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
			frame_system::WeightReclaim::<Runtime>::new(),
		);

		let raw_payload = SignedPayload::from_raw(
			call.clone(),
			tx_ext.clone(),
			(
				(),
				VERSION.spec_version,
				VERSION.transaction_version,
				genesis_hash,
				genesis_hash,
				(),
				(),
				(),
				(),
				(),
				None,
				(),
			),
		);
		let signature = raw_payload.using_encoded(|e| pair.sign(e));

		UncheckedExtrinsic::new_signed(
			call,
			MultiAddress::Id(sender),
			Signature::Dilithium65(signature),
			tx_ext,
		)
	}

	/// Helper to calculate total blocks needed for a governance process
	/// This helps tests understand how many blocks they need to advance
	pub fn calculate_governance_blocks(
		prepare_period: u32,
		decision_period: u32,
		confirm_period: u32,
		min_enactment_period: u32,
	) -> u32 {
		prepare_period + decision_period + confirm_period + min_enactment_period + 5
		// +5 for buffer
	}
}
