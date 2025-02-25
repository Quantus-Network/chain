#![no_std]

use rusty_crystals_dilithium::ml_dsa_87::PublicKey;

// pub fn from_bytes(bytes: &[u8]) -> PublicKey {
// pub fn verify(&self, msg: &[u8], sig: &[u8], ctx: Option<&[u8]>) -> bool {

pub fn verify(pub_key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let pk = PublicKey::from_bytes(pub_key);
    pk.verify(msg, sig, None)
}



// Do the same except with the message, not the signature...

// let sig = ml_dsa_87::sign(&msg, &sk, None);
// let mut combined = Vec::new();
// combined.extend_from_slice(&pk.to_bytes());
// combined.extend_from_slice(&sig);
// let extrinsic = UncheckedExtrinsic::new_signed(
//     call,
//     Address::Id(account_id),
//     ResonanceSignatureScheme::Resonance(combined),
//     signed_extra,
// );