// impl Keypair {
//     /// Generate a Keypair instance.
//     /// 
//     /// # Arguments
//     /// 
//     /// * 'entropy' - optional bytes for determining the generation process
//     /// 
//     /// Returns an instance of Keypair
//     pub fn generate(entropy: Option<&[u8]>) -> Keypair {
//         let mut pk = [0u8; PUBLICKEYBYTES];
//         let mut sk = [0u8; SECRETKEYBYTES];
//         crate::sign::ml_dsa_87::keypair(&mut pk, &mut sk, entropy);
//         Keypair {
//             secret: SecretKey::from_bytes(&sk),
//             public: PublicKey::from_bytes(&pk)
//         }
// use rusty_crystals_dilithium::ml_dsa_87::Keypair;
// use rusty_crystals_dilithium::ml_dsa_87::PublicKey;

use rusty_crystals_dilithium_full::ml_dsa_87::Keypair;
use rusty_crystals_dilithium_full::ml_dsa_87::PublicKey;

// use rusty_crystals_hdwallet::HDLattice;
// use dilithium::ml_dsa_87::PublicKey;
// use dilithium::ml_dsa_87::Keypair;
pub fn generate(entropy: Option<&[u8]>) -> Keypair {
    Keypair::from_entropy(entropy);
}





