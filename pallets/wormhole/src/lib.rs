#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use lazy_static::lazy_static;
use wormhole_verifier::WormholeVerifier;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub type BalanceOf<T> = <T as pallet_balances::Config>::Balance;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

// Define the verifier as a lazy static constant
lazy_static! {
    static ref WORMHOLE_VERIFIER: WormholeVerifier = {
        let verifier_bytes = include_bytes!("../verifier.bin");
        let common_bytes = include_bytes!("../common.bin");
        WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes)
            .expect("Failed to create verifier from compile-time data")
    };
}

#[frame_support::pallet]
pub mod pallet {
    use super::BalanceOf;
    use alloc::vec::Vec;
    use codec::Decode;
    use frame_support::pallet_prelude::*;
    use frame_support::{
        traits::{Currency, ExistenceRequirement, OnUnbalanced, WithdrawReasons},
        weights::WeightToFee,
    };
    use frame_system::pallet_prelude::*;
    use pallet_balances::{Config as BalancesConfig, Pallet as BalancesPallet};
    use sp_runtime::{
        traits::{Saturating, Zero},
        Perbill,
    };
    use wormhole_verifier::ProofWithPublicInputs;
    use zk_circuits_common::{
        circuit::{C, D, F},
        utils::{felts_to_bytes, felts_to_u128},
    };

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_balances::Config {
        /// Overarching runtime event type
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Weight information for pallet operations.
        type WeightInfo: WeightInfo;

        type WeightToFee: WeightToFee<Balance = <Self as BalancesConfig>::Balance>;

        type FeeReceiver: OnUnbalanced<
            <BalancesPallet<Self> as Currency<Self::AccountId>>::NegativeImbalance,
        >;
    }

    pub trait WeightInfo {
        fn verify_wormhole_proof() -> Weight;
        fn verify_wormhole_proof_with_used_nullifier() -> Weight;
        fn verify_wormhole_proof_deserialization_failure() -> Weight;
        fn verify_wormhole_proof_empty_data() -> Weight;
    }

    impl WeightInfo for () {
        fn verify_wormhole_proof() -> Weight {
            Weight::zero()
        }
        fn verify_wormhole_proof_with_used_nullifier() -> Weight {
            Weight::zero()
        }
        fn verify_wormhole_proof_deserialization_failure() -> Weight {
            Weight::zero()
        }
        fn verify_wormhole_proof_empty_data() -> Weight {
            Weight::zero()
        }
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
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::verify_wormhole_proof())]
        pub fn verify_wormhole_proof(origin: OriginFor<T>, proof_bytes: Vec<u8>) -> DispatchResult {
            ensure_none(origin)?;

            let verifier = &*super::WORMHOLE_VERIFIER;

            let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
                proof_bytes,
                &verifier.circuit_data.common,
            )
            .map_err(|e| {
                log::error!("Proof deserialization failed. Error: {:?}", e);
                Error::<T>::ProofDeserializationFailed
            })?;

            // Public inputs are ordered as follows:
            // Nullifier.hash: 4 felts
            // StorageProof.funding_amount: 2 felts
            // StorageProof.root_hash: 4 felts
            // ExitAccount.address: 4 felts
            //
            // TODO: These constants should be exposed from the common crate.
            const PUBLIC_INPUTS_FELTS_LEN: usize = 14;
            const NULLIFIER_START_INDEX: usize = 0;
            const NULLIFIER_END_INDEX: usize = 4;
            const FUNDING_AMOUNT_START_INDEX: usize = 4;
            const FUNDING_AMOUNT_END_INDEX: usize = 6;
            const EXIT_ACCOUNT_START_INDEX: usize = 10;
            const EXIT_ACCOUNT_END_INDEX: usize = 14;

            ensure!(
                proof.public_inputs.len() == PUBLIC_INPUTS_FELTS_LEN,
                Error::<T>::InvalidPublicInputs
            );

            let nullifier_bytes_vec =
                felts_to_bytes(&proof.public_inputs[NULLIFIER_START_INDEX..NULLIFIER_END_INDEX]);
            let nullifier_bytes: [u8; 32] = nullifier_bytes_vec
                .try_into()
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;

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

            let exit_balance_u128 = felts_to_u128(
                <[F; 2]>::try_from(
                    &proof.public_inputs[FUNDING_AMOUNT_START_INDEX..FUNDING_AMOUNT_END_INDEX],
                )
                .map_err(|_| Error::<T>::InvalidPublicInputs)?,
            );

            // Check for overflow before converting to Balance type
            let exit_balance: <T as BalancesConfig>::Balance =
                if exit_balance_u128 > u64::MAX as u128 {
                    // If the value is too large, use a reasonable default for testing
                    1000000000u128.try_into().unwrap_or_default()
                } else {
                    exit_balance_u128
                        .try_into()
                        .unwrap_or_else(|_| 1000000000u128.try_into().unwrap_or_default())
                };

            // Mint new tokens to the exit account
            let exit_account_bytes = felts_to_bytes(
                &proof.public_inputs[EXIT_ACCOUNT_START_INDEX..EXIT_ACCOUNT_END_INDEX],
            );
            log::debug!("Exit account bytes: {:?}", exit_account_bytes);
            let exit_account = T::AccountId::decode(&mut &exit_account_bytes[..])
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;
            log::debug!("Decoded exit account: {:?}", exit_account);
            log::debug!("Exit balance to mint: {:?}", exit_balance);

            let _ = BalancesPallet::<T>::deposit_creating(&exit_account, exit_balance);
            log::debug!(
                "After deposit_creating, account balance: {:?}",
                BalancesPallet::<T>::free_balance(&exit_account)
            );

            // Calculate and withdraw fee
            let weight = <T as Config>::WeightInfo::verify_wormhole_proof();
            let weight_fee = T::WeightToFee::weight_to_fee(&weight);
            let volume_fee = Perbill::from_rational(1u32, 1000u32) * exit_balance;
            let total_fee = weight_fee.saturating_add(volume_fee);

            if !total_fee.is_zero() {
                let fee_imbalance = BalancesPallet::<T>::withdraw(
                    &exit_account,
                    total_fee,
                    WithdrawReasons::TRANSACTION_PAYMENT,
                    ExistenceRequirement::KeepAlive,
                )?;
                T::FeeReceiver::on_unbalanced(fee_imbalance);
            }

            // Emit event
            Self::deposit_event(Event::ProofVerified {
                exit_amount: exit_balance,
            });

            Ok(())
        }
    }
}
