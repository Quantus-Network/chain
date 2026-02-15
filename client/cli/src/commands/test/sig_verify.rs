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

#![cfg(test)]

//! Integration test that the `sign` and `verify` sub-commands work together.

use crate::*;
use clap::Parser;

const SEED: &str = "tide power better crop pencil arrange trouble luxury pistol coach daughter senior scatter portion power harsh addict journey carry gloom fox voice volume marble";
const ALICE: &str = "0x95394e4a4bafd0041243fe02433b4673e2876239ac061caa6575a23ae6d465d294e57e90a21e167de36465820c43e4a15e60cbda105f5fd652c335c2509728bf73a892605567074d9ba7117d9ff303cd5ca49e92d6c425691d5420efe8123a27451ed4d36b7092c95810fada32f1546cdde4529800a92b706dfef0ac358ed7884e3f59a76c65ad052cf7e961f8d6466c12075d0d8feb8ed83836dd07819f20f6452cfc6ba316d44df0d77f74da1c1a1bf90f00a8b1c335a40a91ece0a2fbbccfb3225088e93fbcc7a2d73e108a65ce18f007ca889396dc706e8fd55b5b51d62df62ca3a3e83fc7d172c3a711e6f242e312aacaab168bdcb526a868a9f31af42cc1a3f0cf1c287e0f8e5fe9460fbce120d58501d9c8bff4b05cf512e29b625282a3fb7ba4d0ed69d04980acbbdd87fdabe3982a27c3d75e27867d91a7efc58356d12f46d5b4a443eaf529094b4e052e6ff7773122e200113532eddfea0e8bbc8d3a1e9bdc7c748e0b58e3ad180e471132e868b779e584deb1543df53a295a8170527b458d0e1095b7fb9ed1cb563b62d8a2ddcbbce50c28493d4ff674216a22e3a457717d5148363c47c2d8dcc144a97264c77745d7e63c42332c456e63ccaba974afb3ea78be59cd889295a249cf9577304d352aa945ee1f93fa3e56d56233bdb5c473e28096f8f9d3eb2e394f96f931d65da138d839cf2889d9cc840ad2b0be34bd07b5b078585ed826f0949b4100d9eb33383d3aa342e082f8ba9a889fa3d9a61c8e8db7adef2302d6b1b434bd3b10c88025956726730cb53d1972bfc44914df6c7003288d9ff67bcb43b962cfeabd343c8e372c6f3f9eff0e34b74a4065c95d01125c0fce1f3d87c1e00db3cf71dd4525cd41ca7c70c2feb99b7f4c20b063655497905686e0980dbe087c0f8375b7cfdb456fee5a62aa0cadd75fc01fa1214551219b60c38bc5d2b1951c998d7a57dd810f54cb6f727510247d839b423cc38d3848f862f8bbdc5ff30f799c6625f73d4a2d7f310f164d6664f32be9a39767ce96722fcabc978e668e8b3a6d67af804428aee96dfd315187d756b1dfa6d3fc2873ae528a89be049cffd19ebc74036b6d1e82ab9690a61b2018628b13457d7282f3659848d078ef01e2924bdc60a790113e97ae5d31d28dff3d0b1807532e2d8c56475e9d511e1cb027dc9e162b74ef1613f0e90fe1a1f13f305f014ed50ff7bd16783b6233f453c4528cb873042e3d7764c88d93a8a82be72ca084519c360eff82bee0ddc8d8d1ebf2212d470e5ab16a32ed842ed24d03e074caaf5cd1b11467b53f759a06a2ab0a8b41f923d534ce5b5d5d560d3b90f5b74079ae19cddf0bbc66423214558d9f276f6755f0b17eb1f4db2322c205be6bbc083d3b85b80a0c2e4af615346c93e89388b3919ee0454774f63c4f793a6d32f627ae510c6cb38d7f156a33c8c32cda6e6571fea3a11d24cbc463048ad85c3f8da1b595287758f42981af8253c04957de87afb96ef84ce11ee984e9bf4937b6e6f25cf88888aea25d8f6bd64960c00dd58f1b4d93a94d6cd32893dfd3d4ae13666c3495633145de0c38646686b6cd5d87e9ddca168ca728c2617e3295ed16d34cfa88d3116f6131f82319c0eec35fce2a4a09d88c1406a0e18d80de586b8de7c691e0c6a883f3c88abd11ed42fc15057313df9a63f89293444ee52c3ec3bf653b2436062b2147de68cf85ba03250aef778a4b0fea4fba1fab47f55d3515ad3902280770d0f3ccdaba4f5d86de46dcf881f883c91cb089b7c007dbc4217d02250cc0097d947a4d5e8047edd35acc0bb3bb1ec23cd8d1583a7a42debea87adc558aba413fe3bdfee284c96e7c9d5684b16193bad232a3c1d948ba6f2260e5d9e1dfa72ee2a0a2aa400e50b193994dfb9af6f6c86d197e6a026b8e7ede020d3316dd3926b25643dffb1d97bf8d311b4840478482e50d32a9c2cb7f58cedd898f1b66c74167de063ab735e83f71fd9a5b8f7c640bee976a690adc0f5c534ee55a1a6f1294436105ba11f6a17c2c1ea0edde73269907dbd097abb2969b5a4ffb984f8e35487b416c3e49e2bde6756cb8689505ab7df4f842923e33443e1055b9f39c7dccfdaa2f5edf8ecddff0b8af2c7b4c440d1d04797a87a912c94ce1c6ae2f96209aa8fa9118b3aca3997532915eb272b0a723655dbdd2a6333c693883382dc6c5a9305b36745c065f7e3eeaa8f71c66e9a1989a7606bd0179459c659456eec29ebc516c829e07b4480540d2aca7e7a75c9f05b7c4c3d8ae857c1b0c2f88da20205093204430bb74619c61e77b7c4b6fa462a67cc221ff466252a8456b6def3515a8e8b2222fcbb8e72994e5c590ef7ae1158ffeb218e0ae8b0f7284d776e1910757bd173d6e80c3103e03fe1e61ac0b4a9b08d2c0a81ec928fe6c2c3e1b10b5324d2d5396abc957cf8d98e2972d315285638f711643bd759cdb8321bdaae40d07aa1b22e2c83038ac3f698fed4d924a030abd9957e3d862e1f4643dd5c791dc41eed6623563c1c4dec796fa32508c59e2b2f1b715379084b22b010ef76a88c6c810f7dd2ad663ada37833ed650d570c33ee6a70153648c53ff108e2c96ee2514a57b8d550858b5217146db7ffac56e443be9a7c7eae9da0dbd2fbe2c9b6473b52b5bd380ca8bacb727866eaca6a50430880ae95775e2a2ff2c1dde61a2ad6b2dd022cb36f14a94da76e5c247d981b3505d958eb063ee926278ce560955f451637f30975ea92872b982eec60391502c2368df417fff0f7b5dee1a246b9b014cd5ce03ddcd7b2bb33cd6890c991b1ab9cbfe755e97fb4d1f0d07878a593d43527c2bf6f15d37d06fbf9279c447fde4d541d44768c96263f40c29150c6ac18cfd65b3735d02f9c562607825a796ecc8de5324f10e807133d98d4d86cabc12bb1f1ed8c4327ebc4d2b66e0bcbd5835659fb65329d3d29b2f2424042c94f920eb895840847707a1e4fb4ac3d61803766ac96cddc71da388e78d8aaa669316aa1c4372c74a419f364f80b265f4f9c1d860920a16718dbc10810a847cfd5a2360225ea7a1f7a5d5dda3f418769237614229f9b1f430002247895262f07b3544870f6734a2eea461afb7efc8f555e46fd78c237baad740894fa76d085ac7b3a10b91ef6c02909d64000de81cbd4d6a53f5214e9e0971e97c64655ba2e6be30df885fc7940802e774daee6d6d8e91886782934398483909a163904146cbb1fd861a2a23675ed72c4f7e738c61548431f832967ecdc2b875b85d535cdd74680eb5eee9237ab02aef5bba67d7faca2707714b97107a93131805f58daabb90ce26bc5e5c1b4d8408a9603f9ee31047c79b8d116515d7bf9efbb9539f85464633d252a3851452b2065d3ad3a5e4e64857dc00e2bd3c85745c19c927b888baf6db6972f4f3a118d57368b4263b8ea9b5f82ad6c80c84d5737c6c18a6904253f0c7689652c394fd683c0171187c7e235ef63b524470c9fa0ec55b7ef034c548e3c2f5b0220f195b4d41c2e2a94e01febc702a2cedb0f870684d7eae646a236f10e9ed1f276c07d80d852d5813aa98f55487521bd5c20749d22382bd52c19214b0d5f124469203037380cb5c5aba2dbf4f7c349167ed9b40fee796e7ecdfd5864c43f034efcc1cf2563389fb50693f1";
const BOB: &str = "0x00c3bbe4bf4a6e746ea7f8c946da33dc074cc7442538f7b87a3f7d292174d859bfc653b7fe7f296d9cf5235b3b0de9acff5bd727dad5eb57fd1d44ec69d9aa8ed2e0bbfafd59acddfc7836c5e4bc54b236fd6c8e70473abd6f814e98b72f7612db46ff97c994932341b64595bb9beae9e2e3b2f06e91c513ff9ad5f2842850808bbda0b0fcc125dc48eb8a09c9f7cc370b7ea17d2cb8cd10d9e86866208b92d6062bd83facd45eb3708a72c519d7f87858a233511d389fcf0351b0e93f2655029b60cd5f853e0285718c3821a606ffe326f16a697867abae3b2dd7b8ab62cc6a0d3f116fbd73c04c703b4d16b6c4a5340fd747fe6b26c0af2ee530996ca8a6561f773a31cfc0478d2bac9791bdc69c2b63aa3032f1ad5711b08134edfb730353b294a3e31ad8f8a1119bb9526f625a664923e6f7cd65d7e472d0b2059932e4ab555351799a106bfef824d5ce6eec330de4f5a6b554b2ad53c0aecdd9e288fa102fee79a7498b5a0468d4d8c04e0bc451d96d731304a19e1357efaf3902878ada3f235ff7f63ef193acbc1d5a7cac1055458c68d6085dffd75e5059bcd49f9d0db61ad894f6420822c41d812d781a68489229a21c9ee12dabad70860ea64f383350692f102b7f00215dd5dd22aeae1bc673fd61d5e09f7dc5fe2cb581f2a12401dffaecb907e036995fc7e454f7ca5cdb5625f270c6b9c37dcfda518bbb18eaa19b5984b437778a02227b71cc33efadfa0a4d344bb035ed69299d63c4e3a78712d246fb079649ddaa0e78bb26ae0cc119a751cee46ab9e2ff701fe4664f0b09fa0acbd8c7d75c3b4025e80f9c01059656b76be0e4b45e470e4b195a1f6da0edd13a37c46dbb807394398b3831d130499fac4b9b6c7a4a3a90fb95bc647fdd013b5d37cdb0e8cb285388ea59b67b9af2186313dba5182881a6ddd21c185a8dc6c813b03547f0ea0430c8b80efcd01f495f4ec11406e9949e0fccd534afac16b43f0e8ccc864941526e9ba3b7b990abd85ba0a1872001d1f58f0eb9bc93e58bef88bc56726de04e8bdea28985c6a0c835210f04825ae2cf7d5231ad39e1b19c9de7c26f94888ba41adb6ddb0f6bf1a6112004a66cd2aed5dcc9407b8554cb3854887c2593d4dccb518e535a4cf5d147e881b37014bbf051c48eb801a516de39817eb7d45c081938ce3984233ebaae2364d70588a0ccaacda16351a00d14a20dee4fbbb397703ba95d670008501e09dd6a55874bac871ab3de6330e9090ae495d1c6d34374eda8ac42cc9af46fc407a34771ea0f8fb25da79946e13026c316cdbeae7f4ba35353d837219e29c9a293db71789ea904f6f008a4dd1da16da44d1a69bd46837a6a4f255f0f4d7ff6e90d5398c474b8d53094be4ad9be382d295ba50fdc192894ffd64777ff2a05cc53ba4c202bf28fbb43223fddb3ca5e7f1270078babfb3adf1280c18a50ad19cda6f75edfa6870a544ac25555f5cffb73da3fd930446325bdcaf9d2bed05a2dcc0ec797ae66c5980976cf3858c512c9415dc40789b25cd049f6140fbae7a6d2ef454f1192f08d5e72ff790aa13f95f916465eae158c6f3941644054750f38743c0ab055512bea0448fc2e83d353c60e0c2aaaad94f1ea52d036c7fb2558dcbfd9468e6953cb3f9790afc7fdd4fabfc22237362118f22b404b9820c26343c0fcd5af4346851fe15422f0a5567d6eb1d9bc377f9360f265596e2956b0ed4b8f48329ea712d6099edd7bb3b557d73ae431b830372141f9374aff9e09c7f01f7863f512115e968a4aecbea7d4f24d0948245f5ca7d0973db02785a614d3fcd22fae50aa3e08909f92dd0f51320b61b6daab45d5a6d9e15c1efb66c1c9e42b1251848c47edc5c48134417eb3181f66fc00f98ecd27a957824c85c997b7a05d6127b2578a5a405a5649d781c2002622b2ae36783b339268cc257e4d6cc1e3f4d550a29c550a5b018068a42cc0f3f54cae4453f3c14e21b1f49383fdfc347bd352324fcfed5ef0551de1dfb1ebb7d90eab58e06f515be702132f63b94bd11f91044dc26d0251e6b321e66acf91f4854a6472bafbc0b66f9351665fecb13029fd83a886882340c2e03a4a446e9ad71a54c00ec884e2801e0f118a14cd244a868b9f0933461f3387cc146fe4f66b96e8c12c3be49461c9eeaa103a77debef71ddb4d8c3a926982e6183800b8d2cadbdaeb5d78e9ce39a15d111db34f4cac2eb660f3c84848569d4be4e65d444a78acc458a6441b4ba6920819b03fa4b65a528cafa507e075440ec4a4804fab704704a6ea60c55922b3842fc1279ce22557a3f5c5d28fb9069543528793a4569315d0dec87676470271ec4f3de69a9f141a394b8f516ccc9e9307aae442b13b62749c65f1b38119dde45a015b94c7a50b879a6123ebddd7d250ada22a11f8d809bb3fa568ae6c12b9e15c2e1f6e390a8fef91f1559fd5863ce525a324aa4e44b4a78952d8cad9687cb14bc0ccabf5f9447f4ea2bf3947e660c7967ab3bbe16143bdb141f6720ed359298842f36f977b8d563a2105d38add32347962dcab756d59572c78271a5d3a211132915898977305937765c668096bff94bacb1f3a27ffa6e254758e2d092f80324bbc828ae321be316a5d69199b948327cc97c83726b83dba39b76f4d693b3ebcaed90d52ab09e50d3db8f1cdca72277f5724e1f5a662b4ef15f7ae15ee1784dd5e90906eca161cd74efe303883992c154ae8c75db11561c77cf943f4b33928ecd3606873a7627685cdf4af6105364154afda72f4d29c7095ee93ddfa4330bf80dfccfbebd67825f99d8ab36808fdbd6455a66c297aab5e67ba71dfe26e527ae6586d44d101e1ac748437711f84fec1ebf908faca100bcf0c56fb03e5cf10086d2afe687557ba737a888445e0276428e7a37aa2edec6b820804740493f4b7fd10a3942ab6a40eafd399628b162fcf19452979f5b283ce488af595bbbd6bbd940c23395276fccca0ccaab3fdad89dd00ba2b4be5b16c468da77b0f5632f83f9b097deb18ca1020918cc491eaba3329cd5bc7aff8f93cb40318f22b61ca5aee4e24883cace5b529d2397cc8ae871964ca7182b79bb72949504244d172073750b09582c960ab8baf487bf7bbf5672635277677a1f7a568349a8552212fd6fa1b875236a8bb55a59da43d1223b28ed530cdc96d9caa72a5f301ebc330b8141cac5b10cf75676b0befc6c798a1ae016c44ad7dcaa7e6de9fbd1aa41052ce4ec72e9e1c9c0d76e016360368c5e40fac13f247f9f8a4eac9e6b9fae0bdb11b1b86735bf20d3415597431c9f7ccf4d45c053c1291439a5b54b74141e1da7cec301284c26da389dd2a22cb7df38013ddec6691ff2c48384f57f1cd3dd9c4c3569b31ca56cd27abd487a519c745b37c0bea8343f45a724e77618ee0e3335262aa1a76027d4cba35a355ac5cd2413f2432a26248252224f68b119e704af325b33ff2faa3464a760c20e6a572c6438ab9e8eaef0d19799f325ff458e760f760e1f1b70e6a7c632ed5f39485805c04db0cf1fc9146c2e591fbd1b631077eeef7f5f9362f8d334e11fabcd77442751ff739050e191851693af0b442a0e28e55440dc66bba4256fb8105ff2c648657f0f9333d93e481b42ebca21ed30efa80a6724fa9e875f3995a2af12fc2";

/// Sign a valid UFT-8 message which can be `hex` and passed either via `stdin` or as an argument.
fn sign(msg: &str, hex: bool, stdin: bool) -> String {
	sign_raw(msg.as_bytes(), hex, stdin)
}

/// Sign a raw message which can be `hex` and passed either via `stdin` or as an argument.
fn sign_raw(msg: &[u8], hex: bool, stdin: bool) -> String {
	let mut args = vec!["sign", "--suri", SEED];
	if !stdin {
		args.push("--message");
		args.push(std::str::from_utf8(msg).expect("Can only pass valid UTF-8 as arg"));
	}
	if hex {
		args.push("--hex");
	}
	let cmd = SignCmd::parse_from(&args);
	let res = cmd.sign(|| msg);
	res.expect("signature")
}

/// Verify a valid UFT-8 message which can be `hex` and passed either via `stdin` or as an argument.
fn verify(msg: &str, hex: bool, stdin: bool, who: &str, sig: &str) -> bool {
	verify_raw(msg.as_bytes(), hex, stdin, who, sig)
}

/// Verify a raw message which can be `hex` and passed either via `stdin` or as an argument.
fn verify_raw(msg: &[u8], hex: bool, stdin: bool, who: &str, sig: &str) -> bool {
	let mut args = vec!["verify", sig, who];
	if !stdin {
		args.push("--message");
		args.push(std::str::from_utf8(msg).expect("Can only pass valid UTF-8 as arg"));
	}
	if hex {
		args.push("--hex");
	}
	let cmd = VerifyCmd::parse_from(&args);
	let res = cmd.verify(|| msg);
	res.is_ok()
}

/// Test that sig/verify works with UTF-8 bytes passed as arg.
#[test]
fn sig_verify_arg_utf8_work() {
	let sig = sign("Something", false, false);

	assert!(verify("Something", false, false, ALICE, &sig));
	assert!(!verify("Something", false, false, BOB, &sig));

	assert!(!verify("Wrong", false, false, ALICE, &sig));
	assert!(!verify("Not hex", true, false, ALICE, &sig));
	assert!(!verify("0x1234", true, false, ALICE, &sig));
	assert!(!verify("Wrong", false, false, BOB, &sig));
	assert!(!verify("Not hex", true, false, BOB, &sig));
	assert!(!verify("0x1234", true, false, BOB, &sig));
}

/// Test that sig/verify works with UTF-8 bytes passed via stdin.
#[test]
fn sig_verify_stdin_utf8_work() {
	let sig = sign("Something", false, true);

	assert!(verify("Something", false, true, ALICE, &sig));
	assert!(!verify("Something", false, true, BOB, &sig));

	assert!(!verify("Wrong", false, true, ALICE, &sig));
	assert!(!verify("Not hex", true, true, ALICE, &sig));
	assert!(!verify("0x1234", true, true, ALICE, &sig));
	assert!(!verify("Wrong", false, true, BOB, &sig));
	assert!(!verify("Not hex", true, true, BOB, &sig));
	assert!(!verify("0x1234", true, true, BOB, &sig));
}

/// Test that sig/verify works with hex bytes passed as arg.
#[test]
fn sig_verify_arg_hex_work() {
	let sig = sign("0xaabbcc", true, false);

	assert!(verify("0xaabbcc", true, false, ALICE, &sig));
	assert!(verify("aabBcc", true, false, ALICE, &sig));
	assert!(verify("0xaAbbCC", true, false, ALICE, &sig));
	assert!(!verify("0xaabbcc", true, false, BOB, &sig));

	assert!(!verify("0xaabbcc", false, false, ALICE, &sig));
}

/// Test that sig/verify works with hex bytes passed via stdin.
#[test]
fn sig_verify_stdin_hex_work() {
	let sig = sign("0xaabbcc", true, true);

	assert!(verify("0xaabbcc", true, true, ALICE, &sig));
	assert!(verify("aabBcc", true, true, ALICE, &sig));
	assert!(verify("0xaAbbCC", true, true, ALICE, &sig));
	assert!(!verify("0xaabbcc", true, true, BOB, &sig));

	assert!(!verify("0xaabbcc", false, true, ALICE, &sig));
}

/// Test that sig/verify works with random bytes.
#[test]
fn sig_verify_stdin_non_utf8_work() {
	use rand::RngCore;
	let mut rng = rand::thread_rng();

	for _ in 0..100 {
		let mut raw = [0u8; 32];
		rng.fill_bytes(&mut raw);
		let sig = sign_raw(&raw, false, true);

		assert!(verify_raw(&raw, false, true, ALICE, &sig));
		assert!(!verify_raw(&raw, false, true, BOB, &sig));
	}
}

/// Test that sig/verify works with invalid UTF-8 bytes.
#[test]
fn sig_verify_stdin_invalid_utf8_work() {
	let raw = vec![192u8, 193];
	assert!(String::from_utf8(raw.clone()).is_err(), "Must be invalid UTF-8");

	let sig = sign_raw(&raw, false, true);

	assert!(verify_raw(&raw, false, true, ALICE, &sig));
	assert!(!verify_raw(&raw, false, true, BOB, &sig));
}
