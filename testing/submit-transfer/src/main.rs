//! Submit a signed Dilithium transfer to a running dev node.
//! Usage: cargo run -p submit-transfer
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use codec::Encode;
use qp_dilithium_crypto::pair::{crystal_alice, dilithium_bob};
use quantus_runtime as runtime;
use runtime::UNIT;
use serde_json::{json, Value};
use sp_core::{crypto::Ss58Codec, Pair};
use sp_runtime::traits::IdentifyAccount;
use std::process::Command;

fn rpc(method: &str, params: Value) -> Value {
	let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
	let output = Command::new("curl")
		.args([
			"-s",
			"-X",
			"POST",
			"-H",
			"Content-Type: application/json",
			"-d",
			&body.to_string(),
			"http://127.0.0.1:9944",
		])
		.output()
		.expect("curl");
	serde_json::from_slice(&output.stdout).expect("parse JSON")
}

fn main() {
	let alice = crystal_alice();
	let bob = dilithium_bob();

	let alice_account: runtime::AccountId = alice.public().into_account();
	let bob_account: runtime::AccountId = bob.public().into_account();

	let ss58 = sp_core::crypto::Ss58AddressFormat::custom(189);
	println!("Alice: {}", alice_account.to_ss58check_with_version(ss58));
	println!("Bob:   {}", bob_account.to_ss58check_with_version(ss58));

	let genesis_resp = rpc("chain_getBlockHash", json!([0]));
	let genesis_hash_hex = genesis_resp["result"].as_str().expect("genesis hash");
	let genesis_hash =
		sp_core::H256::from_slice(&hex::decode(genesis_hash_hex.trim_start_matches("0x")).unwrap());

	let best_resp = rpc("chain_getBlockHash", json!([]));
	let best_hash_hex = best_resp["result"].as_str().expect("best hash");
	let best_hash =
		sp_core::H256::from_slice(&hex::decode(best_hash_hex.trim_start_matches("0x")).unwrap());

	let header_resp = rpc("chain_getHeader", json!([best_hash_hex]));
	let best_number_hex = header_resp["result"]["number"].as_str().expect("block number");
	let best_number = u32::from_str_radix(best_number_hex.trim_start_matches("0x"), 16).unwrap();

	let nonce_resp =
		rpc("system_accountNextIndex", json!([alice_account.to_ss58check_with_version(ss58)]));
	let nonce = nonce_resp["result"].as_u64().expect("nonce") as u32;

	println!("Genesis: {genesis_hash_hex}");
	println!("Best:    {best_hash_hex} (#{best_number})");
	println!("Nonce:   {nonce}");

	let call = runtime::RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
		dest: bob_account.into(),
		value: 5 * UNIT,
	});

	let period = 64u64;
	let tx_ext: runtime::TxExtension = (
		frame_system::CheckNonZeroSender::<runtime::Runtime>::new(),
		frame_system::CheckSpecVersion::<runtime::Runtime>::new(),
		frame_system::CheckTxVersion::<runtime::Runtime>::new(),
		frame_system::CheckGenesis::<runtime::Runtime>::new(),
		frame_system::CheckEra::<runtime::Runtime>::from(sp_runtime::generic::Era::mortal(
			period,
			best_number as u64,
		)),
		frame_system::CheckNonce::<runtime::Runtime>::from(nonce),
		frame_system::CheckWeight::<runtime::Runtime>::new(),
		pallet_transaction_payment::ChargeTransactionPayment::<runtime::Runtime>::from(0),
		frame_metadata_hash_extension::CheckMetadataHash::<runtime::Runtime>::new(false),
		runtime::transaction_extensions::ReversibleTransactionExtension::<runtime::Runtime>::new(),
		runtime::transaction_extensions::WormholeProofRecorderExtension::<runtime::Runtime>::new(),
	);

	let raw_payload = runtime::SignedPayload::from_raw(
		call.clone(),
		tx_ext.clone(),
		(
			(),
			runtime::VERSION.spec_version,
			runtime::VERSION.transaction_version,
			genesis_hash,
			best_hash,
			(),
			(),
			(),
			None,
			(),
			(),
		),
	);
	let signature = raw_payload.using_encoded(|e| alice.sign(e));

	let uxt = runtime::UncheckedExtrinsic::new_signed(
		call,
		alice_account.into(),
		runtime::Signature::Dilithium(signature),
		tx_ext,
	);

	let encoded = uxt.encode();
	let hex_ext = format!("0x{}", hex::encode(&encoded));
	println!("Extrinsic size: {} bytes", encoded.len());
	println!("Submitting...");

	let submit_resp = rpc("author_submitExtrinsic", json!([hex_ext]));
	if let Some(err) = submit_resp.get("error") {
		eprintln!("ERROR: {}", serde_json::to_string_pretty(err).unwrap());
	} else {
		let tx_hash = submit_resp["result"].as_str().unwrap_or("?");
		println!("Submitted! tx_hash: {tx_hash}");
	}
}
