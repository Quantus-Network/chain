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
	fs,
	io::Write,
	path::{Path, PathBuf},
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	time::Duration,
};

use jsonrpsee::tokio;
use quantus_miner_api::{
	read_message, write_message, MinerMessage, MiningRequest, MiningResult, MAX_AUTH_TOKEN_LEN,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, RwLock, Semaphore};

/// Default filename for the miner auth token under the chain config directory.
pub const DEFAULT_MINER_AUTH_TOKEN_FILENAME: &str = "miner-auth-token";

/// Default TLS material filenames under the chain config directory.
pub const DEFAULT_MINER_TLS_CERT_FILENAME: &str = "miner-tls-cert.der";
pub const DEFAULT_MINER_TLS_KEY_FILENAME: &str = "miner-tls-key.der";
pub const DEFAULT_MINER_TLS_CERT_SHA256_FILENAME: &str = "miner-tls-cert-sha256";

/// ALPN protocol id, versioned with the wire protocol: bump it on breaking
/// message changes so an incompatible miner fails cleanly at the TLS handshake
/// ("no application protocol") instead of an opaque auth/deserialize error
/// indistinguishable from a wrong token. `/2` = authenticated `Ready { token }`.
pub const MINER_ALPN: &[u8] = b"quantus-miner/2";

/// Max time for stream accept + `Ready` auth before the connection is dropped.
const AUTH_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Cap on connections that have not yet completed auth (prevents pre-auth task/memory DoS).
const MAX_UNAUTHENTICATED_CONNECTIONS: usize = 32;

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
	/// Shared secret required in the miner's `Ready` message.
	auth_token: Arc<str>,
	/// Limits concurrent pre-auth handshakes.
	unauth_slots: Arc<Semaphore>,
}

/// Handle for communicating with a connected miner.
struct MinerHandle {
	/// Channel to send jobs to this miner.
	job_tx: mpsc::Sender<MiningRequest>,
}

impl MinerServer {
	/// Start the QUIC server and listen for miner connections.
	///
	/// Loads (or generates) the auth token and TLS certificate material before
	/// binding. This spawns a background task that accepts incoming connections.
	pub async fn start(
		port: u16,
		auth_token_path: PathBuf,
		tls_dir: PathBuf,
	) -> Result<Arc<Self>, String> {
		let auth_token = load_or_create_miner_auth_token(&auth_token_path)?;
		log::info!(
			"⛏️ Miner auth token file: {} (read this file to configure your miner; token is not logged)",
			auth_token_path.display()
		);

		let tls = load_or_create_miner_tls(&tls_dir)?;
		log::info!(
			"⛏️ Miner TLS cert SHA-256 file: {} (pin this on your miner)",
			tls.fingerprint_path.display()
		);
		log::info!("⛏️ Miner TLS cert SHA-256: {}", tls.fingerprint_hex);

		let (result_tx, result_rx) = mpsc::channel::<MiningResult>(64);

		let server = Arc::new(Self {
			miners: Arc::new(RwLock::new(HashMap::new())),
			result_rx: tokio::sync::Mutex::new(result_rx),
			result_tx,
			current_job: Arc::new(RwLock::new(None)),
			next_miner_id: AtomicU64::new(1),
			auth_token: Arc::from(auth_token),
			unauth_slots: Arc::new(Semaphore::new(MAX_UNAUTHENTICATED_CONNECTIONS)),
		});

		// Start the acceptor task
		let server_clone = server.clone();
		let endpoint = create_server_endpoint(port, &tls).await?;

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

/// Load the miner auth token from `path`, or generate and persist one if missing.
///
/// The token is a 32-byte value encoded as 64 lowercase hex characters.
pub fn load_or_create_miner_auth_token(path: &Path) -> Result<String, String> {
	match fs::read_to_string(path) {
		Ok(contents) => {
			let token = contents.trim().to_string();
			if token.is_empty() {
				return Err(format!(
					"Miner auth token file {} is empty; delete it to regenerate or write a token",
					path.display()
				));
			}
			ensure_secret_file_permissions(path)?;
			validate_auth_token_length(&token, path)?;
			Ok(token)
		},
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
			let mut bytes = [0u8; 32];
			rand::thread_rng().fill_bytes(&mut bytes);
			let token = hex::encode(bytes);

			if let Some(parent) = path.parent() {
				fs::create_dir_all(parent).map_err(|e| {
					format!(
						"Failed to create miner auth token directory {}: {}",
						parent.display(),
						e
					)
				})?;
			}

			write_miner_auth_token_file(path, &token)?;
			log::info!("⛏️ Generated new miner auth token at {}", path.display());
			Ok(token)
		},
		Err(e) => Err(format!("Failed to read miner auth token file {}: {}", path.display(), e)),
	}
}

fn validate_auth_token_length(token: &str, path: &Path) -> Result<(), String> {
	if token.len() > MAX_AUTH_TOKEN_LEN {
		return Err(format!(
			"Miner auth token in {} is {} bytes; max is {} so Ready frames fit under the protocol message limit",
			path.display(),
			token.len(),
			MAX_AUTH_TOKEN_LEN
		));
	}
	// The wire frame is JSON, so escaping can inflate the token well past its
	// raw length (e.g. a token full of quotes doubles). Check what miners will
	// actually send, otherwise every miner fails auth with an opaque
	// "Message size exceeds maximum" framing error.
	let frame = serde_json::to_vec(&MinerMessage::Ready { token: token.to_string() })
		.map_err(|e| format!("Failed to serialize miner auth token from {}: {}", path.display(), e))?;
	if frame.len() > quantus_miner_api::MAX_MESSAGE_SIZE as usize {
		return Err(format!(
			"Miner auth token in {} serializes to a {}-byte Ready frame; max is {}. \
			 Shorten the token or avoid characters that require JSON escaping.",
			path.display(),
			frame.len(),
			quantus_miner_api::MAX_MESSAGE_SIZE
		));
	}
	Ok(())
}

/// Constant-time equality for auth tokens (after trimming the wire value).
fn auth_tokens_equal(wire_token: &str, expected: &str) -> bool {
	let a = wire_token.trim().as_bytes();
	let b = expected.as_bytes();
	if a.len() != b.len() {
		return false;
	}
	let mut diff = 0u8;
	for (x, y) in a.iter().zip(b.iter()) {
		diff |= x ^ y;
	}
	diff == 0
}

fn write_miner_auth_token_file(path: &Path, token: &str) -> Result<(), String> {
	write_secret_text_file(path, token)
}

/// Persisted miner TLS certificate material.
struct MinerTlsMaterial {
	cert_der: Vec<u8>,
	key_der: Vec<u8>,
	fingerprint_hex: String,
	fingerprint_path: PathBuf,
}

/// Load miner TLS cert/key from `tls_dir`, or generate and persist them if missing.
///
/// Also writes `miner-tls-cert-sha256` (SHA-256 of the cert DER as lowercase hex)
/// for miners to pin — but only after the cert/key pair validates as a rustls
/// server config, so a corrupt cert cannot overwrite an already-distributed pin.
pub fn load_or_create_miner_tls(tls_dir: &Path) -> Result<MinerTlsMaterial, String> {
	fs::create_dir_all(tls_dir).map_err(|e| {
		format!("Failed to create miner TLS directory {}: {}", tls_dir.display(), e)
	})?;

	let cert_path = tls_dir.join(DEFAULT_MINER_TLS_CERT_FILENAME);
	let key_path = tls_dir.join(DEFAULT_MINER_TLS_KEY_FILENAME);
	let fingerprint_path = tls_dir.join(DEFAULT_MINER_TLS_CERT_SHA256_FILENAME);

	let cert_exists = cert_path.exists();
	let key_exists = key_path.exists();
	if cert_exists != key_exists {
		return Err(format!(
			"Incomplete miner TLS material in {}: found cert={}, key={}. \
			 Delete both `{}` and `{}` to regenerate.",
			tls_dir.display(),
			cert_exists,
			key_exists,
			DEFAULT_MINER_TLS_CERT_FILENAME,
			DEFAULT_MINER_TLS_KEY_FILENAME
		));
	}

	let (cert_der, key_der) = if cert_exists {
		let cert_der = fs::read(&cert_path)
			.map_err(|e| format!("Failed to read miner TLS cert {}: {}", cert_path.display(), e))?;
		let key_der = fs::read(&key_path)
			.map_err(|e| format!("Failed to read miner TLS key {}: {}", key_path.display(), e))?;
		if cert_der.is_empty() || key_der.is_empty() {
			return Err(format!(
				"Miner TLS cert/key in {} is empty; delete them to regenerate",
				tls_dir.display()
			));
		}
		// The private key is as secret as the auth token; repair over-permissive
		// modes on keys restored from backup or copied in by hand.
		ensure_secret_file_permissions(&key_path)?;
		(cert_der, key_der)
	} else {
		let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
			.map_err(|e| format!("Failed to generate miner TLS certificate: {}", e))?;
		let cert_der = certified.cert.der().as_ref().to_vec();
		let key_der = certified.key_pair.serialize_der();
		// Validate before persisting so we never leave half-written or unusable material.
		validate_miner_tls(&cert_der, &key_der)?;
		persist_miner_tls_pair(&cert_path, &key_path, &cert_der, &key_der)?;
		log::info!(
			"⛏️ Generated new miner TLS certificate at {} / {}",
			cert_path.display(),
			key_path.display()
		);
		(cert_der, key_der)
	};

	// Existing material must also validate before we (re)publish the fingerprint.
	validate_miner_tls(&cert_der, &key_der)?;

	// Only touch the fingerprint file when it is missing or wrong: rewriting it
	// every boot re-runs the temp-file + fsync + rename dance for no reason and
	// creates a crash window on a file operators have already distributed.
	let fingerprint_hex = hex::encode(Sha256::digest(&cert_der));
	let needs_write = match fs::read_to_string(&fingerprint_path) {
		Ok(existing) => {
			let matches = existing.trim().eq_ignore_ascii_case(&fingerprint_hex);
			if !matches {
				log::warn!(
					"⛏️ Miner TLS fingerprint file {} did not match cert; rewriting",
					fingerprint_path.display()
				);
			}
			!matches
		},
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
		Err(e) => {
			log::warn!(
				"⛏️ Could not read miner TLS fingerprint file {}: {}; rewriting",
				fingerprint_path.display(),
				e
			);
			true
		},
	};
	if needs_write {
		// Fingerprint is public pin data — world-readable is fine (and helps remote miners).
		atomic_write_text_file(&fingerprint_path, &fingerprint_hex, 0o644)?;
		log::info!("⛏️ Wrote miner TLS cert SHA-256 to {}", fingerprint_path.display());
	}

	Ok(MinerTlsMaterial { cert_der, key_der, fingerprint_hex, fingerprint_path })
}

fn validate_miner_tls(cert_der: &[u8], key_der: &[u8]) -> Result<(), String> {
	let cert = rustls::pki_types::CertificateDer::from(cert_der.to_vec());
	let key = rustls::pki_types::PrivateKeyDer::try_from(key_der.to_vec())
		.map_err(|e| format!("Failed to parse miner TLS private key: {}", e))?;
	let server_config = rustls::ServerConfig::builder()
		.with_no_client_auth()
		.with_single_cert(vec![cert], key)
		.map_err(|e| format!("Failed to create miner TLS server config: {}", e))?;
	quinn::crypto::rustls::QuicServerConfig::try_from(server_config)
		.map_err(|e| format!("Failed to create miner QUIC TLS config: {}", e))?;
	Ok(())
}

/// Persist cert then key with atomic renames. If the key write fails after the cert
/// is installed, remove the cert so the next start can regenerate a complete pair.
fn persist_miner_tls_pair(
	cert_path: &Path,
	key_path: &Path,
	cert_der: &[u8],
	key_der: &[u8],
) -> Result<(), String> {
	atomic_write_bytes_file(cert_path, cert_der, 0o600)?;
	if let Err(e) = atomic_write_bytes_file(key_path, key_der, 0o600) {
		let _ = fs::remove_file(cert_path);
		return Err(e);
	}
	Ok(())
}

fn write_secret_text_file(path: &Path, contents: &str) -> Result<(), String> {
	atomic_write_text_file(path, contents, 0o600)
}

/// Warn and repair overly-permissive modes on existing secret files (Unix).
///
/// Newly created files are written as 0600, but tokens restored from backup or
/// copied in by hand may be 0644; keep the CLI "mode 0600" claim honest.
fn ensure_secret_file_permissions(path: &Path) -> Result<(), String> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let meta =
			fs::metadata(path).map_err(|e| format!("Failed to stat {}: {}", path.display(), e))?;
		let mode = meta.permissions().mode() & 0o777;
		if mode & 0o077 != 0 {
			log::warn!(
				"⛏️ Miner secret file {} has mode {:04o}; repairing to 0600",
				path.display(),
				mode
			);
			let mut perms = meta.permissions();
			perms.set_mode(0o600);
			fs::set_permissions(path, perms).map_err(|e| {
				format!("Failed to repair permissions on {}: {}", path.display(), e)
			})?;
		}
	}
	#[cfg(not(unix))]
	{
		let _ = path;
	}
	Ok(())
}

fn atomic_write_text_file(path: &Path, contents: &str, mode: u32) -> Result<(), String> {
	let mut payload = contents.as_bytes().to_vec();
	payload.push(b'\n');
	atomic_write_bytes_file(path, &payload, mode)
}

fn atomic_write_bytes_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	// The suffix must be unique across restarts, not just across processes: a
	// stale temp file left by a hard kill (SIGKILL/OOM/power loss) would make
	// `create_new` fail forever if the name were deterministic — under Docker
	// the node is PID 1 on every restart, so a PID-only suffix collides.
	let tmp_name = format!(
		".{}.{}.{:016x}.tmp",
		path.file_name().and_then(|s| s.to_str()).unwrap_or("secret"),
		std::process::id(),
		rand::thread_rng().next_u64()
	);
	let tmp_path = parent.join(tmp_name);

	{
		let mut file = open_file_with_mode(&tmp_path, mode)?;
		file.write_all(contents).and_then(|_| file.sync_all()).map_err(|e| {
			let _ = fs::remove_file(&tmp_path);
			format!("Failed to write {}: {}", path.display(), e)
		})?;
	}

	fs::rename(&tmp_path, path).map_err(|e| {
		let _ = fs::remove_file(&tmp_path);
		format!("Failed to finalize {}: {}", path.display(), e)
	})?;

	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let perms = fs::Permissions::from_mode(mode);
		fs::set_permissions(path, perms)
			.map_err(|e| format!("Failed to set permissions on {}: {}", path.display(), e))?;
	}

	Ok(())
}

fn open_file_with_mode(path: &Path, mode: u32) -> Result<fs::File, String> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.mode(mode)
			.open(path)
			.map_err(|e| format!("Failed to create {}: {}", path.display(), e))
	}
	#[cfg(not(unix))]
	{
		let _ = mode;
		log::warn!(
			"⛏️ Writing {} without Unix mode bits; ensure the file is not world-readable",
			path.display()
		);
		fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(path)
			.map_err(|e| format!("Failed to create {}: {}", path.display(), e))
	}
}

/// Create a QUIC server endpoint with the persisted (or newly generated) certificate.
async fn create_server_endpoint(
	port: u16,
	tls: &MinerTlsMaterial,
) -> Result<quinn::Endpoint, String> {
	let cert_der = rustls::pki_types::CertificateDer::from(tls.cert_der.clone());
	let key_der = rustls::pki_types::PrivateKeyDer::try_from(tls.key_der.clone())
		.map_err(|e| format!("Failed to parse miner TLS private key: {}", e))?;

	let mut server_config = rustls::ServerConfig::builder()
		.with_no_client_auth()
		.with_single_cert(vec![cert_der], key_der)
		.map_err(|e| format!("Failed to create server config: {}", e))?;

	// Set ALPN protocol
	server_config.alpn_protocols = vec![MINER_ALPN.to_vec()];

	// Wrap in QuicServerConfig for quinn 0.11+
	let quic_server_config = quinn::crypto::rustls::QuicServerConfig::try_from(server_config)
		.map_err(|e| format!("Failed to create QUIC server config: {}", e))?;

	let mut quinn_config = quinn::ServerConfig::with_crypto(Arc::new(quic_server_config));

	// Bound how many incomplete handshakes quinn will buffer for the app.
	quinn_config.max_incoming(MAX_UNAUTHENTICATED_CONNECTIONS);

	// Set transport config: one bi-stream (the protocol), no uni-streams, and no
	// server keep-alives so max_idle_timeout can reclaim peers that connect and stall
	// before/during auth. Job traffic is event-driven and can go quiet for minutes,
	// so authenticated miners MUST send client keep-alives (documented in MINING.md;
	// the bundled quantus-miner sends them every 5s) or they hit the idle timeout.
	let mut transport_config = quinn::TransportConfig::default();
	transport_config.max_concurrent_bidi_streams(1u32.into());
	transport_config.max_concurrent_uni_streams(0u32.into());
	transport_config.keep_alive_interval(None);
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

		// Refuse early when the pre-auth budget is exhausted so we do not spawn
		// unbounded tasks for silent connect-and-stall peers.
		let Ok(unauth_permit) = server.unauth_slots.clone().try_acquire_owned() else {
			log::warn!(
				"⛏️ Rejecting miner connection: {} unauthenticated handshakes already in flight",
				MAX_UNAUTHENTICATED_CONNECTIONS
			);
			connecting.refuse();
			continue;
		};
		tokio::spawn(async move {
			// The QUIC/TLS handshake must be bounded too: a peer that sends an
			// Initial and then stalls would otherwise pin this pre-auth permit
			// until max_idle_timeout (60s), letting ~32 cheap half-open
			// handshakes lock every legitimate miner out. Dropping the
			// `Connecting` future on timeout abandons the handshake.
			let remote = connecting.remote_address();
			match tokio::time::timeout(AUTH_HANDSHAKE_TIMEOUT, connecting).await {
				Ok(Ok(connection)) => {
					log::debug!("New QUIC connection from {:?}", connection.remote_address());
					match authenticate_miner_connection(&connection, &server).await {
						Some((send, recv)) => {
							// Auth succeeded — free the pre-auth slot before the
							// long-lived miner session so authenticated miners do
							// not consume the unauth budget.
							drop(unauth_permit);
							serve_authenticated_miner(connection, server, send, recv).await;
							return;
						},
						None => {
							// Rejection/timeout already logged + connection closed.
						},
					}
				},
				Ok(Err(e)) => {
					log::warn!("Failed to accept connection: {}", e);
				},
				Err(_) => {
					log::warn!(
						"⛏️ Rejected miner {}: QUIC handshake timed out after {:?}",
						remote,
						AUTH_HANDSHAKE_TIMEOUT
					);
				},
			}
			drop(unauth_permit);
		});
	}

	log::info!("Acceptor task stopped");
}

/// Run the stream accept + `Ready` auth handshake under a timeout.
///
/// Returns the bi-streams on success. On failure the connection is closed.
async fn authenticate_miner_connection(
	connection: &quinn::Connection,
	server: &MinerServer,
) -> Option<(quinn::SendStream, quinn::RecvStream)> {
	let addr = connection.remote_address();
	log::info!("⛏️ New miner connection from {}", addr);

	let handshake = async {
		log::debug!("Waiting for miner {} to open bidirectional stream...", addr);
		let (send, mut recv) = connection
			.accept_bi()
			.await
			.map_err(|e| format!("Failed to accept stream from {}: {}", addr, e))?;
		log::info!("⛏️ Stream accepted from miner {}", addr);

		log::debug!("Waiting for Ready (auth) from miner {}...", addr);
		match read_message(&mut recv).await {
			// Trim so a miner that sends file contents including a trailing newline
			// still matches the node's trimmed on-disk token. Compare in constant time.
			Ok(MinerMessage::Ready { token })
				if auth_tokens_equal(&token, server.auth_token.as_ref()) =>
			{
				log::debug!("Miner {} authenticated", addr);
				Ok((send, recv))
			},
			Ok(MinerMessage::Ready { .. }) =>
				Err(format!("Rejected miner {}: invalid auth token", addr)),
			Ok(other) => Err(format!("Expected Ready from miner {}, got {:?}", addr, other)),
			Err(e) => Err(format!("Failed to read Ready from miner {}: {}", addr, e)),
		}
	};

	match tokio::time::timeout(AUTH_HANDSHAKE_TIMEOUT, handshake).await {
		Ok(Ok(streams)) => Some(streams),
		Ok(Err(e)) => {
			log::warn!("⛏️ {}", e);
			connection.close(0u32.into(), b"auth failed");
			None
		},
		Err(_) => {
			log::warn!(
				"⛏️ Rejected miner {}: auth handshake timed out after {:?}",
				addr,
				AUTH_HANDSHAKE_TIMEOUT
			);
			connection.close(0u32.into(), b"auth timeout");
			None
		},
	}
}

/// Register and serve a miner that has already passed auth.
async fn serve_authenticated_miner(
	_connection: quinn::Connection,
	server: Arc<MinerServer>,
	send: quinn::SendStream,
	recv: quinn::RecvStream,
) {
	let (job_tx, job_rx) = mpsc::channel::<MiningRequest>(16);
	let miner_id = server.add_miner(job_tx).await;

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

	// Send initial job if there is one (Ready/auth already handled by the caller)
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
					Ok(MinerMessage::Ready { .. }) => {
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
	use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

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

	fn temp_token_path(name: &str) -> PathBuf {
		static COUNTER: AtomicU64 = AtomicU64::new(0);
		let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
		std::env::temp_dir().join(format!("quantus-miner-auth-token-test-{name}-{n}"))
	}

	#[test]
	fn generates_token_file_when_missing() {
		let path = temp_token_path("missing");
		let _ = fs::remove_file(&path);

		let token = load_or_create_miner_auth_token(&path).unwrap();
		assert_eq!(token.len(), 64);
		assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
		assert_eq!(fs::read_to_string(&path).unwrap().trim(), token);

		let _ = fs::remove_file(&path);
	}

	#[test]
	fn reuses_existing_token_file() {
		let path = temp_token_path("existing");
		let _ = fs::remove_file(&path);
		fs::write(&path, "deadbeef\n").unwrap();

		let token = load_or_create_miner_auth_token(&path).unwrap();
		assert_eq!(token, "deadbeef");

		let _ = fs::remove_file(&path);
	}

	#[test]
	fn rejects_empty_token_file() {
		let path = temp_token_path("empty");
		let _ = fs::remove_file(&path);
		fs::write(&path, "   \n").unwrap();

		let err = load_or_create_miner_auth_token(&path).unwrap_err();
		assert!(err.contains("empty"));

		let _ = fs::remove_file(&path);
	}

	#[test]
	fn rejects_token_whose_escaped_frame_exceeds_message_limit() {
		let path = temp_token_path("escape-heavy");
		// 512 raw bytes (passes the raw-length cap) but JSON-escapes to ~1024,
		// pushing the Ready frame past MAX_MESSAGE_SIZE.
		let token = "\"".repeat(MAX_AUTH_TOKEN_LEN);

		let err = validate_auth_token_length(&token, &path).unwrap_err();
		assert!(err.contains("Ready frame"), "unexpected error: {err}");

		// A max-length token without escaping still fits.
		let token = "a".repeat(MAX_AUTH_TOKEN_LEN);
		validate_auth_token_length(&token, &path).unwrap();
	}

	fn temp_tls_dir(name: &str) -> PathBuf {
		static COUNTER: AtomicU64 = AtomicU64::new(0);
		let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
		let dir = std::env::temp_dir().join(format!("quantus-miner-tls-test-{name}-{n}"));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		dir
	}

	#[test]
	fn generates_and_reuses_miner_tls_material() {
		let dir = temp_tls_dir("roundtrip");
		let first = load_or_create_miner_tls(&dir).unwrap();
		assert_eq!(first.fingerprint_hex.len(), 64);
		assert!(first.fingerprint_path.exists());

		let second = load_or_create_miner_tls(&dir).unwrap();
		assert_eq!(first.cert_der, second.cert_der);
		assert_eq!(first.key_der, second.key_der);
		assert_eq!(first.fingerprint_hex, second.fingerprint_hex);

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn corrupt_cert_does_not_overwrite_fingerprint() {
		let dir = temp_tls_dir("corrupt-fp");
		let first = load_or_create_miner_tls(&dir).unwrap();
		let fp_before = fs::read_to_string(&first.fingerprint_path).unwrap();

		fs::write(dir.join(DEFAULT_MINER_TLS_CERT_FILENAME), b"not-a-cert").unwrap();
		assert!(load_or_create_miner_tls(&dir).is_err());

		let fp_after = fs::read_to_string(&first.fingerprint_path).unwrap();
		assert_eq!(fp_before, fp_after, "distributed fingerprint must survive a bad cert load");

		let _ = fs::remove_dir_all(&dir);
	}
}
