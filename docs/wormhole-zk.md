# Wormhole ZK: Leaf, Layer‑0, Layer‑1

> Wormhole ZK: each leaf proof spends 1 nullifier, pays up to 2 exits; layer‑0 aggregates `N` leaves (pads with dummies); the chain accepts multiple aggregation sizes simultaneously (default: `N ∈ {2, 16}`); layer‑1 batches L0 proofs server‑side.

The wormhole flow has three proof levels. The runtime verifies L0 against one
of several baked‑in verifiers, picked at call time via a `num_leaf_proofs`
argument on the extrinsic. L1 is built in the external `qp-wormhole-aggregator`
crate but is **not** wired into `pallet-wormhole` on the current `main`.

| Level | Produced by | Inputs | Outputs | Verified by chain? |
|------:|-------------|--------|---------|--------------------|
| Leaf  | Client (per transfer) | 1 nullifier (1 spend) | Up to 2 exit accounts (spend + change) | No |
| L0    | Client (aggregator) | Up to `N` leaves (rest = dummies). `N` is one of `pallet_wormhole::SUPPORTED_NUM_LEAF_PROOFS`. | `2·N` exit slots, `N` nullifiers | **Yes** |
| L1    | Server / delegated aggregator | `n_inner` full L0 proofs (no padding) | `n_inner · 2N` exit slots, `n_inner · N` nullifiers | **No** (not enabled) |

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

## 2. Layer‑0 aggregated proof (client → chain)

`Layer0Aggregator` in `qp-wormhole-aggregator/src/aggregator.rs` and the
monolithic circuit in `src/layer0/circuit/circuit_logic.rs`. Built into the
pallet by `pallets/wormhole/build.rs`. The pallet bakes in **one verifier per
supported `N`** simultaneously. The set of supported sizes is read from the
build environment:

  - `QP_NUM_LEAF_PROOFS_LIST=2,16` — comma‑separated list (recommended)
  - `QP_NUM_LEAF_PROOFS=N` — legacy single‑value fallback

Default if neither is set: `2,16`. `N = 2` keeps the prover under ~1 GB of
peak memory (mobile‑friendly); `N = 16` provides the strongest privacy
(largest anonymity set per submitted batch). Clients pick which one to
produce based on their device capabilities; the chain accepts both.

What the L0 circuit does:

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

Aggregated PI layout (`qp-wormhole-aggregator/src/layer0/circuit/constants.rs`):

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

`pallet_wormhole::verify_aggregated_proof(origin, proof_bytes, num_leaf_proofs)`
(`pallets/wormhole/src/lib.rs`):

1. `validate_proof`: look up the verifier for `num_leaf_proofs` (rejects with
   `AggregatedVerifierNotAvailable` if no verifier was baked in for that `N`),
   deserialize, parse PIs, check the parsed PI layout matches the claimed
   `N` (defense‑in‑depth — the matching `common_data` already enforces this
   via deserialization), check `asset_id == 0`, `volume_fee_bps` matches
   `T::VolumeFeeRateBps::get()`, `block_hash` matches the on‑chain header
   at `block_number`, no nullifier already in `UsedNullifiers`, then run
   full plonky2 verification.
2. Mark each L0 nullifier used.
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

## 3. Layer‑1 aggregated proof (server‑side) — **not enabled on chain**

`Layer1Aggregator` and `Layer1AggregationCircuit` in
`qp-wormhole-aggregator/src/{aggregator.rs,layer1/...}`. The circuit verifies
`n_inner` L0 proofs and emits a single L1 proof.

- **No padding at L1.** `Layer1Aggregator::aggregate` calls `drain_exact(cap)`
  — it errors unless the buffer holds a full batch of `n_inner` L0 proofs.
- All inner L0 proofs must agree on `block_hash`, `asset_id`,
  `volume_fee_bps`.
- Adds an `aggregator_address` (witness, 4 felts) to the PIs identifying the
  server; otherwise just forwards exit slots and nullifiers (no extra dedupe).

L1 PI layout (`qp-wormhole-aggregator/src/layer1/circuit/constants.rs`):

```text
[ aggregator_address(4),
  asset_id(1), volume_fee_bps(1),
  block_hash(4), block_number(1),
  total_exit_slots(1),
  [sum(1), exit(4)] · (n_inner · 2·N),
  nullifier(4) · (n_inner · N) ]
```

### Status on current `main`

L1 is **not currently verified by the chain.** `pallets/wormhole/build.rs`
calls `generate_all_circuit_binaries(..., num_layer0_proofs = None)`, so no
`layer1_verifier.bin` / `layer1_common.bin` are produced, and the pallet only
embeds the L0 wrapper verifier. Enabling L1 would require:

1. Pass `Some(n_inner)` from `build.rs`.
2. Embed `layer1_verifier.bin` / `layer1_common.bin` in the pallet.
3. Switch `verify_aggregated_proof` to parse the L1 PI layout (which has the
   `aggregator_address` prefix and no `num_unique_exits` field).

---

## Key constants and where to look

| Item | Location |
|---|---|
| Leaf PI length (21) | `qp-wormhole-inputs/src/lib.rs` (`PUBLIC_INPUTS_FELTS_LEN`) |
| L0 wrapper PI layout | `qp-wormhole-aggregator/src/layer0/circuit/constants.rs` |
| L1 wrapper PI layout | `qp-wormhole-aggregator/src/layer1/circuit/constants.rs` |
| Supported `N` set (default `{2, 16}`) | `pallets/wormhole/build.rs` (`QP_NUM_LEAF_PROOFS_LIST`) |
| Runtime accessor for the supported set | `pallet_wormhole::SUPPORTED_NUM_LEAF_PROOFS` |
| Embedded verifier bytes | `pallets/wormhole/src/lib.rs` (`AGGREGATED_VERIFIER`) |
| On‑chain verify entrypoint | `pallet_wormhole::verify_aggregated_proof` |
| Amount scale (10^10) | `pallets/wormhole/src/lib.rs` (`SCALE_DOWN_FACTOR`) |
| 4‑ary Poseidon Merkle tree | `pallets/zk-tree/`, see `docs/zk-trie-architecture.md` |
