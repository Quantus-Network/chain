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
            "dev" => Box::new(chain_spec::development_chain_spec()?),
            "live_resonance_local" => Box::new(chain_spec::live_testnet_chain_spec()?),
            "live_resonance" => Box::new(chain_spec::ChainSpec::from_json_bytes(include_bytes!(
                "chain-specs/live-resonance.json"
            ))?),
            "" | "local" => Box::new(chain_spec::local_chain_spec()?),
            path => Box::new(chain_spec::ChainSpec::from_json_file(
                std::path::PathBuf::from(path),
            )?),
        })
    }
}

/// Parse and run command line arguments
pub fn run() -> sc_cli::Result<()> {
    let cli = Cli::from_args();

    match &cli.subcommand {
        Some(Subcommand::Key(cmd)) => match cmd {
            QuantusKeySubcommand::Sc(sc_cmd) => sc_cmd.run(&cli),
            QuantusKeySubcommand::Quantus {
                scheme,
                seed_hex,
                words,
            } => {
                match scheme {
                    Some(QuantusAddressType::Standard) => {
                        println!("Generating Quantus Standard address...");

                        let actual_seed_for_pair: Vec<u8>;
                        let mut words_to_print: Option<String> = None;

                        if let Some(words_phrase) = words {
                            println!("Using provided words phrase...");
                            let hd_lattice = HDLattice::from_mnemonic(words_phrase, None)
                                .map_err(|e| {
                                    eprintln!("Error processing provided words: {:?}", e);
                                    sc_cli::Error::Input("Failed to process provided words".into())
                                })?;
                            actual_seed_for_pair = hd_lattice.seed.to_vec(); // Assumes HDLattice.seed is pub
                            words_to_print = Some(words_phrase.clone());
                        } else if let Some(hex_seed_str) = seed_hex {
                            println!("Using provided hex seed...");
                            if hex_seed_str.len() != 64 {
                                eprintln!("Error: --seed-hex must be a 64-character hex string (for a 32-byte seed).");
                                return Err("Invalid hex seed length".into());
                            }
                            let decoded_seed_bytes = hex::decode(hex_seed_str).map_err(|_| {
                                eprintln!("Error: --seed-hex must be a valid hex string (0-9, a-f).");
                                sc_cli::Error::Input("Invalid hex seed format".into())
                            })?;
                            if decoded_seed_bytes.len() != 32 {
                                eprintln!("Error: Decoded hex seed must be exactly 32 bytes.");
                                return Err("Invalid decoded hex seed length".into());
                            }
                            actual_seed_for_pair = decoded_seed_bytes;
                        } else {
                            println!("No seed or words provided. Generating a new 24-word phrase...");
                            let new_words = generate_mnemonic(24).map_err(|e| {
                                eprintln!("Error generating new words: {:?}", e);
                                sc_cli::Error::Input("Failed to generate new words".into())
                            })?;
                            println!("Secret phrase: {}", new_words); // Print the new words

                            let hd_lattice = HDLattice::from_mnemonic(&new_words, None)
                                .map_err(|e| {
                                    eprintln!("Error creating HD lattice from new words: {:?}", e);
                                    sc_cli::Error::Input("Failed to process new words".into())
                                })?;
                            actual_seed_for_pair = hd_lattice.seed.to_vec(); // Assumes HDLattice.seed is pub
                            words_to_print = Some(new_words);
                        }

                        let resonance_pair = ResonancePair::from_seed(&actual_seed_for_pair)
                            .map_err(|e| {
                                eprintln!("Error creating ResonancePair: {:?}", e);
                                sc_cli::Error::Input("Failed to create keypair".into())
                            })?;

                        let account_id = AccountId32::from(resonance_pair.public());

                        println!("XXXXXXXXXXXXXXX Quantus Account Details XXXXXXXXXXXXXXXXX");
                        if let Some(phrase) = words_to_print {
                            println!("Secret phrase: {}", phrase);
                        }
                        println!("Address: {}", account_id.to_ss58check());
                        println!("Pub key: 0x{}", hex::encode(resonance_pair.public()));
                        println!(
                            "Secret key (derived private key hex): 0x{}",
                            hex::encode(resonance_pair.secret)
                        );
                        println!("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
                        Ok(())
                    }
                    Some(QuantusAddressType::Wormhole) => {
                        println!("Generating wormhole address...");
                        println!("XXXXXXXXXXXXXXX Reconance Wormhole Details XXXXXXXXXXXXXXXXX");

                        let wormhole_pair = WormholePair::generate_new().unwrap();

                        println!("Address: {:?}", wormhole_pair.address);
                        println!("Secret: 0x{}", hex::encode(wormhole_pair.secret));

                        println!("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
                        Ok(())
                    }
                    _ => {
                        println!("Error: The scheme parameter is required for 'quantus key quantus'");
                        Err("Invalid address scheme or scheme not provided".into())
                    }
                }
            }
        },
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
