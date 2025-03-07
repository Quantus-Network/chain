use jsonrpsee::core::client::{ClientT, Error as JsonRpcError};
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonrpsee::rpc_params;
use sp_core::{H256, storage::StorageKey, Decode};
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Hash;
pub use resonance_runtime::AccountId;
pub use pallet_balances::AccountData;
use frame_system::AccountInfo; 
pub struct TxSdk {
    rpc: HttpClient,
}

impl TxSdk {
    pub fn new(url: &str) -> Self {
        let rpc = HttpClientBuilder::default()
            .build(url)
            .expect("Valid RPC URL required");
        Self { rpc }
    }

    pub async fn send_tx(&self, unsigned_extrinsic: Vec<u8>) -> Result<H256, JsonRpcError> {
        let encoded = hex::encode(unsigned_extrinsic);
        self.rpc
            .request("author_submitExtrinsic", rpc_params![encoded])
            .await
    }

    /// Queries the free balance of an account.
    pub async fn get_balance(&self, account: &AccountId) -> Result<u128, JsonRpcError> {
        // Storage key for Balances.Account (System.Account in FRAME)
        let key = Self::balance_storage_key(account);
        
        // Query storage
        let storage_result: Option<String> = self.rpc
            .request("state_getStorage", rpc_params![key.as_ref()])
            .await?;
        
        match storage_result {
            Some(hex_data) => {
                // Remove "0x" prefix and decode hex to bytes
                let bytes = hex::decode(&hex_data[2..]).map_err(|e| JsonRpcError::Custom(e.to_string()))?;
                // Decode AccountData (assuming { free: u128, reserved: u128, ... })
                let account_data = AccountInfo::<u32, pallet_balances::AccountData<u128>>::decode(&mut &bytes[..])
                    .map_err(|e| JsonRpcError::Custom(e.to_string()))?;
                Ok(account_data.data.free)
            }
            None => Ok(0), // Account doesn't exist or has no balance
        }
    }

    /// Constructs the storage key for an account's balance.
    fn balance_storage_key(account: &AccountId) -> StorageKey {
        use sp_runtime::traits::BlakeTwo256;
        use sp_core::twox_128;

        // FRAME System.Account key: Twox128("System") + Twox128("Account") + Blake2_256(account)
        let mut key = Vec::new();
        key.extend(twox_128(b"System"));
        key.extend(twox_128(b"Account"));
        key.extend(BlakeTwo256::hash(account.as_ref()).as_ref());
        StorageKey(key)
    }
}