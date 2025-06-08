use poseidon_resonance::PoseidonHasher;
use sp_core::Hasher;
use wormhole_circuits::*;

// Helper to generate a valid root and proof for testing
fn generate_test_data() -> (Fr, Vec<[u8; 32]>) {
    let from = vec![2u8; 32];
    let to = vec![3u8; 32];
    let nonce = 1u32;
    let amount = 100u64;

    let mut leaf_hasher = PoseidonHasher::new();
    leaf_hasher.update(&nonce.to_le_bytes());
    leaf_hasher.update(&from);
    leaf_hasher.update(&to);
    leaf_hasher.update(&amount.to_le_bytes());
    let mut current_hash = leaf_hasher.finalize();

    let proof_nodes = vec![[1u8; 32], [2u8; 32]];

    for node in &proof_nodes {
        let mut node_hasher = PoseidonHasher::new();
        node_hasher.update(&current_hash);
        node_hasher.update(node);
        current_hash = node_hasher.finalize();
    }

    (Fr::from_le_bytes_mod_order(&current_hash), proof_nodes)
}

// Helper function to create a circuit for testing
fn create_test_circuit(state_root: Fr, merkle_proof_nodes: Vec<[u8; 32]>) -> WormholeCircuit<Fr> {
    WormholeCircuit {
        secret: vec![1u8; 32],
        from: vec![2u8; 32],
        to: vec![3u8; 32],
        nonce: 1u32,
        amount: Fr::from(100u64),
        recipient: Fr::from_le_bytes_mod_order(&vec![4u8; 32]),
        nullifier: Fr::from(12345u64),
        state_root,
        merkle_proof_nodes,
    }
}

#[test]
fn test_merkle_gadget_valid_proof() {
    let cs = ConstraintSystem::<Fr>::new_ref();
    let (valid_root, valid_proof) = generate_test_data();
    let mut circuit = create_test_circuit(valid_root, valid_proof);
    circuit.gadget(&mut cs.clone()).unwrap();
    assert!(
        cs.is_satisfied().unwrap(),
        "Should be satisfied for a valid proof"
    );
}

#[test]
fn test_merkle_gadget_invalid_state_root() {
    let cs = ConstraintSystem::<Fr>::new_ref();
    let (_valid_root, valid_proof) = generate_test_data();
    let invalid_root = Fr::from(99999u64); // Different root
    let mut circuit = create_test_circuit(invalid_root, valid_proof);
    circuit.gadget(&mut cs.clone()).unwrap();
    assert!(
        !cs.is_satisfied().unwrap(),
        "Should not be satisfied for an invalid root"
    );
}

#[test]
fn test_merkle_gadget_corrupted_proof_node() {
    let cs = ConstraintSystem::<Fr>::new_ref();
    let (valid_root, mut corrupted_proof) = generate_test_data();
    corrupted_proof[1][0] = 99; // Corrupt one byte of the proof
    let mut circuit = create_test_circuit(valid_root, corrupted_proof);
    circuit.gadget(&mut cs.clone()).unwrap();
    assert!(
        !cs.is_satisfied().unwrap(),
        "Should not be satisfied for a corrupted proof"
    );
}
