use frame_support::__private::sp_io;
use frame_support::traits::{Currency, Hooks};
use resonance_runtime::{Balances, Runtime, System, UNIT};
use sp_core::crypto::AccountId32;
use sp_runtime::BuildStorage;

// Helper function to create AccountId32 from a simple index
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
        Balances::make_free_balance_be(&account_id(1), 1000 * UNIT);
        Balances::make_free_balance_be(&account_id(2), 1000 * UNIT);
        Balances::make_free_balance_be(&account_id(3), 1000 * UNIT);
        Balances::make_free_balance_be(&account_id(4), 1000 * UNIT);
    });

    ext
}

// Helper function to run blocks
pub fn run_to_block(n: u32) {
    while System::block_number() < n {
        let b = System::block_number();
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
