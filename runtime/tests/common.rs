use frame_support::traits::{Currency, OnFinalize, OnInitialize};
use quantus_runtime::{configs::TreasuryPalletId, Balances, Runtime, System, UNIT};
use sp_core::crypto::AccountId32;
use sp_runtime::{traits::AccountIdConversion, BuildStorage, Permill};

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
	/// - Treasury pallet storage (account and portion)
	/// - Treasury account balance
	pub fn new_test_ext() -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();

		// Initialize treasury pallet storage properly
		let treasury_account = Self::treasury_account();
		pallet_treasury::GenesisConfig::<Runtime> {
			treasury_account: Some(treasury_account.clone()),
			treasury_portion: Some(Permill::from_percent(30)), // 30% to treasury, 70% to miner
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

	/// Create a test externality with governance track timing based on feature flags
	/// - Without `production-governance-tests`: Uses fast 2-block periods for all governance tracks
	/// - With `production-governance-tests`: Uses production timing (hours/days) This allows CI to
	///   test both fast (for speed) and slow (for correctness) governance
	pub fn new_fast_governance_test_ext() -> sp_io::TestExternalities {
		#[cfg(feature = "production-governance-tests")]
		{
			println!("Production governance test config: Using production timing (hours/days).");
			Self::new_test_ext()
		}

		#[cfg(not(feature = "production-governance-tests"))]
		{
			use quantus_runtime::governance::definitions::GlobalTrackConfig;

			// Set global fast timing for ALL governance tracks (Community, Treasury, Tech
			// Collective)
			GlobalTrackConfig::set_fast_test_timing(); // Sets 2 blocks for all periods

			println!("Fast governance test config activated: All tracks use 2-block periods");
			Self::new_test_ext()
		}
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
