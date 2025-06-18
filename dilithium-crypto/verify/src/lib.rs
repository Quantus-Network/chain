#![no_std]

use rusty_crystals_dilithium::ml_dsa_87::PublicKey;

pub fn verify(pub_key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    match PublicKey::from_bytes(pub_key) {
        Ok(pk) => pk.verify(msg, sig, None),
        Err(e) => {
            log::warn!(target: "dilithium-verify", "public key failed to deserialize {:?}", e);
            false
        }
    }
}
