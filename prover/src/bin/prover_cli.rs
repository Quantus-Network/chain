use anyhow::{anyhow, Result};
use clap::{App, Arg, SubCommand};
use prover::{WormholePrivateInputs, WormholePublicInputs, generate_proof, save_proof};
use serde_json::{from_reader, to_writer_pretty};
use sp_core::H256;
use sp_runtime::AccountId32;
use std::{fs::File, path::PathBuf};

fn main() -> Result<()> {
    env_logger::init();
    
    let matches = App::new("Wormhole Prover CLI")
        .version("0.1")
        .author("Your Name")
        .about("CLI for generating zero-knowledge proofs for Wormhole")
        .subcommand(
            SubCommand::with_name("generate")
                .about("Generate a proof")
                .arg(
                    Arg::with_name("private-inputs")
                        .long("private-inputs")
                        .help("Path to private inputs JSON file")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("public-inputs")
                        .long("public-inputs")
                        .help("Path to public inputs JSON file")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("output")
                        .long("output")
                        .help("Path to output proof file")
                        .takes_value(true)
                        .required(true),
                ),
        )
        .get_matches();

    if let Some(matches) = matches.subcommand_matches("generate") {
        // Read private inputs
        let private_inputs_path = matches.value_of("private-inputs").unwrap();
        let private_inputs_file = File::open(private_inputs_path)
            .map_err(|e| anyhow!("Failed to open private inputs file: {}", e))?;
        let private_inputs: WormholePrivateInputs = from_reader(private_inputs_file)
            .map_err(|e| anyhow!("Failed to parse private inputs: {}", e))?;

        // Read public inputs
        let public_inputs_path = matches.value_of("public-inputs").unwrap();
        let public_inputs_file = File::open(public_inputs_path)
            .map_err(|e| anyhow!("Failed to open public inputs file: {}", e))?;
        let public_inputs: WormholePublicInputs = from_reader(public_inputs_file)
            .map_err(|e| anyhow!("Failed to parse public inputs: {}", e))?;

        // Generate proof
        log::info!("Generating proof...");
        let proof = generate_proof(&private_inputs, &public_inputs)?;

        // Save proof
        let output_path = matches.value_of("output").unwrap();
        log::info!("Saving proof to {}...", output_path);
        save_proof(&proof, &PathBuf::from(output_path))?;
        log::info!("Proof generated successfully!");

        Ok(())
    } else {
        Err(anyhow!("No subcommand specified"))
    }
}

// Example function to create sample input files for testing
#[allow(dead_code)]
fn create_sample_input_files() -> Result<()> {
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
    let private_inputs_file = File::create("private_inputs.json")?;
    to_writer_pretty(private_inputs_file, &private_inputs)?;

    // Save public inputs to file
    let public_inputs_file = File::create("public_inputs.json")?;
    to_writer_pretty(public_inputs_file, &public_inputs)?;

    Ok(())
} 