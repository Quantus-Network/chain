pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

use serde::{Deserialize, Serialize};

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

/// Request payload sent from Node to Miner (`/mine` endpoint).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MiningRequest {
    pub job_id: String,
    /// Hex encoded header hash (32 bytes -> 64 chars, no 0x prefix)
    pub mining_hash: String,
    /// Difficulty (u64 as string)
    pub difficulty: String,
    /// Hex encoded start nonce (U512 -> 128 chars, no 0x prefix)
    pub nonce_start: String,
    /// Hex encoded end nonce (U512 -> 128 chars, no 0x prefix)
    pub nonce_end: String,
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
