use crate::{Wormhole, WormholeError, ADDRESS_SALT, MAX_SECRET_SIZE, PoseidonHasher};
use sp_core::Hasher;

#[test]
fn test_wormhole_address_generation() {
    // Test secret
    let secret = b"my_secret_key_for_testing";

    // Generate wormhole address
    let wormhole_address = Wormhole::generate_wormhole_address(secret).unwrap();

    // Check if result is a valid H256
    assert_eq!(wormhole_address.as_bytes().len(), 32);

    // Check if address generation is deterministic (same input = same output)
    let wormhole_address2 = Wormhole::generate_wormhole_address(secret).unwrap();
    assert_eq!(wormhole_address, wormhole_address2);

    // Check if different secrets produce different addresses
    let different_secret = b"another_secret_key";
    let different_address = Wormhole::generate_wormhole_address(different_secret).unwrap();
    assert_ne!(wormhole_address, different_address);
}

#[test]
fn test_wormhole_address_is_double_hashed() {
    // Test secret
    let secret = b"test_secret";

    // Manually calculate single hash
    let mut combined = Vec::with_capacity(ADDRESS_SALT.len() + secret.len());
    combined.extend_from_slice(&ADDRESS_SALT);
    combined.extend_from_slice(secret);

    let single_hash = PoseidonHasher::hash(&combined);

    // Calculate full wormhole address
    let wormhole_address = Wormhole::generate_wormhole_address(secret).unwrap();

    // Manually calculate second hash
    let double_hash = PoseidonHasher::hash(single_hash.as_ref());

    // Check if wormhole address is a double hash
    assert_eq!(wormhole_address, double_hash);
    assert_ne!(wormhole_address, single_hash);
}

#[test]
fn test_salt_is_included_in_hash() {
    // Test secret
    let secret = b"secret_data";

    // Generate normal address with salt
    let normal_address = Wormhole::generate_wormhole_address(secret).unwrap();

    // Generate address without salt (manually)
    let hash_without_salt = PoseidonHasher::hash(secret);
    let double_hash_without_salt = PoseidonHasher::hash(hash_without_salt.as_ref());

    // They should be different
    assert_ne!(normal_address, double_hash_without_salt);
}

#[test]
fn test_verify_wormhole_address() {
    // Test secret
    let secret = b"verification_secret";

    // Generate wormhole address
    let wormhole_address = Wormhole::generate_wormhole_address(secret).unwrap();

    // Check if verification works correctly
    assert!(Wormhole::verify_wormhole_address(&wormhole_address, secret).unwrap());

    // Check if verification rejects invalid secret
    let wrong_secret = b"wrong_secret";
    assert!(!Wormhole::verify_wormhole_address(&wormhole_address, wrong_secret).unwrap());
}

#[test]
fn test_wormhole_address_format_compatibility() {
    // Check if the address format is compatible with normal addresses
    // in the same network (should have the same length and structure)

    let secret = b"format_test_secret";
    let wormhole_address = Wormhole::generate_wormhole_address(secret).unwrap();

    // Simulated "normal" address - just a single hash
    let normal_address = PoseidonHasher::hash(secret);

    // Check if they have the same length
    assert_eq!(wormhole_address.as_bytes().len(), normal_address.as_bytes().len());

    // Check if both are valid H256
    assert_eq!(wormhole_address.as_bytes().len(), 32);
    assert_eq!(normal_address.as_bytes().len(), 32);
}

#[test]
fn test_large_secrets() {
    // Test with large but valid secret
    let large_secret = [1u8; MAX_SECRET_SIZE];
    let wormhole_address = Wormhole::generate_wormhole_address(&large_secret).unwrap();

    // Check if result is a valid H256
    assert_eq!(wormhole_address.as_bytes().len(), 32);

    // Test with secret that exceeds max size
    let too_large_secret = [1u8; MAX_SECRET_SIZE + 1];
    let result = Wormhole::generate_wormhole_address(&too_large_secret);
    assert_eq!(result, Err(WormholeError::SecretTooLarge));
}

#[test]
fn test_empty_secret() {
    // Test with empty secret
    let empty_secret = b"";
    let result = Wormhole::generate_wormhole_address(empty_secret);

    // Should return an error
    assert_eq!(result, Err(WormholeError::EmptySecret));
}