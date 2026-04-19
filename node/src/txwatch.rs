//! RPC subscription for watching incoming transfers to an address via the tx pool.
//!
//! A single background decoder task processes each pool import once, then fans
//! out to all active listeners via a broadcast channel.  This keeps the cost
//! at O(M) decodes regardless of how many listeners exist.
//!
//! **Integrator note:** notifications reflect mempool/ready-queue visibility, not
//! block inclusion or finality — transactions can still be dropped or replaced.
//! A `from` field of `""` means the sender used a non-`Id` MultiAddress variant
//! or the extrinsic was unsigned.

use std::sync::{
	atomic::{AtomicUsize, Ordering},
	Arc,
};

use codec::{Decode, Encode};
use futures::StreamExt;
use jsonrpsee::{
	core::{async_trait, SubscriptionResult},
	proc_macros::rpc,
	tokio::sync::broadcast,
	PendingSubscriptionSink, SubscriptionMessage,
};
use quantus_runtime::{opaque::Block, AccountId, Balance, RuntimeCall, UncheckedExtrinsic};
use sc_transaction_pool_api::{InPoolTransaction, TransactionPool};
use serde::{Deserialize, Serialize};
use sp_core::crypto::Ss58Codec;
use sp_runtime::{generic::Preamble, traits::LazyExtrinsic, MultiAddress};

const LOG_TARGET: &str = "txwatch";
const MAX_LISTENERS: usize = 32;
const MAX_BATCH_DEPTH: usize = 4;
const BROADCAST_CAPACITY: usize = 256;

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

#[derive(Debug)]
struct DecodedPoolTx {
	tx_hash: String,
	from: Option<AccountId>,
	transfers: Vec<(AccountId, Balance, Option<u32>)>,
}

struct ListenerGuard(Arc<AtomicUsize>);

impl Drop for ListenerGuard {
	fn drop(&mut self) {
		self.0.fetch_sub(1, Ordering::Relaxed);
	}
}

pub struct TxWatch {
	broadcast: broadcast::Sender<Arc<DecodedPoolTx>>,
	active_listeners: Arc<AtomicUsize>,
}

impl TxWatch {
	pub fn new<P>(pool: Arc<P>) -> Self
	where
		P: TransactionPool<Block = Block> + 'static,
	{
		let (broadcast, _) = broadcast::channel(BROADCAST_CAPACITY);
		let fanout = broadcast.clone();

		jsonrpsee::tokio::spawn(async move {
			let stream = pool.import_notification_stream();
			futures::pin_mut!(stream);

			while let Some(tx_hash) = stream.next().await {
				if fanout.receiver_count() == 0 {
					continue;
				}

				let encoded = if let Some(in_pool) = pool.ready_transaction(&tx_hash) {
					Encode::encode(in_pool.data())
				} else {
					let found = pool
						.ready()
						.find(|in_pool| *in_pool.hash() == tx_hash)
						.map(|in_pool| Encode::encode(in_pool.data()));
					let Some(data) = found else {
						log::trace!(target: LOG_TARGET, "Tx {:?} not found in ready queue (future or already finalized)", tx_hash);
						continue;
					};
					data
				};

				let Ok(inner_bytes) = Vec::<u8>::decode(&mut &encoded[..]) else {
					continue;
				};
				let Ok(uxt) = UncheckedExtrinsic::decode_unprefixed(&inner_bytes) else {
					continue;
				};

				let from_account = match &uxt.preamble {
					Preamble::Signed(addr, _, _) => match addr {
						MultiAddress::Id(id) => Some(id.clone()),
						other => {
							log::debug!(target: LOG_TARGET, "Unsupported MultiAddress variant: {:?}", other);
							None
						},
					},
					_ => None,
				};

				let transfers = extract_all_transfers(&uxt.function, 0);
				if transfers.is_empty() {
					continue;
				}

				let _ = fanout.send(Arc::new(DecodedPoolTx {
					tx_hash: format!("{:?}", tx_hash),
					from: from_account,
					transfers,
				}));
			}
		});

		Self { broadcast, active_listeners: Arc::new(AtomicUsize::new(0)) }
	}
}

#[async_trait]
impl TxWatchApiServer for TxWatch {
	async fn watch_address(
		&self,
		pending: PendingSubscriptionSink,
		address: String,
	) -> SubscriptionResult {
		let prev = self.active_listeners.fetch_add(1, Ordering::Relaxed);
		if prev >= MAX_LISTENERS {
			self.active_listeners.fetch_sub(1, Ordering::Relaxed);
			pending
				.reject(jsonrpsee::types::error::ErrorObject::owned(
					5010,
					format!("Too many listeners (max {MAX_LISTENERS})"),
					None::<()>,
				))
				.await;
			return Ok(());
		}

		let guard = ListenerGuard(self.active_listeners.clone());

		let target = match AccountId::from_ss58check(&address) {
			Ok(a) => a,
			Err(_) => {
				pending
					.reject(jsonrpsee::types::error::ErrorObject::owned(
						5011,
						"Invalid SS58 address",
						None::<()>,
					))
					.await;
				return Ok(());
			},
		};

		log::info!(target: LOG_TARGET, "Watching address {}", &address[..12.min(address.len())]);
		let sink = pending.accept().await?;
		let mut listener_rx = self.broadcast.subscribe();

		jsonrpsee::tokio::spawn(async move {
			let _guard = guard;
			loop {
				match listener_rx.recv().await {
					Ok(decoded) => {
						if sink.is_closed() {
							break;
						}
						for (dest, amount, asset_id) in &decoded.transfers {
							if dest != &target {
								continue;
							}
							let notification = TransferNotification {
								tx_hash: decoded.tx_hash.clone(),
								from: decoded
									.from
									.as_ref()
									.map(|a| a.to_ss58check())
									.unwrap_or_default(),
								amount: amount.to_string(),
								asset_id: *asset_id,
							};
							log::info!(
								target: LOG_TARGET,
								"Transfer detected: {} -> watched addr, amount={}, asset={:?}",
								&notification.from[..12.min(notification.from.len())],
								notification.amount,
								notification.asset_id
							);
							let Ok(msg) = SubscriptionMessage::from_json(&notification) else {
								continue;
							};
							if sink.send(msg).await.is_err() {
								return;
							}
						}
					},
					Err(broadcast::error::RecvError::Lagged(n)) => {
						log::warn!(
							target: LOG_TARGET,
							"Listener lagged, skipped {n} transactions"
						);
					},
					Err(broadcast::error::RecvError::Closed) => break,
				}
			}
		});

		Ok(())
	}
}

fn extract_all_transfers(
	call: &RuntimeCall,
	depth: usize,
) -> Vec<(AccountId, Balance, Option<u32>)> {
	if depth > MAX_BATCH_DEPTH {
		return Vec::new();
	}
	let mut results = Vec::new();
	match call {
		RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest, value }) |
		RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { dest, value }) =>
			if let MultiAddress::Id(id) = dest {
				results.push((id.clone(), *value, None));
			},
		RuntimeCall::Assets(pallet_assets::Call::transfer { id, target: dest, amount }) |
		RuntimeCall::Assets(pallet_assets::Call::transfer_keep_alive {
			id,
			target: dest,
			amount,
		}) =>
			if let MultiAddress::Id(d) = dest {
				results.push((d.clone(), *amount, Some(id.0)));
			},
		RuntimeCall::Utility(pallet_utility::Call::batch { calls }) |
		RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }) |
		RuntimeCall::Utility(pallet_utility::Call::force_batch { calls }) =>
			for inner in calls {
				results.extend(extract_all_transfers(inner, depth + 1));
			},
		_ => {},
	}
	results
}

#[cfg(test)]
pub(crate) fn extract_transfers_to(
	call: &RuntimeCall,
	target: &AccountId,
) -> Vec<(Balance, Option<u32>)> {
	extract_all_transfers(call, 0)
		.into_iter()
		.filter(|(dest, _, _)| dest == target)
		.map(|(_, amount, asset_id)| (amount, asset_id))
		.collect()
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

	#[test]
	fn batch_depth_is_capped() {
		fn nest(depth: usize, inner: RuntimeCall) -> RuntimeCall {
			if depth == 0 {
				return inner;
			}
			batch(vec![nest(depth - 1, inner)])
		}
		let deep = nest(MAX_BATCH_DEPTH, native_transfer(&merchant(), UNIT));
		assert_eq!(extract_transfers_to(&deep, &merchant()), vec![(UNIT, None)]);

		let too_deep = nest(MAX_BATCH_DEPTH + 1, native_transfer(&merchant(), UNIT));
		assert!(extract_transfers_to(&too_deep, &merchant()).is_empty());
	}

	#[test]
	fn extract_all_transfers_returns_all_destinations() {
		let call = batch(vec![
			native_transfer(&merchant(), 10 * UNIT),
			native_transfer(&other(), 20 * UNIT),
			asset_transfer(5, &customer(), 300),
		]);
		let all = extract_all_transfers(&call, 0);
		assert_eq!(all.len(), 3);
		assert_eq!(all[0], (merchant(), 10 * UNIT, None));
		assert_eq!(all[1], (other(), 20 * UNIT, None));
		assert_eq!(all[2], (customer(), 300, Some(5)));
	}
}
