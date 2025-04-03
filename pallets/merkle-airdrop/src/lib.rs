#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use frame_support::traits::Currency;
    use sp_std::prelude::*;

    #[pallet::pallet]
    #[pallet::without_storage_info]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        
        /// The currency mechanism.
        type Currency: Currency<Self::AccountId>;
        
        /// The maximum number of airdrops that can be active at once.
        #[pallet::constant]
        type MaxAirdrops: Get<u32>;

        /// The pallet id, used for deriving its sovereign account ID.
        #[pallet::constant]
        type PalletId: Get<frame_support::PalletId>;
    }

    /// Type for storing a Merkle root hash
    pub type MerkleRoot = [u8; 32];
    
    /// Airdrop ID type
    pub type AirdropId = u32;

    /// Storage for Merkle roots of each airdrop
    #[pallet::storage]
    #[pallet::getter(fn airdrop_merkle_roots)]
    pub type AirdropMerkleRoots<T> = StorageMap<_, Blake2_128Concat, AirdropId, MerkleRoot>;

    /// Storage for airdrop balances
    #[pallet::storage]
    #[pallet::getter(fn airdrop_balances)]
    pub type AirdropBalances<T: Config> = StorageMap<
        _, 
        Blake2_128Concat, 
        AirdropId, 
        <<T as Config>::Currency as Currency<T::AccountId>>::Balance
    >;

    /// Storage for claimed status
    #[pallet::storage]
    #[pallet::getter(fn is_claimed)]
    pub type Claimed<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat, AirdropId,
        Blake2_128Concat, T::AccountId,
        bool, ValueQuery
    >;

    /// Counter for airdrop IDs
    #[pallet::storage]
    #[pallet::getter(fn next_airdrop_id)]
    pub type NextAirdropId<T> = StorageValue<_, AirdropId, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new airdrop has been created
        AirdropCreated {
            airdrop_id: AirdropId,
            merkle_root: MerkleRoot,
        },
        /// An airdrop has been funded
        AirdropFunded {
            airdrop_id: AirdropId,
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
        },
        /// A claim has been processed
        Claimed {
            airdrop_id: AirdropId,
            account: T::AccountId,
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Airdrop already exists
        AirdropAlreadyExists,
        /// Insufficient funds in the airdrop
        InsufficientAirdropBalance,
        /// User has already claimed from this airdrop
        AlreadyClaimed,
        /// Invalid Merkle proof
        InvalidProof,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Create a new airdrop with a Merkle root
        #[pallet::call_index(0)]
        #[pallet::weight(10_000)]
        pub fn create_airdrop(
            origin: OriginFor<T>,
            merkle_root: MerkleRoot,
        ) -> DispatchResult {
            let _who = ensure_signed(origin)?;
            
            // TODO
            
            Ok(())
        }

        /// Fund an existing airdrop
        #[pallet::call_index(1)]
        #[pallet::weight(10_000)]
        pub fn fund_airdrop(
            origin: OriginFor<T>,
            airdrop_id: AirdropId,
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
        ) -> DispatchResult {
            let _who = ensure_signed(origin)?;
            
            // TODO
            Ok(())
        }

        /// Claim tokens from an airdrop
        #[pallet::call_index(2)]
        #[pallet::weight(10_000)]
        pub fn claim(
            origin: OriginFor<T>,
            airdrop_id: AirdropId,
            amount: <<T as Config>::Currency as Currency<T::AccountId>>::Balance,
            merkle_proof: Vec<[u8; 32]>,
        ) -> DispatchResult {
            let _who = ensure_signed(origin)?;
            
            // TODO
            
            Ok(())
        }
    }
}

pub use pallet::*; 