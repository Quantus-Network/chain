use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};
use sp_core::H256;

fn hash_from_u64(n: u64) -> H256 {
	H256::from_low_u64_be(n)
}

#[test]
fn genesis_block_works() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);

		// Add genesis block with no parents
		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		// Check genesis block data
		let ghostdag_data = DagConsensus::ghostdag_data(&genesis_hash).unwrap();
		assert_eq!(ghostdag_data.blue_score, 0);
		assert_eq!(ghostdag_data.blue_work, 0);
		assert_eq!(ghostdag_data.mergeset_blues.len(), 0);
		assert_eq!(ghostdag_data.mergeset_reds.len(), 0);

		// Check it's in tips
		let tips = DagConsensus::dag_tips();
		assert_eq!(tips.len(), 1);
		assert!(tips.contains(&genesis_hash));
	});
}

#[test]
fn linear_chain_works() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);
		let block_2_hash = hash_from_u64(2);
		let block_3_hash = hash_from_u64(3);

		// Add genesis
		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		// Add block 2
		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_2_hash,
			vec![genesis_hash]
		));

		// Add block 3
		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_3_hash,
			vec![block_2_hash]
		));

		// Check block 2 data
		let block_2_data = DagConsensus::ghostdag_data(&block_2_hash).unwrap();
		assert_eq!(block_2_data.blue_score, 1); // genesis + itself
		assert_eq!(block_2_data.selected_parent, genesis_hash);
		assert_eq!(block_2_data.mergeset_blues.len(), 0);
		assert_eq!(block_2_data.mergeset_reds.len(), 0);

		// Check block 3 data
		let block_3_data = DagConsensus::ghostdag_data(&block_3_hash).unwrap();
		assert_eq!(block_3_data.blue_score, 2); // genesis + block_2 + itself
		assert_eq!(block_3_data.selected_parent, block_2_hash);

		// Check tips
		let tips = DagConsensus::dag_tips();
		assert_eq!(tips.len(), 1);
		assert!(tips.contains(&block_3_hash));
	});
}

#[test]
fn simple_dag_works() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);
		let block_2_hash = hash_from_u64(2);
		let block_3_hash = hash_from_u64(3);
		let block_4_hash = hash_from_u64(4);

		// Add genesis
		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		// Add two children of genesis (creating a fork)
		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_2_hash,
			vec![genesis_hash]
		));

		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_3_hash,
			vec![genesis_hash]
		));

		// Check that we have two tips after the fork (before merge)
		System::set_block_number(1);
		let tips_after_fork = DagConsensus::dag_tips();
		assert_eq!(tips_after_fork.len(), 2);
		assert!(tips_after_fork.contains(&block_2_hash));
		assert!(tips_after_fork.contains(&block_3_hash));

		// Add block that references both (merge)
		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_4_hash,
			vec![block_2_hash, block_3_hash]
		));

		// After merge, should have one tip
		System::set_block_number(2);
		let tips_after_merge = DagConsensus::dag_tips();
		assert_eq!(tips_after_merge.len(), 1);
		assert!(tips_after_merge.contains(&block_4_hash));

		// Check block 4 has mergeset
		let block_4_data = DagConsensus::ghostdag_data(&block_4_hash).unwrap();
		assert!(block_4_data.mergeset_blues.len() > 0 || block_4_data.mergeset_reds.len() > 0);
	});
}

#[test]
fn reachability_works() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);
		let block_2_hash = hash_from_u64(2);
		let block_3_hash = hash_from_u64(3);

		// Add linear chain
		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_2_hash,
			vec![genesis_hash]
		));

		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_3_hash,
			vec![block_2_hash]
		));

		// Test reachability
		assert!(DagConsensus::is_dag_ancestor_of(genesis_hash, block_2_hash));
		assert!(DagConsensus::is_dag_ancestor_of(genesis_hash, block_3_hash));
		assert!(DagConsensus::is_dag_ancestor_of(block_2_hash, block_3_hash));

		// Test negative cases
		assert!(!DagConsensus::is_dag_ancestor_of(block_2_hash, genesis_hash));
		assert!(!DagConsensus::is_dag_ancestor_of(block_3_hash, block_2_hash));
	});
}

#[test]
fn virtual_state_updates() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);
		let block_2_hash = hash_from_u64(2);

		// Add genesis
		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		let virtual_state_1 = DagConsensus::virtual_state();
		assert_eq!(virtual_state_1.selected_parent, genesis_hash);

		// Add another block
		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_2_hash,
			vec![genesis_hash]
		));

		let virtual_state_2 = DagConsensus::virtual_state();
		assert_eq!(virtual_state_2.selected_parent, block_2_hash);
		assert!(virtual_state_2.blue_work > virtual_state_1.blue_work);
	});
}

#[test]
fn duplicate_block_fails() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);

		// Add genesis
		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		// Try to add same block again
		assert_noop!(
			DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]),
			Error::<Test>::BlockAlreadyExists
		);
	});
}

#[test]
fn too_many_parents_fails() {
	new_test_ext().execute_with(|| {
		let _genesis_hash = hash_from_u64(1);
		let new_block_hash = hash_from_u64(2);

		// Create more parents than allowed
		let mut too_many_parents = Vec::new();
		for i in 0..15u64 {
			// More than MaxBlockParents (10)
			too_many_parents.push(hash_from_u64(i + 10));
		}

		assert_noop!(
			DagConsensus::add_block_to_dag(
				RuntimeOrigin::signed(1),
				new_block_hash,
				too_many_parents
			),
			Error::<Test>::TooManyParents
		);
	});
}

#[test]
fn invalid_parent_fails() {
	new_test_ext().execute_with(|| {
		let nonexistent_parent = hash_from_u64(999);
		let new_block_hash = hash_from_u64(1);

		assert_noop!(
			DagConsensus::add_block_to_dag(
				RuntimeOrigin::signed(1),
				new_block_hash,
				vec![nonexistent_parent]
			),
			Error::<Test>::InvalidParent
		);
	});
}

#[test]
fn events_emitted() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);

		System::set_block_number(1);

		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		// Check that events were emitted
		let events = System::events();
		assert_eq!(events.len(), 3); // DagTipsUpdated + VirtualStateUpdated + BlockAddedToDAG

		// Check BlockAddedToDAG event (it's the last event emitted)
		let block_added_event = &events[2];
		assert!(matches!(
			block_added_event.event,
			RuntimeEvent::DagConsensus(Event::BlockAddedToDAG { block_hash, .. })
			if block_hash == genesis_hash
		));
	});
}

#[test]
fn blue_work_calculation() {
	new_test_ext().execute_with(|| {
		let genesis_hash = hash_from_u64(1);
		let block_2_hash = hash_from_u64(2);

		// Add genesis
		assert_ok!(DagConsensus::add_block_to_dag(RuntimeOrigin::signed(1), genesis_hash, vec![]));

		let genesis_work = DagConsensus::blue_work(&genesis_hash);
		assert_eq!(genesis_work, 0);

		// Add child block
		assert_ok!(DagConsensus::add_block_to_dag(
			RuntimeOrigin::signed(1),
			block_2_hash,
			vec![genesis_hash]
		));

		let block_2_work = DagConsensus::blue_work(&block_2_hash);
		assert!(block_2_work > genesis_work);
	});
}
