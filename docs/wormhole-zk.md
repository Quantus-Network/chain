# Wormhole ZK: Leaf, Private Batch, Public Batch

> Wormhole ZK: each leaf proof spends 1 nullifier, pays up to 2 exits; the private batch aggregates 16 leaves (pads with dummies); the public batch bundles private batches (delegatable). Chain currently verifies only the private batch.

The wormhole flow has three proof levels. Today the runtime only verifies the private batch;
the public batch is built in the external `qp-wormhole-aggregator` crate but is **not** wired
into `pallet-wormhole` on the current `main`.

| Level | Produced by | Inputs | Outputs | Verified by chain? |
|------:|-------------|--------|---------|--------------------|
| Leaf  | Client (per transfer) | 1 nullifier (1 spend) | Up to 2 exit accounts (spend + change) | No |
| Private batch (ZK) | Client (aggregator) | Up to `N = 16` leaves (rest = dummies) | `2·N = 32` exit slots, `N = 16` nullifiers | **Yes** |
| Public batch (non-ZK) | Any aggregator (delegatable) | `n_inner` full private-batch proofs (no padding yet) | `n_inner · 2N` exit slots, `n_inner · N` nullifiers | **No** (not enabled) |

---

## 1. Individual (leaf) proof

One leaf proof = one user "exit" from a wormhole address.

- **Inputs (private):** secret, recipient `transfer_count`, the
  `unspendable_account = H(salt + secret)`, the block header pre‑image, the
  4‑ary ZK Merkle path proving `(to, transfer_count, asset_id, amount)` is in
  the tree rooted at `header.zk_tree_root`, and the raw `input_amount`.
- **Public inputs (`PUBLIC_INPUTS_FELTS_LEN = 21` felts):**
  `asset_id(1), output_amount_1(1), output_amount_2(1), volume_fee_bps(1),`
  `nullifier(4), exit_account_1(4), exit_account_2(4),`
  `block_hash(4), block_number(1)`.
- **Constraints:** nullifier = `H(H(salt ‖ secret ‖ transfer_count))`,
  Merkle proof root = `header.zk_tree_root`, block hash =
  `H(header pre‑image)`, and a Bitcoin‑style fee/balance check:
  `(out_1 + out_2) · 10000 ≤ input · (10000 − fee_bps)`.

So the unit of a leaf proof is **1 input → up to 2 outputs**, not 1‑in/1‑out.
A "dummy" leaf is identified by `block_hash == 0` **and** both outputs `== 0`;
the leaf circuit short‑circuits all validation in that case.

Source: `qp-wormhole-circuit/src/{circuit.rs,zk_merkle_proof.rs}` and
`qp-wormhole-inputs/src/lib.rs`.

---

## 2. Private-batch proof (client → chain)

`PrivateBatchAggregator` in `qp-wormhole-aggregator/src/aggregator.rs` and the
monolithic circuit in `src/private_batch/circuit/circuit_logic.rs`. Built into the
pallet by `pallets/wormhole/build.rs`; `N = num_leaf_proofs = 16` by default
(override with the `QP_NUM_LEAF_PROOFS` env var at build time).

What the private-batch circuit does:

1. Recursively verifies `N` leaf proofs against the leaf verifier data.
2. Enforces all **real** leaves agree on `block_hash`, `asset_id`,
   `volume_fee_bps`. Slots with `block_hash == 0` are treated as dummies and
   exempted from this check.
3. Builds `2·N` exit slots `[sum(1 felt), exit(4 felts)]`. For each slot it
   sums all amounts across all `2·N` outputs whose exit matches; if the slot's
   exit already appeared earlier, the slot is zeroed (dedupe → identical to a
   dummy slot).
4. Replaces dummy nullifiers with `H(H(preimage))` from caller‑provided random
   preimages, so dummies cannot be deduplicated or linked across batches.

Aggregated PI layout (`qp-wormhole-aggregator/src/private_batch/circuit/constants.rs`):

```text
[ num_exit_slots(1), asset_id(1), volume_fee_bps(1),
  block_hash(4), block_number(1),
  [sum(1), exit(4)] · (2·N),
  nullifier(4) · N,
  padding ]                                total = N·21 + 8 felts
```

Anywhere from 1 to 16 real leaves work; the rest are padded with dummies. A
single all‑dummy batch is also valid (block hash on the wrapper output will
be zero).

### On‑chain verification

`pallet_wormhole::verify_private_batch` (`pallets/wormhole/src/lib.rs`):

1. `validate_proof`: deserialize, parse PIs, check `asset_id == 0`,
   `volume_fee_bps` matches `T::VolumeFeeRateBps::get()`, `block_hash` matches
   the on‑chain header at `block_number`, no nullifier already in
   `UsedNullifiers`, then run full plonky2 verification.
2. Mark each nullifier used.
3. Walk the `2·N` exit slots, skipping any with `exit == [0;32]` or `sum == 0`
   (covers dummies + dedup'd slots).
4. Mint `sum · 10^10` (circuit uses 2dp `u32`, chain uses 12dp `u128`) to each
   surviving exit; record each transfer in `pallet-zk-tree` so the new mint
   becomes a fresh leaf available for future wormhole exits.
5. Fee handling: `fee = total_output · bps / (10000 − bps)`. Split per
   `VolumeFeesBurnRate`: burn portion reduces `total_issuance`, miner portion
   minted to the QPoW block author from the pre‑runtime digest. If no author
   is found, the miner portion is burned instead.

---

## 3. Public-batch proof (delegatable) — **not enabled on chain**

`PublicBatchAggregator` and `PublicBatchCircuit` in
`qp-wormhole-aggregator/src/{aggregator.rs,public_batch/...}`. The circuit verifies
`n_inner` private-batch proofs and emits a single public-batch proof.

- **No padding at the public batch (yet; dummy private batches are planned).** `PublicBatchAggregator::aggregate` calls `drain_exact(cap)`
  — it errors unless the buffer holds a full batch of `n_inner` private-batch proofs.
- All inner private-batch proofs must agree on `block_hash`, `asset_id`,
  `volume_fee_bps`.
- Adds an `aggregator_address` (witness, 4 felts) to the PIs identifying the
  server; otherwise just forwards exit slots and nullifiers (no extra dedupe).

public-batch PI layout (`qp-wormhole-aggregator/src/public_batch/circuit/constants.rs`):

```text
[ aggregator_address(4),
  asset_id(1), volume_fee_bps(1),
  block_hash(4), block_number(1),
  total_exit_slots(1),
  [sum(1), exit(4)] · (n_inner · 2·N),
  nullifier(4) · (n_inner · N) ]
```

### Status on current `main`

The public batch is **not currently verified by the chain.** `pallets/wormhole/build.rs`
calls `generate_all_circuit_binaries(..., num_private_batch_proofs = None)`, so no
`public_batch_verifier.bin` / `public_batch_common.bin` are produced, and the pallet only
embeds the private-batch wrapper verifier. Enabling public batches would require:

1. Pass `Some(n_inner)` from `build.rs`.
2. Embed `public_batch_verifier.bin` / `public_batch_common.bin` in the pallet.
3. Add a `verify_public_batch` path parsing the public-batch PI layout (which has the
   `aggregator_address` prefix and no `num_unique_exits` field).

---

## Key constants and where to look

| Item | Location |
|---|---|
| Leaf PI length (21) | `qp-wormhole-inputs/src/lib.rs` (`PUBLIC_INPUTS_FELTS_LEN`) |
| Private-batch wrapper PI layout | `qp-wormhole-aggregator/src/private_batch/circuit/constants.rs` |
| Public-batch wrapper PI layout | `qp-wormhole-aggregator/src/public_batch/circuit/constants.rs` |
| `N = num_leaf_proofs` (default 16) | `pallets/wormhole/build.rs` (`QP_NUM_LEAF_PROOFS`) |
| Embedded verifier bytes | `pallets/wormhole/src/lib.rs` (`PRIVATE_BATCH_VERIFIER`) |
| On‑chain verify entrypoint | `pallet_wormhole::verify_private_batch` |
| Amount scale (10^10) | `pallets/wormhole/src/lib.rs` (`SCALE_DOWN_FACTOR`) |
| 4‑ary Poseidon Merkle tree | `pallets/zk-tree/`, see `docs/zk-trie-architecture.md` |
