use resonance_runtime::{
    Balances, Executive, RuntimeCall, UncheckedExtrinsic,
    AccountId, Address, Header, Block, ResonanceSignatureScheme,
    Runtime, SignedPayload, RuntimeGenesisConfig, // Import the genesis config
};
use sp_runtime::generic::{Era, Preamble};
use sp_keyring::AccountKeyring;
use sp_core::H256;
use frame_support::traits::fungible::Mutate;
use sp_io::TestExternalities;
use sp_runtime::BuildStorage;

mod tests {
    use super::*;
    use codec::Encode;

    fn setup() {
        let _ = env_logger::try_init();
    }

    #[test]
    fn test_transfer_from_alice_to_bob() {
        setup();

        let alice = AccountKeyring::Alice.to_account_id();
        let bob = AccountKeyring::Bob.to_account_id();

        // Use the runtime's default genesis config
        let mut t = TestExternalities::new(RuntimeGenesisConfig::default().build_storage().unwrap());
        t.execute_with(|| {
            // Initialize block 1
            frame_system::Pallet::<Runtime>::initialize(
                &1,
                &H256::default(),
                &Default::default(),
            );

            // Set initial balances
            Balances::set_balance(&alice, 1000000000);
            Balances::set_balance(&bob, 1000000);

            // Add this after setting balances
            frame_system::Pallet::<Runtime>::inc_providers(&alice);
            frame_system::Pallet::<Runtime>::inc_providers(&bob);

            // Create transfer transaction
            let transfer_amount = 500;
            let call = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
                dest: bob.clone().into(),
                value: transfer_amount,
            });

            // Sign the transaction
            let signed_extrinsic = create_signed_extrinsic(
                alice.clone(),
                call,
                AccountKeyring::Alice,
                1000u128, // fee 
            );

            let dispatch_info = call.get_dispatch_info();
            let fee_required = <YourFeeConverter as Convert<Weight, Balance>>::convert(dispatch_info.weight);
            
            // Execute the block
            Executive::execute_block(Block {
                header: Header {
                    parent_hash: H256::default(),
                    number: 1,
                    state_root: H256::default(),
                    extrinsics_root: H256::default(),
                    digest: Default::default(),
                },
                extrinsics: vec![signed_extrinsic],
            });

            // Verify balances
            let alice_balance = Balances::free_balance(&alice);
            let bob_balance = Balances::free_balance(&bob);

            assert_eq!(alice_balance, 1000 - transfer_amount, "Alice's balance should be reduced");
            assert_eq!(bob_balance, transfer_amount, "Bob's balance should be increased");
        });
    }

    fn create_signed_extrinsic(
        sender: AccountId,
        call: RuntimeCall,
        signer: AccountKeyring,
        fee: u128,
    ) -> UncheckedExtrinsic {
        // Compute account nonce
        let account_nonce = frame_system::Pallet::<Runtime>::account_nonce(&sender);

        let extra = (
            frame_system::CheckNonZeroSender::<Runtime>::new(),
            frame_system::CheckSpecVersion::<Runtime>::new(),
            frame_system::CheckTxVersion::<Runtime>::new(),
            frame_system::CheckGenesis::<Runtime>::new(),
            frame_system::CheckMortality::<Runtime>::from(Era::Immortal),
            frame_system::CheckNonce::<Runtime>::from(account_nonce),
            frame_system::CheckWeight::<Runtime>::new(),
            pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(fee),
            frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
        );

        // Create signed payload
        let raw_payload = SignedPayload::new(call.clone(), extra.clone()).unwrap();
        let signature = signer.sign(&raw_payload.encode());

        UncheckedExtrinsic::from_parts(
            call,
            Preamble::Signed(
                Address::Id(sender),
                ResonanceSignatureScheme::Sr25519(signature),
                extra,
            ),
        )
    }
}