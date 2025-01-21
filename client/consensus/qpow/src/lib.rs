use std::{marker::PhantomData, sync::Arc};
use std::future::Future;
use num_traits::Zero;
use tokio::sync::Mutex;
use sc_client_api::{BlockBackend, HeaderBackend};
use sc_consensus::{BlockImport, import_queue::{BasicQueue, BoxBlockImport, Verifier}, BlockImportParams, StateAction, ForkChoiceStrategy};
use sp_api::__private::HeaderT;
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_consensus::{BlockOrigin, Error as ConsensusError};
use sp_consensus_qpow::QPoWApi;
use sp_runtime::{
    traits::{Block as BlockT},
};
use sp_runtime::codec::Encode;
use sp_runtime::traits::{NumberFor, One};

/// QPoW block verifier
pub struct QPoWVerifier<C, B> {
    client: Arc<C>,
    _phantom: PhantomData<B>,
}

impl<C, B> QPoWVerifier<C, B>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{
    /// Create new QPoW verifier.
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            _phantom: PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<B, C> Verifier<B> for QPoWVerifier<C, B>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{
    async fn verify(
        &self,
        block: BlockImportParams<B>,
    ) -> Result<BlockImportParams<B>, String> {
        Ok(block)
    }
}

/// QPoW worker for block production.
pub struct QPoWWorker<B: BlockT, C> {
    client: Arc<C>,
    block_import: Arc<Mutex<dyn BlockImport<B, Error = ConsensusError> + Send + Sync>>,
    last_nonce: Option<u64>,                // Przechowuje ostatni użyty nonce
    last_solution: Option<[u8; 64]>,        // Przechowuje ostatnie rozwiązanie
    target_difficulty: Option<u32>,         // Docelowy poziom trudności (opcjonalne)
    is_running: bool,                       // Flaga wskazująca, czy worker działa
    _phantom: PhantomData<B>,
}

impl<B, C> QPoWWorker<B, C>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + BlockBackend<B> + HeaderBackend<B> + Send + Sync + 'static,
    C::Api: BlockBuilderApi<B> + QPoWApi<B>,
{
    /// Create new QPoW worker.
    pub fn new(
        client: Arc<C>,
        block_import: BoxBlockImport<B>,
    ) -> Self {
        Self {
            client,
            block_import: Arc::new(Mutex::new(block_import)),
            last_nonce: None,                // Brak początkowego nonce
            last_solution: None,             // Brak początkowego rozwiązania
            target_difficulty: None,         // Opcjonalna trudność
            is_running: false,               // Worker początkowo nie działa
            _phantom: PhantomData,
        }
    }

    /// Try to mine a block
    async fn try_mine_block(&mut self) -> Result<(), ConsensusError> {

        let best_hash = self.client.info().best_hash;
        let parent_hash = self.client.info().best_hash;
        let parent_header = self.client
            .header(parent_hash)
            .map_err(|e| ConsensusError::ChainLookup(format!("Failed to get header: {}", e)))?
            .ok_or_else(|| ConsensusError::ChainLookup("Parent block not found".into()))?;

        let best_number = self.client.info().best_number;

        log::info!("TryMainBlock - start: h:{}, n:{}",best_hash,best_number);

        let difficulty = self.client.runtime_api()
            .get_difficulty(best_hash).unwrap_or(16);

        log::info!("TryMainBlock - difficulty: {}",difficulty);
        let next_number = best_number + <<B as BlockT>::Header as HeaderT>::Number::one();

        let mut header = B::Header::new(
            next_number,
            Default::default(),
            Default::default(),
            best_hash,
            Default::default(),
        );

        let mut nonce = self.last_nonce.unwrap_or(0u64);
        let mut solution = self.last_solution.unwrap_or([0u8; 64]);

        nonce+=1;
        solution[0..8].copy_from_slice(&nonce.to_le_bytes());

        //log::info!("N {}, S {:?}",nonce,solution);

        loop{
            let seal = seal_block::<B, C>(
                self.client.clone(),
                header.encode().try_into().unwrap_or([0u8; 32]),
                difficulty,
                solution,
            )?;

            if is_valid_seal(&seal, difficulty) {
                log::info!("Mined block: nonce={}, seal={:?}", nonce, seal);

                header.set_state_root(*parent_header.state_root());

                header.set_extrinsics_root(*parent_header.extrinsics_root());

                header.digest_mut().push(sp_runtime::generic::DigestItem::PreRuntime(
                    sp_consensus_qpow::QPOW_ENGINE_ID,
                    seal.clone(),
                ));
                header.digest_mut().push(sp_runtime::generic::DigestItem::Seal(
                    sp_consensus_qpow::QPOW_ENGINE_ID,
                    seal,
                ));

                let mut block = BlockImportParams::new(
                    BlockOrigin::Own,
                    header,
                );

                block.body = None;
                block.indexed_body = None;
                block.state_action = StateAction::ExecuteIfPossible;
                block.finalized = false;
                block.intermediates = Default::default();
                block.post_digests = vec![];
                block.fork_choice = Some(ForkChoiceStrategy::LongestChain);
                block.import_existing = false;
                block.justifications = None;
                block.auxiliary = vec![];
                block.post_hash = None;

                //self.block_import.lock().await.import_block(block).await?;

                //log::info!("Importing block with header: {:?}", block.header);
                //log::info!("Block body: {:?}", block.body);
                let _result = self.block_import.lock().await.import_block(block).await;
                //log::info!("Import result: {:?}", result);

                self.last_nonce = Some(nonce);
                self.last_solution = Some(solution);

                return Ok(());

            }
            //nonce += 1;
            //solution[0..8].copy_from_slice(&nonce.to_le_bytes());
            //log::info!("SOLUTION FOR THE NEXT NONCE: {:?}",solution);


        }
    }

    pub fn start(&self) -> impl Future<Output = ()> + Send {
        let client = self.client.clone();
        let block_import = self.block_import.clone();
        let last_nonce = self.last_nonce;
        let last_solution = self.last_solution;
        let target_difficulty = self.target_difficulty;
        let is_running = self.is_running;

        async move {
            let mut worker = QPoWWorker {
                client,
                block_import,
                last_nonce,
                last_solution,
                target_difficulty,
                is_running,
                _phantom: PhantomData,
            };

            loop {
                if let Err(e) = worker.try_mine_block().await {
                    log::error!("Error while mining block: {:?}", e);
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

pub fn seal_block<B, C>(
    client: Arc<C>,
    header: [u8; 32],
    difficulty: u32,
    solution: [u8; 64],
) -> Result<Vec<u8>, ConsensusError>
where
    B: BlockT,
    C: sp_api::ProvideRuntimeApi<B> + sc_client_api::BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{
    let block_hash = client.block_hash(NumberFor::<B>::zero())
        .map_err(|e| ConsensusError::ClientImport(format!("Failed to get block hash: {:?}", e)))?
        .ok_or_else(|| ConsensusError::ClientImport("Block hash not found".into()))?;

    let (_result, truncated) = client
        .runtime_api()
        .compute_pow(block_hash, header, difficulty, solution)
        .map_err(|e| ConsensusError::ClientImport(format!("Runtime API error: {:?}", e)))?;

    Ok(truncated)
}

pub fn is_valid_seal(_seal: &[u8], _difficulty: u32) -> bool {
    //let hash_value = u256_from_seal(seal);

    //let target = (U256::one() << 256) / U256::from(difficulty);

    //hash_value < target
    true
}

/// Create QPoW import queue.
pub fn import_queue<B, C>(
    client: Arc<C>,
    block_import: BoxBlockImport<B>,
    spawner: &impl sp_core::traits::SpawnEssentialNamed,
) -> BasicQueue<B>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{
    BasicQueue::new(
        QPoWVerifier::new(client.clone()),
        block_import,
        None,
        spawner,
        None,
    )
}