// Import your runtime types and pallets.
use resonance_runtime::{
    Balances, Executive, RuntimeCall, UncheckedExtrinsic,
    AccountId, Address, Header, Block, ResonanceSignatureScheme,
    Runtime, SignedPayload, RuntimeGenesisConfig,
};
use sp_runtime::generic::{Era, Preamble};
use sp_keyring::AccountKeyring;
use sp_core::H256;
use frame_support::traits::fungible::Mutate;
use sp_io::TestExternalities;
use sp_runtime::BuildStorage;

// For this example, we assume that `frame_system` and the other pallets are in scope.
use frame_system;

#[cfg(test)]
mod tests {
    use super::*;
    use codec::Encode;
    use resonance_runtime::TxExtension;
    use sp_core::H256;
    use sp_runtime::BuildStorage;

    /// A helper ExtBuilder to build the test externalities.
    pub struct ExtBuilder {
        existential_deposit: u128,
        // A list of initial account balances.
        balances: Vec<(AccountId, u128)>,
    }

    impl Default for ExtBuilder {
        fn default() -> Self {
            Self {
                existential_deposit: 1,
                balances: vec![],
            }
        }
    }

    impl ExtBuilder {
        /// Set the existential deposit.
        pub fn existential_deposit(mut self, deposit: u128) -> Self {
            self.existential_deposit = deposit;
            self
        }

        /// Set the initial balances.
        pub fn balances(mut self, balances: Vec<(AccountId, u128)>) -> Self {
            self.balances = balances;
            self
        }

        /// Build the TestExternalities.
        pub fn build(self) -> TestExternalities {
            // Build the storage using the system genesis config.
            let mut storage = frame_system::GenesisConfig::<Runtime>::default()
                .build_storage()
                .unwrap();

            // Assimilate the balances genesis config.
            // (If you also have a transaction payment genesis config, assimilate that here.)
            pallet_balances::GenesisConfig::<Runtime> {
                balances: self.balances,
            }
            .assimilate_storage(&mut storage)
            .unwrap();

            let mut ext = TestExternalities::new(storage);
            // Set the block number to 1 so that nonces and fee calculations work as expected.
            ext.execute_with(|| frame_system::Pallet::<Runtime>::set_block_number(1));
            ext
        }
    }

    /// Create a signed extrinsic with the proper "extra" tuple.
    ///
    /// In our runtime the extra tuple contains nine items:
    /// 1. CheckNonZeroSender
    /// 2. CheckSpecVersion
    /// 3. CheckTxVersion
    /// 4. CheckGenesis
    /// 5. CheckMortality (Era)
    /// 6. CheckNonce
    /// 7. CheckWeight
    /// 8. ChargeTransactionPayment (fee)
    /// 9. CheckMetadataHash
    fn create_signed_extrinsic(
        sender: AccountId,
        call: RuntimeCall,
        signer: AccountKeyring,
        fee: u128,
    ) -> UncheckedExtrinsic {
        // Get the current nonce from the system.
        let account_nonce = frame_system::Pallet::<Runtime>::account_nonce(&sender);

        // Construct the extra tuple exactly as expected.
        // let extra: TxExtension = (
        //     frame_system::CheckNonZeroSender::<Runtime>::new(),
        //     frame_system::CheckSpecVersion::<Runtime>::new(),
        //     frame_system::CheckTxVersion::<Runtime>::new(),
        //     frame_system::CheckGenesis::<Runtime>::new(),
        //     frame_system::CheckMortality::<Runtime>::from(Era::Immortal),
        //     frame_system::CheckNonce::<Runtime>::from(account_nonce),
        //     frame_system::CheckWeight::<Runtime>::new(),
        //     pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(fee),
        //     frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
        // );
        let extra: TxExtension = (); // debug

        // Create the signed payload.
        let raw_payload = SignedPayload::new(call.clone(), extra.clone())
            .expect("SignedPayload creation should not fail");
        let signature = signer.sign(&raw_payload.encode());

        // Build the UncheckedExtrinsic.

        let extrinsic = UncheckedExtrinsic::new_signed(
            call.clone(),
            Address::Id(sender),
            ResonanceSignatureScheme::Sr25519(signature),
            extra,
        );
        extrinsic

        // UncheckedExtrinsic::from_parts(
        //     call,
        //     Preamble::Signed(
        //         Address::Id(sender),
        //         // Wrap the signature in the runtime-specific enum.
        //         ResonanceSignatureScheme::Sr25519(signature),
        //         extra,
        //     ),
        // )
    }

    #[test]
    fn test_transfer_from_alice_to_bob() {
        // Optionally initialize the logger.
        let _ = env_logger::try_init();

        // Get Alice's and Bob's account IDs from the keyring.
        let alice = AccountKeyring::Alice.to_account_id();
        let bob = AccountKeyring::Bob.to_account_id();

        // Build test externalities with high initial balances.
        let mut ext = ExtBuilder::default()
            .existential_deposit(1)
            .balances(vec![
                (alice.clone(), 1_500_000_000),
                (bob.clone(), 1_000_000_000),
            ])
            .build();

        ext.execute_with(|| {
            // Create a call to transfer funds.
            let transfer_amount = 500;
            let call = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
                dest: bob.clone().into(),
                value: transfer_amount,
            });

            // You can choose an appropriate fee. For testing you might use 0 if you want to ignore fees.
            let fee: u128 = 1;

            // Create a signed extrinsic.
            let signed_extrinsic = create_signed_extrinsic(alice.clone(), call, AccountKeyring::Alice, fee);

            // Execute a block containing this extrinsic.
            let block = Block {
                header: Header {
                    parent_hash: H256::default(),
                    number: 1,
                    state_root: H256::default(),
                    extrinsics_root: H256::default(),
                    digest: Default::default(),
                },
                extrinsics: vec![signed_extrinsic],
            };

            Executive::execute_block(block);

            // Verify balances. (Remember that if fees are taken, adjust expectations.)
            let alice_balance = Balances::free_balance(&alice);
            let bob_balance = Balances::free_balance(&bob);

            // If fee is zero, Alice’s balance should be reduced exactly by the transfer amount.
            assert_eq!(
                alice_balance,
                1_000_000 - transfer_amount,
                "Alice's balance should be reduced by the transfer amount"
            );
            assert_eq!(
                bob_balance,
                transfer_amount,
                "Bob's balance should be increased by the transfer amount"
            );
        });
    }
}