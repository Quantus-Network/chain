use super::types::{
	DilithiumPair, DilithiumPublic, DilithiumSignatureScheme, DilithiumSigner, Error,
	WrappedPublicBytes, WrappedSignatureBytes,
};

use crate::{DilithiumSignature, DilithiumSignatureWithPublic};
use alloc::vec::Vec;
use qp_poseidon::PoseidonHasher;
use sp_core::{
	crypto::{Derive, Public, PublicBytes, Signature, SignatureBytes},
	ByteArray, Hasher, H256,
};
use sp_runtime::{
	traits::{IdentifyAccount, Verify},
	AccountId32, CryptoType,
};

/// Verifies a Dilithium ML-DSA-87 signature
///
/// This function performs signature verification using the Dilithium post-quantum
/// cryptographic signature scheme (ML-DSA-87). It validates that the given signature
/// was created by the holder of the private key corresponding to the public key.
///
/// # Arguments
/// * `pub_key` - The public key bytes (must be valid Dilithium public key)
/// * `msg` - The message that was signed
/// * `sig` - The signature bytes to verify
///
/// # Returns
/// `true` if the signature is valid and verification succeeds, `false` otherwise
///
/// # Examples
/// ```ignore
/// use qp_dilithium_crypto::verify;
///
/// let valid = verify(&public_key_bytes, &message, &signature_bytes);
/// if valid {
///     println!("Signature is valid!");
/// }
/// ```
pub fn verify(pub_key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
	use qp_rusty_crystals_dilithium::ml_dsa_87::PublicKey;
	match PublicKey::from_bytes(pub_key) {
		Ok(pk) => pk.verify(msg, sig, None),
		Err(e) => {
			log::warn!("public key failed to deserialize {:?}", e);
			false
		},
	}
}

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
impl<const N: usize, SubTag> CryptoType for WrappedPublicBytes<N, SubTag> {
	type Pair = DilithiumPair;
}
impl<const N: usize, SubTag: Clone + Eq> Public for WrappedPublicBytes<N, SubTag> {}

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

impl IdentifyAccount for DilithiumPublic {
	type AccountId = AccountId32;
	fn into_account(self) -> Self::AccountId {
		AccountId32::new(PoseidonHasher::hash(self.0.as_slice()).0)
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
impl<const N: usize, SubTag> CryptoType for WrappedSignatureBytes<N, SubTag> {
	type Pair = DilithiumPair;
}
impl<const N: usize, SubTag: Clone + Eq> Signature for WrappedSignatureBytes<N, SubTag> {}

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
// Trait implementations for DilithiumPair
//

impl CryptoType for DilithiumPair {
	type Pair = Self;
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
		let Self::Dilithium(sig_public) = self;
		let account = sig_public.public().clone().into_account();
		if account != *signer {
			return false;
		}
		let result =
			verify(sig_public.public().as_ref(), msg.get(), sig_public.signature().as_ref());
		result
	}
}

//
// Trait implementations for DilithiumSigner
//
impl From<DilithiumPublic> for DilithiumSigner {
	fn from(x: DilithiumPublic) -> Self {
		Self::Dilithium(x)
	}
}

impl IdentifyAccount for DilithiumSigner {
	type AccountId = AccountId32;

	fn into_account(self) -> AccountId32 {
		let Self::Dilithium(who) = self;
		PoseidonHasher::hash(who.as_ref()).0.into()
	}
}

impl From<DilithiumPublic> for AccountId32 {
	fn from(public: DilithiumPublic) -> Self {
		public.into_account()
	}
}

//
// Implementation methods for DilithiumPair
//

impl DilithiumPair {
	pub fn from_seed(seed: &[u8]) -> Result<Self, Error> {
		let keypair = crate::pair::generate(Some(seed))?;
		Ok(DilithiumPair { secret: keypair.secret.to_bytes(), public: keypair.public.to_bytes() })
	}
	pub fn public(&self) -> DilithiumPublic {
		DilithiumPublic::from_slice(&self.public).expect("Valid public key bytes")
	}
}

impl alloc::fmt::Debug for DilithiumSignatureWithPublic {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
		write!(
			f,
			"ResonanceSignatureWithPublic {{ signature: {:?}, public: {:?} }}",
			self.signature(),
			self.public()
		)
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, f: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
		write!(f, "ResonanceSignatureWithPublic")
	}
}

impl From<DilithiumSignatureWithPublic> for DilithiumSignatureScheme {
	fn from(x: DilithiumSignatureWithPublic) -> Self {
		Self::Dilithium(x)
	}
}

impl TryFrom<DilithiumSignatureScheme> for DilithiumSignatureWithPublic {
	type Error = ();
	fn try_from(m: DilithiumSignatureScheme) -> Result<Self, Self::Error> {
		let DilithiumSignatureScheme::Dilithium(sig_with_public) = m;
		Ok(sig_with_public)
	}
}

impl AsMut<[u8]> for DilithiumSignatureWithPublic {
	fn as_mut(&mut self) -> &mut [u8] {
		self.bytes.as_mut()
	}
}
impl TryFrom<&[u8]> for DilithiumSignatureWithPublic {
	type Error = ();
	fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
		if data.len() != Self::TOTAL_LEN {
			return Err(());
		}
		let (sig_bytes, pub_bytes) = data.split_at(<DilithiumSignature as ByteArray>::LEN);
		let signature = DilithiumSignature::from_slice(sig_bytes).map_err(|_| ())?;
		let public = DilithiumPublic::from_slice(pub_bytes).map_err(|_| ())?;
		Ok(Self::new(signature, public))
	}
}

impl ByteArray for DilithiumSignatureWithPublic {
	const LEN: usize = Self::TOTAL_LEN;

	fn to_raw_vec(&self) -> Vec<u8> {
		self.to_bytes().to_vec()
	}

	fn from_slice(data: &[u8]) -> Result<Self, ()> {
		if data.len() != Self::LEN {
			return Err(());
		}
		let bytes = <[u8; Self::LEN]>::try_from(data).map_err(|_| ())?;
		Self::from_bytes(&bytes).map_err(|_| ())
	}

	fn as_slice(&self) -> &[u8] {
		self.bytes.as_slice()
	}
}
impl AsRef<[u8; Self::LEN]> for DilithiumSignatureWithPublic {
	fn as_ref(&self) -> &[u8; Self::LEN] {
		&self.bytes
	}
}

impl AsRef<[u8]> for DilithiumSignatureWithPublic {
	fn as_ref(&self) -> &[u8] {
		&self.bytes
	}
}
impl Signature for DilithiumSignatureWithPublic {}

impl CryptoType for DilithiumSignatureWithPublic {
	type Pair = DilithiumPair;
}
