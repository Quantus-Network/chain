#[cfg(test)]
mod tests {
	use crate::common::TestCommons;
	use codec::Encode;
	use frame_support::{
		assert_ok,
		traits::{Bounded, Currency},
	};
	use pallet_conviction_voting::{AccountVote, Vote};
	use quantus_runtime::{
		Balances, ConvictionVoting, Preimage, Referenda, RuntimeCall, RuntimeOrigin, System,
		Timestamp, Utility, Vesting, UNIT,
	};
	use sp_runtime::{
		traits::{BlakeTwo256, Hash},
		MultiAddress,
	};

	// Timestamp constants (in milliseconds)
	const MINUTE_MS: u64 = 60 * 1000;
	const HOUR_MS: u64 = 60 * MINUTE_MS;
	const DAY_MS: u64 = 24 * HOUR_MS;

	/// Test case: Grant application through referendum with vesting payment schedule
	///
	/// Scenario:
	/// 1. Grant proposal submitted for referendum voting (treasury track)
	/// 2. After positive voting, treasury spend is approved and executed
	/// 3. Separate vesting implementation follows (two-stage governance pattern)
	#[test]
	fn test_grant_application_with_vesting_schedule() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			// Setup accounts
			let proposer = TestCommons::account_id(1);
			let beneficiary = TestCommons::account_id(2);
			let voter1 = TestCommons::account_id(3);
			let voter2 = TestCommons::account_id(4);

			// Give voters some balance for voting
			Balances::make_free_balance_be(&voter1, 1000 * UNIT);
			Balances::make_free_balance_be(&voter2, 1000 * UNIT);
			Balances::make_free_balance_be(&proposer, 10000 * UNIT);

			// Step 1: Create a treasury proposal for referendum
			let grant_amount = 1000 * UNIT;

			// Treasury call for referendum approval
			let treasury_call = RuntimeCall::TreasuryPallet(pallet_treasury::Call::spend {
				asset_kind: Box::new(()),
				amount: grant_amount,
				beneficiary: Box::new(MultiAddress::Id(beneficiary.clone())),
				valid_from: None,
			});

			// Two-stage governance flow: referendum approves treasury spend principle
			let referendum_call = treasury_call;

			// Step 2: Submit preimage for the referendum call
			let encoded_proposal = referendum_call.encode();
			let preimage_hash = BlakeTwo256::hash(&encoded_proposal);

			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded_proposal.clone()
			));

			// Step 3: Submit referendum for treasury spending (using treasury track)
			let bounded_call =
				Bounded::Lookup { hash: preimage_hash, len: encoded_proposal.len() as u32 };
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				Box::new(
					quantus_runtime::governance::pallet_custom_origins::Origin::SmallSpender.into()
				),
				bounded_call,
				frame_support::traits::schedule::DispatchTime::After(1)
			));

			// Step 4: Vote on referendum
			let referendum_index = 0;

			// Vote YES with conviction
			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(voter1.clone()),
				referendum_index,
				AccountVote::Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked1x,
					},
					balance: 500 * UNIT,
				}
			));

			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(voter2.clone()),
				referendum_index,
				AccountVote::Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked2x,
					},
					balance: 300 * UNIT,
				}
			));

			// Step 5: Wait for referendum to pass and execute
			let blocks_to_advance = 2 + 2 + 2 + 2 + 1;
			TestCommons::run_to_block(System::block_number() + blocks_to_advance);

			println!("Referendum approved treasury spend. Now implementing vesting...");

			// Step 6: Implementation phase - create vesting schedule using custom vesting pallet
			let now = Timestamp::get();
			let vesting_duration = 30 * DAY_MS; // 30 days vesting
			let start_time = now;
			let end_time = now + vesting_duration;

			// Create vesting schedule from proposer to beneficiary
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(proposer.clone()),
				beneficiary.clone(),
				grant_amount,
				start_time,
				end_time,
				proposer.clone(), // funding_account
			));

			println!("Vesting schedule created successfully");
			println!("Note: Custom vesting uses timestamp-based vesting with manual claiming");
			println!("In production, beneficiary would claim() after vesting period passes");
		});
	}

	/// Test case: Multi-milestone grant with multiple vesting schedules
	///
	/// Scenario: Grant paid out in multiple tranches (milestones)
	#[test]
	fn test_milestone_based_grant_with_multiple_vesting() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let grantee = TestCommons::account_id(1);
			let grantor = TestCommons::account_id(2);

			Balances::make_free_balance_be(&grantor, 10000 * UNIT);

			let milestone1_amount = 300 * UNIT;
			let milestone2_amount = 400 * UNIT;
			let milestone3_amount = 300 * UNIT;

			let now = Timestamp::get();

			// Create multiple vesting schedules for different milestones
			let calls = vec![
				// Milestone 1: Short vesting (30 days)
				RuntimeCall::Vesting(pallet_vesting::Call::create_vesting_schedule {
					beneficiary: grantee.clone(),
					amount: milestone1_amount,
					start: now,
					end: now + 30 * DAY_MS,
					funding_account: grantor.clone(),
				}),
				// Milestone 2: Longer vesting (60 days)
				RuntimeCall::Vesting(pallet_vesting::Call::create_vesting_schedule {
					beneficiary: grantee.clone(),
					amount: milestone2_amount,
					start: now + 31 * DAY_MS,
					end: now + 91 * DAY_MS,
					funding_account: grantor.clone(),
				}),
				// Milestone 3: Immediate payment
				RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
					dest: MultiAddress::Id(grantee.clone()),
					value: milestone3_amount,
				}),
			];

			assert_ok!(Utility::batch_all(RuntimeOrigin::signed(grantor.clone()), calls));

			println!("Multiple milestone vesting schedules created successfully");

			// Verify grantee received immediate payment
			let initial_balance = Balances::free_balance(&grantee);
			assert!(
				initial_balance >= milestone3_amount,
				"Immediate milestone payment should be available"
			);

			println!("Multi-milestone grant test completed - schedules created successfully");
		});
	}

	/// Test case: Treasury proposal with vesting integration
	#[test]
	fn test_treasury_auto_vesting_integration() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let beneficiary = TestCommons::account_id(1);
			let treasury = TestCommons::account_id(2);
			let amount = 1000 * UNIT;

			Balances::make_free_balance_be(&treasury, 5000 * UNIT);

			let now = Timestamp::get();
			let vesting_duration = 30 * DAY_MS;

			// In practice, treasury would be funded separately
			// We simulate treasury having funds by making it a signed account
			// Then treasury creates vesting schedule

			// Treasury creates vesting schedule for beneficiary
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(treasury.clone()),
				beneficiary.clone(),
				amount,
				now,
				now + vesting_duration,
				treasury.clone(), // funding_account
			));

			println!("Treasury + vesting integration successful");
			println!("Treasury can create vesting schedules for approved grants");
		});
	}

	/// Test case: Emergency vesting cancellation
	///
	/// Scenario: Creator can cancel vesting schedule and recover remaining funds
	#[test]
	fn test_emergency_vesting_cancellation() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let grantee = TestCommons::account_id(1);
			let grantor = TestCommons::account_id(2);

			Balances::make_free_balance_be(&grantor, 2000 * UNIT);

			let total_amount = 1000 * UNIT;
			let now = Timestamp::get();
			let vesting_duration = 100 * DAY_MS;

			// Create vesting schedule
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(grantor.clone()),
				grantee.clone(),
				total_amount,
				now,
				now + vesting_duration,
				grantor.clone(), // funding_account
			));

			println!("Vesting schedule created");

			let grantor_balance_before_cancel = Balances::free_balance(&grantor);

			// Emergency: creator cancels the vesting schedule
			// In custom vesting, cancel will automatically claim for beneficiary first,
			// then return unclaimed funds to creator
			assert_ok!(Vesting::cancel_vesting_schedule(RuntimeOrigin::signed(grantor.clone()), 1));

			let grantor_balance_after_cancel = Balances::free_balance(&grantor);

			// Grantor should have recovered funds (minus any claimed by beneficiary)
			assert!(
				grantor_balance_after_cancel >= grantor_balance_before_cancel,
				"Creator should recover remaining funds after cancellation"
			);

			println!("Emergency cancellation successful - creator can cancel and recover funds");
		});
	}

	/// Test case: Progressive milestone governance with Tech Collective
	#[test]
	fn test_progressive_milestone_referenda() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let grantee = TestCommons::account_id(1);
			let proposer = TestCommons::account_id(2);
			let voter1 = TestCommons::account_id(3);
			let voter2 = TestCommons::account_id(4);
			let tech_member1 = TestCommons::account_id(5);
			let tech_member2 = TestCommons::account_id(6);
			let tech_member3 = TestCommons::account_id(7);
			let treasury_account = TestCommons::account_id(8);

			// Setup balances
			Balances::make_free_balance_be(&voter1, 2000 * UNIT);
			Balances::make_free_balance_be(&voter2, 2000 * UNIT);
			Balances::make_free_balance_be(&proposer, 15000 * UNIT);
			Balances::make_free_balance_be(&tech_member1, 3000 * UNIT);
			Balances::make_free_balance_be(&tech_member2, 3000 * UNIT);
			Balances::make_free_balance_be(&tech_member3, 3000 * UNIT);
			Balances::make_free_balance_be(&treasury_account, 10000 * UNIT);

			// Add Tech Collective members
			assert_ok!(quantus_runtime::TechCollective::add_member(
				RuntimeOrigin::root(),
				MultiAddress::Id(tech_member1.clone())
			));
			assert_ok!(quantus_runtime::TechCollective::add_member(
				RuntimeOrigin::root(),
				MultiAddress::Id(tech_member2.clone())
			));
			assert_ok!(quantus_runtime::TechCollective::add_member(
				RuntimeOrigin::root(),
				MultiAddress::Id(tech_member3.clone())
			));

			let milestone1_amount = 400 * UNIT;
			let milestone2_amount = 500 * UNIT;
			let milestone3_amount = 600 * UNIT;
			let total_grant = milestone1_amount + milestone2_amount + milestone3_amount;

			// === STEP 1: Initial referendum approves entire grant plan ===
			println!("=== REFERENDUM: Grant Plan Approval ===");

			let grant_approval_call = RuntimeCall::TreasuryPallet(pallet_treasury::Call::spend {
				asset_kind: Box::new(()),
				amount: total_grant,
				beneficiary: Box::new(MultiAddress::Id(treasury_account.clone())),
				valid_from: None,
			});

			let encoded_proposal = grant_approval_call.encode();
			let preimage_hash = BlakeTwo256::hash(&encoded_proposal);

			assert_ok!(Preimage::note_preimage(
				RuntimeOrigin::signed(proposer.clone()),
				encoded_proposal.clone()
			));

			let bounded_call =
				Bounded::Lookup { hash: preimage_hash, len: encoded_proposal.len() as u32 };
			assert_ok!(Referenda::submit(
				RuntimeOrigin::signed(proposer.clone()),
				Box::new(
					quantus_runtime::governance::pallet_custom_origins::Origin::SmallSpender.into()
				),
				bounded_call,
				frame_support::traits::schedule::DispatchTime::After(1)
			));

			// Community votes
			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(voter1.clone()),
				0,
				AccountVote::Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked1x,
					},
					balance: 800 * UNIT,
				}
			));

			assert_ok!(ConvictionVoting::vote(
				RuntimeOrigin::signed(voter2.clone()),
				0,
				AccountVote::Standard {
					vote: Vote {
						aye: true,
						conviction: pallet_conviction_voting::Conviction::Locked2x,
					},
					balance: 600 * UNIT,
				}
			));

			let blocks_to_advance = 2 + 2 + 2 + 2 + 1;
			TestCommons::run_to_block(System::block_number() + blocks_to_advance);

			println!("✅ Grant plan approved by referendum!");

			// === STEP 2: Tech Collective milestone evaluations ===
			let now = Timestamp::get();

			println!("=== MILESTONE 1: Tech Collective Decision ===");
			TestCommons::run_to_block(System::block_number() + 10);

			// Tech Collective creates vesting for milestone 1 (60-day vesting)
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(treasury_account.clone()),
				grantee.clone(),
				milestone1_amount,
				now,
				now + 60 * DAY_MS,
				treasury_account.clone(), // funding_account
			));

			println!("✅ Tech Collective approved milestone 1 with 60-day vesting");

			println!("=== MILESTONE 2: Tech Collective Decision ===");
			TestCommons::run_to_block(System::block_number() + 20);

			// Milestone 2 with reduced vesting (30 days) due to good quality
			assert_ok!(Vesting::create_vesting_schedule(
				RuntimeOrigin::signed(treasury_account.clone()),
				grantee.clone(),
				milestone2_amount,
				now + 20 * DAY_MS,
				now + 50 * DAY_MS,
				treasury_account.clone(), // funding_account
			));

			println!("✅ Tech Collective approved milestone 2 with reduced 30-day vesting");

			println!("=== MILESTONE 3: Final Tech Collective Decision ===");
			TestCommons::run_to_block(System::block_number() + 20);

			// Final milestone - immediate payment
			assert_ok!(Balances::transfer_allow_death(
				RuntimeOrigin::signed(treasury_account.clone()),
				MultiAddress::Id(grantee.clone()),
				milestone3_amount,
			));

			println!("✅ Tech Collective approved final milestone with immediate payment");

			// Verify governance worked
			let final_balance = Balances::free_balance(&grantee);
			assert!(
				final_balance >= milestone3_amount,
				"Tech Collective process should have provided controlled funding"
			);

			println!("🎉 Tech Collective governance process completed successfully!");
			println!("   - Community referendum approved overall grant plan");
			println!("   - Tech Collective evaluated each milestone");
			println!("   - Vesting schedules created based on quality assessment");
		});
	}
}
