//! Substrate Node Template CLI library.
#![warn(missing_docs)]

mod benchmarking;
mod chain_spec;
mod cli;
mod command;
mod external_miner_client;
mod prometheus;
mod rpc;
mod service;
#[cfg(test)]
mod tests;

fn main() -> sc_cli::Result<()> {
	command::run()
}
