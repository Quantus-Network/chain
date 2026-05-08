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

Nullifier locking remains owned by `pallet-wormhole`. Bundle claim must call the wormhole lock
helpers; candidate submission must never lock nullifiers.

Any test or command that generates ZK proofs must run with `--release`. The current pallet tests use
precomputed fixture proof bytes and do not generate proofs.

MVP limitations:

- Claimed-bundle invalid-candidate challenge and partial miner slashing are not implemented yet.
- L1 proof settlement is wired to the L1 verifier helper, but the test suite currently covers cheap
  rejection only because no local L1 aggregate proof fixture is available.
