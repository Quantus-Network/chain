mod common;

use common::TestCommons;
use frame_support::traits::Contains;
use resonance_runtime::{configs::NoNestingCallFilter, RuntimeCall};

#[test]
fn utility_batch_works() {
    TestCommons::new_test_ext().execute_with(|| {
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1],
            })],
        });
        assert!(NoNestingCallFilter::contains(&call));
    });
}

#[test]
fn nested_utility_batch_is_disallowed() {
    TestCommons::new_test_ext().execute_with(|| {
        let inner_call = RuntimeCall::Utility(pallet_utility::Call::batch { calls: vec![] });
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![inner_call],
        });
        assert!(!NoNestingCallFilter::contains(&call));
    });
}

#[test]
fn nested_utility_force_batch_is_disallowed() {
    TestCommons::new_test_ext().execute_with(|| {
        let inner_call = RuntimeCall::Utility(pallet_utility::Call::force_batch { calls: vec![] });
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![inner_call],
        });
        assert!(!NoNestingCallFilter::contains(&call));
    });
}

#[test]
fn utility_batch_with_non_batch_utility_call_works() {
    TestCommons::new_test_ext().execute_with(|| {
        let inner_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![inner_call],
        });
        assert!(NoNestingCallFilter::contains(&call));
    });
}
