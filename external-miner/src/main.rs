use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use warp::Filter;
use serde::{Deserialize, Serialize};
use hex;
use primitive_types::H256;
use sp_core::U512;
use log::info;
use codec::{Encode, Decode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct QPoWSeal {
    pub nonce: [u8; 64],
}

#[derive(Serialize, Deserialize, Debug)]
struct MiningRequest {
    job_id: String,
    mining_hash: String,
    difficulty: String,
    nonce_start: String,
    nonce_end: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct MiningResponse {
    status: String,
    job_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct MiningResult {
    status: String,
    job_id: String,
    nonce: Option<String>,
    work: Option<String>,
}

#[derive(Clone)]
struct MiningState {
    jobs: Arc<Mutex<HashMap<String, MiningJob>>>,
}

#[derive(Debug)]
struct MiningJob {
    mining_hash: H256,
    difficulty: u64,
    nonce_start: U512,
    nonce_end: U512,
    current_nonce: U512,
    status: String,
}

impl MiningState {
    fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    info!("Starting external miner service...");

    let state = MiningState::new();
    let state_filter = warp::any().map(move || state.clone());

    // POST /mine - Submit a new mining job
    let mine_route = warp::post()
        .and(warp::path("mine"))
        .and(warp::body::json())
        .and(state_filter.clone())
        .and_then(handle_mine_request);

    // GET /result/{job_id} - Check mining result
    let result_route = warp::get()
        .and(warp::path("result"))
        .and(warp::path::param())
        .and(state_filter.clone())
        .and_then(handle_result_request);

    // POST /cancel/{job_id} - Cancel a mining job
    let cancel_route = warp::post()
        .and(warp::path("cancel"))
        .and(warp::path::param())
        .and(state_filter.clone())
        .and_then(handle_cancel_request);

    let routes = mine_route.or(result_route).or(cancel_route);

    info!("External miner service listening on 127.0.0.1:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn handle_mine_request(
    request: MiningRequest,
    state: MiningState,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mining_hash = match hex::decode(&request.mining_hash[2..]) {
        Ok(bytes) => {
            if bytes.len() != 32 {
                return Ok(warp::reply::json(&MiningResponse {
                    status: "error".to_string(),
                    job_id: request.job_id,
                }));
            }
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&bytes);
            H256(hash)
        }
        Err(_) => {
            return Ok(warp::reply::json(&MiningResponse {
                status: "error".to_string(),
                job_id: request.job_id,
            }));
        }
    };

    let difficulty = match request.difficulty.parse::<u64>() {
        Ok(d) => d,
        Err(_) => {
            return Ok(warp::reply::json(&MiningResponse {
                status: "error".to_string(),
                job_id: request.job_id,
            }));
        }
    };

    let nonce_start = match hex::decode(&request.nonce_start[2..]) {
        Ok(bytes) => U512::from_big_endian(&bytes),
        Err(_) => {
            return Ok(warp::reply::json(&MiningResponse {
                status: "error".to_string(),
                job_id: request.job_id,
            }));
        }
    };

    let nonce_end = match hex::decode(&request.nonce_end[2..]) {
        Ok(bytes) => U512::from_big_endian(&bytes),
        Err(_) => {
            return Ok(warp::reply::json(&MiningResponse {
                status: "error".to_string(),
                job_id: request.job_id,
            }));
        }
    };

    let job = MiningJob {
        mining_hash,
        difficulty,
        nonce_start,
        nonce_end,
        current_nonce: nonce_start,
        status: "working".to_string(),
    };

    state.jobs.lock().await.insert(request.job_id.clone(), job);

    Ok(warp::reply::json(&MiningResponse {
        status: "accepted".to_string(),
        job_id: request.job_id,
    }))
}

async fn handle_result_request(
    job_id: String,
    state: MiningState,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut jobs = state.jobs.lock().await;
    
    if let Some(job) = jobs.get_mut(&job_id) {
        if job.current_nonce >= job.nonce_end {
            job.status = "stale".to_string();
            return Ok(warp::reply::json(&MiningResult {
                status: "stale".to_string(),
                job_id,
                nonce: None,
                work: None,
            }));
        }

        // Here you would implement the actual mining algorithm
        // For now, we'll just increment the nonce
        let current_nonce = job.current_nonce;
        job.current_nonce += U512::one();

        if current_nonce % U512::from(100) == U512::zero() {
            // Simulate finding a valid nonce
            let nonce_bytes = current_nonce.to_big_endian();
            let seal = QPoWSeal { nonce: nonce_bytes };
            
            return Ok(warp::reply::json(&MiningResult {
                status: "found".to_string(),
                job_id,
                nonce: Some(format!("0x{}", hex::encode(nonce_bytes))),
                work: Some(format!("0x{}", hex::encode(seal.encode()))),
            }));
        }

        Ok(warp::reply::json(&MiningResult {
            status: "working".to_string(),
            job_id,
            nonce: None,
            work: None,
        }))
    } else {
        Ok(warp::reply::json(&MiningResult {
            status: "not_found".to_string(),
            job_id,
            nonce: None,
            work: None,
        }))
    }
}

async fn handle_cancel_request(
    job_id: String,
    state: MiningState,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut jobs = state.jobs.lock().await;
    
    if let Some(job) = jobs.get_mut(&job_id) {
        job.status = "cancelled".to_string();
        jobs.remove(&job_id);
        
        Ok(warp::reply::json(&MiningResponse {
            status: "cancelled".to_string(),
            job_id,
        }))
    } else {
        Ok(warp::reply::json(&MiningResponse {
            status: "not_found".to_string(),
            job_id,
        }))
    }
} 