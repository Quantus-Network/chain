use super::*;
use qp_wormhole::TransferProofs;

impl<T: Config<I>, I: 'static> TransferProofs<T::Balance, T::AccountId, TransferCountType>
    for Pallet<T, I>
{
    fn transfer_proof_exists(
        count: TransferCountType,
        from: &T::AccountId,
        to: &T::AccountId,
        value: T::Balance,
    ) -> bool {
        TransferProof::<T, I>::get((count, from, to, value)).is_some()
    }

    fn store_transfer_proof(from: &T::AccountId, to: &T::AccountId, value: T::Balance) {
        Pallet::<T, I>::do_store_transfer_proof(from, to, value);
    }

    fn transfer_proof_key(
        transfer_count: u64,
        from: T::AccountId,
        to: T::AccountId,
        value: T::Balance,
    ) -> Vec<u8> {
        Pallet::<T, I>::transfer_proof_storage_key(transfer_count, from, to, value)
    }
}
