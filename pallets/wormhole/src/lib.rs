#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
    use codec::Decode;
    use frame_support::{pallet_prelude::*, traits::Currency};
    use frame_system::pallet_prelude::*;
    use pallet_balances::{Config as BalancesConfig, Pallet as BalancesPallet};
    use sp_std::vec::Vec;
    use wormhole_circuit::circuit::{C, D, F};
    use wormhole_circuit::codec::ByteCodec;
    use wormhole_circuit::inputs::PublicCircuitInputs;
    use wormhole_verifier::ProofWithPublicInputs;
    use wormhole_verifier::WormholeVerifier;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + TypeInfo + pallet_balances::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        type WeightInfo: WeightInfo;
    }

    pub trait WeightInfo {
        fn verify_wormhole_proof() -> Weight;
    }

    pub struct DefaultWeightInfo;

    impl WeightInfo for DefaultWeightInfo {
        fn verify_wormhole_proof() -> Weight {
            Weight::from_parts(10_000, 0)
        }
    }

    #[pallet::storage]
    #[pallet::getter(fn used_nullifiers)]
    pub(super) type UsedNullifiers<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ProofVerified {
            exit_amount: <T as BalancesConfig>::Balance,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        InvalidProof,
        ProofDeserializationFailed,
        InvalidVerificationKey,
        NotInitialized,
        AlreadyInitialized,
        VerificationFailed,
        VerifierNotFound,
        InvalidPublicInputs,
        NullifierAlreadyUsed,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::verify_wormhole_proof())]
        pub fn verify_wormhole_proof(origin: OriginFor<T>, proof_bytes: Vec<u8>) -> DispatchResult {
            ensure_none(origin)?;

            // let proof = ProofWithPublicInputs::from_bytes(proof_bytes.clone(), &*CIRCUIT_DATA)
            // .map_err(|_e| {
            //     // log::error!("Proof deserialization failed: {:?}", e.to_string());
            //     Error::<T>::ProofDeserializationFailed
            // })?;
            let verifier = WormholeVerifier::default();
            let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
                proof_bytes,
                &verifier.circuit_data.common,
            )
            .map_err(|_| Error::<T>::ProofDeserializationFailed)?;

            let public_inputs = PublicCircuitInputs::try_from(proof.clone())
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;
            // log::error!("{:?}", public_inputs.nullifier);
            // log::error!("{:?}", public_inputs.exit_account);
            // log::error!("{:?}", public_inputs.exit_amount);
            // log::error!("{:?}", public_inputs.storage_root);
            // log::error!("{:?}", public_inputs.fee_amount);

            // Verify nullifier hasn't been used
            // ensure!(!UsedNullifiers::<T>::contains_key(&public_inputs.nullifier), Error::<T>::NullifierAlreadyUsed);
            let nullifier_bytes_vec = public_inputs.nullifier.to_bytes();
            let nullifier_bytes: [u8; 32] = nullifier_bytes_vec
                .try_into()
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;
            ensure!(
                !UsedNullifiers::<T>::contains_key(&nullifier_bytes),
                Error::<T>::NullifierAlreadyUsed
            );

            verifier.verify(proof).map_err(|_e| {
                // log::error!("Verification failed: {:?}", e.to_string());
                Error::<T>::VerificationFailed
            })?;

            // Mark nullifier as used
            UsedNullifiers::<T>::insert(&nullifier_bytes, true);

            let exit_balance: <T as BalancesConfig>::Balance = public_inputs
                .funding_amount
                .try_into()
                .map_err(|_| "Conversion from u64 to Balance failed")?;

            // TODO: handle fee amount, should go to miner

            // Mint new tokens to the exit account
            let exit_account_bytes = public_inputs.exit_account.to_bytes();
            let exit_account = T::AccountId::decode(&mut &exit_account_bytes[..])
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;
            let _ = BalancesPallet::<T>::deposit_creating(&exit_account, exit_balance);

            // Emit event
            Self::deposit_event(Event::ProofVerified {
                exit_amount: exit_balance,
            });

            Ok(())
        }
    }
}
