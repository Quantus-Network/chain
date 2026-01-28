//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.
//!
//! This module provides the main service setup for a Quantus node, including:
//! - Network configuration and setup
//! - Transaction pool management
//! - Mining infrastructure (local and external miner support)
//! - RPC endpoint configuration

use futures::FutureExt;
#[cfg(feature = "tx-logging")]
use futures::StreamExt;
use quantus_runtime::{self, apis::RuntimeApi, opaque::Block};
use sc_client_api::Backend;
use sc_consensus_qpow::{ChainManagement, MiningHandle};
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_telemetry::{Telemetry, TelemetryWorker};
#[cfg(feature = "tx-logging")]
use sc_transaction_pool_api::InPoolTransaction;
use sc_transaction_pool_api::{OffchainTransactionPoolFactory, TransactionPool};
use sp_inherents::CreateInherentDataProviders;
use tokio_util::sync::CancellationToken;

use crate::{miner_server::MinerServer, prometheus::BusinessMetrics};
use codec::Encode;
use jsonrpsee::tokio;
use quantus_miner_api::{ApiResponseStatus, MiningRequest, MiningResult};
use sc_basic_authorship::ProposerFactory;
use sc_cli::TransactionPoolType;
use sc_transaction_pool::TransactionPoolOptions;
use sp_api::ProvideRuntimeApi;
use sp_consensus::SyncOracle;
use sp_consensus_qpow::QPoWApi;
use sp_core::{crypto::AccountId32, U512};
use std::sync::Arc;
use std::time::Duration;

/// Frequency of block import logging. Every 1000 blocks.
const LOG_FREQUENCY: u64 = 1000;

// ============================================================================
// External Mining Helper Functions
// ============================================================================

/// Parse a mining result and extract the seal if valid.
fn parse_mining_result(result: &MiningResult, expected_job_id: &str) -> Option<Vec<u8>> {
	// Check job ID matches
	if result.job_id != expected_job_id {
		log::debug!(target: "miner", "Received stale result for job {}, ignoring", result.job_id);
		return None;
	}

	// Check status
	if result.status != ApiResponseStatus::Completed {
		match result.status {
			ApiResponseStatus::Failed => log::warn!("⛏️ Mining job failed"),
			ApiResponseStatus::Cancelled => {
				log::debug!(target: "miner", "Mining job was cancelled")
			},
			_ => log::debug!(target: "miner", "Unexpected result status: {:?}", result.status),
		}
		return None;
	}

	// Extract and decode work
	let work_hex = result.work.as_ref()?;
	match hex::decode(work_hex) {
		Ok(seal) if seal.len() == 64 => Some(seal),
		Ok(seal) => {
			log::warn!("⛏️ Invalid seal length from miner: {} bytes", seal.len());
			None
		},
		Err(e) => {
			log::warn!("⛏️ Failed to decode work hex: {}", e);
			None
		},
	}
}

/// Wait for a mining result from the miner server.
///
/// Returns `Some(seal)` if a valid 64-byte seal is received, `None` otherwise
/// (interrupted, failed, invalid, or stale).
///
/// The `should_stop` closure should return `true` if we should stop waiting
/// (e.g., new block arrived or shutdown requested).
///
/// This function will keep waiting even if all miners disconnect, since newly
/// connecting miners automatically receive the current job and can submit results.
async fn wait_for_mining_result<F>(
	server: &Arc<MinerServer>,
	job_id: &str,
	should_stop: F,
) -> Option<Vec<u8>>
where
	F: Fn() -> bool,
{
	loop {
		if should_stop() {
			return None;
		}

		match server.recv_result_timeout(Duration::from_millis(500)).await {
			Some(result) => {
				if let Some(seal) = parse_mining_result(&result, job_id) {
					return Some(seal);
				}
				// Keep waiting for other miners (stale, failed, or invalid parse)
			},
			None => {
				// Timeout, continue waiting
			},
		}
	}
}

// ============================================================================
// Mining Loop Helpers
// ============================================================================

/// Result of attempting to mine with an external miner.
enum ExternalMiningOutcome {
	/// Successfully found and imported a seal.
	Success,
	/// Mining was interrupted (new block, cancellation, or failure).
	Interrupted,
}

/// Handle a single round of external mining.
///
/// Broadcasts the job to connected miners and waits for results.
/// If a seal fails validation, continues waiting for more seals.
/// Only returns when a seal is successfully imported, or when interrupted.
async fn handle_external_mining(
	server: &Arc<MinerServer>,
	client: &Arc<FullClient>,
	worker_handle: &MiningHandle<
		Block,
		FullClient,
		Arc<sc_network_sync::SyncingService<Block>>,
		(),
	>,
	cancellation_token: &CancellationToken,
	job_counter: &mut u64,
	mining_start_time: &mut std::time::Instant,
) -> ExternalMiningOutcome {
	let metadata = match worker_handle.metadata() {
		Some(m) => m,
		None => return ExternalMiningOutcome::Interrupted,
	};

	// Get difficulty from runtime
	let difficulty = match client.runtime_api().get_difficulty(metadata.best_hash) {
		Ok(d) => d,
		Err(e) => {
			log::warn!("⛏️ Failed to get difficulty: {:?}", e);
			return ExternalMiningOutcome::Interrupted;
		},
	};

	// Create and broadcast job
	*job_counter += 1;
	let job_id = job_counter.to_string();
	let mining_hash = hex::encode(metadata.pre_hash.as_bytes());
	log::info!(
		"⛏️ Broadcasting job {}: pre_hash={}, difficulty={}",
		job_id,
		mining_hash,
		difficulty
	);
	let job = MiningRequest {
		job_id: job_id.clone(),
		mining_hash,
		distance_threshold: difficulty.to_string(),
	};

	server.broadcast_job(job).await;

	// Wait for results from miners, retrying on invalid seals
	let best_hash = metadata.best_hash;
	loop {
		let seal = match wait_for_mining_result(server, &job_id, || {
			cancellation_token.is_cancelled()
				|| worker_handle.metadata().map(|m| m.best_hash != best_hash).unwrap_or(true)
		})
		.await
		{
			Some(seal) => seal,
			None => return ExternalMiningOutcome::Interrupted,
		};

		// Verify the seal before attempting to submit (submit consumes the build)
		if !worker_handle.verify_seal(&seal) {
			log::warn!(
				"⛏️ Invalid seal from miner, continuing to wait for valid seals (job {})",
				job_id
			);
			continue;
		}

		// Seal is valid, submit it
		if futures::executor::block_on(worker_handle.submit(seal.clone())) {
			let mining_time = mining_start_time.elapsed().as_secs();
			log::info!(
				"🥇 Successfully mined and submitted a new block via external miner (mining time: {}s)",
				mining_time
			);
			*mining_start_time = std::time::Instant::now();
			return ExternalMiningOutcome::Success;
		}

		// Submit failed for some other reason (should be rare after verify_seal passed)
		log::warn!("⛏️ Failed to submit verified seal, continuing to wait (job {})", job_id);
	}
}

/// Try to find a valid nonce for local mining.
///
/// Tries 50k nonces from a random starting point, then yields to check for new blocks.
/// With Poseidon2 hashing this takes ~50-100ms, keeping the node responsive.
async fn handle_local_mining(
	client: &Arc<FullClient>,
	worker_handle: &MiningHandle<
		Block,
		FullClient,
		Arc<sc_network_sync::SyncingService<Block>>,
		(),
	>,
) -> Option<Vec<u8>> {
	let metadata = worker_handle.metadata()?;
	let version = worker_handle.version();
	let block_hash = metadata.pre_hash.0;
	let difficulty = client.runtime_api().get_difficulty(metadata.best_hash).unwrap_or_else(|e| {
		log::warn!("API error getting difficulty: {:?}", e);
		U512::zero()
	});

	if difficulty.is_zero() {
		return None;
	}

	let start_nonce = U512::from(rand::random::<u128>());
	let target = U512::MAX / difficulty;

	let found = tokio::task::spawn_blocking(move || {
		let mut nonce = start_nonce;
		for _ in 0..50_000 {
			let nonce_bytes = nonce.to_big_endian();
			if qpow_math::get_nonce_hash(block_hash, nonce_bytes) < target {
				return Some(nonce_bytes);
			}
			nonce = nonce.overflowing_add(U512::one()).0;
		}
		None
	})
	.await
	.ok()
	.flatten();

	found.filter(|_| worker_handle.version() == version).map(|nonce| nonce.encode())
}

/// Submit a mined seal to the worker handle.
///
/// Returns `true` if submission was successful, `false` otherwise.
fn submit_mined_block(
	worker_handle: &MiningHandle<
		Block,
		FullClient,
		Arc<sc_network_sync::SyncingService<Block>>,
		(),
	>,
	seal: Vec<u8>,
	mining_start_time: &mut std::time::Instant,
	source: &str,
) -> bool {
	if futures::executor::block_on(worker_handle.submit(seal)) {
		let mining_time = mining_start_time.elapsed().as_secs();
		log::info!(
			"🥇 Successfully mined and submitted a new block{} (mining time: {}s)",
			source,
			mining_time
		);
		*mining_start_time = std::time::Instant::now();
		true
	} else {
		log::warn!("⛏️ Failed to submit mined block{}", source);
		false
	}
}

/// The main mining loop that coordinates local and external mining.
///
/// This function runs continuously until the cancellation token is triggered.
/// It handles:
/// - Waiting for sync to complete
/// - Coordinating with external miners (if server is available)
/// - Falling back to local mining
async fn mining_loop(
	client: Arc<FullClient>,
	worker_handle: MiningHandle<Block, FullClient, Arc<sc_network_sync::SyncingService<Block>>, ()>,
	sync_service: Arc<sc_network_sync::SyncingService<Block>>,
	miner_server: Option<Arc<MinerServer>>,
	cancellation_token: CancellationToken,
) {
	log::info!("⛏️ QPoW Mining task spawned");

	let mut mining_start_time = std::time::Instant::now();
	let mut job_counter: u64 = 0;

	loop {
		if cancellation_token.is_cancelled() {
			log::info!("⛏️ QPoW Mining task shutting down gracefully");
			break;
		}

		// Don't mine if we're still syncing
		if sync_service.is_major_syncing() {
			log::debug!(target: "pow", "Mining paused: node is still syncing with network");
			tokio::select! {
				_ = tokio::time::sleep(Duration::from_secs(5)) => {}
				_ = cancellation_token.cancelled() => continue
			}
			continue;
		}

		// Wait for mining metadata to be available
		if worker_handle.metadata().is_none() {
			log::debug!(target: "pow", "No mining metadata available");
			tokio::select! {
				_ = tokio::time::sleep(Duration::from_millis(250)) => {}
				_ = cancellation_token.cancelled() => continue
			}
			continue;
		}

		// External mining path
		if let Some(ref server) = miner_server {
			handle_external_mining(
				server,
				&client,
				&worker_handle,
				&cancellation_token,
				&mut job_counter,
				&mut mining_start_time,
			)
			.await;
			continue;
		}

		// Local mining path
		if let Some(seal) = handle_local_mining(&client, &worker_handle).await {
			submit_mined_block(&worker_handle, seal, &mut mining_start_time, "");
		}

		// Yield to let other async tasks run
		tokio::task::yield_now().await;
	}

	log::info!("⛏️ QPoW Mining task terminated");
}

/// Spawn the transaction logger task.
///
/// This task logs transactions as they are added to the pool.
/// Only available when the `tx-logging` feature is enabled.
#[cfg(feature = "tx-logging")]
fn spawn_transaction_logger(
	task_manager: &TaskManager,
	transaction_pool: Arc<sc_transaction_pool::TransactionPoolHandle<Block, FullClient>>,
	tx_stream: impl futures::Stream<Item = sp_core::H256> + Send + 'static,
) {
	task_manager.spawn_handle().spawn("tx-logger", None, async move {
		let tx_stream = tx_stream;
		futures::pin_mut!(tx_stream);
		while let Some(tx_hash) = tx_stream.next().await {
			if let Some(tx) = transaction_pool.ready_transaction(&tx_hash) {
				log::trace!(target: "miner", "New transaction: Hash = {:?}", tx_hash);
				let extrinsic = tx.data();
				log::trace!(target: "miner", "Payload: {:?}", extrinsic);
			} else {
				log::warn!("⛏️ Transaction {:?} not found in pool", tx_hash);
			}
		}
	});
}

/// Spawn all authority-related tasks (mining, metrics, transaction logging).
///
/// This is only called when the node is running as an authority (block producer).
#[allow(clippy::too_many_arguments)]
fn spawn_authority_tasks(
	task_manager: &mut TaskManager,
	client: Arc<FullClient>,
	transaction_pool: Arc<sc_transaction_pool::TransactionPoolHandle<Block, FullClient>>,
	select_chain: FullSelectChain,
	pow_block_import: PowBlockImport,
	sync_service: Arc<sc_network_sync::SyncingService<Block>>,
	prometheus_registry: Option<prometheus::Registry>,
	rewards_address: AccountId32,
	miner_listen_port: Option<u16>,
	tx_stream_for_worker: impl futures::Stream<Item = sp_core::H256> + Send + Unpin + 'static,
	#[cfg(feature = "tx-logging")] tx_stream_for_logger: impl futures::Stream<Item = sp_core::H256>
		+ Send
		+ 'static,
) {
	// Create block proposer factory
	let proposer = ProposerFactory::new(
		task_manager.spawn_handle(),
		client.clone(),
		transaction_pool.clone(),
		prometheus_registry.as_ref(),
		None,
	);

	// Create inherent data providers
	let inherent_data_providers = Box::new(move |_, _| async move {
		let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
		Ok(timestamp)
	})
		as Box<
			dyn CreateInherentDataProviders<
				Block,
				(),
				InherentDataProviders = sp_timestamp::InherentDataProvider,
			>,
		>;

	// Start the mining worker (block building task)
	let (worker_handle, worker_task) = sc_consensus_qpow::start_mining_worker(
		Box::new(pow_block_import),
		client.clone(),
		select_chain,
		proposer,
		sync_service.clone(),
		sync_service.clone(),
		rewards_address,
		inherent_data_providers,
		tx_stream_for_worker,
		Duration::from_secs(10),
	);

	task_manager
		.spawn_essential_handle()
		.spawn_blocking("block-producer", None, worker_task);

	// Start Prometheus business metrics monitoring
	BusinessMetrics::start_monitoring_task(client.clone(), prometheus_registry, task_manager);

	// Setup graceful shutdown for mining
	let mining_cancellation_token = CancellationToken::new();
	let mining_token_clone = mining_cancellation_token.clone();

	task_manager.spawn_handle().spawn("mining-shutdown-listener", None, async move {
		tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
		log::info!("🛑 Received Ctrl+C signal, shutting down qpow-mining worker");
		mining_token_clone.cancel();
	});

	// Spawn the main mining loop
	task_manager.spawn_essential_handle().spawn("qpow-mining", None, async move {
		// Start miner server if port is specified
		let miner_server: Option<Arc<MinerServer>> = if let Some(port) = miner_listen_port {
			match MinerServer::start(port).await {
				Ok(server) => Some(server),
				Err(e) => {
					log::error!("⛏️ Failed to start miner server on port {}: {}", port, e);
					None
				},
			}
		} else {
			None
		};

		mining_loop(client, worker_handle, sync_service, miner_server, mining_cancellation_token)
			.await;
	});

	// Spawn transaction logger (only when tx-logging feature is enabled)
	#[cfg(feature = "tx-logging")]
	spawn_transaction_logger(task_manager, transaction_pool, tx_stream_for_logger);

	log::info!(target: "miner", "⛏️  Pow miner spawned");
}

// ============================================================================
// Type Definitions
// ============================================================================

pub(crate) type FullClient = sc_service::TFullClient<
	Block,
	RuntimeApi,
	sc_executor::WasmExecutor<sp_io::SubstrateHostFunctions>,
>;
type FullBackend = sc_service::TFullBackend<Block>;
type FullSelectChain = sc_consensus_qpow::HeaviestChain<Block, FullClient, FullBackend>;
pub type PowBlockImport = sc_consensus_qpow::PowBlockImport<
	Block,
	Arc<FullClient>,
	FullClient,
	FullSelectChain,
	Box<
		dyn sp_inherents::CreateInherentDataProviders<
			Block,
			(),
			InherentDataProviders = sp_timestamp::InherentDataProvider,
		>,
	>,
	LOG_FREQUENCY,
>;

pub type Service = sc_service::PartialComponents<
	FullClient,
	FullBackend,
	FullSelectChain,
	sc_consensus::DefaultImportQueue<Block>,
	sc_transaction_pool::TransactionPoolHandle<Block, FullClient>,
	(PowBlockImport, Option<Telemetry>),
>;

#[allow(clippy::result_large_err)]
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
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
		telemetry
	});

	let select_chain = sc_consensus_qpow::HeaviestChain::new(backend.clone(), Arc::clone(&client));

	let pool_options = TransactionPoolOptions::new_with_params(
		36772, /* each tx is about 7300 bytes so if we have 268MB for the pool we can fit this
		        * many txs */
		268_435_456,
		None,
		TransactionPoolType::ForkAware.into(),
		false,
	);
	let transaction_pool = Arc::from(
		sc_transaction_pool::Builder::new(
			task_manager.spawn_essential_handle(),
			client.clone(),
			config.role.is_authority().into(),
		)
		.with_options(pool_options)
		.with_prometheus(config.prometheus_registry())
		.build(),
	);

	let inherent_data_providers = Box::new(move |_, _| async move {
		let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
		Ok(timestamp)
	})
		as Box<
			dyn CreateInherentDataProviders<
				Block,
				(),
				InherentDataProviders = sp_timestamp::InherentDataProvider,
			>,
		>;

	let pow_block_import = sc_consensus_qpow::PowBlockImport::new(
		Arc::clone(&client),
		Arc::clone(&client),
		0, // check inherents starting at block 0
		select_chain.clone(),
		inherent_data_providers,
	);

	let import_queue = sc_consensus_qpow::import_queue::<Block, FullClient>(
		Box::new(pow_block_import.clone()),
		None,
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
#[allow(clippy::result_large_err)]
pub fn new_full<
	N: sc_network::NetworkBackend<Block, <Block as sp_runtime::traits::Block>::Hash>,
>(
	config: Configuration,
	rewards_address: AccountId32,
	miner_listen_port: Option<u16>,
	enable_peer_sharing: bool,
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

	let tx_stream_for_worker = transaction_pool.clone().import_notification_stream();
	#[cfg(feature = "tx-logging")]
	let tx_stream_for_logger = transaction_pool.clone().import_notification_stream();

	let net_config = sc_network::config::FullNetworkConfiguration::<
		Block,
		<Block as sp_runtime::traits::Block>::Hash,
		N,
	>::new(&config.network, config.prometheus_registry().cloned());
	let metrics = N::register_notification_metrics(config.prometheus_registry());

	let (network, system_rpc_tx, tx_handler_controller, sync_service) =
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
		let network_for_rpc = if enable_peer_sharing { Some(network.clone()) } else { None };

		Box::new(move |_| {
			let deps = crate::rpc::FullDeps {
				client: client.clone(),
				pool: pool.clone(),
				network: network_for_rpc.clone(),
			};
			crate::rpc::create_full(deps).map_err(Into::into)
		})
	};

	log::info!("🧹 Blocks pruning mode: {:?}", config.blocks_pruning);
	log::info!("📦 State pruning mode: {:?}", config.state_pruning);

	let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		network: network.clone(),
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
		#[cfg(feature = "tx-logging")]
		spawn_authority_tasks(
			&mut task_manager,
			client,
			transaction_pool,
			select_chain.clone(),
			pow_block_import,
			sync_service,
			prometheus_registry,
			rewards_address,
			miner_listen_port,
			tx_stream_for_worker,
			tx_stream_for_logger,
		);
		#[cfg(not(feature = "tx-logging"))]
		spawn_authority_tasks(
			&mut task_manager,
			client,
			transaction_pool,
			select_chain.clone(),
			pow_block_import,
			sync_service,
			prometheus_registry,
			rewards_address,
			miner_listen_port,
			tx_stream_for_worker,
		);
	}

	// Start deterministic-depth finalization task
	ChainManagement::spawn_finalization_task(Arc::new(select_chain.clone()), &task_manager);

	Ok(task_manager)
}
