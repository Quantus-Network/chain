use frame_support::__private::sp_io;
use frame_support::traits::{Currency, OnFinalize, OnInitialize};
use resonance_runtime::{Balances, Runtime, System, UNIT};
use sp_core::crypto::AccountId32;
use sp_runtime::BuildStorage;

pub struct TestCommons;

impl TestCommons {
    pub fn account_id(id: u8) -> AccountId32 {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        AccountId32::new(bytes)
    }

    // Create a test externality
    pub fn new_test_ext() -> sp_io::TestExternalities {
        let t = frame_system::GenesisConfig::<Runtime>::default()
            .build_storage()
            .unwrap();

        let mut ext = sp_io::TestExternalities::new(t);

        // Add balances in the ext
        ext.execute_with(|| {
            Balances::make_free_balance_be(&Self::account_id(1), 1000 * UNIT);
            Balances::make_free_balance_be(&Self::account_id(2), 1000 * UNIT);
            Balances::make_free_balance_be(&Self::account_id(3), 1000 * UNIT);
            Balances::make_free_balance_be(&Self::account_id(4), 1000 * UNIT);
        });

        ext
    }

    /// Create a test externality with FAST governance track timing for ALL tracks
    /// This speeds up ALL governance tests by using 2-block periods for prepare, decision, confirm, and enactment
    /// Should reduce test execution time from 30+ seconds to milliseconds
    pub fn new_fast_governance_test_ext() -> sp_io::TestExternalities {
        use resonance_runtime::governance::definitions::GlobalTrackConfig;

        // Set global fast timing for ALL governance tracks (Community, Treasury, Tech Collective)
        GlobalTrackConfig::set_fast_test_timing(); // Sets 2 blocks for all periods

        println!("Fast governance test config activated: All tracks use 2-block periods");
        Self::new_test_ext()
    }

    /// Create a test externality with CUSTOM governance track timing for ALL tracks
    /// Allows precise control over timing periods for all governance tracks
    pub fn new_custom_governance_test_ext(
        prepare_period: u32,
        decision_period: u32,
        confirm_period: u32,
        min_enactment_period: u32,
    ) -> sp_io::TestExternalities {
        use resonance_runtime::governance::definitions::GlobalTrackConfig;

        // Set custom timing for ALL governance tracks
        GlobalTrackConfig::set_global_timing(
            prepare_period,
            decision_period,
            confirm_period,
            min_enactment_period,
        );

        println!(
            "Custom governance test config: prepare={}, decision={}, confirm={}, enactment={}",
            prepare_period, decision_period, confirm_period, min_enactment_period
        );
        Self::new_test_ext()
    }

    /// Legacy method - still works but now uses global config
    /// Create a test externality with FAST governance track timing for Tech Collective specifically
    /// Note: This now uses the global config system and affects ALL tracks
    pub fn new_fast_tech_collective_test_ext() -> sp_io::TestExternalities {
        println!(
            "Legacy tech collective test config - now using global fast config for all tracks"
        );
        Self::new_fast_governance_test_ext()
    }

    // Helper function to run blocks
    pub fn run_to_block(n: u32) {
        let num_blocks = n - System::block_number();
        if (num_blocks > 300) {
            panic!("Too many blocks to run: {}", num_blocks);
        }
        println!("run_to_block: Fast forwarding {} blocks", num_blocks);

        while System::block_number() < n {
            let b = System::block_number();
            println!("Current block: {} - target block: {}", b, n);
            // Call on_finalize for pallets that need it
            resonance_runtime::Scheduler::on_finalize(b);
            System::on_finalize(b);

            // Move to next block
            System::set_block_number(b + 1);

            // Call on_initialize for pallets that need it
            System::on_initialize(b + 1);
            resonance_runtime::Scheduler::on_initialize(b + 1);
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

    /// Get the default fast governance timing - useful for quick tests
    pub fn fast_governance_timing() -> (u32, u32, u32, u32) {
        (2, 2, 2, 2) // prepare, decision, confirm, enactment
    }
}
