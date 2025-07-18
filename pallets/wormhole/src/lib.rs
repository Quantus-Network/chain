#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use lazy_static::lazy_static;
pub use pallet::*;
use wormhole_verifier::WormholeVerifier;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;
pub use weights::*;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

lazy_static! {
    static ref WORMHOLE_VERIFIER: Option<WormholeVerifier> = {
        let verifier_bytes = include_bytes!("../verifier.bin");
        let common_bytes = include_bytes!("../common.bin");
        WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes).ok()
    };
}

// Add a safe getter function
pub fn get_wormhole_verifier() -> Result<&'static WormholeVerifier, &'static str> {
    WORMHOLE_VERIFIER
        .as_ref()
        .ok_or("Wormhole verifier not available")
}

#[frame_support::pallet]
pub mod pallet {
    use crate::WeightInfo;
    use alloc::vec::Vec;
    use codec::Decode;
    use frame_support::pallet_prelude::*;
    use frame_support::traits::fungible::{Mutate, Unbalanced};
    use frame_support::{
        traits::{Currency, ExistenceRequirement, WithdrawReasons},
        weights::WeightToFee,
    };
    use frame_system::pallet_prelude::*;
    use qp_wormhole::TransferProofs;
    use sp_runtime::{
        traits::{Saturating, Zero},
        Perbill,
    };
    use wormhole_circuit::inputs::{PublicCircuitInputs, PUBLIC_INPUTS_FELTS_LEN};
    use wormhole_verifier::ProofWithPublicInputs;
    use zk_circuits_common::circuit::{C, D, F};

    pub type BalanceOf<T> =
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Overarching runtime event type
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Currency type used for minting tokens and handling wormhole transfers
        type Currency: Mutate<Self::AccountId, Balance = BalanceOf<Self>>
            + TransferProofs<BalanceOf<Self>, Self::AccountId>
            + Unbalanced<Self::AccountId>
            + Currency<Self::AccountId>;

        /// Account ID used as the "from" account when creating transfer proofs for minted tokens
        #[pallet::constant]
        type MintingAccount: Get<Self::AccountId>;

        /// Weight information for pallet operations.
        type WeightInfo: WeightInfo;

        type WeightToFee: WeightToFee<Balance = BalanceOf<Self>>;
    }

    #[pallet::storage]
    #[pallet::getter(fn used_nullifiers)]
    pub(super) type UsedNullifiers<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ProofVerified { exit_amount: BalanceOf<T> },
    }

    #[pallet::error]
    pub enum Error<T> {
        InvalidProof,
        ProofDeserializationFailed,
        VerificationFailed,
        InvalidPublicInputs,
        NullifierAlreadyUsed,
        VerifierNotAvailable,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::verify_wormhole_proof())]
        pub fn verify_wormhole_proof(origin: OriginFor<T>, proof_bytes: Vec<u8>) -> DispatchResult {
            ensure_none(origin)?;

            let verifier =
                crate::get_wormhole_verifier().map_err(|_| Error::<T>::VerifierNotAvailable)?;

            let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
                proof_bytes,
                &verifier.circuit_data.common,
            )
            .map_err(|_| Error::<T>::ProofDeserializationFailed)?;

            ensure!(
                proof.public_inputs.len() == PUBLIC_INPUTS_FELTS_LEN,
                Error::<T>::InvalidPublicInputs
            );

            // Parse public inputs using the existing parser
            let public_inputs = PublicCircuitInputs::try_from(proof.clone())
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;

            let nullifier_bytes = *public_inputs.nullifier;

            // Verify nullifier hasn't been used
            ensure!(
                !UsedNullifiers::<T>::contains_key(nullifier_bytes),
                Error::<T>::NullifierAlreadyUsed
            );

            verifier
                .verify(proof.clone())
                .map_err(|_| Error::<T>::VerificationFailed)?;

            // Mark nullifier as used
            UsedNullifiers::<T>::insert(nullifier_bytes, true);

            let exit_balance_u128 = public_inputs.funding_amount;

            // Convert to Balance type
            let exit_balance: BalanceOf<T> = exit_balance_u128
                .try_into()
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;

            // Decode exit account from public inputs
            let exit_account_bytes = *public_inputs.exit_account;
            let exit_account = T::AccountId::decode(&mut &exit_account_bytes[..])
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;

            // Calculate fees first
            let weight = <T as Config>::WeightInfo::verify_wormhole_proof();
            let weight_fee = T::WeightToFee::weight_to_fee(&weight);
            let volume_fee_perbill = Perbill::from_rational(1u32, 1000u32);
            let volume_fee = volume_fee_perbill * exit_balance;
            let total_fee = weight_fee.saturating_add(volume_fee);

            // Mint tokens to the exit account
            // This does not affect total issuance and does not create an imbalance
            <T::Currency as Unbalanced<_>>::increase_balance(
                &exit_account,
                exit_balance.into(),
                frame_support::traits::tokens::Precision::Exact,
            )?;

            // Withdraw fee from exit account if fees are non-zero
            // This creates a negative imbalance that will be handled by the transaction payment pallet
            if !total_fee.is_zero() {
                let _fee_imbalance = T::Currency::withdraw(
                    &exit_account,
                    total_fee,
                    WithdrawReasons::TRANSACTION_PAYMENT,
                    ExistenceRequirement::KeepAlive,
                )?;
            }

            // Create a transfer proof for the minted tokens
            let mint_account = T::MintingAccount::get();
            T::Currency::store_transfer_proof(&mint_account, &exit_account, exit_balance);

            // Emit event
            Self::deposit_event(Event::ProofVerified {
                exit_amount: exit_balance,
            });

            Ok(())
        }
    }
}
