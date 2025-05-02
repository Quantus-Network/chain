#[cfg(test)]
mod tests {
    use super::*;
    use crate::QPowAlgorithm; // Assuming QPowAlgorithm is in the same crate root or imported
    use primitive_types::{H256, U256, U512}; // Make sure U512 is imported
    use sp_api::{ApiError, ProvideRuntimeApi};
    use sp_blockchain::{AuxStore, HeaderBackend};
    use sp_consensus::Error as ConsensusError;
    use sp_consensus_qpow::QPoWApi; // Correct import for QPoWApi
    use sp_runtime::testing::Header;
    use sp_runtime::traits::{Block as BlockT, Header as HeaderT, NumberFor, One, Zero};
    use std::collections::HashMap;
    use std::sync::Arc;
    use parking_lot::RwLock;
    use sc_client_api::{Backend as ScBackend, BlockBackend as ScBlockBackend, Finalizer}; // Import ScBackend and Finalizer traits


    // --- Mocking Infrastructure ---

    type TestBlock = Header; // Using sp_runtime::testing::Header as our block

    // Mock HeaderBackend
    #[derive(Default, Clone)]
    struct MockHeaderBackend {
        headers: Arc<RwLock<HashMap<H256, Header>>>,
        genesis_hash: H256,
    }

    impl MockHeaderBackend {
        fn new(genesis_header: Header) -> Self {
            let genesis_hash = genesis_header.hash();
            let mut headers = HashMap::new();
            headers.insert(genesis_hash, genesis_header);
            Self {
                headers: Arc::new(RwLock::new(headers)),
                genesis_hash,
            }
        }

        fn insert(&mut self, header: Header) {
            let hash = header.hash();
            self.headers.write().insert(hash, header);
        }

        // Builds a linear chain segment on top of start_parent
        // Returns the headers created (excluding start_parent)
        fn build_chain(&mut self, start_parent_hash: H256, count: u64) -> Vec<Header> {
            let mut headers = Vec::new();
            let mut current_parent_hash = start_parent_hash;

            for _ in 0..count {
                 let parent_header = self.headers.read().get(&current_parent_hash)
                    .expect("Parent header missing during build_chain").clone();
                let number = *parent_header.number() + One::one();
                let header = Header::new(
                    number,
                    H256::random(), // state_root
                    H256::random(), // extrinsics_root
                    current_parent_hash,
                    Default::default(), // digest
                );
                current_parent_hash = header.hash();
                self.headers.write().insert(current_parent_hash, header.clone());
                headers.push(header);
            }
            headers
        }
    }

    impl HeaderBackend<TestBlock> for MockHeaderBackend {
        fn header(&self, hash: H256) -> Result<Option<Header>, sp_blockchain::Error> {
            Ok(self.headers.read().get(&hash).cloned())
        }

        fn info(&self) -> sp_blockchain::Info<TestBlock> {
            // Find the highest block number
            let best_header = self.headers.read().values()
                .max_by_key(|h| *h.number())
                .cloned()
                .unwrap_or_else(|| self.headers.read().get(&self.genesis_hash).cloned().unwrap());

            sp_blockchain::Info {
                best_hash: best_header.hash(),
                best_number: *best_header.number(),
                finalized_hash: self.genesis_hash, // Keep it simple for tests
                finalized_number: Zero::zero(),
                genesis_hash: self.genesis_hash,
                number_leaves: Default::default(), // Not needed for this test
                finalized_state: None, // Not needed
                block_gap: None, // Not needed
            }
        }

        fn status(&self, hash: H256) -> Result<sp_blockchain::BlockStatus, sp_blockchain::Error> {
            if self.headers.read().contains_key(&hash) {
                Ok(sp_blockchain::BlockStatus::InChain)
            } else {
                Ok(sp_blockchain::BlockStatus::Unknown)
            }
        }

        fn number(&self, hash: H256) -> Result<Option<NumberFor<TestBlock>>, sp_blockchain::Error> {
            Ok(self.headers.read().get(&hash).map(|h| *h.number()))
        }

        fn hash(&self, number: NumberFor<TestBlock>) -> Result<Option<H256>, sp_blockchain::Error> {
            Ok(self.headers.read().values().find(|h| *h.number() == number).map(|h| h.hash()))
        }
    }

    // Mock Runtime API Provider
    #[derive(Clone)]
    struct MockRuntimeApiProvider;

    impl ProvideRuntimeApi<TestBlock> for MockRuntimeApiProvider {
        type Api = MockRuntimeApi; // Needs a mock RuntimeApi struct

        fn runtime_api(&self) -> Result<Self::Api, ApiError> {
            Ok(MockRuntimeApi)
        }
    }

    // Mock Runtime API itself (needs QPoWApi trait)
    struct MockRuntimeApi;

    impl sp_api::Core<TestBlock> for MockRuntimeApi {
         // Implement required Core methods if necessary, otherwise leave unimplemented or panic
         fn version(&self, _at: <TestBlock as BlockT>::Hash) -> Result<sp_version::RuntimeVersion, ApiError> { unimplemented!() }
         fn execute_block(&self, _at: <TestBlock as BlockT>::Hash, _block: TestBlock) -> Result<(), ApiError> { unimplemented!() }
         fn initialize_block(&self, _at: <TestBlock as BlockT>::Hash, _header: &<TestBlock as BlockT>::Header) -> Result<(), ApiError> { unimplemented!() }
    }

    // QPoWApi needs specific return types, mock them simply
    impl QPoWApi<TestBlock> for MockRuntimeApi {
        fn get_max_reorg_depth(&self, _at: H256) -> Result<u32, ApiError> { Ok(5) } // Example value
        fn get_distance_threshold(&self, _at: H256) -> Result<U512, ApiError> { Ok(U512::from(1000)) } // Example
        fn get_max_distance(&self, _at: H256) -> Result<U512, ApiError> { Ok(U512::MAX) } // Example
        fn get_nonce_distance(&self, _at: H256, _header_hash: [u8; 32], _nonce: [u8; 64]) -> Result<U512, ApiError> { Ok(U512::from(500)) } // Example
        fn get_total_work(&self, at: H256) -> Result<U512, ApiError> {
            // Simplistic: return block number as work for testing comparison
            // In reality, this would read from storage
            let number = MockHeaderBackend::default().number(at).unwrap().unwrap_or_default(); // HACK: Need access to headers
            Ok(U512::from(number))
        }
    }


    // Mock other traits needed by HeaviestChain or Finalizer
    impl AuxStore for MockHeaderBackend {
        fn insert_aux<
            'a,
            'b: 'a,
            I: IntoIterator<Item = (&'a [u8], &'a [u8])>,
            D: IntoIterator<Item = &'a [u8]>,
        >(
            &self,
            _insert: I,
            _delete: D,
        ) -> Result<(), sp_blockchain::Error> {
            Ok(()) // No-op for tests
        }

        fn get_aux(&self, key: &[u8]) -> Result<Option<Vec<u8>>, sp_blockchain::Error> {
            Ok(None) // Assume ignored chains aren't hit in these specific tests
        }
    }

    // Mock ScBackend trait (empty implementation often sufficient if not used)
    #[derive(Clone, Default)]
    struct MockScBackend;
    impl ScBackend<TestBlock> for MockScBackend {
        // Implement required methods minimally or with panics if they shouldn't be called
         fn storage(&self, _hash: H256, _key: &sp_core::storage::StorageKey) -> Result<Option<sp_core::storage::StorageData>, sp_blockchain::Error> { unimplemented!() }
         fn child_storage(&self, _hash: H256, _child_info: &sp_core::storage::ChildInfo, _key: &sp_core::storage::StorageKey) -> Result<Option<sp_core::storage::StorageData>, sp_blockchain::Error> { unimplemented!() }
         // ... potentially many other methods ...
         fn blockchain(&self) -> &dyn sp_blockchain::Blockchain<TestBlock> {
             panic!("blockchain() not implemented for mock ScBackend");
         }
         // Add other methods as required by the compiler, potentially with panic! or default values
         fn offchain_storage(&self, _prefix: sp_core::offchain::StorageKind, _key: &[u8]) -> Option<Vec<u8>> { unimplemented!() }
         fn apply_changes(&self, _changes: sp_core::changes_trie::StorageChanges<H256>, _config: sp_core::changes_trie::BuildStrategy) -> Result<(), ApiError> { unimplemented!() }
         fn insert_block_import_notification_stream(&self) -> Box<dyn futures::Stream<Item = sc_client_api::BlockImportNotification<TestBlock>> + Send + Unpin> { unimplemented!() }
         fn block_import_notification_stream(&self) -> Box<dyn futures::Stream<Item = sc_client_api::BlockImportNotification<TestBlock>> + Send + Unpin> { unimplemented!() }
         fn justifications(&self, _hash: H256) -> Result<Option<Vec<sp_runtime::Justification>>, sp_blockchain::Error> { unimplemented!() }
         fn block_body(&self, _hash: H256) -> Result<Option<Vec<<TestBlock as BlockT>::Extrinsic>>, sp_blockchain::Error> { unimplemented!() }
         fn block_indexed_body(&self, _hash: H256) -> Result<Option<Vec<Vec<u8>>>, sp_blockchain::Error> { unimplemented!() }
         fn genesis_hash(&self) -> H256 { unimplemented!() }
         fn requires_full_sync(&self) -> bool { unimplemented!() }
    }

    // Mock ScBlockBackend trait
    impl ScBlockBackend<TestBlock> for MockHeaderBackend {
         fn block_body(&self, _hash: H256) -> Result<Option<Vec<<TestBlock as BlockT>::Extrinsic>>, sp_blockchain::Error> { Ok(None) } // Example simple implementation
         // Implement other ScBlockBackend methods as needed
         fn block_indexed_body(&self, _hash: H256) -> Result<Option<Vec<Vec<u8>>>, sp_blockchain::Error> { Ok(None) }
         fn justifications(&self, _hash: H256) -> Result<Option<Vec<sp_runtime::Justification>>, sp_blockchain::Error> { Ok(None) }
    }

    // Mock Finalizer trait
    impl Finalizer<TestBlock, MockScBackend> for MockHeaderBackend {
        fn finalize_block(
            &self,
            hash: H256,
            _justification: Option<sp_runtime::Justification>,
            _request_key_owner_proof: bool,
        ) -> Result<(), sp_blockchain::Error> {
            println!("Mock Finalize: {:?}", hash); // Just print for tests
            Ok(())
        }
         fn get_finalized_key_owner_proof(
            &self,
            _key: &sp_application_crypto::key_types::KeyTypeId,
            _authority_id: &sp_application_crypto::Public,
        ) -> Result<Option<sp_application_crypto::proof::Proof<H256>>, sp_blockchain::Error> { unimplemented!() }
    }



    // Helper function to create HeaviestChain instance with mocks
    fn setup_heaviest_chain(mock_header_backend: MockHeaderBackend) -> HeaviestChain<TestBlock, Arc<MockHeaderBackend>, MockScBackend> {
        // For find_common_ancestor_and_depth, we primarily need the client to implement HeaderBackend.
        // We wrap MockHeaderBackend in Arc as the client type `C` needs to be Arc<Something>.
        // QPowAlgorithm is likely not needed for this specific test function, so a dummy might work.
        // Provide a dummy ScBackend.

        // We need a way to mock the client (`C`) which needs multiple traits.
        // Let's make MockHeaderBackend implement all required traits (HeaderBackend, AuxStore, ScBlockBackend, Finalizer)
        // This isn't perfectly clean, but avoids complex multi-trait mock objects for now.

        let mock_client = Arc::new(mock_header_backend);

        // Dummy QPowAlgorithm (assuming it's not used in find_common_ancestor)
        let dummy_algorithm = QPowAlgorithm::new(
            mock_client.clone(), // Needs client implementing ProvideRuntimeApi
            None, // Example: No external miner URL
            None, // Example: No key
            prometheus::Registry::new(), // Dummy registry
        );

        HeaviestChain::new(
            Arc::new(MockScBackend::default()), // Mock Backend
            mock_client, // Our multi-trait mock client
            dummy_algorithm,
        )
    }


     // --- Actual Tests ---

    #[test]
    fn find_ancestor_genesis() {
        let genesis = Header::new(0, H256::random(), H256::random(), H256::zero(), Default::default());
        let mut mock_backend = MockHeaderBackend::new(genesis.clone());

        let chain_a = mock_backend.build_chain(genesis.hash(), 5); // A: 0 -> 1..5
        let chain_b = mock_backend.build_chain(genesis.hash(), 3); // B: 0 -> 1..3

        let heaviest_chain = setup_heaviest_chain(mock_backend);

        let header_a5 = chain_a.last().unwrap();
        let header_b3 = chain_b.last().unwrap();

        let (ancestor, depth) = heaviest_chain.find_common_ancestor_and_depth(header_a5, header_b3).unwrap();

        assert_eq!(ancestor, genesis.hash(), "Common ancestor should be genesis");
        assert_eq!(depth, 5, "Reorg depth should be 5 (reverting A to genesis)");

        let (ancestor_rev, depth_rev) = heaviest_chain.find_common_ancestor_and_depth(header_b3, header_a5).unwrap();
        assert_eq!(ancestor_rev, genesis.hash(), "Common ancestor should be genesis (reversed)");
        assert_eq!(depth_rev, 3, "Reorg depth should be 3 (reverting B to genesis)");

    }

    #[test]
    fn find_ancestor_simple_fork() {
        let genesis = Header::new(0, H256::random(), H256::random(), H256::zero(), Default::default());
        let mut mock_backend = MockHeaderBackend::new(genesis.clone());

        // 0 -> 1 -> 2
        let common_chain = mock_backend.build_chain(genesis.hash(), 2);
        let common_ancestor_header = common_chain.last().unwrap(); // Header at height 2

        // Fork A: 0 -> 1 -> 2 -> 3a -> 4a
        let chain_a = mock_backend.build_chain(common_ancestor_header.hash(), 2);
        // Fork B: 0 -> 1 -> 2 -> 3b -> 4b -> 5b
        let chain_b = mock_backend.build_chain(common_ancestor_header.hash(), 3);

        let heaviest_chain = setup_heaviest_chain(mock_backend);

        let header_a4 = chain_a.last().unwrap(); // Height 4
        let header_b5 = chain_b.last().unwrap(); // Height 5

        // Case 1: Compare A (shorter) against B (longer)
        let (ancestor1, depth1) = heaviest_chain.find_common_ancestor_and_depth(header_a4, header_b5).unwrap();
        assert_eq!(ancestor1, common_ancestor_header.hash(), "Common ancestor should be header 2");
        assert_eq!(depth1, 2, "Reorg depth should be 2 (reverting A: 4a, 3a)"); // Reverting A back to 2

         // Case 2: Compare B (longer) against A (shorter)
        let (ancestor2, depth2) = heaviest_chain.find_common_ancestor_and_depth(header_b5, header_a4).unwrap();
        assert_eq!(ancestor2, common_ancestor_header.hash(), "Common ancestor should be header 2 (reversed)");
        // Reverting B back to 2. B is at height 5, ancestor is at 2. Depth is 5 - 2 = 3.
        assert_eq!(depth2, 3, "Reorg depth should be 3 (reverting B: 5b, 4b, 3b)");
    }


    #[test]
    fn find_ancestor_direct_descendant() {
        let genesis = Header::new(0, H256::random(), H256::random(), H256::zero(), Default::default());
        let mut mock_backend = MockHeaderBackend::new(genesis.clone());

        // 0 -> 1 -> 2 -> 3 -> 4 -> 5
        let chain = mock_backend.build_chain(genesis.hash(), 5);

        let heaviest_chain = setup_heaviest_chain(mock_backend);

        let header_3 = &chain[2]; // Height 3
        let header_5 = &chain[4]; // Height 5

        // Case 1: Compare 5 (current) against 3 (competing ancestor)
        let (ancestor1, depth1) = heaviest_chain.find_common_ancestor_and_depth(header_5, header_3).unwrap();
        assert_eq!(ancestor1, header_3.hash(), "Common ancestor should be header 3");
        // Reverting 5 back to 3. Depth is 5 - 3 = 2.
        assert_eq!(depth1, 2, "Reorg depth should be 2 (reverting 5, 4)");

        // Case 2: Compare 3 (current) against 5 (competing descendant)
        let (ancestor2, depth2) = heaviest_chain.find_common_ancestor_and_depth(header_3, header_5).unwrap();
        assert_eq!(ancestor2, header_3.hash(), "Common ancestor should be header 3 (reversed)");
        // Reverting 3 back to 3. Depth is 3 - 3 = 0. Loop 2 adjusts height, Loop 3 finds match immediately.
        assert_eq!(depth2, 0, "Reorg depth should be 0 (no reverts needed on current=3)");
    }

     #[test]
    fn find_ancestor_same_header() {
        let genesis = Header::new(0, H256::random(), H256::random(), H256::zero(), Default::default());
        let mut mock_backend = MockHeaderBackend::new(genesis.clone());

        // 0 -> 1 -> 2
        let chain = mock_backend.build_chain(genesis.hash(), 2);

        let heaviest_chain = setup_heaviest_chain(mock_backend);
        let header_2 = chain.last().unwrap();

        let (ancestor, depth) = heaviest_chain.find_common_ancestor_and_depth(header_2, header_2).unwrap();

        assert_eq!(ancestor, header_2.hash(), "Common ancestor should be itself");
        assert_eq!(depth, 0, "Reorg depth should be 0");
    }

}

// --- Helper Structs (Outside mod tests if needed elsewhere, or keep inside) ---

// Example QPowAlgorithm struct definition (adjust fields as necessary)
// We might not need a fully functional one if find_common_ancestor doesn't use it.
#[derive(Clone)]
pub struct QPowAlgorithm<B, C> {
    client: Arc<C>,
    // other fields...
    _marker: std::marker::PhantomData<B>,
}

impl<B, C> QPowAlgorithm<B, C>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + Send + Sync, // Add other necessary traits
    // C::Api: QPoWApi<B>, // This might be needed depending on actual usage
{
    pub fn new(
        client: Arc<C>,
        _external_miner_url: Option<String>,
        _key: Option<sp_core::sr25519::Pair>,
        _registry: prometheus::Registry,
    ) -> Self {
        Self {
            client,
            _marker: std::marker::PhantomData,
        }
    }
} 