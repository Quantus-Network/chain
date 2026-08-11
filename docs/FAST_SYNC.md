# Fast sync (checkpoint warp sync)

A new node normally replays every block from genesis (~1.5–2 h on a year-old
chain, growing with chain age). Warp sync gets a mining-ready node in minutes,
independent of chain age:

1. Resolve a **checkpoint** — a single trusted header, normally the network's
   current finalized head (`best − max_reorg_depth`).
2. Download the checkpoint block from peers.
3. Download the full state at the checkpoint from peers, with every chunk
   Merkle-proof-verified against the checkpoint header's `state_root`.
4. Sync checkpoint → tip with normal full verification (execution + seal
   checks). Mining starts here.

History below the checkpoint is **not** downloaded: a warp-synced node serves
blocks and state from the checkpoint forward only. (Background backfill is
deliberately disabled for now — ascending gap fill would write unverifiable
history into the canonical index, see Follow-ups. Nodes that need full history
must full-sync.)

## Usage

```sh
quantus-node --chain heisenberg --sync warp
```

The checkpoint is resolved automatically (see below). To pin it manually:

```sh
# Explicit checkpoint endpoints (all reachable endpoints must agree):
quantus-node --chain heisenberg --sync warp \
    --checkpoint-url https://a1-heisenberg.quantus.cat \
    --checkpoint-url https://a2-heisenberg.quantus.cat

# Fully operator-pinned target (SCALE-encoded header, 0x hex):
quantus-node --chain heisenberg --sync warp --checkpoint-header 0x...
```

`--sync fast` / `--sync fast-unsafe` are rejected: QPoW seal verification needs
the parent block's state, which fast sync skips creating. Warp sync is the
supported fast path.

Warp-synced nodes keep a rolling state window (256 blocks) instead of archive
state: archive pruning refuses warp sync by design, since pre-checkpoint state
is never downloaded. Nodes that need full historical state must full-sync.

## Checkpoint resolution

First match wins:

1. `--checkpoint-header` — operator-pinned SCALE header hex.
2. Fetched from checkpoint endpoints (`--checkpoint-url`, or the chain spec
   `checkpointUrls` property). Each endpoint reports its finalized head; the
   returned header must hash to the reported finalized hash; the candidate is
   the lowest finalized height among responders and every other responder must
   confirm its hash at that height. Disagreement aborts the fetch.
3. The **release anchor**: the `checkpointHeader` chain spec property. A stale
   anchor still syncs — catch-up just re-executes the blocks mined since the
   release (~25–30 min per month of staleness).

If nothing resolves, the node exits with an error rather than silently full
syncing.

## Trust model

- The network already runs on rolling finality: every node finalizes
  `best − max_reorg_depth` (100 blocks ≈ 20 min) and refuses deeper reorgs, so
  the canonical chain past that depth is already subjective. A checkpoint no
  older than that is exactly as trusted as the binary, genesis, and bootnodes
  it ships alongside.
- Everything except the checkpoint header itself is verified: state by Merkle
  proofs against `state_root`, checkpoint → tip by full execution and seal
  verification.
- Fetched checkpoints are fenced by the release anchor at resolution time: a
  fetched target may never be older than the anchor, and at the anchor's own
  height it must be the anchor. Independently, any imported block at the
  anchor's height that is not the anchor is rejected (this also shields full
  sync from deep fabricated forks).
- Residual trust in the fetch path: if **every** queried checkpoint endpoint
  colludes (and no `--checkpoint-header` is pinned), they can hand out a
  fabricated chain above the anchor height, and nothing in v1 cross-checks its
  ancestry against the real chain. Endpoints are operator-run over TLS and all
  must agree, so this is the same class of trust as the bootnode/release
  channel — but unlike the release anchor it is exercised on every sync. Use
  independent hosts for `checkpointUrls`, or pin `--checkpoint-header` for
  zero fetch trust. The header-chain verification follow-up below closes this
  gap entirely.

## Release process

CI should refresh the anchor at release cut (follow-up: automate in the
release workflow):

```sh
FINALIZED=$(curl -s -H 'Content-Type: application/json' \
    -d '{"id":1,"jsonrpc":"2.0","method":"chain_getFinalizedHead","params":[]}' \
    https://a1-heisenberg.quantus.cat | jq -r .result)
HEADER_HEX=$(quantus-node key … )   # SCALE-encode the header at $FINALIZED
jq --arg h "$HEADER_HEX" '.properties.checkpointHeader = $h' \
    node/src/chain-specs/heisenberg.json > tmp && mv tmp node/src/chain-specs/heisenberg.json
```

(The SCALE encoding of a header is `chain_getHeader` fields in SCALE form;
a small CI helper or `state_getReadProof`-free script can produce it. Until
automated, `checkpointUrls` alone keeps warp sync fresh.)

## Consensus changes behind this (client/consensus/qpow)

- Imports are classified by state availability: the proof-verified state
  target skips runtime verification (its parent's state cannot exist), stray
  skip-execution imports at or below the finalized head skip seal
  re-verification and can never become best, and **everything else keeps
  today's full verification** — near-tip blocks always have their seals
  runtime-verified.
- The state target is imported without recording a block gap (no backfill)
  and is finalized on import, which arms the depth-finalization guard.
- Cumulative work is seeded at the state target (like genesis): post-target
  fork choice only ever compares descendants of the target.

## Follow-ups

- **Verified history backfill.** Upstream gap sync fills ascending from
  genesis and trusts the verifier to validate headers statelessly — possible
  for BABE (epoch data chains forward from genesis), impossible for QPoW
  (target difficulty lives in state). Backfill therefore needs either
  descending fill (each block checked against the parent hash its
  already-imported child commits to) or a pre-verified header chain walked
  down from the checkpoint. Until then `create_gap` stays off.
- **Header-chain verification of fetched targets**: walk anchor → target
  headers (hash linkage + stateless achieved-work via qpow-math, with capped
  per-block contribution to blunt the achieved-work heavy tail). Removes the
  residual collusion trust in checkpoint endpoints and doubles as the
  pre-verified chain for backfill.
- CI automation for the release anchor (`checkpointHeader` injection).
- In-protocol checkpoint fetch (peer quorum over the p2p network) instead of
  RPC endpoints, removing the URL configuration entirely.
