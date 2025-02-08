use std::{marker::PhantomData, sync::Arc};
use std::future::Future;
use num_traits::Zero;
use tokio::sync::Mutex;
use sc_client_api::{BlockBackend, HeaderBackend};
use sc_consensus::{BlockImport, BlockImportParams, StateAction, ForkChoiceStrategy, StorageChanges};
use sp_api::__private::HeaderT;
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_consensus::{BlockOrigin, Environment, Error as ConsensusError, Proposer};
use sp_consensus_qpow::QPoWApi;
use sp_runtime::traits::{Block as BlockT, BlockIdTo, NumberFor};
use sp_runtime::codec::Encode;
use sc_consensus::BoxBlockImport;
use sc_transaction_pool_api::TransactionPool;
use sp_inherents::InherentDataProvider;

pub struct QPoWWorker<B: BlockT, C, P, E>
where
    C: ProvideRuntimeApi<B> + BlockBackend<B> + HeaderBackend<B> + BlockIdTo<B>,
    E: Environment<B> + Send + Sync + Clone + 'static,
    E::Error: std::fmt::Debug,
    E::Proposer: Proposer<B>,
{
    client: Arc<C>,
    block_import: Arc<Mutex<dyn BlockImport<B, Error = ConsensusError> + Send + Sync>>,
    transaction_pool: Arc<P>,
    env: E,
    last_nonce: Option<u64>,
    last_solution: Option<[u8; 64]>,
    target_difficulty: Option<u32>,
    is_running: bool,
    _phantom: PhantomData<B>,
}

impl<B, C, P, E> QPoWWorker<B, C, P, E>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + BlockBackend<B> + HeaderBackend<B> + BlockIdTo<B> + Send + Sync + 'static,
    C::Api: BlockBuilderApi<B> + QPoWApi<B>,
    P: TransactionPool<Block = B> + 'static,
    P::InPoolTransaction: Send + Sync,
    E: Environment<B> + Send + Sync + Clone + 'static,
    E::Error: std::fmt::Debug,
    E::Proposer: Proposer<B>,
{
    pub fn new(
        client: Arc<C>,
        block_import: BoxBlockImport<B>,
        transaction_pool: Arc<P>,
        env: E,
    ) -> Self {
        Self {
            client,
            block_import: Arc::new(Mutex::new(block_import)),
            transaction_pool,
            env,
            last_nonce: None,
            last_solution: None,
            target_difficulty: None,
            is_running: false,
            _phantom: PhantomData,
        }
    }

    async fn try_mine_block(&mut self) -> Result<(), ConsensusError> {
        let best_hash = self.client.info().best_hash;
        let parent_header = self.client
            .header(best_hash)
            .map_err(|e| ConsensusError::ChainLookup(format!("QPOW: Failed to get header: {}", e)))?
            .ok_or_else(|| ConsensusError::ChainLookup("QPOW: Parent block not found".into()))?;

        log::info!("QPOW: TryMainBlock - start: hash:{}", best_hash);

        // Initialize proposer
        let proposer = match self.env.init(&parent_header).await {
            Ok(x) => x,
            Err(err) => {
                log::warn!(
                target: "qpow",
                "Unable to propose new block for authoring. Creating proposer failed: {:?}",
                err,
            );
                return Err(ConsensusError::ClientImport("Failed to create proposer".into()));
            },
        };

        // Create inherent data
        let mut inherent_data = sp_inherents::InherentData::new();
        let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
        timestamp.provide_inherent_data(&mut inherent_data)
            .await
            .map_err(|e| ConsensusError::ClientImport(format!("Failed to create inherent data: {:?}", e)))?;

        // Create proposal
        let proposal = match proposer.propose(
            inherent_data,
            sp_runtime::generic::Digest::default(),
            std::time::Duration::from_secs(2),
            None,
        ).await {
            Ok(p) => p,
            Err(err) => {
                log::warn!(
                target: "qpow",
                "Unable to propose new block for authoring. Creating proposal failed: {:?}",
                err,
            );
                return Err(ConsensusError::ClientImport("Failed to create proposal".into()));
            }
        };

        let mut header = proposal.block.header().clone();
        let difficulty = self.client.runtime_api()
            .get_difficulty(best_hash)
            .unwrap_or(16);

        log::info!("QPOW: Mining block - difficulty: {}", difficulty);

        let mut nonce = self.last_nonce.unwrap_or(0u64);
        let mut solution = self.last_solution.unwrap_or([0u8; 64]);

        nonce += 1;
        solution[0..8].copy_from_slice(&nonce.to_le_bytes());

        loop {
            let valid_seal = seal_block::<B, C>(
                self.client.clone(),
                header.encode().try_into().unwrap_or([0u8; 32]),
                solution,
                difficulty
            )?;

            if valid_seal {
                log::info!("QPOW: Mined block: nonce={}", nonce);

                header.digest_mut().push(sp_runtime::generic::DigestItem::Seal(
                    sp_consensus_qpow::QPOW_ENGINE_ID,
                    solution.to_vec(),
                ));

                let (_block_parts, body) = proposal.block.deconstruct();

                let mut import_block = BlockImportParams::new(BlockOrigin::Own, header);
                import_block.body = Some(body);
                //TODO - this is not real finalization, works only from node perspective - we should investigate it
                /*
                This flag only affects the local state.
                It means that the local node considers this block as finalized,
                but from the runtime and other nodes’ perspectives,
                it has no effect—we can still have forks.
                We should analyze GRANDPA finalization.
                You can set it to true and your console will look beautiful.
                I will leave it off, to remember about this problem.
                 */
                import_block.finalized = false;
                import_block.state_action = StateAction::ApplyChanges(StorageChanges::Changes(proposal.storage_changes));
                import_block.fork_choice = Some(ForkChoiceStrategy::LongestChain);

                //log::info!("QPOW: Importing block...");
                let _result = self.block_import.lock().await.import_block(import_block).await;
                //log::info!("QPOW: Import result: {:?}", result);

                self.last_nonce = Some(nonce);
                self.last_solution = Some(solution);

                return Ok(());
            }

            if nonce % 1000 == 0 {
                log::info!("QPOW: Mining in progress... nonce={}", nonce);
            }
            nonce += 1;
            solution[0..8].copy_from_slice(&nonce.to_le_bytes());
        }
    }

    pub fn start(&self) -> impl Future<Output = ()> + Send {
        let client = self.client.clone();
        let block_import = self.block_import.clone();
        let transaction_pool = self.transaction_pool.clone();
        let env = self.env.clone();
        let last_nonce = self.last_nonce;
        let last_solution = self.last_solution;
        let target_difficulty = self.target_difficulty;
        let is_running = self.is_running;

        async move {
            let mut worker = QPoWWorker {
                client,
                block_import,
                transaction_pool,
                env,
                last_nonce,
                last_solution,
                target_difficulty,
                is_running,
                _phantom: PhantomData,
            };

            loop {
                if let Err(e) = worker.try_mine_block().await {
                    log::error!("QPOW: Error while mining block: {:?}", e);
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}
pub fn seal_block<B, C>(
    client: Arc<C>,
    header: [u8; 32],
    solution: [u8; 64],
    difficulty: u64,
) -> Result<bool, ConsensusError>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{
    let block_hash = client.block_hash(NumberFor::<B>::zero())
        .map_err(|e| ConsensusError::ClientImport(format!("QPOW: Failed to get block hash: {:?}", e)))?
        .ok_or_else(|| ConsensusError::ClientImport("QPOW: Block hash not found".into()))?;

    let valid = client
        .runtime_api()
        .verify_solution(block_hash, header, solution, difficulty)
        .map_err(|e| ConsensusError::ClientImport(format!("QPOW: Failed to verify solution: {:?}", e)))?;

    Ok(valid)
}

