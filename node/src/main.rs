//! Substrate Node Template CLI library.
#![warn(missing_docs)]

mod benchmarking;
mod chain_spec;
mod cli;
mod command;
mod miner_server;
mod prometheus;
mod rpc;
mod service;
#[cfg(test)]
mod tests;

#[allow(clippy::result_large_err)]
fn main() -> sc_cli::Result<()> {
	command::run()
}
