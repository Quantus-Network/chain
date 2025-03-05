use codec::{Decode, Encode};
use dilithium_crypto::{
    ResonanceSignature, ResonanceSignatureScheme, PUB_KEY_BYTES,
};
use hdwallet;
use sp_core::ByteArray;
use sp_io::hashing;
use sp_runtime::{
    generic::UncheckedExtrinsic,generic::Preamble,
    traits::Verify,
    AccountId32, MultiAddress,
}; // Add this to bring as_slice and from_slice into scope

// Placeholder types (replace with your actual runtime types)
type RuntimeCall = u32; // Simplified for testing
type SignedExtra = (); // Simplified for testing
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

#[cfg(test)]
mod tests {
    use sp_keyring::AccountKeyring;

    use super::*;

    fn setup() {
        // Initialize the logger once per test run
        // Using try_init to avoid panics if called multiple times
        let _ = env_logger::try_init();
    }

    //
    // Integration test for dilithium signatures
    // Tests valid case and fail cases 
    //
    #[test]
    fn test_dilithium_extrinsic() {
        setup();

        // Generate a keypair
        let entropy = [0u8; 32]; // Fixed entropy of all zeros
        let keypair = hdwallet::generate(Some(&entropy));
        let pk_bytes: [u8; PUB_KEY_BYTES as usize] = keypair.public.to_bytes();

        println!("Gen Public Key (hex): {:?}", format_hex_truncated(&pk_bytes));

        // Create and sign a payload
        let payload: RuntimeCall = 42; // Example call
        let msg = payload.encode();
        let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");

        println!("Gen Signature (hex): {:?}", format_hex_truncated(&sig_bytes));

        let signature =
            ResonanceSignature::from_slice(&sig_bytes).expect("Signature length mismatch");

        let bytes: &[u8] = signature.as_ref(); // or signature.as_slice()
        println!("Gen Signature bytes: {:?}", format_hex_truncated(&bytes));
        println!("Gen Signature length: {:?}", bytes.len());

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
        let decoded: UncheckedExtrinsic<
            MultiAddress<AccountId32, ()>,
            RuntimeCall,
            ResonanceSignatureScheme,
            (),
        > = UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");

        assert_eq!(
            decoded.function, payload,
            "Decoded function does not match original payload"
        );
        assert_eq!(
            decoded.preamble, extrinsic.preamble,
            "Decoded signature does not match original"
        );

        // Step 6: Verify the signature using the AccountId from the decoded extrinsic
        match decoded.preamble {
            Preamble::Signed(address, signature, extra) => {
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
                        println!("Decoded Public Key: {:?}", format_hex_truncated(&pk_bytes));
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
                let msg_decoded = decoded.function.encode();
                let is_valid = decoded_signature.verify(&msg_decoded[..], &decoded_account_id);

                assert!(
                    is_valid,
                    "Signature verification failed for AccountId: {:?}",
                    decoded_account_id
                );
            }
            _ => panic!("Decoded extrinsic has no signature"),
        }
    }

    #[test]
    fn test_dilithium_extrinsic_fail_signature() {
        setup();

        // Generate a keypair
        let entropy = [0u8; 32]; // Fixed entropy of all zeros
        let keypair = hdwallet::generate(Some(&entropy));
        let pk_bytes: [u8; PUB_KEY_BYTES] = keypair.public.to_bytes();
        let account_id = hashing::blake2_256(&pk_bytes).into();
        let id = Address::Id(account_id);
        let signed_extra: SignedExtra = ();

        // Create a payload
        let payload: RuntimeCall = 99;
        let msg = payload.encode();

        // Sign payload with a different key
        let entropy2 = [1u8; 32]; // Fixed entropy of all zeros
        let keypair2 = hdwallet::generate(Some(&entropy2));
        let sig_bytes_wrong_key = keypair2.sign(&msg, None, false).expect("Signing failed");
        let signature_wrong_key = ResonanceSignature::try_from(&sig_bytes_wrong_key[..])
            .expect("Signature length mismatch");

        // Create transaction with invalid signature
        let extrinsic = UncheckedExtrinsic::new_signed(
            payload,
            id,
            ResonanceSignatureScheme::Resonance(signature_wrong_key, pk_bytes),
            signed_extra,
        );

        // Encode, decode, and verify
        let encoded = extrinsic.encode();

        let decoded: UncheckedExtrinsic<
            MultiAddress<AccountId32, ()>,
            RuntimeCall,
            ResonanceSignatureScheme,
            (),
        > = UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");

        let Preamble::Signed(address, signature, _) = decoded.preamble else {
            unreachable!("Test assumes Preamble::Signed")
        };
        let Address::Id(decoded_account_id) = address else {
            unreachable!("Test assumes Address::Id")
        };
        let msg_decoded = decoded.function.encode();

        let is_valid = signature.verify(&msg_decoded[..], &decoded_account_id);
        assert!(!is_valid, "Signature verification unexpectedly succeeded");
    }

    ///
    /// This test is to verify that the signature verification fails if the account id is wrong
    #[test]
    fn test_dilithium_extrinsic_fail_by_account_id() {
        setup();

        // Generate a keypair
        let entropy = [0u8; 32]; // Fixed entropy of all zeros
        let keypair = hdwallet::generate(Some(&entropy));
        let pk_bytes: [u8; PUB_KEY_BYTES] = keypair.public.to_bytes();

        // Create and sign a payload
        let payload: RuntimeCall = 77;
        let msg = payload.encode();
        let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");
        let signature =
            ResonanceSignature::try_from(&sig_bytes[..]).expect("Signature length mismatch");

        // Create a second account
        let account_id_2 = hashing::blake2_256(&[0u8; PUB_KEY_BYTES]).into();
        let id_2 = Address::Id(account_id_2);
        let signed_extra: SignedExtra = ();

        // Create transaction with wrong account ID.
        let extrinsic = UncheckedExtrinsic::new_signed(
            payload,
            id_2,
            ResonanceSignatureScheme::Resonance(signature, pk_bytes), // correct signature!
            signed_extra,
        );

        // Encode, decode, and verify
        let encoded = extrinsic.encode();

        let decoded: UncheckedExtrinsic<
            MultiAddress<AccountId32, ()>,
            RuntimeCall,
            ResonanceSignatureScheme,
            (),
        > = UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");

        let Preamble::Signed(address, signature, _) = decoded.preamble else {
            unreachable!("Test assumes Preamble::Signed")
        };
        let Address::Id(decoded_account_id) = address else {
            unreachable!("Test assumes Address::Id")
        };
        let msg_decoded = decoded.function.encode();

        let is_valid = signature.verify(&msg_decoded[..], &decoded_account_id);
        assert!(
            !is_valid,
            "Signature verification worked with wrong account id: {:?}",
            decoded_account_id
        );
    }

    #[test]
    fn test_dilithium_extrinsic_fail_payload() {
        setup();

        // Generate a keypair
        let entropy = [0u8; 32]; // Fixed entropy of all zeros
        let keypair = hdwallet::generate(Some(&entropy));
        let pk_bytes: [u8; PUB_KEY_BYTES as usize] = keypair.public.to_bytes();

        // Create and sign a payload
        let payload: RuntimeCall = 42;
        let msg = payload.encode();
        let sig_bytes = keypair.sign(&msg, None, false).expect("Signing failed");
        let signature =
            ResonanceSignature::from_slice(&sig_bytes).expect("Signature length mismatch");
            
        let account_id = hashing::blake2_256(&pk_bytes).into();
        let id = Address::Id(account_id);
        let signed_extra: SignedExtra = ();

        // Create transaction with wrong payload. Should fail.
        let wrong_payload: RuntimeCall = 40;

        let extrinsic = UncheckedExtrinsic::new_signed(
            wrong_payload,
            id,
            ResonanceSignatureScheme::Resonance(signature, pk_bytes),
            signed_extra,
        );

        // Encode, decode, and verify
        let encoded = extrinsic.encode();
        let decoded: UncheckedExtrinsic<
            MultiAddress<AccountId32, ()>,
            RuntimeCall,
            ResonanceSignatureScheme,
            (),
        > = UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");

        let Preamble::Signed(address, signature, _) = decoded.preamble else {
            unreachable!("Test assumes Preamble::Signed")
        };
        let Address::Id(decoded_account_id) = address else {
            unreachable!("Test assumes Address::Id")
        };
        let msg_decoded = decoded.function.encode();
        let is_valid = signature.verify(&msg_decoded[..], &decoded_account_id);
        assert!(
            !is_valid,
            "Signature verification worked with wrong payload"
        );
    }

    //
    // Integration test for sr25519 extrinsic
    // Keep this as long as we still have these around. 
    //
    #[test]
    fn test_sr25519_extrinsic() {
        setup();

        // Generate a keypair
        let alice_keyring = AccountKeyring::Alice;
        let bob_keyring = AccountKeyring::Bob;

        // Create and sign a payload
        let payload: RuntimeCall = 42; // Example call
        let msg = payload.encode();
        let signature = alice_keyring.sign(&msg);

        // Step 3: Derive AccountId and create extrinsic
        let id = Address::Id(alice_keyring.to_account_id());
        println!("Payload AccountId: {:?}", &id);

        let signed_extra: SignedExtra = ();
        let extrinsic = UncheckedExtrinsic::new_signed(
            payload,
            id,
            ResonanceSignatureScheme::Sr25519(signature),
            signed_extra,
        );

        // Step 4: Encode the extrinsic
        let encoded = extrinsic.encode();

        // Step 5: Decode the extrinsic
        let decoded: UncheckedExtrinsic<
            MultiAddress<AccountId32, ()>,
            RuntimeCall,
            ResonanceSignatureScheme,
            (),
        > = UncheckedExtrinsic::decode(&mut &encoded[..]).expect("Decoding failed");

        assert_eq!(
            decoded.function, payload,
            "Decoded function does not match original payload"
        );
        assert_eq!(
            decoded.preamble, extrinsic.preamble,
            "Decoded signature does not match original"
        );

        // Step 6: Verify the signature using the AccountId from the decoded extrinsic
        match decoded.preamble {
            Preamble::Signed(address, signature, extra) => {
                // Extract components into individual variables for debugging
                let decoded_address: Address = address;
                let decoded_signature: ResonanceSignatureScheme = signature;
                let decoded_extra: SignedExtra = extra;

                // Debug output for each component
                println!("Decoded Address: {:?}", decoded_address);
                println!("Decoded Extra: {:?}", decoded_extra);

                match decoded_signature {
                    ResonanceSignatureScheme::Sr25519(ref sig) => {
                        let sig_bytes = sig.as_slice();
                        println!("Decoded Signature: {:?}", format_hex_truncated(&sig_bytes));
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
                let msg_decoded = decoded.function.encode();
                let is_valid = decoded_signature.verify(&msg_decoded[..], &decoded_account_id);

                assert!(
                    is_valid,
                    "Signature verification failed for AccountId: {:?}",
                    decoded_account_id
                );
            }
            _ => panic!("Decoded extrinsic has no signature"),
        }
    }

}