//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use futures::{FutureExt, StreamExt};
use sc_consensus_qpow::{QPoWMiner, QPoWSeal, QPowAlgorithm};
use sc_client_api::{Backend, BlockchainEvents};
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_telemetry::{Telemetry, TelemetryWorker};
use sc_transaction_pool_api::{InPoolTransaction, OffchainTransactionPoolFactory, TransactionPool};
use resonance_runtime::{self, apis::RuntimeApi, opaque::Block};

use std::{sync::Arc, time::Duration};
use codec::Encode;
use jsonrpsee::tokio;
use sp_api::__private::BlockT;
use sp_core::{RuntimeDebug, U512};
use async_trait::async_trait;
use sc_consensus::{BlockCheckParams, BlockImport, BlockImportParams, ImportResult};
use sp_runtime::traits::Header;
use sp_consensus_qpow::QPoWApi;
use crate::prometheus::ResonanceBusinessMetrics;
use sp_api::ProvideRuntimeApi;
use sp_core::crypto::AccountId32;
use sp_wormhole::WormholePair;

pub(crate) type FullClient = sc_service::TFullClient<
    Block,
    RuntimeApi,
    sc_executor::WasmExecutor<sp_io::SubstrateHostFunctions>,
>;
type FullBackend = sc_service::TFullBackend<Block>;
//type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;
type FullSelectChain = sc_consensus_qpow::HeaviestChain<Block, FullClient, FullBackend>;
pub type PowBlockImport = sc_consensus_pow::PowBlockImport<
    Block,
    Arc<FullClient>,
    FullClient,
    FullSelectChain,
    QPowAlgorithm<Block, FullClient>,
    Box<dyn sp_inherents::CreateInherentDataProviders<Block, (), InherentDataProviders=sp_timestamp::InherentDataProvider>>,
>;

#[derive(PartialEq, Eq, Clone, RuntimeDebug)]
pub struct LoggingBlockImport<B: BlockT, I> {
    inner: I,
    _phantom: std::marker::PhantomData<B>,
}

impl<B: BlockT, I> LoggingBlockImport<B, I> {
    fn new(inner: I) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<B: BlockT, I: BlockImport<B> + Sync> BlockImport<B>  for LoggingBlockImport<B, I>
{
    type Error = I::Error;

    async fn check_block(&self, block: BlockCheckParams<B>) -> Result<ImportResult, Self::Error> {
        self.inner.check_block(block).await.map_err(Into::into)
    }

    async fn import_block(&self, block: BlockImportParams<B>) -> Result<ImportResult, Self::Error> {
        log::info!(
            "🏆 Importing block #{}: {:?} - extrinsics_root={:?}, state_root={:?}",
            block.header.number(),
            block.header.hash(),
            block.header.extrinsics_root(),
            block.header.state_root()
        );
        self.inner.import_block(block).await.map_err(Into::into)
    }
}

pub const MAX_REORG_DEPTH: u32 = 10;

pub type Service = sc_service::PartialComponents<
    FullClient,
    FullBackend,
    FullSelectChain,
    sc_consensus::DefaultImportQueue<Block>,
    sc_transaction_pool::TransactionPoolHandle<Block, FullClient>,
    (LoggingBlockImport<Block, PowBlockImport>, Option<Telemetry>),
>;
//TODO Question - for what is this method?
pub fn build_inherent_data_providers(
) -> Result<Box<dyn sp_inherents::CreateInherentDataProviders<Block, (), InherentDataProviders=sp_timestamp::InherentDataProvider>>, ServiceError> {
    struct Provider;
    #[async_trait::async_trait]
    impl sp_inherents::CreateInherentDataProviders<Block, ()> for Provider {
        type InherentDataProviders = sp_timestamp::InherentDataProvider;

        async fn create_inherent_data_providers(
            &self,
            _parent: <Block as BlockT>::Hash,
            _extra: (),
        ) -> Result<Self::InherentDataProviders, Box<dyn std::error::Error + Send + Sync>> {
            let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
            Ok(timestamp)
        }
    }

    Ok(Box::new(Provider))
}

pub fn new_partial(config: &Configuration) -> Result<Service, ServiceError> {
    let telemetry = config
        .telemetry_endpoints
        .clone()
        .filter(|x| !x.is_empty())
        .map(|endpoints| -> Result<_, sc_telemetry::Error> {
            let worker = TelemetryWorker::new(16)?;
            let telemetry = worker.handle().new_telemetry(endpoints);
            Ok((worker, telemetry))
        })
        .transpose()?;

    let executor = sc_service::new_wasm_executor::<sp_io::SubstrateHostFunctions>(&config.executor);
    let (client, backend, keystore_container, task_manager) =
        sc_service::new_full_parts::<Block, RuntimeApi, _>(
            config,
            telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
            executor,
        )?;
    let client = Arc::new(client);

    let telemetry = telemetry.map(|(worker, telemetry)| {
        task_manager
            .spawn_handle()
            .spawn("telemetry", None, worker.run());
        telemetry
    });

    let pow_algorithm = QPowAlgorithm {
        client: client.clone(),
        _phantom: Default::default(),
    };

    //let select_chain = sc_consensus::LongestChain::new(backend.clone());
    // todo: create depth const
    let select_chain = sc_consensus_qpow::HeaviestChain::new(backend.clone(), Arc::clone(&client), pow_algorithm.clone(), MAX_REORG_DEPTH);

    let transaction_pool = Arc::from(
        sc_transaction_pool::Builder::new(
            task_manager.spawn_essential_handle(),
            client.clone(),
            config.role.is_authority().into(),
        )
            .with_options(config.transaction_pool.clone())
            .with_prometheus(config.prometheus_registry())
            .build(),
    );

    let inherent_data_providers = build_inherent_data_providers()?;

    let pow_block_import = sc_consensus_pow::PowBlockImport::new(
        Arc::clone(&client),
        Arc::clone(&client),
        pow_algorithm,
        0, // check inherents starting at block 0
        select_chain.clone(),
        inherent_data_providers,
    );

    let logging_block_import = LoggingBlockImport::new(pow_block_import);

    let import_queue = sc_consensus_pow::import_queue(
        Box::new(logging_block_import.clone()),
        None,
        QPowAlgorithm {
            client: client.clone(),
            _phantom: Default::default(),
        },
        &task_manager.spawn_essential_handle(),
        config.prometheus_registry(),
    )?;

	Ok(sc_service::PartialComponents {
		client,
		backend,
		task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		other: (logging_block_import, telemetry),
	})
}

/// Builds a new service for a full client.
pub fn new_full<
    N: sc_network::NetworkBackend<Block, <Block as sp_runtime::traits::Block>::Hash>,
>(
    config: Configuration,
) -> Result<TaskManager, ServiceError> {
    let sc_service::PartialComponents {
        client,
        backend,
        mut task_manager,
        import_queue,
        keystore_container,
        select_chain,
        transaction_pool,
        other: (pow_block_import, mut telemetry),
    } = new_partial(&config)?;

    let mut tx_stream = transaction_pool.clone().import_notification_stream();

    let net_config = sc_network::config::FullNetworkConfiguration::<
        Block,
        <Block as sp_runtime::traits::Block>::Hash,
        N,
    >::new(&config.network, config.prometheus_registry().cloned());
    let metrics = N::register_notification_metrics(config.prometheus_registry());

    let (network, system_rpc_tx, tx_handler_controller, network_starter, sync_service) =
        sc_service::build_network(sc_service::BuildNetworkParams {
            config: &config,
            net_config,
            client: client.clone(),
            transaction_pool: transaction_pool.clone(),
            spawn_handle: task_manager.spawn_handle(),
            import_queue,
            block_announce_validator_builder: None,
            warp_sync_config: None,
            block_relay: None,
            metrics,
        })?;

	if config.offchain_worker.enabled {
		let offchain_workers =
			sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
				runtime_api_provider: client.clone(),
				is_validator: config.role.is_authority(),
				keystore: Some(keystore_container.keystore()),
				offchain_db: backend.offchain_storage(),
				transaction_pool: Some(OffchainTransactionPoolFactory::new(
					transaction_pool.clone(),
				)),
				network_provider: Arc::new(network.clone()),
				enable_http_requests: true,
				custom_extensions: |_| vec![],
			})?;
		task_manager.spawn_handle().spawn(
			"offchain-workers-runner",
			"offchain-worker",
			offchain_workers.run(client.clone(), task_manager.spawn_handle()).boxed(),
		);
	}

    let role = config.role;
    let prometheus_registry = config.prometheus_registry().cloned();

    let rpc_extensions_builder = {
        let client = client.clone();
        let pool = transaction_pool.clone();

        Box::new(move |_| {
            let deps = crate::rpc::FullDeps {
                client: client.clone(),
                pool: pool.clone(),
            };
            crate::rpc::create_full(deps).map_err(Into::into)
        })
    };

    let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
        network: Arc::new(network.clone()),
        client: client.clone(),
        keystore: keystore_container.keystore(),
        task_manager: &mut task_manager,
        transaction_pool: transaction_pool.clone(),
        rpc_builder: rpc_extensions_builder,
        backend,
        system_rpc_tx,
        tx_handler_controller,
        sync_service: sync_service.clone(),
        config,
        telemetry: telemetry.as_mut(),
    })?;

    if role.is_authority() {

        let proposer = sc_basic_authorship::ProposerFactory::new(
            task_manager.spawn_handle(),
            client.clone(),
            transaction_pool.clone(),
            prometheus_registry.as_ref(),
            None, // lets worry about telemetry later! TODO
        );

        let inherent_data_providers = build_inherent_data_providers()?;

        // Parameter details:
        //   https://substrate.dev/rustdocs/v3.0.0/sc_consensus_pow/fn.start_mining_worker.html
        // Also refer to kulupu config:
        //   https://github.com/kulupu/kulupu/blob/master/src/service.rs

        let pow_algorithm = QPowAlgorithm {
            client: client.clone(),
            _phantom: Default::default(),
        };


        let wormhole_pair = WormholePair::generate_new().unwrap();

        log::info!("Wormhole address {:?}",wormhole_pair.address);
        log::info!("Wormhole secret {:?}",wormhole_pair.secret);

        let miner_id = AccountId32::from(wormhole_pair.address.0);

        log::info!("⛏️ Mining with identity: {:?}", miner_id);

        // Encode the miner ID for pre-runtime digest
        let encoded_miner = miner_id.encode();



        let (worker_handle, worker_task) = sc_consensus_pow::start_mining_worker(
            //block_import: BoxBlockImport<Block>,
            Box::new(pow_block_import),
            client.clone(),
            select_chain,
            pow_algorithm,
            proposer, // Env E == proposer! TODO
            /*sync_oracle:*/ sync_service.clone(),
            /*justification_sync_link:*/ sync_service.clone(),
            Some(encoded_miner), //pre_runtime as Option<Vec<u8>>
            inherent_data_providers,
            // time to wait for a new block before starting to mine a new one
            Duration::from_secs(10),
            // how long to take to actually build the block (i.e. executing extrinsics)
            Duration::from_secs(10),
        );

        task_manager
            .spawn_essential_handle()
            .spawn_blocking("pow", None, worker_task);

        let client_monitoring = client.clone();
        let prometheus_registry_monitoring = prometheus_registry.clone();
        task_manager.spawn_essential_handle().spawn(
            "monitoring_qpow",
            None,
            async move {
                log::info!("⚙️  QPoW Monitoring task spawned");
                let gauge_vec =
                    if let Some(registry) = prometheus_registry_monitoring.as_ref() {
                        Some(ResonanceBusinessMetrics::register_gauge_vec(registry))
                    } else {
                        None
                    };

                let mut sub = client_monitoring.import_notification_stream();
                while let Some(notification) = sub.next().await {
                    let block_hash = notification.hash;
                    if let Some(ref gauge) = gauge_vec {
                        gauge.with_label_values(&["median_block_time"]).set(
                            client_monitoring.runtime_api().get_median_block_time(block_hash).unwrap_or(0) as f64
                        );
                        gauge.with_label_values(&["difficulty"]).set(
                            client_monitoring.runtime_api().get_difficulty(block_hash).unwrap_or(0) as f64
                        );
                        gauge.with_label_values(&["last_block_time"]).set(
                            client_monitoring.runtime_api().get_last_block_time(block_hash).unwrap_or(0) as f64
                        );
                        gauge.with_label_values(&["last_block_duration"]).set(
                            client_monitoring.runtime_api().get_last_block_duration(block_hash).unwrap_or(0) as f64
                        );
                    }else{
                        log::warn!("QPoW Monitoring: Prometheus registry not found");
                    }

                }
            }
        );

        task_manager.spawn_essential_handle().spawn(
            "qpow-mining",
            None,
            async move {
                log::info!("⚙️  QPoW Mining task spawned");
                let mut nonce: U512 = U512::zero();
                loop {
                    // Get mining metadata
                    let metadata = match worker_handle.metadata() {
                        Some(m) => m,
                        None => {
                            log::warn!(target: "pow", "No mining metadata available");
                            tokio::time::sleep(Duration::from_millis(250)).await;
                            continue;
                        }
                    };
                    let version = worker_handle.version();

                    // Mine the block

                    let miner = QPoWMiner::new(client.clone());

                    let seal: QPoWSeal =
                        match miner.try_nonce::<Block>(metadata.best_hash, metadata.pre_hash, nonce.to_big_endian()) {
                            Ok(s) => {
                                log::info!("valid nonce: {} ==> {:?}", nonce, s);
                                s
                            }
                            Err(_) => {
                                nonce += U512::one();
                                continue;
                            }
                        };

                    log::info!("block found");

                    let current_version = worker_handle.version();
                    if current_version == version {
                        if futures::executor::block_on(worker_handle.submit(seal.encode())) {
                            log::info!("Successfully mined and submitted a new block");
                            nonce = U512::zero();
                        } else {
                            log::warn!("Failed to submit mined block");
                            nonce += U512::one();
                        }
                    }
                }
            },
        );

        task_manager.spawn_handle().spawn("tx-logger", None, async move {
            while let Some(tx_hash) = tx_stream.next().await {
                if let Some(tx) = transaction_pool.ready_transaction(&tx_hash) {
                    log::info!("New transaction: Hash = {:?}", tx_hash);
                    let extrinsic = tx.data();
                    log::info!("Payload: {:?}", extrinsic);
                    // log::info!("Signature: {:?}", tx.data());
                    // log::info!("Signer: {:?}", tx.);
                } else {
                    log::warn!("Transaction {:?} not found in pool", tx_hash);
                }
            }
        });

        log::info!("⛏️  Pow miner spawned");
    }


    network_starter.start_network();
    Ok(task_manager)
}