// external-miner/src/lib.rs

use codec::{Decode, Encode};
use crossbeam_channel::{bounded, Receiver, Sender};
use primitive_types::U512;
use qpow_math::{get_nonce_distance, is_valid_nonce};
use resonance_miner_api::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use tokio::sync::Mutex;
use warp::{Rejection, Reply};

#[derive(Debug, Clone, Encode, Decode)]
pub struct QPoWSeal {
    pub nonce: [u8; 64],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct MiningJobResult {
    pub nonce: U512,
    pub work: [u8; 64],
    pub distance: U512,
    pub hash_count: u64,
}

#[derive(Clone)]
pub struct MiningState {
    pub jobs: Arc<Mutex<HashMap<String, MiningJob>>>,
}

#[derive(Debug)]
pub struct MiningJob {
    pub header_hash: [u8; 32],
    pub distance_threshold: U512,
    pub nonce_start: U512,
    pub nonce_end: U512,
    pub status: JobStatus,
    pub start_time: Instant,
    pub total_hash_count: u64,
    pub best_result: Option<MiningJobResult>,
    pub cancel_flag: Arc<AtomicBool>,
    pub result_receiver: Option<Receiver<ThreadResult>>,
    pub thread_handles: Vec<thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct ThreadResult {
    thread_id: usize,
    result: Option<MiningJobResult>,
    hash_count: u64,
    completed: bool,
}

impl MiningJob {
    pub fn new(
        header_hash: [u8; 32],
        distance_threshold: U512,
        nonce_start: U512,
        nonce_end: U512,
    ) -> Self {
        MiningJob {
            header_hash,
            distance_threshold,
            nonce_start,
            nonce_end,
            status: JobStatus::Running,
            start_time: Instant::now(),
            total_hash_count: 0,
            best_result: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            result_receiver: None,
            thread_handles: Vec::new(),
        }
    }

    pub fn start_mining(&mut self, num_cores: usize) {
        let (sender, receiver) = bounded(num_cores * 2);
        self.result_receiver = Some(receiver);

        // Calculate range size and split across cores
        let total_range = self.nonce_end - self.nonce_start + U512::one();
        let range_per_core = total_range / U512::from(num_cores);
        let remainder = total_range % U512::from(num_cores);

        log::info!(
            "Starting mining with {} cores, total range: {}, range per core: {}",
            num_cores,
            total_range,
            range_per_core
        );

        // Start mining threads
        for thread_id in 0..num_cores {
            let start = self.nonce_start + range_per_core * U512::from(thread_id);
            let mut end = start + range_per_core - U512::one();
            
            // Add remainder to the last thread
            if thread_id == num_cores - 1 {
                end = end + remainder;
            }

            // Ensure we don't exceed the original range
            if end > self.nonce_end {
                end = self.nonce_end;
            }

            let header_hash = self.header_hash;
            let distance_threshold = self.distance_threshold;
            let cancel_flag = self.cancel_flag.clone();
            let sender = sender.clone();

            let handle = thread::spawn(move || {
                mine_range(
                    thread_id,
                    header_hash,
                    distance_threshold,
                    start,
                    end,
                    cancel_flag,
                    sender,
                );
            });

            self.thread_handles.push(handle);
        }
    }

    pub fn cancel(&mut self) {
        log::info!("Cancelling mining job");
        self.cancel_flag.store(true, Ordering::Relaxed);
        self.status = JobStatus::Cancelled;
        
        // Wait for all threads to finish
        while let Some(handle) = self.thread_handles.pop() {
            if let Err(e) = handle.join() {
                log::warn!("Error joining mining thread: {:?}", e);
            }
        }
    }

    pub fn update_from_results(&mut self) -> bool {
        let receiver = match &self.result_receiver {
            Some(r) => r,
            None => return false,
        };

        let mut completed_threads = 0;
        let total_threads = self.thread_handles.len();
        let mut any_success = false;

        // Process all available results
        while let Ok(thread_result) = receiver.try_recv() {
            self.total_hash_count += thread_result.hash_count;

            if thread_result.completed {
                completed_threads += 1;
            }

            if let Some(result) = thread_result.result {
                any_success = true;
                
                // Check if this is the best result so far
                let is_better = match &self.best_result {
                    None => true,
                    Some(current_best) => result.distance < current_best.distance,
                };

                if is_better {
                    log::info!(
                        "Found better result from thread {}: distance = {}, nonce = {}",
                        thread_result.thread_id,
                        result.distance,
                        result.nonce
                    );
                    self.best_result = Some(result);
                }

                // Cancel all other threads when we find a result
                log::info!("Cancelling other threads due to successful result");
                self.cancel_flag.store(true, Ordering::Relaxed);
            }
        }

        // Update status based on results
        if any_success {
            self.status = JobStatus::Completed;
            return true;
        }

        if completed_threads >= total_threads {
            self.status = JobStatus::Failed;
            return true;
        }

        false // Still running
    }
}

impl Clone for MiningJob {
    fn clone(&self) -> Self {
        MiningJob {
            header_hash: self.header_hash,
            distance_threshold: self.distance_threshold,
            nonce_start: self.nonce_start,
            nonce_end: self.nonce_end,
            status: self.status.clone(),
            start_time: self.start_time,
            total_hash_count: self.total_hash_count,
            best_result: self.best_result.clone(),
            cancel_flag: self.cancel_flag.clone(),
            result_receiver: None, // Don't clone receiver
            thread_handles: Vec::new(), // Don't clone handles
        }
    }
}

impl Default for MiningState {
    fn default() -> Self {
        Self::new()
    }
}

impl MiningState {
    pub fn new() -> Self {
        MiningState {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_job(&self, job_id: String, mut job: MiningJob, num_cores: usize) -> Result<(), String> {
        let mut jobs = self.jobs.lock().await;
        if jobs.contains_key(&job_id) {
            log::warn!("Attempted to add duplicate job ID: {}", job_id);
            return Err("Job already exists".to_string());
        }

        log::info!("Adding job: {} with {} cores", job_id, num_cores);
        job.start_mining(num_cores);
        jobs.insert(job_id, job);
        Ok(())
    }

    pub async fn get_job(&self, job_id: &str) -> Option<MiningJob> {
        let jobs = self.jobs.lock().await;
        jobs.get(job_id).cloned()
    }

    pub async fn remove_job(&self, job_id: &str) -> Option<MiningJob> {
        let mut jobs = self.jobs.lock().await;
        if let Some(mut job) = jobs.remove(job_id) {
            log::info!("Removing job: {}", job_id);
            job.cancel();
            Some(job)
        } else {
            None
        }
    }

    pub async fn cancel_job(&self, job_id: &str) -> bool {
        let mut jobs = self.jobs.lock().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.cancel();
            true
        } else {
            false
        }
    }

    pub async fn start_mining_loop(&self, num_cores: usize) {
        let jobs = self.jobs.clone();
        log::info!("Starting mining loop with {} cores available...", num_cores);
        
        tokio::spawn(async move {
            loop {
                let mut jobs_guard = jobs.lock().await;
                let completed_jobs: Vec<String> = Vec::new();

                for (job_id, job) in jobs_guard.iter_mut() {
                    if job.status == JobStatus::Running {
                        if job.update_from_results() {
                            log::info!(
                                "Job {} finished with status {:?}, hashes: {}, time: {:?}",
                                job_id,
                                job.status,
                                job.total_hash_count,
                                job.start_time.elapsed()
                            );
                            
                            if job.status == JobStatus::Completed {
                                if let Some(ref result) = job.best_result {
                                    log::info!(
                                        "Best result - nonce: {}, distance: {}, work: {}",
                                        result.nonce,
                                        result.distance,
                                        hex::encode(result.work)
                                    );
                                }
                            }
                        }
                    }
                }

                // Clean up completed jobs that haven't been queried in a while
                // (This is optional - you might want to keep them longer for API queries)
                for _job_id in completed_jobs {
                    // We could add a timestamp check here to remove old completed jobs
                }

                drop(jobs_guard);
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        });
    }
}

fn mine_range(
    thread_id: usize,
    header_hash: [u8; 32],
    distance_threshold: U512,
    start: U512,
    end: U512,
    cancel_flag: Arc<AtomicBool>,
    sender: Sender<ThreadResult>,
) {
    log::debug!(
        "Thread {} mining range {} to {} (inclusive)",
        thread_id,
        start,
        end
    );

    let mut current_nonce = start;
    let mut hash_count = 0u64;
    let mut best_result: Option<MiningJobResult> = None;

    while current_nonce <= end && !cancel_flag.load(Ordering::Relaxed) {
        let nonce_bytes = current_nonce.to_big_endian();
        hash_count += 1;

        if is_valid_nonce(header_hash, nonce_bytes, distance_threshold) {
            let distance = get_nonce_distance(header_hash, nonce_bytes);
            
            let result = MiningJobResult {
                nonce: current_nonce,
                work: nonce_bytes,
                distance,
                hash_count,
            };

            log::info!(
                "Thread {} found valid nonce: {}, distance: {}",
                thread_id,
                current_nonce,
                distance
            );

            // Check if this is better than our current best
            let is_better = match &best_result {
                None => true,
                Some(current_best) => distance < current_best.distance,
            };

            if is_better {
                best_result = Some(result.clone());
            }

            // Send result immediately
            let thread_result = ThreadResult {
                thread_id,
                result: Some(result),
                hash_count,
                completed: false,
            };

            if sender.send(thread_result).is_err() {
                log::warn!("Thread {} failed to send result", thread_id);
                break;
            }

            // Continue mining to potentially find better results
            // until cancelled by main thread
        }

        current_nonce += U512::one();

        // Check for cancellation periodically (every 1000 hashes)
        if hash_count % 1000 == 0 && cancel_flag.load(Ordering::Relaxed) {
            break;
        }
    }

    // Send final completion status
    let final_result = ThreadResult {
        thread_id,
        result: best_result,
        hash_count,
        completed: true,
    };

    if sender.send(final_result).is_err() {
        log::warn!("Thread {} failed to send completion status", thread_id);
    }

    log::debug!("Thread {} completed, hashes: {}", thread_id, hash_count);
}

pub fn validate_mining_request(request: &MiningRequest) -> Result<(), String> {
    // Validate job_id
    if request.job_id.is_empty() {
        return Err("job_id cannot be empty".to_string());
    }

    // Validate mining_hash (should be 64 hex chars for 32 bytes)
    if request.mining_hash.len() != 64 {
        return Err("mining_hash must be 64 hex characters".to_string());
    }
    if hex::decode(&request.mining_hash).is_err() {
        return Err("mining_hash must be valid hex".to_string());
    }

    // Validate distance_threshold
    if U512::from_dec_str(&request.distance_threshold).is_err() {
        return Err("distance_threshold must be a valid decimal number".to_string());
    }

    // Validate nonce_start and nonce_end (should be 128 hex chars for 64 bytes)
    if request.nonce_start.len() != 128 {
        return Err("nonce_start must be 128 hex characters".to_string());
    }
    if request.nonce_end.len() != 128 {
        return Err("nonce_end must be 128 hex characters".to_string());
    }

    let nonce_start = U512::from_str_radix(&request.nonce_start, 16)
        .map_err(|_| "nonce_start must be valid hex".to_string())?;
    let nonce_end = U512::from_str_radix(&request.nonce_end, 16)
        .map_err(|_| "nonce_end must be valid hex".to_string())?;

    if nonce_start > nonce_end {
        return Err("nonce_start must be <= nonce_end".to_string());
    }

    Ok(())
}

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
    let distance_threshold = U512::from_dec_str(&request.distance_threshold).unwrap();
    let nonce_start = U512::from_str_radix(&request.nonce_start, 16).unwrap();
    let nonce_end = U512::from_str_radix(&request.nonce_end, 16).unwrap();

    let job = MiningJob::new(header_hash, distance_threshold, nonce_start, nonce_end);
    
    // Use num_cpus as default, but this could be made configurable per request
    let num_cores = num_cpus::get();

    match state.add_job(request.job_id.clone(), job, num_cores).await {
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
                warp::http::StatusCode::CONFLICT,
            ))
        }
    }
}

pub async fn handle_result_request(
    job_id: String,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
    log::debug!("Received result request for job: {}", job_id);

    let job = match state.get_job(&job_id).await {
        Some(job) => job,
        None => {
            log::warn!("Result request for unknown job: {}", job_id);
            return Ok(warp::reply::with_status(
                warp::reply::json(&resonance_miner_api::MiningResult {
                    status: ApiResponseStatus::NotFound,
                    job_id,
                    nonce: None,
                    work: None,
                    hash_count: 0,
                    elapsed_time: 0.0,
                }),
                warp::http::StatusCode::NOT_FOUND,
            ));
        }
    };

    let api_status = match job.status {
        JobStatus::Running => ApiResponseStatus::Running,
        JobStatus::Completed => ApiResponseStatus::Completed,
        JobStatus::Failed => ApiResponseStatus::Failed,
        JobStatus::Cancelled => ApiResponseStatus::Cancelled,
    };

    let (nonce, work) = match &job.best_result {
        Some(result) => (
            Some(format!("{:x}", result.nonce)),
            Some(hex::encode(result.work)),
        ),
        None => (None, None),
    };

    let elapsed_time = job.start_time.elapsed().as_secs_f64();

    Ok(warp::reply::with_status(
        warp::reply::json(&resonance_miner_api::MiningResult {
            status: api_status,
            job_id,
            nonce,
            work,
            hash_count: job.total_hash_count,
            elapsed_time,
        }),
        warp::http::StatusCode::OK,
    ))
}

pub async fn handle_cancel_request(
    job_id: String,
    state: MiningState,
) -> Result<impl Reply, Rejection> {
    log::debug!("Received cancel request for job: {}", job_id);

    let cancelled = state.cancel_job(&job_id).await;

    if cancelled {
        log::info!("Successfully cancelled job: {}", job_id);
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: ApiResponseStatus::Cancelled,
                job_id,
                message: None,
            }),
            warp::http::StatusCode::OK,
        ))
    } else {
        log::warn!("Cancel request for unknown job: {}", job_id);
        Ok(warp::reply::with_status(
            warp::reply::json(&MiningResponse {
                status: ApiResponseStatus::NotFound,
                job_id,
                message: Some("Job not found".to_string()),
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
        let valid_request = MiningRequest {
            job_id: "test_job".to_string(),
            mining_hash: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            distance_threshold: "1000000".to_string(),
            nonce_start: "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".to_string(),
            nonce_end: "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ffff".to_string(),
        };

        let result = validate_mining_request(&valid_request);
        if let Err(e) = &result {
            println!("Validation error: {}", e);
        }
        assert!(result.is_ok());

        // Test invalid job_id
        let mut invalid_request = valid_request.clone();
        invalid_request.job_id = "".to_string();
        assert!(validate_mining_request(&invalid_request).is_err());

        // Test invalid mining_hash length
        invalid_request = valid_request.clone();
        invalid_request.mining_hash = "short".to_string();
        assert!(validate_mining_request(&invalid_request).is_err());

        // Test invalid nonce range
        invalid_request = valid_request.clone();
        invalid_request.nonce_start = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ffff".to_string();
        invalid_request.nonce_end = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".to_string();
        assert!(validate_mining_request(&invalid_request).is_err());
    }

    #[tokio::test]
    async fn test_mining_state() {
        let state = MiningState::new();
        let job = MiningJob::new(
            [1u8; 32],
            U512::from(1000000u64),
            U512::zero(),
            U512::from(1000u64),
        );

        // Test adding job
        assert!(state.add_job("test".to_string(), job, 1).await.is_ok());

        // Test duplicate job
        let job2 = MiningJob::new(
            [2u8; 32],
            U512::from(1000000u64),
            U512::zero(),
            U512::from(1000u64),
        );
        assert!(state.add_job("test".to_string(), job2, 1).await.is_err());

        // Test getting job
        assert!(state.get_job("test").await.is_some());
        assert!(state.get_job("nonexistent").await.is_none());

        // Test removing job
        assert!(state.remove_job("test").await.is_some());
        assert!(state.remove_job("nonexistent").await.is_none());
    }
}