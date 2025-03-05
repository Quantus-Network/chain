#![no_std]

use rusty_crystals_dilithium::{ml_dsa_87::Keypair, params::SEEDBYTES};

pub fn generate(entropy: Option<&[u8]>) -> Result<Keypair, &'static str> {
    if entropy.is_some() && entropy.unwrap().len() < SEEDBYTES {
        return Err("Entropy must be at least SEEDBYTES long");
    }
    Ok(Keypair::generate(entropy))
}





