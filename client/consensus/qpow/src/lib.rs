use std::{marker::PhantomData, sync::Arc};
use std::future::Future;
use futures::channel::mpsc;
use num_traits::Zero;
use tokio::sync::Mutex;
use sc_client_api::{BlockBackend, BlockOf, HeaderBackend};
use sc_consensus::{BlockImport, import_queue::{BasicQueue, BoxBlockImport, Verifier}, BlockImportParams, StateAction, ForkChoiceStrategy, ImportQueue, DefaultImportQueue, BlockCheckParams, ImportResult};
use sp_api::__private::HeaderT;
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_consensus::{BlockOrigin, Error as ConsensusError, SelectChain};
use sp_consensus_qpow::QPoWApi;
use sp_runtime::{
    traits::{Block as BlockT},
};
use sp_runtime::codec::Encode;
use sp_runtime::traits::{NumberFor, One};

mod worker;
pub use worker::QPoWWorker;


pub struct QPoWBlockImport<B: BlockT, I, C, SC> {
    inner: I,
    client: Arc<C>,
    select_chain: SC,
    _phantom: PhantomData<B>,
}

impl<B: BlockT, I: Clone, C, SC: Clone> Clone for QPoWBlockImport<B, I, C, SC> {
    fn clone(&self) -> Self {
        log::info!("Cloning QPoW block import...");
        Self {
            inner: self.inner.clone(),
            client: self.client.clone(),
            select_chain: self.select_chain.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<B, I, C, SC> QPoWBlockImport<B, I, C, SC>
where
    B: BlockT,
    I: BlockImport<B> + Send + Sync,
    I::Error: Into<ConsensusError>,
    C: ProvideRuntimeApi<B> + Send + Sync + HeaderBackend<B> + BlockOf,
    C::Api: BlockBuilderApi<B> + QPoWApi<B>,
    SC: SelectChain<B>,
{
    pub fn new(
        inner: I,
        client: Arc<C>,
        select_chain: SC,
    ) -> Self {
        log::info!("Creating QPoW block import...");
        Self {
            inner,
            client,
            select_chain,
            _phantom: PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<B, I, C, SC> BlockImport<B> for QPoWBlockImport<B, I, C, SC>
where
    B: BlockT,
    I: BlockImport<B> + Send + Sync,
    I::Error: Into<ConsensusError>,
    C: ProvideRuntimeApi<B> + Send + Sync + HeaderBackend<B> + BlockOf,
    C::Api: BlockBuilderApi<B> + QPoWApi<B>,
    SC: SelectChain<B>,
{
    type Error = ConsensusError;

    async fn check_block(
        &self,
        block: BlockCheckParams<B>,
    ) -> Result<ImportResult, Self::Error> {
        log::info!("Checking block with QPow...");
        self.inner.check_block(block).await.map_err(Into::into)
    }

    async fn import_block(
        &self,
        mut block: BlockImportParams<B>,
    ) -> Result<ImportResult, Self::Error> {
        log::info!(
            target: "qpow",
            "Importing block #{:?}, hash: {:?}",
            block.header.number(),
            block.header.hash()
        );

        // Pobierz najlepszy blok za pomocą select_chain
        let best_header = self.select_chain
            .best_chain()
            .await
            .map_err(|e| ConsensusError::ChainLookup(format!("Failed to get best chain: {}", e)))?;

        // Ustaw strategię wyboru forka jeśli nie jest ustawiona
        if block.fork_choice.is_none() {
            let current_number = block.header.number();
            let best_number = best_header.number();

            log::info!(
                target: "qpow",
                "Current block: #{:?}, Best block: #{:?}",
                current_number,
                best_number
            );

            let is_best = current_number > best_number;
            block.fork_choice = Some(ForkChoiceStrategy::Custom(is_best));

            log::info!(
                target: "qpow",
                "Setting fork choice strategy: is_best = {}",
                is_best
            );
        }

        // Wykonaj import bloku
        let result = self.inner.import_block(block).await.map_err(Into::into);

        log::info!(
            target: "qpow",
            "Import result: {:?}",
            result
        );

        result
    }
}

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
        log::info!("Creating QPoW verifier...");
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

        log::info!("Verifying block: ---------------------------------------");
        Ok(block)
    }
}

/// Create QPoW import queue.
pub fn import_queue<B, C>(
    client: Arc<C>,
    block_import: BoxBlockImport<B>,
    select_chain: impl SelectChain<B> + 'static,
    spawner: &impl sp_core::traits::SpawnEssentialNamed,
) -> Result<DefaultImportQueue<B>,String>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockOf + Send + Sync + 'static,
    C::Api: QPoWApi<B> +BlockBuilderApi<B>,
{
    log::info!("Creating QPoW import queue ....");

    let qpow_block_import = QPoWBlockImport::new(
        block_import,
        client.clone(),
        select_chain
    );

    Ok(DefaultImportQueue::new(
        QPoWVerifier::new(client.clone()),
        Box::new(qpow_block_import),
        None,
        spawner,
        None,
    ))
/*    Ok(BasicQueue::new(
        QPoWVerifier::new(client.clone()),
        block_import,
        None,
        spawner,
        None,
    ))*/
}

pub type QPoWImportQueue<B> = sc_consensus::DefaultImportQueue<B>;