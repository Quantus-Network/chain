//! PoC implementation of the LayerZero endpoint as a Substrate pallet.
//!
//! The current version tries to simplify the flow as much as possible, and serves as a proof of concept.

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

extern crate alloc;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use alloc::{vec, vec::Vec};
    use frame_support::{
        dispatch::{DispatchResult, GetDispatchInfo, Pays},
        pallet_prelude::*,
        traits::{fungibles::Transfer, Get},
    };
    use frame_system::pallet_prelude::*;
    use sp_core::{H256, U256};
    use sp_runtime::traits::{
        DispatchInfoOf, Hash, Keccak256, PostDispatchInfoOf, ValidateUnsigned,
    };

    // Type alias for EVM-style addresses.
    pub type Address = [u8; 20];

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// Message receiver impl.
        type MessageReceiver: Receive<Self::Hash>;
        /// Sender library impl.
        type SendLibrary: SendLib<Self::Hash>;
        type RuntimeCall: Parameter + GetDispatchInfo;
        /// Token id
        #[pallet::constant]
        type LzTokenId: Get<u32>;
        /// Type alias for fungible assets.
        type Fungibles: Transfer<Self::AccountId, AssetId = u32, Balance = u128>;
    }

    #[derive(Clone, Encode, Decode, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
    pub struct Origin {
        pub src_eid: u32,
        pub sender: Address,
        pub nonce: u64,
    }

    #[derive(Clone, Encode, Decode, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
    pub struct MessagingParams {
        pub dst_eid: u32,
        pub receiver: Address,
        pub message: Vec<u8>,
        pub options: Vec<u8>,
        pub pay_in_lz_token: bool,
    }

    #[derive(Clone, Encode, Decode, PartialEq, Eq, TypeInfo, MaxEncodedLen, Default, Debug)]
    pub struct MessagingFee {
        pub native_fee: u128,
        pub lz_token_fee: u128,
    }

    #[derive(Clone, Encode, Decode, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
    pub struct MessagingReceipt<Hash> {
        pub guid: Hash,
        pub nonce: u64,
        pub fee: MessagingFee,
    }

    #[derive(Clone, Encode, Decode, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
    pub struct Packet<Hash> {
        pub nonce: u64,
        pub src_eid: u32,
        pub sender: Address,
        pub dst_eid: u32,
        pub receiver: Address,
        pub guid: Hash,
        pub message: Vec<u8>,
    }

    // --- Traits ---

    pub trait Receive<Hash> {
        fn lz_receive(
            origin: Origin,
            guid: Hash,
            message: Vec<u8>,
            executor: Address,
            extra_data: Vec<u8>,
        ) -> DispatchResult;
    }

    pub trait SendLib<Hash> {
        fn quote(
            packet: &Packet<Hash>,
            options: &[u8],
            pay_in_lz_token: bool,
        ) -> Result<MessagingFee, Error<()>>;

        fn send(
            packet: &Packet<Hash>,
            options: &[u8],
            pay_in_lz_token: bool,
        ) -> Result<(MessagingFee, Vec<u8>), Error<()>>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::storage]
    #[pallet::getter(fn lz_token)]
    pub type LzToken<T: Config> = StorageValue<_, Address, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn delegates)]
    pub type Delegates<T: Config> = StorageMap<_, Blake2_128Concat, Address, Address, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn outbound_nonce)]
    pub type OutboundNonce<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, Address, Blake2_128Concat, u32, u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn inbound_payload_hash)]
    pub type InboundPayloadHash<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, Address>,
            NMapKey<Blake2_128Concat, u32>,
            NMapKey<Blake2_128Concat, Address>,
            NMapKey<Blake2_128Concat, u64>,
        ),
        T::Hash,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn lazy_inbound_nonce)]
    pub type LazyInboundNonce<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, Address>,
            NMapKey<Blake2_128Concat, u32>,
            NMapKey<Blake2_128Concat, Address>,
        ),
        u64,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn send_libraries)]
    pub type SendLibraries<T: Config> =
        StorageMap<_, Blake2_128Concat, Address, Address, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn receive_libraries)]
    pub type ReceiveLibraries<T: Config> =
        StorageMap<_, Blake2_128Concat, Address, Address, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn endpoint_eid)]
    pub type EndpointEid<T: Config> = StorageValue<_, u32, OptionQuery>;

    // --- Events ---

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        LzTokenSet(Address),
        DelegateSet(Address, Address),
        PacketSent(Vec<u8>, Vec<u8>, Address),
        PacketVerified(Origin, Address, T::Hash),
        PacketDelivered(Origin, Address),
        LzReceiveAlert(
            Address,
            Address,
            Origin,
            T::Hash,
            u64,
            u64,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
        ),
    }

    // --- Errors ---

    #[pallet::error]
    pub enum Error<T> {
        LzTokenUnavailable,
        Unauthorized,
        NonceOverflow,
        InvalidSendLibrary,
        InvalidReceiveLibrary,
        PathNotInitializable,
        PathNotVerifiable,
        InsufficientFee,
        ZeroLzTokenFee,
        EidNotSet,
        GuidGenerationFailed,
        InvalidPayloadHash,
        PayloadNotFound,
        InvalidAddress,
    }

    #[pallet::genesis_config]
    pub struct GenesisConfig {
        pub endpoint_eid: u32,
        pub lz_token: Option<Address>,
    }

    impl Default for GenesisConfig {
        fn default() -> Self {
            Self {
                endpoint_eid: 0,
                lz_token: None,
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig {
        fn build(&self) {
            EndpointEid::<T>::put(self.endpoint_eid);
            if let Some(token_acc) = &self.lz_token {
                LzToken::<T>::put(token_acc.clone());
            }
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Send a message to a destination endpoint.
        ///
        /// This function sends a message to a destination endpoint using the provided parameters.
        /// It checks if the sender has sufficient balance and if the destination endpoint is valid.
        /// If the message is successfully sent, it returns a DispatchResult.
        #[pallet::call_index(0)]
        #[pallet::weight((100_000, Pays::No))] // Unsigned
        pub fn send(
            origin: OriginFor<T>,
            sender: Address,
            params: MessagingParams,
            _refund_address: Address,
        ) -> DispatchResult {
            ensure_none(origin)?;
            let eid = EndpointEid::<T>::get().ok_or(Error::<T>::EidNotSet)?;

            if params.pay_in_lz_token && LzToken::<T>::get().is_none() {
                return Err(Error::<T>::LzTokenUnavailable.into());
            }

            let nonce = OutboundNonce::<T>::get(&sender, params.dst_eid)
                .checked_add(1)
                .ok_or(Error::<T>::NonceOverflow)?;
            OutboundNonce::<T>::insert(&sender, params.dst_eid, nonce);

            let guid = Self::generate_guid(nonce, eid, &sender, &params);

            let packet = Packet {
                nonce,
                src_eid: eid,
                sender: sender.clone(),
                dst_eid: params.dst_eid,
                receiver: params.receiver.clone(),
                guid,
                message: params.message.clone(),
            };

            let send_library_account = Self::get_send_library(&sender, params.dst_eid);

            let (_fee, encoded_packet) =
                T::SendLibrary::send(&packet, &params.options, params.pay_in_lz_token)
                    .map_err(|_| Error::<T>::InvalidSendLibrary)?;

            Self::deposit_event(Event::PacketSent(
                encoded_packet,
                params.options,
                send_library_account,
            ));

            Ok(())
        }

        /// Verify a message received from a source endpoint.
        ///
        /// This function verifies a message received from a source endpoint using the provided parameters.
        /// It checks if the sender has sufficient balance and if the destination endpoint is valid.
        /// If the message is successfully verified, it emits an event that the message has been verified.
        #[pallet::call_index(1)]
        #[pallet::weight((100_000, Pays::No))] // Unsigned
        pub fn verify(
            origin: OriginFor<T>,
            oapp: Address,
            origin_info: Origin,
            payload_hash: T::Hash,
        ) -> DispatchResult {
            ensure_none(origin)?;

            let lazy_nonce_key = (&oapp, origin_info.src_eid, &origin_info.sender);
            let _lazy_nonce = LazyInboundNonce::<T>::get(lazy_nonce_key);

            let key = (
                oapp.clone(),
                origin_info.src_eid,
                origin_info.sender.clone(),
                origin_info.nonce,
            );
            InboundPayloadHash::<T>::insert(key, payload_hash);

            Self::deposit_event(Event::PacketVerified(origin_info, oapp, payload_hash));
            Ok(())
        }

        /// This function is called on the receiving chain to process a packet.
        #[pallet::call_index(2)]
        #[pallet::weight((100_000, Pays::No))] // Unsigned
        pub fn lz_receive(
            origin: OriginFor<T>,
            executor: Address,
            oapp: Address,
            origin_info: Origin,
            guid: T::Hash,
            message: Vec<u8>,
            extra_data: Vec<u8>,
        ) -> DispatchResult {
            ensure_none(origin)?;

            let payload = [guid.as_ref(), &message].concat();
            let payload_hash = T::Hashing::hash(&payload);

            Self::clear_payload(&oapp, &origin_info, payload_hash)?;

            T::MessageReceiver::lz_receive(
                origin_info.clone(),
                guid,
                message,
                executor,
                extra_data,
            )?;

            Self::deposit_event(Event::PacketDelivered(origin_info, oapp));
            Ok(())
        }

        #[pallet::call_index(3)]
        #[pallet::weight((10_000, Pays::No))] // Unsigned
        pub fn set_delegate(
            origin: OriginFor<T>,
            oapp: Address,
            delegate: Address,
        ) -> DispatchResult {
            ensure_none(origin)?;
            Delegates::<T>::insert(&oapp, &delegate);
            Self::deposit_event(Event::DelegateSet(oapp, delegate));
            Ok(())
        }

        #[pallet::call_index(4)]
        #[pallet::weight((10_000, Pays::No))] // Unsigned
        pub fn set_lz_token(origin: OriginFor<T>, token_account: Address) -> DispatchResult {
            ensure_none(origin)?;
            LzToken::<T>::put(&token_account);
            Self::deposit_event(Event::LzTokenSet(token_account));
            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            // TODO: Implement proper validation logic here.
            // For now, we allow all unsigned transactions.
            // This is insecure and should be replaced with actual signature verification
            // or other authentication mechanisms appropriate for the relayer/executor model.
            ValidTransaction::with_tag_prefix("LayerZero").build()
        }
    }

    impl<T: Config> Pallet<T> {
        fn generate_guid(
            nonce: u64,
            eid: u32,
            sender: &Address,
            params: &MessagingParams,
        ) -> T::Hash {
            let encoded = (nonce, eid, sender, params.dst_eid, &params.receiver).encode();
            T::Hashing::hash(&encoded)
        }

        fn get_send_library(oapp: &Address, _dst_eid: u32) -> Address {
            SendLibraries::<T>::get(oapp).unwrap_or_else(|| oapp.clone())
        }

        fn clear_payload(
            oapp: &Address,
            origin_info: &Origin,
            expected_hash: T::Hash,
        ) -> DispatchResult {
            let key = (
                oapp.clone(),
                origin_info.src_eid,
                origin_info.sender.clone(),
                origin_info.nonce,
            );
            let stored_hash =
                InboundPayloadHash::<T>::get(key.clone()).ok_or(Error::<T>::PayloadNotFound)?;
            ensure!(stored_hash == expected_hash, Error::<T>::InvalidPayloadHash);
            InboundPayloadHash::<T>::remove(key);
            Ok(())
        }

        fn is_authorized(caller: &Address, oapp: &Address) -> bool {
            caller == oapp || Some(caller) == Delegates::<T>::get(oapp).as_ref()
        }
    }
}
