use anyhow::Result;
use prover::{WormholePrivateInputs, WormholePublicInputs};
use serde_json::to_writer_pretty;
use sp_core::H256;
use sp_runtime::AccountId32;
use std::fs::File;
use std::path::PathBuf;

fn main() -> Result<()> {
    // Create output directory if it doesn't exist
    let output_dir = PathBuf::from("examples");
    std::fs::create_dir_all(&output_dir)?;
    
    // Create sample private inputs
    let private_inputs = WormholePrivateInputs {
        secret: vec![1, 2, 3, 4, 5],
        merkle_path: vec![H256::from([1u8; 32]), H256::from([2u8; 32])],
        merkle_indices: vec![0, 1],
    };

    // Create sample public inputs
    let public_inputs = WormholePublicInputs {
        nullifier: [1u8; 64],
        exit_account: AccountId32::new([2u8; 32]),
        exit_amount: 100,
        fee_amount: 10,
        storage_root: [3u8; 32],
    };

    // Save private inputs to file
    let private_inputs_path = output_dir.join("private_inputs.json");
    let private_inputs_file = File::create(&private_inputs_path)?;
    to_writer_pretty(private_inputs_file, &private_inputs)?;
    println!("Saved private inputs to {:?}", private_inputs_path);

    // Save public inputs to file
    let public_inputs_path = output_dir.join("public_inputs.json");
    let public_inputs_file = File::create(&public_inputs_path)?;
    to_writer_pretty(public_inputs_file, &public_inputs)?;
    println!("Saved public inputs to {:?}", public_inputs_path);

    println!("Example files generated successfully!");
    Ok(())
} 