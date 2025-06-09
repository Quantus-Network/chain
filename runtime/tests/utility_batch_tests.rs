mod common;

use common::TestCommons;
use frame_support::traits::Contains;
use frame_support::weights::Weight;
use resonance_runtime::{Runtime, RuntimeCall};

#[test]
fn test_can_batch_txs() {
    TestCommons::new_test_ext().execute_with(|| {
        let bob = TestCommons::account_id(2);
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![RuntimeCall::Balances(
                pallet_balances::Call::transfer_allow_death {
                    dest: bob.into(),
                    value: 1000,
                },
            )],
        });

        assert!(<Runtime as frame_system::Config>::BaseCallFilter::contains(
            &call
        ));
    });
}

#[test]
fn test_can_batch_non_batch_utility_call() {
    TestCommons::new_test_ext().execute_with(|| {
        let remark_call = RuntimeCall::System(frame_system::Call::remark {
            remark: b"hello".to_vec(),
        });

        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![RuntimeCall::Utility(pallet_utility::Call::with_weight {
                call: Box::new(remark_call),
                weight: Weight::from_parts(10_000, 0),
            })],
        });

        assert!(<Runtime as frame_system::Config>::BaseCallFilter::contains(
            &call
        ));
    });
}

#[test]
fn test_cant_nest_batch_txs() {
    TestCommons::new_test_ext().execute_with(|| {
        let bob = TestCommons::account_id(2);
        let charlie = TestCommons::account_id(3);

        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![
                RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
                    dest: bob.into(),
                    value: 1000,
                }),
                RuntimeCall::Utility(pallet_utility::Call::batch {
                    calls: vec![RuntimeCall::Balances(
                        pallet_balances::Call::transfer_allow_death {
                            dest: charlie.into(),
                            value: 1000,
                        },
                    )],
                }),
            ],
        });

        assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
    });
}

#[test]
fn test_cant_nest_different_batch_types() {
    TestCommons::new_test_ext().execute_with(|| {
        let charlie = TestCommons::account_id(3);

        // batch in batch
        let call = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: vec![RuntimeCall::Utility(pallet_utility::Call::force_batch {
                calls: vec![RuntimeCall::Balances(
                    pallet_balances::Call::transfer_allow_death {
                        dest: charlie.clone().into(),
                        value: 1000,
                    },
                )],
            })],
        });
        assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));

        // batch_all in batch
        let call2 = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: vec![RuntimeCall::Utility(pallet_utility::Call::batch {
                calls: vec![RuntimeCall::Balances(
                    pallet_balances::Call::transfer_allow_death {
                        dest: charlie.clone().into(),
                        value: 1000,
                    },
                )],
            })],
        });
        assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call2));

        // force_batch in batch_all
        let call3 = RuntimeCall::Utility(pallet_utility::Call::force_batch {
            calls: vec![RuntimeCall::Utility(pallet_utility::Call::batch_all {
                calls: vec![RuntimeCall::Balances(
                    pallet_balances::Call::transfer_allow_death {
                        dest: charlie.into(),
                        value: 1000,
                    },
                )],
            })],
        });
        assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call3));
    });
}
