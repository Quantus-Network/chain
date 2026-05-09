# Miner Aggregation Pallet

MVP pallet for delegated Wormhole L1 aggregation.

Current scope:

- signed users submit bounded on-chain L0 aggregate proof candidates
- candidate submission parses public L0 metadata but does not verify the proof
- candidate submission does not lock or mark nullifiers
- compatible candidates are queued by `BundleGroupKey`
- submitter storage bond, validity bond, and aggregation tip are reserved
- aggregation miners register a reward address, job limit, and bond
- registered miners can claim a full compatible bundle
- bundle claim locks nullifiers through `pallet-wormhole`
- bundle timeout unlocks nullifiers and returns unexpired candidates to the queue
- L1 submission performs cheap public-input/effects checks before full L1 verification
- pending expired candidates can be cleaned up and refunded
- pending invalid candidates can be challenged and have their validity bond burned
- claimed invalid candidates can be challenged before L1 settlement

Nullifier locking remains owned by `pallet-wormhole`. Bundle claim must call the wormhole lock
helpers; candidate submission must never lock nullifiers.

Any test or command that generates ZK proofs must run with `--release`. The default pallet test
suite uses precomputed fixture proof bytes and does not generate proofs.

MVP limitations:

- L1 fixture regeneration must use `chain/scripts/generate-delegated-l1-fixture.sh`, which runs
  proving in release mode.
- The positive L1 settlement fixture test requires `QP_GENERATE_LAYER1=true` and
  `QP_NUM_LAYER0_PROOFS=1` so `pallet-wormhole` embeds matching L1 verifier artifacts.
- `Bundle.bundle_root` remains metadata until the L1 circuit exposes a constrained public root.
