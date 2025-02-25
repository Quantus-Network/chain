
// // NOTE: This should be here but is not happy here


// use dilithium_crypto::{
//     ResonanceSignatureScheme, ResonanceSigner, ResonancePublic, ResonanceSignature, PUB_KEY_BYTES, SIGNATURE_BYTES
// };
// use hdwallet;

// use sp_runtime::{
//     generic::UncheckedExtrinsic,
//     traits::{Verify, BlakeTwo256, IdentifyAccount},
//     MultiAddress, AccountId32,
// };
// use sp_std::prelude::*;
// //#![no_std]
// use codec::{Encode, Decode};
// use sp_io::hashing;

// // Placeholder types (replace with your actual runtime types)
// type RuntimeCall = u32; // Simplified for testing
// type SignedExtra = ();  // Simplified for testing
// type Address = MultiAddress<AccountId32, ()>;


// // Integration test
// #[test]
// fn test_dilithium_extrinsic_0() {
//     // Step 1: Generate a keypair
//     let keypair = hdwallet::generate(None); // No entropy for this test
//     let pk_bytes: [u8; PUB_KEY_BYTES] = keypair.public.to_bytes();
//     println!("Public Key (hex): {:02x?}", pk_bytes);

//     // Step 2: Create and sign a payload
//     let payload: RuntimeCall = 42; // Example call
//     let msg = payload.encode();

//     let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");
//     // let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");

//     println!("Signature (hex): {:02x?}", sig_bytes);

//     let signature = ResonanceSignature::from_slice(&sig_bytes).expect("Signature length mismatch");

    
//     // Step 3: Derive AccountId and create extrinsic
//     let account_id = hashing::blake2_256(&pk_bytes).into();
//     let signed_extra: SignedExtra = ();
//     let extrinsic = UncheckedExtrinsic::new_signed(
//         payload,
//         Address::Id(account_id),
//         ResonanceSignatureScheme::Resonance(signature, pk_bytes),
//         signed_extra,
//     );

//     println!("Payload AccountId: {:?}", Address::Id(account_id));

//     // Step 4: Encode the extrinsic
//     let encoded = extrinsic.encode();

//     // Step 5: Decode the extrinsic

//     let decoded = UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");
//     assert_eq!(decoded.function, payload, "Decoded function does not match original payload");
//     assert_eq!(decoded.signature, extrinsic.signature, "Decoded signature does not match original");

//     // Step 6: Verify the signature using the AccountId from the decoded extrinsic
//     match decoded.signature {
//         Some((address, signature, extra)) => {
//             // Extract components into individual variables for debugging
//             let decoded_address: Address = address;
//             let decoded_signature: ResonanceSignatureScheme = signature;
//             let decoded_extra: SignedExtra = extra;

//             // Debug output for each component
//             println!("Decoded Address: {:?}", decoded_address);
//             println!("Decoded Signature: {:?}", decoded_signature);
//             println!("Decoded Extra: {:?}", decoded_extra);

//             // Extract AccountId from Address
//             let decoded_account_id = match decoded_address {
//                 Address::Id(id) => id,
//                 _ => panic!("Expected Address::Id variant, got {:?}", decoded_address),
//             };

//             // Additional debug output for AccountId
//             println!("Decoded AccountId: {:?}", decoded_account_id);

//             // Verify the signature
//             let is_valid = decoded_signature.verify(&msg[..], &decoded_account_id);
//             assert!(is_valid, "Signature verification failed for AccountId: {:?}", decoded_account_id);
//         },
//         None => panic!("Decoded extrinsic has no signature"),
//     }
// }