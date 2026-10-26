use std::sync::Arc;

use codec::{Decode, Encode};
use jsonrpsee::tokio;
use plonky2::{field::types::PrimeField64, plonk::proof::ProofWithPublicInputs};
use qp_wormhole_inputs::{BytesDigest, PrivateBatchPublicInputs};
use qp_zk_circuits_common::circuit::{C, D, F};
use quantus_runtime::{opaque::Block, RuntimeCall, UncheckedExtrinsic};
use sc_transaction_pool::{Interception, SubmissionInterceptor};
use sc_transaction_pool_api::TransactionSource;
use sp_api::ProvideRuntimeApi;
use sp_runtime::{generic::Preamble, traits::Block as BlockT};
use sp_transaction_pool::runtime_api::TaggedTransactionQueue;

use super::{worker::nullifier_used, Aggregator, LOG_TARGET};
use crate::service::FullClient;

type Proof = ProofWithPublicInputs<F, C, D>;

/// Claims private-batch submissions for the aggregator pool (see the module
/// docs for the admission pipeline).
pub struct PrivateBatchInterceptor {
	client: Arc<FullClient>,
	aggregator: Arc<Aggregator>,
}

impl PrivateBatchInterceptor {
	pub fn new(client: Arc<FullClient>, aggregator: Arc<Aggregator>) -> Self {
		Self { client, aggregator }
	}

	fn decline(&self, reason: &str) -> Interception {
		log::debug!(target: LOG_TARGET, "private batch not claimed: {reason}");
		if let Some(m) = &self.aggregator.metrics {
			m.declined.inc();
		}
		Interception::Pass
	}

	/// Whether any of `nullifiers` is already settled in state at `at`.
	fn any_nullifier_used(
		&self,
		at: <Block as BlockT>::Hash,
		nullifiers: &[BytesDigest],
	) -> Result<bool, String> {
		for nullifier in nullifiers {
			if nullifier_used(&self.client, at, nullifier)? {
				return Ok(true);
			}
		}
		Ok(false)
	}
}

/// `Some(proof_bytes)` iff `xt` is a bare `Wormhole::verify_private_batch`.
fn private_batch_proof_bytes(xt: &<Block as BlockT>::Extrinsic) -> Option<Vec<u8>> {
	let uxt = UncheckedExtrinsic::decode(&mut &xt.encode()[..]).ok()?;
	if !matches!(uxt.preamble, Preamble::Bare(_)) {
		return None;
	}
	match uxt.function {
		RuntimeCall::Wormhole(pallet_wormhole::Call::verify_private_batch { proof_bytes }) =>
			Some(proof_bytes),
		_ => None,
	}
}

pub(super) fn parse_private_batch_inputs(
	proof: &Proof,
) -> Result<PrivateBatchPublicInputs, String> {
	let felts: Vec<u64> = proof.public_inputs.iter().map(|f| f.to_canonical_u64()).collect();
	PrivateBatchPublicInputs::try_from_u64_slice(&felts)
		.map_err(|e| format!("public inputs do not parse: {e}"))
}

#[async_trait::async_trait]
impl SubmissionInterceptor<Block> for PrivateBatchInterceptor {
	async fn intercept(
		&self,
		at: <Block as BlockT>::Hash,
		source: TransactionSource,
		xt: &<Block as BlockT>::Extrinsic,
	) -> Interception {
		// Block re-validation is not a submission.
		if source == TransactionSource::InBlock {
			return Interception::Pass;
		}
		let Some(proof_bytes) = private_batch_proof_bytes(xt) else {
			return Interception::Pass;
		};

		// 1. The runtime's own pool-admission checks.
		match self.client.runtime_api().validate_transaction(at, source, xt.clone(), at) {
			Ok(Ok(_)) => {},
			Ok(Err(e)) => return self.decline(&format!("runtime rejected it: {e:?}")),
			Err(e) => return self.decline(&format!("validate_transaction failed: {e}")),
		}

		// 2. Nullifiers must be unspent; the pool only dedupes against itself.
		let proof = match Proof::from_bytes(proof_bytes, &self.aggregator.private_batch_common) {
			Ok(proof) => proof,
			Err(e) => return self.decline(&format!("proof does not deserialize: {e}")),
		};
		let inputs = match parse_private_batch_inputs(&proof) {
			Ok(inputs) => inputs,
			Err(e) => return self.decline(&e),
		};
		match self.any_nullifier_used(at, &inputs.nullifiers) {
			Ok(false) => {},
			Ok(true) => return self.decline("a nullifier is already settled"),
			Err(e) => return self.decline(&e),
		}

		// 3. Verify and pool. Recursive verification is ~10-20ms, so keep it
		// off the async executor.
		let aggregator = self.aggregator.clone();
		let pushed = tokio::task::spawn_blocking(move || {
			let mut pool = aggregator.pool.lock();
			let result = pool.push(proof);
			aggregator.update_pool_gauges(&pool);
			result
		})
		.await;
		match pushed {
			Ok(Ok(_key)) => {
				log::debug!(
					target: LOG_TARGET,
					"claimed private batch for block #{} ({} nullifiers)",
					inputs.block_data.block_number,
					inputs.nullifiers.len()
				);
				if let Some(m) = &self.aggregator.metrics {
					m.claimed.inc();
				}
				Interception::Claimed
			},
			Ok(Err(e)) => self.decline(&format!("proof pool refused it: {e}")),
			Err(e) => self.decline(&format!("pool task failed: {e}")),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use sp_runtime::OpaqueExtrinsic;

	#[test]
	fn bare_private_batch_is_recognised() {
		let call = RuntimeCall::Wormhole(pallet_wormhole::Call::verify_private_batch {
			proof_bytes: vec![1, 2, 3],
		});
		let xt = OpaqueExtrinsic::from(UncheckedExtrinsic::new_bare(call));
		assert_eq!(private_batch_proof_bytes(&xt), Some(vec![1, 2, 3]));
	}

	#[test]
	fn public_batch_and_other_calls_pass_through() {
		let public = RuntimeCall::Wormhole(pallet_wormhole::Call::verify_public_batch {
			proof_bytes: vec![1, 2, 3],
		});
		assert_eq!(
			private_batch_proof_bytes(&OpaqueExtrinsic::from(UncheckedExtrinsic::new_bare(public))),
			None
		);

		let remark = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
		assert_eq!(
			private_batch_proof_bytes(&OpaqueExtrinsic::from(UncheckedExtrinsic::new_bare(remark))),
			None
		);
	}

	#[test]
	fn undecodable_bytes_pass_through() {
		let xt =
			OpaqueExtrinsic::try_from_encoded_extrinsic(&codec::Encode::encode(&vec![0xffu8; 4]))
				.expect("opaque");
		assert_eq!(private_batch_proof_bytes(&xt), None);
	}
}
