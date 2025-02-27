#![no_std]

use rusty_crystals_dilithium::ml_dsa_87::PublicKey;

pub fn verify(pub_key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let pk = PublicKey::from_bytes(pub_key);
    pk.verify(msg, sig, None)
}