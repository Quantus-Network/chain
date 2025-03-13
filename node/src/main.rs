//! Substrate Node Template CLI library.
#![warn(missing_docs)]

mod benchmarking;
mod chain_spec;
mod cli;
mod command;
mod rpc;
mod service;
mod prometheus;

fn main() -> sc_cli::Result<()> {
	command::run()
}
