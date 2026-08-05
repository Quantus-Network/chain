//! QUIC server for accepting connections from external miners.
//!
//! This module provides a QUIC server that miners connect to. It supports
//! multiple concurrent miners, broadcasting jobs to all connected miners
//! and collecting results.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────┐
//! │  Miner 1 │ ────┐
//! └──────────┘     │
//!                  │     ┌─────────────────┐
//! ┌──────────┐     ├────>│   MinerServer   │
//! │  Miner 2 │ ────┤     │  (QUIC Server)  │
//! └──────────┘     │     └─────────────────┘
//!                  │
//! ┌──────────┐     │
//! │  Miner 3 │ ────┘
//! └──────────┘
//! ```
//!
//! # Protocol
//!
//! - Node sends `MinerMessage::NewJob` to all connected miners
//! - Each miner independently selects a random nonce starting point
//! - First miner to find a valid solution sends `MinerMessage::JobResult`
//! - When a new job is broadcast, miners implicitly cancel their current work

use std::{
	collections::HashMap,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	time::Duration,
};

use jsonrpsee::tokio;
use quantus_miner_api::{read_message, write_message, MinerMessage, MiningRequest, MiningResult};
use tokio::sync::{mpsc, RwLock};

/// A QUIC server that accepts connections from miners.
pub struct MinerServer {
	/// Connected miners, keyed by unique ID.
	miners: Arc<RwLock<HashMap<u64, MinerHandle>>>,
	/// Channel to receive results from any miner.
	result_rx: tokio::sync::Mutex<mpsc::Receiver<MiningResult>>,
	/// Sender cloned to each miner connection handler.
	result_tx: mpsc::Sender<MiningResult>,
	/// Current job being mined (sent to newly connecting miners).
	current_job: Arc<RwLock<Option<MiningRequest>>>,
	/// Counter for assigning unique miner IDs.
	next_miner_id: AtomicU64,
}

/// Handle for communicating with a connected miner.
struct MinerHandle {
	/// Channel to send jobs to this miner.
	job_tx: mpsc::Sender<MiningRequest>,
}

impl MinerServer {
	/// Start the QUIC server and listen for miner connections.
	///
	/// This spawns a background task that accepts incoming connections.
	pub async fn start(port: u16) -> Result<Arc<Self>, String> {
		let (result_tx, result_rx) = mpsc::channel::<MiningResult>(64);

		let server = Arc::new(Self {
			miners: Arc::new(RwLock::new(HashMap::new())),
			result_rx: tokio::sync::Mutex::new(result_rx),
			result_tx,
			current_job: Arc::new(RwLock::new(None)),
			next_miner_id: AtomicU64::new(1),
		});

		// Start the acceptor task
		let server_clone = server.clone();
		let endpoint = create_server_endpoint(port).await?;

		tokio::spawn(async move {
			acceptor_task(endpoint, server_clone).await;
		});

		log::info!("⛏️ Miner server listening on port {}", port);

		Ok(server)
	}

	/// Broadcast a job to all connected miners.
	///
	/// This also stores the job so newly connecting miners receive it.
	pub async fn broadcast_job(&self, job: MiningRequest) {
		// Store as current job for new miners
		{
			let mut current = self.current_job.write().await;
			*current = Some(job.clone());
		}

		// Send to all connected miners
		let miners = self.miners.read().await;
		let miner_count = miners.len();

		if miner_count == 0 {
			log::debug!("No miners connected, job queued for when miners connect");
			return;
		}

		log::debug!("Broadcasting job {} to {} miner(s)", job.job_id, miner_count);

		for (id, handle) in miners.iter() {
			if let Err(e) = handle.job_tx.try_send(job.clone()) {
				log::warn!("Failed to send job to miner {}: {}", id, e);
			}
		}
	}

	/// Wait for a mining result with a timeout.
	pub async fn recv_result_timeout(&self, timeout: Duration) -> Option<MiningResult> {
		let mut rx = self.result_rx.lock().await;
		tokio::time::timeout(timeout, rx.recv()).await.ok().flatten()
	}

	/// Add a new miner connection.
	async fn add_miner(&self, job_tx: mpsc::Sender<MiningRequest>) -> u64 {
		let id = self.next_miner_id.fetch_add(1, Ordering::Relaxed);
		let handle = MinerHandle { job_tx };

		self.miners.write().await.insert(id, handle);

		log::info!("⛏️ Miner {} connected (total: {})", id, self.miners.read().await.len());

		id
	}

	/// Remove a miner connection.
	async fn remove_miner(&self, id: u64) {
		self.miners.write().await.remove(&id);
		log::info!("⛏️ Miner {} disconnected (total: {})", id, self.miners.read().await.len());
	}

	/// Get the current job (if any) for newly connecting miners.
	async fn get_current_job(&self) -> Option<MiningRequest> {
		self.current_job.read().await.clone()
	}
}

/// Create a QUIC server endpoint with self-signed certificate.
async fn create_server_endpoint(port: u16) -> Result<quinn::Endpoint, String> {
	// Generate self-signed certificate
	let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
		.map_err(|e| format!("Failed to generate certificate: {}", e))?;

	let cert_der = rustls::pki_types::CertificateDer::from(cert.cert);
	let key_der = rustls::pki_types::PrivateKeyDer::try_from(cert.key_pair.serialize_der())
		.map_err(|e| format!("Failed to serialize private key: {}", e))?;

	let cert_chain = vec![cert_der];

	// Create server config
	let mut server_config = rustls::ServerConfig::builder()
		.with_no_client_auth()
		.with_single_cert(cert_chain, key_der)
		.map_err(|e| format!("Failed to create server config: {}", e))?;

	// Set ALPN protocol
	server_config.alpn_protocols = vec![b"quantus-miner".to_vec()];

	// Wrap in QuicServerConfig for quinn 0.11+
	let quic_server_config = quinn::crypto::rustls::QuicServerConfig::try_from(server_config)
		.map_err(|e| format!("Failed to create QUIC server config: {}", e))?;

	let mut quinn_config = quinn::ServerConfig::with_crypto(Arc::new(quic_server_config));

	// Set transport config
	let mut transport_config = quinn::TransportConfig::default();
	transport_config.keep_alive_interval(Some(Duration::from_secs(10)));
	transport_config.max_idle_timeout(Some(Duration::from_secs(60).try_into().unwrap()));
	quinn_config.transport_config(Arc::new(transport_config));

	// Create endpoint
	let addr = format!("0.0.0.0:{}", port).parse().unwrap();
	let endpoint = quinn::Endpoint::server(quinn_config, addr)
		.map_err(|e| format!("Failed to create server endpoint: {}", e))?;

	Ok(endpoint)
}

/// Background task that accepts incoming miner connections.
async fn acceptor_task(endpoint: quinn::Endpoint, server: Arc<MinerServer>) {
	log::debug!("Acceptor task started");

	while let Some(connecting) = endpoint.accept().await {
		let server = server.clone();

		tokio::spawn(async move {
			match connecting.await {
				Ok(connection) => {
					log::debug!("New QUIC connection from {:?}", connection.remote_address());
					handle_miner_connection(connection, server).await;
				},
				Err(e) => {
					log::warn!("Failed to accept connection: {}", e);
				},
			}
		});
	}

	log::info!("Acceptor task stopped");
}

/// Handle a single miner connection.
async fn handle_miner_connection(connection: quinn::Connection, server: Arc<MinerServer>) {
	let addr = connection.remote_address();
	log::info!("⛏️ New miner connection from {}", addr);
	log::debug!("Waiting for miner {} to open bidirectional stream...", addr);

	// Accept bidirectional stream from miner
	let (send, recv) = match connection.accept_bi().await {
		Ok(streams) => {
			log::info!("⛏️ Stream accepted from miner {}", addr);
			streams
		},
		Err(e) => {
			log::warn!("Failed to accept stream from {}: {}", addr, e);
			return;
		},
	};

	// Create channel for sending jobs to this miner
	let (job_tx, job_rx) = mpsc::channel::<MiningRequest>(16);

	// Register miner
	let miner_id = server.add_miner(job_tx).await;

	// Send current job if there is one
	if let Some(job) = server.get_current_job().await {
		log::debug!("Sending current job {} to newly connected miner {}", job.job_id, miner_id);
		// We'll send it through the connection handler below
	}

	// Handle the connection
	let result = connection_handler(
		miner_id,
		send,
		recv,
		job_rx,
		server.result_tx.clone(),
		server.get_current_job().await,
	)
	.await;

	if let Err(e) = result {
		log::debug!("Miner {} connection ended: {}", miner_id, e);
	}

	// Unregister miner
	server.remove_miner(miner_id).await;
}

/// Maximum number of consecutive results a single connection may drop on a full result
/// channel before it is disconnected as a flooder. A legitimate miner submits at most a
/// couple of messages per job, so its counter is reset by the first successful forward;
/// only a connection that keeps hammering an already-full channel reaches this bound.
const MAX_CONSECUTIVE_RESULT_DROPS: u32 = 8;

/// Forward a miner's result to the shared result channel without ever blocking.
///
/// The result channel is shared by every miner connection and is drained only while the
/// mining loop is actively waiting for results (it pauses during syncing, block template
/// builds and seal imports). A blocking `send` here would let one miner that fills the
/// channel park every other miner's connection handler inside this call; a parked handler
/// also stops servicing its job channel, so the other miners would silently miss new jobs
/// and be unable to submit seals — a one-miner denial of service against the rest.
///
/// Instead, an overflowing result is dropped (losing a result under active flooding is
/// strictly better than stalling every honest connection), and a connection that keeps
/// overflowing is treated as malicious and closed.
async fn forward_result(
	miner_id: u64,
	result_tx: &mpsc::Sender<MiningResult>,
	result: MiningResult,
	consecutive_drops: &mut u32,
) -> Result<(), String> {
	match result_tx.try_send(result) {
		Ok(()) => {
			*consecutive_drops = 0;
			Ok(())
		},
		Err(mpsc::error::TrySendError::Full(result)) => {
			*consecutive_drops += 1;
			log::warn!(
				"⛏️ Result channel full; dropping result from miner {} for job {} ({} consecutive)",
				miner_id,
				result.job_id,
				consecutive_drops
			);
			if *consecutive_drops >= MAX_CONSECUTIVE_RESULT_DROPS {
				Err(format!("Miner {} is flooding the result channel", miner_id))
			} else {
				Ok(())
			}
		},
		Err(mpsc::error::TrySendError::Closed(_)) => Err("Result channel closed".to_string()),
	}
}

/// Handle communication with a single miner.
async fn connection_handler(
	miner_id: u64,
	mut send: quinn::SendStream,
	mut recv: quinn::RecvStream,
	mut job_rx: mpsc::Receiver<MiningRequest>,
	result_tx: mpsc::Sender<MiningResult>,
	initial_job: Option<MiningRequest>,
) -> Result<(), String> {
	let mut consecutive_drops = 0u32;
	// Wait for Ready message from miner (required to establish the stream)
	log::debug!("Waiting for Ready message from miner {}...", miner_id);
	match read_message(&mut recv).await {
		Ok(MinerMessage::Ready) => {
			log::debug!("Received Ready from miner {}", miner_id);
		},
		Ok(other) => {
			log::warn!("Expected Ready from miner {}, got {:?}", miner_id, other);
			return Err("Protocol error: expected Ready message".to_string());
		},
		Err(e) => {
			return Err(format!("Failed to read Ready message: {}", e));
		},
	}

	// Send initial job if there is one
	if let Some(job) = initial_job {
		log::debug!("Sending initial job {} to miner {}", job.job_id, miner_id);
		let msg = MinerMessage::NewJob(job);
		write_message(&mut send, &msg)
			.await
			.map_err(|e| format!("Failed to send initial job: {}", e))?;
	}

	loop {
		tokio::select! {
			// Prioritize reading to detect disconnection faster
			biased;

			// Receive results from miner
			msg_result = read_message(&mut recv) => {
				match msg_result {
					Ok(MinerMessage::JobResult(mut result)) => {
						log::info!(
							"⛏️ Received result from miner {}: job_id={}, status={:?}",
							miner_id,
							result.job_id,
							result.status
						);
					// Tag the result with the miner ID
					result.miner_id = Some(miner_id);
					forward_result(miner_id, &result_tx, result, &mut consecutive_drops)
						.await?;
					}
					Ok(MinerMessage::Ready) => {
						log::debug!("Ignoring duplicate Ready from miner {}", miner_id);
					}
					Ok(MinerMessage::NewJob(_)) => {
						log::warn!("Received unexpected NewJob from miner {}", miner_id);
					}
					Err(e) => {
						if e.kind() == std::io::ErrorKind::UnexpectedEof {
							return Err("Miner disconnected".to_string());
						}
						return Err(format!("Read error: {}", e));
					}
				}
			}

			// Send jobs to miner
			job = job_rx.recv() => {
				match job {
					Some(job) => {
						log::debug!("Sending job {} to miner {}", job.job_id, miner_id);
						let msg = MinerMessage::NewJob(job);
						if let Err(e) = write_message(&mut send, &msg).await {
							return Err(format!("Failed to send job: {}", e));
						}
					}
					None => {
						// Channel closed, shut down
						return Ok(());
					}
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use quantus_miner_api::ApiResponseStatus;

	fn dummy_result(job_id: &str) -> MiningResult {
		MiningResult {
			status: ApiResponseStatus::Completed,
			job_id: job_id.to_string(),
			nonce: None,
			work: None,
			hash_count: 0,
			elapsed_time: 0.0,
			miner_id: Some(1),
		}
	}

	/// The result channel is shared by every miner connection and is drained only while
	/// the mining loop is actively waiting for results. Forwarding must therefore never
	/// block on a full channel: a handler parked inside the send also stops servicing its
	/// job channel, so one miner flooding the shared channel would silently cut every
	/// other miner off from new jobs and result submission.
	#[tokio::test]
	async fn forwarding_a_result_never_blocks_on_a_full_channel() {
		let (tx, _rx) = mpsc::channel::<MiningResult>(64);
		for _ in 0..64 {
			tx.try_send(dummy_result("flood")).unwrap();
		}

		let mut drops = 0;
		let outcome = tokio::time::timeout(
			Duration::from_millis(200),
			forward_result(2, &tx, dummy_result("honest"), &mut drops),
		)
		.await;

		assert!(
			outcome.is_ok(),
			"forward_result must complete promptly (dropping the result) while the \
			 shared channel is full, not park the connection handler"
		);
	}

	/// A connection that keeps overflowing the already-full shared channel is flooding —
	/// a legitimate miner submits at most a couple of messages per job — and must be
	/// disconnected so the channel can drain for the honest miners.
	#[tokio::test]
	async fn sustained_overflow_disconnects_the_miner() {
		let (tx, _rx) = mpsc::channel::<MiningResult>(1);
		tx.try_send(dummy_result("flood")).unwrap();

		let mut drops = 0;
		for attempt in 1..MAX_CONSECUTIVE_RESULT_DROPS {
			let res = forward_result(2, &tx, dummy_result("flood"), &mut drops).await;
			assert!(res.is_ok(), "drop {attempt} is within tolerance and must not disconnect");
		}
		let res = forward_result(2, &tx, dummy_result("flood"), &mut drops).await;
		assert!(res.is_err(), "sustained overflow must disconnect the flooding miner");
	}

	/// A successful forward proves the connection is not hammering a full channel, so the
	/// drop counter must reset: an honest miner that occasionally loses a result to
	/// someone else's flood must never accumulate its way to a disconnect.
	#[tokio::test]
	async fn successful_forward_resets_the_drop_counter() {
		let (tx, mut rx) = mpsc::channel::<MiningResult>(1);

		let mut drops = 0;
		for _ in 0..2 * MAX_CONSECUTIVE_RESULT_DROPS {
			// Fill the channel, overflow once (one drop), then drain and forward
			// successfully (counter resets).
			tx.try_send(dummy_result("filler")).unwrap();
			let res = forward_result(2, &tx, dummy_result("overflow"), &mut drops).await;
			assert!(res.is_ok(), "isolated drops must never disconnect an honest miner");
			rx.recv().await.unwrap();
			let res = forward_result(2, &tx, dummy_result("honest"), &mut drops).await;
			assert!(res.is_ok());
			rx.recv().await.unwrap();
		}
		assert_eq!(drops, 0, "counter must be reset by the last successful forward");
	}
}
