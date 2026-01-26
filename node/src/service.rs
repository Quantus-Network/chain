//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use futures::{FutureExt, StreamExt};
use quantus_runtime::{self, apis::RuntimeApi, opaque::Block};
use sc_client_api::Backend;
use sc_consensus_qpow::ChainManagement;
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_telemetry::{Telemetry, TelemetryWorker};
use sc_transaction_pool_api::{InPoolTransaction, OffchainTransactionPoolFactory, TransactionPool};
use sp_inherents::CreateInherentDataProviders;
use tokio_util::sync::CancellationToken;

use crate::{external_miner_client::QuicMinerClient, prometheus::ResonanceBusinessMetrics};
use codec::Encode;
use jsonrpsee::tokio;
use qpow_math::mine_range;
use quantus_miner_api::{ApiResponseStatus, MiningResult};
use sc_cli::TransactionPoolType;
use sc_transaction_pool::TransactionPoolOptions;
use sp_api::ProvideRuntimeApi;
use sp_consensus::SyncOracle;
use sp_consensus_qpow::QPoWApi;
use sp_core::{crypto::AccountId32, H256, U512};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

/// Frequency of block import logging. Every 1000 blocks.
const LOG_FREQUENCY: u64 = 1000;

// ============================================================================
// External Mining Helper Types and Functions
// ============================================================================

/// Result of waiting for an external mining result.
enum ExternalMiningOutcome {
	/// Successfully found a valid seal (64 bytes).
	Success(Vec<u8>),
	/// Mining completed but result was invalid, stale, cancelled, or failed.
	Failed,
	/// New block arrived, need to send new job.
	NewBlock,
	/// Shutdown requested.
	Shutdown,
}

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

/// Wait for a mining result from the external miner.
///
/// Returns when:
/// - A valid result is received
/// - A new block is detected (need to send new job)
/// - The operation is cancelled
///
/// The `check_new_block` closure should return `true` if a new block has arrived.
async fn wait_for_mining_result<F>(
	miner: &QuicMinerClient,
	job_id: &str,
	check_new_block: F,
	cancellation_token: &CancellationToken,
) -> ExternalMiningOutcome
where
	F: Fn() -> bool,
{
	loop {
		// Check for new block
		if check_new_block() {
			log::debug!(target: "miner", "New block detected, will send new job");
			return ExternalMiningOutcome::NewBlock;
		}

		// Check for shutdown
		if cancellation_token.is_cancelled() {
			return ExternalMiningOutcome::Shutdown;
		}

		// Wait for result with timeout
		match miner.recv_result_timeout(Duration::from_millis(500)).await {
			Some(result) => {
				if let Some(seal) = parse_mining_result(&result, job_id) {
					return ExternalMiningOutcome::Success(seal);
				}
				// For completed but invalid results, or failed/cancelled, stop waiting
				if result.job_id == job_id {
					return ExternalMiningOutcome::Failed;
				}
				// Stale result for different job, keep waiting
			},
			None => {
				// Timeout, continue waiting
			},
		}
	}
}

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
	external_miner_addr: Option<String>,
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
		let proposer = sc_basic_authorship::ProposerFactory::new(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool.clone(),
			prometheus_registry.as_ref(),
			None, // lets worry about telemetry later! TODO
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

		let (worker_handle, worker_task) = sc_consensus_qpow::start_mining_worker(
			Box::new(pow_block_import),
			client.clone(),
			select_chain.clone(),
			proposer,
			sync_service.clone(),
			sync_service.clone(),
			rewards_address,
			inherent_data_providers,
			tx_stream_for_worker,
			Duration::from_secs(10),
		);

		task_manager.spawn_essential_handle().spawn_blocking("pow", None, worker_task);

		ResonanceBusinessMetrics::start_monitoring_task(
			client.clone(),
			prometheus_registry.clone(),
			&task_manager,
		);

		let mining_cancellation_token = CancellationToken::new();
		let mining_token_clone = mining_cancellation_token.clone();

		// Listen for shutdown signals
		task_manager.spawn_handle().spawn("mining-shutdown-listener", None, async move {
			tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
			log::info!("🛑 Received Ctrl+C signal, shutting down qpow-mining worker");
			mining_token_clone.cancel();
		});

		task_manager.spawn_essential_handle().spawn("qpow-mining", None, async move {
			log::info!("⛏️ QPoW Mining task spawned");
			let mut nonce: U512 = U512::one();
			let mut current_job_id: Option<String> = None;

			// Connect to external miner if address is provided
			let miner_client: Option<QuicMinerClient> = if let Some(ref addr_str) = external_miner_addr {
				match addr_str.parse::<std::net::SocketAddr>() {
					Ok(addr) => {
						match QuicMinerClient::connect(addr).await {
							Ok(client) => {
								log::info!("⛏️ Connected to external miner at {}", addr);
								Some(client)
							},
							Err(e) => {
								log::error!("⛏️ Failed to connect to external miner at {}: {}", addr, e);
								None
							}
						}
					},
					Err(e) => {
						log::error!("⛏️ Invalid external miner address '{}': {}", addr_str, e);
						None
					}
				}
			} else {
				None
			};

			// Submit new mining job
			let mut mining_start_time = std::time::Instant::now();
			log::info!("Mining start time: {:?}", mining_start_time);

			loop {
				// Check for cancellation
				if mining_cancellation_token.is_cancelled() {
					log::info!("⛏️ QPoW Mining task shutting down gracefully");
					// QUIC client will clean up on drop (connection closes, miner cancels job)
					break;
				}

				// Don't mine if we're still syncing
				if sync_service.is_major_syncing() {
					log::debug!(target: "pow", "Mining paused: node is still syncing with network");
					tokio::select! {
						_ = tokio::time::sleep(Duration::from_secs(5)) => {},
						_ = mining_cancellation_token.cancelled() => continue,
					}
					continue;
				}

				// Get mining metadata
				let metadata = match worker_handle.metadata() {
					Some(m) => m,
					None => {
						log::debug!(target: "pow", "No mining metadata available");
						tokio::select! {
							_ = tokio::time::sleep(Duration::from_millis(250)) => {},
							_ = mining_cancellation_token.cancelled() => continue,
						}
						continue;
					},
				};
				let version = worker_handle.version();

				// If external miner is connected, use external mining
				if let Some(ref miner) = miner_client {
					// Get difficulty from runtime
					let difficulty = match client.runtime_api().get_difficulty(metadata.best_hash) {
						Ok(d) => d,
						Err(e) => {
							log::warn!("⛏️ Failed to get difficulty: {:?}", e);
							tokio::select! {
								_ = tokio::time::sleep(Duration::from_millis(250)) => {},
								_ = mining_cancellation_token.cancelled() => continue,
							}
							continue;
						},
					};

					// Submit job to external miner
					let job_id = Uuid::new_v4().to_string();
					if let Err(e) = miner
						.send_job(&job_id, &metadata.pre_hash, difficulty, nonce, U512::max_value())
						.await
					{
						log::warn!("⛏️ Failed to submit mining job: {}", e);
						tokio::select! {
							_ = tokio::time::sleep(Duration::from_millis(250)) => {},
							_ = mining_cancellation_token.cancelled() => continue,
						}
						continue;
					}

					// Wait for result
					let best_hash = metadata.best_hash;
					let outcome = wait_for_mining_result(
						miner,
						&job_id,
						|| {
							worker_handle
								.metadata()
								.map(|m| m.best_hash != best_hash)
								.unwrap_or(false)
						},
						&mining_cancellation_token,
					)
					.await;

					match outcome {
						ExternalMiningOutcome::Success(seal) => {
							let current_version = worker_handle.version();
							if current_version != version {
								log::debug!(target: "miner", "Work from external miner is stale, discarding.");
							} else if futures::executor::block_on(worker_handle.submit(seal)) {
								let mining_time = mining_start_time.elapsed().as_secs();
								log::info!(
									"🥇 Successfully mined and submitted a new block via external miner (mining time: {}s)",
									mining_time
								);
								nonce = U512::one();
								mining_start_time = std::time::Instant::now();
							} else {
								log::warn!("⛏️ Failed to submit mined block from external miner");
								nonce += U512::one();
							}
						},
						ExternalMiningOutcome::NewBlock => {
							// Loop will continue and send new job
						},
						ExternalMiningOutcome::Shutdown => {
							break;
						},
						ExternalMiningOutcome::Failed => {
							// Continue to next iteration
						},
					}
				} else {
					// Local mining: try a range of N sequential nonces using optimized path
					let block_hash = metadata.pre_hash.0; // [u8;32]
					let start_nonce_bytes = nonce.to_big_endian();
					let difficulty = client
						.runtime_api()
						.get_difficulty(metadata.best_hash)
						.unwrap_or_else(|e| {
							log::warn!("API error getting difficulty: {:?}", e);
							U512::zero()
						});
					let nonces_to_mine = 300u64;

					let found = match tokio::task::spawn_blocking(move || {
						mine_range(block_hash, start_nonce_bytes, nonces_to_mine, difficulty)
					})
					.await
					{
						Ok(res) => res,
						Err(e) => {
							log::warn!("⛏️Local mining task failed: {}", e);
							None
						},
					};

					let nonce_bytes = if let Some((good_nonce, _distance)) = found {
						good_nonce
					} else {
						nonce += U512::from(nonces_to_mine);
						// Yield back to the runtime to avoid starving other tasks
						tokio::task::yield_now().await;
						continue;
					};

					let current_version = worker_handle.version();
					// TODO: what does this check do?
					if current_version == version {
						if futures::executor::block_on(worker_handle.submit(nonce_bytes.encode())) {
							let mining_time = mining_start_time.elapsed().as_secs();
							log::info!("🥇 Successfully mined and submitted a new block (mining time: {}s)", mining_time);
							nonce = U512::one();
							mining_start_time = std::time::Instant::now();
						} else {
							log::warn!("⛏️Failed to submit mined block");
							nonce += U512::one();
						}
					}

					// Yield after each mining batch to cooperate with other tasks
					tokio::task::yield_now().await;
				}
			}

			log::info!("⛏️ QPoW Mining task terminated");
		});

		task_manager.spawn_handle().spawn("tx-logger", None, async move {
			let mut tx_stream = tx_stream_for_logger;
			while let Some(tx_hash) = tx_stream.next().await {
				if let Some(tx) = transaction_pool.ready_transaction(&tx_hash) {
					log::trace!(target: "miner", "New transaction: Hash = {:?}", tx_hash);
					let extrinsic = tx.data();
					log::trace!(target: "miner", "Payload: {:?}", extrinsic);
				} else {
					log::warn!("⛏️Transaction {:?} not found in pool", tx_hash);
				}
			}
		});

		log::info!(target: "miner", "⛏️  Pow miner spawned");
	}

	// Start deterministic-depth finalization task
	ChainManagement::spawn_finalization_task(Arc::new(select_chain.clone()), &task_manager);

	Ok(task_manager)
}
