// Verifying that our API spec is in sync with the implementation
// Manually generated from the OpenAPI spec but it should still flag when we get out of sync and need to update.

use external_miner::*;
use warp::test::request;
use warp::Filter;
use resonance_miner_api::*; // Use the shared API types
use primitive_types::U512;
use serde_json::json; // For sending custom/invalid JSON

// Helper function to setup the routes with a fresh state for each test
fn setup_routes() -> (MiningState, impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone) {
    let state = MiningState::new();
    let state_clone = state.clone();
    let state_filter = warp::any().map(move || state_clone.clone());

    let mine_route = warp::post()
        .and(warp::path("mine"))
        .and(warp::body::json::<MiningRequest>()) // Expect valid MiningRequest
        .and(state_filter.clone())
        .and_then(handle_mine_request);

    let result_route = warp::get()
        .and(warp::path("result"))
        .and(warp::path::param::<String>())
        .and(state_filter.clone())
        .and_then(handle_result_request);

    let cancel_route = warp::post()
        .and(warp::path("cancel"))
        .and(warp::path::param::<String>())
        .and(state_filter.clone())
        .and_then(handle_cancel_request);

    let routes = mine_route.or(result_route).or(cancel_route);
    (state, routes)
}

// --- /mine Endpoint Tests --- 

#[tokio::test]
async fn spec_mine_valid_request() {
    let (_state, routes) = setup_routes();
    let valid_req = MiningRequest {
        job_id: "job-valid-1".to_string(),
        mining_hash: "a".repeat(64),
        difficulty: "12345".to_string(),
        nonce_start: "0".repeat(128),
        nonce_end: "f".repeat(128),
    };

    let resp = request()
        .method("POST")
        .path("/mine")
        .json(&valid_req)
        .reply(&routes)
        .await;

    // Spec: 200 OK, response matches MiningResponseAccepted
    assert_eq!(resp.status(), 200, "Spec requires 200 OK for valid /mine request");
    let body: MiningResponse = serde_json::from_slice(resp.body()).expect("Valid response should deserialize to MiningResponse");
    assert_eq!(body.status, ApiResponseStatus::Accepted, "Spec requires status 'accepted'");
    assert_eq!(body.job_id, valid_req.job_id);
    assert!(body.message.is_none(), "Spec shows no message field for success");
}

#[tokio::test]
async fn spec_mine_duplicate_job_id() {
    let (state, routes) = setup_routes();
    let valid_req = MiningRequest {
        job_id: "job-duplicate-1".to_string(),
        mining_hash: "b".repeat(64),
        difficulty: "54321".to_string(),
        nonce_start: "1".repeat(128),
        nonce_end: "e".repeat(128),
    };

    // Add the job first
    let job = MiningJob::new(
        hex::decode(valid_req.mining_hash.clone()).unwrap().try_into().unwrap(),
        valid_req.difficulty.parse().unwrap(),
        U512::from_str_radix(&valid_req.nonce_start, 16).unwrap(),
        U512::from_str_radix(&valid_req.nonce_end, 16).unwrap()
    );
    state.add_job(valid_req.job_id.clone(), job).await.unwrap();

    // Try to submit again
    let resp = request()
        .method("POST")
        .path("/mine")
        .json(&valid_req)
        .reply(&routes)
        .await;

    // Spec: 409 Conflict, response matches MiningResponseError
    assert_eq!(resp.status(), 409, "Spec requires 409 Conflict for duplicate job ID");
    let body: MiningResponse = serde_json::from_slice(resp.body()).expect("Valid error response should deserialize to MiningResponse");
    assert_eq!(body.status, ApiResponseStatus::Error, "Spec requires status 'error'");
    assert!(body.message.is_some(), "Spec requires message field for error");
    assert!(body.message.unwrap().contains("Job already exists"));
    assert_eq!(body.job_id, valid_req.job_id);
}

// Test cases for invalid formats (hash length, nonce length, difficulty non-numeric)
// These should ideally result in 400 Bad Request
#[tokio::test]
async fn spec_mine_invalid_hash_format() {
    let (_state, routes) = setup_routes();
    let invalid_req = json!({
        "job_id": "job-invalid-hash",
        "mining_hash": "a".repeat(63), // Too short
        "difficulty": "1000",
        "nonce_start": "0".repeat(128),
        "nonce_end": "f".repeat(128)
    });

    let resp = request()
        .method("POST")
        .path("/mine")
        .json(&invalid_req)
        .reply(&routes)
        .await;
    
    // Spec: 400 Bad Request, response matches MiningResponseError
    assert_eq!(resp.status(), 400, "Spec requires 400 Bad Request for invalid hash format");
    let body: MiningResponse = serde_json::from_slice(resp.body()).expect("Error response should deserialize");
    assert_eq!(body.status, ApiResponseStatus::Error);
    assert!(body.message.is_some());
    assert!(body.message.unwrap().contains("Invalid mining_hash"));
}

// Add similar tests for invalid nonce_start, nonce_end, difficulty format...

#[tokio::test]
async fn spec_mine_missing_required_field() {
    let (_state, routes) = setup_routes();
    // Missing 'difficulty'
    let invalid_req = json!({
        "job_id": "job-missing-field",
        "mining_hash": "c".repeat(64),
        "nonce_start": "0".repeat(128),
        "nonce_end": "f".repeat(128)
    });

    let resp = request()
        .method("POST")
        .path("/mine")
        .json(&invalid_req)
        .reply(&routes)
        .await;
    
    // Warp's default JSON body deserialization handles missing fields
    // It should return 400 Bad Request based on warp::body::json()
    assert_eq!(resp.status(), 400, "Spec requires 400 Bad Request for missing required field");
    // The body might be a plain text error from Warp, not necessarily our JSON structure
    // We can check if the body indicates a deserialization error
    let body_bytes = resp.body();
    let body_string = String::from_utf8_lossy(body_bytes);
    assert!(body_string.contains("Failed to deserialize") || body_string.contains("missing field"), "Response body should indicate deserialization failure");
}


// --- /result/{job_id} Endpoint Tests --- 

#[tokio::test]
async fn spec_result_job_running() {
    let (state, routes) = setup_routes();
    let job_id = "job-running-1".to_string();
    // Add a job
    let job = MiningJob::new(
        [1u8; 32],
        1000,
        U512::from(0),
        U512::from(1000),
    );
    state.add_job(job_id.clone(), job).await.unwrap();

    let resp = request()
        .method("GET")
        .path(&format!("/result/{}", job_id))
        .reply(&routes)
        .await;

    // Spec: 200 OK, response matches MiningResult with status 'running'
    assert_eq!(resp.status(), 200);
    let body: MiningResult = serde_json::from_slice(resp.body()).expect("Should deserialize to MiningResult");
    assert_eq!(body.status, ApiResponseStatus::Running);
    assert_eq!(body.job_id, job_id);
    assert!(body.nonce.is_some()); // Should show current nonce
    assert!(body.work.is_none());   // No work yet
    // hash_count and elapsed_time should be present
    // assert_eq!(body.hash_count, 0); // Initial state
    // assert!(body.elapsed_time >= 0.0);
}

// Add tests for spec_result_job_completed, spec_result_job_failed (need to manipulate state)
// Need to carefully simulate a completed/failed state or run miner loop briefly

#[tokio::test]
async fn spec_result_job_not_found() {
    let (_state, routes) = setup_routes();
    let job_id = "job-not-found-1";

    let resp = request()
        .method("GET")
        .path(&format!("/result/{}", job_id))
        .reply(&routes)
        .await;

    // Spec: 404 Not Found, response matches MiningResultNotFound
    assert_eq!(resp.status(), 404);
    let body: MiningResult = serde_json::from_slice(resp.body()).expect("Should deserialize to MiningResult (NotFound variant)");
    assert_eq!(body.status, ApiResponseStatus::NotFound);
    assert_eq!(body.job_id, job_id);
    assert!(body.nonce.is_none());
    assert!(body.work.is_none());
    assert_eq!(body.hash_count, 0);
    assert_eq!(body.elapsed_time, 0.0);
}


// --- /cancel/{job_id} Endpoint Tests --- 

#[tokio::test]
async fn spec_cancel_existing_job() {
    let (state, routes) = setup_routes();
    let job_id = "job-cancel-1".to_string();
    // Add a job
    let job = MiningJob::new([2u8; 32], 2000, U512::from(0), U512::from(500));
    state.add_job(job_id.clone(), job).await.unwrap();

    let resp = request()
        .method("POST") // POST for cancel
        .path(&format!("/cancel/{}", job_id))
        .reply(&routes)
        .await;

    // Spec: 200 OK, response matches MiningResponseCancelled
    assert_eq!(resp.status(), 200);
    let body: MiningResponse = serde_json::from_slice(resp.body()).expect("Should deserialize to MiningResponse");
    assert_eq!(body.status, ApiResponseStatus::Cancelled);
    assert_eq!(body.job_id, job_id);
    assert!(body.message.is_none());

    // Verify job is actually removed
    assert!(state.get_job(&job_id).await.is_none());
}

#[tokio::test]
async fn spec_cancel_non_existent_job() {
    let (_state, routes) = setup_routes();
    let job_id = "job-cancel-not-found-1";

    let resp = request()
        .method("POST")
        .path(&format!("/cancel/{}", job_id))
        .reply(&routes)
        .await;

    // Spec: 404 Not Found, response matches MiningResponseNotFound
    assert_eq!(resp.status(), 404);
    let body: MiningResponse = serde_json::from_slice(resp.body()).expect("Should deserialize to MiningResponse (NotFound variant)");
    assert_eq!(body.status, ApiResponseStatus::NotFound);
    assert_eq!(body.job_id, job_id);
    assert!(body.message.is_none());
}

// Removed placeholder test 