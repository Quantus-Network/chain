use clap::Parser;
use external_miner::*; // Import everything from lib.rs
use log::info;
use std::net::SocketAddr;
use warp::Filter;

/// Resonance External Miner Service
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port number to listen on
    #[arg(short, long, env = "MINER_PORT", default_value_t = 9833)]
    port: u16,
}

#[tokio::main]
async fn main() {
    let args = Args::parse(); // Parse args - this handles --help and --version
    env_logger::init(); // Initialize logger after parsing args
    info!("Starting external miner service...");

    // Use MiningState from lib.rs
    let state = MiningState::new();

    // --- Start the mining loop ---
    state.start_mining_loop().await;

    // --- Set up Warp filters ---
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

    // Use the port from parsed arguments
    let addr: SocketAddr = ([0, 0, 0, 0], args.port).into();
    info!("Server starting on {}", addr);
    warp::serve(routes).run(addr).await;
}
