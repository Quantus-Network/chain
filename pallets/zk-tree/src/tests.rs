//! Tests for the ZK Tree pallet.

use crate::{self as pallet_zk_tree, tree, *};
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
		ZkTree: pallet_zk_tree,
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
	type AuthorizeUpgradeOrigin = frame_system::EnsureRoot<Self::AccountId>;
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

/// Fold pending leaves into the root, as `on_finalize` does once per block.
fn settle() {
	ZkTree::process_pending_leaves();
}

/// Insert a leaf and immediately settle it, mirroring the pre-batching
/// "every insert recomputes the root" behavior. Returns `(index, root)`.
fn insert_and_settle(
	to: AccountId,
	transfer_count: u64,
	asset_id: u32,
	amount: u128,
) -> (u64, Hash256) {
	let index = ZkTree::insert_leaf(to, transfer_count, asset_id, amount);
	settle();
	(index, ZkTree::root())
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

/// Golden vector for the leaf hash.
///
/// Pins the exact `hash_leaf` output for a fixed canonical leaf so that any change to
/// the encoding (felt layout, quantization, recipient handling) fails loudly. The leaf
/// encoding must stay byte-compatible with the ZK circuit's
/// `ZkLeafTargets::collect_for_hash()`; a cross-check against the real circuit crates
/// lives in `pallet-wormhole`'s `pallet_leaf_hash_matches_circuit_leaf_hash` test.
/// If this value ever needs to change, the deployed withdrawal circuit must change
/// with it — do not update the constant casually.
#[test]
fn hash_leaf_golden_vector() {
	let leaf = ZkLeaf::<AccountId, u32, u128> {
		to: make_account(0x11),
		transfer_count: 7,
		asset_id: 5,
		// 1234 quantized units (hash_leaf divides by AMOUNT_SCALE_DOWN_FACTOR).
		amount: 1234 * tree::AMOUNT_SCALE_DOWN_FACTOR,
	};
	// Cross-checked against the circuit-side computation by
	// `hash_leaf_golden_vector_matches_circuit` in pallet-wormhole's tests.
	let expected: [u8; 32] = [
		195, 94, 210, 27, 96, 177, 127, 68, 16, 231, 47, 227, 104, 21, 175, 254, 219, 85, 224, 111,
		64, 162, 32, 119, 226, 89, 143, 126, 203, 254, 51, 93,
	];
	assert_eq!(tree::hash_leaf::<Test>(&leaf), expected);
}

/// Helper for the amount-commitment tests: a fixed leaf varying only in `amount`.
fn amount_leaf(amount: u128) -> ZkLeaf<AccountId, u32, u128> {
	ZkLeaf { to: make_account(0x11), transfer_count: 7, asset_id: 5, amount }
}

/// Oversized amounts must not alias small amounts in the leaf commitment.
///
/// The leaf commits the quantized amount (`amount / AMOUNT_SCALE_DOWN_FACTOR`) as a
/// u32. Quantized values above `u32::MAX` are unrepresentable and must saturate to
/// `u32::MAX` rather than wrap — otherwise a transfer of `X + 2^32·SCALE` commits to
/// exactly the same leaf data as a transfer of `X`, making the authoritative Merkle
/// root ambiguous between economically different transfers. This is unreachable for
/// the native token (the supply cap keeps quantized amounts below u32::MAX) but fully
/// reachable for permissionlessly minted pallet-assets amounts.
#[test]
fn hash_leaf_oversized_amount_does_not_alias_small_amount() {
	let small = 1234 * tree::AMOUNT_SCALE_DOWN_FACTOR;
	// Quantizes to 1234 + 2^32, which truncates back to a committed amount of 1234
	// if the encoding wraps instead of saturating.
	let wrapping_alias = small + (1u128 << 32) * tree::AMOUNT_SCALE_DOWN_FACTOR;

	assert_ne!(
		tree::hash_leaf::<Test>(&amount_leaf(small)),
		tree::hash_leaf::<Test>(&amount_leaf(wrapping_alias)),
		"an oversized amount must not commit to the same leaf as a small amount"
	);
}

/// Pins the saturation semantics: every quantized amount at or above `u32::MAX`
/// commits to the same capped value, `u32::MAX`.
#[test]
fn hash_leaf_saturates_oversized_amounts_at_u32_max() {
	let cap = (u32::MAX as u128) * tree::AMOUNT_SCALE_DOWN_FACTOR;
	let at_cap = tree::hash_leaf::<Test>(&amount_leaf(cap));

	assert_eq!(
		at_cap,
		tree::hash_leaf::<Test>(&amount_leaf(cap + tree::AMOUNT_SCALE_DOWN_FACTOR)),
		"one quantized unit above the cap must saturate to the cap commitment"
	);
	assert_eq!(
		at_cap,
		tree::hash_leaf::<Test>(&amount_leaf(u128::MAX)),
		"u128::MAX must saturate to the cap commitment"
	);
	assert_ne!(
		at_cap,
		tree::hash_leaf::<Test>(&amount_leaf(cap - tree::AMOUNT_SCALE_DOWN_FACTOR)),
		"amounts below the cap must still commit to their true value"
	);
}

#[test]
fn insert_first_leaf_works() {
	new_test_ext().execute_with(|| {
		let to = make_account(1);
		let index = ZkTree::insert_leaf(to.clone(), 0, 0u32, 100u128);

		assert_eq!(index, 0);
		assert_eq!(ZkTree::leaf_count(), 1);

		// The insert only appends; the root is computed at end of block.
		assert_eq!(ZkTree::unprocessed_leaves(), 1);
		assert_eq!(ZkTree::root(), [0u8; 32]);
		assert_eq!(ZkTree::depth(), 0);

		settle();

		assert_eq!(ZkTree::unprocessed_leaves(), 0);
		assert_ne!(ZkTree::root(), [0u8; 32]);
		assert_eq!(ZkTree::depth(), 1);

		// Check leaf was stored
		let leaf = ZkTree::leaf(0).unwrap();
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
			let (index, root) = insert_and_settle(to, i as u64, 0u32, (i + 1) as u128 * 100);
			assert_eq!(index, i as u64);
			roots.push(root);
		}

		assert_eq!(ZkTree::leaf_count(), 4);
		assert_eq!(ZkTree::depth(), 1); // 4 leaves fit in depth 1

		// Each settled insert should change the root
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
			ZkTree::insert_leaf(to, i as u64, 0u32, 100u128);
		}
		settle();
		assert_eq!(ZkTree::depth(), 1);

		// 5th leaf starts exactly at the old capacity boundary and should
		// trigger growth to depth 2 when settled.
		let to = make_account(5);
		ZkTree::insert_leaf(to, 4, 0u32, 100u128);
		settle();

		assert_eq!(ZkTree::leaf_count(), 5);
		assert_eq!(ZkTree::depth(), 2);
	});
}

#[test]
fn tree_grows_multiple_times() {
	new_test_ext().execute_with(|| {
		// Insert 20 leaves in ONE batch (need depth 3 to fit: 4^3 = 64), so a
		// single settlement grows the tree by two levels at once.
		for i in 0..20 {
			let to = make_account((i % 255) as u8 + 1);
			ZkTree::insert_leaf(to, i as u64, 0u32, 100u128);
		}
		settle();

		assert_eq!(ZkTree::leaf_count(), 20);
		assert_eq!(ZkTree::depth(), 3); // 4^2 = 16 < 20 <= 64 = 4^3
	});
}

#[test]
fn merkle_proof_works() {
	new_test_ext().execute_with(|| {
		// Insert some leaves
		for i in 0..5 {
			let to = make_account(i + 1);
			ZkTree::insert_leaf(to, i as u64, 0u32, (i + 1) as u128 * 100);
		}
		settle();

		// Get proof for leaf 0
		let proof = ZkTree::get_merkle_proof(0).unwrap();
		assert_eq!(proof.leaf_index, 0);
		assert_eq!(proof.siblings.len(), 2); // depth 2

		// Verify the proof
		let leaf = ZkTree::leaf(0).unwrap();
		assert!(ZkTree::verify_proof(&leaf, &proof));
	});
}

#[test]
fn merkle_proof_all_leaves() {
	new_test_ext().execute_with(|| {
		// Insert leaves
		for i in 0..10 {
			let to = make_account(i + 1);
			ZkTree::insert_leaf(to, i as u64, i as u32, (i + 1) as u128 * 100);
		}
		settle();

		// Verify proof for each leaf
		for i in 0..10 {
			let proof = ZkTree::get_merkle_proof(i).unwrap();
			let leaf = ZkTree::leaf(i).unwrap();
			assert!(ZkTree::verify_proof(&leaf, &proof), "Proof failed for leaf {}", i);
		}
	});
}

#[test]
fn invalid_proof_fails() {
	new_test_ext().execute_with(|| {
		// Insert leaves
		for i in 0..5 {
			let to = make_account(i + 1);
			ZkTree::insert_leaf(to, i as u64, 0u32, (i + 1) as u128 * 100);
		}
		settle();

		// Get proof for leaf 0
		let proof = ZkTree::get_merkle_proof(0).unwrap();

		// Try to verify with wrong leaf data
		let wrong_leaf =
			ZkLeaf { to: make_account(99), transfer_count: 0, asset_id: 0u32, amount: 100u128 };
		assert!(!ZkTree::verify_proof(&wrong_leaf, &proof));
	});
}

#[test]
fn proof_for_nonexistent_leaf_fails() {
	new_test_ext().execute_with(|| {
		ZkTree::insert_leaf(make_account(1), 0, 0u32, 100u128);
		settle();

		// Try to get proof for leaf index 5 (doesn't exist)
		assert!(matches!(ZkTree::get_merkle_proof(5), Err(Error::<Test>::LeafIndexOutOfBounds)));
	});
}

#[test]
fn proof_for_pending_leaf_fails_until_settled() {
	new_test_ext().execute_with(|| {
		ZkTree::insert_leaf(make_account(1), 0, 0u32, 100u128);
		settle();
		ZkTree::insert_leaf(make_account(2), 0, 0u32, 200u128);

		// Leaf 1 is appended but not yet folded into the root: `Nodes` doesn't
		// cover it, so no valid proof can exist for it yet.
		assert!(matches!(ZkTree::get_merkle_proof(1), Err(Error::<Test>::LeafNotYetSettled)));
		// Settled leaves stay provable.
		assert!(ZkTree::get_merkle_proof(0).is_ok());

		settle();
		assert!(ZkTree::get_merkle_proof(1).is_ok());
	});
}

#[test]
fn root_changes_on_settle() {
	new_test_ext().execute_with(|| {
		let (_, root1) = insert_and_settle(make_account(1), 0, 0u32, 100u128);
		let (_, root2) = insert_and_settle(make_account(2), 1, 0u32, 200u128);

		assert_ne!(root1, root2);
		assert_eq!(ZkTree::root(), root2);
	});
}

#[test]
fn different_amounts_give_different_hashes() {
	new_test_ext().execute_with(|| {
		// Use amounts large enough to differ after quantization.
		// Amounts are quantized by dividing by 10^10 (AMOUNT_SCALE_DOWN_FACTOR).
		// 1 DEV = 10^12 planck → 100 quantized units
		// 2 DEV = 2*10^12 planck → 200 quantized units
		let one_dev = 1_000_000_000_000u128; // 10^12
		let two_dev = 2_000_000_000_000u128; // 2*10^12

		let (_, root1) = insert_and_settle(make_account(1), 0, 0u32, one_dev);

		// Reset and insert with different amount
		crate::Leaves::<Test>::remove(0);
		crate::LeafCount::<Test>::put(0);
		crate::Depth::<Test>::put(0);
		crate::Root::<Test>::put([0u8; 32]);
		// Also reset the internal nodes
		let _ = crate::Nodes::<Test>::clear(u32::MAX, None);

		let (_, root2) = insert_and_settle(make_account(1), 0, 0u32, two_dev);

		assert_ne!(root1, root2);
	});
}

#[test]
fn zk_tree_root_set_in_frame_system() {
	new_test_ext().execute_with(|| {
		ZkTree::insert_leaf(make_account(1), 0, 0u32, 100u128);

		// on_finalize folds the block's pending leaves into the root and then
		// publishes it to frame_system for the block header.
		ZkTree::on_finalize(1);

		let expected_root = ZkTree::root();
		assert_ne!(expected_root, [0u8; 32], "finalize must fold the pending leaf into the root");
		assert_eq!(ZkTree::unprocessed_leaves(), 0, "finalize must drain pending leaves");

		// Check that the root was set in frame_system storage using the getter
		let stored_root: H256 = frame_system::Pallet::<Test>::zk_tree_root();
		assert_eq!(stored_root.0, expected_root, "ZkTreeRoot not set correctly in frame_system");
	});
}

/// Helper to extract ZkRoot from frame_system storage
fn extract_zk_root_from_frame_system() -> Option<Hash256> {
	let stored: H256 = frame_system::Pallet::<Test>::zk_tree_root();
	// Return Some even for zero since that's a valid empty tree root
	Some(stored.0)
}

/// Simulate a transfer by inserting a leaf into the ZK trie.
/// In production, pallet-wormhole would call this.
fn simulate_transfer(to: AccountId, transfer_count: u64, asset_id: u32, amount: u128) -> u64 {
	ZkTree::insert_leaf(to, transfer_count, asset_id, amount)
}

#[test]
fn integration_many_transfers_updates_root() {
	new_test_ext().execute_with(|| {
		let alice = make_account(1);
		let bob = make_account(2);
		let charlie = make_account(3);

		// === Insert many transfers (settling per "block") and verify the tree
		// grows correctly ===

		// First block: 3 transfers
		let idx0 = simulate_transfer(alice.clone(), 0, 0, 1000);
		let idx1 = simulate_transfer(bob.clone(), 0, 0, 2000);
		let idx2 = simulate_transfer(charlie.clone(), 0, 0, 3000);
		settle();

		assert_eq!(idx0, 0);
		assert_eq!(idx1, 1);
		assert_eq!(idx2, 2);
		assert_eq!(ZkTree::leaf_count(), 3);

		let root_after_3 = ZkTree::root();

		// Verify proofs for first 3 leaves
		for idx in 0..3 {
			let proof = ZkTree::get_merkle_proof(idx).expect("proof should exist");
			let leaf = ZkTree::leaf(idx).expect("leaf should exist");
			assert!(ZkTree::verify_proof(&leaf, &proof), "proof {} should verify", idx);
		}

		// Second block: 5 more transfers - the batch spans the depth-1 capacity
		// boundary (4), so this settlement grows the tree to depth 2.
		simulate_transfer(alice.clone(), 1, 0, 500);
		simulate_transfer(bob.clone(), 1, 0, 600);
		simulate_transfer(charlie.clone(), 1, 0, 700);
		simulate_transfer(alice.clone(), 2, 1, 100); // Different asset
		simulate_transfer(bob.clone(), 2, 1, 200); // Different asset
		settle();

		assert_eq!(ZkTree::leaf_count(), 8);
		assert!(ZkTree::depth() >= 2, "tree should have grown to depth 2");

		let root_after_8 = ZkTree::root();
		assert_ne!(root_after_3, root_after_8, "root should change after new transfers");

		// Verify proofs for all 8 leaves
		for idx in 0..8 {
			let proof = ZkTree::get_merkle_proof(idx).expect("proof should exist");
			let leaf = ZkTree::leaf(idx).expect("leaf should exist");
			assert!(ZkTree::verify_proof(&leaf, &proof), "proof {} should verify", idx);
		}

		// Third block: 10 more transfers (total 18, tree needs depth 3 for capacity 64)
		for i in 0..10u64 {
			let recipient = make_account((i % 5) as u8 + 10);
			simulate_transfer(recipient, i, 0, (i as u128 + 1) * 1000);
		}
		settle();

		assert_eq!(ZkTree::leaf_count(), 18);

		let root_after_18 = ZkTree::root();
		assert_ne!(root_after_8, root_after_18, "root should change after more transfers");

		// Verify ALL proofs still work after tree growth
		for idx in 0..18 {
			let proof = ZkTree::get_merkle_proof(idx).expect("proof should exist");
			let leaf = ZkTree::leaf(idx).expect("leaf should exist");
			assert!(
				ZkTree::verify_proof(&leaf, &proof),
				"proof {} should verify after growth",
				idx
			);
		}

		// === Verify specific leaf data ===
		let leaf_0 = ZkTree::leaf(0).expect("leaf 0 should exist");
		assert_eq!(leaf_0.to, alice);
		assert_eq!(leaf_0.transfer_count, 0);
		assert_eq!(leaf_0.asset_id, 0);
		assert_eq!(leaf_0.amount, 1000);

		let leaf_6 = ZkTree::leaf(6).expect("leaf 6 should exist");
		assert_eq!(leaf_6.to, alice);
		assert_eq!(leaf_6.transfer_count, 2);
		assert_eq!(leaf_6.asset_id, 1); // Different asset
		assert_eq!(leaf_6.amount, 100);

		// === Finally verify root is set in frame_system on finalize ===
		ZkTree::on_finalize(1);
		let stored_root =
			extract_zk_root_from_frame_system().expect("ZkRoot should be in frame_system");
		assert_eq!(stored_root, root_after_18, "frame_system should contain current root");
	});
}

#[test]
fn integration_empty_tree_has_zero_root_in_frame_system() {
	new_test_ext().execute_with(|| {
		// No transfers - tree is empty
		assert_eq!(ZkTree::leaf_count(), 0);
		assert_eq!(ZkTree::depth(), 0);

		let empty_root = ZkTree::root();
		assert_eq!(empty_root, [0u8; 32], "empty tree should have zero root");

		// Finalize and check frame_system storage
		ZkTree::on_finalize(1);
		let stored_root =
			extract_zk_root_from_frame_system().expect("ZkRoot should be in frame_system");
		assert_eq!(stored_root, empty_root);
	});
}

#[test]
fn integration_root_changes_only_on_finalize() {
	new_test_ext().execute_with(|| {
		let alice = make_account(1);

		// Inserting alone must NOT change the root: the batch runs at finalize.
		simulate_transfer(alice.clone(), 0, 0, 1000);
		assert_eq!(ZkTree::root(), [0u8; 32], "insert alone should not change root");

		// Finalize folds the pending leaf in.
		ZkTree::on_finalize(1);
		let root_after_block_1 = ZkTree::root();
		assert_ne!(root_after_block_1, [0u8; 32], "finalize should compute the root");

		// A block without inserts leaves the root untouched.
		ZkTree::on_finalize(2);
		assert_eq!(ZkTree::root(), root_after_block_1, "empty finalize should not change root");

		// Another insert + finalize changes it again.
		simulate_transfer(alice.clone(), 1, 0, 2000);
		assert_eq!(ZkTree::root(), root_after_block_1, "root must stay fixed until finalize");
		ZkTree::on_finalize(3);
		assert_ne!(ZkTree::root(), root_after_block_1, "finalize should fold the new leaf in");
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
		settle();

		assert_eq!(ZkTree::depth(), 2);

		// Verify proofs have correct number of sibling levels
		// No path indices needed - children are sorted before hashing
		for i in 0..5u64 {
			let proof = ZkTree::get_merkle_proof(i).unwrap();
			assert_eq!(proof.siblings.len(), 2, "depth 2 tree should have 2 levels of siblings");

			// Each level should have 3 siblings (4-ary tree)
			for level_siblings in &proof.siblings {
				assert_eq!(level_siblings.len(), 3);
			}

			// Verify the proof works
			let leaf = ZkTree::leaf(i).unwrap();
			assert!(ZkTree::verify_proof(&leaf, &proof), "proof for leaf {} should verify", i);
		}
	});
}

// ============================================================================
// Batched settlement equivalence
// ============================================================================

/// Leaf fixture shared by the equivalence tests.
fn equivalence_leaf(i: u64) -> ZkLeaf<AccountId, u32, u128> {
	ZkLeaf {
		to: make_account((i % 250) as u8 + 1),
		transfer_count: i,
		asset_id: (i % 3) as u32,
		amount: (i + 1) as u128 * 100,
	}
}

fn insert_equivalence_leaf(i: u64) {
	let leaf = equivalence_leaf(i);
	ZkTree::insert_leaf(leaf.to, leaf.transfer_count, leaf.asset_id, leaf.amount);
}

/// Independent reference: build the whole positional 4-ary tree in memory.
///
/// Mirrors the pallet's storage semantics where an absent (never written) node
/// reads as the zero hash: a subtree with all-empty children stays the zero hash
/// instead of being hashed.
fn reference_root(leaf_hashes: &[Hash256]) -> Hash256 {
	assert!(!leaf_hashes.is_empty());
	let mut depth = 0u8;
	while tree::capacity_at_depth(depth) < leaf_hashes.len() as u64 {
		depth += 1;
	}

	let mut level = leaf_hashes.to_vec();
	level.resize(tree::capacity_at_depth(depth) as usize, tree::empty_hash());
	for _ in 0..depth {
		level = level
			.chunks(4)
			.map(|children| {
				let children: [Hash256; 4] = [children[0], children[1], children[2], children[3]];
				if children == [tree::empty_hash(); 4] {
					tree::empty_hash()
				} else {
					tree::hash_node(&children)
				}
			})
			.collect();
	}
	level[0]
}

/// The once-per-block batch must produce exactly the same root as settling after
/// every single insert (the pre-batching behavior), and both must match an
/// independent in-memory reference. Sizes cross the depth-1 (4) and depth-2 (16)
/// capacity boundaries, including multi-level growth inside one batch.
#[test]
fn batched_settlement_matches_per_insert_settlement_and_reference() {
	for n in [1u64, 2, 3, 4, 5, 7, 15, 16, 17, 20, 40, 65] {
		let batch_root = new_test_ext().execute_with(|| {
			for i in 0..n {
				insert_equivalence_leaf(i);
			}
			settle();
			ZkTree::root()
		});

		let per_insert_root = new_test_ext().execute_with(|| {
			for i in 0..n {
				insert_equivalence_leaf(i);
				settle();
			}
			ZkTree::root()
		});

		let reference = new_test_ext().execute_with(|| {
			let hashes: Vec<Hash256> =
				(0..n).map(|i| tree::hash_leaf::<Test>(&equivalence_leaf(i))).collect();
			reference_root(&hashes)
		});

		assert_eq!(batch_root, per_insert_root, "batch vs per-insert mismatch for n={n}");
		assert_eq!(batch_root, reference, "batch vs reference mismatch for n={n}");
	}
}

/// Split the same leaf sequence into blocks at every possible point (including
/// splits landing exactly on capacity boundaries): the final root must never
/// depend on how the inserts were grouped, and all leaves must stay provable.
#[test]
fn root_is_independent_of_block_grouping() {
	let total = 21u64; // spans the 4 and 16 capacity boundaries

	let all_at_once = new_test_ext().execute_with(|| {
		for i in 0..total {
			insert_equivalence_leaf(i);
		}
		settle();
		ZkTree::root()
	});

	for split in 1..total {
		let grouped = new_test_ext().execute_with(|| {
			for i in 0..split {
				insert_equivalence_leaf(i);
			}
			settle();
			for i in split..total {
				insert_equivalence_leaf(i);
			}
			settle();

			// Every settled leaf must still be provable against the final root.
			for i in 0..total {
				let proof = ZkTree::get_merkle_proof(i).expect("proof should exist");
				let leaf = ZkTree::leaf(i).expect("leaf should exist");
				assert!(
					ZkTree::verify_proof(&leaf, &proof),
					"proof {i} should verify (split={split})"
				);
			}
			ZkTree::root()
		});
		assert_eq!(grouped, all_at_once, "root mismatch for split={split}");
	}
}

/// A settlement starting exactly at the old capacity (start == 4^depth) relies on
/// `grow_tree` parking the old root at `(old_depth, 0)`; a settlement spanning
/// the boundary overwrites the parked value. Both must agree with the reference.
#[test]
fn growth_boundary_settlements_are_consistent() {
	// Exactly at the boundary: 4 leaves settled, then 1 more in its own block.
	let at_boundary = new_test_ext().execute_with(|| {
		for i in 0..4 {
			insert_equivalence_leaf(i);
		}
		settle();
		insert_equivalence_leaf(4);
		settle();
		ZkTree::root()
	});

	// Spanning the boundary: 3 settled, then 2 in one block.
	let spanning = new_test_ext().execute_with(|| {
		for i in 0..3 {
			insert_equivalence_leaf(i);
		}
		settle();
		insert_equivalence_leaf(3);
		insert_equivalence_leaf(4);
		settle();
		ZkTree::root()
	});

	let reference = new_test_ext().execute_with(|| {
		let hashes: Vec<Hash256> =
			(0..5).map(|i| tree::hash_leaf::<Test>(&equivalence_leaf(i))).collect();
		reference_root(&hashes)
	});

	assert_eq!(at_boundary, reference);
	assert_eq!(spanning, reference);
}
