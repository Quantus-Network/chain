use external_miner::*;
use warp::test::request;
use warp::Filter;
use primitive_types::U512;

#[tokio::test]
async fn test_mine_endpoint() {
    let state = MiningState::new();
    let state_clone = state.clone();
    let state_filter = warp::any().map(move || state_clone.clone());

    let mine_route = warp::post()
        .and(warp::path("mine"))
        .and(warp::body::json())
        .and(state_filter.clone())
        .and_then(handle_mine_request);

    // Test valid request
    let valid_request = MiningRequest {
        job_id: "test".to_string(),
        mining_hash: "a".repeat(64),
        difficulty: "1000".to_string(),
        nonce_start: "0".repeat(128),
        nonce_end: "1".repeat(128),
    };

    let resp = request()
        .method("POST")
        .path("/mine")
        .json(&valid_request)
        .reply(&mine_route)
        .await;

    assert_eq!(resp.status(), 200);
    let body: MiningResponse = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body.status, "accepted");
    assert_eq!(body.job_id, "test");

    // Test duplicate job ID
    let resp = request()
        .method("POST")
        .path("/mine")
        .json(&valid_request)
        .reply(&mine_route)
        .await;

    assert_eq!(resp.status(), 409); 
    let body: MiningResponse = serde_json::from_slice(resp.body()).unwrap();
    assert!(body.status.contains("error"));
    assert!(body.status.contains("Job already exists")); // Be more specific if possible

    // Test invalid request
    let invalid_request = MiningRequest {
        job_id: "".to_string(), // Empty job ID
        mining_hash: "a".repeat(64),
        difficulty: "1000".to_string(),
        nonce_start: "0".repeat(128),
        nonce_end: "1".repeat(128),
    };

    let resp = request()
        .method("POST")
        .path("/mine")
        .json(&invalid_request)
        .reply(&mine_route)
        .await;

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_result_endpoint() {
    let state = MiningState::new();
    let state_clone = state.clone();
    let state_filter = warp::any().map(move || state_clone.clone());

    // First create a job
    let job = MiningJob {
        header_hash: [0; 32],
        difficulty: 1000,
        nonce_start: U512::from(0),
        nonce_end: U512::from(1000),
        current_nonce: U512::from(0),
        status: "running".to_string(),
        hash_count: 0,
        start_time: std::time::Instant::now(),
    };
    state.add_job("test".to_string(), job).await.unwrap();

    let result_route = warp::get()
        .and(warp::path("result"))
        .and(warp::path::param())
        .and(state_filter.clone())
        .and_then(handle_result_request);

    // Test existing job
    let resp = request()
        .method("GET")
        .path("/result/test")
        .reply(&result_route)
        .await;

    assert_eq!(resp.status(), 200);
    let body: MiningResult = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body.status, "running");
    assert_eq!(body.job_id, "test");

    // Test non-existent job
    let resp = request()
        .method("GET")
        .path("/result/nonexistent")
        .reply(&result_route)
        .await;

    assert_eq!(resp.status(), 404);
    let body: MiningResult = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body.status, "not_found");
}

#[tokio::test]
async fn test_cancel_endpoint() {
    let state = MiningState::new();
    let state_clone = state.clone();
    let state_filter = warp::any().map(move || state_clone.clone());

    // First create a job
    let job = MiningJob {
        header_hash: [0; 32],
        difficulty: 1000,
        nonce_start: U512::from(0),
        nonce_end: U512::from(1000),
        current_nonce: U512::from(0),
        status: "running".to_string(),
        hash_count: 0,
        start_time: std::time::Instant::now(),
    };
    state.add_job("test".to_string(), job).await.unwrap();

    let cancel_route = warp::post()
        .and(warp::path("cancel"))
        .and(warp::path::param())
        .and(state_filter.clone())
        .and_then(handle_cancel_request);

    // Test cancel existing job
    let resp = request()
        .method("POST")
        .path("/cancel/test")
        .reply(&cancel_route)
        .await;

    assert_eq!(resp.status(), 200);
    let body: MiningResponse = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body.status, "cancelled");
    assert_eq!(body.job_id, "test");

    // Test cancel non-existent job
    let resp = request()
        .method("POST")
        .path("/cancel/nonexistent")
        .reply(&cancel_route)
        .await;

    assert_eq!(resp.status(), 404);
    let body: MiningResponse = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body.status, "not_found");
}

#[tokio::test]
async fn test_concurrent_access() {
    let state = MiningState::new();
    let state_clone = state.clone();
    let state_filter = warp::any().map(move || state_clone.clone());

    // Create multiple jobs concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let state = state.clone();
        let handle = tokio::spawn(async move {
            let job = MiningJob {
                header_hash: [0; 32],
                difficulty: 1000,
                nonce_start: U512::from(0),
                nonce_end: U512::from(1000),
                current_nonce: U512::from(0),
                status: "running".to_string(),
                hash_count: 0,
                start_time: std::time::Instant::now(),
            };
            state.add_job(format!("test{}", i), job).await
        });
        handles.push(handle);
    }

    // Wait for all jobs to be created
    for handle in handles {
        assert!(handle.await.unwrap().is_ok());
    }

    // Verify all jobs exist
    for i in 0..10 {
        assert!(state.get_job(&format!("test{}", i)).await.is_some());
    }

    // Test concurrent result checks
    let result_route = warp::get()
        .and(warp::path("result"))
        .and(warp::path::param())
        .and(state_filter.clone())
        .and_then(handle_result_request);

    let mut result_handles = vec![];
    for i in 0..10 {
        let route = result_route.clone();
        let handle = tokio::spawn(async move {
            request()
                .method("GET")
                .path(&format!("/result/test{}", i))
                .reply(&route)
                .await
        });
        result_handles.push(handle);
    }

    // Verify all result checks succeed
    for handle in result_handles {
        let resp = handle.await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: MiningResult = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body.status, "running");
    }
}