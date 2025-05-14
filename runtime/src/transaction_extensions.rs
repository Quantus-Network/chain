//! Custom signed extensions for the runtime.
use crate::*;
use codec::{Decode, Encode};
use core::{marker::PhantomData, u8};
use frame_support::pallet_prelude::{InvalidTransaction, ValidTransaction};
use frame_support::traits::Contains;
use frame_support::traits::fungible::Inspect;
use frame_support::traits::tokens::Preservation;
use frame_system::ensure_signed;
use frame_system::ensure_root;
use pallet_reversible_transfers::DelayPolicy;
use pallet_reversible_transfers::WeightInfo;
use scale_info::TypeInfo;
use sp_core::Get;
use sp_runtime::{traits::TransactionExtension, Weight};

/// Transaction extension for reversible accounts
///
/// This extension is used to intercept delayed transactions for users that opted in
/// for reversible transactions. Based on the policy set by the user, the transaction
/// will either be denied or intercepted and delayed.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug)]
#[scale_info(skip_type_params(T))]
pub struct ReversibleTransactionExtension<T: pallet_reversible_transfers::Config>(PhantomData<T>);

impl<T: pallet_reversible_transfers::Config + Send + Sync> ReversibleTransactionExtension<T> {
    /// Creates new `TransactionExtension` to check genesis hash.
    pub fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

impl<T: pallet_reversible_transfers::Config + Send + Sync + alloc::fmt::Debug>
    TransactionExtension<RuntimeCall> for ReversibleTransactionExtension<T>
{
    type Pre = ();
    type Val = ();
    type Implicit = ();

    const IDENTIFIER: &'static str = "ReversibleTransactionExtension";

    fn weight(&self, call: &RuntimeCall) -> Weight {
        if matches!(
            call,
            RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. })
                | RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. })
                | RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. })
        ) {
            return <T as pallet_reversible_transfers::Config>::WeightInfo::schedule_transfer();
        }
        // For reading the reversible accounts
        T::DbWeight::get().reads(1)
    }

    fn prepare(
        self,
        _val: Self::Val,
        _origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
        _call: &RuntimeCall,
        _info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
        _len: usize,
    ) -> Result<Self::Pre, frame_support::pallet_prelude::TransactionValidityError> {
        Ok(())
    }

    fn validate(
        &self,
        origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
        call: &RuntimeCall,
        _info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl sp_runtime::traits::Implication,
        _source: frame_support::pallet_prelude::TransactionSource,
    ) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
        let who = ensure_signed(origin.clone()).map_err(|_| {
            frame_support::pallet_prelude::TransactionValidityError::Invalid(
                InvalidTransaction::BadSigner,
            )
        })?;

        if let Some((_, policy)) = ReversibleTransfers::is_reversible(&who) {
            match policy {
                // If explicit, do not allow Transfer calls
                DelayPolicy::Explicit => {
                    if matches!(
                        call,
                        RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. })
                            | RuntimeCall::Balances(
                                pallet_balances::Call::transfer_allow_death { .. }
                            )
                            | RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. })
                    ) {
                        return Err(
                            frame_support::pallet_prelude::TransactionValidityError::Invalid(
                                InvalidTransaction::Custom(0),
                            ),
                        );
                    }
                }
                DelayPolicy::Intercept => {
                    // Only intercept token transfers
                    let (dest, amount) = match call {
                        RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                            dest,
                            value,
                        }) => (dest, value),
                        RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
                            dest,
                            value,
                        }) => (dest, value),
                        RuntimeCall::Balances(pallet_balances::Call::transfer_all {
                            dest,
                            keep_alive,
                        }) => (
                            dest,
                            &Balances::reducible_balance(
                                &who,
                                if *keep_alive {
                                    Preservation::Preserve
                                } else {
                                    Preservation::Expendable
                                },
                                frame_support::traits::tokens::Fortitude::Polite,
                            ),
                        ),
                        _ => return Ok((ValidTransaction::default(), (), origin)),
                    };

                    // Schedule the transfer

                    let _ = ReversibleTransfers::do_schedule_transfer(
                        origin.clone(),
                        dest.clone(),
                        *amount,
                    )
                    .map_err(|e| {
                        log::error!("Failed to schedule transfer: {:?}", e);
                        frame_support::pallet_prelude::TransactionValidityError::Invalid(
                            InvalidTransaction::Custom(1),
                        )
                    })?;

                    return Err(
                        frame_support::pallet_prelude::TransactionValidityError::Unknown(
                            frame_support::pallet_prelude::UnknownTransaction::Custom(u8::MAX),
                        ),
                    );
                }
            }
        }

        Ok((ValidTransaction::default(), (), origin))
    }
}

/// Transaction extension for checking if a voter is a member of TechCollective for track 0.
///
/// This extension intercepts voting transactions on track 0 referenda
/// and checks whether the sender is a member of TechCollective. If not,
/// the transaction will be rejected.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug)]
#[scale_info(skip_type_params(T))]
pub struct TechCollectiveExtension<T: frame_system::Config>(PhantomData<T>);

impl<T: frame_system::Config> TechCollectiveExtension<T> {
    /// Creates a new `TechCollectiveExtension`.
    pub fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

impl<T: frame_system::Config + Send + Sync + alloc::fmt::Debug>
TransactionExtension<RuntimeCall> for TechCollectiveExtension<T>
{
    type Pre = ();
    type Val = ();
    type Implicit = ();

    const IDENTIFIER: &'static str = "TechCollectiveExtension";

    fn weight(&self, call: &RuntimeCall) -> Weight {
        match call {
            RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote { .. }) => {
                // We need additional reads to check the referendum track
                // and TechCollective membership
                T::DbWeight::get().reads(2)
            },
            _ => Weight::zero(),
        }
    }

    fn prepare(
        self,
        _val: Self::Val,
        _origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
        _call: &RuntimeCall,
        _info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
        _len: usize,
    ) -> Result<Self::Pre, frame_support::pallet_prelude::TransactionValidityError> {
        Ok(())
    }

    fn validate(
        &self,
        origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
        call: &RuntimeCall,
        _info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl sp_runtime::traits::Implication,
        _source: frame_support::pallet_prelude::TransactionSource,
    ) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
        match call {
            // Handle voting
            RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote { poll_index, .. }) => {
                if let Some(info) = pallet_referenda::ReferendumInfoFor::<Runtime>::get(poll_index) {
                    if let pallet_referenda::ReferendumInfo::Ongoing(status) = info {
                        if status.track == 0 {
                            // For track 0, check if voter is either a member or root
                            let who = ensure_signed(origin.clone()).map_err(|_| {
                                frame_support::pallet_prelude::TransactionValidityError::Invalid(
                                    InvalidTransaction::BadSigner,
                                )
                            })?;
                            if !pallet_membership::Pallet::<Runtime>::contains(&who) {
                                return Err(
                                    frame_support::pallet_prelude::TransactionValidityError::Invalid(
                                        InvalidTransaction::Custom(42),
                                    ),
                                );
                            }
                        }
                    }
                }
            },
            // Handle referendum submission
            RuntimeCall::Referenda(pallet_referenda::Call::submit { proposal_origin: origin_caller, .. }) => {
                if let OriginCaller::system(frame_system::RawOrigin::Root) = **origin_caller {
                    // For track 0, check if submitter is either a member or root
                    match ensure_signed(origin.clone()) {
                        Ok(who) => {
                            // If signed, must be a member
                            if !pallet_membership::Pallet::<Runtime>::contains(&who) {
                                return Err(
                                    frame_support::pallet_prelude::TransactionValidityError::Invalid(
                                        InvalidTransaction::Custom(43),
                                    ),
                                );
                            }
                        },
                        Err(_) => {
                            // If not signed, must be root
                            if ensure_root(origin.clone()).is_err() {
                                return Err(
                                    frame_support::pallet_prelude::TransactionValidityError::Invalid(
                                        InvalidTransaction::Custom(43),
                                    ),
                                );
                            }
                        }
                    }
                }
            },
            _ => {}
        }

        Ok((ValidTransaction::default(), (), origin))
    }
}

#[cfg(test)]
mod tests {
    use frame_support::assert_ok;
    use frame_support::pallet_prelude::{TransactionValidityError, UnknownTransaction};
    use frame_support::traits::Currency;
    use pallet_reversible_transfers::PendingTransfers;
    use sp_runtime::{traits::TxBaseImplication, AccountId32};
    use sp_runtime::traits::Hash;
    use super::*;
    fn alice() -> AccountId {
        AccountId32::from([1; 32])
    }

    fn bob() -> AccountId {
        AccountId32::from([2; 32])
    }
    fn charlie() -> AccountId {
        AccountId32::from([3; 32])
    }

    // Build genesis storage according to the mock runtime.
    pub fn new_test_ext() -> sp_io::TestExternalities {
        let mut t = frame_system::GenesisConfig::<Runtime>::default()
            .build_storage()
            .unwrap();

        pallet_balances::GenesisConfig::<Runtime> {
            balances: vec![
                (alice(), EXISTENTIAL_DEPOSIT * 10000),
                (bob(), EXISTENTIAL_DEPOSIT * 2),
                (charlie(), EXISTENTIAL_DEPOSIT * 100),
            ],
        }
        .assimilate_storage(&mut t)
        .unwrap();

        pallet_reversible_transfers::GenesisConfig::<Runtime> {
            initial_reversible_accounts: vec![(alice(), 10)],
        }
        .assimilate_storage(&mut t)
        .unwrap();

        t.into()
    }

    #[test]
    fn test_reversible_transaction_extension() {
        new_test_ext().execute_with(|| {
            // Other calls should not be intercepted
            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });

            let origin = RuntimeOrigin::signed(alice());
            let ext = ReversibleTransactionExtension::<Runtime>::new();

            let result = ext.validate(
                origin,
                &call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            // we should not fail here
            assert!(result.is_ok());

            // Test the reversible transaction extension
            let ext = ReversibleTransactionExtension::<Runtime>::new();
            let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: MultiAddress::Id(bob()),
                value: 10 * EXISTENTIAL_DEPOSIT,
            });
            let origin = RuntimeOrigin::signed(alice());

            // Test the prepare method
            let pre = ext
                .clone()
                .prepare((), &origin, &call, &Default::default(), 0)
                .unwrap();
            assert_eq!(pre, ());

            // Test the validate method
            let result = ext.validate(
                origin,
                &call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );
            // we should fail here with `InvalidTransaction::Custom(0)` since default policy is
            // `DelayPolicy::Explicit`
            assert_eq!(
                result.unwrap_err(),
                TransactionValidityError::Invalid(InvalidTransaction::Custom(0))
            );
            // Pending transactions should be empty
            assert_eq!(PendingTransfers::<Runtime>::iter().count(), 0);

            // Charlie opts in for intercept
            ReversibleTransfers::set_reversibility(
                RuntimeOrigin::signed(charlie()),
                None,
                DelayPolicy::Intercept,
            )
            .unwrap();

            // Charlie sends bob a transaction
            let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: MultiAddress::Id(bob()),
                value: 10 * EXISTENTIAL_DEPOSIT,
            });

            let origin = RuntimeOrigin::signed(charlie());

            // Test the prepare method
            let pre = ext
                .clone()
                .prepare((), &origin, &call, &Default::default(), 0)
                .unwrap();

            assert_eq!(pre, ());

            // Test the validate method
            let result = ext.validate(
                origin,
                &call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );
            // we should fail here with `UnknownTransaction::Custom(u8::MAX)` since default policy is
            // `DelayPolicy::Explicit`
            assert_eq!(
                result.unwrap_err(),
                TransactionValidityError::Unknown(UnknownTransaction::Custom(u8::MAX))
            );

            // Pending transactions should contain the transaction
            assert_eq!(PendingTransfers::<Runtime>::iter().count(), 1);

            // Other calls should not be intercepted
            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            let origin = RuntimeOrigin::signed(charlie());
            let result = ext.validate(
                origin,
                &call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            // we should not fail here
            assert!(result.is_ok());
        });
    }

    fn setup_referendum(track_id: u16) -> u32 {
        // Add charlie to TechCollective
        TechCollective::add_member(
            RuntimeOrigin::root(),
            MultiAddress::Id(charlie()),
        ).unwrap();

        // Ensure funds for the test accounts
        Balances::make_free_balance_be(&alice(), 1000 * UNIT);
        Balances::make_free_balance_be(&bob(), 1000 * UNIT);
        Balances::make_free_balance_be(&charlie(), 1000 * UNIT);

        // Prepare the proposal
        let proposal = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });

        // Encode the proposal
        let encoded_call = proposal.encode();
        let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

        // Store the preimage
        Preimage::note_preimage(
            RuntimeOrigin::signed(alice()),
            encoded_call.clone()
        ).unwrap();

        // Prepare bounded call
        let bounded_call = frame_support::traits::Bounded::Lookup {
            hash: preimage_hash,
            len: encoded_call.len() as u32
        };

        // Submit referendum with appropriate origin based on track
        let origin = match track_id {
            0 => Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
            1 => Box::new(OriginCaller::system(frame_system::RawOrigin::Signed(alice()))),
            _ => panic!("Unsupported track ID: {}", track_id),
        };

        Referenda::submit(
            RuntimeOrigin::signed(alice()),
            origin,
            bounded_call,
            frame_support::traits::schedule::DispatchTime::After(0u32.into())
        ).unwrap();

        let referendum_index = 0;

        // Place decision deposit to start deciding phase
        Referenda::place_decision_deposit(
            RuntimeOrigin::signed(alice()),
            referendum_index
        ).unwrap();

        // Verify the referendum is on the correct track
        let info = pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
        match info {
            pallet_referenda::ReferendumInfo::Ongoing(status) => {
                assert_eq!(status.track, track_id, "Referendum should be on track {}", track_id);
            },
            _ => panic!("Referendum should be ongoing"),
        }

        referendum_index
    }

    #[test]
    fn test_tech_collective_vote_extension() {
        new_test_ext().execute_with(|| {
            let referendum_index = setup_referendum(0);

            // Create the extension
            let ext = TechCollectiveExtension::<Runtime>::new();

            // Create a vote call
            let vote_call = RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote {
                poll_index: referendum_index,
                vote: pallet_conviction_voting::AccountVote::Standard {
                    vote: pallet_conviction_voting::Vote {
                        aye: true,
                        conviction: pallet_conviction_voting::Conviction::Locked1x,
                    },
                    balance: 100 * UNIT,
                },
            });

            // Test validation for non-member (bob)
            let bob_origin = RuntimeOrigin::signed(bob());
            let bob_result = ext.validate(
                bob_origin,
                &vote_call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            println!("BR: {:?}", bob_result);

            // Bob's validation should fail with custom error 42
            assert!(bob_result.is_err(), "Non-member should not be allowed to vote on track 0");
            if let Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(code))) = bob_result {
                assert_eq!(code, 42, "Expected error code 42 for non-member");
            } else {
                panic!("Expected InvalidTransaction::Custom(42), got: {:?}", bob_result);
            }

            // Test validation for member (charlie)
            let charlie_origin = RuntimeOrigin::signed(charlie());
            let charlie_result = ext.validate(
                charlie_origin,
                &vote_call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            println!("CR: {:?}", charlie_result);

            // Charlie's validation should pass
            assert!(charlie_result.is_ok(), "Member should be allowed to vote on track 0");
        });
    }

    #[test]
    fn test_tech_collective_vote_extension_other_track() {
        new_test_ext().execute_with(|| {
            let referendum_index = setup_referendum(1);

            // Create the extension
            let ext = TechCollectiveExtension::<Runtime>::new();

            // Create a vote call
            let vote_call = RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote {
                poll_index: referendum_index,
                vote: pallet_conviction_voting::AccountVote::Standard {
                    vote: pallet_conviction_voting::Vote {
                        aye: true,
                        conviction: pallet_conviction_voting::Conviction::Locked1x,
                    },
                    balance: 100 * UNIT,
                },
            });

            // Test validation for non-member (bob) - should pass for other tracks
            let bob_origin = RuntimeOrigin::signed(bob());
            let bob_result = ext.validate(
                bob_origin,
                &vote_call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            // Bob's validation should pass for non-track-0
            assert!(bob_result.is_ok(), "Non-member should be allowed to vote on non-track-0");
        });
    }

    #[test]
    fn test_tech_collective_submit_referendum_extension_track_0() {
        new_test_ext().execute_with(|| {
            // Add charlie to TechCollective
            TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::Id(charlie()),
            ).unwrap();

            // Ensure funds for the test accounts
            Balances::make_free_balance_be(&alice(), 1000 * UNIT);
            Balances::make_free_balance_be(&bob(), 1000 * UNIT);
            Balances::make_free_balance_be(&charlie(), 1000 * UNIT);

            // Create the extension
            let ext = TechCollectiveExtension::<Runtime>::new();

            // Prepare the proposal for referendum
            let proposal = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
            let encoded_call = proposal.encode();
            let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

            // Store the preimage
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(alice()), // Submitter of preimage doesn't matter for this test
                encoded_call.clone()
            ));

            // Prepare bounded call for referendum
            let bounded_call = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash,
                len: encoded_call.len() as u32
            };

            // Define the submit call for track 0 (Root origin for the proposal)
            let submit_call_track_0 = RuntimeCall::Referenda(pallet_referenda::Call::submit {
                proposal_origin: Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                proposal: bounded_call.clone(),
                enactment_moment: frame_support::traits::schedule::DispatchTime::After(10u32.into()), // Arbitrary future time
            });

            // Test validation for non-member (bob) trying to submit to track 0
            let bob_origin = RuntimeOrigin::signed(bob());
            let bob_result = ext.validate(
                bob_origin.clone(),
                &submit_call_track_0,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            // Bob's validation should fail with custom error 43
            assert!(bob_result.is_err(), "Non-member should not be allowed to submit to track 0");
            if let Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(code))) = bob_result {
                assert_eq!(code, 43, "Expected error code 43 for non-member submitting to track 0");
            } else {
                panic!("Expected InvalidTransaction::Custom(43), got: {:?}", bob_result);
            }

            // Test validation for member (charlie) trying to submit to track 0
            let charlie_origin = RuntimeOrigin::signed(charlie());
            let charlie_result = ext.validate(
                charlie_origin.clone(),
                &submit_call_track_0,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            // Charlie's validation should pass
            assert!(charlie_result.is_ok(), "Member should be allowed to submit to track 0");

            // Test validation for Root origin trying to submit to track 0
            let root_origin_tx = RuntimeOrigin::root(); // This is the origin of the transaction itself
            let root_result = ext.validate(
                root_origin_tx,
                &submit_call_track_0,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );
            // Root's validation should pass
            assert!(root_result.is_ok(), "Root should be allowed to submit to track 0");


            // Test that a member can submit a referendum for a non-Root proposal origin (e.g. track 1)
            // This is to ensure the extension doesn't block valid submissions to other tracks.
            let submit_call_track_1 = RuntimeCall::Referenda(pallet_referenda::Call::submit {
                proposal_origin: Box::new(OriginCaller::system(frame_system::RawOrigin::Signed(alice()))),
                proposal: bounded_call.clone(),
                enactment_moment: frame_support::traits::schedule::DispatchTime::After(10u32.into()),
            });

            let charlie_result_track_1 = ext.validate(
                charlie_origin.clone(), // Charlie is a member
                &submit_call_track_1,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );
            assert!(charlie_result_track_1.is_ok(), "Member should be allowed to submit to track 1");

            let bob_result_track_1 = ext.validate(
                bob_origin.clone(), // Bob is not a member
                &submit_call_track_1,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );
            // This should also pass as the restriction is only for proposal_origin = Root
            assert!(bob_result_track_1.is_ok(), "Non-member should be allowed to submit to track 1");
        });
    }
}
