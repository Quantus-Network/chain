#![no_std]

use rusty_crystals_dilithium::ml_dsa_87::Keypair;

pub fn generate(entropy: Option<&[u8]>) -> Keypair {
    Keypair::generate(entropy)
}





