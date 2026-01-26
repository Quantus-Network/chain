//! QUIC client for communicating with external miners.
//!
//! This module provides a persistent QUIC connection to an external miner service,
//! enabling bidirectional streaming for mining job submission and result delivery.
//!
//! # Protocol
//!
//! - Node sends `MinerMessage::NewJob` to submit a mining job (implicitly cancels any previous)
//! - Miner sends `MinerMessage::JobResult` when mining completes
//!
//! # Connection Management
//!
//! The client maintains a persistent connection and automatically reconnects on failure.

use std::{net::SocketAddr, sync::Arc};

use jsonrpsee::tokio;
use quantus_miner_api::{read_message, write_message, MinerMessage, MiningRequest, MiningResult};
use rustls::client::ServerCertVerified;
use sp_core::{H256, U512};
use tokio::sync::{mpsc, Mutex};

/// A QUIC client for communicating with an external miner.
///
/// This client maintains a persistent connection and provides methods to send
/// mining jobs and receive results asynchronously.
pub struct QuicMinerClient {
	/// The address of the miner to connect to.
	addr: SocketAddr,
	/// Channel to send commands to the connection handler task.
	command_tx: mpsc::Sender<MinerCommand>,
	/// Channel to receive mining results.
	result_rx: Mutex<mpsc::Receiver<MiningResult>>,
}

/// Commands sent to the connection handler task.
enum MinerCommand {
	SendJob(MiningRequest),
	Shutdown,
}

impl QuicMinerClient {
	/// Create a new QUIC miner client and connect to the miner.
	///
	/// This spawns a background task that maintains the connection and handles
	/// sending jobs and receiving results.
	pub async fn connect(addr: SocketAddr) -> Result<Self, String> {
		let (command_tx, command_rx) = mpsc::channel::<MinerCommand>(16);
		let (result_tx, result_rx) = mpsc::channel::<MiningResult>(16);

		// Spawn the connection handler task
		let addr_clone = addr;
		tokio::spawn(async move {
			connection_handler(addr_clone, command_rx, result_tx).await;
		});

		log::info!("QUIC miner client created for {}", addr);

		Ok(Self { addr, command_tx, result_rx: Mutex::new(result_rx) })
	}

	/// Send a mining job to the miner.
	///
	/// This sends a `NewJob` message which implicitly cancels any previous job.
	pub async fn send_job(
		&self,
		job_id: &str,
		mining_hash: &H256,
		distance_threshold: U512,
		nonce_start: U512,
		nonce_end: U512,
	) -> Result<(), String> {
		let request = MiningRequest {
			job_id: job_id.to_string(),
			mining_hash: hex::encode(mining_hash.as_bytes()),
			distance_threshold: distance_threshold.to_string(),
			nonce_start: format!("{:0128x}", nonce_start),
			nonce_end: format!("{:0128x}", nonce_end),
		};

		self.command_tx
			.send(MinerCommand::SendJob(request))
			.await
			.map_err(|e| format!("Failed to send job command: {}", e))?;

		Ok(())
	}

	/// Try to receive a mining result without blocking.
	///
	/// Returns `Some(result)` if a result is available, `None` otherwise.
	pub async fn try_recv_result(&self) -> Option<MiningResult> {
		let mut rx = self.result_rx.lock().await;
		rx.try_recv().ok()
	}

	/// Wait for a mining result with a timeout.
	///
	/// Returns the result if one is received within the timeout, or `None` if the timeout expires.
	pub async fn recv_result_timeout(&self, timeout: std::time::Duration) -> Option<MiningResult> {
		let mut rx = self.result_rx.lock().await;
		tokio::time::timeout(timeout, rx.recv()).await.ok().flatten()
	}

	/// Get the address of the miner this client is connected to.
	pub fn addr(&self) -> SocketAddr {
		self.addr
	}
}

impl Drop for QuicMinerClient {
	fn drop(&mut self) {
		// Try to send shutdown command (non-blocking)
		let _ = self.command_tx.try_send(MinerCommand::Shutdown);
	}
}

/// Background task that maintains the QUIC connection and handles messages.
async fn connection_handler(
	addr: SocketAddr,
	mut command_rx: mpsc::Receiver<MinerCommand>,
	result_tx: mpsc::Sender<MiningResult>,
) {
	let mut reconnect_delay = std::time::Duration::from_secs(1);
	const MAX_RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(30);

	loop {
		log::info!("Connecting to miner at {}...", addr);

		match establish_connection(addr).await {
			Ok((connection, send, recv)) => {
				log::info!("Connected to miner at {}", addr);
				reconnect_delay = std::time::Duration::from_secs(1); // Reset delay on success

				// Handle the connection until it fails
				if let Err(e) =
					handle_connection(connection, send, recv, &mut command_rx, &result_tx).await
				{
					log::warn!("Connection to miner lost: {}", e);
				}
			},
			Err(e) => {
				log::warn!("Failed to connect to miner at {}: {}", addr, e);
			},
		}

		// Check for shutdown command before reconnecting
		match command_rx.try_recv() {
			Ok(MinerCommand::Shutdown) => {
				log::info!("Miner client shutting down");
				return;
			},
			_ => {},
		}

		log::info!("Reconnecting to miner in {:?}...", reconnect_delay);
		tokio::time::sleep(reconnect_delay).await;

		// Exponential backoff
		reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
	}
}

/// Establish a QUIC connection to the miner.
async fn establish_connection(
	addr: SocketAddr,
) -> Result<(quinn::Connection, quinn::SendStream, quinn::RecvStream), String> {
	// Create client config with insecure certificate verification
	let mut crypto = rustls::ClientConfig::builder()
		.with_safe_defaults()
		.with_custom_certificate_verifier(Arc::new(InsecureCertVerifier))
		.with_no_client_auth();

	// Set ALPN protocol to match the miner server
	crypto.alpn_protocols = vec![b"quantus-miner".to_vec()];

	let mut client_config = quinn::ClientConfig::new(Arc::new(crypto));

	// Set transport config
	// - Keep-alive pings every 10 seconds to prevent idle timeout
	// - Max idle timeout of 60 seconds to handle gaps between mining jobs
	let mut transport_config = quinn::TransportConfig::default();
	transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(10)));
	transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(60).try_into().unwrap()));
	client_config.transport_config(Arc::new(transport_config));

	// Create endpoint
	let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
		.map_err(|e| format!("Failed to create QUIC endpoint: {}", e))?;
	endpoint.set_default_client_config(client_config);

	// Connect to the miner
	let connection = endpoint
		.connect(addr, "localhost")
		.map_err(|e| format!("Failed to initiate connection: {}", e))?
		.await
		.map_err(|e| format!("Failed to establish connection: {}", e))?;

	// Open a bidirectional stream
	let (send, recv) = connection
		.open_bi()
		.await
		.map_err(|e| format!("Failed to open stream: {}", e))?;

	Ok((connection, send, recv))
}

/// Handle an established connection, processing commands and receiving results.
async fn handle_connection(
	_connection: quinn::Connection,
	mut send: quinn::SendStream,
	mut recv: quinn::RecvStream,
	command_rx: &mut mpsc::Receiver<MinerCommand>,
	result_tx: &mpsc::Sender<MiningResult>,
) -> Result<(), String> {
	loop {
		tokio::select! {
			// Handle commands from the main task
			cmd = command_rx.recv() => {
				match cmd {
					Some(MinerCommand::SendJob(request)) => {
						log::debug!("Sending NewJob to miner: job_id={}", request.job_id);
						let msg = MinerMessage::NewJob(request);
						write_message(&mut send, &msg)
							.await
							.map_err(|e| format!("Failed to send message: {}", e))?;
					}
					Some(MinerCommand::Shutdown) => {
						log::info!("Connection handler shutting down");
						return Ok(());
					}
					None => {
						// Command channel closed, shut down
						return Ok(());
					}
				}
			}

			// Handle incoming messages from the miner
			msg_result = read_message(&mut recv) => {
				match msg_result {
					Ok(MinerMessage::JobResult(result)) => {
						log::info!(
							"Received JobResult from miner: job_id={}, status={:?}",
							result.job_id,
							result.status
						);
						if result_tx.send(result).await.is_err() {
							log::warn!("Failed to forward result (receiver dropped)");
							return Ok(());
						}
					}
					Ok(MinerMessage::NewJob(_)) => {
						// Miner should not send NewJob to node
						log::warn!("Received unexpected NewJob from miner, ignoring");
					}
					Err(e) => {
						if e.kind() == std::io::ErrorKind::UnexpectedEof {
							return Err("Miner disconnected".to_string());
						}
						return Err(format!("Failed to read message: {}", e));
					}
				}
			}
		}
	}
}

/// A certificate verifier that accepts any certificate.
///
/// This is used because the miner uses a self-signed certificate.
/// In production, you might want to use certificate pinning instead.
struct InsecureCertVerifier;

impl rustls::client::ServerCertVerifier for InsecureCertVerifier {
	fn verify_server_cert(
		&self,
		_end_entity: &rustls::Certificate,
		_intermediates: &[rustls::Certificate],
		_server_name: &rustls::ServerName,
		_scts: &mut dyn Iterator<Item = &[u8]>,
		_ocsp_response: &[u8],
		_now: std::time::SystemTime,
	) -> Result<ServerCertVerified, rustls::Error> {
		// Accept any certificate
		Ok(ServerCertVerified::assertion())
	}
}
