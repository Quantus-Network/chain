use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use hex;
use primitive_types::U512;
use codec::{Encode, Decode};
use warp::{Rejection, Reply};

#[derive(Debug, Clone, Encode, Decode)]
pub struct QPoWSeal {
    pub nonce: [u8; 64],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiningRequest {
    pub job_id: String,
    pub mining_hash: String,
    pub difficulty: String,
    pub nonce_start: String,
    pub nonce_end: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiningResponse {
    pub status: String,
    pub job_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiningResult {
    pub status: String,
    pub job_id: String,
    pub nonce: Option<String>,
    pub work: Option<String>,
}

#[derive(Clone)]
pub struct MiningState {
    pub jobs: Arc<Mutex<HashMap<String, MiningJob>>>,
}

#[derive(Debug, Clone)]
pub struct MiningJob {
    pub header_hash: [u8; 32],
    pub difficulty: u64,
    pub nonce_start: U512,
    pub nonce_end: U512,
    pub current_nonce: U512,
    pub status: String,
}

impl MiningState {
    pub fn new() -> Self {
        MiningState {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_job(&self, job_id: String, job: MiningJob) -> Result<(), String> {
        let mut jobs = self.jobs.lock().await;
        if jobs.contains_key(&job_id) {
            return Err("Job already exists".to_string());
        }
        jobs.insert(job_id, job);
        Ok(())
    }

    pub async fn get_job(&self, job_id: &str) -> Option<MiningJob> {
        let jobs = self.jobs.lock().await;
        jobs.get(job_id).cloned()
    }

    pub async fn remove_job(&self, job_id: &str) -> Option<MiningJob> {
        let mut jobs = self.jobs.lock().await;
        jobs.remove(job_id)
    }
}

pub fn validate_mining_request(request: &MiningRequest) -> Result<(), String> {
    if request.job_id.is_empty() {
        return Err("Job ID cannot be empty".to_string());
    }
    if request.mining_hash.len() != 64 {
        return Err("Mining hash must be 64 characters".to_string());
    }
    if request.difficulty.parse::<u64>().is_err() {
        return Err("Invalid difficulty".to_string());
    }
    if request.nonce_start.len() != 128 || request.nonce_end.len() != 128 {
        return Err("Nonce must be 128 characters".to_string());
    }
    Ok(())
}

// HTTP Handlers
pub async fn handle_mine_request(
    request: MiningRequest,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
    if let Err(e) = validate_mining_request(&request) {
        return Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: format!("error: {}", e),
                job_id: request.job_id,
            }),
            warp::http::StatusCode::BAD_REQUEST,
        ));
    }

    let job = MiningJob {
        header_hash: hex::decode(&request.mining_hash)
            .unwrap()
            .try_into()
            .unwrap(),
        difficulty: request.difficulty.parse().unwrap(),
        nonce_start: U512::from_str_radix(&request.nonce_start, 16).unwrap(),
        nonce_end: U512::from_str_radix(&request.nonce_end, 16).unwrap(),
        current_nonce: U512::from_str_radix(&request.nonce_start, 16).unwrap(),
        status: "running".to_string(),
    };

    if let Err(e) = state.add_job(request.job_id.clone(), job).await {
        return Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: format!("error: {}", e),
                job_id: request.job_id,
            }),
            warp::http::StatusCode::BAD_REQUEST,
        ));
    }

    Ok(warp::reply::with_status(
        warp::reply::json(&MiningResponse {
            status: "accepted".to_string(),
            job_id: request.job_id,
        }),
        warp::http::StatusCode::OK,
    ))
}

pub async fn handle_result_request(
    job_id: String,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
    if let Some(job) = state.get_job(&job_id).await {
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResult {
                status: job.status,
                job_id,
                nonce: Some(format!("{:016x}", job.current_nonce.low_u64())),
                work: None,
            }),
            warp::http::StatusCode::OK,
        ))
    } else {
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResult {
                status: "not_found".to_string(),
                job_id,
                nonce: None,
                work: None,
            }),
            warp::http::StatusCode::NOT_FOUND,
        ))
    }
}

pub async fn handle_cancel_request(
    job_id: String,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
    if state.remove_job(&job_id).await.is_some() {
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: "cancelled".to_string(),
                job_id,
            }),
            warp::http::StatusCode::OK,
        ))
    } else {
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: "not_found".to_string(),
                job_id,
            }),
            warp::http::StatusCode::NOT_FOUND,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_mining_request() {
        // Test valid request
        let valid_request = MiningRequest {
            job_id: "test".to_string(),
            mining_hash: "a".repeat(64),
            difficulty: "1000".to_string(),
            nonce_start: "0".repeat(128),
            nonce_end: "1".repeat(128),
        };
        assert!(validate_mining_request(&valid_request).is_ok());

        // Test empty job ID
        let invalid_request = MiningRequest {
            job_id: "".to_string(),
            mining_hash: "a".repeat(64),
            difficulty: "1000".to_string(),
            nonce_start: "0".repeat(128),
            nonce_end: "1".repeat(128),
        };
        assert!(validate_mining_request(&invalid_request).is_err());

        // Test invalid mining hash length
        let invalid_hash = MiningRequest {
            job_id: "test".to_string(),
            mining_hash: "a".repeat(63), // Too short
            difficulty: "1000".to_string(),
            nonce_start: "0".repeat(128),
            nonce_end: "1".repeat(128),
        };
        assert!(validate_mining_request(&invalid_hash).is_err());

        // Test invalid difficulty
        let invalid_difficulty = MiningRequest {
            job_id: "test".to_string(),
            mining_hash: "a".repeat(64),
            difficulty: "not_a_number".to_string(),
            nonce_start: "0".repeat(128),
            nonce_end: "1".repeat(128),
        };
        assert!(validate_mining_request(&invalid_difficulty).is_err());

        // Test invalid nonce length
        let invalid_nonce = MiningRequest {
            job_id: "test".to_string(),
            mining_hash: "a".repeat(64),
            difficulty: "1000".to_string(),
            nonce_start: "0".repeat(127), // Too short
            nonce_end: "1".repeat(128),
        };
        assert!(validate_mining_request(&invalid_nonce).is_err());
    }

    #[tokio::test]
    async fn test_mining_state() {
        let state = MiningState::new();
        let job = MiningJob {
            header_hash: [0; 32],
            difficulty: 1000,
            nonce_start: U512::from(0),
            nonce_end: U512::from(1000),
            current_nonce: U512::from(0),
            status: "running".to_string(),
        };

        // Test adding a job
        assert!(state.add_job("test".to_string(), job.clone()).await.is_ok());
        
        // Test adding duplicate job
        assert!(state.add_job("test".to_string(), job.clone()).await.is_err());
        
        // Test getting a job
        let retrieved_job = state.get_job("test").await;
        assert!(retrieved_job.is_some());
        assert_eq!(retrieved_job.unwrap().difficulty, 1000);
        
        // Test removing a job
        let removed_job = state.remove_job("test").await;
        assert!(removed_job.is_some());
        
        // Test job no longer exists
        assert!(state.get_job("test").await.is_none());
    }

    #[tokio::test]
    async fn test_concurrent_state_access() {
        let state = MiningState::new();
        let mut handles = vec![];

        // Spawn multiple tasks to add jobs concurrently
        for i in 0..10 {
            let state = state.clone();
            let job = MiningJob {
                header_hash: [0; 32],
                difficulty: 1000,
                nonce_start: U512::from(0),
                nonce_end: U512::from(1000),
                current_nonce: U512::from(0),
                status: "running".to_string(),
            };
            let handle = tokio::spawn(async move {
                state.add_job(format!("job{}", i), job).await
            });
            handles.push(handle);
        }

        // Wait for all jobs to be added
        for handle in handles {
            assert!(handle.await.unwrap().is_ok());
        }

        // Verify all jobs exist
        for i in 0..10 {
            assert!(state.get_job(&format!("job{}", i)).await.is_some());
        }
    }

    #[tokio::test]
    async fn test_job_status_transitions() {
        let state = MiningState::new();
        let job = MiningJob {
            header_hash: [0; 32],
            difficulty: 1000,
            nonce_start: U512::from(0),
            nonce_end: U512::from(1000),
            current_nonce: U512::from(0),
            status: "running".to_string(),
        };

        // Add job
        assert!(state.add_job("test".to_string(), job).await.is_ok());

        // Get and update job status
        let mut jobs = state.jobs.lock().await;
        if let Some(job) = jobs.get_mut("test") {
            job.status = "completed".to_string();
        }
        drop(jobs);

        // Verify status update
        let updated_job = state.get_job("test").await;
        assert_eq!(updated_job.unwrap().status, "completed");
    }
} 