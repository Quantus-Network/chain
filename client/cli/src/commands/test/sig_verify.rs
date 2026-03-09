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
const ALICE: &str = "0x9afdd50af3f00e454f85987a107560c91a8f6c7b40d193c3946a217d94b6dfb8fafad1edd7e9482367797099f24fe144ca139e0416e1f870e95fea0d0d015b43c30ff982e84bacce4cd1ad5ecf8d65c4087dff130545ceb236e7fb5c553a5c7b01c6175c9490cfbefde19cf997506843a5bb5f8e6b8d2d5d9c19731baa91a85efb6ebb26d2acb0129911d5e001c543aeea2ba6b770e4931bfc4b1d269c07f591a549ec5727e59fc6f30097c10adafbc1d0046d070fd75ca7b7c81a5600671cfb5e8b367330d819260ea9befb362296260cc4f5c42f07ee655a539fdc549a5ab17d230f78a9e56c1006a2c5b4b37bb1e40278d15c4f5e257fdf3d3b24003549e56d35544dd862af0537c16edf9902ec89ec4c30f085ed243baffa52eb5e12a449db53ca0acc1305d795c9180b79adde592389fbd0846ece7f32e7eb2c274f565a97b34e422367d80550c1951d73761af9dd0f953a3f8495347eb5408825b2dce7657c7d6c801837e1f481132075d39c19296b8e377fccefe5c1a392201dc6a631bfef9e3c55a1982088d9589101265857c645c6ce606728de08c7de17918f5e79db957e54ecbcbace987c10df9d49b4d2f4520dda583138371a3090aafcb75030022ba46a9494ed6c4abcdeecde762edc4644643aa16d24883a11dea2b8e2917534528f5dfbcf2528e09050684660db0606fe86cdf69e6baae1be3afcbe84ed08cda8889b8d811897f26e32054bcc9c96f983e82f175e1f7358842d93b21c1bc1f8bbe8f000ea87ca8be121d71bb1c55df1e9be1a535e8197b201fa096dfe4aae6d7bccd7fe8d5591621af4f8010526d64b2aafa7c6cc996e752b2232550740d37bab47ce307a392f7e9676d900abc102fde9c52624ff6ab81747f0f9109110c18bf92a40e651e81a3ac5433d83ff64bb90a1641325785bb3f7a7006e8604dd9dc6267341a481c6ce35077e12cf911bbb1c478436f7b0aae1a491058413b721a26bc63a40614cc7f4404dd15457242e5d86638af98a6dcd2f2eae8197dbea852b882883b81c01b0a33e56ba92878116c76417fe18f62fc97977bb2c942cc4021af20039675f1ed162c0faee8d68a77f338b6d5c7b573b1ed53810feb84128a127325d8388d458541fdd68c8ab03ab01e712a7615e1e52362ebbce82cbe7309a26bb5ceec5166140338ae0930d646bd76f1ade341e0b822433d4a36ca8fb39fab711684b7de6e6f85a07218f363633980ddd01ad6ae496f8ef6b221e45a830765fff341bd528bc7fb64246b575aa3dc6537aab9c695bfa37f93db12024688d18e370e39e957aba3dc071d2e70e117c20335d8969cea035caeea784ff2ae5d2253d1ed894e404c68b43d50f5a0f3d7f37d7960183386b6d4e234d005fc1a874f894874c2f3cc4f593207520e30701fc358b8b7edf8b9b97cc9730fa77ee5e98d97621809cc4de6bf125498d8c991157bb1183852da66336f24971dcd341f77c3785d4e8b50ca5d3eda0c655e51a986ac30ec06d3a2f0a1ca00e17e2534416495f4578f43db845db86e72a81effbf3ad7531b75923a1908a2203064f8f727c00085f2453c774ca5f541effe89638f9e9dbfa9f55512fe717136bbe699be9ff6710498d33322f51e3ac76154f16cebb01ce42e872170bf36d09c1279d33da971b919b6e58a9bcae60b2333c3378926d95d6afb0a864091ddf706cdc0dc150108bffea2b483776c49ff56a797a4e46f304f0af49d595d5117b6a2f276c1ae20a46ec573e96ff573d68618664765a8f72cf5a1e2dd2b24587d101e4696e4689268fb18a3185b631f3e8ce72f85b9d86f55d38322ad22ddbc35f263c3cc17501e4f9b0e895247e2f34523572b48cc0f1acf4c0d3aba9f787f46dc5e7489ebb8066709683a916ad9c1d26c6fa36cf0be0157b796d33fc476bc88dcec9e7e23e5a6bb17c3fdf881d4eff66ae4f32985fe59d9609687826e2d82096ea49a085c1ce69ecfec07f9fe7dc3fb5cb9ecf98a9148789b10d1bcf29ff0e22fb439646d707ef93da33e9245c990dfd8831dbf77f235bc0380f19003503cb770e8c7f0279fc592ce582241c864420a9811cb2c6395f1643be47a480cb2c82bb2f6b965a471f18717c7051d636e4f91577c8e0e2b48da9c20eb486575375432fee7fa9561e693663aa42b97426d4211fa47b857a66b59581a3d7b2461d95e24236e10c6feacb6aa52fe94a8b7e91e303d34842432bbd5b403b0be81fdb6ed2f60a74424d815d2dea8883c92f51830040531db3d63da0e3227823c866873ac33827699d0b00c7f190eb4e22ac4382dfbd8789b711adda913d2fe7ee2b495bfccfc44efbbfcbdefc97f8ff5ee5600c0ee010e65cdd9ecd2b81eb20d11ba4ee7dbb77e439e9872836ad89e525b35d9ad3be414091a49fe8ab174c46caf5b9a5c6deadbef9986462eecbecde3990d5bd74270776aed6eda6903ac76a15dcad927426cf6fc5c88ecb4e4cb316b3c22aaf3912dad5a8e600d489c5f08cb97f8f4b5edb14f3d02567e659059fe2652af506ac0f92e43bd237fe813df47488823be44e08e32509f5091c6c06c3f681820261d6c0daaa9de69cda492a6d9fc3ef57d42a30cbfee376e127c9625045022c4469868c0cc1cdc4bbf7aebca20db4860efe71abb3d25181763e02d0813b3d8acde40394d201769298382e8b25f95a3e9592ffb6245501144f6bb61bc0d054f83e66a46ff0448d9abe9efe79aad672a5518770589a575023cbb3da05fba5370720b82243c59424b89784fc71b940574db90ec0e447e6f58732714c3cba5f921540b327da4477a0f01639cc0bac64e9c02f67a23a778c4f8976d6364b8d4b18d71764af9fcd435fc8a0229adb4bebaa44c9de853975d5b77252b5df74bf505decc480357baa51977bc3d037a2183eb45cb93c55eff5efc09a2d2e500f7183e9125f2294024487e67579e45dbdb404c1e1c5b4466e3b55b5c5230328959d517b9470199f0f4eb451fabfb0294c89c0efdcda321c1f44991e7c6747110ef9c5ecee903a2b081b3f973b47e20c8d7cac4b86ba7ac76efd315d4ce1e28233b4b201bef09f8c809d0514b1d2233c9f217e21996f61b58f9e76194b3584271ee6d6fc9c85748480d9be42b3e540fc804113e97d3398a6ac98523126c0543bb4cbc200647143480d3d90271b100cc6459d77cf3b82ceba979b11c7bf0882fa9db477406628f5ec28f1c698d42279c7217ffaba8c131a881104cdd81d3ad1c58b5a8edc709e4c69e6d970062aa0250ddb5e4d6dbcdb75c7ff9570c2321647caeb524198076dceb8bcec11945553140d314ea2476dd414cf5a1c65f2a3b3d868006c3c12a0b21cb7129eb04b8068aa23f19314ccbfcb1e3ea780269ac172a9c8abfa80f64b6029027bbf4e86d57dd5c20bcf42e663c10412b560450e289da5e565b3e970c3117625564dcf774e1727f20e5b7ccf5f6894af915af69dc2dd246a2ecb98965da3d6b2296b42f98c1fc285891cd5c094f32b91faa6165fb9babfda671c3a67e58037b324379343310dbf8da7cb727078448aeb134a8d2ec86b770799e9c552facb7d1ca85e3b8b3ce175841947cdde5c5d59079d3f6a308a986bc8aa16a9c9118701125ea4874ded10596f2c448554e505d4fec9cb1e77d21cb12396b434";
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
	cmd.sign(|| msg).expect("Static data is good; Must sign; qed")
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
	cmd.verify(|| msg).is_ok()
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
