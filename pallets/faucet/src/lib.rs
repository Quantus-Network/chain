#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
    pallet_prelude::*,
    traits::{fungible::Mutate, Currency},
    weights::Weight,
};
use frame_system::pallet_prelude::*;
use sp_runtime::traits::StaticLookup;

pub use pallet::*;

// Define the BalanceOf type using the Inspect trait for consistency with Mutate
pub type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

#[frame_support::pallet]
pub mod pallet {
    use super::*;

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_balances::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The currency type (defines the token type used in transfers)
        type Currency: Currency<Self::AccountId> + Mutate<Self::AccountId>;

        #[pallet::constant]
        type MaxTokenAmount: Get<BalanceOf<Self>>;

        #[pallet::constant]
        type DefaultMintAmount: Get<BalanceOf<Self>>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Tokens were successfully transferred
        TokensMinted {
            recipient: T::AccountId,
            amount: BalanceOf<T>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        MintFailed,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// mint new tokens
        #[pallet::call_index(0)]
        #[pallet::weight(Weight::from_parts(10_000, 0))]
        pub fn mint_new_tokens(
            _origin: OriginFor<T>,
            dest: <T::Lookup as StaticLookup>::Source,
            _seed: u64
        ) -> DispatchResult {
            // Get the destination address
            let dest = T::Lookup::lookup(dest)?;

            let balance = T::Currency::free_balance(&dest);

            if balance < T::MaxTokenAmount::get() {
                let minted = T::Currency::issue(T::DefaultMintAmount::get());

                T::Currency::resolve_creating(&dest, minted);

                // Emit the mint event
                Self::deposit_event(Event::TokensMinted {
                    recipient: dest,
                    amount: T::DefaultMintAmount::get(),
                });
            } else {
                log::info!(
                    "Mint failed for {:?} - address has reached max token amount",
                    dest
                );
            }
            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            match call {
                Call::mint_new_tokens { dest, seed} => ValidTransaction::with_tag_prefix("Faucet")
                    .priority(100)
                    .longevity(64)
                    .propagate(true)
                    .and_provides((dest,seed,<frame_system::Pallet<T>>::block_number()))
                    .build(),
                _ => Err(TransactionValidityError::Invalid(InvalidTransaction::Call)),
            }
        }
    }
}
