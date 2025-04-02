use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::pallet_prelude::TypeInfo;
use frame_support::traits::{Consideration, Footprint, ReservableCurrency};
use sp_runtime::DispatchError;
use crate::{AccountId, Balance, Balances, MICRO_UNIT, UNIT};


///Preimage pallet fee model

#[derive(Encode, Decode, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
pub struct PreimageDeposit {
    amount: Balance,
}

impl Consideration<AccountId, Footprint> for PreimageDeposit {
    fn new(who: &AccountId, footprint: Footprint) -> Result<Self, DispatchError> {
        // Simple fee model: 0.1 UNIT + 0.0001 UNIT for one byte
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
        let size = (footprint.size as u128).saturating_add(footprint.count as u128);
        let amount = base.saturating_add(per_byte.saturating_mul(size));

        Balances::reserve(who, amount)?;
        Ok(Self { amount })
    }

    fn update(self, who: &AccountId, new_footprint: Footprint) -> Result<Self, DispatchError> {
        // Calculate new amount
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
        let size = (new_footprint.size as u128).saturating_add(new_footprint.count as u128);
        let new_amount = base.saturating_add(per_byte.saturating_mul(size));

        // Release old deposite
        Balances::unreserve(who, self.amount);

        // Take new deposite
        Balances::reserve(who, new_amount)?;

        Ok(Self { amount: new_amount })
    }

    fn drop(self, who: &AccountId) -> Result<(), DispatchError> {
        Balances::unreserve(who, self.amount);
        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn ensure_successful(who: &AccountId, footprint: Footprint) {
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
        let size = (footprint.size as u128).saturating_add(footprint.count as u128);
        let amount = base.saturating_add(per_byte.saturating_mul(size));

        // Check if user has enough coins
        if Balances::free_balance(who) < amount {
            Balances::make_free_balance_be(who, amount.saturating_mul(2));
        }
    }
}