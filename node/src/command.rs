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
use sc_cli::SubstrateCli;
use sc_network::config::{NodeKeyConfig, Secret};
use sc_service::{BlocksPruning, PartialComponents, PruningMode};
use sp_core::crypto::AccountId32;
use sp_core::crypto::Ss58Codec;
use sp_keyring::Sr25519Keyring;
use sp_runtime::traits::IdentifyAccount;

#[derive(Debug, PartialEq)]
pub struct QuantusKeyDetails {
    pub address: String,
    pub raw_address: String,
    pub public_key_hex: String, // Full public key, hex encoded with "0x" prefix
    pub secret_key_hex: String, // Secret key, hex encoded with "0x" prefix
    pub seed_hex: String,       // Derived seed, hex encoded with "0x" prefix
    pub secret_phrase: Option<String>, // Mnemonic phrase
    pub inner_hash: Option<String>, // If wormhole key, this is first hash
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
                inner_hash: None,
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
                inner_hash: Some(hex::encode(wormhole_pair.first_hash)),
            })
        }
    }
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
    sp_core::crypto::set_default_ss58_version(sp_core::crypto::Ss58AddressFormat::custom(189));
    let cli = Cli::from_args();
    match &cli.subcommand {
        Some(Subcommand::Key(cmd)) => {
            match cmd {
                QuantusKeySubcommand::Sc(sc_cmd) => sc_cmd.run(&cli),
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

                // Note: We parse node_key_file here to make a Dilithium keypair.
                // We then override the net config object parsed by sc_cli so we don't have to
                // fork sc_cli.
                let key_path =
                    if let Some(path_str) = &cli.run.network_params.node_key_params.node_key_file {
                        let path = std::path::Path::new(path_str);
                        if path.is_absolute() {
                            path.to_path_buf()
                        } else {
                            // This is a valid assumption because the node is run from the shell
                            std::env::current_dir().unwrap().join(path)
                        }
                    } else {
                        config
                            .network
                            .net_config_path
                            .clone()
                            .unwrap()
                            .join("secret_dilithium")
                    };

                log::debug!(target: "network", "node identity file: {:?}", key_path);

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
}
