use super::types::{
	DilithiumSignatureScheme, DilithiumSigner, WrappedPublicBytes, WrappedSignatureBytes,
};

use alloc::vec::Vec;
use qp_poseidon_core::hash_bytes;
use sp_core::{
	crypto::{Derive, Public, PublicBytes, Signature, SignatureBytes},
	ByteArray, H256,
};
use sp_runtime::{
	traits::{IdentifyAccount, Verify},
	AccountId32,
};

//
// Trait implementations for WrappedPublicBytes
//

impl<const N: usize, SubTag> Derive for WrappedPublicBytes<N, SubTag> {}
impl<const N: usize, SubTag> AsMut<[u8]> for WrappedPublicBytes<N, SubTag> {
	fn as_mut(&mut self) -> &mut [u8] {
		self.0.as_mut()
	}
}
impl<const N: usize, SubTag> AsRef<[u8]> for WrappedPublicBytes<N, SubTag> {
	fn as_ref(&self) -> &[u8] {
		self.0.as_slice()
	}
}
impl<const N: usize, SubTag> TryFrom<&[u8]> for WrappedPublicBytes<N, SubTag> {
	type Error = ();
	fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
		PublicBytes::from_slice(data)
			.map(|bytes| WrappedPublicBytes(bytes))
			.map_err(|_| ())
	}
}
impl<const N: usize, SubTag> ByteArray for WrappedPublicBytes<N, SubTag> {
	fn as_slice(&self) -> &[u8] {
		self.0.as_slice()
	}
	const LEN: usize = N;
	fn from_slice(data: &[u8]) -> Result<Self, ()> {
		PublicBytes::from_slice(data)
			.map(|bytes| WrappedPublicBytes(bytes))
			.map_err(|_| ())
	}
	fn to_raw_vec(&self) -> Vec<u8> {
		self.0.as_slice().to_vec()
	}
}
impl<const N: usize, SubTag: Clone + Eq> Public for WrappedPublicBytes<N, SubTag> where
	Self: sp_runtime::CryptoType
{
}

impl<const N: usize, SubTag> Default for WrappedPublicBytes<N, SubTag> {
	fn default() -> Self {
		WrappedPublicBytes(PublicBytes::default())
	}
}
impl<const N: usize, SubTag> alloc::fmt::Debug for WrappedPublicBytes<N, SubTag> {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
		use sp_core::bytes::to_hex;

		write!(f, "{}", to_hex(self.0.as_ref(), false))
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, _: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
		Ok(())
	}
}

pub struct WormholeAddress(pub H256);

// AccountID32 for a wormhole address is the same as the address itself
impl IdentifyAccount for WormholeAddress {
	type AccountId = AccountId32;
	fn into_account(self) -> Self::AccountId {
		AccountId32::new(self.0.as_bytes().try_into().unwrap())
	}
}

//
// Trait implementations for WrappedSignatureBytes
//
impl<const N: usize, SubTag> Derive for WrappedSignatureBytes<N, SubTag> {}
impl<const N: usize, SubTag> AsMut<[u8]> for WrappedSignatureBytes<N, SubTag> {
	fn as_mut(&mut self) -> &mut [u8] {
		self.0.as_mut()
	}
}
impl<const N: usize, SubTag> AsRef<[u8]> for WrappedSignatureBytes<N, SubTag> {
	fn as_ref(&self) -> &[u8] {
		self.0.as_slice()
	}
}
impl<const N: usize, SubTag> TryFrom<&[u8]> for WrappedSignatureBytes<N, SubTag> {
	type Error = ();
	fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
		SignatureBytes::from_slice(data)
			.map(|bytes| WrappedSignatureBytes(bytes))
			.map_err(|_| ())
	}
}
impl<const N: usize, SubTag> ByteArray for WrappedSignatureBytes<N, SubTag> {
	fn as_slice(&self) -> &[u8] {
		self.0.as_slice()
	}
	const LEN: usize = N;
	fn from_slice(data: &[u8]) -> Result<Self, ()> {
		SignatureBytes::from_slice(data)
			.map(|bytes| WrappedSignatureBytes(bytes))
			.map_err(|_| ())
	}
	fn to_raw_vec(&self) -> Vec<u8> {
		self.0.as_slice().to_vec()
	}
}
impl<const N: usize, SubTag: Clone + Eq> Signature for WrappedSignatureBytes<N, SubTag> where
	Self: sp_runtime::CryptoType
{
}

impl<const N: usize, SubTag> Default for WrappedSignatureBytes<N, SubTag> {
	fn default() -> Self {
		WrappedSignatureBytes(SignatureBytes::default())
	}
}

impl<const N: usize, SubTag> alloc::fmt::Debug for WrappedSignatureBytes<N, SubTag> {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		use sp_core::bytes::to_hex;

		write!(f, "{}", to_hex(self.0.as_slice(), false))
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, _: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
		Ok(())
	}
}

//
// Trait implementations for DilithiumSignatureScheme
//

impl Verify for DilithiumSignatureScheme {
	type Signer = DilithiumSigner;

	fn verify<L: sp_runtime::traits::Lazy<[u8]>>(
		&self,
		mut msg: L,
		signer: &<Self::Signer as IdentifyAccount>::AccountId,
	) -> bool {
		match self {
			Self::Dilithium87(sig_public) => {
				let account = sig_public.public().clone().into_account();
				if account != *signer {
					return false;
				}
				crate::verify_ml_dsa_87(
					sig_public.public().as_ref(),
					msg.get(),
					sig_public.signature().as_ref(),
				)
			},
			Self::Dilithium65(sig_public) => {
				let account = sig_public.public().clone().into_account();
				if account != *signer {
					return false;
				}
				crate::verify_ml_dsa_65(
					sig_public.public().as_ref(),
					msg.get(),
					sig_public.signature().as_ref(),
				)
			},
			// SigOnly variants carry no public key and AccountId32 is a hash of
			// the key, so a pure verify has nothing to check against. Resolving
			// the key from on-chain storage is done by `pallet_pubkey::CachedSignature`.
			Self::Dilithium87SigOnly(_) | Self::Dilithium65SigOnly(_) => false,
		}
	}
}

//
// Trait implementations for DilithiumSigner
//

impl IdentifyAccount for DilithiumSigner {
	type AccountId = AccountId32;

	fn into_account(self) -> AccountId32 {
		// Use injective encoding for account ID derivation (collision-resistant for security)
		match self {
			Self::Dilithium87(who) => hash_bytes(who.as_ref()).into(),
			Self::Dilithium65(who) => hash_bytes(who.as_ref()).into(),
		}
	}
}
