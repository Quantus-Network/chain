// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// this module is used by the client, so it's ok to panic/unwrap here
#![allow(clippy::expect_used)]

use crate::{AccountId, BalancesConfig, RuntimeGenesisConfig, SudoConfig, TreasuryConfigConfig};
use alloc::{vec, vec::Vec};
use qp_dilithium_crypto::pair::{
	crystal_alice, crystal_charlie, crystal_eve, dilithium_bob, dilithium_dave,
};
use serde_json::Value;
use sp_core::crypto::Ss58Codec;
use sp_genesis_builder::{self, PresetId};
use sp_runtime::traits::IdentifyAccount;

/// Identifier for the heisenberg runtime preset.
pub const HEISENBERG_RUNTIME_PRESET: &str = "heisenberg";

/// Identifier for the dirac runtime preset.
pub const DIRAC_RUNTIME_PRESET: &str = "dirac";

fn heisenberg_root_account() -> AccountId {
	account_from_ss58("qznYQKUeV5un22rXh7CCQB7Bsac74jynVDs2qbHk1hpPMjocB")
}

fn dirac_root_account() -> AccountId {
	account_from_ss58("qznYQKUeV5un22rXh7CCQB7Bsac74jynVDs2qbHk1hpPMjocB")
}
fn dirac_faucet_account() -> AccountId {
	account_from_ss58("qzn2h1xdg8N1QCLbL5BYxAikYvpVnyELtFkYqHEhwrDTx9bhr")
}

/// Treasury threshold used across all networks (3-of-5 multisig)
pub const TREASURY_THRESHOLD: u16 = 3;

// Treasury multisig signatories for development/heisenberg (5 signatories)
fn dev_treasury_signatories() -> Vec<AccountId> {
	vec![
		crystal_alice().into_account(),
		dilithium_bob().into_account(),
		crystal_charlie().into_account(),
		dilithium_dave().into_account(),
		crystal_eve().into_account(),
	]
}

// Treasury multisig signatories for dirac (mainnet) (5 signatories)
fn dirac_treasury_signatories() -> Vec<AccountId> {
	vec![
		// TODO: Replace with actual mainnet signatories
		dirac_root_account(),
		account_from_ss58("qznYQKUeV5un22rXh7CCQB7Bsac74jynVDs2qbHk1hpPMjocB"),
		account_from_ss58("qzn2h1xdg8N1QCLbL5BYxAikYvpVnyELtFkYqHEhwrDTx9bhr"),
		dirac_faucet_account(),
		account_from_ss58("qznYQKUeV5un22rXh7CCQB7Bsac74jynVDs2qbHk1hpPMjocB"), // TODO: 5th signatory
	]
}

/// Public API: Get treasury signatories and threshold for a given chain ID.
/// This matches the genesis config for that chain.
/// Returns (signatories, threshold) tuple.
pub fn get_treasury_config_for_chain(chain_id: &str) -> Option<(Vec<AccountId>, u16)> {
	match chain_id {
		"dev" => Some((dev_treasury_signatories(), TREASURY_THRESHOLD)),
		"heisenberg" => Some((dev_treasury_signatories(), TREASURY_THRESHOLD)),
		"dirac" => Some((dirac_treasury_signatories(), TREASURY_THRESHOLD)),
		_ => None,
	}
}

fn dilithium_default_accounts() -> Vec<AccountId> {
	vec![
		crystal_alice().into_account(),
		dilithium_bob().into_account(),
		crystal_charlie().into_account(),
		dilithium_dave().into_account(),
		crystal_eve().into_account(),
	]
}
// Returns the genesis config presets populated with given parameters.
fn genesis_template(
	endowed_accounts: Vec<AccountId>,
	root: AccountId,
	treasury_signatories: Vec<AccountId>,
	treasury_threshold: u16,
) -> Value {
	let balances = endowed_accounts.iter().cloned().map(|k| (k, 1u128 << 60)).collect::<Vec<_>>();

	// Note: Treasury multisig address will be computed from signatories in genesis
	// We don't pre-fund it here - funds can be transferred to it after chain initialization

	let config = RuntimeGenesisConfig {
		balances: BalancesConfig { balances },
		sudo: SudoConfig { key: Some(root.clone()) },
		treasury_config: TreasuryConfigConfig {
			signatories: treasury_signatories,
			threshold: treasury_threshold,
		},
		..Default::default()
	};

	serde_json::to_value(config).expect("Could not build genesis config.")
}

/// Return the development genesis config.
pub fn development_config_genesis() -> Value {
	let mut endowed_accounts = vec![];
	endowed_accounts.extend(dilithium_default_accounts());
	let ss58_version = sp_core::crypto::Ss58AddressFormat::custom(189);
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version));
		log::info!("🍆 Endowed account raw: {:?}", account);
	}

	genesis_template(
		endowed_accounts,
		crystal_alice().into_account(),
		dev_treasury_signatories(),
		TREASURY_THRESHOLD,
	)
}

pub fn heisenberg_config_genesis() -> Value {
	let mut endowed_accounts = vec![heisenberg_root_account()];
	endowed_accounts.extend(dilithium_default_accounts());
	let ss58_version = sp_core::crypto::Ss58AddressFormat::custom(189);
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version));
	}
	genesis_template(
		endowed_accounts,
		heisenberg_root_account(),
		dev_treasury_signatories(),
		TREASURY_THRESHOLD,
	)
}

pub fn dirac_config_genesis() -> Value {
	let endowed_accounts = vec![dirac_root_account(), dirac_faucet_account()];
	let ss58_version = sp_core::crypto::Ss58AddressFormat::custom(189);
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version));
	}

	genesis_template(
		endowed_accounts,
		dirac_root_account(),
		dirac_treasury_signatories(),
		TREASURY_THRESHOLD,
	)
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::DEV_RUNTIME_PRESET => development_config_genesis(),
		HEISENBERG_RUNTIME_PRESET => heisenberg_config_genesis(),
		DIRAC_RUNTIME_PRESET => dirac_config_genesis(),
		_ => return None,
	};
	Some(
		serde_json::to_string(&patch)
			.expect("serialization to json is expected to work. qed.")
			.into_bytes(),
	)
}

fn account_from_ss58(ss58: &str) -> AccountId {
	AccountId::from_ss58check_with_version(ss58)
		.expect("Failed to decode SS58 address")
		.0
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
	vec![
		PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
		PresetId::from(HEISENBERG_RUNTIME_PRESET),
		PresetId::from(DIRAC_RUNTIME_PRESET),
	]
}
