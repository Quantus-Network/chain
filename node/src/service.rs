//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use futures::FutureExt;
use sc_consensus_qpow::{QPoWMiner, QPoWSeal, QPowAlgorithm};
use sc_client_api::Backend;
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_telemetry::{Telemetry, TelemetryWorker};
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use resonance_runtime::{self, apis::RuntimeApi, opaque::Block};

use std::{sync::Arc, time::Duration};
use codec::Encode;
use jsonrpsee::tokio;
use sp_api::__private::BlockT;

pub(crate) type FullClient = sc_service::TFullClient<
    Block,
    RuntimeApi,
    sc_executor::WasmExecutor<sp_io::SubstrateHostFunctions>,
>;
type FullBackend = sc_service::TFullBackend<Block>;
type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;

pub type PowBlockImport = sc_consensus_pow::PowBlockImport<
    Block,
    Arc<FullClient>,
    FullClient,
    FullSelectChain,
    QPowAlgorithm<Block, FullClient>,
    Box<dyn sp_inherents::CreateInherentDataProviders<Block, (), InherentDataProviders=sp_timestamp::InherentDataProvider>>,
>;
pub type Service = sc_service::PartialComponents<
    FullClient,
    FullBackend,
    FullSelectChain,
    sc_consensus::DefaultImportQueue<Block>,
    sc_transaction_pool::TransactionPoolHandle<Block, FullClient>,
    (PowBlockImport, Option<Telemetry>),
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

    let select_chain = sc_consensus::LongestChain::new(backend.clone());

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

    let pow_algorithm = QPowAlgorithm {
        client: client.clone(),
        _phantom: Default::default(),
    };

    let pow_block_import = sc_consensus_pow::PowBlockImport::new(
        client.clone(),
        client.clone(),
        pow_algorithm,
        0, // check inherents starting at block 0
        select_chain.clone(),
        inherent_data_providers,
    );

    let import_queue = sc_consensus_pow::import_queue(
        Box::new(pow_block_import.clone()),
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
		other: (pow_block_import, telemetry),
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
            transaction_pool,
            prometheus_registry.as_ref(),
            None, // lets worry about telemetry later! TODO
        );

        // let can_author_with =
        // 	sp_consensus::CanAuthorWithNativeVersion::new(client.executor().clone());

        let inherent_data_providers = build_inherent_data_providers()?;

        // Parameter details:
        //   https://substrate.dev/rustdocs/v3.0.0/sc_consensus_pow/fn.start_mining_worker.html
        // Also refer to kulupu config:
        //   https://github.com/kulupu/kulupu/blob/master/src/service.rs

        let pow_algorithm = QPowAlgorithm {
            client: client.clone(),
            _phantom: Default::default(),
        };

        let (worker_handle, worker_task) = sc_consensus_pow::start_mining_worker(
            //block_import: BoxBlockImport<Block>,
            Box::new(pow_block_import),
            client.clone(),
            select_chain,
            pow_algorithm,
            proposer, // Env E == proposer! TODO
            /*sync_oracle:*/ sync_service.clone(),
            /*justification_sync_link:*/ sync_service.clone(),
            //pre_runtime: Option<Vec<u8>>,
            None,
            inherent_data_providers,
            // time to wait for a new block before starting to mine a new one
            Duration::from_secs(10),
            // how long to take to actually build the block (i.e. executing extrinsics)
            Duration::from_secs(10),
        );

        task_manager
            .spawn_essential_handle()
            .spawn_blocking("pow", None, worker_task);

        task_manager.spawn_essential_handle().spawn(
            "pow-mining-actualy-real-mining-happening",
            None,
            async move {
                let mut nonce = 0;
                loop {
                    // Get mining metadata
                    log::info!("getting metadata");

                    let metadata = match worker_handle.metadata() {
                        Some(m) => m,
                        None => {
                            log::warn!(target: "pow", "No mining metadata available");
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                            continue;
                        }
                    };
                    let version = worker_handle.version();

                    log::info!("mine block");

                    // Mine the block

                    let miner = QPoWMiner::new(client.clone());

                    let seal: QPoWSeal =
                        match miner.try_nonce::<Block>(metadata.best_hash, metadata.pre_hash, nonce, metadata.difficulty) {
                            Ok(s) => {
                                log::info!("valid seal: {:?}", s);
                                s
                            }
                            Err(_) => {
                                log::info!("error - seal not valid");
                                nonce += 1;
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                continue;
                            }
                        };

                    log::info!("block found");

                    let current_version = worker_handle.version();
                    if current_version == version {
                        if futures::executor::block_on(worker_handle.submit(seal.encode())) {
                            log::info!("Successfully mined and submitted a new block");
                            nonce = 0;
                        } else {
                            log::info!("Failed to submit mined block");
                            nonce += 1;
                        }
                    }

                    // Sleep to avoid spamming
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }, // .boxed()
        );

        log::info!("⛏️  Pow miner spawned");
    }

    network_starter.start_network();
    Ok(task_manager)
}
/*
#[cfg(test)]
mod tests {
    use sc_service::{new_full_parts, TFullClient};
    use qpow::INITIAL_DIFFICULTY;

    use super::*;
    use sp_core::H256;
    // Import OpaqueExtrinsic (our opaque extrinsic type)
    use sp_runtime::OpaqueExtrinsic;
    // Define a TestXt with OpaqueExtrinsic as the Call and () as the Extra.
    type TestXtType = sp_runtime::testing::TestXt<OpaqueExtrinsic, ()>;
    // Now define our test block using that TestXtType:
    type TestBlockType = sp_runtime::testing::Block<TestXtType>;

    // Create a convenient type alias for our test block.
    // type TestBlockType = TestBlock<TestXt>;
    #[test]
    fn test_try_nonce_valid_seal() {
        // Setup test data
        let pre_hash = H256::from_slice(&[1; 32]);
        let difficulty = U256::from(INITIAL_DIFFICULTY);

        // First, find a valid nonce
        let mut nonce = 0;
        let mut valid_seal = None;
        while nonce < 1000 {
            log::info!("testing nonce: {:?}", nonce);
            if let Ok(seal) = try_nonce::<TestBlockType>(pre_hash, nonce, difficulty) {
                valid_seal = Some(seal);
                break;
            }
            nonce += 1;
        }

        log::info!("valid seal: {:?}", valid_seal);
        log::info!("nonce: {:?}", nonce);

        // Verify we found a valid seal
        assert!(valid_seal.is_some(), "Should find a valid seal");

        // Test that the valid seal passes verification
        let result = try_nonce::<TestBlockType>(pre_hash, valid_seal.unwrap().nonce, difficulty);
        assert!(result.is_ok(), "Valid seal should pass verification");
    }

    #[test]
    fn test_try_nonce_invalid_seal() {
        // Setup test data
        let pre_hash = H256::from_slice(&[1; 32]);
        let difficulty = U256::from(INITIAL_DIFFICULTY);

        // Use an obviously invalid nonce
        let invalid_nonce = 12345;

        // Test that the invalid seal fails verification
        let result = try_nonce::<TestBlockType>(pre_hash, invalid_nonce, difficulty);
        assert!(result.is_err(), "Invalid seal should fail verification");
    }
}
 */