//! Substrate Node Template CLI library.
#![warn(missing_docs)]

mod benchmarking;
mod chain_spec;
mod cli;
mod command;
mod external_miner_client;
mod faucet;
mod prometheus;
mod rpc;
mod service;
#[cfg(test)]
mod tests;

fn main() -> sc_cli::Result<()> {
    sp_core::crypto::set_default_ss58_version(sp_core::crypto::Ss58AddressFormat::custom(189));
    command::run()
}
