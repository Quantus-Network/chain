use crate::{
	benchmarking::{inherent_benchmark_data, RemarkBuilder, TransferKeepAliveBuilder},
	chain_spec,
	cli::{Cli, Subcommand},
	service,
};
use frame_benchmarking_cli::{BenchmarkCmd, ExtrinsicFactory, SUBSTRATE_REFERENCE_HARDWARE};
use rand::Rng;
use sc_cli::SubstrateCli;
use sc_service::PartialComponents;
use sp_core::crypto::AccountId32;
use resonance_runtime::{Block, EXISTENTIAL_DEPOSIT};
use sp_keyring::Sr25519Keyring;
use dilithium_crypto::ResonancePair;
use sp_wormhole::WormholePair;
use crate::cli::{ResonanceAddressType, ResonanceKeySubcommand};

impl SubstrateCli for Cli {
	fn impl_name() -> String {
		"Resonance Node".into()
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
			"" | "local" => Box::new(chain_spec::local_chain_spec()?),
			path =>
				Box::new(chain_spec::ChainSpec::from_json_file(std::path::PathBuf::from(path))?),
		})
	}
}

/// Parse and run command line arguments
pub fn run() -> sc_cli::Result<()> {
	let cli = Cli::from_args();

	match &cli.subcommand {
		Some(Subcommand::Key(cmd)) => match cmd {
			ResonanceKeySubcommand::Sc(sc_cmd) => sc_cmd.run(&cli),
			ResonanceKeySubcommand::Resonance { scheme, seed} => {


			match scheme {
				Some(ResonanceAddressType::Standard) => {
					println!("Generating resonance address...");

					let seed = match seed {
						Some(seed_str) => {
							// Accept a 64-character hex string representing 32 bytes
							if seed_str.len() != 64 {
								eprintln!("Error: Seed must be a 64-character hex string");
								eprintln!("Example: 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");
								return Err("Invalid seed length".into());
							}

							// Decode hex string to bytes
							match hex::decode(seed_str) {
								Ok(bytes) => {
									if bytes.len() != 32 {
										eprintln!("Error: Decoded seed must be exactly 32 bytes");
										return Err("Invalid seed length".into());
									}

									// Convert Vec<u8> to [u8; 32]
									let mut array = [0u8; 32];
									array.copy_from_slice(&bytes);
									array
								},
								Err(_) => {
									eprintln!("Error: Seed must be a valid hex string (0-9, a-f)");
									eprintln!("Example: 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");
									return Err("Invalid seed format".into());
								}
							}
						},
						None => {
							let mut rng = rand::thread_rng();
							let mut random_seed = [0u8; 32];
							rng.fill(&mut random_seed);

							println!("No seed provided. Using random seed:");
							println!("Seed: {}", hex::encode(&random_seed));

							random_seed
						}
					};

					let resonance_pair = ResonancePair::from_seed(&seed).unwrap();
					let account_id = AccountId32::from(resonance_pair.public());

					println!("XXXXXXXXXXXXXXX Resonance Account Details XXXXXXXXXXXXXXXXX");
					println!("Address: 0x{}", hex::encode(account_id));
					println!("Seed: {}", hex::encode(seed));
					println!("Pub key: 0x{}", hex::encode(resonance_pair.public()));
					println!("Secret: 0x{}", hex::encode(resonance_pair.secret));
					println!("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
					Ok(())

				},
				Some(ResonanceAddressType::Wormhole) => {
					println!("Generating wormhole address...");
					println!("XXXXXXXXXXXXXXX Reconance Wormhole Details XXXXXXXXXXXXXXXXX");

					let wormhole_pair = WormholePair::generate_new().unwrap();

					println!("Address: {:?}",wormhole_pair.address);
					println!("Secret: 0x{}",hex::encode(wormhole_pair.secret));

					println!("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
					Ok(())
				},
				_ => {
					println!("Error: The scheme parameter is required");
					return Err("Invalid address scheme".into());
				}
			}

		}
	},
		Some(Subcommand::BuildSpec(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.sync_run(|config| cmd.run(config.chain_spec, config.network))
		},
		Some(Subcommand::CheckBlock(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.async_run(|config| {
				let PartialComponents { client, task_manager, import_queue, .. } =
					service::new_partial(&config)?;
				Ok((cmd.run(client, import_queue), task_manager))
			})
		},
		Some(Subcommand::ExportBlocks(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.async_run(|config| {
				let PartialComponents { client, task_manager, .. } = service::new_partial(&config)?;
				Ok((cmd.run(client, config.database), task_manager))
			})
		},
		Some(Subcommand::ExportState(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.async_run(|config| {
				let PartialComponents { client, task_manager, .. } = service::new_partial(&config)?;
				Ok((cmd.run(client, config.chain_spec), task_manager))
			})
		},
		Some(Subcommand::ImportBlocks(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.async_run(|config| {
				let PartialComponents { client, task_manager, import_queue, .. } =
					service::new_partial(&config)?;
				Ok((cmd.run(client, import_queue), task_manager))
			})
		},
		Some(Subcommand::PurgeChain(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.sync_run(|config| cmd.run(config.database))
		},
		Some(Subcommand::Revert(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.async_run(|config| {
				let PartialComponents { client, task_manager, backend, .. } =
					service::new_partial(&config)?;
				let aux_revert = Box::new(|_client, _, _blocks| {
					unimplemented!("TODO - g*randpa was removed.");
				});
				Ok((cmd.run(client, backend, Some(aux_revert)), task_manager))
			})
		},
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
					},
					BenchmarkCmd::Block(cmd) => {
						let PartialComponents { client, .. } = service::new_partial(&config)?;
						cmd.run(client)
					},
					#[cfg(not(feature = "runtime-benchmarks"))]
					BenchmarkCmd::Storage(_) => Err(
						"Storage benchmarking can be enabled with `--features runtime-benchmarks`."
							.into(),
					),
					#[cfg(feature = "runtime-benchmarks")]
					BenchmarkCmd::Storage(cmd) => {
						let PartialComponents { client, backend, .. } =
							service::new_partial(&config)?;
						let db = backend.expose_db();
						let storage = backend.expose_storage();

						cmd.run(config, client, db, storage)
					},
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
					},
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
					},
					BenchmarkCmd::Machine(cmd) =>
						cmd.run(&config, SUBSTRATE_REFERENCE_HARDWARE.clone()),
				}
			})
		},
		Some(Subcommand::ChainInfo(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.sync_run(|config| cmd.run::<Block>(&config))
		},
		None => {
			let runner = cli.create_runner(&cli.run)?;
			runner.run_node_until_exit(|config| async move {
				match config.network.network_backend {
					sc_network::config::NetworkBackendType::Libp2p => service::new_full::<
						sc_network::NetworkWorker<
							resonance_runtime::opaque::Block,
							<resonance_runtime::opaque::Block as sp_runtime::traits::Block>::Hash,
						>,
					>(config, cli.rewards_address.clone(), cli.external_miner_url.clone())
					.map_err(sc_cli::Error::Service),
					sc_network::config::NetworkBackendType::Litep2p =>
						service::new_full::<sc_network::Litep2pNetworkBackend>(config, cli.rewards_address.clone(), cli.external_miner_url.clone())
							.map_err(sc_cli::Error::Service),
				}
			})
		},
	}
}
