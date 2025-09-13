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

const SEED: &str = "0x6f528ab09e2d8345aae8e21215b8569334f8339e1226267c6d05fd97b7698823";
const ALICE: &str = "1865ccdda3d7efff3344c81803fb5238d83580153a34b065f1cfc637382193892d89dd59568dcad6fd08022badfde9d88ac90127fdbc02e030f343b3b003531b6b1b674c301713c2206875bd522a49ce54aaca4b8aee26c3ce6ef1360a2e825c292adaee0885020ed92f97bc423b913a0272c4f87179bc13964dd3adcc1c5b06c3b6563ce7ff31dc362ce69ab7a8e9efab863437768931f1b0bdd4453922f740f4166a90c308353665ded9e29096c5325860c6bff0f916a1cbd50261605c7a7bd5e0591755a8c6a757f8dabc505c912d1786dc45ec7b56c1273b63f66ee35f3e405487ceee3eb69440f11955f30ec0b9ad953467f93d4e87facc7846a9d96931d3bb9ed406effe10967c64d2305e6c159c69d026a3f12d9170adf88b7e8b4b6f51acd7177b20f6be3a1de021d08384b756bcb008aa0df7c5a8ef4cb75234097c955e63299eb51554f0a1a095ec8ea377d90098da03c43dcdd670fcbadf6727611963ad8e531d193be2ac37c5aea42b298d27496799c7246b064936432912c805484a7d82b5071b902f72a9bfd39885e0870ae436216d42d0a0a7f79464f15e49d6a201b6eb5f11930a0d707f4e3c9f5dfcfb62974348a9d9216cf9fada99c36b8a5329fe959ac859dc343bc3e8f75c1346e7559e4115a097f8e390099f72ae465cf65b843df64e8f7b1c8537b0bed711c612b424096b48d1c44a82a650c7913ae0870c3adf14c596bcc388775760fe350756f439c2ba821a6365789353e75eeef9e4879960ee5c1dae7f57e51980d8158ed00d56562f3fe9ace377479b2908d907af349d1f6743dd97b89a3d2a3a7f373cdafde7e0eec51c46dfedd87cf4e1d3a24efa15a8dbee39df51a30e282d436965cbe5cd765307ae40eb1ce1ad184d4713c7b78f8c03410f63246bba56df351cb7b7cac0ea979c8b093306b5d2c5f8340fdc4de5ba4e88d32bc70225842c20555d6381852b4c0a9604be9ea35dd2a78e7f03a10a176d950aa3c8088e51d0925478f010b6bed13de033b136cb0e4f9be5cf44751741e18a9cc96eba26e1ba3831ac32c59a7f173245906ef273cf8eb17c5e47a608191620cc8fa3826ada217682c501bf158d02e8e30f8a46cfa21b8ef642a77f3d1a491f2b5f5d41d91fcaf794aa9ee09d490e9a8f938ee548e151f0be20587c7c9aef5d2b6e4a5f9ae9563bd6e3c24380f9e098af98756707c682053e957b244234750d13917ca25c3015ae04297562a4fdc1c4bfc9ed234e82c000b1123e22025f93a7cd27ff7b70445e45c3a44eef26e13919f4ec113f5cdda6162554f5ee386a644b41e6d9717707c6221c42d5412e78c0ee8946fbe5cfcff58b8fd0121d22e9fc8ed2c85c176df9dcc943d30adec6425bbc9ad948d83ada1f5c0a769ea3f07d0488662c3b209ca4bafd29257f4637b2736628407a9c4054b2185fb565c990be697c77b2187b86039fced69f07defd34d445cf5d941cb74e3d64b64892e45806233fcfdfb07291428da1bd991b2da9adfd4932d05dfdf726b5849eb8d9a4d1af29e61aa80d9c61aa92d417d4bebc69a35a4bf0c3b77d741ac5931b1a0fe863998a3943f30ad8c22da81e4129f365eb1c2a36ef83f75f5bdc54076986144df92259e0d84b900deec87269dff8732f72cd42358a51e1bbe4ef0cc5f60794ef12b0a9158f55ccb427a816e78b64dbb0c7d62335077e1a9de0e8d480a143ee1095a2aa6b0980f3a795c05a3d2b041728fcf29221a2e2a774c9213b689e3f3f725c16ca631f99de49e999eca7e461b9e1aee5ebc4bdb36bd4a4256a18126671f9b2ab052c4e0db3d91a4962e594c9fa51531b068451f69b663b62e0e9ac5878a53a2cff55bef9811c2e5f4e7b6c5f0ec8da60c50dbae3dd0366306f31a7ac88650be5023d70103004d31ee9fdb7e995a9db87bee73710f3053095d422555747befd541622faff2d00ecd7f241b527c1d8fc8f8ac3842332ea71f76fe23d9f41cf91c1c1dd7f99d10901a7a701c997859deff88f53e076f786dc0fb90dc57045535e343153f4eaa9f241e0bf3046a21efa7e8886a59a22a157c7a31de5d6240c008aac113bec9aa26b6779696b894f0c03ffd53e2fc6287a79d3f09f11f5f01e1739a75adecbe13a870a6e7f93a3a883a16ff8635ea58ff7720c359ed268b851993560856b98b1f56ba8cfa2a72541c22a3ca8659ed70203bb18bf1a45d3947e1e7b86dd96aa96b8951d547e30b65a062742c15f680e294f827b959f5d8072d553ef252c9e89dc088cbb32981b05db2ec3cc0072758d93576481e94ff44d2db3e75de7160e9afda76481d7a05e9ea112354f29e55e11b4652b939ee4affee907bdbea71b62ae5c6b2086338227069533352731a315352ff436dee31828a304bd3decf0c861e2635618fe3a1fab52964b33b40300070aff4bf58036739193b8115b2323e04bd0bf663a342d21e036b1897997ae84743516d7df5f3a838f896045f8c7c1843e37b5f11e54dbd6bc992f3c0b899788c1fe489b052d1f45a7782eea6feb5e368ebdefc9e86624ad4ce891a4fcb919b8dec1d73222de7f1446b5f2c53e3e631730f25a1ffc38af0dd79d46061e61fb040c79c34e3cd7f0a037159d61117c4f077918dea28633fe5d21ebeda6a58b3af9d75c072d537f823f2990defd7371dbbec9402c9b4040dd4cf65397d27c0942838f749eea45879b4c4420d3a6b249b65343a14e4968b3f32cbad5e0773b3388736dc9dd70a274ef0e5267e640359f9fa81772a6322fc635aa01d3f7d3acf0a8c52fe776c4ae991efab1db2ea11f1fd851e4fd829da9b5894cf7030da1c360058f22d9887f0b843398533fdd0bfcec88aa3cf832e4211d040f7a3190dbc808cae00a55176465e518f095b995b9eef292725476da02fb009aaff5aec786a3be9f7193bd5635476d5d7828e7e243882f4d6a70976e19ac253be555f10ec37e0a814f56173d84d915930b08cfb87da89993ae857b0d3a98a3eb0bdc1cdff160b4a48f42f3bd389e1efa49e4d463923e811c3bd32cdbf8d1189a31674e8e4d53037f9de06cabdd628abed3c0470c844cf4b258c9e2effe5887c496183e7cf2d2468aa255632e26d1cef533f1c3ed013b0e1ff7e47b18d2a5c69645926311fe8f1bd0a8e519eda85b299cfccbf2ae7a7dff32c0e60d7997acc92475f8bcbb48c0fca510f80c4ce23fc9f47a527540af22d1e9003f8a51b071cece505b40890b02a43a4758a348a00589e322d2921c42c15970599968493361729ba0909e293b4307b39290cd393b91270c7559b46824ccdd550f9c73025ebf1b9eaced1d0d5a76ed76573c422b3083d61dcb4720d8d2998137e1ba57b8995ddb75a17903db0f34493a3c1f5826ccb3e5c4256329662829129c806cb07a1c3b406e7a2a87425b3957d987233a59c3f5b5e30c563cd41aeaf67830cf36a3a0bbeffcf245342f482ee85d0982dfb7260f8a33e8fdc2c2f6c3e8835111b8489d03b70e7567b862848c8d4253d990ce759e75145aae62c401b20fb78ce5b9de9a7506a9de1330c0ddfcd056883d0a35f5a452cdec04ba55c8803e2447c98ddca9f99aa81255a666bf583f350849bcd5786fd7201ff17a5e105986f2d5811b53d2ddb6b0e1534c3e6ef57b89fb8e3437f2b8a5ee8a5c54";
const BOB: &str = "17a6878f7a9289c84ccc8a0928e6df317a91fd41d8777fdc51f3de139857620453f744ce2ef599b847b9a71b45742ab82a781a3651dbaf8db2af4756cffe4c5acb45855321732720702074897e02027294546c2c454cf052980a542a0231ebee7c26a137bca0da6b9636ba56e6e743f37aac1cbdeb4533bd7f90f553565d8ef4af743f2a668aa5c83c424cf69253442cc5e4083d405e05944d79fa1ae5c1a1b84273675fd36118c34de91d5d03908ecdfd2797f23bf14a21a1be67b90296df3362bafa3a2f82fc855c25a5ad8cd0d5144b7b7cb8ffd33676498249d7c21331f6ace26eb5a164716f19e882b9a2397079143d7783654c8c40daa986f9232ae15f5cb0569febfa3355260f9e2e01d0790f9b96b8a2de07b332b88c497526461e1924df51af5125b9926c746b43ebb4a7e9a53e9788ad775733ee1050d9255ae862c082b0477dffe7cc1611e916f9c7174b12de778b017b79a1284dfb09b40c55ce9052baec4618f3bb77eb4d1fbdd56638054aaa3976f9c3d7e3788661013c1dfd3e80020bc44572ee75136ca55794a0eec4d5072703e43667a40d604c17e8674f911505e92a13e4895b044fa8d275110be748c9348cfd8e69ac01d183797d1fd64ed6fea2b932f554739306a93c44ce3eacc0c2a9b9270e5771607e1c38808fa8426763c3e720ed4a55760d26c439352ad1441b042ae3627167748dc8aab3e10e37265671592b248616d5cf1b2debfe5ea4f720f537557ba00e57311236d7edad7c31427f124e0e175e4e402263795b112e41fa83f9d3937f6a9048956447ef5666f2c2801f02ac55e6925d1fe14a732ca241aaaac54f638e33f9d7b7f456560f3625b739893633b3fc872e2cd239d14234e850fd4c932df84e0c5573abc7638eca1549d9cf5d92f2b98c0c411bf04e06855a49cb0f04952f0330fea812b448bbe9e94b81f8eb5a4e7e5a10ef01acf0b54cf7d58a2a3d258ed78fff014a3e19ccb232e27fcbe2f5c811ec9c5e0188146b55f518487b877dc6804bde55283bb3d4b4c6a584a8e552173050fb65ea544fd29236badc8e8a9e3c0594a2cf17a30eaf6223bc58fdc4447a8946d5e696b71dd95a251adaadd208b11f96f7026f2514603ff4d4e12a1dbc648c04eeed1a15a49362c52aea42311492442652fc42b3c569c0812fe43d0b8f0e4955a857a3544e7f8811f5cb2e99870b596af08c7d83506144ecc7c0dc52b55952963f4ada14f271969c4c90044e8e13fbb077862d20c3e7347362badf4570531fec2426e00baf6849386389fb2ca9649712564a35445e700d49966d4cecffd42791dfb95a9afbd7b6da407382e47ecbe4eb9b232ab5e47517a9c05d691a9bb9e382fcd4109b66ae52dbc056144b2741c66f0037694bbd0e9cc7b2756e269f3afc3ec66899aa326b4a5f662f721a81b095be785985bcda3bfbce6a22972680d0a213d7c3f86f3799171aafd033e46d2741d29a6d934da5dab41cac2cc36817a0d0e15060677e8377a30943909208340b60c248fdb99d9911c98e75b76394185b62bce4189ab3128bc6ce94b264dde1ac760575de1fea29e50bd5ce476317e09fdf2c25c9accbeb8d2e07f2913a80f471f7443391aefd317e81f81487e91df441600c0cf6aee95c74cb7ce9958b712672c4f54f858edaaa4f97fffbc53521cb83270e3c3da1f710ae455bf02e92830c52c53c0012def20cfe4c6b9c6adc307b2e3feead3654e855c79e7584acbfdc1315edd80eb225b3b40ac01040ca622f71f50be9b87caef3979e2f495ed2b0560cfe3ba4234c4a97f5d4007176e5203c44a39257953584a2ffc109fcf07639abc764d4d2a27ebb469893adbd977c58d07db62a050ac2afeafc00f6241356aa031f96784e1ba401582bd8b64562856a7b6ac471d91c26e7a92d1c50b4f4ab49c0814490319bcd7e175300ae49449bf767f144c60f6f32ac7a420c87de28ddfa728a3a423c84de37b8297bcecf71880d2642baf0e7a365c8dbef1604619e39d50c674ff25ed2105cf4702a5da7776707071d78961768c85008cc245ffb4626db43a55a1d47c1d03ae53bb7e7a3e45368b89fafdbb37f4236543142a5838f7d9e512fa12295ab74c3e5e53855b6ad333117031b7bd0b0cdc2fb973e845dfc00275aaff8c8f845a627efd96834c8b14dd068833d65e7f28097e0e84849a061783cf43a8ea62b288b1098f77fa1797286e290020f289478fd9d44e8da8a6a7825865a2e0ae042f2452eebcef8c42c606397e144edbd40362f6dc0c4e2a68e0fd0138e24c6e327b8399af3c8c17b7490911d93d3ef60dac708be8a92022a541d75aa489733e200a159971f54b8af5c7a06a89197c4b060500df5c7d05e7fd6c69a05aa12959559677a970f6b39fe55a15bd4969ff0c556e3f30e112b241c361ab2f792ea72ea3b0f2f35039471af0d45a47d3b1a2f2ba218f4c7e0b75f56f123c55ba755fc645eeb3d2d11cdd7550f366dcad5e9e72dbaae820363c0f2aa4b7ff9c41d8cf0ff013fb0966a2ef6b84f2902b90437603687f964040e1ba8d5ee8e67a0e44e142ce44118d630f2252e74fd174afb9bafdfc03b1625e74ed0702b79c6f2407941c4acf84e5f53d32b684082f0d36d38cf82ba552f09979f18b69927c7afdf49f15ca5b0f3a1584f1ada1d44318342d76ef60ea54c56c290a4c5e88335db03ca6cef379954e122e236cfdf3c005b29fb06711b3274acbdd69f743eb0aa0e96e7b4f6489844446893fd5b951a84c8faf5ca949055bb1c2229d453fcc8a8706d4db519b16429de141e562d9fffb14fd5fe093d7bd8333bf1d2591229f12d048989c8be2be386233f005f3fd27518acd44d8ca89e74b8b0aec38a33eaf12fadca6e13bb409fa21ec2c66f59ece2e71c9da7f46d24097864d4ff2966fb5f9edc1fee4bf3d63441395e8f78248cf0474444945a31d85942fedb8d83207bf85cc21db26871346cc3ebf68ac7a30b9f57b3e30212fb5a537a7ce27ff544b8636f9b16a7c26899b9b594262e78c266db49ea323d671048dfe6b65638402ecb2052a6f145865cf2448feee3a6848af0c2f6cce7641a1f2fb1aca1139e0fcc0176a44dc3cb4059759952d92f28d8a5eb0e3af4216015ec68adae788589e734b8a6ec3090ecd2fed88d0fc0f325e0815fb3ba77c24e29ab2fd2458112f8780e460128200b3e00c175722c4e39ce706322415be1206d3aec8ef5cc7ab56239550c1766af535dd68dbe07b0750ea52e304b77ddec9a030b4c7ea13dced5b22ac89defd8345b3a055cb8a6a5aefb15bdce79e8eaf416b3fecce556962fd137c9607b573d562f23526563c0d45e8b2e707a2ac3b79742b16920cf7cdf7f2c9cedc9c0e1f04b014b4a6296c08a4882c9017e7c24636fd549a58a5642fc85bbe38b7ab81d4a769bbaa79d59e6b3ec8435cad22d0225d5746d08eb666ecd58c84355a2b160c6936b3d59444097d853efd4a3c4821280b641c81409f3f544f38d68f0456c7a2c8e37fc5e2c8919ed149f9a0c522afda961e0feebcfba1fe7fce2a5f5a3bb2118489f938d0a9bc41c14b99e4ac1ad766a425845d6d67612066cdd59b1f10311347b63d648ac105cf45b1711259c313463a98bcec91abc20a82e97a1df17b2385d65338ac0205ed7947fa9a007";

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
