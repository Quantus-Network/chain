#![cfg_attr(not(feature = "std"), no_std)]

use plonky2::plonk::config::{GenericConfig, PoseidonGoldilocksConfig};

// We use Goldilocks field for the circuit and Poseidon for the hash function.
pub type C = PoseidonGoldilocksConfig;
pub type F = <C as GenericConfig<D>>::F;
const D: usize = 2;

/// The wormhole circuit.
///
/// This circuit is used to prove that a user knows a secret `s` such that:
/// 1. `H(s) = commitment` (the commitment is the wormhole address)
/// 2. `H(s, nullifier)` is a valid nullifier hash.
/// 3. The `commitment` is part of a Merkle tree of deposits.
///
/// For now, we will skip the Merkle tree part and focus on the hashing.
pub struct WormholeCircuit {
    // Private inputs
    pub secret: F,
    pub nullifier: F,

    // Public inputs
    pub commitment: F,
    pub nullifier_hash: F,
}

impl WormholeCircuit {
    /// Generates the circuit and the proof.
    #[cfg(feature = "std")]
    pub fn build() -> anyhow::Result<()> {
        use plonky2::field::goldilocks_field::GoldilocksField;
        use plonky2::field::types::Field;
        use plonky2::hash::poseidon::PoseidonHash;
        use plonky2::iop::witness::{PartialWitness, WitnessWrite};
        use plonky2::plonk::circuit_builder::CircuitBuilder;
        use plonky2::plonk::circuit_data::CircuitConfig;
        use plonky2::plonk::config::Hasher;
        use std::println;
        use std::vec::Vec;

        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        // --- Define the circuit ---

        // Public inputs
        let commitment_target = builder.add_virtual_public_input();
        let nullifier_hash_target = builder.add_virtual_public_input();

        // Private inputs (witnesses)
        let secret_target = builder.add_virtual_target();
        let nullifier_target = builder.add_virtual_target();

        // 1. H(secret) == commitment
        let mut state1 = Vec::new();
        for _ in 0..8 {
            state1.push(builder.zero());
        }
        state1[0] = secret_target;
        let computed_commitment_target =
            builder.hash_n_to_m_no_pad::<PoseidonHash>(state1.to_vec(), 1);
        builder.connect(commitment_target, computed_commitment_target[0]);

        // 2. H(secret, nullifier) == nullifier_hash
        let mut state2 = Vec::new();
        for _ in 0..8 {
            state2.push(builder.zero());
        }
        state2[0] = secret_target;
        state2[1] = nullifier_target;
        let computed_nullifier_hash_target =
            builder.hash_n_to_m_no_pad::<PoseidonHash>(state2.to_vec(), 1);
        builder.connect(nullifier_hash_target, computed_nullifier_hash_target[0]);

        // --- Generate the proof ---
        let data = builder.build::<C>();

        // Example values for witness generation
        let secret = GoldilocksField::from_canonical_u64(123);
        let nullifier = GoldilocksField::from_canonical_u64(456);

        let mut pw = PartialWitness::new();

        pw.set_target(secret_target, secret)?;
        pw.set_target(nullifier_target, nullifier)?;

        // Calculate public inputs based on private inputs
        let commitment = PoseidonHash::hash_no_pad(&[
            secret,
            F::ZERO,
            F::ZERO,
            F::ZERO,
            F::ZERO,
            F::ZERO,
            F::ZERO,
            F::ZERO,
        ])
        .elements[0];
        let nullifier_hash = PoseidonHash::hash_no_pad(&[
            secret,
            nullifier,
            F::ZERO,
            F::ZERO,
            F::ZERO,
            F::ZERO,
            F::ZERO,
            F::ZERO,
        ])
        .elements[0];

        pw.set_target(commitment_target, commitment)?;
        pw.set_target(nullifier_hash_target, nullifier_hash)?;

        println!("Generating proof...");
        let proof = data.prove(pw)?;
        println!("Proof generated successfully.");

        data.verify(proof)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = WormholeCircuit::build();
        assert!(result.is_ok());
    }
}
