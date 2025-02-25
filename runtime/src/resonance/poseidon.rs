use scale_info::TypeInfo;
use sp_runtime::traits::Hash;
use sp_core::Hasher;
use sp_core::H256;
use sp_runtime::{Deserialize, RuntimeDebug, Serialize, Vec};
use sp_storage::StateVersion;
use dusk_poseidon::{Hash as DuskPoseidonHash, Domain};
use dusk_bls12_381::BlsScalar;
use sp_trie::TrieConfiguration;
use core::hash::Hasher as StdHasher;
use log;

#[derive(Default)]
pub struct PoseidonStdHasher(Vec<u8>);

impl StdHasher for PoseidonStdHasher {
    fn finish(&self) -> u64 {
        let hash = poseidon_hash(self.0.as_slice()).0;
        u64::from_le_bytes(hash[0..8].try_into().unwrap())
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes)
    }
}


#[derive(PartialEq, Eq, Clone, RuntimeDebug, TypeInfo)]
#[derive(Serialize, Deserialize)]
pub struct PoseidonHasher;


impl Hasher for PoseidonHasher {
    type Out = H256;
    type StdHasher = PoseidonStdHasher;
    const LENGTH: usize = 0;

    fn hash(x: &[u8]) -> H256 {
        poseidon_hash(x)
    }

}


fn poseidon_hash(x: &[u8]) -> H256 {
    const BYTES_PER_ELEMENT: usize = 32;

    let mut field_elements: Vec<BlsScalar> = Vec::new();
    for chunk in x.chunks(BYTES_PER_ELEMENT) {
        // Pad with zeros if the chunk is smaller than BYTES_PER_ELEMENT
        let mut padded_chunk = [0u8; BYTES_PER_ELEMENT];
        padded_chunk[..chunk.len()].copy_from_slice(chunk);
        // Convert the chunk to a field element
        let field_element = BlsScalar::from_bytes(&padded_chunk).expect("Invalid field element");
        field_elements.push(field_element);
    }

    if x.len() == 0 {
        field_elements.push(BlsScalar::zero());
    }

    let hash = DuskPoseidonHash::digest(Domain::Other, &field_elements);
    log::error!("hash output: {:?}", hash);
    assert_eq!(hash.len(), 1, "Expected exactly 1 BlsScalar");
    H256::from_slice(&hash[0].to_bytes())
}

impl Hash for PoseidonHasher {
    type Output = H256;

    fn ordered_trie_root(input: Vec<Vec<u8>>, _state_version: StateVersion) -> Self::Output {
        let input = input.into_iter().map(|v| (v, Vec::new()));
        Self::Output::from(sp_trie::LayoutV1::<PoseidonHasher>::trie_root(input))
    }

    fn trie_root(input: Vec<(Vec<u8>, Vec<u8>)>, _state_version: StateVersion) -> Self::Output {
        Self::Output::from(sp_trie::LayoutV1::<PoseidonHasher>::trie_root(input))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let result = <PoseidonHasher as Hasher>::hash(&[]);
        assert_eq!(result.0.len(), 32);
    }

    #[test]
    fn test_single_byte() {
        let input = vec![42u8];
        let result = <PoseidonHasher as Hasher>::hash(&input);
        assert_eq!(result.0.len(), 32);
    }

    #[test]
    fn test_exactly_32_bytes() {
        let input = [1u8; 32];
        let result = <PoseidonHasher as Hasher>::hash(&input);
        assert_eq!(result.0.len(), 32);
    }

    #[test]
    fn test_multiple_chunks() {
        let input = [2u8; 64]; // Two chunks
        let result = <PoseidonHasher as Hasher>::hash(&input);
        assert_eq!(result.0.len(), 32);
    }

    #[test]
    fn test_partial_chunk() {
        let input = [3u8; 40]; // One full chunk plus 8 bytes
        let result = <PoseidonHasher as Hasher>::hash(&input);
        assert_eq!(result.0.len(), 32);
    }

    // #[test]
    // fn test_known_value() {
    //     // Replace these with actual known input/output pairs for your implementation
    //     let input = decode("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20").unwrap();
    //     let result = <PoseidonHasher as Hasher>::hash(&input);
    //     assert_eq!(result.0.len(), 32);
    // }

    #[test]
    fn test_consistency() {
        let input = [4u8; 50];
        let hash1 = <PoseidonHasher as Hasher>::hash(&input);
        let hash2 = <PoseidonHasher as Hasher>::hash(&input);
        assert_eq!(hash1, hash2, "Hash function should be deterministic");
    }

    #[test]
    fn test_different_inputs() {
        let input1 = [5u8; 32];
        let input2 = [6u8; 32];
        let hash1 = <PoseidonHasher as Hasher>::hash(&input1);
        let hash2 = <PoseidonHasher as Hasher>::hash(&input2);
        assert_ne!(hash1, hash2, "Different inputs should produce different hashes");
    }
}