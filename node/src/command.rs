use crate::cli::{QuantusAddressType, QuantusKeySubcommand};
use crate::{
    benchmarking::{inherent_benchmark_data, RemarkBuilder, TransferKeepAliveBuilder},
    chain_spec,
    cli::{Cli, Subcommand},
    service,
};
use dilithium_crypto::{traits::WormholeAddress, ResonancePair};
use frame_benchmarking_cli::{BenchmarkCmd, ExtrinsicFactory, SUBSTRATE_REFERENCE_HARDWARE};
use quantus_runtime::{Block, EXISTENTIAL_DEPOSIT};
use rusty_crystals_hdwallet::wormhole::WormholePair;
use rusty_crystals_hdwallet::{generate_mnemonic, HDLattice};
use sc_cli::{Error, KeySubcommand, SubstrateCli};
use sc_network::config::{NodeKeyConfig, Secret};
use sc_network::{Keypair};
use sc_service::{BasePath, BlocksPruning, PartialComponents, PruningMode};
use sp_core::crypto::AccountId32;
use sp_core::crypto::Ss58Codec;
use sp_keyring::Sr25519Keyring;
use sp_runtime::traits::IdentifyAccount;
use std::io::Write;
use std::path::PathBuf;
use std::{fs, io};
#[derive(Debug, PartialEq)]
pub struct QuantusKeyDetails {
    pub address: String,
    pub raw_address: String,
    pub public_key_hex: String, // Full public key, hex encoded with "0x" prefix
    pub secret_key_hex: String, // Secret key, hex encoded with "0x" prefix
    pub seed_hex: String,       // Derived seed, hex encoded with "0x" prefix
    pub secret_phrase: Option<String>, // Mnemonic phrase
}

const NODE_KEY_DILITHIUM_FILE: &str = "secret_dilithium";
const DEFAULT_NETWORK_CONFIG_PATH: &str = "network";

/// Returns the value of `base_path` or the default_path if it is None
pub(crate) fn base_path_or_default(
    base_path: Option<BasePath>,
    executable_name: &String,
) -> BasePath {
    base_path.unwrap_or_else(|| BasePath::from_project("", "", executable_name))
}

/// Returns the default path for configuration  directory based on the chain_spec
pub(crate) fn build_config_dir(base_path: &BasePath, chain_spec_id: &str) -> PathBuf {
    base_path.config_dir(chain_spec_id)
}

/// Returns the default path for the network configuration inside the configuration dir
pub(crate) fn build_net_config_dir(config_dir: &PathBuf) -> PathBuf {
    config_dir.join(DEFAULT_NETWORK_CONFIG_PATH)
}

/// Returns the default path for the network directory starting from the provided base_path
/// or from the default base_path.
pub(crate) fn build_network_key_dir_or_default(
    base_path: Option<BasePath>,
    chain_spec_id: &str,
    executable_name: &String,
) -> PathBuf {
    let config_dir = build_config_dir(
        &base_path_or_default(base_path, executable_name),
        chain_spec_id,
    );
    build_net_config_dir(&config_dir)
}

pub fn generate_quantus_key(
    scheme: QuantusAddressType,
    seed: Option<String>,
    words: Option<String>,
) -> Result<QuantusKeyDetails, sc_cli::Error> {
    match scheme {
        QuantusAddressType::Standard => {
            let actual_seed_for_pair: Vec<u8>;
            let mut words_to_print: Option<String> = None;

            if let Some(words_phrase) = words {
                let hd_lattice = HDLattice::from_mnemonic(&words_phrase, None).map_err(|e| {
                    eprintln!("Error processing provided words: {:?}", e);
                    sc_cli::Error::Input("Failed to process provided words".into())
                })?;
                actual_seed_for_pair = hd_lattice.seed.to_vec();
                words_to_print = Some(words_phrase.clone());
            } else if let Some(mut hex_seed_str) = seed {
                if hex_seed_str.starts_with("0x") {
                    hex_seed_str = hex_seed_str.trim_start_matches("0x").to_string();
                }

                if hex_seed_str.len() != 128 {
                    eprintln!(
                        "Error: --seed must be a 128-character hex string (for a 64-byte seed)."
                    );
                    return Err("Invalid hex seed length".into());
                }
                let decoded_seed_bytes = hex::decode(hex_seed_str).map_err(|_| {
                    eprintln!("Error: --seed must be a valid hex string (0-9, a-f).");
                    sc_cli::Error::Input("Invalid hex seed format".into())
                })?;
                if decoded_seed_bytes.len() != 64 {
                    eprintln!("Error: Decoded hex seed must be exactly 64 bytes.");
                    return Err("Invalid decoded hex seed length".into());
                }
                actual_seed_for_pair = decoded_seed_bytes;
            } else {
                let new_words = generate_mnemonic(24).map_err(|e| {
                    eprintln!("Error generating new words: {:?}", e);
                    sc_cli::Error::Input("Failed to generate new words".into())
                })?;

                let hd_lattice = HDLattice::from_mnemonic(&new_words, None).map_err(|e| {
                    eprintln!("Error creating HD lattice from new words: {:?}", e);
                    sc_cli::Error::Input("Failed to process new words".into())
                })?;
                actual_seed_for_pair = hd_lattice.seed.to_vec();
                words_to_print = Some(new_words);
            }

            let resonance_pair = ResonancePair::from_seed(&actual_seed_for_pair).map_err(|e| {
                eprintln!("Error creating ResonancePair: {:?}", e);
                sc_cli::Error::Input("Failed to create keypair".into())
            })?;

            let account_id = AccountId32::from(resonance_pair.public());

            Ok(QuantusKeyDetails {
                address: account_id.to_ss58check(),
                raw_address: format!("0x{}", hex::encode(account_id)),
                public_key_hex: format!("0x{}", hex::encode(resonance_pair.public())),
                secret_key_hex: format!("0x{}", hex::encode(resonance_pair.secret)),
                seed_hex: format!("0x{}", hex::encode(&actual_seed_for_pair)),
                secret_phrase: words_to_print,
            })
        }
        QuantusAddressType::Wormhole => {
            let wormhole_pair = WormholePair::generate_new().map_err(|e| {
                eprintln!("Error generating WormholePair: {:?}", e);
                sc_cli::Error::Input(format!("Wormhole generation error: {:?}", e).into())
            })?;

            // Convert wormhole address to account ID using WormholeAddress type
            let wormhole_address = WormholeAddress(wormhole_pair.address);
            let account_id = wormhole_address.into_account();

            Ok(QuantusKeyDetails {
                address: account_id.to_ss58check(),
                raw_address: format!("0x{}", hex::encode(account_id)),
                public_key_hex: format!("0x{}", hex::encode(wormhole_pair.address)),
                secret_key_hex: format!("0x{}", hex::encode(wormhole_pair.secret)),
                seed_hex: "N/A (Wormhole)".to_string(),
                secret_phrase: None,
            })
        }
    }
}

/// This is copied from sc-cli and adapted to dilithium
fn generate_key_in_file(
    file: &Option<PathBuf>,
    chain_spec_id: Option<&str>,
    base_path: &Option<PathBuf>,
    default_base_path: bool,
    executable_name: Option<&String>,
    keypair: Option<&Keypair>,
) -> Result<(), Error> {
    let kp: Keypair;
    if let Some(k) = keypair {
        kp = k.clone();
    } else {
        kp = Keypair::generate_dilithium();
    }
    let file_data = kp.to_protobuf_encoding().unwrap();

    match (file, base_path, default_base_path) {
        (Some(file), None, false) => fs::write(file, file_data)?,
        (None, Some(_), false) | (None, None, true) => {
            let network_path = build_network_key_dir_or_default(
                base_path.clone().map(BasePath::new),
                chain_spec_id.unwrap_or_default(),
                executable_name.ok_or(Error::Input("Executable name not provided".into()))?,
            );

            fs::create_dir_all(network_path.as_path())?;

            let key_path = network_path.join(NODE_KEY_DILITHIUM_FILE);
            if key_path.exists() {
                eprintln!("Skip generation, a key already exists in {:?}", key_path);
                return Err(Error::KeyAlreadyExistsInPath(key_path));
            } else {
                eprintln!("Generating key in {:?}", key_path);
                fs::write(key_path, file_data)?
            }
        }
        (None, None, false) => io::stdout().lock().write_all(&file_data)?,
        (_, _, _) => {
            // This should not happen, arguments are marked as mutually exclusive.
            return Err(Error::Input("Mutually exclusive arguments provided".into()));
        }
    }

    eprintln!("{}", kp.public().to_peer_id());

    Ok(())
}

impl SubstrateCli for Cli {
    fn impl_name() -> String {
        "Quantus Node".into()
    }

    fn impl_version() -> String {
        env!("SUBSTRATE_CLI_IMPL_VERSION").into()
    }

    fn description() -> String {
        env!("CARGO_PKG_DESCRIPTION").into()
    }

    fn author() -> String {
        env!("CARGO_PKG_AUTHORS").into()
    }

    fn support_url() -> String {
        "support.anonymous.an".into()
    }

    fn copyright_start_year() -> i32 {
        2017
    }

    fn load_spec(&self, id: &str) -> Result<Box<dyn sc_service::ChainSpec>, String> {
        Ok(match id {
            "dev" => {
                Box::new(chain_spec::development_chain_spec()?) as Box<dyn sc_service::ChainSpec>
            }
            "live_resonance_local" => {
                Box::new(chain_spec::live_testnet_chain_spec()?) as Box<dyn sc_service::ChainSpec>
            }
            "live_resonance" => Box::new(chain_spec::ChainSpec::from_json_bytes(include_bytes!(
                "chain-specs/live-resonance.json"
            ))?) as Box<dyn sc_service::ChainSpec>,
            "" | "local" => {
                Box::new(chain_spec::local_chain_spec()?) as Box<dyn sc_service::ChainSpec>
            }
            path => Box::new(chain_spec::ChainSpec::from_json_file(
                std::path::PathBuf::from(path),
            )?) as Box<dyn sc_service::ChainSpec>,
        })
    }
}

/// Parse and run command line arguments
pub fn run() -> sc_cli::Result<()> {
    let cli = Cli::from_args();
    match &cli.subcommand {
        Some(Subcommand::Key(cmd)) => {
            match cmd {
                QuantusKeySubcommand::Sc(sc_cmd) => match sc_cmd {
                    KeySubcommand::GenerateNodeKey(gen_cmd) => {
                        let chain_spec = cli.load_spec(gen_cmd.chain.as_deref().unwrap_or(""))?;
                        generate_key_in_file(
                            &None,
                            Some(chain_spec.id()),
                            &None,
                            true,
                            Some(&Cli::executable_name()),
                            None,
                        )
                    }
                    _ => sc_cmd.run(&cli),
                },
                QuantusKeySubcommand::Quantus {
                    scheme,
                    seed,
                    words,
                } => {
                    match generate_quantus_key(scheme.clone(), seed.clone(), words.clone()) {
                        Ok(details) => {
                            match scheme {
                                QuantusAddressType::Standard => {
                                    println!("Generating Quantus Standard address...");
                                    if seed.is_some() {
                                        println!("Using provided hex seed...");
                                    } else if words.is_some() {
                                        println!("Using provided words phrase...");
                                    } else {
                                        println!(
                                            "No seed or words provided. Generating a new 24-word phrase..."
                                        );
                                    }

                                    println!(
                                        "XXXXXXXXXXXXXXX Quantus Account Details XXXXXXXXXXXXXXXXX"
                                    );
                                    if let Some(phrase) = &details.secret_phrase {
                                        println!("Secret phrase: {}", phrase);
                                    }
                                    println!("Address: {}", details.address);
                                    println!("Seed: {}", details.seed_hex);
                                    println!("Pub key: {}", details.public_key_hex);
                                    println!("Secret key: {}", details.secret_key_hex);
                                    println!(
                                        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"
                                    );
                                }
                                QuantusAddressType::Wormhole => {
                                    println!("Generating wormhole address...");
                                    println!(
                                        "XXXXXXXXXXXXXXX Quantus Wormhole Details XXXXXXXXXXXXXXXXX"
                                    );
                                    println!("Address: {}", details.address);
                                    println!("Wormhole Address: {}", details.public_key_hex);
                                    println!("Secret: {}", details.secret_key_hex);
                                    // Pub key and Seed are N/A for wormhole as per QuantusKeyDetails
                                    println!(
                                        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"
                                    );
                                }
                            }
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
            }
        }
        Some(Subcommand::BuildSpec(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.sync_run(|config| cmd.run(config.chain_spec, config.network))
        }
        Some(Subcommand::CheckBlock(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|config| {
                let PartialComponents {
                    client,
                    task_manager,
                    import_queue,
                    ..
                } = service::new_partial(&config)?;
                Ok((cmd.run(client, import_queue), task_manager))
            })
        }
        Some(Subcommand::ExportBlocks(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|config| {
                let PartialComponents {
                    client,
                    task_manager,
                    ..
                } = service::new_partial(&config)?;
                Ok((cmd.run(client, config.database), task_manager))
            })
        }
        Some(Subcommand::ExportState(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|config| {
                let PartialComponents {
                    client,
                    task_manager,
                    ..
                } = service::new_partial(&config)?;
                Ok((cmd.run(client, config.chain_spec), task_manager))
            })
        }
        Some(Subcommand::ImportBlocks(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|config| {
                let PartialComponents {
                    client,
                    task_manager,
                    import_queue,
                    ..
                } = service::new_partial(&config)?;
                Ok((cmd.run(client, import_queue), task_manager))
            })
        }
        Some(Subcommand::PurgeChain(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.sync_run(|config| cmd.run(config.database))
        }
        Some(Subcommand::Revert(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|config| {
                let PartialComponents {
                    client,
                    task_manager,
                    backend,
                    ..
                } = service::new_partial(&config)?;
                let aux_revert = Box::new(|_client, _, _blocks| {
                    unimplemented!("TODO - g*randpa was removed.");
                });
                Ok((cmd.run(client, backend, Some(aux_revert)), task_manager))
            })
        }
        Some(Subcommand::Benchmark(cmd)) => {
            let runner = cli.create_runner(cmd)?;

            runner.sync_run(|config| {
                // This switch needs to be in the client, since the client decides
                // which sub-commands it wants to support.
                match cmd {
                    BenchmarkCmd::Pallet(cmd) => {
                        if !cfg!(feature = "runtime-benchmarks") {
                            return Err(
                                "Runtime benchmarking wasn't enabled when building the node. \
							You can enable it with `--features runtime-benchmarks`."
                                    .into(),
                            );
                        }

                        cmd.run_with_spec::<sp_runtime::traits::HashingFor<Block>, ()>(Some(
                            config.chain_spec,
                        ))
                    }
                    BenchmarkCmd::Block(cmd) => {
                        let PartialComponents { client, .. } = service::new_partial(&config)?;
                        cmd.run(client)
                    }
                    #[cfg(not(feature = "runtime-benchmarks"))]
                    BenchmarkCmd::Storage(_) => Err(
                        "Storage benchmarking can be enabled with `--features runtime-benchmarks`."
                            .into(),
                    ),
                    #[cfg(feature = "runtime-benchmarks")]
                    BenchmarkCmd::Storage(cmd) => {
                        let PartialComponents {
                            client, backend, ..
                        } = service::new_partial(&config)?;
                        let db = backend.expose_db();
                        let storage = backend.expose_storage();

                        cmd.run(config, client, db, storage)
                    }
                    BenchmarkCmd::Overhead(cmd) => {
                        let PartialComponents { client, .. } = service::new_partial(&config)?;
                        let ext_builder = RemarkBuilder::new(client.clone());

                        cmd.run(
                            config.chain_spec.name().into(),
                            client,
                            inherent_benchmark_data()?,
                            Vec::new(),
                            &ext_builder,
                            false,
                        )
                    }
                    BenchmarkCmd::Extrinsic(cmd) => {
                        let PartialComponents { client, .. } = service::new_partial(&config)?;
                        // Register the *Remark* and *TKA* builders.
                        let ext_factory = ExtrinsicFactory(vec![
                            Box::new(RemarkBuilder::new(client.clone())),
                            Box::new(TransferKeepAliveBuilder::new(
                                client.clone(),
                                Sr25519Keyring::Alice.to_account_id(),
                                EXISTENTIAL_DEPOSIT,
                            )),
                        ]);

                        cmd.run(client, inherent_benchmark_data()?, Vec::new(), &ext_factory)
                    }
                    BenchmarkCmd::Machine(cmd) => {
                        cmd.run(&config, SUBSTRATE_REFERENCE_HARDWARE.clone())
                    }
                }
            })
        }
        Some(Subcommand::ChainInfo(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.sync_run(|config| cmd.run::<Block>(&config))
        }
        None => {
            log::info!("Run until exit ....");
            let runner = cli.create_runner(&cli.run)?;
            runner.run_node_until_exit(|mut config| async move {
                //Obligatory configuration for all node holders
                config.blocks_pruning = BlocksPruning::KeepFinalized;
                config.state_pruning = Some(PruningMode::ArchiveCanonical);
                // NOTE: at this point the net_config_path is a pointer to a file that does not yet exist
                // so it is safe to change it here
                let key_path = config
                    .network
                    .net_config_path
                    .clone()
                    .unwrap()
                    .join("secret_dilithium");
                // log::info!("Node Key: {:?}, dir: {:?}, path: {:?}, contents: {:?}",
                //     config.network.node_key,
                //     config.network.net_config_path,
                //     p,
                //     std::fs::read_to_string(
                //         config.network.net_config_path.clone().unwrap().join("secret_ed25519")
                //     )
                // );
                config.network.node_key = NodeKeyConfig::Dilithium(Secret::File(key_path));

                match config.network.network_backend.unwrap_or_default() {
                    sc_network::config::NetworkBackendType::Libp2p => service::new_full::<
                        sc_network::NetworkWorker<
                            quantus_runtime::opaque::Block,
                            <quantus_runtime::opaque::Block as sp_runtime::traits::Block>::Hash,
                        >,
                    >(
                        config,
                        cli.rewards_address.clone(),
                        cli.external_miner_url.clone(),
                    )
                    .map_err(sc_cli::Error::Service),
                    sc_network::config::NetworkBackendType::Litep2p => {
                        panic!("Litep2p not supported");
                    }
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::QuantusAddressType;
    use crate::tests::data::quantus_key_test_data::{
        EXPECTED_PUBLIC_KEY_HEX, EXPECTED_SECRET_KEY_HEX, TEST_ADDRESS, TEST_MNEMONIC,
        TEST_SEED_HEX,
    };
    use tempfile::TempDir;

    #[test]
    fn test_generate_quantus_key_standard_new_mnemonic() {
        // Test generating a standard address with a new mnemonic
        let result = generate_quantus_key(QuantusAddressType::Standard, None, None);
        assert!(result.is_ok());
        assert!(result.unwrap().secret_phrase.is_some());
    }

    #[test]
    fn test_generate_quantus_key_standard_from_mnemonic() {
        // Test generating a standard address from a provided mnemonic
        let mnemonic =
            "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title"
                .to_string();
        let result =
            generate_quantus_key(QuantusAddressType::Standard, None, Some(mnemonic.clone()));
        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.secret_phrase, Some(mnemonic));
    }

    #[test]
    fn test_generate_quantus_key_standard_from_seed() {
        // Test generating a standard address from a provided seed (0x prefixed and not)
        let seed_hex_no_prefix = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(); // 128 hex chars
        let seed_hex_with_prefix = format!("0x{}", seed_hex_no_prefix);

        let result_no_prefix = generate_quantus_key(
            QuantusAddressType::Standard,
            Some(seed_hex_no_prefix.clone()),
            None,
        );
        assert!(result_no_prefix.is_ok());
        let details_no_prefix = result_no_prefix.unwrap();
        assert_eq!(details_no_prefix.seed_hex, seed_hex_with_prefix); // Output is always 0x prefixed
        assert!(details_no_prefix.secret_phrase.is_none());

        let result_with_prefix = generate_quantus_key(
            QuantusAddressType::Standard,
            Some(seed_hex_with_prefix.clone()),
            None,
        );
        assert!(result_with_prefix.is_ok());
        let details_with_prefix = result_with_prefix.unwrap();
        assert_eq!(details_with_prefix.seed_hex, seed_hex_with_prefix);
        assert!(details_with_prefix.secret_phrase.is_none());
    }

    #[test]
    fn test_generate_quantus_key_wormhole() {
        // Test generating a wormhole address
        let result = generate_quantus_key(QuantusAddressType::Wormhole, None, None);
        assert!(result.is_ok());
        let details = result.unwrap();
        assert!(details.public_key_hex.starts_with("0x"));
        assert!(details.secret_key_hex.starts_with("0x"));
        assert_eq!(details.seed_hex, "N/A (Wormhole)");
        assert!(details.secret_phrase.is_none());
        let address = details.address;
        assert!(
            AccountId32::from_ss58check_with_version(&address).is_ok(),
            "Generated address should be valid SS58: {}",
            address
        );
    }

    #[test]
    fn test_generate_quantus_key_invalid_seed_length() {
        // Test error handling for invalid seed length
        let seed = Some("0123456789abcdef".to_string()); // Too short (16 chars, expected 128)
        let result = generate_quantus_key(QuantusAddressType::Standard, seed, None);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(format!("{:?}", e), "Input(\"Invalid hex seed length\")");
        }
    }

    #[test]
    fn test_generate_quantus_key_invalid_seed_format() {
        // Test error handling for invalid seed format (non-hex characters)
        // Ensure the string is 128 chars long but contains an invalid hex char.
        let seed = Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg0123456789abcdef".to_string()); // Contains 'g', now 128 chars
        let result = generate_quantus_key(QuantusAddressType::Standard, seed, None);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(format!("{:?}", e), "Input(\"Invalid hex seed format\")");
        }
    }

    #[test]
    fn test_generate_quantus_key_standard_known_values() {
        let mnemonic = TEST_MNEMONIC.to_string();
        let expected_seed_hex = TEST_SEED_HEX.to_string();
        let expected_address = TEST_ADDRESS.to_string();
        let expected_public_key_hex = EXPECTED_PUBLIC_KEY_HEX.to_string();
        let expected_secret_key_hex = EXPECTED_SECRET_KEY_HEX.to_string();

        let result =
            generate_quantus_key(QuantusAddressType::Standard, None, Some(mnemonic.clone()));
        assert!(result.is_ok());
        let details = result.unwrap();

        assert_eq!(details.secret_phrase, Some(mnemonic));
        assert_eq!(details.seed_hex, expected_seed_hex.clone());
        assert_eq!(details.address, expected_address.clone());
        assert_eq!(details.public_key_hex, expected_public_key_hex.clone());
        assert_eq!(details.secret_key_hex, expected_secret_key_hex.clone());

        let result = generate_quantus_key(
            QuantusAddressType::Standard,
            Some(expected_seed_hex.clone()),
            None,
        );
        assert!(result.is_ok());
        let details = result.unwrap();

        assert_eq!(details.seed_hex, expected_seed_hex);
        assert_eq!(details.address, expected_address);
        assert_eq!(details.public_key_hex, expected_public_key_hex);
        assert_eq!(details.secret_key_hex, expected_secret_key_hex);
    }

    #[test]
    fn test_generate_key_in_file_explicit_path() {
        // Setup: Create a temporary directory and file path.
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.key");
        let original_keypair = Keypair::generate_dilithium();
        // Generate and write keypair.
        let result = generate_key_in_file(
            &Some(file_path.clone()),
            None,
            &None,
            false,
            Some(&"test-exec".to_string()),
            Some(&original_keypair),
        );
        assert!(result.is_ok(), "Failed to generate key: {:?}", result);

        // Read the file contents.
        let file_data = fs::read(&file_path).expect("Failed to read file");

        // Deserialize the keypair.
        let deserialized_keypair =
            Keypair::from_protobuf_encoding(&file_data).expect("Failed to deserialize keypair");

        // Verify the deserialized keypair matches the original.
        assert_eq!(
            deserialized_keypair.public().to_peer_id(),
            original_keypair.public().to_peer_id(),
            "Deserialized public key does not match original"
        );
    }

    #[test]
    fn test_generate_key_in_file_default_path() {
        // Setup: Create a temporary directory as base path.
        let temp_dir = TempDir::new().unwrap();
        let base_path = Some(temp_dir.path().to_path_buf());
        let chain_spec_id = "test-chain";
        let executable_name = "test-exec";
        let original_keypair = Keypair::generate_dilithium();

        // Generate and write keypair.
        let result = generate_key_in_file(
            &None,
            Some(chain_spec_id),
            &base_path,
            false,
            Some(&executable_name.to_string()),
            Some(&original_keypair),
        );
        assert!(result.is_ok(), "Failed to generate key: {:?}", result);

        // Construct the expected key file path.
        let expected_path = temp_dir
            .path()
            .join("chains")
            .join(chain_spec_id)
            .join("network")
            .join(NODE_KEY_DILITHIUM_FILE);

        // Read the file contents.
        let file_data = fs::read(&expected_path).expect("Failed to read file");

        // Deserialize the keypair.
        let deserialized_keypair =
            Keypair::from_protobuf_encoding(&file_data).expect("Failed to deserialize keypair");

        // Verify the deserialized keypair matches the original.
        assert_eq!(
            deserialized_keypair.public().to_peer_id(),
            original_keypair.public().to_peer_id(),
            "Deserialized public key does not match original"
        );
    }

    #[test]
    fn test_generate_key_in_file_key_already_exists() {
        // Setup: Create a temporary directory and pre-create the key file.
        let temp_dir = TempDir::new().unwrap();
        let base_path = Some(temp_dir.path().to_path_buf());
        let chain_spec_id = "test-chain";
        let executable_name = "test-exec";

        // Create the network directory and key file.
        let network_path = temp_dir
            .path()
            .join("chains")
            .join(chain_spec_id)
            .join("network");
        fs::create_dir_all(&network_path).unwrap();
        let key_path = network_path.join(NODE_KEY_DILITHIUM_FILE);
        fs::write(&key_path, vec![0u8; 8]).unwrap(); // Write dummy data.

        // Attempt to generate key (should fail due to existing file).
        let result = generate_key_in_file(
            &None,
            Some(chain_spec_id),
            &base_path,
            false,
            Some(&executable_name.to_string()),
            None,
        );
        assert!(
            matches!(result, Err(Error::KeyAlreadyExistsInPath(_))),
            "Expected KeyAlreadyExistsInPath error, got: {:?}",
            result
        );
    }
}
