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
}

#[derive(Clone)]
pub struct MiningState {
    pub jobs: Arc<Mutex<HashMap<String, MiningJob>>>,
    pub num_cores: usize,
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
    completed_threads: usize,
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
            completed_threads: 0,
        }
    }

    pub fn start_mining(&mut self, num_cores: usize) {
        let (sender, receiver) = bounded(num_cores * 2);
        self.result_receiver = Some(receiver);

        let total_range = (self.nonce_end - self.nonce_start).saturating_add(U512::from(1));
        let range_per_core = total_range / U512::from(num_cores as u64);
        let remainder = total_range % U512::from(num_cores as u64);

        log::info!(
            "Starting mining with {} cores, total range: {}, range per core: {}",
            num_cores,
            total_range,
            range_per_core
        );

        for thread_id in 0..num_cores {
            let start = self.nonce_start + range_per_core * U512::from(thread_id as u64);
            let mut end = start + range_per_core - U512::from(1);

            if thread_id == num_cores - 1 {
                end = end + remainder;
            }

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

        while let Some(handle) = self.thread_handles.pop() {
            let _ = handle.join();
        }
    }

    pub fn update_from_results(&mut self) -> bool {
        let receiver = match &self.result_receiver {
            Some(r) => r,
            None => return false,
        };

        while let Ok(thread_result) = receiver.try_recv() {
            self.total_hash_count += thread_result.hash_count;

            if thread_result.completed {
                self.completed_threads += 1;
            }

            if let Some(result) = thread_result.result {
                let is_better = self
                    .best_result
                    .as_ref()
                    .map_or(true, |current_best| result.distance < current_best.distance);

                if is_better {
                    log::info!(
                        "Found better result from thread {}: distance = {}, nonce = {}",
                        thread_result.thread_id,
                        result.distance,
                        result.nonce
                    );
                    self.best_result = Some(result);
                    self.cancel_flag.store(true, Ordering::Relaxed);
                }
            }
        }

        if self.status == JobStatus::Running {
            if self.best_result.is_some() {
                self.status = JobStatus::Completed;
            } else if self.completed_threads >= self.thread_handles.len()
                && !self.thread_handles.is_empty()
            {
                self.status = JobStatus::Failed;
            }
        }

        self.status != JobStatus::Running
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
            result_receiver: None,
            thread_handles: Vec::new(),
            completed_threads: self.completed_threads,
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
            num_cores: num_cpus::get(),
        }
    }

    pub async fn add_job(&self, job_id: String, mut job: MiningJob) -> Result<(), String> {
        let mut jobs = self.jobs.lock().await;
        if jobs.contains_key(&job_id) {
            log::warn!("Attempted to add duplicate job ID: {}", job_id);
            return Err("Job already exists".to_string());
        }

        log::info!("Adding job: {} with {} cores", job_id, self.num_cores);
        job.start_mining(self.num_cores);
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

    pub async fn start_mining_loop(&self) {
        let jobs = self.jobs.clone();
        log::info!("Starting mining loop...");

        tokio::spawn(async move {
            loop {
                let mut jobs_guard = jobs.lock().await;

                jobs_guard.retain(|job_id, job| {
                    if job.status == JobStatus::Running {
                        if job.update_from_results() {
                            log::info!(
                                "Job {} finished with status {:?}, hashes: {}, time: {:?}",
                                job_id,
                                job.status,
                                job.total_hash_count,
                                job.start_time.elapsed()
                            );
                        }
                    }

                    // Retain jobs that are running or recently finished (e.g., within 5 minutes)
                    let retain = job.status == JobStatus::Running
                        || job.start_time.elapsed().as_secs() < 300;
                    if !retain {
                        log::info!("Cleaning up old job {}", job_id);
                    }
                    retain
                });

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

    while current_nonce <= end && !cancel_flag.load(Ordering::Relaxed) {
        let nonce_bytes = current_nonce.to_big_endian();
        hash_count += 1;

        if is_valid_nonce(header_hash, nonce_bytes, distance_threshold) {
            let distance = get_nonce_distance(header_hash, nonce_bytes);

            let result = MiningJobResult {
                nonce: current_nonce,
                work: nonce_bytes,
                distance,
            };

            log::info!(
                "Thread {} found valid nonce: {}, distance: {}",
                thread_id,
                current_nonce,
                distance
            );

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
            hash_count = 0;
        }

        current_nonce += U512::from(1);

        if hash_count > 0 && hash_count % 4096 == 0 {
            if cancel_flag.load(Ordering::Relaxed) {
                break;
            }
        }
    }

    let final_result = ThreadResult {
        thread_id,
        result: None,
        hash_count,
        completed: true,
    };

    if sender.send(final_result).is_err() {
        log::warn!("Thread {} failed to send completion status", thread_id);
    }

    log::debug!("Thread {} completed.", thread_id);
}

pub fn validate_mining_request(request: &MiningRequest) -> Result<(), String> {
    if request.job_id.is_empty() {
        return Err("job_id cannot be empty".to_string());
    }

    if request.mining_hash.len() != 64 {
        return Err("mining_hash must be 64 hex characters".to_string());
    }
    if hex::decode(&request.mining_hash).is_err() {
        return Err("mining_hash must be valid hex".to_string());
    }

    if U512::from_dec_str(&request.distance_threshold).is_err() {
        return Err("distance_threshold must be a valid decimal number".to_string());
    }

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

    let header_hash: [u8; 32] = hex::decode(&request.mining_hash)
        .unwrap()
        .try_into()
        .expect("Validated hex string is 32 bytes");
    let distance_threshold = U512::from_dec_str(&request.distance_threshold).unwrap();
    let nonce_start = U512::from_str_radix(&request.nonce_start, 16).unwrap();
    let nonce_end = U512::from_str_radix(&request.nonce_end, 16).unwrap();

    let job = MiningJob::new(header_hash, distance_threshold, nonce_start, nonce_end);

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

    if state.cancel_job(&job_id).await {
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
    use tokio::time::{sleep, Duration};

    #[test]
    fn test_validate_mining_request() {
        let valid_request = MiningRequest {
            job_id: "test_job".to_string(),
            mining_hash: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .to_string(),
            distance_threshold: "1000000".to_string(),
            nonce_start: "0".repeat(128),
            nonce_end: "f".repeat(128),
        };
        assert!(validate_mining_request(&valid_request).is_ok());
    }

    #[tokio::test]
    async fn test_mining_state_add_get_remove() {
        let state = MiningState::new();
        let job = MiningJob::new(
            [1u8; 32],
            U512::from(1000000u64),
            U512::zero(),
            U512::from(1000u64),
        );
        assert!(state.add_job("test".to_string(), job).await.is_ok());
        assert!(state.get_job("test").await.is_some());
        assert!(state.remove_job("test").await.is_some());
        assert!(state.get_job("test").await.is_none());
    }

    #[tokio::test]
    async fn test_job_lifecycle_fail() {
        // Test that a job fails if no nonce is found
        let state = MiningState::new();
        state.start_mining_loop().await;

        // Impossible threshold
        let header_hash = [1u8; 32];
        let distance_threshold = U512::zero();
        let nonce_start = U512::from(0);
        let nonce_end = U512::from(100);

        let job = MiningJob::new(header_hash, distance_threshold, nonce_start, nonce_end);
        state.add_job("fail_job".to_string(), job).await.unwrap();

        // Give the mining loop time to process the job
        sleep(Duration::from_millis(500)).await;

        let finished_job = state.get_job("fail_job").await.unwrap();
        assert_eq!(finished_job.status, JobStatus::Failed);
        assert!(finished_job.total_hash_count > 0);
    }

    #[tokio::test]
    async fn test_job_lifecycle_success() {
        // Test that a job completes successfully
        let state = MiningState::new();
        state.start_mining_loop().await;

        // Easy threshold
        let header_hash = [1u8; 32];
        let distance_threshold = U512::MAX; // Easiest difficulty
        let nonce_start = U512::from(0);
        let nonce_end = U512::from(10000);

        let job = MiningJob::new(header_hash, distance_threshold, nonce_start, nonce_end);
        state.add_job("success_job".to_string(), job).await.unwrap();

        // Give the mining loop time to find a solution
        sleep(Duration::from_millis(500)).await;

        let finished_job = state.get_job("success_job").await.unwrap();
        assert_eq!(finished_job.status, JobStatus::Completed);
        assert!(finished_job.best_result.is_some());
    }
}
