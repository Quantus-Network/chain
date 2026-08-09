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
	AccountId, BalancesConfig, RuntimeGenesisConfig, EXISTENTIAL_DEPOSIT, MILLIS_PER_DAY, UNIT,
};
use alloc::{
	string::{String, ToString},
	vec,
	vec::Vec,
};
use pallet_multisig::Pallet as Multisig;
use qp_dilithium_crypto::{
	pair::{crystal_alice, crystal_charlie, dilithium_bob},
	Dilithium87Pair,
};
use serde_json::Value;
use sp_core::{crypto::Ss58Codec, Pair};
use sp_genesis_builder::{self, PresetId};
use sp_runtime::{traits::IdentifyAccount, Permill};

/// Minimum tech-collective size the tech-referenda approval/support curves in
/// [`crate::governance::definitions`] are designed for (see the 5-member analysis on
/// `TechCollectiveTracksInfo`). A non-empty genesis seed smaller than this would let a minority
/// authorize Root, so [`seed_tech_collective`] rejects it (fail-early).
pub const MIN_TECH_COLLECTIVE_MEMBERS: usize = 5;

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

/// Milliseconds since the unix epoch, the time basis of vesting schedules.
type VestingMoment = u64;

/// One vesting genesis entry: `(beneficiary, start_ms, cliff_ms, end_ms, total)`.
type VestingScheduleTuple = (AccountId, VestingMoment, VestingMoment, VestingMoment, u128);

const fn days_ms(days: u64) -> VestingMoment {
	days * MILLIS_PER_DAY
}

/// Genesis vesting starts at this wall-clock time: **2026-08-05 00:00:00 UTC**.
/// Re-derive this (midnight UTC of the intended start date) before any real launch.
const GENESIS_VESTING_START_MS: VestingMoment = 1_785_888_000_000;
/// Testnet example cliff: 90 days after start.
const GENESIS_VESTING_CLIFF_MS: VestingMoment = GENESIS_VESTING_START_MS + days_ms(90);
/// Testnet example vesting end: 1 year after start.
const GENESIS_VESTING_END_MS: VestingMoment = GENESIS_VESTING_START_MS + days_ms(365);
/// Testnet example grant size.
const GENESIS_VESTING_TOTAL: u128 = 10_000 * UNIT;

/// Identifier for the heisenberg runtime preset.
///
/// Heisenberg is the internal integration testnet. Its genesis deliberately
/// reuses [`dilithium_default_accounts`] (well-known public keys) — see that
/// helper for why that is acceptable here and must not be copied onto a
/// mainnet / value-bearing chain.
pub const HEISENBERG_RUNTIME_PRESET: &str = "heisenberg";

/// Identifier for the planck runtime preset.
pub const PLANCK_RUNTIME_PRESET: &str = "planck";

/// SS58 address format used by all Quantus chains.
fn ss58_version() -> sp_core::crypto::Ss58AddressFormat {
	sp_core::crypto::Ss58AddressFormat::custom(189)
}

/// Well-known Dilithium accounts used by the `dev` and `heisenberg` presets
/// (`crystal_alice` / `dilithium_bob` / `crystal_charlie`, derived from the
/// public seeds `[0u8; 32]` / `[1u8; 32]` / `[2u8; 32]`).
///
/// These keys are intentionally public. That is fine for local development
/// (`dev`) and for Heisenberg, which is an **integration testnet** with no
/// monetary value, where CI and integrators need reproducible endowed accounts,
/// treasury signers, and tech-collective members without secret distribution.
/// It would **not** be acceptable for a mainnet or any chain whose tokens or
/// governance have real-world value — those must use unique, privately held
/// keys (as Planck does for its live treasury signers).
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

/// Multisig nonce for Heisenberg treasury: same three well-known signers as
/// `dev` (acceptable because Heisenberg is an integration testnet — see
/// [`dilithium_default_accounts`]), different on-chain address from development
/// (different nonce) so presets are distinguishable.
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

/// Treasury genesis params per profile. The portion only configures the ongoing mining-reward
/// split; the treasury receives NO genesis endowment (no pre-mine) and accumulates solely from
/// its share of block rewards.
#[derive(Clone)]
struct TreasuryGenesis {
	account: AccountId,
	portion: Permill,
}

/// Two extra well-known Dilithium accounts (public seeds `[3u8; 32]` / `[4u8; 32]`) that pad the
/// `dev` and `heisenberg` tech collectives to the [`MIN_TECH_COLLECTIVE_MEMBERS`] size the
/// tech-referenda curves are designed for. These public keys are acceptable only for those
/// non-value-bearing chains — see [`dilithium_default_accounts`].
fn dilithium_extra_collective_members() -> Vec<AccountId> {
	[[3u8; 32], [4u8; 32]]
		.into_iter()
		.map(|seed| {
			Dilithium87Pair::from_seed_slice(&seed)
				.expect("static 32-byte seed is valid")
				.into_account()
		})
		.collect()
}

/// Initial tech collective members for the development preset. Grown to
/// [`MIN_TECH_COLLECTIVE_MEMBERS`] so the tech-referenda curves behave as designed.
fn development_tech_collective_seed() -> Vec<AccountId> {
	let mut members = dilithium_default_accounts();
	members.extend(dilithium_extra_collective_members());
	members
}

/// Initial tech collective members for Heisenberg. Grown to [`MIN_TECH_COLLECTIVE_MEMBERS`] so the
/// tech-referenda curves behave as designed.
fn heisenberg_tech_collective_seed() -> Vec<AccountId> {
	let mut members = heisenberg_treasury_signers();
	members.extend(dilithium_extra_collective_members());
	members
}

/// Initial tech collective members for Planck: the three treasury signers plus two dedicated
/// members, giving the [`MIN_TECH_COLLECTIVE_MEMBERS`] the tech-referenda curves assume.
fn planck_tech_collective_seed() -> Vec<AccountId> {
	let mut members = planck_treasury_signers();
	members.extend([
		account_from_ss58("qzmTAz3UUw1WGUuVh8nbFmPwcftomduwy6twq6NDR6y9qqtEs"),
		account_from_ss58("qzm5QCox8Dp5A3oSXZZYHD8YoYgPz7enykZb6RPUropdCyN5h"),
	]);
	members
}

/// Returns the genesis config populated with given parameters. Treasury is per-profile.
///
/// All endowed addresses automatically get transfer proofs recorded at block 1 (the
/// wormhole pallet derives them from the genesis balances — there is no separate
/// endowment list), enabling them to spend their funds via ZK proofs. The chain doesn't
/// distinguish between "wormhole addresses" and regular addresses - any address can
/// spend via ZK proofs if they know the corresponding secret.
fn genesis_template(
	endowed_accounts: Vec<AccountId>,
	treasury: TreasuryGenesis,
	tech_collective_members: Vec<AccountId>,
	extra_balances: Vec<(AccountId, u128)>,
	vesting_schedules: Vec<VestingScheduleTuple>,
) -> Value {
	const ENDOWED_BALANCE_UNITS: u128 = 100_000;
	let mut balances = endowed_accounts
		.iter()
		.cloned()
		.map(|k| (k, ENDOWED_BALANCE_UNITS.saturating_mul(UNIT)))
		.collect::<Vec<_>>();
	balances.extend(extra_balances);

	// No pre-mine: the treasury starts at zero balance and is funded only by its share of
	// mining rewards. It is intentionally NOT added to `balances`.

	// The pot must hold exactly the sum of all schedule totals plus its existential-
	// deposit buffer (asserted by the vesting pallet's genesis build). It is endowed
	// with at least the ED even when no schedules exist, so `create_schedule` works on
	// every chain from day one. Wormhole transfer proofs at block 1 derive from these
	// balances (including the pot); the pot is keyless so its leaf is unspendable.
	let mut vesting_total: u128 = 0;
	for (_, _, _, _, total) in &vesting_schedules {
		vesting_total = vesting_total
			.checked_add(*total)
			.expect("vesting genesis allocation overflows u128");
	}
	let pot_account = pallet_vesting::Pallet::<crate::Runtime>::pot_account_id();
	balances.push((
		pot_account,
		vesting_total
			.checked_add(EXISTENTIAL_DEPOSIT)
			.expect("vesting pot endowment overflows u128"),
	));

	let config = RuntimeGenesisConfig {
		balances: BalancesConfig { balances, dev_accounts: None },
		treasury_pallet: pallet_treasury::GenesisConfig::<crate::Runtime> {
			treasury_account: Some(treasury.account),
			treasury_portion: Some(treasury.portion),
		},
		vesting: pallet_vesting::GenesisConfig::<crate::Runtime> { schedules: vesting_schedules },
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

/// Testnet vesting table for `dev` and `heisenberg`: Bob holds two schedules
/// (exercises multi-schedule-per-account), Charlie one.
fn testnet_vesting_schedules() -> Vec<VestingScheduleTuple> {
	let accounts = dilithium_default_accounts();
	vec![
		(
			accounts[1].clone(),
			GENESIS_VESTING_START_MS,
			GENESIS_VESTING_CLIFF_MS,
			GENESIS_VESTING_END_MS,
			GENESIS_VESTING_TOTAL,
		),
		(
			accounts[1].clone(),
			GENESIS_VESTING_START_MS,
			GENESIS_VESTING_CLIFF_MS,
			GENESIS_VESTING_END_MS,
			GENESIS_VESTING_TOTAL,
		),
		(
			accounts[2].clone(),
			GENESIS_VESTING_START_MS,
			GENESIS_VESTING_CLIFF_MS,
			GENESIS_VESTING_END_MS,
			GENESIS_VESTING_TOTAL,
		),
	]
}

/// Dev additionally vests the keyless test wormhole address: it can never sign, so its
/// grants are claimable only via the permissionless third-party `claim` — exercising the
/// exact path wormhole beneficiaries rely on.
fn development_vesting_schedules() -> Vec<VestingScheduleTuple> {
	let mut schedules = testnet_vesting_schedules();
	schedules.push((
		test_wormhole_account(),
		GENESIS_VESTING_START_MS,
		GENESIS_VESTING_CLIFF_MS,
		GENESIS_VESTING_END_MS,
		GENESIS_VESTING_TOTAL,
	));
	schedules
}

fn log_vesting_schedules(preset: &str, schedules: &[VestingScheduleTuple]) {
	let ss58 = ss58_version();
	let pot = pallet_vesting::Pallet::<crate::Runtime>::pot_account_id();
	log::info!("[{preset}] 🪙 Vesting pot: {:?}", pot.to_ss58check_with_version(ss58));
	for (beneficiary, start, cliff, end, total) in schedules {
		log::info!(
			"[{preset}] 🪙 Vesting: {:?} total={total} start={start} cliff={cliff} end={end}",
			beneficiary.to_ss58check_with_version(ss58),
		);
	}
}

fn log_genesis_accounts(
	preset: &str,
	endowed: &[AccountId],
	treasury_account: &AccountId,
	treasury_signers: &[AccountId],
	tech_collective: &[AccountId],
) {
	let ss58 = ss58_version();
	for account in endowed {
		log::info!("[{preset}] 💰 Endowed: {:?}", account.to_ss58check_with_version(ss58));
	}
	log::info!("[{preset}] 🏦 Treasury: {:?}", treasury_account.to_ss58check_with_version(ss58));
	for signer in treasury_signers {
		log::info!("[{preset}] 🔑 Treasury signer: {:?}", signer.to_ss58check_with_version(ss58));
	}
	for member in tech_collective {
		log::info!("[{preset}] 🏛️  Tech collective: {:?}", member.to_ss58check_with_version(ss58));
	}
}

/// Return the development genesis config.
pub fn development_config_genesis() -> Value {
	let mut endowed_accounts = dilithium_default_accounts();
	let test_account = test_wormhole_account();
	endowed_accounts.push(test_account.clone());
	let treasury_account = development_treasury_account();
	let tech_collective = development_tech_collective_seed();
	log_genesis_accounts(
		"dev",
		&endowed_accounts,
		&treasury_account,
		&dilithium_default_accounts(),
		&tech_collective,
	);
	log::info!("[dev] 🕳️  Test ZK: {:?}", test_account.to_ss58check_with_version(ss58_version()));
	let vesting_schedules = development_vesting_schedules();
	log_vesting_schedules("dev", &vesting_schedules);

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
		let guardian = crystal_alice().into_account();
		let delay = 10u32;

		let rt_genesis = pallet_reversible_transfers::GenesisConfig::<Runtime> {
			initial_high_security_accounts: vec![(multisig_address, guardian, delay)],
		};

		let treasury =
			TreasuryGenesis { account: treasury_account, portion: Permill::from_percent(50) };
		let mut template_value = genesis_template(
			endowed_accounts,
			treasury,
			tech_collective,
			vec![],
			vesting_schedules,
		);
		// `genesis_template` adds a chain-spec-only field that `RuntimeGenesisConfig` cannot
		// deserialize; strip it before deserializing, then restore it on the returned JSON so
		// `build_state` still seeds the tech collective (otherwise a benchmark dev chain starts
		// with an empty collective and nobody can pass RootOrMemberForTechReferendaOrigin).
		let tech_collective_members = template_value
			.as_object_mut()
			.expect("RuntimeGenesisConfig serializes to a JSON object")
			.remove(TECH_COLLECTIVE_SEED_MEMBERS_KEY);
		let mut config: RuntimeGenesisConfig =
			serde_json::from_value(template_value).expect("genesis_template returns valid config");
		config.reversible_transfers = rt_genesis;
		let mut out = serde_json::to_value(config).expect("Could not build genesis config.");
		if let Some(members) = tech_collective_members {
			out.as_object_mut()
				.expect("RuntimeGenesisConfig serializes to a JSON object")
				.insert(TECH_COLLECTIVE_SEED_MEMBERS_KEY.into(), members);
		}
		return out;
	}

	#[cfg(not(feature = "runtime-benchmarks"))]
	{
		let treasury =
			TreasuryGenesis { account: treasury_account, portion: Permill::from_percent(50) };
		genesis_template(endowed_accounts, treasury, tech_collective, vec![], vesting_schedules)
	}
}

pub fn heisenberg_config_genesis() -> Value {
	let endowed_accounts = dilithium_default_accounts();
	let treasury_signers = heisenberg_treasury_signers();
	let tech_collective = heisenberg_tech_collective_seed();
	let treasury_account = heisenberg_treasury_account();
	log_genesis_accounts(
		"heisenberg",
		&endowed_accounts,
		&treasury_account,
		&treasury_signers,
		&tech_collective,
	);
	let vesting_schedules = testnet_vesting_schedules();
	log_vesting_schedules("heisenberg", &vesting_schedules);
	let treasury =
		TreasuryGenesis { account: treasury_account, portion: Permill::from_percent(50) };
	genesis_template(endowed_accounts, treasury, tech_collective, vec![], vesting_schedules)
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
///
/// # Trust model (deliberately no size limits)
///
/// This runs inside the `GenesisBuilder` runtime API, which is only invoked by the node
/// operator's own tooling (chain-spec building / genesis initialization) with the chain
/// spec that operator chose to launch. It is not reachable by network peers or on a
/// running chain. Whoever supplies this JSON already controls *everything* about the
/// chain being built — balances, keys, code — so input-size bounds here would not
/// protect anyone: an oversized or hostile genesis can only stall the chain of the
/// operator who supplied it. This matches upstream Substrate, whose `build_state`
/// helper deserializes the full unbounded config the same way.
///
/// The same reasoning covers failure semantics: semantically invalid genesis data
/// (duplicate balance entries, sub-ED endowments, ...) *panics* inside the pallets'
/// `BuildGenesisConfig::build` rather than returning `Err`. That is FRAME's design —
/// `build` returns `()` and has no error channel; only JSON deserialization (which runs
/// before the trait) can return `Err`. The panics are inherited verbatim from upstream
/// Substrate and are the intended fail-fast: they abort the operator's own chain-spec
/// build with the assertion message, and the failed build's candidate storage is
/// discarded, so nothing half-built can persist.
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
///
/// The member list is caller-supplied via the genesis JSON, so adding can fail (duplicate
/// entries, accounts already members, `MaxMemberCount` exceeded). Failures are returned as
/// `Err(String)` for `build_state` to surface through `sp_genesis_builder::Result` instead
/// of trapping the runtime call.
pub fn seed_tech_collective(members: &[AccountId]) -> Result<(), String> {
	if members.is_empty() {
		return Ok(());
	}
	if members.len() < MIN_TECH_COLLECTIVE_MEMBERS {
		return Err(alloc::format!(
			"tech collective seed has {} members; the governance curves require at least {}",
			members.len(),
			MIN_TECH_COLLECTIVE_MEMBERS
		));
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
		.map_err(|e| {
			alloc::format!(
				"failed to seed tech collective member {}: {e:?}",
				member.to_ss58check_with_version(ss58)
			)
		})?;
	}
	Ok(())
}

pub fn planck_config_genesis() -> Value {
	let treasury_signers = planck_treasury_signers();
	let tech_collective = planck_tech_collective_seed();
	let treasury_account = planck_treasury_account();
	let endowed_accounts = vec![planck_faucet_account()];
	// Each signer needs enough to create + propose on the treasury multisig once: MultisigFee
	// (0.6) burned + ProposalFee (~1.03) burned + ProposalDeposit (1.0) reserved = ~2.63 UNIT,
	// plus transaction fees and the existential deposit. 1 UNIT was insufficient.
	let signer_fee_seed: Vec<_> = treasury_signers.iter().cloned().map(|a| (a, 3 * UNIT)).collect();
	log_genesis_accounts(
		"planck",
		&endowed_accounts,
		&treasury_account,
		&treasury_signers,
		&tech_collective,
	);
	// No vesting allocations on Planck; the pot still receives its ED buffer so
	// `create_schedule` works post-genesis.
	log_vesting_schedules("planck", &[]);
	let treasury =
		TreasuryGenesis { account: treasury_account, portion: Permill::from_percent(50) };
	genesis_template(endowed_accounts, treasury, tech_collective, signer_fee_seed, vec![])
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

#[cfg(test)]
mod tests {
	use super::*;
	use sp_runtime::BuildStorage;

	#[test]
	fn seed_tech_collective_rejects_undersized_seed() {
		let too_few: Vec<AccountId> = (0..(MIN_TECH_COLLECTIVE_MEMBERS as u8 - 1))
			.map(|i| AccountId::new([i; 32]))
			.collect();
		assert!(seed_tech_collective(&too_few).is_err());
		// An absent seed (empty) stays valid: it just means the collective is not seeded here.
		assert!(seed_tech_collective(&[]).is_ok());
	}

	#[test]
	fn all_presets_meet_the_tech_collective_floor() {
		for seed in [
			development_tech_collective_seed(),
			heisenberg_tech_collective_seed(),
			planck_tech_collective_seed(),
		] {
			assert!(seed.len() >= MIN_TECH_COLLECTIVE_MEMBERS);
		}
	}

	/// 2020-01-01 and 2100-01-01 UTC, sanity bounds for genesis vesting dates.
	const YEAR_2020_MS: u64 = 1_577_836_800_000;
	const YEAR_2100_MS: u64 = 4_102_444_800_000;

	#[test]
	fn days_ms_is_exact() {
		assert_eq!(days_ms(1), 86_400_000);
		assert_eq!(days_ms(365), 31_536_000_000);
	}

	#[test]
	fn genesis_vesting_times_are_sane() {
		assert_eq!(GENESIS_VESTING_START_MS % MILLIS_PER_DAY, 0, "start must be midnight UTC");
		assert!(GENESIS_VESTING_START_MS > YEAR_2020_MS);
		assert!(GENESIS_VESTING_START_MS < YEAR_2100_MS);
		assert!(GENESIS_VESTING_START_MS <= GENESIS_VESTING_CLIFF_MS);
		assert!(GENESIS_VESTING_CLIFF_MS <= GENESIS_VESTING_END_MS);
		assert!(GENESIS_VESTING_START_MS < GENESIS_VESTING_END_MS);
	}

	/// Every shipped preset must build genesis storage, including the vesting pallet's
	/// pot-balance assertions. Wormhole transfer proofs need no preset entry: they are
	/// derived from genesis balances at block 1.
	#[test]
	fn all_presets_build_genesis_storage() {
		for id in preset_names() {
			let bytes = get_preset(&id).expect("listed preset must resolve");
			let (config_bytes, _members) = prepare_genesis_build_input(bytes)
				.unwrap_or_else(|e| panic!("preset {:?}: invalid genesis JSON: {e}", id));
			let config: RuntimeGenesisConfig = serde_json::from_slice(&config_bytes)
				.unwrap_or_else(|e| panic!("preset {:?} must deserialize: {e}", id));
			config
				.build_storage()
				.unwrap_or_else(|e| panic!("preset {:?} must build genesis storage: {e:?}", id));
		}
	}

	/// The vesting pot's genesis endowment must exactly cover the schedule table.
	#[test]
	fn preset_pot_endowment_matches_schedules() {
		let raw = get_preset(&PresetId::from(HEISENBERG_RUNTIME_PRESET)).expect("preset exists");
		let (json, _) = prepare_genesis_build_input(raw).expect("well-formed");
		let config: RuntimeGenesisConfig = serde_json::from_slice(&json).expect("deserializes");
		let pot = pallet_vesting::Pallet::<crate::Runtime>::pot_account_id();
		let pot_balance = config
			.balances
			.balances
			.iter()
			.find(|(who, _)| *who == pot)
			.map(|(_, amount)| *amount)
			.expect("pot must be endowed");
		let schedule_sum: u128 =
			config.vesting.schedules.iter().map(|(_, _, _, _, total)| *total).sum();
		assert_eq!(pot_balance, schedule_sum + EXISTENTIAL_DEPOSIT);
	}

	/// Every advertised vesting schedule must lock for the published 90-day cliff.
	/// Regression: Bob's second testnet schedule shipped with the start timestamp in
	/// its cliff field, so it accrued linearly from day 0 in both `dev` and
	/// `heisenberg` instead of honoring the 90-day lock.
	#[test]
	fn preset_vesting_schedules_enforce_the_published_cliff() {
		let mut checked = 0usize;
		for id in preset_names() {
			let raw = get_preset(&id).expect("listed preset must resolve");
			let (json, _) = prepare_genesis_build_input(raw).expect("well-formed");
			let config: RuntimeGenesisConfig =
				serde_json::from_slice(&json).expect("deserializes");
			for (who, start, cliff, end, _) in &config.vesting.schedules {
				assert_eq!(
					*cliff,
					start + days_ms(90),
					"preset {id:?}: schedule for {who:?} must lock for the 90-day cliff"
				);
				assert!(cliff < end, "preset {id:?}: cliff must precede the vesting end");
				checked += 1;
			}
		}
		assert!(checked >= 4, "dev and heisenberg must carry the example vesting schedules");
	}
}
