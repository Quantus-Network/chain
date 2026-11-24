# Wormhole ZK: Leaf, Private Batch, Public Batch

> Wormhole ZK: each leaf proof burns one deposit and reveals 1 nullifier; a
> private batch aggregates 8 leaves and pays up to 32 exits, and a public batch
> aggregates private batches. Both batch levels are verified on-chain.

The wormhole flow has three proof levels:

| Level | Produced by | Inputs | Outputs | Verified by chain? |
|------:|-------------|--------|---------|--------------------|
| Leaf  | Client (per transfer) | 1 deposit | 1 nullifier and the burned amount | No |
| Private batch (ZK) | Client | Up to `N = 8` leaves (rest = dummies) | `M = 32` exit slots, `N = 8` nullifiers | **Yes** |
| Public batch (non-ZK) | Any aggregator (delegatable) | Up to `n_inner = 86` private-batch proofs (rest = dummies) | `n_inner · M` exit slots, `n_inner · N` nullifiers | **Yes** |

---

## 1. Individual (leaf) proof

One leaf proof = one proof of burn: it shows that a deposit to a wormhole
address exists and reveals its nullifier. It names no exit.

- **Inputs (private):** secret, recipient `transfer_count`, the
  `unspendable_account = H(salt + secret)`, the block header pre-image and the
  4-ary ZK Merkle path proving `(to, transfer_count, asset_id, amount)` is in
  the tree rooted at `header.zk_tree_root`.
- **Public inputs (`PUBLIC_INPUTS_FELTS_LEN = 11` felts):**
  `asset_id(1), nullifier(4), block_hash(4), block_number(1), input_amount(1)`.
  `input_amount` is consumed by the private batch and is not forwarded.
- **Constraints:** nullifier = `H(H(salt ‖ secret ‖ transfer_count))`,
  Merkle proof root = `header.zk_tree_root`, block hash =
  `H(header pre-image)`.

Exits and the fee are decided one level up, in the private batch. A "dummy"
leaf is identified by `block_hash == 0`; the leaf circuit skips all validation
in that case and constrains `input_amount == 0`. The leaf circuit is not
zero-knowledge, so a leaf proof never leaves the client.

Source: `qp-wormhole-circuit/src/{circuit.rs,zk_merkle_proof.rs}` and
`qp-wormhole-inputs/src/lib.rs`.

---

## 2. Private-batch proof (client → chain)

`PrivateBatchProver` in `qp-wormhole-aggregator/src/private_batch/prover/` and
the circuit in `src/private_batch/circuit/circuit_logic.rs`. Built into the
pallet by `pallets/wormhole/build.rs` with the deployed shape `N = 8` leaves and
`M = 32` exit slots (`qp_wormhole_inputs::production`; override with the
`QP_NUM_LEAF_PROOFS` / `QP_NUM_EXIT_SLOTS` env vars at build time).

What the private-batch circuit does:

1. Recursively verifies `N` leaf proofs against the leaf verifier data.
2. Enforces all **real** leaves agree on `block_hash` and `asset_id`. Leaves
   with `block_hash == 0` are treated as dummies and exempted from this check.
3. Takes `M` prover-chosen exit slots `[amount(1 felt), exit(4 felts)]` and the
   batch `volume_fee_bps` as witnesses, sums the `input_amount` of every real
   leaf and enforces the fee/balance check
   `sum(amounts) · 10000 ≤ sum(inputs) · (10000 − fee_bps)`.
   Slots are forwarded verbatim: unused slots are `(0, zero account)` and
   several slots may name the same account.
4. Replaces dummy nullifiers with `H(H(preimage))` from caller-provided random
   preimages, so dummies cannot be linked across batches.

Aggregated PI layout (`qp_wormhole_inputs::private_batch_pi`):

```text
[ num_exit_slots(1), asset_id(1), volume_fee_bps(1),
  block_hash(4), block_number(1),
  [amount(1), exit(4)] · M,
  nullifier(4) · N ]                       total = 8 + 5·M + 4·N felts
```

Anywhere from 1 to `N` real leaves work; the rest are padded with dummies. A
single all‑dummy batch is also valid (block hash on the wrapper output will
be zero).

### On‑chain verification

`pallet_wormhole::verify_private_batch` (`pallets/wormhole/src/lib.rs`) is a
`None`-origin call submitted as a bare unsigned extrinsic, so it is gated by
`ValidateUnsigned`:

1. `validate_unsigned` (pool admission): deserialize, parse PIs, check
   `asset_id == 0`, `volume_fee_bps` matches `T::VolumeFeeRateBps::get()`,
   `block_hash` matches the on‑chain header at `block_number`, and at least one
   exit slot is mintable. No per‑nullifier storage reads and no ZK verify here,
   so a flood of garbage blobs costs the node only the cheap structural work.
2. `pre_dispatch` (block inclusion): repeat the structural checks, require that
   some segment still has an unused nullifier, run full plonky2 verification,
   then write `VerifiedProofHash = blake2_256(proof_bytes)`.
3. The call body consumes `VerifiedProofHash` and fails with `ProofNotVerified`
   if it is missing or does not match the submitted bytes. This is what keeps
   any other dispatch route (batching, governance, future `None`-origin
   forwarders) from settling an unverified proof. It then re-checks nullifiers
   against `UsedNullifiers`, since an earlier extrinsic in the same block may
   have spent them, and marks each nullifier used.
4. Walk the `M` exit slots, skipping any with `exit == [0;32]` or
   `amount == 0` (unused slots).
5. Mint `amount · 10^10` (circuit uses 2dp `u32`, chain uses 12dp `u128`) to each
   surviving exit; record each transfer in `pallet-zk-tree` so the new mint
   becomes a fresh leaf available for future wormhole exits.
6. Fee handling: for each accepted private segment, compute
   `segment_fee = ceil(segment_minted_quanta · bps / (10000 − bps))`, then sum
   the segment fees. A private-batch proof has one segment; a public batch has
   one segment per inner private batch. Split the total per
   `VolumeFeesBurnRate`: the burn portion reduces `total_issuance`, and the
   miner portion is minted to the QPoW block author. If no author is found,
   the miner portion is burned instead.

---

## 3. Public-batch proof (delegatable)

`PublicBatchAggregator` and `PublicBatchCircuit` in
`qp-wormhole-aggregator/src/{aggregator.rs,public_batch/...}`. The circuit verifies
`n_inner` private-batch proofs and emits a single public-batch proof.

- The aggregator pads unused capacity with dummy private-batch proofs.
- All inner private-batch proofs must agree on `block_hash`, `asset_id`,
  `volume_fee_bps`.
- Adds an `aggregator_address` (witness, 4 felts) to the PIs identifying the
  server; otherwise just forwards exit slots and nullifiers.

public-batch PI layout (`qp-wormhole-aggregator/src/public_batch/circuit/constants.rs`):

```text
[ aggregator_address(4),
  asset_id(1), volume_fee_bps(1),
  block_hash(4), block_number(1),
  total_exit_slots(1),
  [amount(1), exit(4)] · (n_inner · M),
  nullifier(4) · (n_inner · N) ]
```

### On-chain verification

`pallets/wormhole/build.rs` generates and embeds the public-batch verifier for
`QP_NUM_PRIVATE_BATCH_PROOFS` (86 by default). `verify_public_batch` parses the
proof into one settlement segment per inner private batch. A spent nullifier
denies only its segment; other valid segments still settle.

Transaction-pool tags sort nullifiers within each segment before hashing, so a
private nullifier permutation does not create another pool identity. Segment
order and boundaries remain part of the tag. The independently rounded fee of
each accepted segment is summed before the burn/miner split, and public batches
can redirect part of the burn bucket to their proof-bound aggregator address.

---

## Key constants and where to look

| Item | Location |
|---|---|
| Leaf PI length (11) | `qp-wormhole-inputs/src/lib.rs` (`PUBLIC_INPUTS_FELTS_LEN`) |
| Private-batch wrapper PI layout | `qp-wormhole-inputs/src/lib.rs` (`private_batch_pi`) |
| Public-batch wrapper PI layout | `qp-wormhole-inputs/src/lib.rs` (`public_batch_pi`) |
| `N`, `M`, public-batch capacity (8, 32, 86) | `qp-wormhole-inputs/src/lib.rs` (`production`); `pallets/wormhole/build.rs` overrides them with `QP_NUM_LEAF_PROOFS`, `QP_NUM_EXIT_SLOTS`, `QP_NUM_PRIVATE_BATCH_PROOFS` |
| Canonical batch circuit configs | `qp-wormhole-verifier/src/lib.rs` (`canonical_private_batch_circuit_config`, `canonical_public_batch_circuit_config`) |
| Embedded verifier bytes | `pallets/wormhole/src/lib.rs` (`PRIVATE_BATCH_VERIFIER`, `PUBLIC_BATCH_VERIFIER`) |
| On‑chain verify entrypoints | `pallet_wormhole::{verify_private_batch, verify_public_batch}` |
| Amount scale (10^10) | `pallets/wormhole/src/lib.rs` (`SCALE_DOWN_FACTOR`) |
| Per-leaf amount cap and credit splitting | `pallets/zk-tree/src/tree.rs` (`LEAF_AMOUNT_CAP`, `credit_chunks`) |
| 4‑ary Poseidon Merkle tree | `pallets/zk-tree/`, see `docs/zk-trie-architecture.md` |
