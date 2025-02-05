//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use futures::FutureExt;
use sc_client_api::Backend;
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_telemetry::{log, Telemetry, TelemetryWorker};
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use resonance_runtime::{self, apis::RuntimeApi, opaque::Block};
use sc_consensus_qpow::{QPoWWorker, import_queue as qpow_import_queue, QPoWBlockImport};
use std::sync::Arc;
use sc_basic_authorship::ProposerFactory;
use sp_consensus::DisableProofRecording;

pub(crate) type FullClient = sc_service::TFullClient<
	Block,
	RuntimeApi,
	sc_executor::WasmExecutor<sp_io::SubstrateHostFunctions>,
>;
type FullBackend = sc_service::TFullBackend<Block>;
type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;

pub type Service = sc_service::PartialComponents<
	FullClient,
	FullBackend,
	FullSelectChain,
	sc_consensus_qpow::QPoWImportQueue<Block>,
	sc_transaction_pool::FullPool<Block, FullClient>,
	(
		QPoWWorker<
			Block,
			FullClient,
			sc_transaction_pool::FullPool<Block, FullClient>,
			ProposerFactory<
				sc_transaction_pool::FullPool<Block, FullClient>,
				FullClient,
				DisableProofRecording
			>
		>,
		Option<Telemetry>,
	),
>;

pub fn new_partial(config: &mut Configuration) -> Result<Service, ServiceError> {
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

	log::info!("QPOW: Pruning NewPartial mode before: {:?}", config.state_pruning);
	config.state_pruning = Option::from(sc_service::config::PruningMode::ArchiveAll);
	log::info!("QPOW: Pruning NewPartial mode after: {:?}", config.state_pruning);
	
	let executor = sc_service::new_wasm_executor::<sp_io::SubstrateHostFunctions>(&config.executor);
	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts::<Block, RuntimeApi, _>(
			config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
		)?;
	let client = Arc::new(client);

	let telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
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

	let base_block_import = QPoWBlockImport::new(
		client.clone(),
		client.clone(),
		select_chain.clone(),
	);

	let proposer_factory = ProposerFactory::new(
		task_manager.spawn_handle(),
		client.clone(),
		transaction_pool.clone(),
		config.prometheus_registry(),
		telemetry.as_ref().map(|t| t.handle()),
	);

	let qpow_worker = QPoWWorker::new(
		client.clone(),
		Box::new(base_block_import.clone()),
		transaction_pool.clone(),
		proposer_factory
	);

	let import_queue = {
		//log::info!(target: "qpow", "🔄 Setting up import queue ....");
		qpow_import_queue(
			client.clone(),
			Box::new(base_block_import.clone()),
			select_chain.clone(),
			&task_manager.spawn_essential_handle(),
		).expect("Failed to create QPoW import queue")
	};

	Ok(sc_service::PartialComponents {
		client,
		backend,
		task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		other: (qpow_worker, telemetry),
	})

}

/// Builds a new service for a full client.
pub fn new_full<
	N: sc_network::NetworkBackend<Block, <Block as sp_runtime::traits::Block>::Hash>,
>(
	mut config: Configuration,
) -> Result<TaskManager, ServiceError> {
	let sc_service::PartialComponents {
		client,
		backend,
		mut task_manager,
		import_queue,
		keystore_container,
		select_chain: _,
		transaction_pool,
		other: (qpow_worker, mut telemetry),
	} = new_partial(&mut config)?;

	log::info!("QPOW: Pruning NewFull mode before: {:?}", config.state_pruning);
	config.state_pruning = Option::from(sc_service::config::PruningMode::ArchiveAll);
	log::info!("QPOW: Pruning NewFull mode after: {:?}", config.state_pruning);

	//let mut net_config = sc_network::config::FullNetworkConfiguration::new(&config.network, config.prometheus_registry().cloned());

	let net_config = sc_network::config::FullNetworkConfiguration::<
		Block,<Block as sp_runtime::traits::Block>::Hash,N,>::new(&config.network, config.prometheus_registry().cloned());

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
	//let prometheus_registry = config.prometheus_registry().cloned();
	let rpc_extensions_builder = {
		let client = client.clone();
		let pool = transaction_pool.clone();

		Box::new(move |_| {
			let deps = crate::rpc::FullDeps { client: client.clone(), pool: pool.clone() };
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

		// Start the QPoW worker
		task_manager.spawn_essential_handle().spawn_blocking(
			"qpow-worker",
			None,
			qpow_worker.start(),
		);
	}

	network_starter.start_network();
	Ok(task_manager)
}
