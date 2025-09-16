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
const ALICE: &str = "0xaa61beab56ee598d1f0170ceb774ff8d17be46ae44bb081986a6ce90cbe996ec5b8e296aff8be27261cde504b98bf6c6a79f1dbdd94bd8fe9cb36e01c32a79e2a4d1ae407d431674b45ac2cad5306fc8897fae118a7281b416d762dbe9760f277e8589b158c972623df738f363bd127cf6582fac3ff6ce5960930fbab9ca167bd5cd0c5b528065c96e58fb14e6d3230a7067987781126565537225d4c537703c1dd879adb943e51069a3f2a15a0fdd150de57e16045b2134fe6d48c998aa1ee533185ab444230a47599bdf42b1f8d73bacf3adb158fbf966f5f64f31984c72e4153244116208ba69ca737e88c9f8bea2043ce465be7c73637045e945cae98f2bede855525a179a6392709a03e9c1df6c02441650d240fad8b3ae6d178e32fa3a9d7af84b1283d3d16ce45491c1a5ff9a582c51ba2e6e93c4a12f573ee4fa07feed393312c996b5a84dd980a959574f65cd4e028c161c3badc9512963d99caeed49ab74a0fc3d1de09be84558f6720a995b612bf28dd905c7e332fcdbe355169c60574ce3721d9d0b2960055bfbd5efdaf5a1df4aec675db2652718c259827431d6b24ea6284b17e3acc2e6b6da12672fce54353e8c2a53c671e23a57efb71056e022f8e4b749d7e67d8b172464a1bd9a1fd6a2333763b3ffb636f75ec57efd00917fe5f9dac8ed6cbe0865bc9f03b108f5a0a48e13cd0bfbf53eebd137eaf00ec7a4a04548c6c54eea1bf62f10d206bb55b54a107dcda34a28d49506b74c687e895e4f5957becbdee320c94e934e682ef5a53086f60e0a71978282e7b473d3bf7b564d1faed66e18c64f38ce610fd16495a834aca0bf5f5266c6a8c3e2cf895b7cbf3cf7bfd61be2d06dffc32d7b27846c974dd5f0c655f0c29326acb5936b6b50f51049e054d449c3e7586425d3aa770f7f064d84f1f2a32803746cf5b1063e8599b95a546f8b9482810a895326b921fd7b1a15a82227a777ee0bd3d339cfb904772d52fd086625643519e0887383092a21820357cb1fa5b362f2a3100f76222e3a8b857c425048dd3327f8f13e505f161e315fd4dd4a0ead74326780ff016cfc5317f6eccd630eeef3564ed79dd91ae4dd1fc9814cda400c19a0e1ccb73b960ba0ef837d91e98c255595522f6c86cd5bb4fbf15585f17d100500cd82145a35bc14af8adb98f3f715640b1c6ad7c2536548465b4963e71730e4e9bb5381e4d7703e6935435c2ccf40b81326dc579a829886d518c036d73cee7f316de8e71090ea5d44e5c78600c36e25e007626274cb96055976400f38b8eb69bf92f397c4fc9794a5f82e6a0dc73ba7a50f69db8dcd0d8b6614f2c63a4be27a3507cfe06d18f7dbd4c9176df7e963c7a85cbcf94c6f02b1745bb5de6e0b876337bdf58b2a6d05245667450ff6585e8ac54b7b301a94d9480db6ba552aa936917aa6543cf18e34ecd7552418b727c4f39a348724045c569c9d2e66331fc7fd7a7b50f66982f1807f56dd1327187d3ba47ff0106abdb2cbde56413086952e9a6926d18426138be7d84b0d882663a839865db67945fe6335958e8a84f803173b91c4d2fc0f6b9cb8e4a2d8871ad581a606471194db8b0a23f41ec5d17969f382ff9d80f846c5fb662f3339dc470d67525f7efb6da45bcc5ffb004f5d75e31548f03e690f3ebfe6c74830ff845fc75cab8f4c03aee528a788f567ac23fa913139dd6c4a88ef9ddc6af798ca87e1f47f0c26f278116a0188241bc6deaea9b7d9b22a4800a807a34c46a5a03a163f1006c8792bd67a3de31559ec76964bd5e427763785bf02ac0d0bd95df45537aa670a4c7e2fd934e4f41ab612af5a6c9ca127eb99fed19268a6e7cdd01d14843dab3a00a7f79c292ad5566ef3549f07f79d358f837419f077cfff4bee124da41a88b8a18d1dcf225fe3d946ca51652496e6268e7c28c7cdd23d698e88b32214d41dd069e2c1a99335a7771b17851316d3778fe7615038ab9e7c5c5521aa8bf0dc8638f45f40360de6e04685924de94b5181db8235411648ccd7a191692e813d07e9a7b16425bdb33091efd41c315fb9fcdd45e96d25df04e9eb5adad61c859053e8e3bf1b3288b9221f69d1bb4427277a0e3b98f9b3f4adb592c2bdb723add6a42a17c92f8f9014a49742337f4ea12ce63bd511af107a812993faeaec8944d248b2d90183c5a8e2e34ef957290832d11c47a6b71e930a6814430724ac48f31a1c879e7428bae01ef88fecf55e2d4ba1c2c20a2b33479cd2655390003af819c08484e8a7e24dbc78b41d0ef150f6d6f002edb6feea017e7d76b850f741a8a8844282efbfe40c9e40e1ce0ed82a420be18b50cfdc9115d886bfcd67a8a0d5b59fb76a40a3489fc66f682c297047e8cf65897f93b1c382274b5ac5192a17179620faedd0257595d58ac2e4e96d179aa3e5589941282f6ae8d0f4ee460caabadf742c35b35021d75e8414a245f7cd896ed657280d162f3dac46b7559f650038b414f6466b8ebf378482270bc718d772a869538fec271cc9520da2d2eb5239287581b24b83b9a2cffaa6862a2dae35733408d88369a47d8c005e2898235162463f80fd50c66f2d47fb2ec1845db890b725a71177fb726b73bd53d07b7d5a055fcc3583bbb735b3d2f523f926588a417d9eadffbc113b26467d86389431ad50f65026bb3ece92d067451549ab2c92b02d1313e34ea19eb4e21839f30345e32fbb3e2f75ebd853bb5569312864cecf8535795e6601ef2d5f737ddb2db1087e827f85c848ce369b49137d8b5e3da8ef6160efc99cb80dec0a1cb72c8c58e233356de8fee0cc7809160be6c61230e2483b5ded750136cc07f4876cb2dd907aaa40b6e12f606695af0928d5f70a0b0371ba8e2a6bdc72fe65d57b96933372e8b6df165c7ce37dc73c08dc9b8f2d1ee2a731d51a0285dbc214162f5df73c17a89361bb31fd4ffc5ffce330534635204c5e0c620ce31cf68bfe08eadf58d85d1ffc988c813b1085c2a9d99939e610d1e35910ab7314db8afaa99dab94f3cff6033686e2c0bc74bd0c9c5a1fd31dea8bb01f0eefb8f97f995a36032efaa0819a387b6133d5ddce4b5c2fb0bdefebe9b194f28d515ea935e194038c7cc8168c707e9e43d6c731066658fa3c6b89be9597d67f71395fb65ffd2b75e241279c9b65a908c91e1708ca526005ed08e3158f685600b153a8c8c62045d8f729c4a9094c2704dc2859e0e160f52976e6cf4f54d9a8ac897bc2ef784938a740b442abb5e06fd6fca64a928cd0ff6afbab8a743d2be0a9b216fe875b050c1d9217ee399d001f61aa15b836231ba2f176206df1796045571b4a7399091dff95c197b1c0a98fa70b18cbc18b3be428eb15b0605cdc4923f594c598118cdd303638d929b8d775175e3e202bb39aa7855e9dad64fa739a4c518a02ce8d46774b0af175d65d44eba4f7d1b96a9b8e53c05ef476f5ef990209ff6b46ac0c2de454d9345a9763cbc7fb32a0a52cde71a304530824384e73bd77a86d35231f241b1e1956314e6a53ca3f5173423f3032f667ea563bbbc1bc35ec99f27db5ed4359a96f58f66b764628d9e3b69a5bbe35f4ccfdd10baccac104ef7555e2c72d038d45a9f0c00fa05d8cf2d02d0af3bbf2bf2b9c452f215a5c1a4a3256bebd9fb3c5c87c66ebd448a2749f";
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
	let res = cmd.verify(|| msg);
	println!("Verified {:?}", res);
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
