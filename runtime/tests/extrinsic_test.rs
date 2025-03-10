// use frame_support::{
//     assert_ok,
//     traits::{Currency, OnInitialize, OnFinalize},
// };
use resonance_runtime::{
    Balances, Executive, RuntimeCall, UncheckedExtrinsic,
    AccountId, Address, Header, Block,
    ResonanceSignatureScheme, Runtime,
    SignedPayload
};
use sp_runtime::generic::{Era, Preamble};
use sp_keyring::AccountKeyring;
use sp_core::H256;
use frame_support::traits::fungible::Mutate;
use sp_io::TestExternalities;

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

        // Create test runtime with genesis state
        let mut t = TestExternalities::new_empty();
        t.execute_with(|| {
            // Initialize genesis block (block 0)
            frame_system::Pallet::<Runtime>::initialize(
                &1, // Start at block 1
                &H256::default(), // Parent hash
                &Default::default(), // Digest
            );
            Balances::set_balance(&alice, 1000);
            Balances::set_balance(&bob, 0);

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
            );

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
    ) -> UncheckedExtrinsic {
        // Compute account nonce
        let account_nonce = frame_system::Pallet::<Runtime>::account_nonce(&sender);


        // Use Era::Mortal aligned with current block (1)
        // let current_block = frame_system::Pallet::<Runtime>::block_number();
        // let period = 64; // Typical period
        // let phase = current_block % period;

        let extra = (
            frame_system::CheckNonZeroSender::<Runtime>::new(),
            frame_system::CheckSpecVersion::<Runtime>::new(),
            frame_system::CheckTxVersion::<Runtime>::new(),
            frame_system::CheckGenesis::<Runtime>::new(),
            frame_system::CheckMortality::<Runtime>::from(Era::immortal()),
            frame_system::CheckNonce::<Runtime>::from(account_nonce),
            frame_system::CheckWeight::<Runtime>::new(),
            pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(1000u128),
            frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(true),
        );

        // Create signed payload
        let raw_payload = SignedPayload::new(call.clone(), extra.clone()).unwrap();
        let signature = signer.sign(&raw_payload.encode()); // Fixed encoding

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