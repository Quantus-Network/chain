# Delegated L1 Aggregation

Wormhole proving is split into two layers:

- Layer 0 remains client-side because L0 proof generation touches private witness data.
- Layer 1 is delegated to bonded aggregation miners and only aggregates already-public L0 aggregate proofs.

The protocol invariant is:

```text
Candidate submission does not lock nullifiers.
Bundle claim locks nullifiers.
L1 settlement consumes locked nullifiers and marks them used.
Direct L0 verification rejects used or locked nullifiers.
```

The MVP candidate path stores bounded L0 aggregate proof bytes on-chain in
`pallet-miner-aggregation`. Submission parses public metadata and queues candidates by
`BundleGroupKey`, but it does not run full ZK verification, settle exits, lock nullifiers, or mark
nullifiers used.

Nullifier state is owned by `pallet-wormhole`:

- `UsedNullifiers` tracks nullifiers consumed by settlement.
- `LockedNullifiers` tracks nullifiers exclusively leased to an aggregation bundle.

This prevents the direct-L0 race after a miner claims a bundle, while avoiding the opposite attack
where unverified candidate spam locks nullifiers.

Current reward and bond model:

- candidate submitters reserve a storage bond, validity bond, and aggregation tip
- aggregation miners register a reward address, active-job limit, and bond
- bundle claim reserves a miner bond and locks all bundle nullifiers
- timeout releases bundle locks, returns unexpired candidates to the queue, and releases the miner bond
- successful L1 settlement consumes locked nullifiers, marks candidates settled, releases candidate bonds, and pays candidate tips to the configured aggregation reward account
- pending expired candidates can be dropped and refunded
- pending invalid candidates can be challenged; if full L0 verification fails, the candidate is marked invalid and its validity bond is burned

MVP limitations:

- L1 public inputs do not yet expose a constrained `bundle_root`; settlement compares the reconstructed public effects instead.
- Invalid-candidate challenge currently handles pending candidates only. Claimed-bundle challenge and partial miner slashing are left for the next hardening pass.
- The external miner has separate ZK aggregation CLI/config/worker-pool scaffolding, but the chain RPC watcher and L1 prover submission loop are not complete.
- The full local E2E needs generated L1 prover/verifier artifacts and client L0 proof fixtures.

ZK proving tests must use release mode:

```bash
cargo test --release -p <zk-crate> <test_name> -- --nocapture
cargo run --release -p <prover-binary> -- <args>
```

Parser-only, storage-only, mock-runtime, and precomputed-proof fixture tests may run without
`--release`.
