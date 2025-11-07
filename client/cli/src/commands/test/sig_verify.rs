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
const ALICE: &str = "0x9d8e9dda32c7737886973f0958984250c28dce0277c14e9530ab79fbbc0991ed822a8abf138f685f8e76dc5157c4c0fa9b8f2cb8050725ff223c5fbb641e0e196a2b2b22247f91c7c26657ebde1829f8a938b7cac16f6e2b2f1076e2a4a01ea6a8fc9464ecae653d80002905d6019afa14e2a7a12aebf805c7a541120fcb55aac301acdfaddd898410544e8fb01040eb5482b5bb8ad3dec2746f5e86358ce941231eb5173e26db22f686883dc3d76e56b6579f30d2b9d82c1694d025c18091b11988a12ac7a730df77a61220b38e6271a4a53d4ae98c9dca4ec8b1c7fd62565083bcdb317680d35f1d525878a55d48bbd79b055b712af2a7ebbe41615f35462ecf3ca04ca8dbeaf3cb593f2802b63e13c35d49e37c643808c400e486172a0d9c47a6bb2cbdf15df7de0c6ffe72281cb713630827d8ab94ea9008ae506f08302621d6ee82932cadd93039f98d453f9d426921cf116dc6b097a9e6f810e054482967af0d9ab783d3407c3a2579dc46500bfaf0aa074f3611d42f6078917e25469f5dbeb4c20fbb917bd6334f1eddaa1776c38684e91625635a14a1845a9bb8d533f80ac4356f30f26b6d7932feaa70e07d74e33ca220a1dae85f14c0668fc481c72fa93843c3c9e421313292d9c358dda46687a1c438537cab660fe61d1b6c79e17f9c3f9dd3e500a24734224b21873831221e648a18e6811bb993ac1aae5b782e1a7ad71721373be626beb448ba213d24a97ede290fc038f417ae45faffcba2db1b9ef5439894089cb9e5666384a7e6bed22367af43f98e1e598e15d4965e2e1a78a9b0dafc4037a07e0c016e4ec2d58974828fb2084c37b01849d05f6373edef31bd29755e2f9f0bc05997363ba154921359299a562c5e045fbe9cdead60113b2a2085c03051556d6e8105e18516f3c0743fffe3e310ff99b872dea8b53bab1d6dd6c1c70b0cf0bf3d1367be2aac0860e787e63d7a0de25ce50874a4ae736f177c6f2b1fb7fe9c25e0979a583c1ecb0de763e482769776ba9689fc9a02931f92b5338c91c0abe919f4a6604105de828749e0a69c319aa5d9b44475ec5a349342e13cee27967b63dc1123a12b53a092a5bbe9b752ac7f6ddbd0face0111ba702b95e6275293f1b4ab777ae5e00b1295ca7b19728e19266570b59d08b663a94ee2f51078f3d682a725f1f2d05a08b01798f9d1152601de8c5598c1f50dca184afc03513fc12d90300f29e259da1945dda09dd68348bbbfba0c9cec830825d1bcd0aa3151987a98a6b91e8744440a45413f7efba3417f88876b3142ca35db59fe73870a8764c01ef89e119c8d94c06387034f19615cb30a5895d3c00049b9784b5972ab2294dba88a43a7d1615a0231c5a1a2c2b5f7a70d938d6d86e85a793b9b19ef26c970ca64d5d8c02dfd649aa3f82a8b6463836e7db096a212eeb07e775232396ce90055e50a4fbed68620abb9dd4187d2680bcfc6e81370c4f60f25605aac9edcd246d60d8cb3bf50ffef80b1a7b23c19360ccac638cafc6b3a06d86dc8e4f3793fc4a186288b1f481b136c439484a8aa0ca580abf071a6210f0bb60755a6fb6f6179a390914309ed7361c735ec6e692d5148fb94a2f2e80f2b5a0c5c2f0f26e92959ca65ced65e2a2ea6540c09ebc503a3b2e6d29433c81030e6bb8e3ef0f30669757e1635eecc0912b66291eaa8170d91009f238fa871385bc2e18f6e9c53d4b989c88971347d3ab673a2ef8e24160d7f89217897275056c16abd2a23d02c452cc333d00ad14d8c8141733c68db736ffa06bf151d3955f0e3c55cf86709db01c4b7fd8aaed2e081c7d6a7b03b46594ee4cd1c27de0fcd6ac3b0f023c1bfcd84245e3ee7f85a84992fa77032c3c21ad069ad6c34616d2704be6f71fa0b465e11a66b2dda2f3256b31829d821724425e11d1dc2b112ac32acf0f974f843b709d08d44fddfcd539f546308d12bc1763e624da1c5c23c2550651fbb1041898ffbc696fc561babb0879262328a5a91dac730a265ba2c0aeea2054a6347290bd279c4914049b681e48604ef2fef0f7f67ed5939220557fdff780df49d83d6dcbdb52c8c05277cbc6054115086701769bfdd227d9f8415625da69d4d073196b66f56e206411f46852ad6ccf22a422dc894cf8ab89a32a2a28ab8d55b6230dfcc62b2188d5c71313ee3d7dc1b388a37eeb43eb7088523e9adf87a3fc07b5681023ce2758b2b5383d949b75df7f7bef4cc33cc96c43d37149d9fe0ce22cb7f86f385a54539b71ac7d1915995c2f6b75d2f6f70e694caedfe68bc25994e64d7ece620938dee757a49490b2479ddc350ee9fa61edf39b18060910875d615b1f22530cecf0130d510d61af26fd6048f29a161812d1d23df724b964a8933baa237e6b49a00936ed9d68389989dc0fe41f0d73e840eb791484b0401a9363fa49eb79808e7c5102e5329064ddfcaff80df6a49b130fa377b2f1edc51206a1c58d019c0af7f0548c7509f8c8a8c4d577fe79b7fb3624c02e0065e91f24622d3730d426fb62ad4b20de6d91ce84767c36007fc58b311da12967af38c542ba435867a0709e700a46e965dade2bbfd01af832c302a8d5658e86b895c5d3f99acf332ed02fae4190ecdf2d74340322b3a4e4c38016e966f45e4030185345c4ef085e793fa973d5591cad321b21e0658d57a685dece54c06d01fed9418cfd6c150b8a681f2ed6df864de5329b2fa9e6c2b0987b7c7315278a7f03adb6790b540469977d364457130f307fda63bd0d5c77ea1582a9b6cb941c51cce93aad7cddce92478da0a09a76fea9e2ee598a9dc720f843bbad78484ed99ce2e2a356d631c8a2bba999b1e1f1c062fc0efc6e014ad45d0b2975442ef075e03f3245286827b1a1cc84ee2b1e7d4db047211cf4a469698a41196a34188de063634c5ce32010f9a74de825ad2483a91dadc6a2d978ba0e9b17fab81e5a75551ddd848cf2893d5dfac259b5e172a6f837b57f0d2e15f101cbece29c67cc2874c20441e438ff703132286aa11fbeb9cc9605e3e694262c5975f5bb3256e430dec581892409800bfd40e04f8116ad797022377ec03b615a1bab24a54224c593beac918a6fd8f68d55dcbb527a29c3a7220514217b5987b99e4fa5f3dd590787a9cbad7460801f7f5bde16ee6d9ec38285ebdec1950f5a13bb86aded843bde0ea5e98ca0f506c55d3c35a7061eaa81ccababf3671ecaf805af434c5c790010ba6694db82f6e8d558a7d92d798dd620d49075d9e0afb774f5fc2fe9cd3522577d2aa2e892cea18d35cd63465e71d6c360b498a1a9890124da9e363498dad0b0273d86e7b237a4fd97112cb0e6db60b62557242639d3b8c0938fd4df9b02f17860919b735832605472900414c38c0172e741183c6481057c27d84404b65b503b182b652f2e62f78e6447df3c1364e525a7965cc5d726e69541e88a3ce5664558da64b7e8778684f61f8d1f8b7bf4665324c9190f4e9c265c839f5679d1c8416537c9936d168f8846fcb8b18b2d70efbb41a369f07a060b7ca8bb9eb9c63048d2cbaf8bc46af00bbb07ab14409bebbc5f2a0bad886f0495b5035b5f79c6ccc3a28f75ea23a83cf2714b875a6530c47ae47692b95bd7a531ea2fa3ace385f63b4a738731506e8b6b402a102f33b1114c0f5db";
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
