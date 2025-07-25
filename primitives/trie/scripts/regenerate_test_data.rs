use sp_core::{Blake2Hasher, H256};
use sp_trie::{
    LayoutV0, MemoryDB, TrieDBMutBuilder, TrieMut, StorageProof, 
    generate_trie_proof, TrieConfiguration
};
use codec::{Encode, Decode};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create initial trie data
    let initial_data = vec![
        (b"key1".to_vec(), b"value1".to_vec()),
        (b"key2".to_vec(), b"value2".to_vec()),
        (b"key3".to_vec(), b"value3".to_vec()),
        (b"another_key".to_vec(), b"another_value".to_vec()),
        (b"test_key".to_vec(), b"test_value".to_vec()),
    ];

    // Build initial trie
    let mut db = MemoryDB::new(&0u64.to_le_bytes());
    let mut root = H256::default();
    
    {
        let mut trie = TrieDBMutBuilder::<LayoutV0<Blake2Hasher>>::new(&mut db, &mut root).build();
        for (key, value) in &initial_data {
            trie.insert(key, value)?;
        }
    }

    // Generate storage proof for some keys
    let proof_keys = vec![b"key1".to_vec(), b"key3".to_vec(), b"test_key".to_vec()];
    let proof = generate_trie_proof::<LayoutV0<Blake2Hasher>, _, _, _>(
        &db,
        root,
        &proof_keys,
    )?;

    // Create valid delta order (insertions and updates)
    let valid_delta: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![
        (b"new_key1".to_vec(), Some(b"new_value1".to_vec())),
        (b"key2".to_vec(), Some(b"updated_value2".to_vec())), // update existing
        (b"new_key2".to_vec(), Some(b"new_value2".to_vec())),
        (b"key1".to_vec(), None), // delete existing
        (b"final_key".to_vec(), Some(b"final_value".to_vec())),
    ];

    // Create invalid delta order (same operations, different order)
    let invalid_delta: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![
        (b"final_key".to_vec(), Some(b"final_value".to_vec())),
        (b"key1".to_vec(), None), // delete existing
        (b"new_key2".to_vec(), Some(b"new_value2".to_vec())),
        (b"key2".to_vec(), Some(b"updated_value2".to_vec())), // update existing
        (b"new_key1".to_vec(), Some(b"new_value1".to_vec())),
    ];

    // Encode and save all test data
    let proof_encoded = proof.encode();
    let storage_root_encoded = root.encode();
    let valid_delta_encoded = valid_delta.encode();
    let invalid_delta_encoded = invalid_delta.encode();

    // Write to files
    fs::write("test-res/proof", &proof_encoded)?;
    fs::write("test-res/storage_root", &storage_root_encoded)?;
    fs::write("test-res/valid-delta-order", &valid_delta_encoded)?;
    fs::write("test-res/invalid-delta-order", &invalid_delta_encoded)?;

    println!("Successfully regenerated test data files:");
    println!("  test-res/proof ({} bytes)", proof_encoded.len());
    println!("  test-res/storage_root ({} bytes)", storage_root_encoded.len());
    println!("  test-res/valid-delta-order ({} bytes)", valid_delta_encoded.len());
    println!("  test-res/invalid-delta-order ({} bytes)", invalid_delta_encoded.len());

    // Verify the data can be decoded correctly
    let _decoded_proof = StorageProof::decode(&mut &proof_encoded[..])?;
    let _decoded_root = H256::decode(&mut &storage_root_encoded[..])?;
    let _decoded_valid_delta = Vec::<(Vec<u8>, Option<Vec<u8>>)>::decode(&mut &valid_delta_encoded[..])?;
    let _decoded_invalid_delta = Vec::<(Vec<u8>, Option<Vec<u8>>)>::decode(&mut &invalid_delta_encoded[..])?;

    println!("All test data verified successfully!");
    
    Ok(())
}