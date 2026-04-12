//! Tests for the ZK Trie pallet.

use crate::{self as pallet_zk_trie, tree, *};
use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU32, Everything, Hooks},
};
use sp_core::{crypto::AccountId32, H256};
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};

construct_runtime!(
	pub enum Test {
		System: frame_system,
		ZkTrie: pallet_zk_trie,
	}
);

pub type AccountId = AccountId32;
pub type Block = frame_system::mocking::MockBlock<Test>;

parameter_types! {
	pub const BlockHashCount: u64 = 250;
}

impl frame_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type BaseCallFilter = Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type RuntimeTask = ();
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block;
	type BlockHashCount = BlockHashCount;
	type DbWeight = ();
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
	type ExtensionsWeightInfo = ();
}

impl Config for Test {
	type AssetId = u32;
	type Balance = u128;
}

fn new_test_ext() -> sp_io::TestExternalities {
	let t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| System::set_block_number(1));
	ext
}

fn make_account(seed: u8) -> AccountId {
	AccountId::new([seed; 32])
}

#[test]
fn test_capacity_at_depth() {
	assert_eq!(tree::capacity_at_depth(0), 0);
	assert_eq!(tree::capacity_at_depth(1), 4);
	assert_eq!(tree::capacity_at_depth(2), 16);
	assert_eq!(tree::capacity_at_depth(3), 64);
	assert_eq!(tree::capacity_at_depth(4), 256);
}

#[test]
fn test_hash_node() {
	let children = [[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];
	let hash = tree::hash_node(&children);
	assert_ne!(hash, [0u8; 32]);

	// Same input should give same output
	let hash2 = tree::hash_node(&children);
	assert_eq!(hash, hash2);

	// Different input should give different output
	let children2 = [[1u8; 32], [2u8; 32], [3u8; 32], [5u8; 32]];
	let hash3 = tree::hash_node(&children2);
	assert_ne!(hash, hash3);
}

#[test]
fn insert_first_leaf_works() {
	new_test_ext().execute_with(|| {
		let to = make_account(1);
		let (index, root) = ZkTrie::insert_leaf(to.clone(), 0, 0u32, 100u128);

		assert_eq!(index, 0);
		assert_ne!(root, [0u8; 32]);
		assert_eq!(ZkTrie::leaf_count(), 1);
		assert_eq!(ZkTrie::depth(), 1);
		assert_eq!(ZkTrie::root(), root);

		// Check leaf was stored
		let leaf = ZkTrie::leaf(0).unwrap();
		assert_eq!(leaf.to, to);
		assert_eq!(leaf.transfer_count, 0);
		assert_eq!(leaf.asset_id, 0);
		assert_eq!(leaf.amount, 100);
	});
}

#[test]
fn insert_multiple_leaves_works() {
	new_test_ext().execute_with(|| {
		let mut roots = Vec::new();

		for i in 0..4 {
			let to = make_account(i + 1);
			let (index, root) = ZkTrie::insert_leaf(to, i as u64, 0u32, (i + 1) as u128 * 100);
			assert_eq!(index, i as u64);
			roots.push(root);
		}

		assert_eq!(ZkTrie::leaf_count(), 4);
		assert_eq!(ZkTrie::depth(), 1); // 4 leaves fit in depth 1

		// Each insert should change the root
		for i in 1..roots.len() {
			assert_ne!(roots[i], roots[i - 1]);
		}
	});
}

#[test]
fn tree_grows_at_capacity() {
	new_test_ext().execute_with(|| {
		// Fill depth 1 (4 leaves)
		for i in 0..4 {
			let to = make_account(i + 1);
			ZkTrie::insert_leaf(to, i as u64, 0u32, 100u128);
		}
		assert_eq!(ZkTrie::depth(), 1);

		// 5th leaf should trigger growth to depth 2
		let to = make_account(5);
		ZkTrie::insert_leaf(to, 4, 0u32, 100u128);

		assert_eq!(ZkTrie::leaf_count(), 5);
		assert_eq!(ZkTrie::depth(), 2);
	});
}

#[test]
fn tree_grows_multiple_times() {
	new_test_ext().execute_with(|| {
		// Insert 20 leaves (need depth 3 to fit: 4^3 = 64)
		for i in 0..20 {
			let to = make_account((i % 255) as u8 + 1);
			ZkTrie::insert_leaf(to, i as u64, 0u32, 100u128);
		}

		assert_eq!(ZkTrie::leaf_count(), 20);
		assert_eq!(ZkTrie::depth(), 3); // 4^2 = 16 < 20 <= 64 = 4^3
	});
}

#[test]
fn merkle_proof_works() {
	new_test_ext().execute_with(|| {
		// Insert some leaves
		for i in 0..5 {
			let to = make_account(i + 1);
			ZkTrie::insert_leaf(to, i as u64, 0u32, (i + 1) as u128 * 100);
		}

		// Get proof for leaf 0
		let proof = ZkTrie::get_merkle_proof(0).unwrap();
		assert_eq!(proof.leaf_index, 0);
		assert_eq!(proof.siblings.len(), 2); // depth 2

		// Verify the proof
		let leaf = ZkTrie::leaf(0).unwrap();
		assert!(ZkTrie::verify_proof(&leaf, &proof));
	});
}

#[test]
fn merkle_proof_all_leaves() {
	new_test_ext().execute_with(|| {
		// Insert leaves
		for i in 0..10 {
			let to = make_account(i + 1);
			ZkTrie::insert_leaf(to, i as u64, i as u32, (i + 1) as u128 * 100);
		}

		// Verify proof for each leaf
		for i in 0..10 {
			let proof = ZkTrie::get_merkle_proof(i).unwrap();
			let leaf = ZkTrie::leaf(i).unwrap();
			assert!(ZkTrie::verify_proof(&leaf, &proof), "Proof failed for leaf {}", i);
		}
	});
}

#[test]
fn invalid_proof_fails() {
	new_test_ext().execute_with(|| {
		// Insert leaves
		for i in 0..5 {
			let to = make_account(i + 1);
			ZkTrie::insert_leaf(to, i as u64, 0u32, (i + 1) as u128 * 100);
		}

		// Get proof for leaf 0
		let proof = ZkTrie::get_merkle_proof(0).unwrap();

		// Try to verify with wrong leaf data
		let wrong_leaf =
			ZkLeaf { to: make_account(99), transfer_count: 0, asset_id: 0u32, amount: 100u128 };
		assert!(!ZkTrie::verify_proof(&wrong_leaf, &proof));
	});
}

#[test]
fn proof_for_nonexistent_leaf_fails() {
	new_test_ext().execute_with(|| {
		ZkTrie::insert_leaf(make_account(1), 0, 0u32, 100u128);

		// Try to get proof for leaf index 5 (doesn't exist)
		let result = ZkTrie::get_merkle_proof(5);
		assert!(result.is_err());
	});
}

#[test]
fn root_changes_on_insert() {
	new_test_ext().execute_with(|| {
		let (_, root1) = ZkTrie::insert_leaf(make_account(1), 0, 0u32, 100u128);
		let (_, root2) = ZkTrie::insert_leaf(make_account(2), 1, 0u32, 200u128);

		assert_ne!(root1, root2);
		assert_eq!(ZkTrie::root(), root2);
	});
}

#[test]
fn different_amounts_give_different_hashes() {
	new_test_ext().execute_with(|| {
		let (_, root1) = ZkTrie::insert_leaf(make_account(1), 0, 0u32, 100u128);

		// Reset and insert with different amount
		crate::Leaves::<Test>::remove(0);
		crate::LeafCount::<Test>::put(0);
		crate::Depth::<Test>::put(0);
		crate::Root::<Test>::put([0u8; 32]);

		let (_, root2) = ZkTrie::insert_leaf(make_account(1), 0, 0u32, 200u128);

		assert_ne!(root1, root2);
	});
}

#[test]
fn digest_log_contains_root() {
	new_test_ext().execute_with(|| {
		ZkTrie::insert_leaf(make_account(1), 0, 0u32, 100u128);
		let expected_root = ZkTrie::root();

		// Trigger on_finalize
		ZkTrie::on_finalize(1);

		// Check digest
		let digest = System::digest();
		assert!(!digest.logs.is_empty());

		// Find the ZkRoot log
		let found = digest.logs.iter().any(|item| {
			if let sp_runtime::generic::DigestItem::Other(data) = item {
				data.as_slice() == expected_root
			} else {
				false
			}
		});
		assert!(found, "ZkRoot not found in digest");
	});
}

/// Helper to extract ZkRoot from digest
fn extract_zk_root_from_digest() -> Option<Hash256> {
	let digest = System::digest();
	for item in digest.logs.iter() {
		if let sp_runtime::generic::DigestItem::Other(data) = item {
			if data.len() == 32 {
				let mut root = [0u8; 32];
				root.copy_from_slice(data);
				return Some(root);
			}
		}
	}
	None
}

/// Simulate a transfer by inserting a leaf into the ZK trie.
/// In production, pallet-wormhole would call this.
fn simulate_transfer(
	to: AccountId,
	transfer_count: u64,
	asset_id: u32,
	amount: u128,
) -> (u64, Hash256) {
	ZkTrie::insert_leaf(to, transfer_count, asset_id, amount)
}

#[test]
fn integration_many_transfers_updates_root_in_digest() {
	new_test_ext().execute_with(|| {
		let alice = make_account(1);
		let bob = make_account(2);
		let charlie = make_account(3);

		// === Insert many transfers and verify tree grows correctly ===

		// First 3 transfers
		let (idx0, _) = simulate_transfer(alice.clone(), 0, 0, 1000);
		let (idx1, _) = simulate_transfer(bob.clone(), 0, 0, 2000);
		let (idx2, _) = simulate_transfer(charlie.clone(), 0, 0, 3000);

		assert_eq!(idx0, 0);
		assert_eq!(idx1, 1);
		assert_eq!(idx2, 2);
		assert_eq!(ZkTrie::leaf_count(), 3);

		let root_after_3 = ZkTrie::root();

		// Verify proofs for first 3 leaves
		for idx in 0..3 {
			let proof = ZkTrie::get_merkle_proof(idx).expect("proof should exist");
			let leaf = ZkTrie::leaf(idx).expect("leaf should exist");
			assert!(ZkTrie::verify_proof(&leaf, &proof), "proof {} should verify", idx);
		}

		// 5 more transfers - tree will grow from depth 1 (capacity 4) to depth 2 (capacity 16)
		simulate_transfer(alice.clone(), 1, 0, 500);
		simulate_transfer(bob.clone(), 1, 0, 600);
		simulate_transfer(charlie.clone(), 1, 0, 700);
		simulate_transfer(alice.clone(), 2, 1, 100); // Different asset
		simulate_transfer(bob.clone(), 2, 1, 200); // Different asset

		assert_eq!(ZkTrie::leaf_count(), 8);
		assert!(ZkTrie::depth() >= 2, "tree should have grown to depth 2");

		let root_after_8 = ZkTrie::root();
		assert_ne!(root_after_3, root_after_8, "root should change after new transfers");

		// Verify proofs for all 8 leaves
		for idx in 0..8 {
			let proof = ZkTrie::get_merkle_proof(idx).expect("proof should exist");
			let leaf = ZkTrie::leaf(idx).expect("leaf should exist");
			assert!(ZkTrie::verify_proof(&leaf, &proof), "proof {} should verify", idx);
		}

		// Add 10 more transfers (total 18, tree needs depth 3 for capacity 64)
		for i in 0..10u64 {
			let recipient = make_account((i % 5) as u8 + 10);
			simulate_transfer(recipient, i, 0, (i as u128 + 1) * 1000);
		}

		assert_eq!(ZkTrie::leaf_count(), 18);

		let root_after_18 = ZkTrie::root();
		assert_ne!(root_after_8, root_after_18, "root should change after more transfers");

		// Verify ALL proofs still work after tree growth
		for idx in 0..18 {
			let proof = ZkTrie::get_merkle_proof(idx).expect("proof should exist");
			let leaf = ZkTrie::leaf(idx).expect("leaf should exist");
			assert!(
				ZkTrie::verify_proof(&leaf, &proof),
				"proof {} should verify after growth",
				idx
			);
		}

		// === Verify specific leaf data ===
		let leaf_0 = ZkTrie::leaf(0).expect("leaf 0 should exist");
		assert_eq!(leaf_0.to, alice);
		assert_eq!(leaf_0.transfer_count, 0);
		assert_eq!(leaf_0.asset_id, 0);
		assert_eq!(leaf_0.amount, 1000);

		let leaf_6 = ZkTrie::leaf(6).expect("leaf 6 should exist");
		assert_eq!(leaf_6.to, alice);
		assert_eq!(leaf_6.transfer_count, 2);
		assert_eq!(leaf_6.asset_id, 1); // Different asset
		assert_eq!(leaf_6.amount, 100);

		// === Finally verify root appears in digest on finalize ===
		ZkTrie::on_finalize(1);
		let digest_root = extract_zk_root_from_digest().expect("ZkRoot should be in digest");
		assert_eq!(digest_root, root_after_18, "digest should contain current root");
	});
}

#[test]
fn integration_empty_tree_has_zero_root_in_digest() {
	new_test_ext().execute_with(|| {
		// No transfers - tree is empty
		assert_eq!(ZkTrie::leaf_count(), 0);
		assert_eq!(ZkTrie::depth(), 0);

		let empty_root = ZkTrie::root();
		assert_eq!(empty_root, [0u8; 32], "empty tree should have zero root");

		// Finalize and check digest
		ZkTrie::on_finalize(1);
		let digest_root = extract_zk_root_from_digest().expect("ZkRoot should be in digest");
		assert_eq!(digest_root, empty_root);
	});
}

#[test]
fn integration_root_changes_only_on_insert() {
	new_test_ext().execute_with(|| {
		let alice = make_account(1);

		// Insert a leaf
		simulate_transfer(alice.clone(), 0, 0, 1000);
		let root_after_insert = ZkTrie::root();

		// Finalize - root should not change
		ZkTrie::on_finalize(1);
		assert_eq!(ZkTrie::root(), root_after_insert, "finalize should not change root");

		// Another finalize - still same root
		ZkTrie::on_finalize(2);
		assert_eq!(ZkTrie::root(), root_after_insert, "second finalize should not change root");

		// Insert another leaf - NOW root should change
		simulate_transfer(alice.clone(), 1, 0, 2000);
		assert_ne!(ZkTrie::root(), root_after_insert, "insert should change root");
	});
}

#[test]
fn integration_proof_siblings_at_correct_depth() {
	new_test_ext().execute_with(|| {
		// Insert 5 leaves to force depth 2
		for i in 0..5u64 {
			let account = make_account(i as u8);
			simulate_transfer(account, 0, 0, (i as u128 + 1) * 100);
		}

		assert_eq!(ZkTrie::depth(), 2);

		// Verify proofs have correct number of sibling levels
		// No path indices needed - children are sorted before hashing
		for i in 0..5u64 {
			let proof = ZkTrie::get_merkle_proof(i).unwrap();
			assert_eq!(proof.siblings.len(), 2, "depth 2 tree should have 2 levels of siblings");

			// Each level should have 3 siblings (4-ary tree)
			for level_siblings in &proof.siblings {
				assert_eq!(level_siblings.len(), 3);
			}

			// Verify the proof works
			let leaf = ZkTrie::leaf(i).unwrap();
			assert!(ZkTrie::verify_proof(&leaf, &proof), "proof for leaf {} should verify", i);
		}
	});
}
