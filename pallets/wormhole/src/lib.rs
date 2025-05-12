#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::{pallet_prelude::*, traits::Currency};
    use frame_system::pallet_prelude::*;
    use sp_std::vec::Vec;
    use codec::{Encode, Decode};
    // use wormhole_verifier::{WormholeVerifier, ProofWithPublicInputs, CircuitInputs};
    use plonky2::{
        plonk::{
            config::{GenericConfig, PoseidonGoldilocksConfig},
            circuit_data::{VerifierCircuitData, CommonCircuitData},
            proof::ProofWithPublicInputs
        },
        field::{goldilocks_field::GoldilocksField, types::PrimeField64},
        util::serialization::DefaultGateSerializer
    };
    use lazy_static::lazy_static;
    use pallet_balances::{Pallet as BalancesPallet, Config as BalancesConfig};

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

    const D: usize = 2;
    type C = PoseidonGoldilocksConfig;
    type F = <C as GenericConfig<D>>::F;

    #[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo)]
    pub struct WormholePublicInputs<T: Config> {
        pub nullifier: [u8; 64],
        pub exit_account: T::AccountId,
        pub exit_amount: u64,
        pub fee_amount: u64,
        pub storage_root: [u8; 32],
    }

    impl<T: Config> WormholePublicInputs<T> {
        // Convert from a vector of GoldilocksField elements
        pub fn from_fields(fields: &[GoldilocksField]) -> Result<Self, Error<T>> {
            if fields.len() < 16 { // Ensure we have enough fields
                return Err(Error::<T>::InvalidPublicInputs);
            }

            // Convert fields to bytes, each GoldilocksField is 8 bytes (u64)
            let mut nullifier = [0u8; 64];
            let mut account_bytes = [0u8; 32];
            let mut storage_root = [0u8; 32];

            // First 8 fields (64 bytes) are the nullifier
            for i in 0..8 {
                nullifier[i*8..(i+1)*8].copy_from_slice(&fields[i].to_canonical_u64().to_le_bytes());
            }

            // Next 4 fields (32 bytes) are the exit account
            for i in 0..4 {
                account_bytes[i*8..(i+1)*8].copy_from_slice(&fields[i+8].to_canonical_u64().to_le_bytes());
            }

            // Next field is exit amount
            let exit_amount = fields[12].to_canonical_u64();

            // Next field is fee amount
            let fee_amount = fields[13].to_canonical_u64();

            // Last 2 fields are storage root
            for i in 0..4 {
                storage_root[i*8..(i+1)*8].copy_from_slice(&fields[i+14].to_canonical_u64().to_le_bytes());
            }

            let exit_account = T::AccountId::decode(&mut &account_bytes[..])
                .map_err(|_| Error::<T>::InvalidPublicInputs)?;

            Ok(WormholePublicInputs {
                nullifier,
                exit_account,
                exit_amount,
                fee_amount,
                storage_root,
            })
        }
    }

    // Define the circuit data as a lazy static constant
    lazy_static! {
        static ref CIRCUIT_DATA: CommonCircuitData<F, D> = {
            let bytes = include_bytes!("../common.hex");
            CommonCircuitData::from_bytes(bytes.to_vec(), &DefaultGateSerializer)
                .expect("Failed to parse circuit data")
        };
        static ref VERIFIER_DATA: VerifierCircuitData<F, C, D> = {
            let bytes = include_bytes!("../verifier.hex");
            VerifierCircuitData::from_bytes(bytes.to_vec(), &DefaultGateSerializer)
                .expect("Failed to parse verifier data")
        };
        // static ref VERIFIER: WormholeVerifier = WormholeVerifier::new();
    }

    #[pallet::storage]
    #[pallet::getter(fn used_nullifiers)]
    pub(super) type UsedNullifiers<T: Config> = StorageMap<_, Blake2_128Concat, [u8; 64], bool, ValueQuery>;

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
        pub fn verify_wormhole_proof(
            origin: OriginFor<T>,
            proof_bytes: Vec<u8>,
        ) -> DispatchResult {
            ensure_none(origin)?;

            let proof = ProofWithPublicInputs::from_bytes(proof_bytes.clone(), &*CIRCUIT_DATA)
                .map_err(|_e| {
                    // log::error!("Proof deserialization failed: {:?}", e.to_string());
                    Error::<T>::ProofDeserializationFailed
                })?;

            let public_inputs = WormholePublicInputs::<T>::from_fields(&proof.public_inputs)?;
            // log::error!("{:?}", public_inputs.nullifier);
            // log::error!("{:?}", public_inputs.exit_account);
            // log::error!("{:?}", public_inputs.exit_amount);
            // log::error!("{:?}", public_inputs.storage_root);
            // log::error!("{:?}", public_inputs.fee_amount);


            // Verify nullifier hasn't been used
            ensure!(!UsedNullifiers::<T>::contains_key(&public_inputs.nullifier), Error::<T>::NullifierAlreadyUsed);

            VERIFIER_DATA.verify(proof)
                .map_err(|_e| {
                    // log::error!("Verification failed: {:?}", e.to_string());
                    Error::<T>::VerificationFailed
                })?;

            // Mark nullifier as used
            UsedNullifiers::<T>::insert(&public_inputs.nullifier, true);

            // let exit_balance: <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance
            let exit_balance: <T as BalancesConfig>::Balance
                = public_inputs.exit_amount
                .try_into()
                .map_err(|_| "Conversion from u64 to Balance failed")?;

            // Mint new tokens to the exit account
            let _ = BalancesPallet::<T>::deposit_creating(
                &public_inputs.exit_account,
                exit_balance
            );

            // // Emit event
            Self::deposit_event(Event::ProofVerified {
                exit_amount: exit_balance,
            });

            Ok(())
        }
    }
}
