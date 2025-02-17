use futures::executor::block_on;
use tokio::time::Duration;
use sc_service::TaskManager;
use resonance_runtime::Block;
use sp_runtime::traits::Block as BlockT;
use sp_core::H256;
use qpow::QPow;
use sp_core::U256;
use qpow::QPoWSeal;
use qpow::Compute;
use sp_core::Encode;
use crate::worker::WorkerHandle;
use std::sync::Arc;



fn try_nonce<B: BlockT>(
    pre_hash: B::Hash,
    nonce: u64,
    difficulty: U256,
) -> Result<QPoWSeal, ()> {

    let compute = Compute {
        difficulty,
        pre_hash: H256::from_slice(pre_hash.as_ref()),
        nonce,
    };

    // Compute the seal
    println!("compute difficulty: {:?}", difficulty);
    let seal = compute.compute();

    println!("compute done");

    // Convert pre_hash to [u8; 32] for verification
    // TODO normalize all the different ways we do calculations
    let header = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

    // Verify the solution using QPoW
    if !QPow::verify_solution(header, seal.work, difficulty.low_u64()) {
        println!("invalid seal");
        return Err(());
    }
    println!("good seal");

    // Print the hash rate in MH/s
    // let hashrate_mh = difficulty.low_u64() as f64 / 1_000_000.0; // Assuming difficulty represents the hash rate
    // println!("Hash Rate: {:.2} MH/s", hashrate_mh); // Print formatted hash rate

    Ok(seal)

}

pub trait Miner {
    fn mine(&self, worker_handle: Arc<WorkerHandle>, task_manager: &TaskManager);
}

pub struct CpuMiner;

impl Miner for CpuMiner {
    fn mine(&self, worker_handle: Arc<WorkerHandle>, task_manager: &TaskManager) {
        task_manager.spawn_essential_handle().spawn(
            "cpu-mining-loop",
            None,
            async move {
                let worker_handle = Arc::clone(&worker_handle);
                let mut nonce = 0;
                loop {
                    // Get mining metadata
                    println!("getting metadata");

                    let metadata = match worker_handle.metadata() {
                        Some(m) => m,
                        None => {
                            log::warn!(target: "pow", "No mining metadata available");
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                            continue;
                        }
                    };
                    let version = worker_handle.version();

                    println!("mine block");

                    // Mine the block
                    let seal = match try_nonce::<Block>(metadata.pre_hash, nonce, metadata.difficulty) {
                        Ok(s) => {
                            println!("valid seal: {:?}", s);
                            s
                        }
                        Err(_) => {
                            println!("error - seal not valid");
                            nonce += 1;
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                    };

                    println!("block found");

                    let current_version = worker_handle.version();
                    if current_version == version {
                        if block_on(worker_handle.submit(seal.encode())) {
                            println!("Successfully mined and submitted a new block");
                            nonce = 0;
                        } else {
                            println!("Failed to submit mined block");
                            nonce += 1;
                        }
                    }

                    // Sleep to avoid spamming
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }, // .boxed()
        );
    }
} 