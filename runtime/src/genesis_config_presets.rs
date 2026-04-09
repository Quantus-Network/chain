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
	AccountId, AssetsConfig, BalancesConfig, RuntimeGenesisConfig, EXISTENTIAL_DEPOSIT, UNIT,
};
use alloc::{
	string::{String, ToString},
	vec,
	vec::Vec,
};
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

fn dilithium_default_accounts() -> Vec<AccountId> {
	vec![
		crystal_alice().into_account(),
		dilithium_bob().into_account(),
		crystal_charlie().into_account(),
	]
}

/// Treasury as 2-of-3 multisig from Alice, Bob, Charlie (`dilithium_default_accounts`), nonce 0.
fn development_treasury_account() -> AccountId {
	let signers = dilithium_default_accounts();
	Multisig::<crate::Runtime>::derive_multisig_address(&signers, 2, 0)
}

/// Multisig nonce for Heisenberg treasury: same three signers as dev, different on-chain address
/// from development (different nonce) so presets are distinguishable.
const HEISENBERG_TREASURY_MULTISIG_NONCE: u64 = 1;

/// Top-level genesis JSON field listing initial tech collective members as SS58 strings.
/// Stripped in [`prepare_genesis_build_input`] before deserializing [`RuntimeGenesisConfig`].
const TECH_COLLECTIVE_SEED_MEMBERS_KEY: &str = "tech_collective_seed_members";

fn heisenberg_treasury_signers() -> Vec<AccountId> {
	dilithium_default_accounts()
}

/// Treasury as 2-of-3 multisig (Alice, Bob, Charlie) with nonce
/// [`HEISENBERG_TREASURY_MULTISIG_NONCE`].
fn heisenberg_treasury_account() -> AccountId {
	Multisig::<crate::Runtime>::derive_multisig_address(
		&heisenberg_treasury_signers(),
		2,
		HEISENBERG_TREASURY_MULTISIG_NONCE,
	)
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

/// Initial tech collective members for the development preset (configurable independently of
/// treasury).
fn development_tech_collective_seed() -> Vec<AccountId> {
	dilithium_default_accounts()
}

/// Initial tech collective members for Heisenberg (defaults to the same accounts as treasury
/// signers; kept as a separate hook if the two diverge).
fn heisenberg_tech_collective_seed() -> Vec<AccountId> {
	heisenberg_treasury_signers()
}

/// Initial tech collective members for Planck (defaults to the same accounts as treasury signers).
fn planck_tech_collective_seed() -> Vec<AccountId> {
	planck_treasury_signers()
}

/// Returns the genesis config populated with given parameters. Treasury is per-profile.
///
/// The treasury account is also the `pallet-assets` owner for **asset id 0** (native-in-assets path
/// for wormhole). It is not FRAME `Root`.
///
/// All endowed addresses automatically get transfer proofs recorded, enabling them to
/// spend their funds via ZK proofs. The chain doesn't distinguish between "wormhole
/// addresses" and regular addresses - any address can spend via ZK proofs if they
/// know the corresponding secret.
fn genesis_template(
	endowed_accounts: Vec<AccountId>,
	treasury: TreasuryGenesis,
	tech_collective_members: Vec<AccountId>,
) -> Value {
	const ENDOWED_BALANCE_UNITS: u128 = 100_000;
	let mut balances = endowed_accounts
		.iter()
		.cloned()
		.map(|k| (k, ENDOWED_BALANCE_UNITS.saturating_mul(UNIT)))
		.collect::<Vec<_>>();

	let total_supply_raw = GENESIS_SUPPLY.saturating_mul(UNIT);
	let treasury_balance = treasury.portion.mul_floor(total_supply_raw);
	let treasury_account = treasury.account.clone();
	balances.push((treasury_account.clone(), treasury_balance));

	let config = RuntimeGenesisConfig {
		balances: BalancesConfig { balances: balances.clone(), dev_accounts: None },
		treasury_pallet: pallet_treasury::GenesisConfig::<crate::Runtime> {
			treasury_account: Some(treasury.account),
			treasury_portion: Some(treasury.portion),
		},
		assets: AssetsConfig {
			// Reserve asset id 0 for native token representation used with wormhole.
			assets: vec![(Zero::zero(), treasury_account, false, EXISTENTIAL_DEPOSIT)],
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

	let mut v = serde_json::to_value(config).expect("Could not build genesis config.");
	if !tech_collective_members.is_empty() {
		let arr = tech_collective_members
			.iter()
			.map(|a| Value::String(a.to_ss58check_with_version(ss58_version())))
			.collect::<Vec<_>>();
		v.as_object_mut()
			.expect("RuntimeGenesisConfig serializes to a JSON object")
			.insert(TECH_COLLECTIVE_SEED_MEMBERS_KEY.into(), Value::Array(arr));
	}
	v
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
		"🕳️  Test ZK address (use TEST_WORMHOLE_SECRET to spend): {:?}",
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
			treasury,
			development_tech_collective_seed(),
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
		genesis_template(endowed_accounts, treasury, development_tech_collective_seed())
	}
}

pub fn heisenberg_config_genesis() -> Value {
	let endowed_accounts = dilithium_default_accounts();
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version()));
	}
	let treasury = TreasuryGenesis {
		account: heisenberg_treasury_account(),
		portion: Permill::from_percent(30),
	};
	genesis_template(endowed_accounts, treasury, heisenberg_tech_collective_seed())
}

fn planck_faucet_account() -> AccountId {
	account_from_ss58("qzka7DZXAT7GnzgXQfxiSwrPKRWgW6m6G89QRsQiLThThZ6Cw")
}

fn planck_treasury_signers() -> Vec<AccountId> {
	vec![
		account_from_ss58("qzoRRfx5bUSdq2YWSXBXrmFFSwe24bNSUoMu3Vhz5hrtPri7D"),
		account_from_ss58("qzkscJp9ofGZzQbhAdySSNx3pmfKBDq9vqdfT8ZHNjp4GiwFq"),
		account_from_ss58("qzjxqVV6hzauZkBPBvvMxVv1o7ifp2XZj1J1qLTnze3yh7uhu"),
	]
}

fn planck_treasury_account() -> AccountId {
	Multisig::<crate::Runtime>::derive_multisig_address(&planck_treasury_signers(), 2, 0)
}

/// Parses genesis JSON, removes [`TECH_COLLECTIVE_SEED_MEMBERS_KEY`] if present, and returns
/// serialized config for [`frame_support::genesis_builder_helper::build_state`] plus the optional
/// member list.
pub fn prepare_genesis_build_input(
	config: Vec<u8>,
) -> Result<(Vec<u8>, Option<Vec<AccountId>>), String> {
	let mut value: Value =
		serde_json::from_slice(&config).map_err(|e| alloc::format!("genesis JSON: {e}"))?;
	let obj = value
		.as_object_mut()
		.ok_or_else(|| "genesis config JSON must be an object".to_string())?;
	let raw = obj.remove(TECH_COLLECTIVE_SEED_MEMBERS_KEY);
	let members = match raw {
		Some(v) => Some(parse_tech_collective_members_array(v)?),
		None => None,
	};
	let out = serde_json::to_vec(&value).map_err(|e| alloc::format!("{e}"))?;
	Ok((out, members))
}

fn parse_tech_collective_members_array(v: Value) -> Result<Vec<AccountId>, String> {
	let arr = v.as_array().ok_or_else(|| {
		alloc::format!("{TECH_COLLECTIVE_SEED_MEMBERS_KEY} must be a JSON array of SS58 strings")
	})?;
	let mut out = Vec::with_capacity(arr.len());
	for el in arr {
		let s = el
			.as_str()
			.ok_or_else(|| "tech collective seed member must be an SS58 string".to_string())?;
		let (account, _) = AccountId::from_ss58check_with_version(s).map_err(|e| {
			alloc::format!("invalid SS58 in {TECH_COLLECTIVE_SEED_MEMBERS_KEY}: {e:?}")
		})?;
		out.push(account);
	}
	Ok(out)
}

/// Seed tech collective members at genesis. Call after `build_state` when the genesis JSON
/// included [`TECH_COLLECTIVE_SEED_MEMBERS_KEY`].
pub fn seed_tech_collective(members: &[AccountId]) {
	if members.is_empty() {
		return;
	}
	log::info!("🏛️ Seeding tech collective with {} members", members.len());
	let ss58 = ss58_version();
	for member in members {
		log::info!(
			"🏛️ Adding tech collective member: {:?}",
			member.to_ss58check_with_version(ss58)
		);
		pallet_ranked_collective::Pallet::<crate::Runtime>::do_add_member_to_rank(
			member.clone(),
			0,
			false,
		)
		.expect("Failed to seed tech collective member");
	}
}

pub fn planck_config_genesis() -> Value {
	let mut endowed_accounts = vec![planck_faucet_account()];
	endowed_accounts.extend(dilithium_default_accounts());
	for account in endowed_accounts.iter() {
		log::info!("🍆 Endowed account: {:?}", account.to_ss58check_with_version(ss58_version()));
	}
	let treasury =
		TreasuryGenesis { account: planck_treasury_account(), portion: Permill::from_percent(30) };
	genesis_template(endowed_accounts, treasury, planck_tech_collective_seed())
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
