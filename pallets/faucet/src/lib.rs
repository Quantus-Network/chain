// pallets/faucet/src/lib.rs
#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
    pallet_prelude::*,
    traits::{Currency, fungible::Mutate},
    weights::Weight,
};
use frame_support::dispatch::RawOrigin;
use frame_system::pallet_prelude::*;
use sp_runtime::traits::StaticLookup;

pub use pallet::*;

// Define the BalanceOf type using the Inspect trait for consistency with Mutate
pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

#[frame_support::pallet]
pub mod pallet {
    use super::*;

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_balances::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The currency type (defines the token type used in transfers)
        type Currency: Currency<Self::AccountId> + Mutate<Self::AccountId>;

        /// Faucet account (source of transferred tokens)
        #[pallet::constant]
        type FaucetAccount: Get<Self::AccountId>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Tokens were successfully transferred
        TokensTransferred {
            recipient: T::AccountId,
            amount: BalanceOf<T>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Transfer failed
        TransferFailed,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Transfer tokens from the faucet account to a recipient
        ///
        /// This function can only be called by None origin (through runtime API)
        /// or by Root origin (through sudo).
        #[pallet::call_index(0)]
        #[pallet::weight(Weight::from_parts(10_000, 0))]
        pub fn transfer(
            origin: OriginFor<T>,
            dest: <T::Lookup as StaticLookup>::Source,
            #[pallet::compact] value: BalanceOf<T>,
        ) -> DispatchResult {
            // Require None origin (from runtime API) or Root
            ensure_none_or_root(origin)?;

            // Get the faucet account from configuration
            //let faucet_account = T::FaucetAccount::get();

            // Get the destination address
            let dest = T::Lookup::lookup(dest)?;


            let balance = T::Currency::free_balance(&dest);
            log::info!("-------------------------------------------------------------------------------Before balance: {:?}", balance);

            // Execute the transfer using fungible::Mutate trait
            // <T::Currency as Mutate<T::AccountId>>::transfer(
            //     &faucet_account,
            //     &dest,
            //     value,
            //     Preservation::Preserve,
            // )?;
            let minted = T::Currency::issue(value);

            T::Currency::resolve_creating(&dest, minted);

            let balance = T::Currency::free_balance(&dest);
            log::info!("-------------------------------------------------------------------------------After balance: {:?}", balance);

            // Emit the transfer event
            Self::deposit_event(Event::TokensTransferred {
                recipient: dest,
                amount: value
            });

            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            match call {
                Call::transfer { dest, value } => {
                    ValidTransaction::with_tag_prefix("Faucet")
                        .priority(100)
                        .longevity(64)
                        .propagate(true)
                        .and_provides((dest, value))  // Dodajemy tagi
                        .build()
                }
                _ => Err(TransactionValidityError::Invalid(InvalidTransaction::Call)),
            }
        }
    }
}

// Helper function to check if origin is None or Root
fn ensure_none_or_root<OuterOrigin, AccountId>(o: OuterOrigin) -> DispatchResult
where
    OuterOrigin: Into<Result<RawOrigin<AccountId>, OuterOrigin>>,
{
    match o.into() {
        Ok(RawOrigin::Root) => Ok(()),
        Ok(RawOrigin::None) => Ok(()),
        _ => Err(DispatchError::BadOrigin),
    }
}