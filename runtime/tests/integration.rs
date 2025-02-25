use dilithium_crypto::{
    ResonanceSignatureScheme, ResonanceSigner, ResonancePublic, ResonanceSignature, WrappedPublicBytes, WrappedSignatureBytes, PUB_KEY_BYTES, SIGNATURE_BYTES
};
use hdwallet;
use sp_runtime::{
    generic::UncheckedExtrinsic,
    traits::{Verify, BlakeTwo256, IdentifyAccount},
    MultiAddress, AccountId32
};
use codec::{Encode, Decode};
use sp_io::hashing;
use sp_core::ByteArray; // Add this to bring as_slice and from_slice into scope


// Placeholder types (replace with your actual runtime types)
type RuntimeCall = u32; // Simplified for testing
type SignedExtra = ();  // Simplified for testing
type Address = MultiAddress<AccountId32, ()>;

pub fn format_hex_truncated(bytes: &[u8]) -> String {
    if bytes.len() <= 16 {
        format!("{:02x?}", bytes)
    } else {
        let first = &bytes[..8];
        let last = &bytes[bytes.len() - 8..];
        format!("{:02x?}..{:02x?}", first, last)
    }
}
// Integration test
#[test]
fn test_dilithium_extrinsic() {
    // Initialize the logger
    env_logger::init();

    // Step 1: Generate a keypair
    let entropy = [0u8; 32]; // Fixed entropy of all zeros
    let keypair = hdwallet::generate(Some(&entropy));
    let pk_bytes: [u8; PUB_KEY_BYTES as usize] = keypair.public.to_bytes();
    println!("Public Key (hex): {:?}", format_hex_truncated(&pk_bytes));

    // Step 2: Create and sign a payload
    let payload: RuntimeCall = 42; // Example call
    let msg = payload.encode();

    let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");

    println!("Signature (hex): {:?}", format_hex_truncated(&sig_bytes));
    let signature = ResonanceSignature::from_slice(&sig_bytes).expect("Signature length mismatch");
    // let signature = ResonanceSignature::from_slice(&sig_bytes).expect("Signature length mismatch");

    // let signature_bytes = signature.as_slice();

    let bytes: &[u8] = signature.as_ref();  // or signature.as_slice()
    println!("Signature bytes: {:?}", format_hex_truncated(&bytes));

    // println!("signature length: {:?}", signature.bytes.len());
    
    // Step 3: Derive AccountId and create extrinsic
    let account_id = hashing::blake2_256(&pk_bytes).into();
    let id = Address::Id(account_id);
    println!("Payload AccountId: {:?}", &id);
    let signed_extra: SignedExtra = ();
    let extrinsic = UncheckedExtrinsic::new_signed(
        payload,
        id,
        ResonanceSignatureScheme::Resonance(signature, pk_bytes),
        signed_extra,
    );


    // Step 4: Encode the extrinsic
    let encoded = extrinsic.encode();

    // Step 5: Decode the extrinsic

    let decoded: UncheckedExtrinsic<MultiAddress<AccountId32, ()>, RuntimeCall, ResonanceSignatureScheme, ()> = 
        UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");
    
    assert_eq!(decoded.function, payload, "Decoded function does not match original payload");
    assert_eq!(decoded.signature, extrinsic.signature, "Decoded signature does not match original");


    // Step 6: Verify the signature using the AccountId from the decoded extrinsic
    match decoded.signature {
        Some((address, signature, extra)) => {
            // Extract components into individual variables for debugging
            let decoded_address: Address = address;
            let decoded_signature: ResonanceSignatureScheme = signature;
            let decoded_extra: SignedExtra = extra;

            // Debug output for each component
            println!("Decoded Address: {:?}", decoded_address);
            println!("Decoded Extra: {:?}", decoded_extra);

            match decoded_signature {
                ResonanceSignatureScheme::Resonance(ref sig, pk_bytes) => {
                    let sig_bytes = sig.as_slice();
                    println!("Decoded Signature: {:?}", format_hex_truncated(&sig_bytes));
                    println!("Public Key: {:?}", format_hex_truncated(&pk_bytes));
                }
                _ => println!("Decoded Signature: --"),
            }
            // Extract AccountId from Address
            let decoded_account_id = match decoded_address {
                Address::Id(id) => id,
                _ => panic!("Expected Address::Id variant, got {:?}", decoded_address),
            };

            // Additional debug output for AccountId
            println!("Decoded AccountId: {:?}", decoded_account_id);
            println!("Decoded Payload: {:?}", decoded.function);

            // Verify the signature
            let is_valid = decoded_signature.verify(&msg[..], &decoded_account_id);
            println!("valid: {}", is_valid);

            assert!(is_valid, "Signature verification failed for AccountId: {:?}", decoded_account_id);
        },
        None => panic!("Decoded extrinsic has no signature"),
    }
}

#[test]
fn test_dilithium_extrinsic_fail_verify() {
    // Step 1: Generate a keypair
    let entropy = [0u8; 32]; // Fixed entropy of all zeros
    let entropy2 = [1u8; 32]; // Fixed entropy of all zeros
    let keypair = hdwallet::generate(Some(&entropy));
    let keypair2 = hdwallet::generate(Some(&entropy2));
    let pk_bytes: [u8; PUB_KEY_BYTES] = keypair.public.to_bytes();
    // let pk_bytes2: [u8; PUB_KEY_BYTES] = keypair2.public.to_bytes();
    // Step 2: Create and sign a payload
    let payload: RuntimeCall = 99;
    let msg = payload.encode();

    // sign with key 2
    let sig_bytes = keypair2.sign(&msg, None, false).expect("Signing failed");

    let signature = ResonanceSignature::try_from(&sig_bytes[..]).expect("Signature length mismatch");

    // let signature = ResonanceSignature::from_slice(&sig_bytes).expect("Signature length mismatch");

    
    // Step 3: Derive AccountId and create extrinsic
    let account_id = hashing::blake2_256(&pk_bytes).into();
    // let account_id_2 = hashing::blake2_256(&pk_bytes2).into();
    let id = Address::Id(account_id);
    let signed_extra: SignedExtra = ();

    // pass in account id 1, and pk_bytes (public key of account 1)
    let extrinsic = UncheckedExtrinsic::new_signed(
        payload,
        id,
        ResonanceSignatureScheme::Resonance(signature, pk_bytes),
        signed_extra,
    );

    // Step 4: Encode the extrinsic
    let encoded = extrinsic.encode();

    // Step 5: Decode the extrinsic
    let decoded: UncheckedExtrinsic<MultiAddress<AccountId32, ()>, RuntimeCall, ResonanceSignatureScheme, ()> = 
        UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");
    
    assert_eq!(decoded.function, payload, "Decoded function does not match original payload");
    assert_eq!(decoded.signature, extrinsic.signature, "Decoded signature does not match original");


    // Step 6: Verify the signature using the AccountId from the decoded extrinsic
    match decoded.signature {
        Some((address, signature, extra)) => {
            // Extract components into individual variables for debugging
            let decoded_address: Address = address;
            let decoded_signature: ResonanceSignatureScheme = signature;

            // Extract AccountId from Address
            let decoded_account_id = match decoded_address {
                Address::Id(id) => id,
                _ => panic!("Expected Address::Id variant, got {:?}", decoded_address),
            };

            // Additional debug output for AccountId
            println!("Decoded AccountId: {:?}", decoded_account_id);
            println!("Decoded Payload: {:?}", decoded.function);

            // Verify the signature
            let is_valid = decoded_signature.verify(&msg[..], &decoded_account_id);

            assert!(!is_valid, "Signature verification worked with wrong signature: {:?}", decoded_account_id);
        },
        None => panic!("Decoded extrinsic has no signature"),
    }
}

///
/// This test is to verify that the signature verification fails if the account id is wrong
#[test]
fn test_dilithium_extrinsic_fail_by_account_id() {
    let entropy = [0u8; 32]; // Fixed entropy of all zeros
    let keypair = hdwallet::generate(Some(&entropy));
    let pk_bytes: [u8; PUB_KEY_BYTES] = keypair.public.to_bytes();
    let payload: RuntimeCall = 77;
    let msg = payload.encode();

    // So we create a valid public key and signature for account 1 but then we try to sign something on behalf
    // of account 2. We send the wrong address. Should fail. 
    let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");
    let signature = ResonanceSignature::try_from(&sig_bytes[..]).expect("Signature length mismatch");
    
    // Make a random account that has nothing to do with our public key
    let account_id_2 = hashing::blake2_256(&[0u8; PUB_KEY_BYTES]).into();
    let id = Address::Id(account_id_2);
    let signed_extra: SignedExtra = ();

    // pass in account id 1, and pk_bytes (public key of account 1)
    let extrinsic = UncheckedExtrinsic::new_signed(
        payload,
        id,
        ResonanceSignatureScheme::Resonance(signature, pk_bytes), // correct signature! 
        signed_extra,
    );

    // Step 4: Encode the extrinsic
    let encoded = extrinsic.encode();

    // Step 5: Decode the extrinsic
    let decoded: UncheckedExtrinsic<MultiAddress<AccountId32, ()>, RuntimeCall, ResonanceSignatureScheme, ()> = 
        UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");
    
    assert_eq!(decoded.function, payload, "Decoded function does not match original payload");
    assert_eq!(decoded.signature, extrinsic.signature, "Decoded signature does not match original");

    // Step 6: Verify the signature using the AccountId from the decoded extrinsic
    match decoded.signature {
        Some((address, signature, extra)) => {
            // Extract components into individual variables for debugging
            let decoded_address: Address = address;
            let decoded_signature: ResonanceSignatureScheme = signature;

            // Extract AccountId from Address
            let decoded_account_id = match decoded_address {
                Address::Id(id) => id,
                _ => panic!("Expected Address::Id variant, got {:?}", decoded_address),
            };

            // Additional debug output for AccountId
            println!("Decoded AccountId: {:?}", decoded_account_id);
            println!("Decoded Payload: {:?}", decoded.function);

            // Verify the signature
            let is_valid = decoded_signature.verify(&msg[..], &decoded_account_id);

            assert!(!is_valid, "Signature verification worked with wrong account id: {:?}", decoded_account_id);
        },
        None => panic!("Decoded extrinsic has no signature"),
    }
}