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
//!
//! # Certificate Security
//!
//! The server uses post-quantum cryptography for TLS:
//! - Certificate: ML-DSA-87 (NIST FIPS 204 standardized Dilithium)
//! - Key exchange: X25519MLKEM768 (hybrid classical + post-quantum)
//!
//! Note: Due to rcgen 0.14 limitations, the certificate fingerprint changes on
//! each restart. This will be fixed when rcgen adds ML-DSA key persistence support.

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
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
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
	///
	/// # Arguments
	/// * `port` - The port to listen on for miner connections
	///
	/// # Note
	/// Due to rcgen 0.14 limitations, the certificate fingerprint changes on each restart.
	/// This will be fixed when rcgen adds ML-DSA key persistence support.
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

		log::info!("Miner server listening on port {port}");

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

/// Create a QUIC server endpoint with self-signed ML-DSA (post-quantum) certificate.
///
/// Uses post-quantum cryptography:
/// - Certificate: ML-DSA-87 (NIST FIPS 204)
/// - Key exchange: X25519MLKEM768 (hybrid classical + post-quantum)
///
/// Note: Due to rcgen 0.14 limitations, ML-DSA keys cannot be persisted and reloaded.
/// The certificate fingerprint will change on each restart until this is fixed upstream.
async fn create_server_endpoint(port: u16) -> Result<quinn::Endpoint, String> {
	// Generate ML-DSA-87 key pair
	// TODO: Implement deterministic key derivation when rcgen adds ML-DSA key persistence support
	let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ML_DSA_87)
		.map_err(|e| format!("Failed to generate ML-DSA key pair: {e}"))?;

	let cert_params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
		.map_err(|e| format!("Failed to create certificate params: {e}"))?;

	let cert = cert_params
		.self_signed(&key_pair)
		.map_err(|e| format!("Failed to generate certificate: {e}"))?;

	let cert_der = cert.der().clone();
	let key_der = key_pair.serialize_der();

	// Log certificate fingerprint for miners to use with --node-cert-fingerprint
	let fingerprint = compute_cert_fingerprint(&cert_der);
	log::info!("⛏️ Miner server certificate fingerprint: {}", fingerprint);
	log::info!("⛏️ Certificate algorithm: ML-DSA-87 (post-quantum)");
	log::info!("⛏️ Key exchange: X25519MLKEM768 (hybrid post-quantum)");
	log::warn!("⛏️ NOTE: Fingerprint changes on restart (rcgen ML-DSA key persistence bug)");
	log::info!("⛏️ Miners should use: --node-cert-fingerprint {}", fingerprint);

	let cert_chain = vec![cert_der];
	let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));

	// Create server config with post-quantum crypto provider (aws-lc-rs with ML-DSA support)
	let mut server_config =
		rustls::ServerConfig::builder_with_provider(Arc::new(rustls_post_quantum::provider()))
			.with_safe_default_protocol_versions()
			.map_err(|e| format!("Failed to set protocol versions: {e}"))?
			.with_no_client_auth()
			.with_single_cert(cert_chain, key)
			.map_err(|e| format!("Failed to create server config: {e}"))?;

	// Set ALPN protocol (required by QUIC RFC 9001)
	server_config.alpn_protocols = vec![b"quantus-miner".to_vec()];

	let mut quinn_config = quinn::ServerConfig::with_crypto(Arc::new(
		quinn::crypto::rustls::QuicServerConfig::try_from(server_config)
			.map_err(|e| format!("Failed to create QUIC server config: {e}"))?,
	));

	// Set transport config
	let mut transport_config = quinn::TransportConfig::default();
	transport_config.keep_alive_interval(Some(Duration::from_secs(10)));
	transport_config.max_idle_timeout(Some(Duration::from_secs(60).try_into().unwrap()));
	quinn_config.transport_config(Arc::new(transport_config));

	// Create endpoint
	let addr = format!("0.0.0.0:{port}").parse().unwrap();
	let endpoint = quinn::Endpoint::server(quinn_config, addr)
		.map_err(|e| format!("Failed to create server endpoint: {e}"))?;

	Ok(endpoint)
}

/// Compute SHA-256 fingerprint of a certificate in the format `sha256:<hex>`.
fn compute_cert_fingerprint(cert_der: &CertificateDer) -> String {
	let mut hasher = Sha256::new();
	hasher.update(cert_der.as_ref());
	let hash = hasher.finalize();
	format!("sha256:{}", hex::encode(hash))
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

/// Handle communication with a single miner.
async fn connection_handler(
	miner_id: u64,
	mut send: quinn::SendStream,
	mut recv: quinn::RecvStream,
	mut job_rx: mpsc::Receiver<MiningRequest>,
	result_tx: mpsc::Sender<MiningResult>,
	initial_job: Option<MiningRequest>,
) -> Result<(), String> {
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
						if result_tx.send(result).await.is_err() {
							return Err("Result channel closed".to_string());
						}
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
