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
	AccountId, AssetsConfig, BalancesConfig, RuntimeGenesisConfig, SudoConfig, EXISTENTIAL_DEPOSIT,
	UNIT,
};
use alloc::{vec, vec::Vec};
use pallet_multisig::Pallet as Multisig;
use qp_dilithium_crypto::pair::{crystal_alice, crystal_charlie, dilithium_bob};
use serde_json::Value;
use sp_core::crypto::Ss58Codec;
use sp_genesis_builder::{self, PresetId};
use sp_runtime::{
	traits::{IdentifyAccount, Zero},
	Permill,
};

/// Well-known test secret for testing ZK proof spending.
/// This is a simple pattern (`[42u8; 32]`) for easy testing.
/// Use this secret with `quantus wormhole prove` to spend from the test address.
pub const TEST_WORMHOLE_SECRET: [u8; 32] = [42u8; 32];

/// Pre-computed address for TEST_WORMHOLE_SECRET.
///
/// This address was computed using: `quantus wormhole address --secret 0x2a2a...2a`
/// The derivation is: H(H("wormhole" || secret)) using the circuit's Poseidon2Hash::hash_no_pad.
/// SS58: qzokTZkdWXxMgSXyF86ECHxG8o8yRX5ibrX2Uw8YmqkHRdj1V
///
/// IMPORTANT: If you change TEST_WORMHOLE_SECRET, you must recompute this address using
/// the quantus CLI to ensure it matches what the ZK circuit expects.
const TEST_WORMHOLE_ADDRESS: [u8; 32] = [
	0xbe, 0x13, 0xa1, 0x89, 0xf9, 0x9c, 0x44, 0xa9, 0x59, 0xe2, 0x66, 0x94, 0xff, 0xe5, 0xe4, 0xba,
	0x22, 0x30, 0x92, 0xf3, 0xed, 0xbe, 0x82, 0x59, 0xc1, 0xd4, 0x5a, 0xd0, 0x8e, 0xdb, 0x40, 0x3d,
];

/// Get the test address derived from TEST_WORMHOLE_SECRET.
/// This address is endowed at genesis in the dev profile for testing ZK spending.
fn test_wormhole_account() -> AccountId {
	AccountId::new(TEST_WORMHOLE_ADDRESS)
}

/// Identifier for the heisenberg runtime preset.
pub const HEISENBERG_RUNTIME_PRESET: &str = "heisenberg";

/// Identifier for the planck runtime preset.
pub const PLANCK_RUNTIME_PRESET: &str = "planck";

/// SS58 address format used by all Quantus chains.
fn ss58_version() -> sp_core::crypto::Ss58AddressFormat {
	sp_core::crypto::Ss58AddressFormat::custom(189)
}

fn heisenberg_root_account() -> AccountId {
	account_from_ss58("qzk69g9d4d3iehaFR1gN7FJQXpfEegfT3EpiGmFei1BG5impU")
}

fn dilithium_default_accounts() -> Vec<AccountId> {
	vec![
		crystal_alice().into_account(),
		dilithium_bob().into_account(),
		crystal_charlie().into_account(),
	]
}

/// Treasury as 2-of-3 multisig derived from the three crystal/dilithium accounts (dev only).
fn development_treasury_account() -> AccountId {
	let signers = dilithium_default_accounts();
	Multisig::<crate::Runtime>::derive_multisig_address(&signers, 2, 0)
}

/// Treasury as 2-of-3 multisig derived from three heisenberg-specific accounts.
fn heisenberg_treasury_account() -> AccountId {
	let signers = vec![
		heisenberg_root_account(),
		crystal_alice().into_account(),
		crystal_charlie().into_account(),
	];
	Multisig::<crate::Runtime>::derive_multisig_address(&signers, 2, 0)
}

/// Total supply used for genesis (same portion% goes to treasury at genesis as in pallet).
const GENESIS_SUPPLY: u128 = 21_000_000;

/// Treasury genesis params per profile. Initial balance = portion of GENESIS_SUPPLY (same as
/// pallet portion).
#[derive(Clone)]
struct TreasuryGenesis {
	account: AccountId,
	portion: Permill,
}

/// Returns the genesis config populated with given parameters. Treasury is per-profile.
///
/// All endowed addresses automatically get transfer proofs recorded, enabling them to
/// spend their funds via ZK proofs. The chain doesn't distinguish between "wormhole
/// addresses" and regular addresses - any address can spend via ZK proofs if they
/// know the corresponding secret.
fn genesis_template(
	endowed_accounts: Vec<AccountId>,
	root: AccountId,
	treasury: TreasuryGenesis,
) -> Value {
	const ENDOWED_BALANCE_UNITS: u128 = 100_000;
	let mut balances = endowed_accounts
		.iter()
		.cloned()
		.map(|k| (k, ENDOWED_BALANCE_UNITS.saturating_mul(UNIT)))
		.collect::<Vec<_>>();

	let total_supply_raw = GENESIS_SUPPLY.saturating_mul(UNIT);
	let treasury_balance = treasury.portion.mul_floor(total_supply_raw);
	balances.push((treasury.account.clone(), treasury_balance));

	let config = RuntimeGenesisConfig {
		balances: BalancesConfig { balances: balances.clone(), dev_accounts: None },
		sudo: SudoConfig { key: Some(root.clone()) },
		treasury_pallet: pallet_treasury::GenesisConfig::<crate::Runtime> {
			treasury_account: Some(treasury.account),
			treasury_portion: Some(treasury.portion),
		},
		assets: AssetsConfig {
			// We need to initialize and reserve the first asset id for the native token transfers
			// with wormhole.
			assets: vec![(Zero::zero(), root.clone(), false, EXISTENTIAL_DEPOSIT)], /* (asset_id,
			                                                                         * owner, is_sufficient,
			                                                                         * min_balance) */
			..Default::default()
		},
		wormhole: pallet_wormhole::GenesisConfig::<crate::Runtime> {
			// Record transfer proofs for ALL endowed addresses, enabling ZK spending.
			// Events are emitted in on_initialize at block 1 for indexer compatibility.
			endowed_addresses: balances
				.into_iter()
				.map(|(account, amount)| (account.into(), amount))
				.collect(),
		},
		..Default::default()
	};

	serde_json::to_value(config).expect("Could not build genesis config.")
}

/// Return the development genesis config.
pub fn development_config_genesis() -> Value {
	let mut endowed_accounts = vec![];
	endowed_accounts.extend(dilithium_default_accounts());

	// Add the test address derived from TEST_WORMHOLE_SECRET.
	// This is useful for testing ZK spending with a known secret.
	let test_account = test_wormhole_account();
	endowed_accounts.push(test_account.clone());

	let ss58_version = sp_core::crypto::Ss58AddressFormat::custom(189);
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version));
	}
	log::info!(
		"🕳️ Test ZK address (use TEST_WORMHOLE_SECRET to spend): {:?}",
		test_account.to_ss58check_with_version(ss58_version)
	);

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

		let treasury = TreasuryGenesis {
			account: development_treasury_account(),
			portion: Permill::from_percent(30),
		};
		let mut config: RuntimeGenesisConfig = serde_json::from_value(genesis_template(
			endowed_accounts,
			crystal_alice().into_account(),
			treasury,
		))
		.expect("genesis_template returns valid config");
		config.reversible_transfers = rt_genesis;
		return serde_json::to_value(config).expect("Could not build genesis config.");
	}

	#[cfg(not(feature = "runtime-benchmarks"))]
	{
		let treasury = TreasuryGenesis {
			account: development_treasury_account(),
			portion: Permill::from_percent(30),
		};
		genesis_template(endowed_accounts, crystal_alice().into_account(), treasury)
	}
}

pub fn heisenberg_config_genesis() -> Value {
	let mut endowed_accounts = vec![heisenberg_root_account()];
	endowed_accounts.extend(dilithium_default_accounts());
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version()));
	}
	let treasury = TreasuryGenesis {
		account: heisenberg_treasury_account(),
		portion: Permill::from_percent(30),
	};
	genesis_template(endowed_accounts, heisenberg_root_account(), treasury)
}

fn planck_root_account() -> AccountId {
	account_from_ss58("qzk69g9d4d3iehaFR1gN7FJQXpfEegfT3EpiGmFei1BG5impU")
}

fn planck_faucet_account() -> AccountId {
	account_from_ss58("qzka7DZXAT7GnzgXQfxiSwrPKRWgW6m6G89QRsQiLThThZ6Cw")
}

fn planck_treasury_account() -> AccountId {
	let signers = vec![
		account_from_ss58("qzoRRfx5bUSdq2YWSXBXrmFFSwe24bNSUoMu3Vhz5hrtPri7D"),
		account_from_ss58("qzkscJp9ofGZzQbhAdySSNx3pmfKBDq9vqdfT8ZHNjp4GiwFq"),
		account_from_ss58("qzjxqVV6hzauZkBPBvvMxVv1o7ifp2XZj1J1qLTnze3yh7uhu"),
	];
	Multisig::<crate::Runtime>::derive_multisig_address(&signers, 2, 0)
}

pub fn planck_config_genesis() -> Value {
	let mut endowed_accounts = vec![planck_root_account(), planck_faucet_account()];
	endowed_accounts.extend(dilithium_default_accounts());
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version()));
	}
	let treasury =
		TreasuryGenesis { account: planck_treasury_account(), portion: Permill::from_percent(30) };
	genesis_template(endowed_accounts, planck_root_account(), treasury)
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::DEV_RUNTIME_PRESET => development_config_genesis(),
		HEISENBERG_RUNTIME_PRESET => heisenberg_config_genesis(),
		PLANCK_RUNTIME_PRESET => planck_config_genesis(),
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
		PresetId::from(PLANCK_RUNTIME_PRESET),
	]
}
