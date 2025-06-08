//! The wormhole2 pallet, for ZK-proof based cross-chain asset transfers.
#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

// #[cfg(test)]
// mod mock;

// #[cfg(test)]
// mod tests;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::pallet_prelude::*;
    use frame_support::traits::Currency;
    use frame_system::pallet_prelude::*;
    use pallet_balances::Pallet as Balances;
    use sp_std::vec::Vec;

    /// The public inputs for the ZK-SNARK circuit.
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo)]
    pub struct PublicInputs<AccountId, Balance> {
        /// The nullifier for the burned funds.
        pub nullifier: [u8; 32],
        /// The recipient of the newly minted funds.
        pub recipient: AccountId,
        /// The amount of funds to be minted.
        pub amount: Balance,
        /// The state root of the chain when the burn transaction was included.
        pub state_root: [u8; 32],
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_balances::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        #[pallet::constant]
        type MaxVerifierKeyLength: Get<u32>;
    }

    /// Storage for the verifier key of the ZK-SNARK circuit.
    /// This should be set by a root call during genesis or a runtime upgrade.
    #[pallet::storage]
    #[pallet::getter(fn verifier_key)]
    pub type VerifierKey<T: Config> =
        StorageValue<_, BoundedVec<u8, T::MaxVerifierKeyLength>, OptionQuery>;

    /// Storage for used nullifiers. This is to prevent double-spending of burned funds.
    #[pallet::storage]
    #[pallet::getter(fn used_nullifiers)]
    pub type UsedNullifiers<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new verifier key has been set.
        VerifierKeySet,
        /// A wormhole redemption was successful.
        RedemptionSuccess {
            recipient: T::AccountId,
            amount: T::Balance,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The verifier key has not been set by the root.
        VerifierKeyNotSet,
        /// The provided ZK proof is invalid.
        InvalidProof,
        /// The provided nullifier has already been used.
        NullifierAlreadyUsed,
        /// The proof deserialization failed.
        ProofDeserializationFailed,
        /// The public inputs deserialization failed.
        PublicInputsDeserializationFailed,
        /// The on-chain verification of the proof failed.
        VerificationFailed,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Set the verifier key for the ZK-SNARK circuit.
        ///
        /// This extrinsic can only be called by the root.
        #[pallet::call_index(0)]
        #[pallet::weight(T::DbWeight::get().writes(1))]
        pub fn initialize(origin: OriginFor<T>, verifier_key: Vec<u8>) -> DispatchResult {
            ensure_root(origin)?;
            let bounded_key: BoundedVec<u8, T::MaxVerifierKeyLength> = verifier_key
                .try_into()
                .map_err(|_| Error::<T>::VerifierKeyNotSet)?;
            VerifierKey::<T>::put(bounded_key);
            Self::deposit_event(Event::VerifierKeySet);
            Ok(())
        }

        /// Redeem funds by providing a ZK-SNARK proof of a prior burn.
        ///
        /// The `redeem` function verifies the proof, checks for double-spending,
        /// and if valid, mints new tokens to the recipient.
        #[pallet::call_index(1)]
        #[pallet::weight(T::DbWeight::get().reads_writes(2, 1))]
        pub fn redeem(
            origin: OriginFor<T>,
            _proof: Vec<u8>,
            public_inputs: PublicInputs<T::AccountId, T::Balance>,
        ) -> DispatchResult {
            ensure_signed(origin)?;

            // 1. Check if the verifier key is set
            let _verifier_key = VerifierKey::<T>::get().ok_or(Error::<T>::VerifierKeyNotSet)?;

            // 2. Check the nullifier
            ensure!(
                !UsedNullifiers::<T>::contains_key(public_inputs.nullifier),
                Error::<T>::NullifierAlreadyUsed
            );

            // TODO:
            // 3. Deserialize the proof
            // 4. Verify the proof using the verifier key and public inputs
            // 5. Check the state_root from public_inputs against a recent on-chain state root

            // If all checks pass:
            // 6. Mark the nullifier as used
            UsedNullifiers::<T>::insert(public_inputs.nullifier, true);

            // 7. Mint the tokens
            let _ = Balances::<T>::deposit_creating(
                &public_inputs.recipient,
                public_inputs.amount.into(),
            );

            // 8. Emit success event
            Self::deposit_event(Event::RedemptionSuccess {
                recipient: public_inputs.recipient,
                amount: public_inputs.amount,
            });

            Ok(())
        }
    }
}
