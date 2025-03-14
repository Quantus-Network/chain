use std::marker::PhantomData;
use std::sync::Arc;
use primitive_types::{H256, U256};
use sc_client_api::BlockBackend;
use sp_api::ProvideRuntimeApi;
use sp_runtime::traits::Block as BlockT;
use sp_consensus_qpow::QPoWApi;
use crate::QPoWSeal;

pub struct QPoWMiner<B,C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B>
{
    pub client: Arc<C>,
    pub _phantom: PhantomData<B>,
}


impl<B, C> QPoWMiner<B, C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{

    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            _phantom: PhantomData,
        }
    }

    pub fn try_nonce<BA: BlockT<Hash = H256>>(
        &self,
        parent_hash: BA::Hash,
        pre_hash: BA::Hash,
        nonce: [u8; 64],
        difficulty: U256,
    ) -> Result<QPoWSeal, ()> {

        // Convert pre_hash to [u8; 32] for verification
        // TODO normalize all the different ways we do calculations
        let block_hash = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

        // Verify the nonce using runtime api
        match self.client.runtime_api().submit_nonce(parent_hash, block_hash, nonce) {
            Ok(true) => {
                log::info!("good seal");
                Ok(QPoWSeal { nonce })
            }
            Ok(false) => {
                Err(())
            }
            Err(e) => {
                log::info!("API error in verify_nonce: {:?}", e);
                Err(())
            }
        }

    }
}