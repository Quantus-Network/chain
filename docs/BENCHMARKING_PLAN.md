# Plan: In-Runtime Benchmarking to Replace Hand-Rolled Weight Accounting

## Why

Every pallet in this runtime currently uses **borrowed weight numbers**: the
`weights.rs` files vendored under `pallets/*` are copies of upstream
polkadot-sdk benchmarks, measured on a vanilla Substrate runtime with sr25519
signatures, Blake2 hashing, and none of this chain's hooks. Because those
numbers cannot see our runtime, every cost specific to this chain has been
modeled by hand, and that hand-modeling is where a disproportionate share of
review findings and code now lives:

- `ChargePubkeyCacheVerify`: a hand-priced verify surcharge (`PubkeyCacheVerifyWeight`) plus a
  `count_reaps` call matcher, an `ExtrinsicReaps` counter, an execution-phase check in
  `OnKilledAccount`, and post-dispatch refund/shortfall reconciliation — all to charge **one DB
  write** that in-runtime benchmarks would have priced automatically (the vendored
  `pallet_balances` benchmark for `transfer_allow_death` deliberately reaps the sender, so a
  benchmark run against *this* runtime executes our `OnKilledAccount = Pubkey` hook and measures
  its cost into the call weight, compositionally through `utility.batch` for free).
- `pallet_zk_tree`: hand-derived constants (`INSERT_LEAF_DB_OPS`, `INSERT_LEAF_HASH_REF_TIME_PS`,
  `TREE_KEY_POV`) consumed by the wormhole recorder's `per_transfer_weight`.
- `pallet_vesting`: a hand-built PoV model the 2026-08 security review flagged as double-counting
  (accepted as a conservative overcharge, deferred).
- `ExtrinsicBaseWeight` / `BlockExecutionWeight` / `RocksDbWeight`: upstream constants. The base
  extrinsic weight in particular was measured with **sr25519** verification; this chain verifies
  **ML-DSA (Dilithium)** signatures inside the wasm runtime, which is far more expensive. This is
  a live undercharge, not just an elegance problem.

The structural fix is to run `frame-benchmarking` against this runtime on
reference hardware and regenerate all weights. Costs incurred by hooks,
custom crypto, and custom storage layouts then land in the benchmarked call
weights by construction, and most of the bespoke accounting code can be
deleted.

## Current state (inventory)

Already in place — the setup cost is lower than it looks:

| Piece | Status |
| --- | --- |
| `benchmarking.rs` per pallet | Present in all vendored pallets (`balances`, `multisig`, `recovery`, `reversible-transfers`, `wormhole`, `vesting`, `qpow`, `mining-rewards`, …) |
| `runtime/src/benchmarks.rs` | `define_benchmarks!` already lists 17 targets (system, balances, timestamp, transaction-payment, all local pallets) |
| Node CLI | `benchmark` subcommand fully wired (`frame-benchmarking-cli`, `ExtrinsicFactory` with real Dilithium-signed extrinsics in `node/src/benchmarking.rs`) |
| CI | `cargo check --features runtime-benchmarks,try-runtime` gate exists (compile-only) |

Missing:

| Gap | Consequence today |
| --- | --- |
| Nobody runs the benchmarks; `weights.rs` are upstream copies | All hand-modeling described above |
| `pallet_qpow`, `pallet_timestamp`: `type WeightInfo = ()` | Zero-weight inherents |
| `pallet_zk_tree`: no `benchmarking.rs`/`weights.rs` | Hand-derived insert constants |
| `frame_system` uses `SolochainDefaultConfig` defaults for `SystemWeightInfo`/`ExtensionsWeightInfo` | Stock numbers for system calls and `Check*` extensions |
| No measured `ExtrinsicBaseWeight`/`BlockExecutionWeight`/`DbWeight` | sr25519-era base costs on a Dilithium chain |
| No benchmark machine / CI cadence | Numbers would rot |

## Phase 0 — Reference hardware and ground rules

1. Pick the reference machine (the minimum-spec validator/miner target, not a laptop). Run
   `quantus-node benchmark machine` and record the result in `docs/` — this is the hardware
   contract behind every weight.
2. Ground rules to adopt up front:
   - Weights are only ever regenerated on the reference machine (or its pinned CI runner), with
     the wasm executor, release build.
   - Every regeneration lands as its own PR with the generation command in the commit message, so
     diffs are reviewable and reproducible.

## Phase 1 — Foundational constants

These feed everything else, so they go first:

1. **`benchmark storage`** → generate a real `DbWeight` (RocksDB read/write on the reference
   machine) replacing `RocksDbWeight` in `frame_system::Config`.
2. **`benchmark overhead`** → measured `BlockExecutionWeight` and `ExtrinsicBaseWeight`. The
   extrinsic factory already builds genuine Dilithium-signed transfers, so the measured base
   weight includes ML-DSA verification and the `CachedSignature` cache read.
   - **Decision point:** the current design deliberately keeps signature-verification cost *out*
     of class-wide `base_extrinsic` so bare unsigned extrinsics (wormhole `verify_*`) don't pay
     for a `Verify` they never run, charging it via `ChargePubkeyCacheVerify` instead. Two
     options:
     - (a) Keep that split: set `base_extrinsic` from an *unsigned* overhead run, and set the
       extension's surcharge from the measured signed/unsigned delta (replacing the hand-modeled
       `PubkeyCacheVerifyWeight`). More precise, keeps the extension.
     - (b) Fold the signed overhead into `base_extrinsic` and drop the extension's verify term,
       accepting that unsigned wormhole extrinsics overpay in weight terms. Simpler, taxes a
       rare path.
     Recommendation: (a) — it preserves the reviewed rationale and the extension survives for
     other reasons anyway (see Phase 3).
3. Wire the generated constants into `runtime/src/configs/mod.rs` (`RuntimeBlockWeights`,
   `DbWeight`) behind a `weights/` module in the runtime, mirroring polkadot-sdk layout
   (`runtime/src/weights/{block_weights,extrinsic_weights,rocksdb_weights,...}.rs`).

Expect fees to move when this lands (Dilithium verification is currently underpriced). Re-check
the `WeightToFee`/`LengthToFee` calibration comments in `configs/mod.rs` (1s compute ≈ 1 UNIT)
against the new base costs before shipping.

## Phase 2 — Pallet weights, regenerated against this runtime

1. Run the omnibench for every `define_benchmarks!` target, e.g.:

   ```bash
   quantus-node benchmark pallet \
     --runtime <wasm> --pallet "*" --extrinsic "*" \
     --steps 50 --repeat 20 --heap-pages 4096 \
     --template ./scripts/weight-template.hbs \
     --output pallets/<pallet>/src/weights.rs
   ```

   and commit the regenerated `weights.rs` into the vendored pallets (they are already in-tree,
   so this is a normal diff, not a fork-maintenance problem).
2. Fill the gaps:
   - Write `benchmarking.rs` + `weights.rs` for **`pallet_zk_tree`** (worst case: `insert_leaf`
     at `CIRCUIT_MAX_TREE_DEPTH`, capacity-boundary growth). This replaces the hand-derived
     `INSERT_LEAF_*` constants; the wormhole recorder's `per_transfer_weight` then calls the
     benchmarked function instead of doing DbWeight arithmetic.
   - Generate weights for **`pallet_qpow`** and **`pallet_timestamp`** and drop
     `type WeightInfo = ()`.
   - Switch `frame_system::Config` to generated `SystemWeightInfo` and `ExtensionsWeightInfo`.
3. Verification gates to keep us honest (these are cheap and permanent):
   - Keep the differential regression in `runtime/tests/transactions/sig_only.rs` (a reaping
     `transfer_all` must out-weigh a non-reaping one) — after Phase 3 it pins that the
     *benchmarked* weight really covers the hook.
   - Add one assert per hand-model being replaced: benchmarked weight ≥ the old modeled floor
     (catches a benchmark that silently stopped exercising the worst case).

## Phase 3 — Delete the hand-rolled accounting (the payoff)

Once Phase 2 weights are in:

| Site | Action |
| --- | --- |
| `ChargePubkeyCacheVerify::count_reaps`, `Pre = (u64, u64)`, post-dispatch reconciliation | **Delete.** Kill-capable call weights now include the reap hook. Non-reaping `transfer_allow_death` overpays one write with no refund — the standard FRAME trade, and strictly less machinery. |
| `pallet_pubkey::ExtrinsicReaps` + phase check in `OnKilledAccount` | **Delete** (hook keeps only `Pubkeys::remove`; scheduler-enacted reaps are covered because the scheduler charges the dispatched call's benchmarked weight). Requires a storage migration only if it ships after the counter reaches mainnet; if Phases 1–3 land in one release train, none is needed. |
| `PubkeyCacheVerifyWeight` hand model | **Replace** with the measured signed/unsigned overhead delta (Phase 1, option a). |
| `pallet_zk_tree::INSERT_LEAF_*` constants | **Replace** with benchmarked `WeightInfo`. |
| Vesting PoV double-count (review item, deferred) | **Superseded** by regenerated vesting weights. |
| `WormholeProofRecorderExtension::per_transfer_weight` | **Simplify**: compose from benchmarked zk-tree insert + benchmarked event-deposit costs instead of raw DbWeight math. |

What benchmarking **cannot** replace (keep, documented as such):

- The wormhole recorder's post-dispatch **event scan** and its `register_extra_weight_unchecked`
  reconciliation: its cost depends on how many events *earlier transactions in the block*
  emitted, which no static per-call weight can express. This stays the one deliberate post-hoc
  mechanism (rationale already recorded on the extension).
- `count_transfers` in the wormhole recorder: proof recording is per-*realized* transfer with
  refunds, which a flat benchmarked call weight cannot express. (Its per-unit price gets better
  numbers from Phase 2, but the matcher stays.)
- The `ReversibleTransactionExtension` whitelist read (trivial, already priced).

## Phase 4 — Keeping the numbers alive

1. **CI (every PR, cheap):** extend the existing `runtime-benchmarks` check job to also *execute*
   `benchmark pallet --steps 2 --repeat 1` across all pallets (smoke run, no output committed).
   This catches benchmarks that panic or stop compiling — the most common rot.
2. **Reference runs (scheduled/manual):** a workflow on the pinned reference runner that
   regenerates all weights and opens a PR with the diff. Trigger: manually per release, and
   automatically when `pallets/**` or `runtime/**` storage/dispatch logic changes materially.
3. **Policy:** a PR that adds a call, a storage item touched per dispatch, or a hook must either
   include regenerated weights or explicitly state why the existing worst case still covers it.
   Add this to the review checklist.
4. Consider migrating from the node-embedded CLI to `frame-omni-bencher` (runs against the
   runtime wasm directly, no node build needed) once the pipeline works — mechanical swap, nicer
   CI ergonomics.

## Risks / open decisions

- **Fee level shifts.** Measured Dilithium base weight will raise the base fee of every signed
  extrinsic; regenerated pallet weights move fees both directions. Decide whether to re-tune
  `WeightToFee` (currently `IdentityFee`, 1s ≈ 1 UNIT) or accept the new levels. Do this
  deliberately, in one release, with before/after fee tables for the common extrinsics.
- **Proof size.** Benchmarks measure PoV, but the runtime currently neither caps nor charges
  `proof_size` (documented rationale in `configs/mod.rs`). Landing measured PoV numbers is the
  natural moment to revisit that decision; until then the PoV components are recorded but inert.
- **Worst-case discipline.** A benchmark only covers what it exercises. The two regression
  gates in Phase 2.3 exist precisely because upstream benchmark bodies can drift away from our
  runtime's worst cases (e.g. a future balances update that stops reaping in the benchmark).
- **Sequencing.** Phases 1–3 should land within one release train so the interim state (measured
  base weights + old pallet weights, or counter shipped then deleted) never reaches mainnet.

## Rough effort

| Phase | Estimate |
| --- | --- |
| 0 — machine + rules | ~1 day |
| 1 — storage/overhead constants + wiring | ~2–3 days (incl. fee recalibration analysis) |
| 2 — regenerate + zk-tree/qpow/timestamp gaps | ~1 week (zk-tree benchmark is the only real code) |
| 3 — deletions + test updates | ~2–3 days |
| 4 — CI + reference runner | ~2–3 days |
