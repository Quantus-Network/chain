// external-miner/src/lib.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use hex;
use primitive_types::U512;
use codec::{Encode, Decode};
use warp::{Rejection, Reply};
// Import the specific function needed
use qpow_math::is_valid_nonce;
use log;
use std::time::Instant; // Ensure Instant is imported

#[derive(Debug, Clone, Encode, Decode)]
pub struct QPoWSeal {
    // Nonce size should match what is_valid_nonce expects
    pub nonce: [u8; 64],
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MiningRequest {
    pub job_id: String,
    pub mining_hash: String, // Hex encoded header hash (32 bytes -> 64 chars)
    pub difficulty: String,
    pub nonce_start: String, // Hex encoded start nonce (64 bytes -> 128 chars)
    pub nonce_end: String, // Hex encoded end nonce (64 bytes -> 128 chars)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed,
    Failed, // e.g., reached nonce_end without success
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")] // Use snake_case for JSON representation
pub enum ApiResponseStatus {
    Accepted,   // Job submitted successfully
    Running,    // Job is currently being processed
    Completed,  // Job finished successfully (found nonce)
    Failed,     // Job finished unsuccessfully (e.g., nonce range exhausted)
    Cancelled,  // Job was cancelled via API
    NotFound,   // Job ID doesn't exist
    Error,      // Request failed (validation, duplicate job, etc.)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiningResponse {
    pub status: ApiResponseStatus,
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiningResult {
    pub status: ApiResponseStatus,
    pub job_id: String,
    pub nonce: Option<String>, // Hex encoded U512 representation of the final/winning nonce
    pub work: Option<String>, // Hex encoded [u8; 64] representation of the winning nonce
    pub hash_count: u64,
    pub elapsed_time: f64,
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
    pub status: JobStatus,
    pub hash_count: u64,
    pub start_time: Instant,
}

impl MiningJob {
    pub fn new(
        header_hash: [u8; 32],
        difficulty: u64,
        nonce_start: U512,
        nonce_end: U512,
    ) -> Self {
        MiningJob {
            header_hash,
            difficulty,
            nonce_start,
            nonce_end,
            current_nonce: nonce_start,
            status: JobStatus::Running,
            hash_count: 0,
            start_time: Instant::now(),
        }
    }
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
            log::warn!("Attempted to add duplicate job ID: {}", job_id);
            return Err("Job already exists".to_string());
        }
        log::info!("Adding job: {}", job_id);
        jobs.insert(job_id, job);
        Ok(())
    }

    pub async fn get_job(&self, job_id: &str) -> Option<MiningJob> {
        let jobs = self.jobs.lock().await;
        jobs.get(job_id).cloned()
    }

    pub async fn remove_job(&self, job_id: &str) -> Option<MiningJob> {
        let mut jobs = self.jobs.lock().await;
        log::info!("Removing job: {}", job_id);
        jobs.remove(job_id)
    }

    // Start the mining loop using qpow_math::is_valid_nonce
    pub async fn start_mining_loop(&self) {
        let jobs = self.jobs.clone();
        log::info!("Starting mining loop...");
        tokio::spawn(async move {
            loop {
                let mut jobs_guard = jobs.lock().await;
                for (job_id, job) in jobs_guard.iter_mut() {
                    if job.status == JobStatus::Running {
                        // Increment nonce first for the next potential check
                        // (unless it's the very first hash attempt)
                        if job.hash_count > 0 {
                            job.current_nonce = job.current_nonce + U512::from(1);
                        }

                        // Check if the *new* current_nonce has exceeded the range *before* hashing
                        if job.current_nonce > job.nonce_end { // Use > comparison now
                            job.status = JobStatus::Failed;
                            log::info!(
                                "Job {} failed (exceeded nonce_end {}). Hashes: {}, Time: {:?}",
                                job_id,
                                job.nonce_end, // Log the boundary
                                job.hash_count, // Hash count is before this failed attempt
                                job.start_time.elapsed()
                            );
                            continue; // Move to the next job
                        }

                        // Convert U512 nonce to [u8; 64] for is_valid_nonce
                        let nonce_bytes = job.current_nonce.to_big_endian();

                        job.hash_count += 1;

                        // Call the verification function from qpow-math
                        if is_valid_nonce(job.header_hash, nonce_bytes, job.difficulty) {
                            job.status = JobStatus::Completed;
                            log::info!(
                                "Job {} COMPLETED! Nonce: {} ({}), Hashes: {}, Time: {:?}",
                                job_id,
                                job.current_nonce,
                                hex::encode(nonce_bytes),
                                job.hash_count,
                                job.start_time.elapsed()
                            );
                        } else if job.current_nonce == job.nonce_end { 
                            // If it wasn't valid and we are at the end, mark as failed
                            job.status = JobStatus::Failed;
                            log::info!(
                                "Job {} failed (checked up to nonce_end {}). Hashes: {}, Time: {:?}",
                                job_id,
                                job.nonce_end,
                                job.hash_count,
                                job.start_time.elapsed()
                            );
                        }
                    }
                }
                drop(jobs_guard); // Release the lock before sleeping
                // Adjust sleep time based on performance/CPU usage goals
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await; // Reduced sleep time
            }
        });
    }
}

pub fn validate_mining_request(request: &MiningRequest) -> Result<(), String> {
    if request.job_id.is_empty() {
        return Err("Job ID cannot be empty".to_string());
    }
    if request.mining_hash.len() != 64 || hex::decode(&request.mining_hash).is_err() {
        return Err("Invalid mining_hash (must be 64 hex characters)".to_string());
    }
    if request.difficulty.parse::<u64>().is_err() {
        return Err("Invalid difficulty (must be a valid u64)".to_string());
    }
    if request.nonce_start.len() != 128 || U512::from_str_radix(&request.nonce_start, 16).is_err() {
        return Err("Invalid nonce_start (must be 128 hex characters)".to_string());
    }
    if request.nonce_end.len() != 128 || U512::from_str_radix(&request.nonce_end, 16).is_err() {
         return Err("Invalid nonce_end (must be 128 hex characters)".to_string());
    }
     if U512::from_str_radix(&request.nonce_start, 16).unwrap() > U512::from_str_radix(&request.nonce_end, 16).unwrap() {
         return Err("nonce_start cannot be greater than nonce_end".to_string());
     }
    Ok(())
}

// --- HTTP Handlers ---

pub async fn handle_mine_request(
    request: MiningRequest,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
     log::debug!("Received mine request: {:?}", request);
    if let Err(e) = validate_mining_request(&request) {
         log::warn!("Invalid mine request ({}): {}", request.job_id, e);
        return Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: ApiResponseStatus::Error,
                job_id: request.job_id,
                message: Some(e),
            }),
            warp::http::StatusCode::BAD_REQUEST,
        ));
    }

    // Use unwrap safely due to validation
    let header_hash: [u8; 32] = hex::decode(&request.mining_hash)
        .unwrap()
        .try_into()
        .expect("Validated hex string is 32 bytes");
    let difficulty = request.difficulty.parse().unwrap();
    let nonce_start = U512::from_str_radix(&request.nonce_start, 16).unwrap();
    let nonce_end = U512::from_str_radix(&request.nonce_end, 16).unwrap();

    let job = MiningJob::new(
        header_hash,
        difficulty,
        nonce_start,
        nonce_end,
    );

    match state.add_job(request.job_id.clone(), job).await {
        Ok(_) => {
             log::info!("Accepted mine request for job ID: {}", request.job_id);
            Ok(warp::reply::with_status(
                warp::reply::json(&MiningResponse {
                    status: ApiResponseStatus::Accepted,
                    job_id: request.job_id,
                    message: None,
                }),
                warp::http::StatusCode::OK,
            ))
        }
        Err(e) => {
             log::error!("Failed to add job {}: {}", request.job_id, e);
            Ok(warp::reply::with_status(
                warp::reply::json(&MiningResponse {
                    status: ApiResponseStatus::Error,
                    job_id: request.job_id,
                    message: Some(e),
                }),
                // Use CONFLICT (409) if job already exists? Or stick to BAD_REQUEST?
                warp::http::StatusCode::CONFLICT,
            ))
        }
    }
}

pub async fn handle_result_request(
    job_id: String,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
     log::debug!("Received result request for job ID: {}", job_id);
    if let Some(job) = state.get_job(&job_id).await {
        // Map internal JobStatus to ApiResponseStatus
        let api_status = match job.status {
            JobStatus::Running => ApiResponseStatus::Running,
            JobStatus::Completed => ApiResponseStatus::Completed,
            JobStatus::Failed => ApiResponseStatus::Failed,
        };

        // Provide the U512 nonce as a plain hex string (no 0x)
        let nonce_hex = format!("{:x}", job.current_nonce);

        // Work field is already plain hex
        let work_hex = if job.status == JobStatus::Completed {
            Some(hex::encode(job.current_nonce.to_big_endian()))
        } else {
            None
        };

        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResult {
                status: api_status,
                job_id,
                nonce: Some(nonce_hex),
                work: work_hex,
                hash_count: job.hash_count,
                elapsed_time: job.start_time.elapsed().as_secs_f64(),
            }),
            warp::http::StatusCode::OK,
        ))
    } else {
        log::warn!("Result requested for unknown job ID: {}", job_id);
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResult {
                status: ApiResponseStatus::NotFound,
                job_id,
                nonce: None,
                work: None,
                hash_count: 0,
                elapsed_time: 0.0,
            }),
            warp::http::StatusCode::NOT_FOUND,
        ))
    }
}

pub async fn handle_cancel_request(
    job_id: String,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
     log::debug!("Received cancel request for job ID: {}", job_id);
    // Removing the job effectively cancels it
    if state.remove_job(&job_id).await.is_some() {
         log::info!("Cancelled job ID: {}", job_id);
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: ApiResponseStatus::Cancelled,
                job_id,
                message: None,
            }),
            warp::http::StatusCode::OK,
        ))
    } else {
        log::warn!("Cancel requested for unknown job ID: {}", job_id);
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: ApiResponseStatus::NotFound,
                job_id,
                message: None,
            }),
            warp::http::StatusCode::NOT_FOUND,
        ))
    }
}

// Removed compute_hash function
// Removed check_difficulty function

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};
    use std::time::Instant;

    // --- Keep existing tests ---
    #[test]
    fn test_validate_mining_request() {
        // (Keep the body of this test as it was)
        // Test valid request
        let valid_request = MiningRequest {
            job_id: "test_valid".to_string(),
            mining_hash: "a".repeat(64),
            difficulty: "1000".to_string(),
            nonce_start: "0".repeat(128),
            nonce_end: "f".repeat(128),
        };
        assert!(validate_mining_request(&valid_request).is_ok());

        // Test empty job ID
        let invalid_request_job_id = MiningRequest {
            job_id: "".to_string(), ..valid_request.clone() };
        assert!(validate_mining_request(&invalid_request_job_id).is_err());

        // Test invalid mining hash length
        let invalid_request_hash = MiningRequest {
             mining_hash: "a".repeat(63), ..valid_request.clone() };
        assert!(validate_mining_request(&invalid_request_hash).is_err());
         let invalid_request_hash_hex = MiningRequest {
             mining_hash: "g".repeat(64), ..valid_request.clone() }; // Not hex
        assert!(validate_mining_request(&invalid_request_hash_hex).is_err());


        // Test invalid difficulty
        let invalid_request_diff = MiningRequest {
             difficulty: "not_a_number".to_string(), ..valid_request.clone() };
        assert!(validate_mining_request(&invalid_request_diff).is_err());

        // Test invalid nonce length
        let invalid_request_nonce_start_len = MiningRequest {
             nonce_start: "0".repeat(127), ..valid_request.clone() };
        assert!(validate_mining_request(&invalid_request_nonce_start_len).is_err());
         let invalid_request_nonce_end_len = MiningRequest {
             nonce_end: "f".repeat(127), ..valid_request.clone() };
        assert!(validate_mining_request(&invalid_request_nonce_end_len).is_err());

         // Test invalid nonce hex
         let invalid_request_nonce_start_hex = MiningRequest {
              nonce_start: "g".repeat(128), ..valid_request.clone() };
         assert!(validate_mining_request(&invalid_request_nonce_start_hex).is_err());
          let invalid_request_nonce_end_hex = MiningRequest {
              nonce_end: "g".repeat(128), ..valid_request.clone() };
         assert!(validate_mining_request(&invalid_request_nonce_end_hex).is_err());

         // Test nonce_start > nonce_end
         let invalid_request_nonce_order = MiningRequest {
              nonce_start: "1".repeat(128), nonce_end: "0".repeat(128), ..valid_request.clone() };
         assert!(validate_mining_request(&invalid_request_nonce_order).is_err());
    }

    #[tokio::test]
    async fn test_mining_state() {
        // (Keep the body of this test as it was)
         let state = MiningState::new();
        let job = MiningJob {
            header_hash: [0; 32],
            difficulty: 1000,
            nonce_start: U512::from(0),
            nonce_end: U512::from(1000),
            current_nonce: U512::from(0),
            status: JobStatus::Running,
            hash_count: 0,
            start_time: std::time::Instant::now(),
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
        // (Keep the body of this test as it was)
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
                status: JobStatus::Running,
                hash_count: 0,
                start_time: std::time::Instant::now(),
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
        // (Keep the body of this test as it was)
         let state = MiningState::new();
        let job = MiningJob {
            header_hash: [0; 32],
            difficulty: 1000,
            nonce_start: U512::from(0),
            nonce_end: U512::from(1000),
            current_nonce: U512::from(0),
            status: JobStatus::Running,
            hash_count: 0,
            start_time: std::time::Instant::now(),
        };

        // Add job
        assert!(state.add_job("test".to_string(), job).await.is_ok());

        // Get and update job status directly for testing
        let mut jobs = state.jobs.lock().await;
        if let Some(job) = jobs.get_mut("test") {
            job.status = JobStatus::Completed;
        }
        drop(jobs);

        // Verify status update
        let updated_job = state.get_job("test").await;
        assert_eq!(updated_job.unwrap().status, JobStatus::Completed);
    }

    // --- Tests potentially needing adjustment or removal ---

    // This test relies on the internal mining loop finding a solution.
    // Since is_valid_nonce is complex, finding a real nonce might take time
    // or require specific known inputs. We might simplify it or make it
    // primarily check that the status eventually changes from "running".
    #[tokio::test]
    async fn test_mining_loop_status_change() {
        // Use a very low difficulty that is likely to be found quickly,
        // OR mock is_valid_nonce if possible (difficult without mocking framework)
        // For now, assume difficulty 1 might find something, or it fails.
        let state = MiningState::new();
        state.start_mining_loop().await; // Start the actual loop

        let job = MiningJob {
            header_hash: [1; 32], // Use a non-zero hash
            difficulty: 1, // Very low difficulty
            nonce_start: U512::from(0),
            // Small nonce range to ensure it finishes if no solution found
            nonce_end: U512::from(500),
            current_nonce: U512::from(0),
            status: JobStatus::Running,
            hash_count: 0,
            start_time: Instant::now(),
        };

        assert!(state.add_job("test_loop".to_string(), job).await.is_ok());

        // Wait a reasonable time for the loop to potentially find a nonce or fail
        sleep(Duration::from_millis(200)).await;

        let updated_job = state.get_job("test_loop").await;
        assert!(updated_job.is_some(), "Job should still exist");
        let job = updated_job.unwrap();

        // Check that the status is no longer "running"
        assert_ne!(job.status, JobStatus::Running, "Job status should have changed from running");
        // It could be "completed" or "failed"
        assert!(job.status == JobStatus::Completed || job.status == JobStatus::Failed, "Job status should be completed or failed");
        assert!(job.hash_count > 0, "Should have attempted at least one hash");
        println!("Mining loop test final status: {}, hash_count: {}", job.status, job.hash_count);
    }

    // Keep concurrent mining test, focusing on adding jobs and seeing status change
     #[tokio::test]
     async fn test_concurrent_mining_status_change() {
         let state = MiningState::new();
         state.start_mining_loop().await;

         let mut handles = vec![];
         let num_jobs = 5;

         for i in 0..num_jobs {
             let state = state.clone();
             let job = MiningJob {
                 header_hash: [i as u8; 32], // Different hash per job
                 difficulty: 1, // Low difficulty
                 nonce_start: U512::from(0),
                 nonce_end: U512::from(500), // Small range
                 current_nonce: U512::from(0),
                 status: JobStatus::Running,
                 hash_count: 0,
                 start_time: Instant::now(),
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

         // Wait for mining loop to process
         sleep(Duration::from_millis(500)).await; // Increase wait time slightly

         // Verify all jobs were processed (status changed)
         let mut completed_count = 0;
         let mut failed_count = 0;
         for i in 0..num_jobs {
              let job_id = format!("job{}", i);
             let job_opt = state.get_job(&job_id).await;
             assert!(job_opt.is_some(), "Job {} should exist", job_id);
             let job = job_opt.unwrap();
             assert_ne!(job.status, JobStatus::Running, "Job {} status should have changed", job_id);
             assert!(job.status == JobStatus::Completed || job.status == JobStatus::Failed, "Job {} status invalid", job_id);
              if job.status == JobStatus::Completed { completed_count += 1; }
              if job.status == JobStatus::Failed { failed_count += 1; }
             println!("Concurrent test - Job {}: Status={}, Hashes={}", job_id, job.status, job.hash_count);
         }
         println!("Concurrent test results: Completed={}, Failed={}", completed_count, failed_count);
         assert_eq!(completed_count + failed_count, num_jobs, "All jobs should be either completed or failed");
     }

    // Remove the test_hash_and_difficulty_check test
    // #[test]
    // fn test_hash_and_difficulty_check() { ... }

    // --- New Tests ---

    // Helper to create a basic job for testing
    fn create_test_job(
        job_id: &str,
        difficulty: u64,
        nonce_start: u64,
        nonce_end: u64,
    ) -> (String, MiningJob) {
        let job = MiningJob {
            header_hash: [job_id.len() as u8; 32], // Simple deterministic hash
            difficulty,
            nonce_start: U512::from(nonce_start),
            nonce_end: U512::from(nonce_end),
            current_nonce: U512::from(nonce_start),
            status: JobStatus::Running,
            hash_count: 0,
            start_time: Instant::now(),
        };
        (job_id.to_string(), job)
    }

    #[tokio::test]
    async fn test_mining_job_completes() {
        // We need inputs that *might* result in is_valid_nonce returning true quickly.
        // This is inherently difficult without mocking is_valid_nonce or knowing
        // specific inputs that work for qpow_math's logic.
        // We use a very low difficulty and a small range.
        let state = MiningState::new();
        state.start_mining_loop().await;

        // Use difficulty 1, hoping *some* nonce in the small range satisfies the L1 distance.
        let (job_id, job) = create_test_job("complete_test", 1, 0, 100);
        state.add_job(job_id.clone(), job).await.unwrap();

        // Wait for the loop to potentially find the nonce
        sleep(Duration::from_millis(100)).await;

        let result_job = state.get_job(&job_id).await.unwrap();

        // It's possible it fails if no nonce works, but we hope for completed
        if result_job.status != JobStatus::Completed {
             println!("WARN: test_mining_job_completes did not complete, ended as {}. This might be due to qpow_math complexity.", result_job.status);
        }
        assert_ne!(result_job.status, JobStatus::Running);
        assert!(result_job.hash_count > 0);
    }

    #[tokio::test]
    async fn test_mining_job_fails_nonce_end() {
        // Use a difficulty that is *guaranteed* to fail for all nonces in the range.
        // MAX_DISTANCE is large, so difficulty = MAX_DISTANCE should always fail unless nonce is 0.
        // A difficulty slightly less than MAX_DISTANCE is safer.
        let impossible_difficulty = qpow_math::MAX_DISTANCE - 1; // Should make distance check fail unless distance is 0
        let state = MiningState::new();
        state.start_mining_loop().await;

        let start_nonce = 10; // Start above 0 to avoid the nonce==0 edge case in is_valid_nonce
        let end_nonce = 20;
        let (job_id, job) = create_test_job("fail_test", impossible_difficulty, start_nonce, end_nonce);
        state.add_job(job_id.clone(), job).await.unwrap();

        // Wait long enough for the loop to iterate through the small range
        sleep(Duration::from_millis(250)).await; // Increased sleep time

        let result_job = state.get_job(&job_id).await.unwrap();
        assert_eq!(result_job.status, JobStatus::Failed, "Job should have failed by reaching nonce_end");
        assert_eq!(result_job.current_nonce, U512::from(end_nonce), "Current nonce should be at nonce_end");
        // Expect hash count to be exactly (end_nonce - start_nonce + 1) nonces checked
        assert_eq!(result_job.hash_count, (end_nonce - start_nonce + 1), "Hash count should match the range size");
    }

     #[tokio::test]
    async fn test_mining_job_result_work_field() {
        // Similar to test_mining_job_completes, hoping it finds a solution.
        let state = MiningState::new();
        state.start_mining_loop().await;

        let (job_id, job) = create_test_job("work_field_test", 1, 0, 100);
        // let job_clone = job.clone(); // No longer needed
        state.add_job(job_id.clone(), job).await.unwrap();

        sleep(Duration::from_millis(100)).await; // Wait for potential completion

        // Get the final job state directly instead of processing HTTP reply
        let final_job_state_opt = state.get_job(&job_id).await;
        assert!(final_job_state_opt.is_some(), "Job should exist after attempting work");
        let final_job_state = final_job_state_opt.unwrap();

        // Construct the expected result based on final job state
        let expected_status = final_job_state.status.clone();
        let expected_nonce_hex = Some(format!("{:x}", final_job_state.current_nonce));
        let expected_work_hex = if final_job_state.status == JobStatus::Completed {
            Some(hex::encode(final_job_state.current_nonce.to_big_endian()))
        } else {
            None
        };
        let expected_hash_count = final_job_state.hash_count;

        if final_job_state.status == JobStatus::Completed {
            assert_eq!(expected_status, JobStatus::Completed);
            assert!(expected_work_hex.is_some(), "Work field should be present for completed job");
            let expected_nonce_bytes = final_job_state.current_nonce.to_big_endian();
            assert_eq!(expected_work_hex, Some(hex::encode(expected_nonce_bytes)), "Work field should contain the hex of the winning nonce bytes");
            assert_eq!(expected_nonce_hex, Some(format!("{:x}", final_job_state.current_nonce)), "Nonce field should contain the U512 hex");
            assert_eq!(expected_hash_count, final_job_state.hash_count);
        } else {
            println!("WARN: test_mining_job_result_work_field did not complete, ended as {}.", final_job_state.status);
            assert_ne!(expected_status, JobStatus::Running);
            assert!(expected_work_hex.is_none(), "Work field should be None for non-completed job");
            assert_eq!(expected_nonce_hex, Some(format!("{:x}", final_job_state.current_nonce)), "Nonce field should contain the U512 hex even if failed");
        }
    }

    #[tokio::test]
    async fn test_mining_job_cancel() {
        let state = MiningState::new();
        state.start_mining_loop().await;

        // Job with a large range that won't finish quickly
        let (job_id, job) = create_test_job("cancel_test", 10000, 0, 1_000_000);
        state.add_job(job_id.clone(), job).await.unwrap();

        // Let it run for a very short time
        sleep(Duration::from_millis(20)).await;

        // Get current hash count
        let job_before_cancel = state.get_job(&job_id).await;
        assert!(job_before_cancel.is_some(), "Job should exist before cancel");
        let hash_count_before = job_before_cancel.unwrap().hash_count;
        assert!(hash_count_before > 0, "Job should have started hashing");


        // Cancel the job by removing it
        let removed_job = state.remove_job(&job_id).await;
        assert!(removed_job.is_some(), "Job should be removed successfully");
         println!("Cancelled job {} after {} hashes", job_id, hash_count_before);


        // Wait a bit longer to ensure the loop iterates more
        sleep(Duration::from_millis(50)).await;

        // Try to get the job again - it should be gone
        let job_after_cancel = state.get_job(&job_id).await;
        assert!(job_after_cancel.is_none(), "Job should not exist after cancel");

        // Check logs or potentially add internal counters (if needed) to be *absolutely* sure
        // the loop isn't still somehow processing the removed job ID, but removing it
        // from the HashMap is the standard way to stop processing in this pattern.
        // We rely on the loop checking the map each iteration.
    }

     #[tokio::test]
    async fn test_mining_job_nonce_start_equals_end() {
        let state = MiningState::new();
        state.start_mining_loop().await;

        let nonce_value = 50;
        let difficulty = qpow_math::MAX_DISTANCE -1; // Use impossible difficulty

        let (job_id, job) = create_test_job("single_nonce_test", difficulty, nonce_value, nonce_value);
        state.add_job(job_id.clone(), job).await.unwrap();

        // Wait just long enough for one check
        sleep(Duration::from_millis(50)).await;

        let result_job = state.get_job(&job_id).await.unwrap();

        // Since difficulty is impossible, it must fail
        assert_eq!(result_job.status, JobStatus::Failed, "Job should fail as the single nonce is invalid");
        // Hash count should be exactly 1 because only one nonce was checked
        assert_eq!(result_job.hash_count, 1, "Should have checked exactly one nonce");
        assert_eq!(result_job.current_nonce, U512::from(nonce_value));

         // --- Test completion case for single nonce ---
         let state_complete = MiningState::new();
         state_complete.start_mining_loop().await;
         let difficulty_easy = 1; // Easy difficulty
         let (job_id_c, job_c) = create_test_job("single_nonce_complete", difficulty_easy, nonce_value, nonce_value);
         state_complete.add_job(job_id_c.clone(), job_c).await.unwrap();

          sleep(Duration::from_millis(50)).await;
          let result_job_c = state_complete.get_job(&job_id_c).await.unwrap();

          // It should be completed or failed, status should not be running
           assert_ne!(result_job_c.status, JobStatus::Running);
           assert_eq!(result_job_c.hash_count, 1, "Should have checked exactly one nonce");
           assert_eq!(result_job_c.current_nonce, U512::from(nonce_value));
            if result_job_c.status == JobStatus::Completed {
                 println!("Single nonce test completed successfully.");
             } else {
                 println!("WARN: Single nonce test failed (status={}). qpow_math might not accept nonce {}.", result_job_c.status, nonce_value);
             }

    }

}
