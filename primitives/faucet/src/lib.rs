#![cfg_attr(not(feature = "std"), no_std)]
use codec::Codec;
use sp_api::decl_runtime_apis;

decl_runtime_apis! {
    pub trait FaucetApi<AccountId, Balance, Nonce>
    where
        AccountId: Codec,
        Balance: Codec,
        Nonce: Codec,
    {
        fn account_balance(account: AccountId) -> (Balance, Balance);
    }
}
