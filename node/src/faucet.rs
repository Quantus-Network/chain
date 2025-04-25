use codec::{Decode, Encode};
use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use sc_transaction_pool_api::TransactionPool;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_core::crypto::Ss58Codec;
use sp_runtime::MultiAddress;
use sp_runtime::traits::Block as BlockT;
use sp_runtime::transaction_validity::TransactionSource;
use substrate_frame_rpc_system::AccountNonceApi;
use resonance_runtime::{AccountId, Balance, Nonce, RuntimeCall, UncheckedExtrinsic};
use resonance_runtime::opaque::Block;
use sp_faucet::FaucetApi;

#[rpc(client,server)]
pub trait FaucetApi<BlockHash> {

    #[method(name = "faucet_getAccountInfo")]
    fn get_account_info(&self, address: String, at: Option<BlockHash>) -> RpcResult<AccountInfo>;

    #[method(name = "faucet_requestTokens")]
    fn request_tokens(&self, address: String, at: Option<BlockHash>) -> RpcResult<bool>;
}

#[derive(Encode, Decode, Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub free_balance: u128,
    pub reserved_balance: u128,
}

/// Faucet RPC implementation
pub struct Faucet<C, P> {
    client: Arc<C>,
    pool: Arc<P>,
}

impl<C, P> Faucet<C, P> {
    /// Create new Faucet RPC handler
    pub fn new(client: Arc<C>, pool: Arc<P>) -> Self {
        Self {
            client,
            pool,
        }
    }

    fn parse_address(address: String) -> Result<AccountId, jsonrpsee::types::error::ErrorObject<'static>> {
        if address.starts_with("0x") {
            // Format hex
            let hex_str = &address[2..];
            match hex::decode(hex_str) {
                Ok(bytes) => {
                    if bytes.len() != 32 {
                        return Err(jsonrpsee::types::error::ErrorObject::owned(
                            4001,
                            "Invalid hex address length, expected 32 bytes",
                            None::<()>
                        ));
                    }
                    let mut array = [0u8; 32];
                    array.copy_from_slice(&bytes);
                    Ok(AccountId::from(array))
                },
                Err(_) => {
                    Err(jsonrpsee::types::error::ErrorObject::owned(
                        4001,
                        "Invalid hex address format",
                        None::<()>
                    ))
                }
            }
        } else {
            // Format SS58
            match resonance_runtime::AccountId::from_string(&address) {
                Ok(account) => Ok(account),
                Err(_) => {
                    Err(jsonrpsee::types::error::ErrorObject::owned(
                        4001,
                        "Invalid address format, expected 0x-prefixed hex or valid SS58",
                        None::<()>
                    ))
                }
            }
        }
    }
}

impl<C, P> FaucetApiServer<<Block as BlockT>::Hash> for Faucet<C, P>
where
    C: ProvideRuntimeApi<Block>
    + HeaderBackend<Block>
    + Send
    + Sync
    + 'static,
    C::Api: AccountNonceApi<Block, AccountId, Nonce>,
    C::Api: FaucetApi<Block, AccountId, Balance, Nonce>,
    P: TransactionPool<Block = Block> + 'static,
{
    fn get_account_info(&self, address: String, at: Option<<Block as BlockT>::Hash>) -> RpcResult<AccountInfo> {
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        let account_id = Self::parse_address(address)?;

        let (free_balance, reserved_balance) = match self.client.runtime_api().account_balance(at, account_id.clone()) {
            Ok((free, reserved)) => {
                log::info!("Successfully retrieved balances - free: {}, reserved: {}", free, reserved);
                (free, reserved)
            },
            Err(err) => {
                log::error!("Failed to get account balances: {:?}", err);
                (0, 0)
            }
        };

        Ok(AccountInfo {
            free_balance,
            reserved_balance,
        })
    }

    fn request_tokens(&self, address: String, at: Option<<Block as BlockT>::Hash>) -> RpcResult<bool> {
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        let account_id = Self::parse_address(address)?;

        let seed = rand::random::<u64>();

        let call = RuntimeCall::Faucet(pallet_faucet::Call::mint_new_tokens {
            dest: MultiAddress::Id(account_id.clone()),
            seed
        });

        let extrinsic = UncheckedExtrinsic::new_bare(call);

        match futures::executor::block_on(self.pool.submit_one(
            at,
            TransactionSource::Local,
            extrinsic.into(),
        )) {
            Ok(tx_hash) => {
                log::info!("Successfully submitted faucet transaction: {:?}", tx_hash);
                Ok(true)
            },
            Err(e) => {
                log::error!("Failed to submit faucet transaction: {:?}", e);
                Ok(false)
            }
        }
    }
}