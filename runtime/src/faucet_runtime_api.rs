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
        //fn request_tokens(to: AccountId, amount: Balance) -> Result<(), DispatchError>;
        //fn transfer_proof_exists(nonce: Nonce, from: AccountId, to: AccountId, amount: Balance) -> bool;
    }
}