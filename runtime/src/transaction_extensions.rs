//! Custom signed extensions for the runtime.
use crate::*;
use codec::{Decode, Encode};
use core::{marker::PhantomData, u8};
use frame_support::pallet_prelude::{InvalidTransaction, ValidTransaction};
use frame_support::traits::fungible::Inspect;
use frame_support::traits::tokens::Preservation;
use frame_system::ensure_signed;
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

                    ReversibleTransfers::do_schedule_transfer(
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

#[cfg(test)]
mod tests {
    use frame_support::pallet_prelude::{TransactionValidityError, UnknownTransaction};
    use pallet_reversible_transfers::PendingTransfers;
    use sp_runtime::{traits::TxBaseImplication, AccountId32};

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
            ext
                .clone()
                .prepare((), &origin, &call, &Default::default(), 0)
                .unwrap();
            assert_eq!((), ());

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
            ext
                .clone()
                .prepare((), &origin, &call, &Default::default(), 0)
                .unwrap();

            assert_eq!((), ());

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
}
