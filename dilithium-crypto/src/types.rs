use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use rusty_crystals_dilithium::ml_dsa_87::{PUBLICKEYBYTES, SECRETKEYBYTES};
use scale_info::TypeInfo;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use sp_core::{
	crypto::{PublicBytes, SignatureBytes},
	ByteArray, RuntimeDebug,
};
#[cfg(feature = "std")]
use thiserror::Error;

///
/// Resonance Crypto Types
///
/// Currently implementing the Dilithum cryprographic scheme for post quantum security
///
/// It is modeled after the Substrate MultiSignature and Signature types such as sr25519.
///
/// For traits implemented see traits.rs

#[derive(
	Clone,
	Eq,
	PartialEq,
	Debug,
	Hash,
	Encode,
	Decode,
	TypeInfo,
	Ord,
	PartialOrd,
	DecodeWithMemTracking,
)]
pub struct DilithiumCryptoTag;

/// Dilithium cryptographic key pair
///
/// Contains both secret and public key material for Dilithium ML-DSA-87 operations
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DilithiumPair {
	pub secret: [u8; SECRETKEYBYTES],
	pub public: [u8; PUBLICKEYBYTES],
}

impl Default for DilithiumPair {
	fn default() -> Self {
		let seed = alloc::vec![0u8; 32];
		DilithiumPair::from_seed(&seed).expect("Failed to generate keypair")
	}
}

/// Wrapper around Substrate's PublicBytes to provide Dilithium-specific implementations
///
/// This wrapper enables the implementation of required traits for Dilithium public keys
/// while maintaining compatibility with Substrate's crypto infrastructure.
#[derive(
	Clone,
	Eq,
	PartialEq,
	Hash,
	Encode,
	Decode,
	TypeInfo,
	MaxEncodedLen,
	Ord,
	PartialOrd,
	DecodeWithMemTracking,
)]
pub struct WrappedPublicBytes<const N: usize, SubTag>(pub PublicBytes<N, SubTag>);

/// Wrapper around Substrate's SignatureBytes to provide Dilithium-specific implementations
///
/// This wrapper enables the implementation of required traits for Dilithium signatures
/// while maintaining compatibility with Substrate's crypto infrastructure.
#[derive(
	Clone,
	Eq,
	PartialEq,
	Hash,
	Encode,
	Decode,
	TypeInfo,
	MaxEncodedLen,
	Ord,
	PartialOrd,
	DecodeWithMemTracking,
)]
pub struct WrappedSignatureBytes<const N: usize, SubTag>(pub SignatureBytes<N, SubTag>);

pub type DilithiumPublic = WrappedPublicBytes<{ crate::PUB_KEY_BYTES }, DilithiumCryptoTag>;
pub type DilithiumSignature = WrappedSignatureBytes<{ crate::SIGNATURE_BYTES }, DilithiumCryptoTag>;

/// Dilithium signature scheme - drop-in replacement for MultiSignature
///
/// Currently supports only Dilithium, but structured as an enum to allow
/// for future signature schemes to be added easily.
#[derive(
	Eq,
	PartialEq,
	Clone,
	Encode,
	Decode,
	MaxEncodedLen,
	RuntimeDebug,
	TypeInfo,
	DecodeWithMemTracking,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DilithiumSignatureScheme {
	Dilithium(DilithiumSignatureWithPublic),
}

/// Dilithium signer - replacement for MultiSigner
///
/// Identifies the signer of a transaction using Dilithium public key
#[derive(
	Eq,
	PartialEq,
	Ord,
	PartialOrd,
	Clone,
	Encode,
	Decode,
	RuntimeDebug,
	TypeInfo,
	DecodeWithMemTracking,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DilithiumSigner {
	Dilithium(DilithiumPublic),
}

#[derive(Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum Error {
	#[cfg_attr(feature = "std", error("Failed to generate keypair"))]
	KeyGenerationFailed,
	#[cfg_attr(feature = "std", error("Invalid length"))]
	InvalidLength,
	#[cfg_attr(
		feature = "std",
		error("Entropy must be at least {required} bytes long, got {actual}")
	)]
	InsufficientEntropy { required: usize, actual: usize },
	#[cfg_attr(feature = "std", error("Failed to parse secret key"))]
	InvalidSecretKey,
	#[cfg_attr(feature = "std", error("Failed to parse public key"))]
	InvalidPublicKey,
}

/// Combined signature and public key structure for Dilithium
///
/// This structure contains both the signature and the public key in a single
/// byte array, which is required for certain Substrate operations. The layout
/// is: [signature_bytes][public_key_bytes].
#[derive(
	Clone,
	Eq,
	PartialEq,
	Hash,
	Encode,
	Decode,
	TypeInfo,
	MaxEncodedLen,
	Ord,
	PartialOrd,
	DecodeWithMemTracking,
)]
pub struct DilithiumSignatureWithPublic {
	/// Raw bytes containing both signature and public key
	pub bytes: [u8; DilithiumSignatureWithPublic::TOTAL_LEN],
}

impl DilithiumSignatureWithPublic {
	const SIGNATURE_LEN: usize = <DilithiumSignature as ByteArray>::LEN;
	const PUBLIC_LEN: usize = <DilithiumPublic as ByteArray>::LEN;
	pub const TOTAL_LEN: usize = Self::SIGNATURE_LEN + Self::PUBLIC_LEN;

	/// Creates a new combined signature and public key structure
	///
	/// # Arguments
	/// * `signature` - The Dilithium signature
	/// * `public` - The Dilithium public key
	///
	/// # Returns
	/// A new `DilithiumSignatureWithPublic` instance
	pub fn new(signature: DilithiumSignature, public: DilithiumPublic) -> Self {
		let mut bytes = [0u8; Self::LEN];
		bytes[..Self::SIGNATURE_LEN].copy_from_slice(signature.as_ref());
		bytes[Self::SIGNATURE_LEN..].copy_from_slice(public.as_ref());
		Self { bytes }
	}

	/// Extracts the signature portion
	///
	/// # Returns
	/// The `DilithiumSignature` contained in this structure
	pub fn signature(&self) -> DilithiumSignature {
		DilithiumSignature::from_slice(&self.bytes[..Self::SIGNATURE_LEN])
			.expect("Invalid signature")
	}

	/// Extracts the public key portion
	///
	/// # Returns
	/// The `DilithiumPublic` key contained in this structure
	pub fn public(&self) -> DilithiumPublic {
		DilithiumPublic::from_slice(&self.bytes[Self::SIGNATURE_LEN..]).expect("Invalid public key")
	}

	/// Returns the raw bytes
	///
	/// # Returns
	/// A copy of the internal byte array
	pub fn to_bytes(&self) -> [u8; Self::TOTAL_LEN] {
		self.bytes
	}

	/// Creates a `DilithiumSignatureWithPublic` from raw bytes
	///
	/// # Arguments
	/// * `bytes` - Raw bytes containing signature and public key
	///
	/// # Returns
	/// `Ok(DilithiumSignatureWithPublic)` on success, `Err(Error)` if the bytes are invalid
	///
	/// # Errors
	/// Returns `Error::InvalidLength` if the byte array is not the expected length
	pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
		if bytes.len() != Self::TOTAL_LEN {
			return Err(Error::InvalidLength);
		}

		let signature = DilithiumSignature::from_slice(&bytes[..Self::SIGNATURE_LEN])
			.map_err(|_| Error::InvalidLength)?;
		let public = DilithiumPublic::from_slice(&bytes[Self::SIGNATURE_LEN..])
			.map_err(|_| Error::InvalidLength)?;

		Ok(Self::new(signature, public))
	}
}
