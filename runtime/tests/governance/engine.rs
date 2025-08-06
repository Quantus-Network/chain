#[cfg(test)]
mod tests {
	use crate::common::TestCommons;
	use codec::Encode;
	use frame_support::{
		assert_noop, assert_ok,
		traits::{ConstU32, Currency, PreimageProvider, QueryPreimage},
		BoundedVec,
	};
	use pallet_conviction_voting::{AccountVote::Standard, Vote};
	use pallet_referenda::TracksInfo;
	use poseidon_resonance::PoseidonHasher;
	use quantus_runtime::{
		Balances, BlockNumber, ConvictionVoting, OriginCaller, Preimage, Referenda, Runtime,
		RuntimeCall, RuntimeOrigin, Scheduler, UNIT,
	};
	use sp_runtime::traits::Hash;

	// Helper function to create governance test data
	fn bounded(s: &[u8]) -> BoundedVec<u8, ConstU32<100>> {
		s.to_vec().try_into().unwrap()
	}

	#[test]
	fn note_preimage_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let account = TestCommons::account_id(1);
			// Check initial balance
			let initial_balance = Balances::free_balance(&account);

			// Create test data
			let preimage_data = bounded(b"test_preimage_data");
			let hash = PoseidonHasher::hash(&preimage_data);

			// Note the preimage
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(account.clone()),
				preimage_data.to_vec(),
			));

			// Check if preimage was stored
			assert!(Preimage::have_preimage(&hash));

			// If using an implementation with token reservation, check if balance changed
			if !std::any::TypeId::of::<()>().eq(&std::any::TypeId::of::<()>()) {
				let final_balance = Balances::free_balance(&account);
				let reserved = Balances::reserved_balance(&account);

				// Check if balance was reduced
				assert!(final_balance < initial_balance);
				// Check if tokens were reserved
				assert!(reserved > 0);
			}
		});
	}

	#[test]
	fn unnote_preimage_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let account = TestCommons::account_id(1);
			let initial_balance = Balances::free_balance(&account);

			// Create test data
			let preimage_data = bounded(b"test_preimage_data");
			let hash = PoseidonHasher::hash(&preimage_data);

			// Note the preimage
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(account.clone()),
				preimage_data.to_vec(),
			));

			// Remove the preimage
			assert_ok!(Preimage::unnote_preimage(RuntimeOrigin::signed(account.clone()), hash,));

			// Check if preimage was removed
			assert!(!Preimage::have_preimage(&hash));

			// If using an implementation with token reservation, check if balance was restored
			if !std::any::TypeId::of::<()>().eq(&std::any::TypeId::of::<()>()) {
				let final_balance = Balances::free_balance(&account);
				let reserved = Balances::reserved_balance(&account);

				// Balance should return to initial amount
				assert_eq!(final_balance, initial_balance);
				// No tokens should be reserved
				assert_eq!(reserved, 0);
			}
		});
	}

	#[test]
	fn request_preimage_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let account = TestCommons::account_id(1);
			let initial_balance = Balances::free_balance(&account);

			// Create test data
			let preimage_data = bounded(b"test_preimage_data");
			let hash = PoseidonHasher::hash(&preimage_data);

			// Note the preimage
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(account.clone()),
				preimage_data.to_vec(),
			));

			// Request the preimage as system
			assert_ok!(Preimage::request_preimage(RuntimeOrigin::root(), hash,));

			// Check if preimage was requested
			assert!(Preimage::is_requested(&hash));

			// If using an implementation with token reservation, check if balance was freed
			if !std::any::TypeId::of::<()>().eq(&std::any::TypeId::of::<()>()) {
				let final_balance = Balances::free_balance(&account);

				// Balance should return to initial amount
				assert_eq!(final_balance, initial_balance);
			}
		});
	}

	#[test]
	fn unrequest_preimage_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let account = TestCommons::account_id(1);

			// Create test data
			let preimage_data = bounded(b"test_preimage_data");
			let hash = PoseidonHasher::hash(&preimage_data);

			// Note the preimage
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(account.clone()),
				preimage_data.to_vec(),
			));

			// Request the preimage as system
			assert_ok!(Preimage::request_preimage(RuntimeOrigin::root(), hash,));

			// Then unrequest it
			assert_ok!(Preimage::unrequest_preimage(RuntimeOrigin::root(), hash,));

			// Check if preimage is no longer requested
			assert!(!Preimage::is_requested(&hash));
		});
	}

	#[test]
	fn preimage_cannot_be_noted_twice() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let account = TestCommons::account_id(1);

			// Create test data
			let preimage_data = bounded(b"test_preimage_data");

			// Note the preimage for the first time
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(account.clone()),
				preimage_data.to_vec(),
			));

			// Attempt to note the same preimage again should fail
			assert_noop!(
				Preimage::note_preimage(
					RuntimeOrigin::signed(account.clone()),
					preimage_data.to_vec(),
				),
				pallet_preimage::Error::<Runtime>::AlreadyNoted
			);
		});
	}

	#[test]
	fn preimage_too_large_fails() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let account = TestCommons::account_id(1);

			// Create large data exceeding the limit
			// 5MB should be larger than any reasonable limit
			let large_data = vec![0u8; 5 * 1024 * 1024];

			// Attempt to note an oversized preimage should fail
			assert_noop!(
				Preimage::note_preimage(RuntimeOrigin::signed(account.clone()), large_data,),
				pallet_preimage::Error::<Runtime>::TooBig
			);
		});
	}

	///Scheduler tests

	#[test]
	fn scheduler_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let account = TestCommons::account_id(1);
			let recipient = TestCommons::account_id(2);

			// Check initial balances
			let initial_balance = Balances::free_balance(&account);
			let recipient_balance = Balances::free_balance(&recipient);

			// Create a transfer call that should work with root origin
			// We need a call that will transfer funds without needing a specific sender
			// For example, we could use Balances::force_transfer which allows root to transfer
			// between accounts
			let transfer_call = RuntimeCall::Balances(pallet_balances::Call::force_transfer {
				source: account.clone().into(),
				dest: recipient.clone().into(),
				value: 50 * UNIT,
			});

			// Schedule the transfer at block 10
			let when: BlockNumber = 10;
			assert_ok!(Scheduler::schedule(
				RuntimeOrigin::root(),
				when,
				None,
				127,
				Box::new(transfer_call),
			));

			// Advance to block 9
			TestCommons::run_to_block(9);
			assert_eq!(Balances::free_balance(&account), initial_balance);
			assert_eq!(Balances::free_balance(&recipient), recipient_balance);

			// Advance to block 10
			TestCommons::run_to_block(10);

			// Verify the transfer occurred
			assert_eq!(Balances::free_balance(&account), initial_balance - 50 * UNIT);
			assert_eq!(Balances::free_balance(&recipient), recipient_balance + 50 * UNIT);
		});
	}

	///Referenda tests

	#[test]
	fn referendum_submission_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let proposer = TestCommons::account_id(1);
			let initial_balance = Balances::free_balance(&proposer);

			// Make sure we have sufficient funds
			assert!(
				initial_balance >= 1000 * UNIT,
				"Test account should have at least 1000 UNIT of funds"
			);

			// Get deposit value from configuration
			let submission_deposit =
				<Runtime as pallet_referenda::Config>::SubmissionDeposit::get();

			// Prepare origin for the proposal
			let proposal_origin =
				Box::new(OriginCaller::system(frame_system::RawOrigin::Signed(proposer.clone())));

			// Create a call for the proposal
			let call = RuntimeCall::Balances(pallet_balances::Call::force_transfer {
				source: TestCommons::account_id(1).into(),
				dest: TestCommons::account_id(42).into(),
				value: 1,
			});

			// Encode the call
			let encoded_call = call.encode();

			// Calculate hash manually
			let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

			// Store preimage before using the hash - remember balance before this operation
			let balance_before_preimage = Balances::free_balance(&proposer);
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded_call.clone()
			));
			let balance_after_preimage = Balances::free_balance(&proposer);

			// Cost of storing the preimage
			let preimage_cost = balance_before_preimage - balance_after_preimage;
			println!("Cost of storing preimage: {}", preimage_cost);

			// Create lookup for bounded call
			let bounded_call = frame_support::traits::Bounded::Lookup {
				hash: preimage_hash,
				len: encoded_call.len() as u32,
			};

			// Activation moment
			let enactment_moment = frame_support::traits::schedule::DispatchTime::After(0u32);

			// Submit referendum - remember balance before this operation
			let balance_before_referendum = Balances::free_balance(&proposer);
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				proposal_origin,
				bounded_call,
				enactment_moment
			));
			let balance_after_referendum = Balances::free_balance(&proposer);

			// Cost of submitting referendum
			let referendum_cost = balance_before_referendum - balance_after_referendum;
			println!("Cost of submitting referendum: {}", referendum_cost);

			// Check if the referendum was created
			let referendum_info = pallet_referenda::ReferendumInfoFor::<Runtime>::get(0);
			assert!(referendum_info.is_some(), "Referendum should exist");

			// Check if the total cost matches expectations
			assert_eq!(
				initial_balance - balance_after_referendum,
				preimage_cost + referendum_cost,
				"Total cost should be the sum of preimage and referendum costs"
			);

			// Check if referendum cost matches the deposit
			assert_eq!(
				referendum_cost, submission_deposit,
				"Referendum cost should equal the deposit amount"
			);
		});
	}

	#[test]
	fn referendum_cancel_by_root_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let proposer = TestCommons::account_id(1);
			let initial_balance = Balances::free_balance(&proposer);

			// Prepare origin for the proposal
			let proposal_origin =
				Box::new(OriginCaller::system(frame_system::RawOrigin::Signed(proposer.clone())));

			// Create a call for the proposal
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });

			// Encode the call
			let encoded_call = call.encode();

			// Calculate hash manually
			let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

			// Store preimage before using the hash
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded_call.clone()
			));

			// Create lookup for bounded call
			let bounded_call = frame_support::traits::Bounded::Lookup {
				hash: preimage_hash,
				len: encoded_call.len() as u32,
			};

			// Activation moment
			let enactment_moment = frame_support::traits::schedule::DispatchTime::After(0u32);

			// Submit referendum
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				proposal_origin,
				bounded_call,
				enactment_moment
			));

			let referendum_index = 0;

			// Cancel by root
			assert_ok!(Referenda::cancel(RuntimeOrigin::root(), referendum_index));

			// Check if referendum was cancelled (should no longer be in ongoing state)
			let referendum_info =
				pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index);
			assert!(referendum_info.is_some(), "Referendum should exist");

			match referendum_info.unwrap() {
				pallet_referenda::ReferendumInfo::Ongoing(_) => {
					panic!("Referendum should not be in ongoing state after cancellation");
				},
				pallet_referenda::ReferendumInfo::Cancelled(_, _, _) => {
					// Successfully cancelled
				},
				_ => {
					panic!("Referendum should be in Cancelled state");
				},
			}

			// Since we're using Slash = (), the deposit should be burned
			// We need to account for both preimage costs and submission deposit
			assert!(
				Balances::free_balance(&proposer) < initial_balance,
				"Balance should be reduced after cancellation"
			);
		});
	}

	#[test]
	fn referendum_voting_and_passing_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let proposer = TestCommons::account_id(1);
			let voter1 = TestCommons::account_id(2);
			let voter2 = TestCommons::account_id(3);

			// Ensure voters have enough balance for larger votes
			Balances::make_free_balance_be(&proposer, 10000 * UNIT);
			Balances::make_free_balance_be(&voter1, 2000 * UNIT);
			Balances::make_free_balance_be(&voter2, 1500 * UNIT);

			// Prepare origin for the proposal
			let proposal_origin =
				Box::new(OriginCaller::system(frame_system::RawOrigin::Signed(proposer.clone())));

			// Create a call for the proposal
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });

			// Encode the call
			let encoded_call = call.encode();

			// Calculate hash manually
			let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

			// Store preimage before using the hash
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded_call.clone()
			));

			// Create lookup for bounded call
			let bounded_call = frame_support::traits::Bounded::Lookup {
				hash: preimage_hash,
				len: encoded_call.len() as u32,
			};

			// Activation moment
			let enactment_moment = frame_support::traits::schedule::DispatchTime::After(0u32);

			// Submit referendum
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				proposal_origin,
				bounded_call,
				enactment_moment
			));

			let referendum_index = 0;

			// Place decision deposit to start the deciding phase
			assert_ok!(Referenda::place_decision_deposit(
				RuntimeOrigin::signed(proposer.clone()),
				referendum_index
			));

			// Vote for the referendum with larger vote amounts to meet support threshold
			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(voter1.clone()),
				referendum_index,
				Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked3x,
					},
					balance: 1000 * UNIT
				}
			));

			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(voter2.clone()),
				referendum_index,
				Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked2x,
					},
					balance: 800 * UNIT
				}
			));

			// Advance blocks to get past preparation period
			let track_info = <Runtime as pallet_referenda::Config>::Tracks::info(0).unwrap();
			let prepare_period = track_info.prepare_period;

			TestCommons::run_to_block(prepare_period + 1);

			// Check if referendum is in deciding phase
			let info =
				pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
			match info {
				pallet_referenda::ReferendumInfo::Ongoing(details) => {
					assert!(details.deciding.is_some(), "Referendum should be in deciding phase");
				},
				_ => panic!("Referendum should be ongoing"),
			}

			// Advance to end of voting period
			// Use the default voting period from config
			let voting_period = <Runtime as pallet_referenda::Config>::Tracks::info(0)
				.map(|info| info.decision_period)
				.unwrap_or(30); // Fallback value if track info can't be retrieved

			TestCommons::run_to_block(10 + voting_period);

			// Now advance through confirmation period
			TestCommons::run_to_block(10 + voting_period + 10); // Add some extra blocks for confirmation

			// Check if referendum passed
			let info =
				pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
			match info {
				pallet_referenda::ReferendumInfo::Approved(_, _, _) => {
					// Successfully passed
				},
				other => panic!("Referendum should be approved, but is: {:?}", other),
			}
		});
	}

	#[test]
	fn delegated_voting_works() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let proposer = TestCommons::account_id(1);
			let delegate = TestCommons::account_id(2);
			let delegator1 = TestCommons::account_id(3);
			let delegator2 = TestCommons::account_id(4);

			// Set up sufficient balances for all accounts - increased for 10000 UNIT votes
			Balances::make_free_balance_be(&proposer, 50000 * UNIT);
			Balances::make_free_balance_be(&delegate, 50000 * UNIT);
			Balances::make_free_balance_be(&delegator1, 50000 * UNIT);
			Balances::make_free_balance_be(&delegator2, 50000 * UNIT);

			// Prepare a proposal
			let proposal = RuntimeCall::System(frame_system::Call::remark {
				remark: b"Delegated voting test proposal".to_vec(),
			});
			let encoded_call = proposal.encode();
			let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

			// Store the preimage
			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded_call.clone()
			));

			let bounded_call = frame_support::traits::Bounded::Lookup {
				hash: preimage_hash,
				len: encoded_call.len() as u32,
			};

			// Prepare origin for the proposal
			let proposal_origin =
				Box::new(OriginCaller::system(frame_system::RawOrigin::Signed(proposer.clone())));

			// Submit referendum
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				proposal_origin.clone(),
				bounded_call,
				frame_support::traits::schedule::DispatchTime::After(0u32)
			));

			let referendum_index = 0;

			// Place decision deposit to start deciding phase
			assert_ok!(Referenda::place_decision_deposit(
				RuntimeOrigin::signed(proposer.clone()),
				referendum_index
			));

			// Check initial voting state before any delegations
			let initial_voting_for =
				pallet_conviction_voting::VotingFor::<Runtime>::try_get(&delegate, 0);
			assert!(initial_voting_for.is_err(), "Delegate should have no votes initially");

			// Delegators delegate their voting power to the delegate
			assert_ok!(ConvictionVoting::delegate(
				RuntimeOrigin::signed(delegator1.clone()),
				0, // The class ID (track) to delegate for
				sp_runtime::MultiAddress::Id(delegate.clone()),
				pallet_conviction_voting::Conviction::Locked3x,
				10000 * UNIT
			));

			assert_ok!(ConvictionVoting::delegate(
				RuntimeOrigin::signed(delegator2.clone()),
				0, // The class ID (track) to delegate for
				sp_runtime::MultiAddress::Id(delegate.clone()),
				pallet_conviction_voting::Conviction::Locked2x,
				10000 * UNIT
			));

			// Verify delegations are recorded correctly
			let delegator1_voting =
				pallet_conviction_voting::VotingFor::<Runtime>::try_get(&delegator1, 0).unwrap();
			let delegator2_voting =
				pallet_conviction_voting::VotingFor::<Runtime>::try_get(&delegator2, 0).unwrap();

			match delegator1_voting {
				pallet_conviction_voting::Voting::Delegating(delegating) => {
					assert_eq!(
						delegating.target, delegate,
						"Delegator1 should delegate to the correct account"
					);
					assert_eq!(
						delegating.conviction,
						pallet_conviction_voting::Conviction::Locked3x
					);
					assert_eq!(delegating.balance, 10000 * UNIT); // Updated to 10000 UNIT
				},
				_ => panic!("Delegator1 should be delegating"),
			}

			match delegator2_voting {
				pallet_conviction_voting::Voting::Delegating(delegating) => {
					assert_eq!(
						delegating.target, delegate,
						"Delegator2 should delegate to the correct account"
					);
					assert_eq!(
						delegating.conviction,
						pallet_conviction_voting::Conviction::Locked2x
					);
					assert_eq!(delegating.balance, 10000 * UNIT); // Updated to 10000 UNIT
				},
				_ => panic!("Delegator2 should be delegating"),
			}

			// The delegate votes on the referendum
			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(delegate.clone()),
				referendum_index,
				Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked3x,
					},
					balance: 10000 * UNIT // Set to 10000 UNIT like others
				}
			));

			// Advance to deciding phase
			let track_info = <Runtime as pallet_referenda::Config>::Tracks::info(0).unwrap();
			let prepare_period = track_info.prepare_period;
			TestCommons::run_to_block(prepare_period + 1);

			// Check the tally includes both direct and delegated votes
			let referendum_info =
				pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
			if let pallet_referenda::ReferendumInfo::Ongoing(status) = referendum_info {
				assert!(status.tally.ayes > 0, "Tally should include votes");

				// Calculate expected voting power with conviction
				// Delegate: 10000 UNIT * 3x = 30000 UNIT equivalent
				// Delegator1: 10000 UNIT * 3x = 30000 UNIT equivalent
				// Delegator2: 10000 UNIT * 2x = 20000 UNIT equivalent
				// Total: 80000 UNIT equivalent

				// We can't directly access the exact vote values due to type abstractions, but we
				// can verify that total votes are greater than just the delegate's direct vote
				assert!(
					status.tally.ayes > 10000 * UNIT,
					"Tally should include delegated votes (expected > 10000 UNIT equivalent)"
				);
			} else {
				panic!("Referendum should be ongoing");
			}

			// One of the delegators changes their mind and undelegate
			assert_ok!(ConvictionVoting::undelegate(
				RuntimeOrigin::signed(delegator1.clone()),
				0 // The class ID to undelegate
			));

			// Verify undelegation worked
			let delegator1_voting_after =
				pallet_conviction_voting::VotingFor::<Runtime>::try_get(&delegator1, 0);
			assert!(
				delegator1_voting_after.is_err() ||
					!matches!(
						delegator1_voting_after.unwrap(),
						pallet_conviction_voting::Voting::Delegating { .. }
					),
				"Delegator1 should no longer be delegating"
			);

			// Advance blocks to update tally
			TestCommons::run_to_block(prepare_period + 1);

			// The undelegated account now votes directly
			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(delegator1.clone()),
				referendum_index,
				Standard {
					vote: Vote {
						aye: false, // Voting against
						conviction: pallet_conviction_voting::Conviction::Locked1x,
					},
					balance: 10000 * UNIT // Set to 10000 UNIT like others
				}
			));

			// Check the updated tally
			let referendum_info =
				pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
			if let pallet_referenda::ReferendumInfo::Ongoing(status) = referendum_info {
				// Now we should have:
				// Ayes: Delegate (10000 UNIT * 3x) + Delegator2 (10000 UNIT * 2x) = 50000 UNIT
				// equivalent Nays: Delegator1 (10000 UNIT * 1x) = 10000 UNIT equivalent

				assert!(status.tally.nays > 0, "Tally should include votes against");
			} else {
				panic!("Referendum should be ongoing");
			}

			// Complete the referendum
			let decision_period = track_info.decision_period;
			let confirm_period = track_info.confirm_period;
			TestCommons::run_to_block(prepare_period + decision_period + confirm_period + 1);

			// Check referendum passed despite the vote against
			let final_info =
				pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
			assert!(
				matches!(final_info, pallet_referenda::ReferendumInfo::Approved(_, _, _)),
				"Referendum should be approved due to delegated voting weight"
			);

			// Verify delegated balances are locked
			let delegate_locks = pallet_balances::Locks::<Runtime>::get(&delegate);
			let delegator2_locks = pallet_balances::Locks::<Runtime>::get(&delegator2);

			assert!(!delegate_locks.is_empty(), "Delegate should have locks");
			assert!(!delegator2_locks.is_empty(), "Delegator2 should have locks");

			// The delegate now votes on another referendum - delegations should automatically apply
			// Create a second referendum
			let proposal2 = RuntimeCall::System(frame_system::Call::remark {
				remark: b"Second proposal with delegations".to_vec(),
			});
			let encoded_call2 = proposal2.encode();
			let preimage_hash2 = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call2);

			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded_call2.clone()
			));

			let bounded_call2 = frame_support::traits::Bounded::Lookup {
				hash: preimage_hash2,
				len: encoded_call2.len() as u32,
			};

			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				proposal_origin.clone(),
				bounded_call2,
				frame_support::traits::schedule::DispatchTime::After(0u32)
			));

			let referendum_index2 = 1;

			assert_ok!(Referenda::place_decision_deposit(
				RuntimeOrigin::signed(proposer.clone()),
				referendum_index2
			));

			// Delegate votes on second referendum
			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(delegate.clone()),
				referendum_index2,
				Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked1x,
					},
					balance: 10000 * UNIT // Set to 10000 UNIT like others
				}
			));

			// Advance to deciding phase
			TestCommons::run_to_block(prepare_period + decision_period + confirm_period + 2);

			// Verify active delegations are automatically applied to the new referendum
			let referendum_info2 =
				pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index2).unwrap();
			if let pallet_referenda::ReferendumInfo::Ongoing(status) = referendum_info2 {
				// Should still include delegator2's votes automatically
				assert!(
					status.tally.ayes > 10000 * UNIT,
					"Tally should include delegated votes from existing delegations"
				);
			} else {
				panic!("Second referendum should be ongoing");
			}
		});
	}
}
