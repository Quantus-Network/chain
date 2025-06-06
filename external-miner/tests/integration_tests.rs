use external_miner::*;
use primitive_types::U512;
use resonance_miner_api::*;
use warp::test::request;
use warp::Filter;

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
        distance_threshold: "1000".to_string(),
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
    assert_eq!(body.status, ApiResponseStatus::Accepted);
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
    assert_eq!(body.status, ApiResponseStatus::Error);
    assert!(body.message.is_some());
    assert!(body.message.unwrap().contains("Job already exists"));

    // Test invalid request
    let invalid_request = MiningRequest {
        job_id: "".to_string(), // Empty job ID
        mining_hash: "a".repeat(64),
        distance_threshold: "1000".to_string(),
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

    // Create a job directly using the new structure
    let job = MiningJob::new(
        [0; 32],
        U512::from(1000),
        U512::from(0),
        U512::from(1000),
    );
    state.add_job("test".to_string(), job, 1).await.unwrap();

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
    assert_eq!(body.status, ApiResponseStatus::Running);
    assert_eq!(body.job_id, "test");

    // Test non-existent job
    let resp = request()
        .method("GET")
        .path("/result/nonexistent")
        .reply(&result_route)
        .await;

    assert_eq!(resp.status(), 404);
    let body: MiningResult = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body.status, ApiResponseStatus::NotFound);
}

#[tokio::test]
async fn test_cancel_endpoint() {
    let state = MiningState::new();
    let state_clone = state.clone();
    let state_filter = warp::any().map(move || state_clone.clone());

    // Create a job with a large range that won't finish quickly
    let job = MiningJob::new(
        [1; 32],
        U512::from(1),  // Very low threshold - will likely fail
        U512::from(0),
        U512::from(100000), // Large range
    );
    state.add_job("test".to_string(), job, 1).await.unwrap();

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
    assert_eq!(body.status, ApiResponseStatus::Cancelled);
    assert_eq!(body.job_id, "test");

    // Test cancel non-existent job
    let resp = request()
        .method("POST")
        .path("/cancel/nonexistent")
        .reply(&cancel_route)
        .await;

    assert_eq!(resp.status(), 404);
    let body: MiningResponse = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body.status, ApiResponseStatus::NotFound);
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
            let job = MiningJob::new(
                [i as u8; 32],
                U512::from(1000),
                U512::from(0),
                U512::from(1000),
            );
            state.add_job(format!("test{}", i), job, 1).await
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
        // Status could be Running, Completed, or Failed depending on timing
        assert!(
            body.status == ApiResponseStatus::Running ||
            body.status == ApiResponseStatus::Completed ||
            body.status == ApiResponseStatus::Failed
        );
    }
}

#[tokio::test]
async fn test_multi_core_mining() {
    let state = MiningState::new();
    
    // Start the mining loop
    state.start_mining_loop(4).await;
    
    // Create a job with multiple cores
    let job = MiningJob::new(
        [2; 32],
        U512::MAX, // High threshold to potentially find a result
        U512::from(0),
        U512::from(1000),
    );
    
    state.add_job("multicore_test".to_string(), job, 4).await.unwrap();
    
    // Wait longer for mining to potentially find something or fail
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
    
    let job_result = state.get_job("multicore_test").await;
    assert!(job_result.is_some());
    
    let job = job_result.unwrap();
    // Job should have moved beyond Running status
    assert!(
        job.status == JobStatus::Completed ||
        job.status == JobStatus::Failed ||
        job.status == JobStatus::Cancelled
    );
    
    // Should have attempted some hashes
    assert!(job.total_hash_count > 0);
}

#[tokio::test]
async fn test_job_cancellation_during_mining() {
    let state = MiningState::new();
    
    // Start the mining loop
    state.start_mining_loop(2).await;
    
    // Create a job with a very large range
    let job = MiningJob::new(
        [3; 32],
        U512::from(1), // Very difficult - unlikely to complete quickly
        U512::from(0),
        U512::from(1000000), // Large range
    );
    
    state.add_job("cancel_test".to_string(), job, 2).await.unwrap();
    
    // Let it run briefly
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Cancel the job
    let cancelled = state.cancel_job("cancel_test").await;
    assert!(cancelled);
    
    // Check that job status is cancelled
    let job_result = state.get_job("cancel_test").await;
    assert!(job_result.is_some());
    let job = job_result.unwrap();
    assert_eq!(job.status, JobStatus::Cancelled);
}