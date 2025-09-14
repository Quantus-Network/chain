// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Implementation of the `inspect` subcommand

use crate::{
	utils::{self, print_from_public, print_from_uri},
	with_crypto_scheme, CryptoSchemeFlag, Error, KeystoreParams, NetworkSchemeFlag, OutputTypeFlag,
};
use clap::Parser;
use sp_core::crypto::{ExposeSecret, SecretString, SecretUri, Ss58Codec};
use std::str::FromStr;

/// The `inspect` command
#[derive(Debug, Parser)]
#[command(
	name = "inspect",
	about = "Gets a public key and a SS58 address from the provided Secret URI"
)]
pub struct InspectKeyCmd {
	/// A Key URI to be inspected. May be a secret seed, secret URI
	/// (with derivation paths and password), SS58, public URI or a hex encoded public key.
	/// If it is a hex encoded public key, `--public` needs to be given as argument.
	/// If the given value is a file, the file content will be used
	/// as URI.
	/// If omitted, you will be prompted for the URI.
	uri: Option<String>,

	/// Is the given `uri` a hex encoded public key?
	#[arg(long)]
	public: bool,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub keystore_params: KeystoreParams,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub network_scheme: NetworkSchemeFlag,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub output_scheme: OutputTypeFlag,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub crypto_scheme: CryptoSchemeFlag,

	/// Expect that `--uri` has the given public key/account-id.
	/// If `--uri` has any derivations, the public key is checked against the base `uri`, i.e. the
	/// `uri` without any derivation applied. However, if `uri` has a password or there is one
	/// given by `--password`, it will be used to decrypt `uri` before comparing the public
	/// key/account-id.
	/// If there is no derivation in `--uri`, the public key will be checked against the public key
	/// of `--uri` directly.
	#[arg(long, conflicts_with = "public")]
	pub expect_public: Option<String>,
}

impl InspectKeyCmd {
	/// Run the command
	pub fn run(&self) -> Result<(), Error> {
		let uri = utils::read_uri(self.uri.as_ref())?;
		let password = self.keystore_params.read_password()?;

		if self.public {
			with_crypto_scheme!(
				self.crypto_scheme.scheme,
				print_from_public(
					&uri,
					self.network_scheme.network,
					self.output_scheme.output_type,
				)
			)?;
		} else {
			if let Some(ref expect_public) = self.expect_public {
				with_crypto_scheme!(
					self.crypto_scheme.scheme,
					expect_public_from_phrase(expect_public, &uri, password.as_ref())
				)?;
			}
			with_crypto_scheme!(
				self.crypto_scheme.scheme,
				print_from_uri(
					&uri,
					password,
					self.network_scheme.network,
					self.output_scheme.output_type,
				)
			);
		}

		Ok(())
	}
}

/// Checks that `expect_public` is the public key of `suri`.
///
/// If `suri` has any derivations, `expect_public` is checked against the public key of the "bare"
/// `suri`, i.e. without any derivations.
///
/// Returns an error if the public key does not match.
fn expect_public_from_phrase<Pair: sp_core::Pair>(
	expect_public: &str,
	suri: &str,
	password: Option<&SecretString>,
) -> Result<(), Error> {
	let secret_uri = SecretUri::from_str(suri).map_err(|e| format!("{:?}", e))?;
	let expected_public = if let Some(public) = expect_public.strip_prefix("0x") {
		let hex_public = array_bytes::hex2bytes(public)
			.map_err(|_| format!("Invalid expected public key hex: `{}`", expect_public))?;
		Pair::Public::try_from(&hex_public)
			.map_err(|_| format!("Invalid expected public key: `{}`", expect_public))?
	} else {
		Pair::Public::from_string_with_version(expect_public)
			.map_err(|_| format!("Invalid expected account id: `{}`", expect_public))?
			.0
	};

	let pair = Pair::from_string_with_seed(
		secret_uri.phrase.expose_secret().as_str(),
		password
			.or_else(|| secret_uri.password.as_ref())
			.map(|p| p.expose_secret().as_str()),
	)
	.map_err(|e| format!("Invalid secret uri: {} {}", suri, e))?
	.0;

	if pair.public() == expected_public {
		Ok(())
	} else {
		Err(format!("Expected public ({}) key does not match.", expect_public).into())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use dilithium_crypto::DilithiumPair;
	use sp_core::crypto::Ss58AddressFormat;
	use sp_core::crypto::{ByteArray, Pair};
	use sp_runtime::traits::IdentifyAccount;

	#[test]
	fn inspect() {
		let words =
			"click swarm disagree amazing search riot illness light autumn crash client genius";
		let seed = "0x51c5d97537e23bf36fbae060a8d096e4f84c4d19fc2bfc5e6132bc521486cc8b";

		let inspect = InspectKeyCmd::parse_from(&["inspect-key", words, "--password", "12345"]);
		assert!(inspect.run().is_ok());

		let inspect = InspectKeyCmd::parse_from(&["inspect-key", seed]);
		assert!(inspect.run().is_ok());
	}

	#[test]
	fn inspect_public_key() {
		let public = "0x2c56c79fc5bc332e689d4c6bb600e94b517b02facf8ff275f8a045b441a121d0c14897cacb107534cdd8bb18770295d9188950426ec2c5be379f59e1161f381942b8168523892df3fdfa9115be679f77cc83fd2be9d05694ff11518e447aa2f16920f8edf09b754299e8a4ff73e84fe30fd8f7d6a3bebfbf94e35aeacf8c7903493142f49991a0e5e85865ce1d93c60f7af80e7a0e54b512a6afbdd419456a4bee1e3b39e35e44558b2eb4b8f17d42175fcdedc9b8b11c37b8b0d3e8660e61614421eae24729c1f6563764e5f830c77940bfb78f6078235e0615f0a68db61baab77cf0d9b589a70ff9dcd108ab91dc68548ddf9da6014da3b8ee0af85cc8520d1086d5d28923c80019822297826418ea16cc698b863b51ac63eb33861880992a31096e136a3711b75dd5f1e6a1c4e215318840acdd52534c5567132619569174f4345c39ea9c97cff1efb3057960d601dc8d4fd685970a4765cd0d62c552cbdf6a771e3bf8364a7bbaab8638d750771f7410e06b47258877ecd92b089b454f8536429379b2435f4952bd53f9467ecf248c2c170675bcb2d00f6307dce51333906215e9b516bd130765785c0609e62ca4a8c099b79324a51ad28fa96ccf7182ee9eb3f240262a581b311c95036d79ad8cb9e4ceda41fac45edcdcbd102f880335751e63c1da991fe3564f41208407ca97b2c92c23f2ab81c3f38a52811005f38dd606f7ae9b7b5a3125be41ac795283f56a647eced87362b21792e28d7095149d5415c2c8920779c11ba154c4e73dfa5433c50580e545c148188050eaeb5bbcf051c985e4dec034c8ddcd1274ffd611ae901ec79713407a87af19ea27db2d3409e8d776b3005783d8b14819ff266b9d2e2864e7d18e88684c14c0ccd97c38390d7ce909d3074fcf59da8259fec59d570983b779dc9bfc0127aa1cc5200578a20045ec68d9e75f3eeddd65fb23e1a11a105732e6e8bea542b4c3c9bb343929502bc58a840a54b8a4cab3081a8c9f24b92d68063208871f9ce18e88c7ec4bd5a36375b5d169e61a81eb467540c0c098f3d357a49f34e3219ac7bee62f8b285fcfb4cf0c88d0b87233f14e35bb2aa7d6e584c7a29ba878c46abf45b2b5c6a44b8fcae40c037e34fcb8804f765fab4711c662e009a9370533e09cdca568eec00ae122beb9a7282ee951d173d05b2b6d7ebf5b4026dc059d905223222898603f4524d4ad885ad5c41749abe357c1f79531a6390fd59e88826212d2c23740ba3c6d4d4d2f90a1b973753e429c3eb02f0dabd7890eda7ff5f9986e4931dc0ce2e4977d1b37edb7673e5d851750a94ffd55cfb2a2f3d80cd8dcc0cce5fe06e1fb5f0d0dabcbbd6db8e3a14d3f6ad18a467d6519bfc621dd2441dafe6a6b6423f34c53d93445a712f0ffc613b03fd650f2ff0870e0ff85d300b7aa65c1297597dca99d8598d9f36945cfe2d7e84b9a90749a66750183fcc70171ef967082691655b6b4723fa05162dc083c7a69180d0b0ae22ae3cbdf62795925937dde46b2814cb8062b253bb59577d8d158415dc345c1cbbe4027c2a3f7d7e81cc7fd81f1e003a15a53592f52ac4bcdbe9482be1a63aeb45c894486a8dba00ee4f5b7ecea48f1b27be12d217f54568de9e6e8e4e661d082639a7df606d648d1c47ad867555c4e58ea8dc2b46a5f4e078a40ede766711e66ac1881906e0615a55a415decf82b4d890eaa3339a98950292e85bbe471fae7ade76a0780f751f9c1881e1d0145ca2e89ff25d6392b13471d9be7dabf9a9f723ee7cc923ac8580ee0a1beb8bded86c684eb59870557ef1ec2781fc320d9f8f992bfa0a98326cf8d1d254e51ffa0c34add9d87a8b45ed61e8d24742ac3902b2b24335a9ce4f6f20edb2a1bad45984ea9553cb7ca2f2812f9576cda87883e185eea0b62f866d0fd59356665b2017c26ba76a03ae411fce4bf113cca72f43af55daef4631fb8bdd0f21caa1ab043cc5cc0f932357e988b57f343f36ee3fef019292c715c8eee16527d7d2fff9e31a83420a07e273abb863ee857652dee297a44df42273e6e87a004d173fd884f65318b04fe4deb9c82ebf2f011399d09b2faf47614dad22513a9bad62b834d027b27ba04ad97fd0e7aacab91068ee1e91964b8c5be763342b81f43bbcf11ff9f3d76e8cd87a43128411584bbad2536ff72ca674cfee132aea9d9e0d47fe6696aad13f7d73f6ee97c7ef2fd1a28b333acf3d7db6e44381026c9f392e96f4e4fb43bab81c94f786354ff00c7018d4276fb684d4a19d0c1e560bd2e5a1b05995fefaddaebe3c5dc47bc95160d3a2019edc9dc3d41c2c0af76b40bb826e68b17f1d620f695093ba7e817445115211b8e2cdab37ce2cf4d3468024bfe2a8cf9deac7e2951feb31e846f053ac2a71ceb63e0a6acaf66fee5aa93c3c19663ff9be818738b6fcab05b6e3ccce8032bf4e800f9db5ea24d989837c93390f6eac07cb55b05c682edf167447b3637b1f155d668b26f0236102eb33b9547d8b2b617cf9d9d20b03289255a83a21b3f4853533175e50cd6ed8fba31fcecc2e413df02b4a08a58b92d0ba11b1eb1e4cfa37d92a1925c2e7f6b6ebfb4dfa1ca3630ad3c14332913268b282bf907571aafb4a0206dd29d4eafabf7572e9c049d1ca7696299ded2448d914c62af966e3140071047b99bfc6dc4bf459b2e59190ad1f078daccb3cdcac10f44cb01477a0999d1f62468ef7f1608cb67d87d71aa67cc97434cd31ac857a4772edd2024ba5c27b14997d6451d169b2d00099c30c49048b3c8cb9f1706747ebcf6699176db9dfbae88cdb5a1ad2c965393b709e36127cdc4f9cbe9789cfeaeb79e48d6abc275eee8c30c5f51269274573980ee045500ac38469b87a246654880fb560b5da7155e4ac58fd4f62fdb22047b90f729703098842699c1fcfce61fdf910cb63eff88385827fa7cc03b173623b66d77c0232d3f0eccc3274040b9cc3dc47b01b49003a5a77c60452fb77ffd9a251b48ea8950e652b7419fc3f254b6cca3b81cfb54f68c5508a86d5da2f1930b85b6b84150fa0fa13121e62466a346547652fe6c45f4154bc480d59e09c2a501bea2815ffd66c350a24c222d6f9e9b282a5a5f72faf9916f43fe8dd4ac2955cb62487ee71783bd67e9bc69c8a31954a14cf2446b846783d9f42a118b76b8bfd67b6eb52498e6da40062c79ffecaf1051e84a1e7c313c7583cfcd5121327540b8b4081d16693e935cd2ec29704ae5bca185d1bc05f3910f3177904a7bab807af7937bd73c3369f7abbcaefb93e0bbb24a6057318682c3912e939b693b90cc0cd68d3b1a3daa8fafe431a16d9562edde5cdf12a239eea251f9b4e47a2f85bd8916327c9455d350e7e5a17b385c0b316d37e0e7b55cf296ee444c8e85289ba388a83d0eed3d657a2baf1196aa715208fa104a8167c8ed34d9889bde44abe807fde76e5d29097b4cfffc39d609e3772925eb8c5d26a9659bbff17c504f71b62e7fa68a2b6f71965e5e847ec4332a57809dfa9b1c2d38d017c429e3fe58e59e9113077433968a39a2f3dcdffea5a45c61796167c4906e76984690e9587bf28b5741b5e8bc05ec974118e7833c32634edd9041eb970f45f55912c5e7197c07c9b4c42e47c0cee3c116e3bcb949083ce96639b05b4777ac2eab";

		let inspect = InspectKeyCmd::parse_from(&["inspect-key", "--public", public]);
		assert!(inspect.run().is_ok());
	}

	#[test]
	fn inspect_with_expected_public_key() {
		let check_cmd = |seed, expected_public, success| {
			let inspect = InspectKeyCmd::parse_from(&[
				"inspect-key",
				"--expect-public",
				expected_public,
				seed,
			]);
			let res = inspect.run();

			if success {
				assert!(res.is_ok());
			} else {
				println!("res {:?}", &res);
				assert!(res.unwrap_err().to_string().contains(&format!(
					"Expected public ({}) key does not match.",
					expected_public
				)));
			}
		};

		// Helper function to get public key using the same CLI approach
		let get_public_key = |seed_phrase: &str| -> (String, String) {
			use sp_core::crypto::SecretUri;
			use std::str::FromStr;

			let uri = SecretUri::from_str(seed_phrase).expect("Valid URI");
			let password: Option<&str> = uri.password.as_ref().map(|s| s.expose_secret().as_ref());
			let pair: DilithiumPair =
				Pair::from_string(uri.phrase.expose_secret().as_str(), password).expect("Valid");
			let public = pair.public();
			let public_hex = array_bytes::bytes2hex("0x", public.as_slice());
			let account_id = format!(
				"{}",
				public.into_account().to_ss58check_with_version(Ss58AddressFormat::custom(189))
			);
			(public_hex, account_id)
		};

		let seed =
			"remember fiber forum demise paper uniform squirrel feel access exclude casual effort";
		let invalid_public = "0x2c56c79fc5bc332e689d4c6bb600e94b517b02facf8ff275f8a045b441a121d0c14897cacb107534cdd8bb18770295d9188950426ec2c5be379f59e1161f381942b8168523892df3fdfa9115be679f77cc83fd2be9d05694ff11518e447aa2f16920f8edf09b754299e8a4ff73e84fe30fd8f7d6a3bebfbf94e35aeacf8c7903493142f49991a0e5e85865ce1d93c60f7af80e7a0e54b512a6afbdd419456a4bee1e3b39e35e44558b2eb4b8f17d42175fcdedc9b8b11c37b8b0d3e8660e61614421eae24729c1f6563764e5f830c77940bfb78f6078235e0615f0a68db61baab77cf0d9b589a70ff9dcd108ab91dc68548ddf9da6014da3b8ee0af85cc8520d1086d5d28923c80019822297826418ea16cc698b863b51ac63eb33861880992a31096e136a3711b75dd5f1e6a1c4e215318840acdd52534c5567132619569174f4345c39ea9c97cff1efb3057960d601dc8d4fd685970a4765cd0d62c552cbdf6a771e3bf8364a7bbaab8638d750771f7410e06b47258877ecd92b089b454f8536429379b2435f4952bd53f9467ecf248c2c170675bcb2d00f6307dce51333906215e9b516bd130765785c0609e62ca4a8c099b79324a51ad28fa96ccf7182ee9eb3f240262a581b311c95036d79ad8cb9e4ceda41fac45edcdcbd102f880335751e63c1da991fe3564f41208407ca97b2c92c23f2ab81c3f38a52811005f38dd606f7ae9b7b5a3125be41ac795283f56a647eced87362b21792e28d7095149d5415c2c8920779c11ba154c4e73dfa5433c50580e545c148188050eaeb5bbcf051c985e4dec034c8ddcd1274ffd611ae901ec79713407a87af19ea27db2d3409e8d776b3005783d8b14819ff266b9d2e2864e7d18e88684c14c0ccd97c38390d7ce909d3074fcf59da8259fec59d570983b779dc9bfc0127aa1cc5200578a20045ec68d9e75f3eeddd65fb23e1a11a105732e6e8bea542b4c3c9bb343929502bc58a840a54b8a4cab3081a8c9f24b92d68063208871f9ce18e88c7ec4bd5a36375b5d169e61a81eb467540c0c098f3d357a49f34e3219ac7bee62f8b285fcfb4cf0c88d0b87233f14e35bb2aa7d6e584c7a29ba878c46abf45b2b5c6a44b8fcae40c037e34fcb8804f765fab4711c662e009a9370533e09cdca568eec00ae122beb9a7282ee951d173d05b2b6d7ebf5b4026dc059d905223222898603f4524d4ad885ad5c41749abe357c1f79531a6390fd59e88826212d2c23740ba3c6d4d4d2f90a1b973753e429c3eb02f0dabd7890eda7ff5f9986e4931dc0ce2e4977d1b37edb7673e5d851750a94ffd55cfb2a2f3d80cd8dcc0cce5fe06e1fb5f0d0dabcbbd6db8e3a14d3f6ad18a467d6519bfc621dd2441dafe6a6b6423f34c53d93445a712f0ffc613b03fd650f2ff0870e0ff85d300b7aa65c1297597dca99d8598d9f36945cfe2d7e84b9a90749a66750183fcc70171ef967082691655b6b4723fa05162dc083c7a69180d0b0ae22ae3cbdf62795925937dde46b2814cb8062b253bb59577d8d158415dc345c1cbbe4027c2a3f7d7e81cc7fd81f1e003a15a53592f52ac4bcdbe9482be1a63aeb45c894486a8dba00ee4f5b7ecea48f1b27be12d217f54568de9e6e8e4e661d082639a7df606d648d1c47ad867555c4e58ea8dc2b46a5f4e078a40ede766711e66ac1881906e0615a55a415decf82b4d890eaa3339a98950292e85bbe471fae7ade76a0780f751f9c1881e1d0145ca2e89ff25d6392b13471d9be7dabf9a9f723ee7cc923ac8580ee0a1beb8bded86c684eb59870557ef1ec2781fc320d9f8f992bfa0a98326cf8d1d254e51ffa0c34ffd9d87a8b45ed61e8d24742ac3902b2b24335a9ce4f6f20edb2a1bad45984ea9553cb7ca2f2812f9576cda87883e185eea0b62f866d0fd59356665b2017c26ba76a03ae411fce4bf113cca72f43af55daef4631fb8bdd0f21caa1ab043cc5cc0f932357e988b57f343f36ee3fef019292c715c8eee16527d7d2fff9e31a83420a07e273abb863ee857652dee297a44df42273e6e87a004d173fd884f65318b04fe4deb9c82ebf2f011399d09b2faf47614dad22513a9bad62b834d027b27ba04ad97fd0e7aacab91068ee1e91964b8c5be763342b81f43bbcf11ff9f3d76e8cd87a43128411584bbad2536ff72ca674cfee132aea9d9e0d47fe6696aad13f7d73f6ee97c7ef2fd1a28b333acf3d7db6e44381026c9f392e96f4e4fb43bab81c94f786354ff00c7018d4276fb684d4a19d0c1e560bd2e5a1b05995fefaddaebe3c5dc47bc95160d3a2019edc9dc3d41c2c0af76b40bb826e68b17f1d620f695093ba7e817445115211b8e2cdab37ce2cf4d3468024bfe2a8cf9deac7e2951feb31e846f053ac2a71ceb63e0a6acaf66fee5aa93c3c19663ff9be818738b6fcab05b6e3ccce8032bf4e800f9db5ea24d989837c93390f6eac07cb55b05c682edf167447b3637b1f155d668b26f0236102eb33b9547d8b2b617cf9d9d20b03289255a83a21b3f4853533175e50cd6ed8fba31fcecc2e413df02b4a08a58b92d0ba11b1eb1e4cfa37d92a1925c2e7f6b6ebfb4dfa1ca3630ad3c14332913268b282bf907571aafb4a0206dd29d4eafabf7572e9c049d1ca7696299ded2448d914c62af966e3140071047b99bfc6dc4bf459b2e59190ad1f078daccb3cdcac10f44cb01477a0999d1f62468ef7f1608cb67d87d71aa67cc97434cd31ac857a4772edd2024ba5c27b14997d6451d169b2d00099c30c49048b3c8cb9f1706747ebcf6699176db9dfbae88cdb5a1ad2c965393b709e36127cdc4f9cbe9789cfeaeb79e48d6abc275eee8c30c5f51269274573980ee045500ac38469b87a246654880fb560b5da7155e4ac58fd4f62fdb22047b90f729703098842699c1fcfce61fdf910cb63eff88385827fa7cc03b173623b66d77c0232d3f0eccc3274040b9cc3dc47b01b49003a5a77c60452fb77ffd9a251b48ea8950e652b7419fc3f254b6cca3b81cfb54f68c5508a86d5da2f1930b85b6b84150fa0fa13121e62466a346547652fe6c45f4154bc480d59e09c2a501bea2815ffd66c350a24c222d6f9e9b282a5a5f72faf9916f43fe8dd4ac2955cb62487ee71783bd67e9bc69c8a31954a14cf2446b846783d9f42a118b76b8bfd67b6eb52498e6da40062c79ffecaf1051e84a1e7c313c7583cfcd5121327540b8b4081d16693e935cd2ec29704ae5bca185d1bc05f3910f3177904a7bab807af7937bd73c3369f7abbcaefb93e0bbb24a6057318682c3912e939b693b90cc0cd68d3b1a3daa8fafe431a16d9562edde5cdf12a239eea251f9b4e47a2f85bd8916327c9455d350e7e5a17b385c0b316d37e0e7b55cf296ee444c8e85289ba388a83d0eed3d657a2baf1196aa715208fa104a8167c8ed34d9889bde44abe807fde76e5d29097b4cfffc39d609e3772925eb8c5d26a9659bbff17c504f71b62e7fa68a2b6f71965e5e847ec4332a57809dfa9b1c2d38d017c429e3fe58e59e9113077433968a39a2f3dcdffea5a45c61796167c4906e76984690e9587bf28b5741b5e8bc05ec974118e7833c32634edd9041eb970f45f55912c5e7197c07c9b4c42e47c0cee3c116e3bcb949083ce96639b05b4777ac2eab";
		let (valid_public_hex, _) = get_public_key(seed);
		println!("Aa {valid_public_hex}");

		// It should fail with the invalid public key
		check_cmd(seed, invalid_public, false);

		println!("Aa {seed}");
		// It should work with the valid public key & account id
		check_cmd(seed, &valid_public_hex, true);

		let password = "test12245";
		let seed_with_password = format!("{}///{}", seed, password);
		let (valid_public_hex_with_password, _) = get_public_key(&seed_with_password);

		// Only the public key that corresponds to the seed with password should be accepted.
		check_cmd(&seed_with_password, &valid_public_hex, false);

		check_cmd(&seed_with_password, &valid_public_hex_with_password, true);
	}
}
