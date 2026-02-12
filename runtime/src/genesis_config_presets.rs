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

use crate::{
	configs::TreasuryPalletId, AccountId, BalancesConfig, RuntimeGenesisConfig, SudoConfig, UNIT,
};
use alloc::{vec, vec::Vec};
use qp_dilithium_crypto::pair::{crystal_alice, crystal_charlie, dilithium_bob};
use serde_json::Value;
use sp_core::crypto::Ss58Codec;
use sp_genesis_builder::{self, PresetId};
use sp_runtime::traits::{AccountIdConversion, IdentifyAccount};

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

fn dilithium_default_accounts() -> Vec<AccountId> {
	vec![
		crystal_alice().into_account(),
		dilithium_bob().into_account(),
		crystal_charlie().into_account(),
	]
}
// Returns the genesis config presets populated with given parameters.
fn genesis_template(endowed_accounts: Vec<AccountId>, root: AccountId) -> Value {
	let mut balances = endowed_accounts
		.iter()
		.cloned()
		.map(|k| (k, 100_000 * UNIT))
		.collect::<Vec<_>>();

	const INITIAL_TREASURY: u128 = 21_000_000 * 30 * UNIT / 100; // 30% tokens go to investors
	let treasury_account = TreasuryPalletId::get().into_account_truncating();
	balances.push((treasury_account, INITIAL_TREASURY));

	let config = RuntimeGenesisConfig {
		balances: BalancesConfig { balances },
		sudo: SudoConfig { key: Some(root.clone()) },
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

	#[cfg(feature = "runtime-benchmarks")]
	{
		use crate::Runtime;
		use frame_benchmarking::v2::{account, whitelisted_caller};
		use pallet_multisig::Pallet as Multisig;

		const SEED: u32 = 0;
		let caller = whitelisted_caller::<AccountId>();
		let signer1 = account::<AccountId>("signer1", 0, SEED);
		let signer2 = account::<AccountId>("signer2", 1, SEED);
		let mut signers = vec![caller, signer1, signer2];
		signers.sort();
		let multisig_address = Multisig::<Runtime>::derive_multisig_address(&signers, 2, 0);
		let interceptor = crystal_alice().into_account();
		let delay = 10u32;

		let rt_genesis = pallet_reversible_transfers::GenesisConfig::<Runtime> {
			initial_high_security_accounts: vec![(multisig_address, interceptor, delay)],
		};

		let config = RuntimeGenesisConfig {
			balances: BalancesConfig {
				balances: endowed_accounts
					.iter()
					.cloned()
					.map(|k| (k, 100_000 * UNIT))
					.chain([(
						TreasuryPalletId::get().into_account_truncating(),
						21_000_000 * 30 * UNIT / 100,
					)])
					.collect::<Vec<_>>(),
			},
			sudo: SudoConfig { key: Some(crystal_alice().into_account()) },
			reversible_transfers: rt_genesis,
			..Default::default()
		};
		return serde_json::to_value(config).expect("Could not build genesis config.");
	}

	#[cfg(not(feature = "runtime-benchmarks"))]
	genesis_template(endowed_accounts, crystal_alice().into_account())
}

pub fn heisenberg_config_genesis() -> Value {
	let mut endowed_accounts = vec![heisenberg_root_account()];
	endowed_accounts.extend(dilithium_default_accounts());
	let ss58_version = sp_core::crypto::Ss58AddressFormat::custom(189);
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version));
	}
	genesis_template(endowed_accounts, heisenberg_root_account())
}

pub fn dirac_config_genesis() -> Value {
	let endowed_accounts = vec![dirac_root_account(), dirac_faucet_account()];
	let ss58_version = sp_core::crypto::Ss58AddressFormat::custom(189);
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version));
	}

	genesis_template(endowed_accounts, dirac_root_account())
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
