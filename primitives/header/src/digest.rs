use codec::Encode;
use sp_runtime::generic::{Digest, DigestItem};

use crate::DIGEST_LOGS_SIZE;

pub const POW_PREIMAGE_LEN: usize = 32;
pub const POW_SEAL_LEN: usize = 64;

/// The only sealed digest QPoW accepts: one 32-byte rewards preimage and one
/// 64-byte seal. Encodes on the wire as a Substrate [`Digest`] so existing
/// headers keep decoding.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QpowDigest {
	pub engine_id: [u8; 4],
	pub preimage: [u8; POW_PREIMAGE_LEN],
	pub seal: [u8; POW_SEAL_LEN],
	/// 0.8.x `set_code` leftover. Current WASM never sets this.
	pub historical_runtime_environment_updated: bool,
}

impl QpowDigest {
	pub fn new(
		engine_id: [u8; 4],
		preimage: [u8; POW_PREIMAGE_LEN],
		seal: [u8; POW_SEAL_LEN],
	) -> Self {
		Self { engine_id, preimage, seal, historical_runtime_environment_updated: false }
	}

	pub fn to_digest(&self) -> Digest {
		let mut logs = alloc::vec![DigestItem::PreRuntime(self.engine_id, self.preimage.to_vec())];
		if self.historical_runtime_environment_updated {
			logs.push(DigestItem::RuntimeEnvironmentUpdated);
		}
		logs.push(DigestItem::Seal(self.engine_id, self.seal.to_vec()));
		Digest { logs }
	}

	/// Sealed header only. Returns `Err(encoded_len)` if the digest is not
	/// exactly `[PreRuntime(32), Seal(64)]`, or that pair plus one grandfathered
	/// `RuntimeEnvironmentUpdated`.
	pub fn try_from_sealed(digest: &Digest) -> Result<Self, usize> {
		let encoded_len = digest.encode().len();
		let parsed = match digest.logs.as_slice() {
			[DigestItem::PreRuntime(id, pre), DigestItem::Seal(sid, seal)] =>
				parse_pair(*id, pre, *sid, seal, false),
			[DigestItem::PreRuntime(id, pre), DigestItem::RuntimeEnvironmentUpdated, DigestItem::Seal(sid, seal)] =>
				parse_pair(*id, pre, *sid, seal, true),
			_ => None,
		};
		let parsed = parsed.ok_or(encoded_len)?;
		let expected = if parsed.historical_runtime_environment_updated {
			DIGEST_LOGS_SIZE + 1
		} else {
			DIGEST_LOGS_SIZE
		};
		if encoded_len != expected {
			return Err(encoded_len);
		}
		Ok(parsed)
	}

	/// In-progress header after inherents, before the seal is appended.
	pub fn check_pre_seal(digest: &Digest) -> Result<([u8; 4], [u8; POW_PREIMAGE_LEN]), usize> {
		match digest.logs.as_slice() {
			[DigestItem::PreRuntime(id, pre)] if pre.len() == POW_PREIMAGE_LEN => {
				let mut preimage = [0u8; POW_PREIMAGE_LEN];
				preimage.copy_from_slice(pre);
				Ok((*id, preimage))
			},
			_ => Err(digest.encode().len()),
		}
	}
}

fn parse_pair(
	id: [u8; 4],
	pre: &[u8],
	sid: [u8; 4],
	seal: &[u8],
	historical_reu: bool,
) -> Option<QpowDigest> {
	if id != sid || pre.len() != POW_PREIMAGE_LEN || seal.len() != POW_SEAL_LEN {
		return None;
	}
	let mut preimage = [0u8; POW_PREIMAGE_LEN];
	preimage.copy_from_slice(pre);
	let mut seal_arr = [0u8; POW_SEAL_LEN];
	seal_arr.copy_from_slice(seal);
	Some(QpowDigest {
		engine_id: id,
		preimage,
		seal: seal_arr,
		historical_runtime_environment_updated: historical_reu,
	})
}

/// Import gate. Thin wrapper over [`QpowDigest::try_from_sealed`].
pub fn check_digest_commitment_window(digest: &Digest) -> Result<(), usize> {
	QpowDigest::try_from_sealed(digest).map(|_| ())
}
