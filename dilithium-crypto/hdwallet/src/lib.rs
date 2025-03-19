#![no_std]

use rusty_crystals_dilithium::{ml_dsa_87::{Keypair, SecretKey, PublicKey}, params::SEEDBYTES};

pub fn generate(entropy: Option<&[u8]>) -> Result<Keypair, &'static str> {
    if entropy.is_some() && entropy.unwrap().len() < SEEDBYTES {
        return Err("Entropy must be at least SEEDBYTES long");
    }
    Ok(Keypair::generate(entropy))
}

pub fn create_keypair(public_key: &[u8], secret_key: &[u8]) -> Result<Keypair, &'static str> {
    let keypair = Keypair {
        secret: SecretKey::from_bytes(&secret_key),
        public: PublicKey::from_bytes(&public_key)
    };
    Ok(keypair)
}




