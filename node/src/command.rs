use crate::cli::{QuantusAddressType, QuantusKeySubcommand};
use crate::{
    benchmarking::{inherent_benchmark_data, RemarkBuilder, TransferKeepAliveBuilder},
    chain_spec,
    cli::{Cli, Subcommand},
    service,
};
use dilithium_crypto::ResonancePair;
use frame_benchmarking_cli::{BenchmarkCmd, ExtrinsicFactory, SUBSTRATE_REFERENCE_HARDWARE};
use resonance_runtime::{Block, EXISTENTIAL_DEPOSIT};
use rusty_crystals_hdwallet::{generate_mnemonic, HDLattice};
use sc_cli::SubstrateCli;
use sc_service::{BlocksPruning, PartialComponents, PruningMode};
use sp_core::crypto::AccountId32;
use sp_core::crypto::Ss58Codec;
use sp_keyring::Sr25519Keyring;
use sp_wormhole::WormholePair;

#[derive(Debug, PartialEq)]
pub struct QuantusKeyDetails {
    pub address: String,
    pub public_key_hex: String, // Full public key, hex encoded with "0x" prefix
    pub secret_key_hex: String, // Secret key, hex encoded with "0x" prefix
    pub seed_hex: String,       // Derived seed, hex encoded with "0x" prefix
    pub secret_phrase: Option<String>, // Mnemonic phrase
}

// Modified function to handle Quantus key generation logic
pub fn generate_quantus_key(
    scheme: QuantusAddressType,
    seed: Option<String>,
    words: Option<String>,
) -> Result<QuantusKeyDetails, sc_cli::Error> {
    // Changed return type
    match scheme {
        QuantusAddressType::Standard => {
            // println!("Generating Quantus Standard address..."); // Will be printed by caller

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

            // Populate and return the struct instead of printing
            Ok(QuantusKeyDetails {
                address: account_id.to_ss58check(),
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

            // Wormhole addresses are H256, not directly SS58. We'll show its hex representation.
            // For consistency with the struct, we use the `address` field.
            Ok(QuantusKeyDetails {
                address: format!("0x{}", hex::encode(wormhole_pair.address)),
                public_key_hex: "N/A (Wormhole)".to_string(),
                secret_key_hex: format!("0x{}", hex::encode(wormhole_pair.secret)),
                seed_hex: "N/A (Wormhole)".to_string(),
                secret_phrase: None,
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
                    // generate_quantus_key now returns Result<QuantusKeyDetails, sc_cli::Error>
                    match generate_quantus_key(scheme.clone(), seed.clone(), words.clone()) {
                        Ok(details) => {
                            // Print details based on the scheme used (which we know from `scheme`)
                            match scheme {
                                QuantusAddressType::Standard => {
                                    println!("Generating Quantus Standard address..."); // Print context
                                    if seed.is_some() {
                                        println!("Using provided hex seed...");
                                    } else if words.is_some() {
                                        println!("Using provided words phrase...");
                                    } else {
                                        println!("No seed or words provided. Generating a new 24-word phrase...");
                                    }

                                    println!(
                                        "XXXXXXXXXXXXXXX Quantus Account Details XXXXXXXXXXXXXXXXX"
                                    );
                                    if let Some(phrase) = &details.secret_phrase {
                                        println!("Secret phrase: {}", phrase);
                                    }
                                    println!("Address: {}", details.address);
                                    println!("Pub key: {}", details.public_key_hex);
                                    println!("Secret key: {}", details.secret_key_hex);
                                    println!("Seed (hex): {}", details.seed_hex);
                                    println!("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
                                }
                                QuantusAddressType::Wormhole => {
                                    println!("Generating wormhole address..."); // Print context
                                    println!("XXXXXXXXXXXXXXX Reconance Wormhole Details XXXXXXXXXXXXXXXXX");
                                    println!("Address: {}", details.address); // This is 0x prefixed H256 hex
                                    println!("Secret: {}", details.secret_key_hex);
                                    // Pub key and Seed (hex) are N/A for wormhole as per QuantusKeyDetails
                                    println!("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
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

                match config.network.network_backend.unwrap_or_default() {
                    sc_network::config::NetworkBackendType::Libp2p => service::new_full::<
                        sc_network::NetworkWorker<
                            resonance_runtime::opaque::Block,
                            <resonance_runtime::opaque::Block as sp_runtime::traits::Block>::Hash,
                        >,
                    >(
                        config,
                        cli.rewards_address.clone(),
                        cli.external_miner_url.clone(),
                    )
                    .map_err(sc_cli::Error::Service),
                    sc_network::config::NetworkBackendType::Litep2p => {
                        service::new_full::<sc_network::Litep2pNetworkBackend>(
                            config,
                            cli.rewards_address.clone(),
                            cli.external_miner_url.clone(),
                        )
                        .map_err(sc_cli::Error::Service)
                    }
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*; // Import items from the parent module
    use crate::cli::QuantusAddressType;

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
        assert!(details.address.starts_with("0x"));
        assert_eq!(details.public_key_hex, "N/A (Wormhole)");
        assert!(details.secret_key_hex.starts_with("0x"));
        assert_eq!(details.seed_hex, "N/A (Wormhole)");
        assert!(details.secret_phrase.is_none());
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
        let mnemonic =
            "orchard answer curve patient visual flower maze noise retreat penalty cage small earth domain scan pitch bottom crunch theme club client swap slice raven"
                .to_string();

        let expected_seed_hex = "0xb57c169d43b539edb123c14d598dcc29303023b6b83cdf4cf548813c281153a712aab6d2d1b670e7a7ddf5ed291eb3ccd9f36066dc17ffa76ae91233eda19456".to_string();
        let expected_address = "5H72qHthhaHuwybTUZDi9gtVqGbitcYPxUgbid8BLdZyiiMM".to_string();
        let expected_public_key_hex = "0xa1063473a31ea7462f1f2324d6fdb30971dc2b300f81daf03564791913c8422f0a301693d590f4d0f503f4c2a620a08beacbde4acfd661385641c89f151f02397bbb8866294970e69907e093a6eb32fdf9a7919b764df445ce9238fbaad0fb23d8c122648669a81cff69bc43d4e5f369039f33888219beaa7338dbdcbb2f6663e10619112366c4a5a6897c181bbb31e4aa78b55f5d40cc2d2c7bf07e67879be6119ea5210a95bb53ddd0a513c472300a83190fbc64391a447ddcd34ce558ee066f4aa6a13253f9729bc6eef9307240db66200fdcc76bc4fd3a0706371db46c3b13410de623a030d282226239a1463034b5e35f187317d2a2ade405b4b17a91a4c4aeda46e0b5af1e507a43ec0fd24c9186aa92ebdaf3ecf5dda5846ac4ebe6076352909d7d9f0e02e5a61430cebad063939eb8e757a12246157b81810268a3606d1513f4ef8168634992707f71b3c286e95f39c9f86b5a8fbfaf5b281abb09dfa495a3988901c9eb02dc3d6276877f37c8900bc43347296191499a6079575c5f5870a0f9d5590c30445b8d10a5122f0c832f92d514e5f5876fcd77bab3e44827776b73d5a72d7a8f6346d1f5a603bc87a41d2bbf050c2138033bf798f52bb64908de33b22e8636fa78def163dcad08834342d4faf5f8790b59ee1de3750fa7ef0e13bd33ef03ddb85468fe817401e17ca01d04738c47a89b97b35a8c7852c37ac45edd64a0fa0090a662f1c7840dfb743d6c6634696b2fb28789f052312e46750bd00eb75803cdbb71735290079ab9f32ce40925c774b5bc316f184cb2e12cbb45e809b1b39ab1a4d7cfbbf7680bf7e2ee206a97b35a8c28fe169e5ddc258a0700bc772ad181c2201f09ca7975d4483b74688ab71fe8fa227cc1f2e017c3c7e27a1fdaf938e03a692f607e36a99f48ff4224a0d0626769395d0c23c0b34db311100ca22051672b5c78971f0dc15b806ec9fcd78ee2fd446e4e1ccd998b39f7b1672ebb2067eda389554d9d7c7b5552c771d8f25844366ce206bdedc855fc3e0d85eafdc0ec0cd4b5eb6a76ed0e45ed552a5bf9f21039bc265dbc25b0efe62b254a6925b0ecc6890cde35b33cf4e939d7585ef6e8373d1aafa0279a82ba4592e7e25aaebb4a90c76380478ed2f4816b71074bac1f53425d7e79d58ec0ffb104ef77bbbf35eb9752574a78a6ee31f0026a921a26ceea82120521a30f8015e5414499f688c56a882595579d035a6d48509fecf6e4373f52e9f1a3bf005e681d832399ed8b38f9dd83b29c4dc7970a082b86aa3f593169f434da7e5723c7c086baf2ef25375f48cebc162686b78747f4fcd3b0ae2cac37edc649df4bd142530449d03ec2945146e543b4370d5372183797e51ba741bbbff7c0451f513926a749e75acf285022fdb1e41233101114f5c0955cb6474e80f8dbd89849994d6ea3f4de87f54ad51d628d28feb04af86b53d8dbf9bccae9eff3f7364bf6f6776c3a96331bf9f389092aa6df650c550cd9d6d3a60160b753e10f3d0477490e3719a91b4448e15e60ce287a96577e414344a4f28d90cb0e05f39223d9b97174c72914abcfbd876de04e6e9b435d40cc56bc91af8c165e0fd8bb5f6f70084c53434fc9ce23d0c0bb46a6928265c8f0b0a1c417330c774d918f233b56ccf9bff80ceb7ae050ca5a5e534933f77f873a2735db4ccd0a728569fb32986414497ef26263fe52df7a2d7ebca24186c50a36106fceaa8d07ba2cd0a232d00f207f6ff5d1965dafafda01940f168ce99627cc4a5b6954f7cca72a2de8e9031566f4780132ff97853ca9e8f0698ab9e2929121eef3dfc5ebe7a5db4a71d46680874f09fce6e75de2035abe0818fa4d008a8bf3e985aaf15e977fb0d4c9abc2fd04472d170e00cc13bf0957d89d6e26ae84fe4e24e06297d0eb473ec7c8fe78d0dfd77fb2c80f27a058ab52ad54c3ad7bd981950157b60cc7d6c2568c5b88c8864b3d0de1e5456f72c8a65b5ea5c02d10f1c90f6e3331dfcade1033161623fe248632f3b55a02c5288d92391054e464e51545527e4eea44bc1685f88a0a37d5134b0b5c6882ba27c9840954c2d8a22dd87f660666715cee0b56c55cca0d23365e21b27becb46356ed70b3f0d361f0a797507e34e980f701a49ac206afb9fc14dcfb3891ae3eae98dbfe36e345c65b5e99982458960bd01a5573e300cb42ba71237391cfcc6f6c0dd9e83fd70ac3046700b7bbaba918889d267e954743834a106af9d99542101314d602177bc2b5fa93e8e08a03e1588c95c6d2542604459b1693215a75952343ef2d2c1db570192968a31b20a3b87e120a89c08a683757f71b0086b96da17248fc5423e1be78958b0b9a1a12b5c47e448cd021a07e103a053bb2061ebabadfe86bac6a650f6b0e5e122c867464ade254e7f76ad572972c2212e47a233bb30a10442715c2855d2f14b5c04103a56ab18de8fa58d9cc94c10dabb14d63d85164d64a4764c4960b82ee41b7be34806ef0159bb343b0e7961c2e872ed7c7c3b3a7beac4997549f5420a84f8f4ea8d480de2531fec9ef9169cfb891efad56702d417536b6f607e400a19420508d25e17fd1f123c2abef263b2d3534a7345f383aa3378aabc7541c4b12bfd251db8b2cbc5ba7e53f809e641ae3387e7ec7cc2a85a221b8885cf62a38d7f7c956590927db0d0fa2c207983ca4b70e759c9163869127d4e89463469fc381cc4dce196ca29775b344ce2bf43ded087a6de42f89b45e31faefac7970c27cc34e159d2d1729249b19265c5e16c98c4c65094a7e3d35438a0d980dff8963f3f68968957cd9d5dd73df73d7393bab8c2a9e325c819cf3a301dc678139f0649070723551b00fb6a842ae09b79285f86b5fac3a318b6b31b9e3090bd66cbc7c75c7932020c0003824927f9c09f0300011ad380483a11691c23c99497bebea253f0e65fd8e028bc8e50cab4a1ab37fe788262489508714c5deea8b5c1b70dd3b3e6a252899687e7aaad7c16984ad9aaf69335cc8e0b3e39612c1db36e72f1627a4d35e7c53bd6186f2666f059705d585020bc5ca30ddc0d5b7f19e04d76efaf18ea48b1882197807d8a381c17451b6444cd58d1233a226d6c6c9ea20a19dbe740e3094470f1d46f5c51a37a6a1263a171499961620a97a14f5fe99ae7e31f5babd453ca103cb550b9d2cae47f0f27d83b045c33b079640a3f2fe697e412940ee7c72c7d5a665415589297793f9dc4a87c9b9a5a4bdfd517792401b2a96434226faee9efb8dbfefa90fd7ad28dca4a80674534c36d8a65e9eafc7feb683211bafc3be739cc6e851650e78adcd41ec3c065e1a36b6f041218ce8000c65fccc4eee3633224209b4b081521ec7bcde49bc348161c562cbf1d7a229e5492c8a70e0abc674323a68634cdb2e70b0f43553ec49209747ea260417e33212e79360cbb14d7d5ea8e990d6dcfbd0eb42f09857c97cac3acec5efe6c5670a7fe377e2cc2f19dd5cef4bf5fd49c8af1a0d4c5d428a525568a056100b0a9727086f304ccb1d67bed6c16d58411f46d6bd8f7bccf251b710cc303994d1890739df7b9233074c509506055f3b09343742e2688d64c80d728155bb424cb6c6076e21ae1c56b645f199a87954d7117d3372bd76ffce291a7a65ebb3813a87dd77eff1063c0adff0".to_string();
        let expected_secret_key_hex = "0xa1063473a31ea7462f1f2324d6fdb30971dc2b300f81daf03564791913c8422f11f6a2d875ebbd3890f3007ae540de4f94d12a61cf9ca69307b1f596027e4fc33bedc627d58ec5ec13c710b919f8409d70ef1dbbaa4ad1d4d0971536931583b4f56fde0e1cf3283e9a7f19aee5f6effd0cb82e6df0b97aa7e5d8109923cd7517d1364898128940b448593832549484111044502262c2380a04a26dcc800d0a444ea1426423440c00a329524809082491e01089248230644250dc0261e3385152c0896114505a986500b84d03430ddc96845c06028238099c386560426002456e4ca66518a348e40222220122192820c2384991b24410261009442e9208814c28886290700484091b374043204510a28504410453106e0230460cb18c93882c22c36dca443019b02ce1a62d19398541166d099084d140016226129a842dd43450044631a338000b2104511646132370e1360c0c322601c14d22105224064ed9c890a3286990168a13a18dcc248624a368d482055a383198c48c9080811449092237864b188a51346e1c4320c1108a23384d82808d18b74c0a0420c30480d1a280a13611a4c60c24c00dc22205c0a400002051e2326218b00163440cd38888102850d02042a1484880968d0381651000649a1861d2a85122388d50a461da1622dab20402450250c28d1c344a0a22201040655b408823a020a0428ac240010cc97041068120b1204b268ad834455c026c92088019488a0bb47122212641807004232014c02510b82de19248d41272a2841089863121b46194184812c2894bc288e3286d200229211702d19850810684c320211b94851285819a3662220941c4a031da0491d4186c04446eda265188c84d1ba5050c436008a2708a1450818044023884e1886523234989028a59428a8c128ec112820196091b8480d1223241802823c02801042c140411091942e31828dc82909b324109c56540c02d51303110b08541142612004603468dcb82249834864c388819312920c66580220209101012176a92c0001ab1685cc651e4c24800998190c8491c248ea4c664a1a629084046a2141101361014c80942c490e0064512162ac2428e9a424019a1100921518bb88d20c34ce1a060810271c42020cbc291e242500b324d53924dd090488ba60c99b40c922442e3a8241a3066d1a40848448adb188d1cc040482070221344e234861a494524390408a68d13c72d08378d59b090833230d4904d0aa94ca020280948200b3061d8c22998b284143450024548584230e192080c44095c442d22058621b97042b06593120522c8414a26082039692101614b248a83b00093b428842026db28890b292a5b32114aa48cdc202a1ba0500c1828d3b6680880850c114a48a60821c37020320edb84880c07850a114099144248188c9b221023084a214288c2164dcc80101bc12ce4948de4386699c460e2108418800d0b0804db2411d2928450386cda9828d2160412932d53164d84b69000178e84c4619a46849140100c366c62404e24994d84444590c48d08084610472a19b16d00128d24170002001020148a48988c504066a21461db162e0a460c92b02dd2080923191013454021178223478002c1600942101a9105cc068ed410215436021b082660882c23a805ca8030630401523648249328c0c488cc84455a02428a4231a3002dd0160904002219445140006224462e91a8051c380e5c1888d41689c0b88da218082231481b805010139123992c501068041042d2a02d0944108b306600826898360e19419142168921c40864284624b24c50028401c3615b1612831608648671a1306000c6600932468b2204d436519b080404b50ddb248c03b425d1148004213164c868e022090409911c255008499092247192988d1c2301c3084440104c5b182e0b832ce0904d5b0490c81281d94865022470800810633824942220220722191629c9a06811c629dc2011cb0052d318001bb4001b0782ca427063b82d03c3815328250420650ab35149a0489a102d1c32098038110b4608414872189205d8260800c825530412494280d4c64859368243b40448a66808188e9b822183324940084964c64c1a301083440dda3606e1088c14c39001b10999840852202c9bb0002128859a144d021302a0824081982963301261481241b289081032583621e09849c8102c144250655dc3a4dce1a8c4712e64bf3d05e4ef23df6daeb729a2351aab7e22d37faa08b361ed7cea89c83111f099c72b1376e4968a3d821911a549b26e46ba6247511e324958bb7f9dc518034afd9d09d66c0bd355eefd2a6d4070cef60f2155b6f59fd85103ef9de1c0aff34c4d02844888a8e8b065746ba24b6267c7fa6a4e403aa3527b24220fd967acb3165055276242b7318013dfdeb890f0493929d971d09ae8011c11481b3f2265900390607a268d038cd1a0be03ebe88483154d715d67179dc844c7060641a71cfbf0ba685144f405a241c849849f76cb9ac63aaebe747248a4a6bb7d5a0f7c22e32b09fd9c5eea788fa4b872229faf65bb54084e8819fc471350fdbbb836462c8b0adf8464143e473b039019644eecd8301731a5448826fd8f83cebc4691a1137ad55dafb6179bf9564a179f11dcb841b49fa5f3fbb34237a8f4f053846987cfb8926a25cf295923fc62cd7e60f987853dd50b7155d89b0d95f9bcfcc6c557d399af233518d888a5cf78e6cfa2120196c8b9e63bc8b4ac577d8fe0f8cf0878714133a21ed6906106f1aa144b1018ff50ff3a6f535846d0b8fc26b9f2c3105af0051563ef9538e0132e03020e7fac1a714da0a62f539770626c581ecabc50358a3745fedcea00d3709fa8e7d56d0df91a0d230a18cd031e5c47247c9576dfae7fc67b326f36475035ff53c080c7bc9eb88ee5842dbbc4cfad9917a68d65b297303b8a5b73eec41c39005ea42a6ac1e3d6251acefaab289d3290974710ef78695b9041e068f8769f085367151869aa5f9c3dff07e51bfaa590eef9accfaad41b31cb1c968dfd1d2cd5353edb0b775ebff12b54e69c8e5619081f1d4cb4014c81ee6476413103420da21010a565b6e926bb4e4a259c33fbab5b12ca11b5dba9056714b42c928794e583b4f2dbf2d69ff62273e4c5ead272a7adcd1c402f7b65c3fad8f9b3bb57334292081b0b36586b094a7e3226666340ae40d0fd52e3a9ed6624acef17f02ab14bd6357aaf512da58c1e3d93d8ec46ed3330dacbd405f750d2729dfef63187f53b1fd1bf35380ae4bfca7c6c2d95150384093c8bd4d3a04bcd0c99c8b00d81493d6b0774b74bea2312c9f63fbfd4b919948f4f3a577208b9d1b5a6762cf00cdd10d59a424bbe2e8d8ee978671a42f843aeb0ff4685fa2602315d9c77b170a70250fca724c7b78e20c2f76cf3c9847823da30b7677405c38483523e08d182cfc22418957111a38ef4cc531b7844b9417497bc7b9d22d76a8b6b8ee9d18da00f61cd2033f4f32a3b971cb51cb27f10b93c3300a8fa2e35a40e3d46c1cbd4760f5fd7d6f301f83a790d559744f0dbfb12279c361642934fb6b01ea950fa6d31c13957689053ad103220edf98b91e5c318e2c4c756a2f2672437ca8b0c6c66531072a967af9040a277ee026e3aa03882f65e54125e0970bc2b95db971faa65d50161ffc70cec95a4950aa475106a85f812b547047cf22c73bd88103a6d037267dd26ac13285c7c82ae7c723ebf6d5f9e93bd59fa975ac5558b9de73c20574ea22e4182903e953db95220fd621556f4d977a1dff55f9bd21487a52582462016aeffca39b59685bdaeec8115722e8dd4c813a36b814de855887211f31a80ef26a3256eac905e510fbc87a97ea6654e72eee25bc517708cd13f3148c08a4901c66982a5c9f7cd50918859311d2ba1b9e5cb2db68271d6078fa1f73e679932f6bbb6ec4766aa978586c5ecf9922f9af2c84ab9d27b7f7c8f2536c8dc2e08b84dd16df078d99dc1b63aad28dc154d34f6db37348dbde8591354361f453d209cf5f2e09535d9ba6f1ae2fad13445fbcf2fc56a42a348c7251253b15e9854ec662d1d5e1130d0929433042a8bee136a9e7bef3d3b398d567f8431e81f5738036de0a83e7c145a472237e3c122988d1da62dec684511f3decd242834a9a80c44cb6e8497326bed9a1724ec2a3464628593cf8d03d2599ff2a338fbc0321be2b07b258939bdd24c8d3726192d2b3973b0f65072e3085dc5e21184bd26e4185774a4498391f81615b0507e40b0a420757a5b7b6dec315613414ff2479d8d96db18cde4058ab907375202bbdc9928ab9b4fc1fab5302e8a239d804315ceddae6180744d7fd1cb5be38fd963d1b7da3b59624a273f284c018bc0c600eda379915d18b61e173294fca090df0ada03203d9ca803d4bd765911a5f95ce57ac6a9fecacb05f89e873481b33d16d6ffbb43d0c3146128103d9b914a491dac6277fea7823600c5bb9b7289a16db70de25896f3ecbdc0f6ad9668af5ab8ba7e3635650d5f71102e706aecead5e267c1fb53b8bf51486bb8978504ddfdc13c3bcf9b7bc9baef58631856681e70a0c23b745ae4e0dfc9892d061f3bb56faf3d89d10c8088eed672887072d116a5a84646726175573af1cc9570f5bc2e17b5b57f9d6dbdbe4be7e0d12676c9f91b607fc432aceac38182b409d30ed32cf7b3f2fb4c75ad7a1e398883a68ac88f8f4a545b342d559852b668c5054a480fe1d1bc7af9d922ae4b6acce5725b4e8eb87a94d258d82695066d1704f8d172eabeeab7dc0b23e0515a1767fee0875827d8c6187e7e1066f42ce38732f60c624f76959495cde211a9480f2cb3f715a6e063d86c8f304d9295a2f7632e7ac293bba2fc34248de78ac4b053714e6ce1fa84cb59030fef01c0502e2597f5142875f2d8e339d09eded1587bd805c9424c9de84c4d6fd93dfdfe9eefb36a71ca2dba0836ebe52f6d5dd8005224a8f4f190ee3f0f04f3e360ce5436cf528bd84f7bbeed4b59a17cc45d812be2e321525a22617aa0fc760f970989800052474041852cb0c4b1b5ac63241dd980c3ead84ad6ed9483db5aa2984096ad7c2c13b97b128b141d30f70dd6df742a7372bb757ae438fd05ffab6e00fb0a8a132b27c89be0c64f5c873d31c7bdc60c496f3b3ca4b3cca6521139ec3f8d23bae6e8ae1697b5bc2d5f3e07f95346ef5fbdee3339a99d5eb61eafd032f1d3be8f58e1259414f751822a2693e84158c230db3b23016bee4fd0727c2bd6b754678f612265c33813071feded335a9827e5148f83f6d85a68a32b09681ac5beef8eaabfbc29fa80de24ea4c46881f324831015a70b8263f23506c69169f0e3522531589198952314fc0375fced6482cf10df8fc26d964ac0db8fcf2a0e4d708fee08b53ce1c3d1df0948b4b84075347ca0d5a116eef575cda584a165dc749193bb557e4111c76322744297298e15623f9420315c47786fd2078673aaab52c68d3ab6ab07d51756c86a4f64bfd5516da64f70c3867b61c564ab30dbd587bbe161581308f36496dcd2d506848603e3d496a7d092820cabf621d002bebdd5140fdb2d0fbf9b4ae5219c0b3992ba7ff17fecf67a386ab8a9eaf03de7c444d7f812263fe9ab78660effdca562d0fae01ee27bf225fb179fad480480d12ed9fcb3ecda2c3c1bad67f14c0f380c1b8023534517529fb1f94e69e649dc7472e6a5b1bf35a1cf539c1004e173b5d0ea436842c19a07f8cc0d8a3f1cbfafa6b2d6ef81256f2c377b8258f0948a7b1ee374ab715f12a7f4a678f0a24cf8cf6a2b4cc38930e7b2aea79743451fd8b23b112e1d8bd7fff6aa14869e45b2d5793996281a29dfbb9be580b904994e62b9a700720991841e37c46de7bab081c3f66e612409f7d74369da465b2b52a24b44f67f78daad31dc759b3c6b1454f5fc0f3c2ae16af2a0cd6b17dd15eaf223758ee5bd49ec09f62cf0354ffaa9279d5cc3fdf0ae16fa7c227118f8df674d5a38be12c0b0de9112a83fbc666de52fc14bdd26f6a93ad56103b75354e830bbd995bcead2331b9485c93817f95ef089e3805907ae96617e85af4b3c77faaecac248f24a06c471d617408eab66d788b843da28ec0733068e87e2d5c817e47dc51fb1945de02dbd8e24e191617881eda0ef4695e17749646629035619252b8ce130e881ec6a16dd52a4d6f7880fcb9f8aea636041cd30c4530ae5e47e306739bb314a515c8ada897f4a5fb1f990802db10a1b0ff2800491a409799e9a518e3451f6a88a35b4b675f0f56f3cfaeb88aa7a2ae81977b1d3975a67d551afd8148321c0eb96c6d8e4ef8580dbe9f6d593ffc17dd82388da764fa57ee63fa2b42d2600555c588a9b8550f8db1b0dd5bd97b2ef602cce495819a26d693b443f4bc9ae5dfbeab7fbb2d1d669c123a39d807f76252038c7c3e720cf91a962f616392aea34fba6d69fc9efeff48753129392262ec43c86c7e530afdf749fd8dd09907e5b39a8f61c6a98e1494d6ce7200fe3d1aa1a088ab8d80858d101e7a6319b23261942b145626112b32470d25a3035e8b97565266b2cc7197a70b0cf87c0a5d935b937e30d095b29088a9865a5698ee55e8642a3f21453cb943dbf5d53e7a53d7b5c4165c4595d62d83f4d9bf98557ca105cda24ac964c48bd4e8d24efe408091b11f9d3dc9778cea013838adfdb82662d596b0e196229b946b98eaef293010088a9e458aab39093f59e370766795ca6b5fa4596f60c9adf1f44e2586e2a31e4decef909217d37e7efc7d4f9022867603d3dccf53c54d7b51c247dd616b0d9a3160a91a662590caf6231add1474305b355fc31f36870d4cd6ced84357783c16e7e650e1bca33a00c2223e648d9a7b4c651b0dc06ee6699a818329060d225bb64e11d737b71d54c03ce8".to_string();

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
