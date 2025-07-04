//! Benchmarking setup for pallet-reversible-transfers

#![cfg(feature = "runtime-benchmarks")]

use super::*;

use crate::Pallet as ReversibleTransfers; // Alias the pallet
use frame_benchmarking::{account as benchmark_account, v2::*, BenchmarkError};
use frame_support::traits::Get;
use frame_system::RawOrigin;
use sp_runtime::traits::{BlockNumberProvider, Hash, StaticLookup};
use sp_runtime::Saturating;

const SEED: u32 = 0;

// Helper to create a RuntimeCall (e.g., a balance transfer)
// Adjust type parameters as needed for your actual Balance type if not u128
fn make_transfer_call<T: Config + pallet_balances::Config>(
    dest: T::AccountId,
    value: u128,
) -> Result<T::RuntimeCall, &'static str>
where
    T::RuntimeCall: From<pallet_balances::Call<T>>,
    BalanceOf<T>: From<u128>,
{
    let dest = <T as frame_system::Config>::Lookup::unlookup(dest);

    let call: T::RuntimeCall = pallet_balances::Call::<T>::transfer_keep_alive {
        dest,
        value: value.into(),
    }
    .into();
    Ok(call)
}

// Helper function to set reversible state directly for benchmark setup
fn setup_reversible_account<T: Config>(
    who: T::AccountId,
    delay: BlockNumberOrTimestampOf<T>,
    policy: DelayPolicy,
    reverser: Option<T::AccountId>,
) {
    ReversibleAccounts::<T>::insert(
        who,
        ReversibleAccountData {
            delay,
            policy,
            explicit_reverser: reverser,
        },
    );
}

// Helper to fund an account (requires Balances pallet in mock runtime)
fn fund_account<T: Config>(account: &T::AccountId, amount: BalanceOf<T>)
where
    T: pallet_balances::Config, // Add bounds for Balances
{
    let _ = <pallet_balances::Pallet<T> as frame_support::traits::Currency<
        T::AccountId,
    >>::make_free_balance_be(account, amount * <pallet_balances::Pallet<T> as frame_support::traits::Currency<
        T::AccountId,
    >>::minimum_balance());
}

// Helper to get the pallet's account ID
fn pallet_account<T: Config>() -> T::AccountId {
    ReversibleTransfers::<T>::account_id()
}

// Type alias for Balance, requires Balances pallet in config
type BalanceOf<T> = <T as pallet_balances::Config>::Balance;

#[benchmarks(
    where
    T: Send + Sync,
    T: Config + pallet_balances::Config,
    <T as pallet_balances::Config>::Balance: From<u128> + Into<u128>,
    T::RuntimeCall: From<pallet_balances::Call<T>> + From<frame_system::Call<T>>,
)]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn set_reversibility() -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        let explicit_reverser: T::AccountId = benchmark_account("explicit_reverser", 0, SEED);
        let delay: BlockNumberOrTimestampOf<T> = T::DefaultDelay::get();
        let policy = DelayPolicy::Explicit;

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            Some(delay.clone()),
            policy.clone(),
            Some(explicit_reverser.clone()),
        );

        assert_eq!(
            ReversibleAccounts::<T>::get(&caller),
            Some(ReversibleAccountData {
                delay,
                policy,
                explicit_reverser: Some(explicit_reverser),
            })
        );

        Ok(())
    }

    #[benchmark]
    fn schedule_transfer() -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        // Ensure caller has funds if the call requires it (e.g., transfer)
        fund_account::<T>(&caller, BalanceOf::<T>::from(1000u128));
        let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
        let transfer_amount = 100u128;

        // Setup caller as reversible
        let delay = T::DefaultDelay::get();
        setup_reversible_account::<T>(caller.clone(), delay.clone(), DelayPolicy::Explicit, None);

        let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;
        let global_nonce = GlobalNonce::<T>::get();
        let tx_id = T::Hashing::hash_of(&(caller.clone(), call, global_nonce).encode());

        let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient);
        // Schedule the dispatch
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            recipient_lookup,
            transfer_amount.into(),
        );

        assert_eq!(AccountPendingIndex::<T>::get(&caller), 1);
        assert!(PendingTransfers::<T>::contains_key(&tx_id));
        // Check scheduler state (can be complex, checking count is simpler)
        let execute_at = T::BlockNumberProvider::current_block_number().saturating_add(
            delay
                .as_block_number()
                .expect("Timestamp delay not supported in benchmark"),
        );
        let task_name = ReversibleTransfers::<T>::make_schedule_id(&tx_id)?;
        assert_eq!(T::Scheduler::next_dispatch_time(task_name)?, execute_at);

        Ok(())
    }

    #[benchmark]
    fn cancel() -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        let reverser: T::AccountId = benchmark_account("reverser", 1, SEED);

        fund_account::<T>(&caller, BalanceOf::<T>::from(1000u128));
        fund_account::<T>(&reverser, BalanceOf::<T>::from(1000u128));
        let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
        let transfer_amount = 100u128;

        // Setup caller as reversible and schedule a task in setup
        let delay = T::DefaultDelay::get();
        // Worst case scenario: reverser is explicit
        setup_reversible_account::<T>(
            caller.clone(),
            delay,
            DelayPolicy::Explicit,
            Some(reverser.clone()),
        );
        let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;

        // Use internal function directly in setup - requires RuntimeOrigin from Config
        let origin = RawOrigin::Signed(caller.clone()).into(); // T::RuntimeOrigin

        // Call the *internal* scheduling logic here for setup
        let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient);
        let global_nonce = GlobalNonce::<T>::get();
        let tx_id = T::Hashing::hash_of(&(caller.clone(), call, global_nonce).encode());

        ReversibleTransfers::<T>::do_schedule_transfer(
            origin,
            recipient_lookup,
            transfer_amount.into(),
        )?;

        // Ensure setup worked before benchmarking cancel
        assert_eq!(AccountPendingIndex::<T>::get(&caller), 1);
        assert!(PendingTransfers::<T>::contains_key(&tx_id));

        // Benchmark the cancel extrinsic
        #[extrinsic_call]
        _(RawOrigin::Signed(reverser), tx_id);

        assert_eq!(AccountPendingIndex::<T>::get(&caller), 0);
        assert!(!PendingTransfers::<T>::contains_key(&tx_id));
        // Check scheduler cancelled (agenda item removed)
        let task_name = ReversibleTransfers::<T>::make_schedule_id(&tx_id)?;
        assert!(T::Scheduler::next_dispatch_time(task_name).is_err());

        Ok(())
    }

    #[benchmark]
    fn execute_transfer() -> Result<(), BenchmarkError> {
        let owner: T::AccountId = whitelisted_caller();
        fund_account::<T>(&owner, BalanceOf::<T>::from(100u128)); // Fund owner
        let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
        fund_account::<T>(&recipient, BalanceOf::<T>::from(100u128)); // Fund recipient
        let transfer_amount = 100u128;

        // Setup owner as reversible and schedule a task in setup
        let delay = T::DefaultDelay::get();
        setup_reversible_account::<T>(owner.clone(), delay, DelayPolicy::Explicit, None);
        let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;

        let owner_origin = RawOrigin::Signed(owner.clone()).into();
        let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient.clone());
        let global_nonce = GlobalNonce::<T>::get();
        let tx_id = T::Hashing::hash_of(&(owner.clone(), call, global_nonce).encode());

        ReversibleTransfers::<T>::do_schedule_transfer(
            owner_origin,
            recipient_lookup,
            transfer_amount.into(),
        )?;

        // Ensure setup worked
        assert_eq!(AccountPendingIndex::<T>::get(&owner), 1);
        assert!(PendingTransfers::<T>::contains_key(&tx_id));

        // Determine the origin for the execute_dispatch call
        // This MUST match what execute_dispatch expects (e.g., pallet account or Root)
        // Assuming execute_dispatch expects Signed(Self::account_id())
        let pallet_account = pallet_account::<T>();
        fund_account::<T>(&pallet_account, BalanceOf::<T>::from(10000u128)); // Fund pallet account slightly if needed for fees
        let execute_origin = RawOrigin::Signed(pallet_account);

        // Benchmark the execute_dispatch extrinsic
        // This assumes execute_dispatch is callable externally by the pallet account.
        // If it expects Root, use RawOrigin::Root().
        #[extrinsic_call]
        _(execute_origin, tx_id);

        // Check state cleaned up
        assert_eq!(AccountPendingIndex::<T>::get(&owner), 0);
        assert!(!PendingTransfers::<T>::contains_key(&tx_id));
        // Check side effect of inner call (balance transfer)
        let initial_balance = <pallet_balances::Pallet<T> as frame_support::traits::Currency<
            T::AccountId,
        >>::minimum_balance()
            * 100_u128.into();
        assert_eq!(
                <pallet_balances::Pallet<T> as frame_support::traits::Currency<T::AccountId>>::free_balance(&recipient),
                BalanceOf::<T>::from(initial_balance.into() + transfer_amount) // Initial + transfer
            );

        Ok(())
    }

    impl_benchmark_test_suite!(
        ReversibleTransfers,
        crate::mock::new_test_ext(),
        crate::mock::Test
    );
}
