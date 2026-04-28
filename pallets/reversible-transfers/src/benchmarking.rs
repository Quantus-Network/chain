//! Benchmarking setup for pallet-reversible-transfers

use super::*;

use crate::Pallet as ReversibleTransfers; // Alias the pallet
use frame_benchmarking::{account as benchmark_account, v2::*, BenchmarkError};
use frame_support::traits::{fungible::Mutate, fungibles::Create, Get};
use frame_system::RawOrigin;
use sp_runtime::{
	traits::{BlockNumberProvider, Hash, One, StaticLookup},
	Saturating,
};

const SEED: u32 = 0;

/// Helper for external benchmarks (e.g., `pallet-multisig`) to set up HS storage state.
/// Bypasses all validation - direct storage write only for benchmarking.
pub fn insert_hs_account_for_benchmark<T>(
	who: T::AccountId,
	data: HighSecurityAccountData<T::AccountId, BlockNumberOrTimestampOf<T>>,
) where
	T: Config,
{
	HighSecurityAccounts::<T>::insert(who, data);
}

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
	guardian: T::AccountId,
) {
	HighSecurityAccounts::<T>::insert(who, HighSecurityAccountData { delay, guardian });
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
    T: Config + pallet_balances::Config + pallet_assets::Config,
    <T as pallet_balances::Config>::Balance: From<u128> + Into<u128>,
    <T as pallet_assets::Config>::AssetId: From<u32>,
    RuntimeCallOf<T>: From<pallet_balances::Call<T>> + From<frame_system::Call<T>> + From<pallet_assets::Call<T>>,
)]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn set_high_security() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf::<T>::from(10000u128));
		let guardian: T::AccountId = benchmark_account("guardian", 0, SEED);
		let delay: BlockNumberOrTimestampOf<T> = T::DefaultDelay::get();

		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), delay, guardian.clone());

		assert_eq!(
			HighSecurityAccounts::<T>::get(&caller),
			Some(HighSecurityAccountData { delay, guardian })
		);

		Ok(())
	}

	#[benchmark]
	fn schedule_transfer() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf::<T>::from(10000u128));
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
		let guardian: T::AccountId = benchmark_account("guardian", 1, SEED);
		let transfer_amount = 100u128;

		// Setup caller as reversible
		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(caller.clone(), delay, guardian.clone());

		let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;
		let current_tx_id = NextTransactionId::<T>::get();
		let tx_id = T::Hashing::hash_of(&(caller.clone(), call, current_tx_id).encode());

		let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient);
		// Schedule the dispatch
		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), recipient_lookup, transfer_amount.into());

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
	fn schedule_asset_transfer() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		fund_account::<T>(&caller, BalanceOf::<T>::from(10000u128));
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
		let guardian: T::AccountId = benchmark_account("guardian", 1, SEED);
		let transfer_amount: BalanceOf<T> = 100u128.into();

		// Create and mint an asset for the benchmark
		let asset_id: <T as pallet_assets::Config>::AssetId = 1u32.into();
		let min_balance: BalanceOf<T> = 1u128.into();

		// Create the asset with caller as admin
		<pallet_assets::Pallet<T> as Create<T::AccountId>>::create(
			asset_id.clone(),
			caller.clone(),
			true, // is_sufficient
			min_balance,
		)?;

		// Mint more assets than transfer amount to ensure sufficient balance for hold
		let mint_amount: BalanceOf<T> = 10000u128.into();
		<pallet_assets::Pallet<T> as frame_support::traits::fungibles::Mutate<T::AccountId>>::mint_into(
			asset_id.clone(),
			&caller,
			mint_amount,
		)?;

		// Setup caller as high security
		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(caller.clone(), delay, guardian.clone());

		// Build the expected call for tx_id calculation
		let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient.clone());
		let asset_call: RuntimeCallOf<T> = pallet_assets::Call::<T>::transfer_keep_alive {
			id: asset_id.clone().into(),
			target: recipient_lookup.clone(),
			amount: transfer_amount,
		}
		.into();
		let current_tx_id = NextTransactionId::<T>::get();
		let tx_id = T::Hashing::hash_of(&(caller.clone(), asset_call, current_tx_id).encode());

		// Schedule the asset transfer
		#[extrinsic_call]
		_(RawOrigin::Signed(caller.clone()), asset_id, recipient_lookup, transfer_amount);

		assert!(PendingTransfers::<T>::contains_key(tx_id));
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
		let guardian: T::AccountId = benchmark_account("guardian", 1, SEED);

		fund_account::<T>(&caller, BalanceOf::<T>::from(10000u128));
		fund_account::<T>(&guardian, BalanceOf::<T>::from(10000u128));
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
		let transfer_amount = 100u128;

		// Setup caller as reversible and schedule a task in setup
		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(caller.clone(), delay, guardian.clone());

		let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;

		let origin = RawOrigin::Signed(caller.clone()).into();

		let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient);
		let current_tx_id = NextTransactionId::<T>::get();
		let tx_id = T::Hashing::hash_of(&(caller.clone(), call, current_tx_id).encode());

		ReversibleTransfers::<T>::do_schedule_transfer(
			origin,
			recipient_lookup,
			transfer_amount.into(),
		)?;

		// Ensure setup worked before benchmarking cancel
		assert!(PendingTransfers::<T>::contains_key(tx_id));

		// Benchmark the cancel extrinsic
		#[extrinsic_call]
		_(RawOrigin::Signed(guardian), tx_id);

		assert!(!PendingTransfers::<T>::contains_key(tx_id));
		// Check scheduler cancelled (agenda item removed)
		let task_name = ReversibleTransfers::<T>::make_schedule_id(&tx_id)?;
		assert!(T::Scheduler::next_dispatch_time(task_name).is_err());

		Ok(())
	}

	#[benchmark]
	fn execute_transfer() -> Result<(), BenchmarkError> {
		let owner: T::AccountId = whitelisted_caller();
		fund_account::<T>(&owner, BalanceOf::<T>::from(10000u128));
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);
		fund_account::<T>(&recipient, BalanceOf::<T>::from(100u128));
		let guardian: T::AccountId = benchmark_account("guardian", 1, SEED);
		let transfer_amount = 100u128;

		// Setup owner as reversible and schedule a task in setup
		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(owner.clone(), delay, guardian);
		let call = make_transfer_call::<T>(recipient.clone(), transfer_amount)?;

		let owner_origin = RawOrigin::Signed(owner.clone()).into();
		let recipient_lookup = <T as frame_system::Config>::Lookup::unlookup(recipient.clone());
		let current_tx_id = NextTransactionId::<T>::get();
		let tx_id = T::Hashing::hash_of(&(owner.clone(), call, current_tx_id).encode());

		ReversibleTransfers::<T>::do_schedule_transfer(
			owner_origin,
			recipient_lookup,
			transfer_amount.into(),
		)?;

		// Ensure setup worked
		assert!(PendingTransfers::<T>::contains_key(tx_id));

		let pallet_account = pallet_account::<T>();
		fund_account::<T>(&pallet_account, BalanceOf::<T>::from(10000u128));
		let execute_origin = RawOrigin::Signed(pallet_account);

		#[extrinsic_call]
		_(execute_origin, tx_id);

		assert!(!PendingTransfers::<T>::contains_key(tx_id));

		Ok(())
	}

	#[benchmark]
	fn recover_funds(n: Linear<0, 16>) -> Result<(), BenchmarkError> {
		assert_eq!(
			T::MaxPendingPerAccount::get(),
			16,
			"Linear upper bound must match MaxPendingPerAccount"
		);

		let account: T::AccountId = whitelisted_caller();
		let guardian: T::AccountId = benchmark_account("guardian", 0, SEED);
		let recipient: T::AccountId = benchmark_account("recipient", 0, SEED);

		fund_account::<T>(&account, BalanceOf::<T>::from(1_000_000u128));
		fund_account::<T>(&guardian, BalanceOf::<T>::from(10000u128));

		let delay = T::DefaultDelay::get();
		setup_high_security_account::<T>(account.clone(), delay, guardian.clone());

		let transfer_amount: BalanceOf<T> = 100u128.into();
		for i in 0..n {
			if i > 0 && i % 8 == 0 {
				let bn = frame_system::Pallet::<T>::block_number();
				frame_system::Pallet::<T>::set_block_number(bn + BlockNumberFor::<T>::one());
			}
			let lookup = <T as frame_system::Config>::Lookup::unlookup(recipient.clone());
			ReversibleTransfers::<T>::do_schedule_transfer(
				RawOrigin::Signed(account.clone()).into(),
				lookup,
				transfer_amount,
			)?;
		}

		#[extrinsic_call]
		_(RawOrigin::Signed(guardian.clone()), account.clone());

		assert_eq!(PendingTransfersBySender::<T>::get(&account).len(), 0);

		Ok(())
	}

	impl_benchmark_test_suite!(
		ReversibleTransfers,
		crate::tests::mock::new_test_ext(),
		crate::tests::mock::Test
	);
}
