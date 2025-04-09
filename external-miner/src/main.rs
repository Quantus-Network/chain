use warp::Filter;
use log::info;
use external_miner::*; // Import everything from lib.rs
use std::net::SocketAddr;
use env_logger;

#[tokio::main]
async fn main() {
    env_logger::init();
    info!("Starting external miner service...");

    // Use MiningState from lib.rs
    let state = MiningState::new();
    let state_clone = state.clone(); // Clone state for the filter closure
    let state_filter = warp::any().map(move || state_clone.clone());

    // Use handle_mine_request from lib.rs
    let mine_route = warp::post()
        .and(warp::path("mine"))
        .and(warp::body::json()) // Expect MiningRequest from lib.rs
        .and(state_filter.clone())
        .and_then(handle_mine_request);

    // Use handle_result_request from lib.rs
    let result_route = warp::get()
        .and(warp::path("result"))
        .and(warp::path::param())
        .and(state_filter.clone())
        .and_then(handle_result_request);

    // Use handle_cancel_request from lib.rs
    let cancel_route = warp::post()
        .and(warp::path("cancel"))
        .and(warp::path::param())
        .and(state_filter.clone())
        .and_then(handle_cancel_request);

    let routes = mine_route.or(result_route).or(cancel_route);

    let addr: SocketAddr = ([0, 0, 0, 0], 3000).into();
    info!("Server starting on {}", addr);
    warp::serve(routes).run(addr).await;
}
