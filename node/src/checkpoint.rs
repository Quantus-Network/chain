//! Warp sync checkpoint resolution.
//!
//! A checkpoint is a single trusted header: the warp sync target whose
//! `state_root` the downloaded state is proof-verified against. Everything
//! else about fast sync is verified; the checkpoint header is the one input
//! that must come from a trusted place. Resolution ladder (first match wins):
//!
//! 1. `--checkpoint-header` — operator-pinned SCALE-encoded header hex.
//! 2. Fetched from checkpoint RPC endpoints (`--checkpoint-url`, or the chain spec `checkpointUrls`
//!    property): the network's current finalized head. Every endpoint that responds must agree on
//!    the chosen block, and the result must not contradict the release anchor.
//! 3. The release anchor baked into the chain spec (`checkpointHeader` property) — refreshed by CI
//!    at release cut. Stale anchors still sync, they just re-execute the blocks mined since the
//!    release.
//!
//! Independent of resolution, the release anchor is enforced at import time:
//! backfilled history must contain the anchor block, so a fabricated target
//! from compromised endpoints is detected minutes after joining.

use codec::Decode;
use quantus_runtime::opaque::Block;
use sc_service::ChainSpec;
use serde::Deserialize;
use sp_core::H256;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};
use std::time::Duration;

/// The node's header type (Poseidon block hash, BlakeTwo256 state trie).
pub type Header = <Block as BlockT>::Header;

const CHECKPOINT_HEADER_PROPERTY: &str = "checkpointHeader";
const CHECKPOINT_URLS_PROPERTY: &str = "checkpointUrls";
const RPC_TIMEOUT: Duration = Duration::from_secs(10);

fn parse_h256(value: &str) -> Result<H256, String> {
	let bytes = hex::decode(value.trim_start_matches("0x"))
		.map_err(|e| format!("invalid hex in hash {value:?}: {e}"))?;
	if bytes.len() != 32 {
		return Err(format!("hash {value:?} is {} bytes, expected 32", bytes.len()));
	}
	Ok(H256::from_slice(&bytes))
}

/// Decode a SCALE-encoded header from 0x-prefixed hex.
pub fn decode_header_hex(value: &str) -> Result<Header, String> {
	let bytes = hex::decode(value.trim_start_matches("0x"))
		.map_err(|e| format!("checkpoint header is not valid hex: {e}"))?;
	Header::decode(&mut &bytes[..])
		.map_err(|e| format!("checkpoint header does not SCALE-decode: {e}"))
}

/// The release anchor pinned in the chain spec, if any.
pub fn anchor_from_spec(spec: &dyn ChainSpec) -> Result<Option<Header>, String> {
	match spec.properties().get(CHECKPOINT_HEADER_PROPERTY) {
		Some(value) => {
			let hex = value.as_str().ok_or_else(|| {
				format!("chain spec property {CHECKPOINT_HEADER_PROPERTY} must be a hex string")
			})?;
			decode_header_hex(hex).map(Some)
		},
		None => Ok(None),
	}
}

/// Checkpoint RPC endpoints listed in the chain spec, if any.
pub fn urls_from_spec(spec: &dyn ChainSpec) -> Vec<String> {
	spec.properties()
		.get(CHECKPOINT_URLS_PROPERTY)
		.and_then(|v| v.as_array().cloned())
		.map(|urls| urls.iter().filter_map(|u| u.as_str().map(str::to_owned)).collect())
		.unwrap_or_default()
}

/// A fetched target must extend (or be) the release anchor's chain: it can
/// never be older than the anchor, and at the anchor's own height it must be
/// the anchor.
fn validate_against_anchor(target: &Header, anchor: Option<&Header>) -> Result<(), String> {
	let Some(anchor) = anchor else { return Ok(()) };
	if target.number() < anchor.number() {
		return Err(format!(
			"fetched checkpoint #{} is older than the release anchor #{}; \
			 endpoints are stale or on a different chain",
			target.number(),
			anchor.number()
		));
	}
	if target.number() == anchor.number() && target.hash() != anchor.hash() {
		return Err(format!(
			"fetched checkpoint {:?} contradicts the release anchor {:?} at height {}",
			target.hash(),
			anchor.hash(),
			anchor.number()
		));
	}
	Ok(())
}

#[derive(Deserialize)]
struct RpcEnvelope<T> {
	result: Option<T>,
	error: Option<serde_json::Value>,
}

async fn rpc_call<T: serde::de::DeserializeOwned>(
	client: &reqwest::Client,
	url: &str,
	method: &str,
	params: serde_json::Value,
) -> Result<T, String> {
	let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
	let response = client
		.post(url)
		.json(&body)
		.send()
		.await
		.map_err(|e| format!("{method} request to {url} failed: {e}"))?;
	let envelope: RpcEnvelope<T> = response
		.json()
		.await
		.map_err(|e| format!("{method} response from {url} malformed: {e}"))?;
	if let Some(error) = envelope.error {
		return Err(format!("{method} on {url} returned error: {error}"));
	}
	envelope.result.ok_or_else(|| format!("{method} on {url} returned no result"))
}

/// Fetch an endpoint's finalized head and verify the returned header actually
/// hashes to it, so an endpoint cannot pair a plausible hash with a body of
/// its choosing.
async fn fetch_finalized_header(client: &reqwest::Client, url: &str) -> Result<Header, String> {
	let finalized: String =
		rpc_call(client, url, "chain_getFinalizedHead", serde_json::json!([])).await?;
	let header: Header =
		rpc_call(client, url, "chain_getHeader", serde_json::json!([finalized])).await?;
	let expected = parse_h256(&finalized)?;
	if header.hash() != expected {
		return Err(format!(
			"{url} returned a header hashing to {:?} for finalized head {expected:?}",
			header.hash()
		));
	}
	Ok(header)
}

/// Fetch the network's finalized head from every endpoint. The candidate is
/// the lowest finalized height among responders (every other responder has
/// finalized at least that height, so all honest endpoints agree on its hash),
/// and every other responder must confirm the candidate's hash at that height.
async fn fetch_agreed_target(urls: &[String]) -> Result<Header, String> {
	let client = reqwest::Client::builder()
		.timeout(RPC_TIMEOUT)
		.build()
		.map_err(|e| format!("failed to build checkpoint HTTP client: {e}"))?;

	let mut fetched: Vec<(&str, Header)> = Vec::new();
	for url in urls {
		match fetch_finalized_header(&client, url).await {
			Ok(header) => fetched.push((url, header)),
			Err(e) => log::warn!("Checkpoint endpoint unavailable: {e}"),
		}
	}
	let Some((candidate_url, candidate)) = fetched.iter().min_by_key(|(_, h)| *h.number()).cloned()
	else {
		return Err(format!("no checkpoint endpoint reachable (tried {})", urls.join(", ")));
	};

	let candidate_hash = candidate.hash();
	for (url, _) in fetched.iter().filter(|(url, _)| *url != candidate_url) {
		let confirmed: Option<String> =
			rpc_call(&client, url, "chain_getBlockHash", serde_json::json!([candidate.number()]))
				.await?;
		let confirmed = confirmed
			.ok_or_else(|| format!("{url} has no block at height {}", candidate.number()))
			.and_then(|h| parse_h256(&h))?;
		if confirmed != candidate_hash {
			return Err(format!(
				"checkpoint endpoints disagree at height {}: {candidate_url} has {candidate_hash:?}, \
				 {url} has {confirmed:?}; refusing to pick a side",
				candidate.number()
			));
		}
	}

	if fetched.len() < urls.len() {
		log::warn!(
			"Checkpoint agreed by {}/{} endpoints (the rest were unreachable)",
			fetched.len(),
			urls.len()
		);
	}
	Ok(candidate)
}

/// Resolve the warp sync target via the ladder described in the module docs.
pub async fn resolve_warp_target(
	cli_header_hex: Option<&str>,
	cli_urls: &[String],
	spec: &dyn ChainSpec,
) -> Result<Header, String> {
	if let Some(hex) = cli_header_hex {
		let header = decode_header_hex(hex)?;
		log::info!(
			"🎯 Warp sync target pinned via --checkpoint-header: #{} ({:?})",
			header.number(),
			header.hash()
		);
		return Ok(header);
	}

	let anchor = anchor_from_spec(spec)?;
	let urls = if cli_urls.is_empty() { urls_from_spec(spec) } else { cli_urls.to_vec() };

	if !urls.is_empty() {
		match fetch_agreed_target(&urls).await {
			Ok(target) => match validate_against_anchor(&target, anchor.as_ref()) {
				Ok(()) => {
					log::info!(
						"🎯 Warp sync target fetched from checkpoint endpoints: #{} ({:?})",
						target.number(),
						target.hash()
					);
					return Ok(target);
				},
				Err(e) => log::warn!("Fetched checkpoint rejected: {e}"),
			},
			Err(e) => log::warn!("Checkpoint fetch failed: {e}"),
		}
		if anchor.is_some() {
			log::warn!("Falling back to the release anchor checkpoint");
		}
	}

	if let Some(anchor) = anchor {
		log::warn!(
			"🎯 Warp sync target is the release anchor #{} ({:?}); blocks mined since the \
			 release will be re-executed during catch-up",
			anchor.number(),
			anchor.hash()
		);
		return Ok(anchor);
	}

	Err(format!(
		"warp sync needs a checkpoint: pass --checkpoint-header or --checkpoint-url, or use a \
		 chain spec with {CHECKPOINT_HEADER_PROPERTY}/{CHECKPOINT_URLS_PROPERTY} properties"
	))
}

#[cfg(test)]
mod tests {
	use super::*;
	use codec::Encode;
	use sp_core::Hasher;
	use sp_runtime::traits::BlakeTwo256;

	fn header(number: u32) -> Header {
		<Header as HeaderT>::new(
			number,
			BlakeTwo256::hash(b"extrinsics"),
			BlakeTwo256::hash(b"state"),
			H256::repeat_byte(number as u8),
			Default::default(),
		)
	}

	#[test]
	fn header_hex_round_trips() {
		let original = header(856_000);
		let hex = format!("0x{}", hex::encode(original.encode()));
		let decoded = decode_header_hex(&hex).expect("round trip");
		assert_eq!(decoded, original);
		assert_eq!(decoded.hash(), original.hash());
	}

	#[test]
	fn malformed_header_hex_is_rejected() {
		assert!(decode_header_hex("0xzz").is_err());
		assert!(decode_header_hex("0x0102").is_err());
	}

	#[test]
	fn target_must_extend_the_anchor() {
		let anchor = header(100);
		assert!(validate_against_anchor(&header(101), Some(&anchor)).is_ok());
		assert!(validate_against_anchor(&anchor.clone(), Some(&anchor)).is_ok());
		assert!(validate_against_anchor(&header(99), Some(&anchor)).is_err());
		// Same height, different content: contradicts the anchor.
		let mut impostor = header(100);
		impostor.set_parent_hash(H256::repeat_byte(0xEE));
		assert!(validate_against_anchor(&impostor, Some(&anchor)).is_err());
		assert!(validate_against_anchor(&header(1), None).is_ok());
	}

	#[test]
	fn spec_without_checkpoint_properties_yields_nothing() {
		let spec = crate::chain_spec::development_chain_spec().expect("dev spec");
		assert!(anchor_from_spec(&spec).expect("no anchor is fine").is_none());
		assert!(urls_from_spec(&spec).is_empty());
	}
}
