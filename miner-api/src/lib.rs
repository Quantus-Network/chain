use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum message size (16 MB) to prevent memory exhaustion attacks.
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Status codes returned in API responses.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiResponseStatus {
	Accepted,
	Running,
	Completed,
	Failed,
	Cancelled,
	NotFound,
	Error,
}

/// QUIC protocol messages exchanged between node and miner.
///
/// The protocol is:
/// - Miner sends `Ready` immediately after connecting to establish the stream
/// - Node sends `NewJob` to submit a mining job (implicitly cancels any previous job)
/// - Miner sends `JobResult` when mining completes
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MinerMessage {
	/// Miner → Node: Sent immediately after connecting to establish the stream.
	/// This is required because QUIC streams are lazily initialized.
	Ready,

	/// Node → Miner: Submit a new mining job.
	/// If a job is already running, it will be cancelled and replaced.
	NewJob(MiningRequest),

	/// Miner → Node: Mining result (completed, failed, or cancelled).
	JobResult(MiningResult),
}

/// Write a length-prefixed JSON message to an async writer.
///
/// Wire format: 4-byte big-endian length prefix followed by JSON payload.
pub async fn write_message<W: AsyncWrite + Unpin>(
	writer: &mut W,
	msg: &MinerMessage,
) -> std::io::Result<()> {
	let json = serde_json::to_vec(msg)
		.map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
	let len = json.len() as u32;
	writer.write_all(&len.to_be_bytes()).await?;
	writer.write_all(&json).await?;
	Ok(())
}

/// Read a length-prefixed JSON message from an async reader.
///
/// Wire format: 4-byte big-endian length prefix followed by JSON payload.
/// Returns an error if the message exceeds MAX_MESSAGE_SIZE.
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<MinerMessage> {
	let mut len_buf = [0u8; 4];
	reader.read_exact(&mut len_buf).await?;
	let len = u32::from_be_bytes(len_buf);

	if len > MAX_MESSAGE_SIZE {
		return Err(std::io::Error::new(
			std::io::ErrorKind::InvalidData,
			format!("Message size {} exceeds maximum {}", len, MAX_MESSAGE_SIZE),
		));
	}

	let mut buf = vec![0u8; len as usize];
	reader.read_exact(&mut buf).await?;
	serde_json::from_slice(&buf)
		.map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Request payload sent from Node to Miner.
///
/// The miner will choose its own random starting nonce, enabling multiple
/// miners to work on the same job without coordination.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MiningRequest {
	pub job_id: String,
	/// Hex encoded header hash (32 bytes -> 64 chars, no 0x prefix)
	pub mining_hash: String,
	/// Distance threshold (U512 as decimal string)
	pub distance_threshold: String,
}

/// Response payload for job submission (`/mine`) and cancellation (`/cancel`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MiningResponse {
	pub status: ApiResponseStatus,
	pub job_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub message: Option<String>,
}

/// Response payload for checking job results (`/result/{job_id}`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MiningResult {
	pub status: ApiResponseStatus,
	pub job_id: String,
	/// Hex encoded U512 representation of the final/winning nonce (no 0x prefix).
	pub nonce: Option<String>,
	/// Hex encoded [u8; 64] representation of the winning nonce (128 chars, no 0x prefix).
	/// This is the primary field the Node uses for verification.
	pub work: Option<String>,
	pub hash_count: u64,
	pub elapsed_time: f64,
}
