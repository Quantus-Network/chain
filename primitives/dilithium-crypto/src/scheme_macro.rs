/// Generates a complete Dilithium signature scheme (tag, pair, public/signature
/// aliases, signature-with-public type, trait impls and a raw verify function)
/// for one `qp_rusty_crystals_dilithium` parameter-set module (e.g. `ml_dsa_87`).
///
/// All variants feed the shared `DilithiumSignatureScheme` / `DilithiumSigner`
/// enums through the `variant` enum case name.
macro_rules! define_dilithium_scheme {
	(
		variant = $variant:ident,
		tag = $tag:ident,
		pair = $pair:ident,
		public = $public:ident,
		signature = $signature:ident,
		sig_with_public = $sig_with_public:ident,
		module = $module:ident,
		verify_fn = $verify_fn:ident
		$(,)?
	) => {
		#[derive(
			Clone,
			Eq,
			PartialEq,
			Debug,
			Hash,
			codec::Encode,
			codec::Decode,
			scale_info::TypeInfo,
			Ord,
			PartialOrd,
			codec::DecodeWithMemTracking,
		)]
		pub struct $tag;

		/// Dilithium cryptographic key pair
		///
		/// Contains both secret and public key material for this parameter set.
		#[derive(Clone, Eq, PartialEq)]
		pub struct $pair {
			pub(crate) secret: [u8; qp_rusty_crystals_dilithium::$module::SECRETKEYBYTES],
			pub(crate) public: [u8; qp_rusty_crystals_dilithium::$module::PUBLICKEYBYTES],
		}

		impl alloc::fmt::Debug for $pair {
			fn fmt(&self, f: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
				f.debug_struct(stringify!($pair))
					.field("public", &self.public)
					.finish_non_exhaustive()
			}
		}

		pub type $public = $crate::types::WrappedPublicBytes<
			{ qp_rusty_crystals_dilithium::$module::PUBLICKEYBYTES },
			$tag,
		>;
		pub type $signature = $crate::types::WrappedSignatureBytes<
			{ qp_rusty_crystals_dilithium::$module::SIGNBYTES },
			$tag,
		>;

		/// Combined signature and public key structure.
		///
		/// Required because the runtime address is a hash of the public key, so the
		/// signature must carry the public key along. Byte layout:
		/// `[signature_bytes][public_key_bytes]`.
		#[derive(
			Clone,
			Eq,
			PartialEq,
			Hash,
			codec::Encode,
			codec::Decode,
			scale_info::TypeInfo,
			codec::MaxEncodedLen,
			Ord,
			PartialOrd,
			codec::DecodeWithMemTracking,
		)]
		pub struct $sig_with_public {
			/// Raw bytes containing both signature and public key
			pub bytes: [u8; $sig_with_public::TOTAL_LEN],
		}

		impl $sig_with_public {
			const SIGNATURE_LEN: usize = <$signature as sp_core::ByteArray>::LEN;
			const PUBLIC_LEN: usize = <$public as sp_core::ByteArray>::LEN;
			pub const TOTAL_LEN: usize = Self::SIGNATURE_LEN + Self::PUBLIC_LEN;

			/// Creates a new combined signature and public key structure
			pub fn new(signature: $signature, public: $public) -> Self {
				let mut bytes = [0u8; <Self as sp_core::ByteArray>::LEN];
				bytes[..Self::SIGNATURE_LEN].copy_from_slice(signature.as_ref());
				bytes[Self::SIGNATURE_LEN..].copy_from_slice(public.as_ref());
				Self { bytes }
			}

			/// Extracts the signature portion
			pub fn signature(&self) -> $signature {
				<$signature as sp_core::ByteArray>::from_slice(&self.bytes[..Self::SIGNATURE_LEN])
					.expect("Invalid signature")
			}

			/// Extracts the public key portion
			pub fn public(&self) -> $public {
				<$public as sp_core::ByteArray>::from_slice(&self.bytes[Self::SIGNATURE_LEN..])
					.expect("Invalid public key")
			}

			/// Returns the raw bytes
			pub fn to_bytes(&self) -> [u8; Self::TOTAL_LEN] {
				self.bytes
			}

			/// Creates from raw bytes containing signature and public key
			///
			/// # Errors
			/// Returns `Error::InvalidLength` if the byte array is not the expected length
			pub fn from_bytes(bytes: &[u8]) -> Result<Self, $crate::types::Error> {
				if bytes.len() != Self::TOTAL_LEN {
					return Err($crate::types::Error::InvalidLength);
				}

				let signature = <$signature as sp_core::ByteArray>::from_slice(
					&bytes[..Self::SIGNATURE_LEN],
				)
				.map_err(|_| $crate::types::Error::InvalidLength)?;
				let public = <$public as sp_core::ByteArray>::from_slice(
					&bytes[Self::SIGNATURE_LEN..],
				)
				.map_err(|_| $crate::types::Error::InvalidLength)?;

				Ok(Self::new(signature, public))
			}
		}

		impl alloc::fmt::Debug for $sig_with_public {
			#[cfg(feature = "std")]
			fn fmt(&self, f: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
				write!(
					f,
					"{} {{ signature: {:?}, public: {:?} }}",
					stringify!($sig_with_public),
					self.signature(),
					self.public()
				)
			}

			#[cfg(not(feature = "std"))]
			fn fmt(&self, f: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
				write!(f, stringify!($sig_with_public))
			}
		}

		impl From<$sig_with_public> for $crate::types::DilithiumSignatureScheme {
			fn from(x: $sig_with_public) -> Self {
				Self::$variant(x)
			}
		}

		impl TryFrom<$crate::types::DilithiumSignatureScheme> for $sig_with_public {
			type Error = ();
			fn try_from(m: $crate::types::DilithiumSignatureScheme) -> Result<Self, Self::Error> {
				match m {
					$crate::types::DilithiumSignatureScheme::$variant(sig_with_public) =>
						Ok(sig_with_public),
					_ => Err(()),
				}
			}
		}

		impl AsMut<[u8]> for $sig_with_public {
			fn as_mut(&mut self) -> &mut [u8] {
				self.bytes.as_mut()
			}
		}

		impl TryFrom<&[u8]> for $sig_with_public {
			type Error = ();
			fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
				if data.len() != Self::TOTAL_LEN {
					return Err(());
				}
				let (sig_bytes, pub_bytes) = data.split_at(Self::SIGNATURE_LEN);
				let signature =
					<$signature as sp_core::ByteArray>::from_slice(sig_bytes).map_err(|_| ())?;
				let public = <$public as sp_core::ByteArray>::from_slice(pub_bytes).map_err(|_| ())?;
				Ok(Self::new(signature, public))
			}
		}

		impl sp_core::ByteArray for $sig_with_public {
			const LEN: usize = Self::TOTAL_LEN;

			fn to_raw_vec(&self) -> alloc::vec::Vec<u8> {
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

		impl AsRef<[u8; <Self as sp_core::ByteArray>::LEN]> for $sig_with_public {
			fn as_ref(&self) -> &[u8; <Self as sp_core::ByteArray>::LEN] {
				&self.bytes
			}
		}

		impl AsRef<[u8]> for $sig_with_public {
			fn as_ref(&self) -> &[u8] {
				&self.bytes
			}
		}

		impl sp_core::crypto::Signature for $sig_with_public {}

		impl sp_runtime::CryptoType for $sig_with_public {
			type Pair = $pair;
		}

		impl<const N: usize> sp_runtime::CryptoType for $crate::types::WrappedPublicBytes<N, $tag> {
			type Pair = $pair;
		}

		impl<const N: usize> sp_runtime::CryptoType for $crate::types::WrappedSignatureBytes<N, $tag> {
			type Pair = $pair;
		}

		impl sp_runtime::traits::IdentifyAccount for $public {
			type AccountId = sp_runtime::AccountId32;
			fn into_account(self) -> Self::AccountId {
				// Use injective encoding for account ID derivation (collision-resistant for security)
				let public_bytes: &[u8] = self.as_ref();
				sp_runtime::AccountId32::new(qp_poseidon_core::hash_bytes(public_bytes))
			}
		}

		impl From<$public> for sp_runtime::AccountId32 {
			fn from(public: $public) -> Self {
				sp_runtime::traits::IdentifyAccount::into_account(public)
			}
		}

		impl From<$public> for $crate::types::DilithiumSigner {
			fn from(x: $public) -> Self {
				Self::$variant(x)
			}
		}

		impl sp_runtime::CryptoType for $pair {
			type Pair = Self;
		}

		impl sp_runtime::traits::IdentifyAccount for $pair {
			type AccountId = sp_runtime::AccountId32;
			fn into_account(self) -> sp_runtime::AccountId32 {
				sp_runtime::AccountId32::new(qp_poseidon_core::hash_bytes(&self.public))
			}
		}

		impl $pair {
			pub fn from_seed(seed: &[u8]) -> Result<Self, $crate::types::Error> {
				if seed.len() < qp_rusty_crystals_dilithium::params::SEEDBYTES {
					return Err($crate::types::Error::InsufficientEntropy {
						required: qp_rusty_crystals_dilithium::params::SEEDBYTES,
						actual: seed.len(),
					});
				}
				let mut entropy_array = [0u8; 32];
				entropy_array.copy_from_slice(&seed[..32]);
				let keypair = qp_rusty_crystals_dilithium::$module::Keypair::generate(
					qp_rusty_crystals_dilithium::SensitiveBytes32::from(&mut entropy_array),
				);
				Ok(Self {
					secret: *keypair.secret().to_bytes(),
					public: keypair.public().to_bytes(),
				})
			}

			pub fn from_keypair(keypair: qp_rusty_crystals_dilithium::$module::Keypair) -> Self {
				Self {
					secret: *keypair.secret().to_bytes(),
					public: keypair.public().to_bytes(),
				}
			}

			/// Create a pair from raw public and secret key bytes.
			/// Use when reconstructing a pair from stored/serialized key material (e.g. wallet
			/// restore).
			///
			/// Rejects mismatched or corrupted key pairs: `Keypair::from_parts` re-derives the
			/// public key from the secret and requires an exact match, so a pair that would
			/// cause non-obvious signature verification failures downstream is unrepresentable.
			pub fn from_raw(public: &[u8], secret: &[u8]) -> Result<Self, $crate::types::Error> {
				let secret = qp_rusty_crystals_dilithium::$module::SecretKey::from_bytes(secret)
					.map_err(|_| $crate::types::Error::InvalidSecretKey)?;
				let public = qp_rusty_crystals_dilithium::$module::PublicKey::from_bytes(public)
					.map_err(|_| $crate::types::Error::InvalidPublicKey)?;
				let keypair = qp_rusty_crystals_dilithium::$module::Keypair::from_parts(secret, public)
					.map_err(|_| $crate::types::Error::InvalidPublicKey)?;
				Ok(Self {
					secret: *keypair.secret().to_bytes(),
					public: keypair.public().to_bytes(),
				})
			}

			pub fn secret_bytes(&self) -> &[u8] {
				&self.secret
			}

			pub fn public_bytes(&self) -> &[u8] {
				&self.public
			}
		}

		impl sp_core::Pair for $pair {
			type Public = $public;
			type Seed = [u8; 32];
			type Signature = $sig_with_public;
			type ProofOfPossession = $sig_with_public;

			// Dilithium doesn't support path-based derivation - use hdwallet in
			// qp-rusty-crystals instead. An empty path is a valid no-op returning self unchanged.
			fn derive<Iter: Iterator<Item = sp_core::crypto::DeriveJunction>>(
				&self,
				path_iter: Iter,
				seed: Option<Self::Seed>,
			) -> Result<(Self, Option<Self::Seed>), sp_core::crypto::DeriveError> {
				let mut path_iter = path_iter.peekable();
				if path_iter.peek().is_none() {
					return Ok((self.clone(), seed));
				}

				log::error!(
					"Pair::derive() is not supported for Dilithium. \
					 Use qp_rusty_crystals_hdwallet::derive_key_from_mnemonic() for HD key derivation."
				);
				// SoftKeyInPath is the only error available for this trait definition.
				Err(sp_core::crypto::DeriveError::SoftKeyInPath)
			}

			fn from_seed_slice(seed: &[u8]) -> Result<Self, sp_core::crypto::SecretStringError> {
				Self::from_seed(seed).map_err(|_| sp_core::crypto::SecretStringError::InvalidSeed)
			}

			#[cfg(feature = "full_crypto")]
			fn sign(&self, message: &[u8]) -> $sig_with_public {
				let secret = qp_rusty_crystals_dilithium::$module::SecretKey::from_bytes(
					&self.secret,
				)
				.expect("Failed to parse secret key");

				let signature = secret.sign(message, None, None).expect("Signing should not fail");
				let signature_bytes: &[u8] = signature.as_ref();
				let signature = <$signature as TryFrom<&[u8]>>::try_from(signature_bytes)
					.expect("Wrap doesn't fail");

				$sig_with_public::new(signature, <Self as sp_core::Pair>::public(self))
			}

			fn verify<M: AsRef<[u8]>>(
				sig: &$sig_with_public,
				message: M,
				pubkey: &$public,
			) -> bool {
				let sig_scheme = $crate::types::DilithiumSignatureScheme::$variant(sig.clone());
				let signer = $crate::types::DilithiumSigner::$variant(pubkey.clone());
				let account = sp_runtime::traits::IdentifyAccount::into_account(signer);
				sp_runtime::traits::Verify::verify(&sig_scheme, message.as_ref(), &account)
			}

			fn public(&self) -> Self::Public {
				<$public as sp_core::ByteArray>::from_slice(&self.public)
					.expect("Valid public key bytes")
			}

			fn to_raw_vec(&self) -> alloc::vec::Vec<u8> {
				// this is modeled after sr25519 which returns the private key for this method
				self.secret.to_vec()
			}

			// NOTE: This method does not parse all secret uris correctly, like
			// "mnemonic///password///account" This was supported in standard substrate, if
			// there is demand, we can support it in the future
			fn from_string(
				s: &str,
				password_override: Option<&str>,
			) -> Result<Self, sp_core::crypto::SecretStringError> {
				let res = <Self as sp_core::Pair>::from_phrase(s, password_override)
					.map_err(|_| sp_core::crypto::SecretStringError::InvalidPhrase)?;
				Ok(res.0)
			}

			#[cfg(feature = "std")]
			fn from_phrase(
				phrase: &str,
				password: Option<&str>,
			) -> Result<(Self, Self::Seed), sp_core::crypto::SecretStringError> {
				// Default derivation path for Quantus: m/44'/189189'/0'/0'/0'
				const DEFAULT_PATH: &str = "m/44'/189189'/0'/0'/0'";
				let keypair = qp_rusty_crystals_hdwallet::$module::derive_key_from_mnemonic(
					phrase,
					password,
					DEFAULT_PATH,
				)
				.map_err(|_| sp_core::crypto::SecretStringError::InvalidPhrase)?;
				let pair = Self::from_keypair(keypair);
				let seed_bytes = qp_rusty_crystals_hdwallet::mnemonic_to_seed(
					phrase.to_string(),
					password,
				)
				.map_err(|_| sp_core::crypto::SecretStringError::InvalidPhrase)?;
				let mut seed = [0u8; 32];
				seed.copy_from_slice(&seed_bytes[..32]);
				Ok((pair, seed))
			}

			#[cfg(feature = "std")]
			fn from_string_with_seed(
				s: &str,
				password: Option<&str>,
			) -> Result<(Self, Option<Self::Seed>), sp_core::crypto::SecretStringError> {
				// Default derivation path for Quantus: m/44'/189189'/0'/0'/0'
				const DEFAULT_PATH: &str = "m/44'/189189'/0'/0'/0'";
				let keypair = qp_rusty_crystals_hdwallet::$module::derive_key_from_mnemonic(
					s,
					password,
					DEFAULT_PATH,
				)
				.map_err(|_| sp_core::crypto::SecretStringError::InvalidPhrase)?;
				let pair = Self::from_keypair(keypair);

				// Return the pair with no seed since Dilithium doesn't use traditional
				// seed-based generation
				Ok((pair, None))
			}
		}

		#[cfg(feature = "std")]
		impl $public {
			/// Attempt to parse a Dilithium public key from a string and return it with the
			/// associated SS58 address format (version).
			///
			/// We primarily support hex-encoded (0x-prefixed) public keys here; SS58
			/// AccountId32 addresses are not convertible back into Dilithium public keys (the
			/// CLI falls back to parsing AccountId32 when this returns an error).
			pub fn from_string_with_version(
				s: &str,
			) -> Result<(Self, sp_core::crypto::Ss58AddressFormat), sp_core::crypto::PublicError>
			{
				use sp_core::crypto::{default_ss58_version, PublicError};
				// Accept 0x-prefixed hex of the raw Dilithium public key bytes.
				let maybe_hex = s.strip_prefix("0x").unwrap_or(s);
				// Expect exact hex length for a Dilithium public key
				let expected_hex_len = <Self as sp_core::ByteArray>::LEN * 2;
				if maybe_hex.len() == expected_hex_len &&
					maybe_hex.chars().all(|c| c.is_ascii_hexdigit())
				{
					let mut bytes = alloc::vec![0u8; <Self as sp_core::ByteArray>::LEN];
					for (i, chunk) in maybe_hex.as_bytes().chunks(2).enumerate() {
						let h = (chunk[0] as char)
							.to_digit(16)
							.ok_or(PublicError::InvalidFormat)? as u8;
						let l = (chunk[1] as char)
							.to_digit(16)
							.ok_or(PublicError::InvalidFormat)? as u8;
						bytes[i] = (h << 4) | l;
					}
					let pk = <Self as sp_core::ByteArray>::from_slice(&bytes)
						.map_err(|_| PublicError::BadLength)?;
					return Ok((pk, default_ss58_version()));
				}
				// Not a supported Dilithium public key representation here.
				Err(PublicError::InvalidFormat)
			}
		}

		/// Verifies a Dilithium signature for this parameter set.
		///
		/// Returns `true` if the signature is valid for `msg` under `pub_key`,
		/// `false` otherwise (including malformed public keys).
		pub fn $verify_fn(pub_key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
			match qp_rusty_crystals_dilithium::$module::PublicKey::from_bytes(pub_key) {
				Ok(pk) => pk.verify(msg, sig, None),
				Err(e) => {
					log::warn!("public key failed to deserialize {:?}", e);
					false
				},
			}
		}
	};
}

pub(crate) use define_dilithium_scheme;
