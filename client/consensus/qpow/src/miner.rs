use std::marker::PhantomData;
use std::sync::Arc;
use primitive_types::{H256, U256};
use sc_client_api::BlockBackend;
use sp_api::ProvideRuntimeApi;
use sp_runtime::traits::Block as BlockT;
use sp_consensus_qpow::QPoWApi;
use crate::{Compute, QPoWSeal};

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
        nonce: u64,
        difficulty: U256,
    ) -> Result<QPoWSeal, ()> {

        let compute = Compute {
            difficulty,
            pre_hash: H256::from_slice(pre_hash.as_ref()),
            nonce,
            _phantom: Default::default(),
        };

        // Compute the seal
        log::info!("compute difficulty: {:?}", difficulty);
        let seal = match compute.compute(parent_hash.clone(), &self.client) {
            Ok(seal) => seal,
            Err(e) => {
                log::info!("compute error: {:?}", e);
                return Err(());
            }
        };


        log::info!("compute done");

        // Convert pre_hash to [u8; 32] for verification
        // TODO normalize all the different ways we do calculations
        let header = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

        // Verify the solution using QPoW

        match self.client.runtime_api().verify_solution(parent_hash, header, seal.work, difficulty.low_u64()) {
            Ok(true) => {
                log::info!("good seal");
                Ok(seal)
            }
            Ok(false) => {
                log::info!("invalid seal");
                Err(())
            }
            Err(e) => {
                log::info!("API error in verify_solution: {:?}", e);
                Err(())
            }
        }

    }
}