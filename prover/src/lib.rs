use anyhow::{anyhow, Result};
use codec::{Encode, Decode};
use plonky2::{
    plonk::{
        config::{GenericConfig, PoseidonGoldilocksConfig},
        proof::ProofWithPublicInputs,
        prover::prove,
        circuit_data::{ProverCircuitData, CommonCircuitData, VerifierCircuitData},
    },
    field::{goldilocks_field::GoldilocksField, types::Field},
    util::serialization::DefaultGateSerializer
};
use serde::{Serialize, Deserialize};
use sp_core::H256;
use sp_runtime::AccountId32;
use lazy_static::lazy_static;
use std::{vec::Vec, fs, path::Path};

mod circuit;

const D: usize = 2;
type C = PoseidonGoldilocksConfig;
type F = <C as GenericConfig<D>>::F;

// Define the circuit data as a lazy static constant
lazy_static! {
    static ref COMMON_DATA: CommonCircuitData<F, D> = {
        let bytes = include_bytes!("data/common.hex");
        if bytes.is_empty() {
            panic!("Empty common circuit data!");
        }
        CommonCircuitData::from_bytes(bytes.to_vec(), &DefaultGateSerializer)
            .expect("Failed to parse common circuit data")
    };
    
    static ref PROVER_DATA: ProverCircuitData<F, C, D> = {
        let bytes = include_bytes!("data/prover.hex");
        if bytes.is_empty() {
            panic!("Empty prover circuit data!");
        }
        ProverCircuitData::from_bytes(bytes.to_vec(), &DefaultGateSerializer)
            .expect("Failed to parse prover circuit data")
    };
    
    static ref VERIFIER_DATA: VerifierCircuitData<F, C, D> = {
        let bytes = include_bytes!("data/verifier.hex");
        if bytes.is_empty() {
            panic!("Empty verifier circuit data!");
        }
        VerifierCircuitData::from_bytes(bytes.to_vec(), &DefaultGateSerializer)
            .expect("Failed to parse verifier circuit data")
    };
}

/// Represents the private inputs for the wormhole circuit
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WormholePrivateInputs {
    pub secret: Vec<u8>,
    pub merkle_path: Vec<H256>,
    pub merkle_indices: Vec<u64>,
    // Add more private inputs as needed
}

/// Represents the public inputs for the wormhole circuit
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct WormholePublicInputs {
    pub nullifier: [u8; 64],
    pub exit_account: AccountId32,
    pub exit_amount: u64,
    pub fee_amount: u64,
    pub storage_root: [u8; 32],
}

impl WormholePublicInputs {
    /// Convert public inputs to GoldilocksField elements
    pub fn to_fields(&self) -> Vec<F> {
        let mut fields = Vec::new();
        
        // Convert nullifier (64 bytes) to 8 field elements
        for i in 0..8 {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&self.nullifier[i*8..(i+1)*8]);
            let value = u64::from_le_bytes(bytes);
            fields.push(F::from_canonical_u64(value));
        }
        
        // Convert exit_account (32 bytes) to 4 field elements
        let account_bytes = self.exit_account.encode();
        for i in 0..4 {
            let start = i * 8;
            let end = (i + 1) * 8;
            if start < account_bytes.len() {
                let mut bytes = [0u8; 8];
                let slice_end = std::cmp::min(end, account_bytes.len());
                bytes[0..(slice_end - start)].copy_from_slice(&account_bytes[start..slice_end]);
                let value = u64::from_le_bytes(bytes);
                fields.push(F::from_canonical_u64(value));
            } else {
                fields.push(F::ZERO);
            }
        }
        
        // Convert exit_amount to a field element
        fields.push(F::from_canonical_u64(self.exit_amount));
        
        // Convert fee_amount to a field element
        fields.push(F::from_canonical_u64(self.fee_amount));
        
        // Convert storage_root (32 bytes) to 4 field elements
        for i in 0..4 {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&self.storage_root[i*8..(i+1)*8]);
            let value = u64::from_le_bytes(bytes);
            fields.push(F::from_canonical_u64(value));
        }
        
        fields
    }
    
    /// Convert from GoldilocksField elements to public inputs
    pub fn from_fields(fields: &[GoldilocksField]) -> Result<Self> {
        if fields.len() < 18 { // Ensure we have enough fields
            return Err(anyhow!("Invalid number of public input fields"));
        }

        // Convert fields to bytes, each GoldilocksField is 8 bytes (u64)
        let mut nullifier = [0u8; 64];
        let mut account_bytes = [0u8; 32];
        let mut storage_root = [0u8; 32];

        // First 8 fields (64 bytes) are the nullifier
        for i in 0..8 {
            nullifier[i*8..(i+1)*8].copy_from_slice(&fields[i].to_canonical_u64().to_le_bytes());
        }

        // Next 4 fields (32 bytes) are the exit account
        for i in 0..4 {
            account_bytes[i*8..(i+1)*8].copy_from_slice(&fields[i+8].to_canonical_u64().to_le_bytes());
        }

        // Next field is exit amount
        let exit_amount = fields[12].to_canonical_u64();

        // Next field is fee amount
        let fee_amount = fields[13].to_canonical_u64();

        // Last 4 fields are storage root
        for i in 0..4 {
            storage_root[i*8..(i+1)*8].copy_from_slice(&fields[i+14].to_canonical_u64().to_le_bytes());
        }

        let exit_account = AccountId32::decode(&mut &account_bytes[..])
            .map_err(|_| anyhow!("Invalid account ID encoding"))?;

        Ok(WormholePublicInputs {
            nullifier,
            exit_account,
            exit_amount,
            fee_amount,
            storage_root,
        })
    }
}

/// Generate a proof for the wormhole circuit
pub fn generate_proof(
    private_inputs: &WormholePrivateInputs,
    public_inputs: &WormholePublicInputs,
) -> Result<Vec<u8>> {
    // If the circuit data files are empty, we can't generate a proof
    if include_bytes!("data/common.hex").is_empty() ||
       include_bytes!("data/prover.hex").is_empty() ||
       include_bytes!("data/verifier.hex").is_empty() {
        return Err(anyhow!("Circuit data files are empty. Please provide real circuit data files."));
    }
    
    // Convert public inputs to field elements
    let public_input_fields = public_inputs.to_fields();
    
    // Process private inputs to prepare for the circuit
    let targets = PROVER_DATA.prover_only.targets.clone();
    let witness_values = process_private_inputs(private_inputs, public_inputs)?;
    
    // Generate the proof
    let proof = prove(
        &PROVER_DATA.prover_only,
        &COMMON_DATA,
        targets,
        witness_values,
        &mut rand::thread_rng(),
    )?;
    
    // Serialize the proof
    let proof_bytes = proof.to_bytes(&COMMON_DATA)?;
    
    // Verify the proof locally before returning it
    let verification_result = VERIFIER_DATA.verify(ProofWithPublicInputs::from_bytes(proof_bytes.clone(), &COMMON_DATA)?);
    if verification_result.is_err() {
        return Err(anyhow!("Proof verification failed: {:?}", verification_result.err()));
    }
    
    Ok(proof_bytes)
}

/// Process private inputs into witness values for the circuit
fn process_private_inputs(
    private_inputs: &WormholePrivateInputs,
    public_inputs: &WormholePublicInputs,
) -> Result<Vec<Vec<F>>> {
    // Call the circuit-specific implementation
    circuit::process_witness(private_inputs, public_inputs)
}

/// Save a proof to a file
pub fn save_proof(proof_bytes: &[u8], file_path: &Path) -> Result<()> {
    fs::write(file_path, proof_bytes)?;
    Ok(())
}

/// Load a proof from a file
pub fn load_proof(file_path: &Path) -> Result<Vec<u8>> {
    let proof_bytes = fs::read(file_path)?;
    Ok(proof_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    
    #[test]
    fn test_public_inputs_conversion() {
        // Skip test if files are empty
        if include_bytes!("data/common.hex").is_empty() {
            println!("Skipping test_public_inputs_conversion - empty circuit data");
            return;
        }
        
        // Create sample public inputs
        let public_inputs = WormholePublicInputs {
            nullifier: [1u8; 64],
            exit_account: AccountId32::new([2u8; 32]),
            exit_amount: 100,
            fee_amount: 10,
            storage_root: [3u8; 32],
        };
        
        // Convert to fields
        let fields = public_inputs.to_fields();
        
        // Convert back
        let converted = WormholePublicInputs::from_fields(&fields).unwrap();
        
        // Verify
        assert_eq!(public_inputs.nullifier, converted.nullifier);
        assert_eq!(public_inputs.exit_account, converted.exit_account);
        assert_eq!(public_inputs.exit_amount, converted.exit_amount);
        assert_eq!(public_inputs.fee_amount, converted.fee_amount);
        assert_eq!(public_inputs.storage_root, converted.storage_root);
    }
}
