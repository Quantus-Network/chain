use tx_sdk::TxSdk;
use sp_core::{H256, crypto::AccountId32};
use dilithium_crypto::{
    ResonanceSignature, ResonanceSignatureScheme, PUB_KEY_BYTES,
};
use dilithium_crypto::pair::{crystal_alice, dilithium_bob, crystal_charlie};
use sp_runtime::traits::IdentifyAccount;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = TxSdk::new("http://localhost:9944");

    // Define accounts to check
    let accounts = vec![
        ("Crystal Alice", crystal_alice().into_account()),
        ("Dilithium Bob", dilithium_bob().into_account()),
    ];

    // Check balances
    for (name, account) in accounts {
        let balance = sdk.get_balance(&account).await?;
        println!("Balance for {}: {}", name, balance);
    }

    // Optional: Send a test extrinsic
    // let unsigned_extrinsic = vec![0x41, 0x02, 0x00]; // Replace with your extrinsic
    // match sdk.send_tx(unsigned_extrinsic).await {
    //     Ok(hash) => println!("Transaction submitted with hash: {:?}", hash),
    //     Err(e) => eprintln!("Failed to submit transaction: {:?}", e),
    // }

    Ok(())
}

// use tx_sdk::TxSdk;
// use sp_core::H256;

// use codec::{Decode, Encode};
// use dilithium_crypto::{
//     ResonanceSignature, ResonanceSignatureScheme, PUB_KEY_BYTES,
// };
// use hdwallet;
// use sp_core::ByteArray;
// use sp_io::hashing;
// use sp_runtime::{
//     generic::UncheckedExtrinsic,generic::Preamble,
//     traits::Verify,
//     AccountId32, MultiAddress,
// }; // Add this to bring as_slice and from_slice into scope

// // Placeholder types (replace with your actual runtime types)
// type RuntimeCall = u32; // Simplified for testing
// type SignedExtra = (); // Simplified for testing
// type Address = MultiAddress<AccountId32, ()>;

// // This is just for testing the tx-sdk

// pub fn format_hex_truncated(bytes: &[u8]) -> String {
//     if bytes.len() <= 16 {
//         format!("{:02x?}", bytes)
//     } else {
//         let first = &bytes[..8];
//         let last = &bytes[bytes.len() - 8..];
//         format!("{:02x?}..{:02x?}", first, last)
//     }
// }

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     // Initialize the SDK with your node's RPC URL
//     let sdk = TxSdk::new("http://localhost:9944");


//         // Generate a keypair
//         let entropy = [0u8; 32]; // Fixed entropy of all zeros
//         let keypair = hdwallet::generate(Some(&entropy)).expect("Failed to generate keypair");
//         let pk_bytes: [u8; PUB_KEY_BYTES as usize] = keypair.public.to_bytes();

//         println!("Gen Public Key (hex): {:?}", format_hex_truncated(&pk_bytes));

//         // Create and sign a payload
//         // TODO: Replace with actual payload - we want to send coins from alice to bob
//         let payload: RuntimeCall = 42; // Example call
//         let msg = payload.encode();
//         let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");

//         println!("Gen Signature (hex): {:?}", format_hex_truncated(&sig_bytes));

//         let signature =
//             ResonanceSignature::from_slice(&sig_bytes).expect("Signature length mismatch");

//         let bytes: &[u8] = signature.as_ref(); // or signature.as_slice()
//         println!("Gen Signature bytes: {:?}", format_hex_truncated(&bytes));
//         println!("Gen Signature length: {:?}", bytes.len());

//         // Step 3: Derive AccountId and create extrinsic
//         let account_id = hashing::blake2_256(&pk_bytes).into();
//         let id = Address::Id(account_id);
//         println!("Payload AccountId: {:?}", &id);
//         let signed_extra: SignedExtra = ();
//         let extrinsic = UncheckedExtrinsic::new_signed(
//             payload,
//             id,
//             ResonanceSignatureScheme::Resonance(signature, pk_bytes),
//             signed_extra,
//         );

//         // Step 4: Encode the extrinsic
//         let encoded = extrinsic.encode();
    


//     // Example unsigned extrinsic (replace with your actual extrinsic bytes)
//     let unsigned_extrinsic = vec![0x41, 0x02, 0x00]; // Dummy data for testing

//     // Send the transaction
//     match sdk.send_tx(unsigned_extrinsic).await {
//         Ok(hash) => println!("Transaction submitted successfully with hash: {:?}", hash),
//         Err(e) => eprintln!("Failed to submit transaction: {:?}", e),
//     }

//     Ok(())
// }