//! RPC subscription for watching incoming transfers to an address via the tx pool.
//!
//! Designed for POS (Point of Sale) systems that need to detect when a payment
//! has been submitted. Transactions in the pool have already been validated
//! (signature checked), so acceptance into the pool confirms authenticity.

use std::sync::Arc;

use codec::{Decode, Encode};
use futures::StreamExt;
use jsonrpsee::{
	core::{async_trait, SubscriptionResult},
	proc_macros::rpc,
	PendingSubscriptionSink, SubscriptionMessage,
};
use quantus_runtime::{opaque::Block, AccountId, Balance, RuntimeCall, UncheckedExtrinsic};
use sc_transaction_pool_api::{InPoolTransaction, TransactionPool};
use serde::{Deserialize, Serialize};
use sp_core::crypto::Ss58Codec;
use sp_runtime::{generic::Preamble, MultiAddress};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferNotification {
	pub tx_hash: String,
	pub from: String,
	pub amount: String,
	pub asset_id: Option<u32>,
}

#[rpc(server)]
pub trait TxWatchApi {
	#[subscription(
		name = "txWatch_watchAddress" => "txWatch_transfer",
		unsubscribe = "txWatch_unwatchAddress",
		item = TransferNotification
	)]
	async fn watch_address(&self, address: String) -> SubscriptionResult;
}

pub struct TxWatch<P> {
	pool: Arc<P>,
}

impl<P> TxWatch<P> {
	pub fn new(pool: Arc<P>) -> Self {
		Self { pool }
	}
}

#[async_trait]
impl<P> TxWatchApiServer for TxWatch<P>
where
	P: TransactionPool<Block = Block> + 'static,
{
	async fn watch_address(
		&self,
		pending: PendingSubscriptionSink,
		address: String,
	) -> SubscriptionResult {
		let target = match AccountId::from_ss58check(&address) {
			Ok(a) => a,
			Err(_) => {
				pending
					.reject(jsonrpsee::types::error::ErrorObject::owned(
						5002,
						"Invalid SS58 address",
						None::<()>,
					))
					.await;
				return Ok(());
			},
		};

		let pool = self.pool.clone();
		let sink = pending.accept().await?;

		jsonrpsee::tokio::spawn(async move {
			let stream = pool.import_notification_stream();
			futures::pin_mut!(stream);

			while let Some(tx_hash) = stream.next().await {
				if sink.is_closed() {
					break;
				}

				let notifications = {
					let Some(in_pool_tx) = pool.ready_transaction(&tx_hash) else { continue };
					let encoded = Encode::encode(in_pool_tx.data());
					drop(in_pool_tx);

					let Ok(opaque_bytes) = Vec::<u8>::decode(&mut &encoded[..]) else {
						continue;
					};
					let Ok(uxt) = UncheckedExtrinsic::decode(&mut &opaque_bytes[..]) else {
						continue;
					};

					let sender = match &uxt.preamble {
						Preamble::Signed(addr, _, _) => match addr {
							MultiAddress::Id(id) => Some(id.to_ss58check()),
							_ => None,
						},
						_ => None,
					};

					let tx_hash_str = format!("{:?}", tx_hash);
					extract_transfers_to(&uxt.function, &target)
						.into_iter()
						.map(|(amount, asset_id)| TransferNotification {
							tx_hash: tx_hash_str.clone(),
							from: sender.clone().unwrap_or_default(),
							amount: amount.to_string(),
							asset_id,
						})
						.collect::<Vec<_>>()
				};

				for notification in notifications {
					let Ok(msg) = SubscriptionMessage::from_json(&notification) else {
						continue;
					};
					if sink.send(msg).await.is_err() {
						return;
					}
				}
			}
		});

		Ok(())
	}
}

pub(crate) fn extract_transfers_to(
	call: &RuntimeCall,
	target: &AccountId,
) -> Vec<(Balance, Option<u32>)> {
	let mut results = Vec::new();
	match call {
		RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest, value }) |
		RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { dest, value }) => {
			if matches!(dest, MultiAddress::Id(id) if id == target) {
				results.push((*value, None));
			}
		},
		RuntimeCall::Assets(pallet_assets::Call::transfer { id, target: dest, amount }) |
		RuntimeCall::Assets(pallet_assets::Call::transfer_keep_alive {
			id,
			target: dest,
			amount,
		}) => {
			if matches!(dest, MultiAddress::Id(d) if d == target) {
				results.push((*amount, Some(id.0)));
			}
		},
		RuntimeCall::Utility(pallet_utility::Call::batch { calls }) |
		RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }) |
		RuntimeCall::Utility(pallet_utility::Call::force_batch { calls }) => {
			for inner in calls {
				results.extend(extract_transfers_to(inner, target));
			}
		},
		_ => {},
	}
	results
}

#[cfg(test)]
mod tests {
	use super::*;
	use quantus_runtime::UNIT;
	use sp_runtime::AccountId32;

	fn merchant() -> AccountId {
		AccountId32::from([1; 32])
	}

	fn customer() -> AccountId {
		AccountId32::from([2; 32])
	}

	fn other() -> AccountId {
		AccountId32::from([3; 32])
	}

	fn addr(who: &AccountId) -> MultiAddress<AccountId, ()> {
		MultiAddress::Id(who.clone())
	}

	fn native_transfer(dest: &AccountId, value: Balance) -> RuntimeCall {
		RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
			dest: addr(dest),
			value,
		})
	}

	fn native_transfer_allow_death(dest: &AccountId, value: Balance) -> RuntimeCall {
		RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
			dest: addr(dest),
			value,
		})
	}

	fn asset_transfer(asset_id: u32, dest: &AccountId, amount: Balance) -> RuntimeCall {
		RuntimeCall::Assets(pallet_assets::Call::transfer {
			id: codec::Compact(asset_id),
			target: addr(dest),
			amount,
		})
	}

	fn asset_transfer_keep_alive(asset_id: u32, dest: &AccountId, amount: Balance) -> RuntimeCall {
		RuntimeCall::Assets(pallet_assets::Call::transfer_keep_alive {
			id: codec::Compact(asset_id),
			target: addr(dest),
			amount,
		})
	}

	fn batch(calls: Vec<RuntimeCall>) -> RuntimeCall {
		RuntimeCall::Utility(pallet_utility::Call::batch { calls })
	}

	fn batch_all(calls: Vec<RuntimeCall>) -> RuntimeCall {
		RuntimeCall::Utility(pallet_utility::Call::batch_all { calls })
	}

	fn force_batch(calls: Vec<RuntimeCall>) -> RuntimeCall {
		RuntimeCall::Utility(pallet_utility::Call::force_batch { calls })
	}

	#[test]
	fn detects_native_transfer_keep_alive() {
		let call = native_transfer(&merchant(), 100 * UNIT);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(100 * UNIT, None)]);
	}

	#[test]
	fn detects_native_transfer_allow_death() {
		let call = native_transfer_allow_death(&merchant(), 50 * UNIT);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(50 * UNIT, None)]);
	}

	#[test]
	fn ignores_transfer_to_different_address() {
		let call = native_transfer(&other(), 100 * UNIT);
		let result = extract_transfers_to(&call, &merchant());
		assert!(result.is_empty());
	}

	#[test]
	fn detects_asset_transfer() {
		let call = asset_transfer(42, &merchant(), 500);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(500, Some(42))]);
	}

	#[test]
	fn detects_asset_transfer_keep_alive() {
		let call = asset_transfer_keep_alive(7, &merchant(), 1000);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(1000, Some(7))]);
	}

	#[test]
	fn ignores_asset_transfer_to_different_address() {
		let call = asset_transfer(42, &other(), 500);
		let result = extract_transfers_to(&call, &merchant());
		assert!(result.is_empty());
	}

	#[test]
	fn detects_transfers_inside_batch() {
		let call = batch(vec![
			native_transfer(&merchant(), 10 * UNIT),
			native_transfer(&other(), 20 * UNIT),
			asset_transfer(5, &merchant(), 300),
		]);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(10 * UNIT, None), (300, Some(5))]);
	}

	#[test]
	fn detects_transfers_inside_batch_all() {
		let call = batch_all(vec![
			native_transfer(&merchant(), 10 * UNIT),
			asset_transfer(1, &merchant(), 200),
		]);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(10 * UNIT, None), (200, Some(1))]);
	}

	#[test]
	fn detects_transfers_inside_force_batch() {
		let call = force_batch(vec![native_transfer(&merchant(), 77 * UNIT)]);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(77 * UNIT, None)]);
	}

	#[test]
	fn detects_transfers_in_nested_batches() {
		let call = batch(vec![
			batch(vec![native_transfer(&merchant(), 10 * UNIT)]),
			native_transfer_allow_death(&merchant(), 20 * UNIT),
		]);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result, vec![(10 * UNIT, None), (20 * UNIT, None)]);
	}

	#[test]
	fn ignores_non_transfer_calls() {
		let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
		let result = extract_transfers_to(&call, &merchant());
		assert!(result.is_empty());
	}

	#[test]
	fn batch_with_no_matching_transfers_returns_empty() {
		let call = batch(vec![
			native_transfer(&other(), 100 * UNIT),
			RuntimeCall::System(frame_system::Call::remark { remark: vec![] }),
			asset_transfer(1, &customer(), 50),
		]);
		let result = extract_transfers_to(&call, &merchant());
		assert!(result.is_empty());
	}

	#[test]
	fn empty_batch_returns_empty() {
		let call = batch(vec![]);
		let result = extract_transfers_to(&call, &merchant());
		assert!(result.is_empty());
	}

	#[test]
	fn multiple_transfers_to_same_target_in_batch() {
		let call = batch(vec![
			native_transfer(&merchant(), 10 * UNIT),
			native_transfer(&merchant(), 20 * UNIT),
			native_transfer(&merchant(), 30 * UNIT),
		]);
		let result = extract_transfers_to(&call, &merchant());
		assert_eq!(result.len(), 3);
		let total: Balance = result.iter().map(|(a, _)| a).sum();
		assert_eq!(total, 60 * UNIT);
	}

	#[test]
	fn notification_serializes_correctly() {
		let n = TransferNotification {
			tx_hash: "0xabc".into(),
			from: "5GrwvaEF...".into(),
			amount: "1000000000000".into(),
			asset_id: None,
		};
		let json = serde_json::to_value(&n).unwrap();
		assert_eq!(json["tx_hash"], "0xabc");
		assert_eq!(json["amount"], "1000000000000");
		assert!(json["asset_id"].is_null());

		let n_asset = TransferNotification {
			tx_hash: "0xdef".into(),
			from: "5FHneW46...".into(),
			amount: "500".into(),
			asset_id: Some(42),
		};
		let json = serde_json::to_value(&n_asset).unwrap();
		assert_eq!(json["asset_id"], 42);
	}
}
