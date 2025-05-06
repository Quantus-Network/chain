#![cfg(test)]

use std::sync::Arc;

use sc_client_api::Backend as BackendT;
use sp_api::ProvideRuntimeApi;
use sp_consensus_qpow::QPoWApi;
use sp_runtime::traits::{Block as BlockT, Header};

use substrate_test_runtime_client::{
    runtime::Block, DefaultTestClientBuilderExt, TestClientBuilderExt,
};

use crate::{HeaviestChain, QPowAlgorithm};

type TestBlock = Block;
type Hash = <TestBlock as BlockT>::Hash;
type Number = <<TestBlock as BlockT>::Header as Header>::Number;

/// Helper that gives us an Arc<Client> + Arc<Backend> ready to use.
fn new_client() -> (Arc<substrate_test_runtime_client::TestClient>, Arc<substrate_test_runtime_client::Backend>) {
    let builder = substrate_test_runtime_client::TestClientBuilder::new();
    builder.build_with_backend()
}

#[tokio::test]
async fn reorg_depth_is_computed_correctly() {
    let (client, backend) = new_client();

    // Build a tiny fork: A –> B –> C   (canonical)
    //                    \\-> B2       (fork one block behind)
    //
    // We only care about numbers/hashes so opaque bodies are fine.
    let genesis_hash = client.genesis_hash();
    let mut parent = genesis_hash;

    // canonical: #1
    let block1 = client.import_block(parent, Vec::<u8>::new(), true, Default::default()).unwrap();
    parent = block1.block.header.hash();

    // canonical: #2
    let block2 = client.import_block(parent, Vec::new(), true, Default::default()).unwrap();
    let tip_canonical = block2.block.header.clone();

    // fork: alternative #2 (B2)
    let fork_block = client.import_block(block1.block.header.hash(), Vec::new(), true, Default::default()).unwrap();
    let tip_fork = fork_block.block.header.clone();

    // QPoWAlgorithm isn’t used by `find_common_ancestor_and_depth`, so default is fine.
    let hc = HeaviestChain::<TestBlock, _, _>::new(backend, client.clone(), QPowAlgorithm::default());

    let (_ancestor, depth) =
        hc.find_common_ancestor_and_depth(&tip_canonical, &tip_fork).unwrap();

    assert_eq!(depth, 1u32);
}

/// When two chains have equal total work, the longer one should win.
/// We fake “total work” by setting QPoWApi::get_total_work to `Ok(height)`.
struct DummyApi;
impl QPoWApi<TestBlock> for DummyApi {
    fn get_total_work(&self, hash: Hash) -> Result<primitive_types::U512, sp_api::ApiError> {
        let h: Number = hash.as_ref()[0].into(); // super­cheap “height” extraction
        Ok(primitive_types::U512::from(h))
    }
    fn get_max_reorg_depth(&self, _: Hash) -> Result<u32, sp_api::ApiError> {
        Ok(100)
    }
    fn get_max_distance(&self, _: Hash) -> Result<primitive_types::U512, sp_api::ApiError> {
        Ok(primitive_types::U512::zero())
    }
    fn get_nonce_distance(
        &self,
        _: Hash,
        _: [u8; 32],
        _: [u8; 64],
    ) -> Result<primitive_types::U512, sp_api::ApiError> {
        Ok(primitive_types::U512::zero())
    }
    fn get_distance_threshold(&self, _: Hash) -> Result<primitive_types::U512, sp_api::ApiError> {
        Ok(primitive_types::U512::zero())
    }
}

impl ProvideRuntimeApi<TestBlock> for substrate_test_runtime_client::TestClient {
    type Api = DummyApi;
    fn runtime_api(&self) -> sp_api::ApiRef<'_, Self::Api> {
        sp_api::ApiRef::Borrowed(&DummyApi)
    }
}

#[tokio::test]
async fn tie_breaker_prefers_longer_chain_on_equal_work() {
    let (client, backend) = new_client();

    // Build canonical of length 3, competing of length 4 but equal “work”.
    let mut parent = client.genesis_hash();
    for _ in 0..3 {
        let b = client.import_block(parent, Vec::new(), true, Default::default()).unwrap();
        parent = b.block.header.hash();
    }
    let tip_short = client.header(parent).unwrap().unwrap();

    let mut parent_fork = client.header(client.genesis_hash()).unwrap().unwrap().hash();
    for _ in 0..4 {
        let b = client.import_block(parent_fork, Vec::new(), true, Default::default()).unwrap();
        parent_fork = b.block.header.hash();
    }
    let tip_long = client.header(parent_fork).unwrap().unwrap();

    let hc = HeaviestChain::<TestBlock, _, _>::new(backend, client.clone(), QPowAlgorithm::default());

    let best = hc.best_chain().await.unwrap();
    assert_eq!(best.hash(), tip_long.hash());
}