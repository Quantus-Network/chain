//! Benchmarking setup for pallet-reversible-transfers

use super::*;

use crate::Pallet as ReversibleTransfers; // Alias the pallet
use frame_benchmarking::{account as benchmark_account, v2::*, BenchmarkError};
use frame_support::traits::{fungible::Mutate, Get};
use frame_system::RawOrigin;
use sp_runtime::{
	traits::{BlockNumberProvider, Hash, StaticLookup},
	Saturating,
};

const SEED: u32 = 0;

// Helper to create a RuntimeCall (e.g., a balance transfer)
// Adjust type parameters as needed for your actual Balance type if not u128
fn make_transfer_call<T: Config>(
	dest: T::AccountId,
	value: u128,
) -> Result<RuntimeCallOf<T>, &'static str>
where
	RuntimeCallOf<T>: From<pallet_balances::Call<T>>,
	BalanceOf<T>: From<u128>,
{
	let dest_lookup = <T as frame_system::Config>::Lookup::unlookup(dest);

	let call: RuntimeCallOf<T> =
		pallet_balances::Call::<T>::transfer_keep_alive { dest: dest_lookup, value: value.into() }
			.into();
	Ok(call)
}

// Helper function to set reversible state directly for benchmark setup
fn setup_high_security_account<T: Config>(
	who: T::AccountId,
	delay: BlockNumberOrTimestampOf<T>,
	interceptor: T::AccountId,
) {
	HighSecurityAccounts::<T>::insert(who, HighSecurityAccountData { delay, interceptor });
}

// Helper to fund an account (requires Balances pallet in mock runtime)
fn fund_account<T>(account: &T::AccountId, amount: BalanceOf<T>)
where
	T: Config + pallet_balances::Config,
{
	let _ = <pallet_balances::Pallet<T> as Mutate<T::AccountId>>::mint_into(
        account,
        amount *
            <pallet_balances::Pallet<T> as frame_support::traits::Currency<
                T::AccountId,
            >>::minimum_balance(),
    );
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
    RuntimeCallOf<T>: From<pallet_balances::Call<T>> + From<frame_system::Call<T>>,
)]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn set_high_security() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf::<T>::from(10000u128));
		let interceptor: T::AccountId = benchmark_account("interceptor", 0, SEED);
		let delay: BlockNumberOrTimestampOf<T> = T::DefaultDelay::get();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), delay, interceptor.clone());

		assert_eq!(
			HighSecurityAccounts::<T>::get(&caller),
			Some(HighSecurityAccountData { delay, interceptor })
		);

		Ok(())
	}

	#[benchmark]
	fn schedule_transfer() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf::<T>::from(10000u128));
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
		let interceptor: T::AccountId = benchmark_account("interceptor", 1, SEED);
		let transfer_amount = 100u128;

		// Setup caller as reversible
		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(caller.clone(), delay, interceptor.clone());

		let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;
		let global_nonce = GlobalNonce::<T>::get();
		let tx_id = T::Hashing::hash_of(&(caller.clone(), call, global_nonce).encode());

		let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient);
		// Schedule the dispatch
		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), recipient_lookup, transfer_amount.into());

		assert_eq!(AccountPendingIndex::<T>::get(&caller), 1);
		assert!(PendingTransfers::<T>::contains_key(tx_id));
		// Check scheduler state (can be complex, checking count is simpler)
		let execute_at = <T as pallet::Config>::BlockNumberProvider::current_block_number()
			.saturating_add(
				delay.as_block_number().expect("Timestamp delay not supported in benchmark"),
			);
		let task_name = ReversibleTransfers::<T>::make_schedule_id(&tx_id)?;
		assert_eq!(T::Scheduler::next_dispatch_time(task_name)?, execute_at);

		Ok(())
	}

	#[benchmark]
	fn cancel() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		let interceptor: T::AccountId = benchmark_account("interceptor", 1, SEED);

		fund_account::<T>(&caller, BalanceOf::<T>::from(10000u128));
		fund_account::<T>(&interceptor, BalanceOf::<T>::from(10000u128));
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
		let transfer_amount = 100u128;

		// Setup caller as reversible and schedule a task in setup
		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(caller.clone(), delay, interceptor.clone());

		let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;

		let origin = RawOrigin::Signed(caller.clone()).into();

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
		assert!(PendingTransfers::<T>::contains_key(tx_id));

		// Benchmark the cancel extrinsic
		#[extrinsic_call]
		_(RawOrigin::Signed(interceptor), tx_id);

		assert_eq!(AccountPendingIndex::<T>::get(&caller), 0);
		assert!(!PendingTransfers::<T>::contains_key(tx_id));
		// Check scheduler cancelled (agenda item removed)
		let task_name = ReversibleTransfers::<T>::make_schedule_id(&tx_id)?;
		assert!(T::Scheduler::next_dispatch_time(task_name).is_err());

		Ok(())
	}

	#[benchmark]
	fn execute_transfer() -> Result<(), BenchmarkError> {
		let owner: T::AccountId = whitelisted_caller();
		fund_account::<T>(&owner, BalanceOf::<T>::from(10000u128)); // Fund owner
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
		// Fund recipient with minimum_balance * 100 to match assertion expectation
		let initial_balance = <pallet_balances::Pallet<T> as frame_support::traits::Currency<
			T::AccountId,
		>>::minimum_balance() *
			100_u128.into();
		fund_account::<T>(&recipient, initial_balance);
		let interceptor: T::AccountId = benchmark_account("interceptor", 1, SEED);
		let transfer_amount = 100u128;

		// Setup owner as reversible and schedule a task in setup
		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(owner.clone(), delay, interceptor);
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
		assert!(PendingTransfers::<T>::contains_key(tx_id));

		let pallet_account = pallet_account::<T>();
		fund_account::<T>(&pallet_account, BalanceOf::<T>::from(10000u128));
		let execute_origin = RawOrigin::Signed(pallet_account);

		#[extrinsic_call]
		_(execute_origin, tx_id);

		// Check state cleaned up
		assert_eq!(AccountPendingIndex::<T>::get(&owner), 0);
		assert!(!PendingTransfers::<T>::contains_key(tx_id));
		// Check side effect of inner call (balance transfer)
		let initial_balance = <pallet_balances::Pallet<T> as frame_support::traits::Currency<
			T::AccountId,
		>>::minimum_balance() *
			100_u128.into();
		let expected_balance = initial_balance.saturating_add(transfer_amount.into());
		assert_eq!(
            <pallet_balances::Pallet<T> as frame_support::traits::Currency<T::AccountId>>::free_balance(
                &recipient
            ),
            expected_balance
        );

		Ok(())
	}

	#[benchmark]
	fn recover_funds() -> Result<(), BenchmarkError> {
		let account: T::AccountId = whitelisted_caller();
		let guardian: T::AccountId = benchmark_account("guardian", 0, SEED);

		fund_account::<T>(&account, BalanceOf::<T>::from(10000u128));
		fund_account::<T>(&guardian, BalanceOf::<T>::from(100u128));

		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(account.clone(), delay, guardian.clone());

		#[extrinsic_call]
		_(RawOrigin::Signed(guardian.clone()), account.clone());

		Ok(())
	}

	impl_benchmark_test_suite!(
		ReversibleTransfers,
		crate::tests::mock::new_test_ext(),
		crate::tests::mock::Test
	);
}
