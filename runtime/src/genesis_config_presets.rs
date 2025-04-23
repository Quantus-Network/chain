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

use crate::{AccountId, BalancesConfig, RuntimeGenesisConfig, SudoConfig};
use alloc::{vec, vec::Vec};
use dilithium_crypto::pair::{crystal_alice, dilithium_bob, crystal_charlie};
use serde_json::Value;
use sp_core::crypto::Ss58Codec;
use sp_genesis_builder::{self, PresetId};
use sp_keyring::AccountKeyring;
use sp_runtime::traits::IdentifyAccount;

/// Identifier for the live testnet runtime preset.
pub const LIVE_TESTNET_RUNTIME_PRESET: &str = "live_testnet";

// Returns the genesis config presets populated with given parameters.
fn testnet_genesis(
	endowed_accounts: Vec<AccountId>,
	root: AccountId,
) -> Value {
	let config = RuntimeGenesisConfig {
		balances: BalancesConfig {
			balances: endowed_accounts
				.iter()
				.cloned()
				.map(|k| (k, 1u128 << 60))
				.collect::<Vec<_>>(),
		},
		sudo: SudoConfig { key: Some(root) },
		..Default::default()
	};

	serde_json::to_value(config).expect("Could not build genesis config.")
}

/// Return the development genesis config.
pub fn development_config_genesis() -> Value {
    let mut endowed_accounts = vec![
        AccountKeyring::Alice.to_account_id(),
        AccountKeyring::Bob.to_account_id(),
        AccountKeyring::AliceStash.to_account_id(),
        AccountKeyring::BobStash.to_account_id(),
    ];

    // Add Dilithium-based accounts
    let dilithium_accounts = vec![
        crystal_alice().into_account(),
        dilithium_bob().into_account(),
        crystal_charlie().into_account(),
    ];
    endowed_accounts.extend(dilithium_accounts);

	//use sp_core::crypto::ByteArray;

	//log::info!("crystal_alice: {:?}", crystal_alice().public().into_account());
	//log::info!("dilithium_bob: {:?}", dilithium_bob().public().as_slice());
	//log::info!("crystal_charlie: {:?}", crystal_charlie().public().as_slice());

	// crystal_alice: 5DzUw8DMrf54xf49UeARmYvcGxJFrupDCT1SYxB3w2RXF9Eq
	// dilithium_bob: 5CxEUqBNycBAW5VvTaRXgkr4uK5HpMuS921gaTLVV9b3QYJx
	// crystal_charlie: 5Fj6VYnJGMeAPg9y5oWzEyXakZbJMGSy9VdbehdE5suDvB4t
	// crystal_alice: 553ffb66c8f627b6b6bd982ef564e144e779fc745f24241fdedac7e43f3ea486 (5DzUw8DM...)    
	// dilithium_bob: 274c9a7ecffb52c25173be718b5fcf2d383bf6e465d63a34cbc26de56efa24f0 (5CxEUqBN...)    
	// crystal_charlie: a1fc398e6f48f42c820cb3dcc3da40758a57f1a3243674ffe81832cd051c094c (5Fj6VYnJ...)    


    testnet_genesis(
        endowed_accounts,
        AccountKeyring::Alice.to_account_id(), // Keep Alice as sudo
    )
}

/// Return the live testnet genesis config.
///
/// Endows only the specified test account and sets it as Sudo.
pub fn live_testnet_config_genesis() -> Value {
    let test_account_id = AccountId::from_ss58check("5FktBKPnRkY5QvF2NmFNUNh55mJvBtgMth5QoBjFJ4E4BbFf")
        .expect("Failed to decode testnet account ID");

    let endowed_accounts = vec![test_account_id.clone()];
	log::info!("endowed account: {:?}", test_account_id.to_ss58check());

    testnet_genesis(
        endowed_accounts,
        test_account_id, // Set the test account as sudo for this testnet
	)
}

/// Return the local genesis config preset.
pub fn local_config_genesis() -> Value {
	testnet_genesis(
		AccountKeyring::iter()
			.filter(|v| v != &AccountKeyring::One && v != &AccountKeyring::Two)
			.map(|v| v.to_account_id())
			.collect::<Vec<_>>(),
		AccountKeyring::Alice.to_account_id(),
	)
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::DEV_RUNTIME_PRESET => development_config_genesis(),
		sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET => local_config_genesis(),
        LIVE_TESTNET_RUNTIME_PRESET => live_testnet_config_genesis(),
		_ => return None,
	};
	Some(
		serde_json::to_string(&patch)
			.expect("serialization to json is expected to work. qed.")
			.into_bytes(),
	)
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
	vec![
		PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
		PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
        PresetId::from(LIVE_TESTNET_RUNTIME_PRESET),
	]
}
