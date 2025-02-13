//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use futures::FutureExt;
use qpow::{QPow, Compute, QPoWSeal, MinimalQPowAlgorithm};
use sc_client_api::Backend;
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_telemetry::{Telemetry, TelemetryWorker};
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use resonance_runtime::{self, apis::RuntimeApi, opaque::Block};
use sp_core::{H256, U256};

use std::{sync::Arc, time::Duration};

use jsonrpsee::tokio;

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
    MinimalQPowAlgorithm,
    impl sp_inherents::CreateInherentDataProviders<Block, ()>,
>;
pub type Service = sc_service::PartialComponents<
    FullClient,
    FullBackend,
    FullSelectChain,
    sc_consensus::DefaultImportQueue<Block>,
    sc_transaction_pool::FullPool<Block, FullClient>,
    (PowBlockImport, Option<Telemetry>),
>;

pub fn build_inherent_data_providers(
) -> Result<impl sp_inherents::CreateInherentDataProviders<Block, ()>, ServiceError> {
    Ok(|_parent, _extra: ()| async move {
        let provider = sp_timestamp::InherentDataProvider::from_system_time();
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(provider)
    })
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

    let transaction_pool = sc_transaction_pool::BasicPool::new_full(
        config.transaction_pool.clone(),
        config.role.is_authority().into(),
        config.prometheus_registry(),
        task_manager.spawn_essential_handle(),
        client.clone(),
    );

    let inherent_data_providers = build_inherent_data_providers()?;
    let pow_block_import = sc_consensus_pow::PowBlockImport::new(
        client.clone(),
        client.clone(),
        MinimalQPowAlgorithm,
        0, // check inherents starting at block 0
        select_chain.clone(),
        inherent_data_providers,
    );

    let import_queue = sc_consensus_pow::import_queue(
        Box::new(pow_block_import.clone()),
        None,
        MinimalQPowAlgorithm,
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
        task_manager.spawn_handle().spawn(
            "offchain-workers-runner",
            "offchain-worker",
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
            })
            .run(client.clone(), task_manager.spawn_handle())
            .boxed(),
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

        let (worker_handle, worker_task) = sc_consensus_pow::start_mining_worker(
            //block_import: BoxBlockImport<Block>,
            Box::new(pow_block_import),
            client,
            select_chain,
            MinimalQPowAlgorithm,
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
                    println!("getting metadata");

                    let metadata = match worker_handle.metadata() {
                        Some(m) => m,
                        None => {
                            log::warn!(target: "pow", "No mining metadata available");
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                            continue;
                        }
                    };
                    let version = worker_handle.version();

                    println!("mine block");

                    // Mine the block
                    let seal =
					match try_nonce::<Block>(metadata.pre_hash, nonce, metadata.difficulty) {
                            Ok(s) => {
                                println!("valid seal: {:?}", s);
                                s
                            }
                            Err(_) => {
                                println!("error - seal not valid");
                                nonce += 1;
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                continue;
                            }
                        };

                    println!("block found");

                    let current_version = worker_handle.version();
                    if current_version == version {
                        if futures::executor::block_on(worker_handle.submit(seal.encode())) {
                            println!("Successfully mined and submitted a new block");
                            nonce = 0;
                        } else {
                            println!("Failed to submit mined block");
                            nonce += 1;
                        }
                    }

                    // Sleep to avoid spamming
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }, // .boxed()
        );

        println!("⛏️  Pow miner spawned");
    }

    network_starter.start_network();
    Ok(task_manager)
}

use codec::Encode;
use sp_runtime::traits::Block as BlockT;

fn try_nonce<B: BlockT<Hash = H256>>(
    pre_hash: B::Hash,
    nonce: u64,
    difficulty: U256,
) -> Result<QPoWSeal, ()> {

    let compute = Compute {
        difficulty,
        pre_hash: H256::from_slice(pre_hash.as_ref()),
        nonce,
    };

    // Compute the seal
    println!("compute difficulty: {:?}", difficulty);
    let seal = compute.compute();

    println!("compute done");

    // Convert pre_hash to [u8; 32] for verification
    // TODO normalize all the different ways we do calculations
    let header = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

    // Verify the solution using QPoW
    if !QPow::verify_solution(header, seal.work, difficulty.low_u64()) {
        println!("invalid seal");
        return Err(());
    }
    println!("good seal");

    Ok(seal)

}

#[cfg(test)]
mod tests {
    use qpow::INITIAL_DIFFICULTY;

    use super::*;
    use sp_core::H256;
    use sp_runtime::testing::{Block as TestBlock, H256 as TestH256, TestXt};
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
            println!("testing nonce: {:?}", nonce);
            if let Ok(seal) = try_nonce::<TestBlockType>(pre_hash, nonce, difficulty) {
                valid_seal = Some(seal);
                break;
            }
            nonce += 1;
        }

        println!("valid seal: {:?}", valid_seal);
        println!("nonce: {:?}", nonce);

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