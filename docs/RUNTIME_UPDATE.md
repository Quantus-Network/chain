# Testnet Runtime Update Playbook (Heisenberg & Planck)

End-to-end runbook for shipping a new runtime to the **Heisenberg** (internal) and
**Planck** testnets, and smoke-testing them with the chain exercise suite.

Runtime upgrades on this chain are **governance-only** — `pallet-sudo` was removed, so
there is no `sudo.setCode` shortcut. An upgrade is a Tech-Referenda proposal on the
`fast_upgrade` track that, once passed, executes `system.authorize_upgrade(code_hash)`;
the WASM itself is then delivered via the permissionless `quantus runtime apply`
(`system.apply_authorized_upgrade`). No node restart is required.

> This flow needs a runtime that has the `fast_upgrade` track and `FastUpgrade`
> origin. To upgrade an older live runtime that predates them, use the matching
> older CLI (which submits `system.set_code` on the Root track).

> Polkadot-JS Apps and the standard `@polkadot/api` **cannot** sign for this chain
> (Dilithium / ML-DSA signatures). Every step below uses the **Quantus CLI**
> (`quantus`), built from <https://github.com/Quantus-Network/quantus-cli>.

See also: [`RUNTIME_UPGRADE_VIA_GOVERNANCE.md`](./RUNTIME_UPGRADE_VIA_GOVERNANCE.md) (the
generic single-network procedure) and [`RUNTIME_SURFACE.md`](./RUNTIME_SURFACE.md) (why
sudo is gone and how the Tech track is configured).

## Version numbers

Do **not** increment `spec_version` or the crate version in `runtime/Cargo.toml`
in feature or security PRs. A consensus-critical change (new verifier rules,
changed weights, new dispatchables) still merges at the current
`spec_version`. Reviewers asking for a bump on that PR: the bump is not
missing.

- **`spec_version`** is incremented by the **Quantus - Release Proposal**
  workflow (`.github/workflows/quantus-release-proposal.yml`) when
  `is_runtime_upgrade` is set. That PR updates `VERSION` in
  `runtime/src/lib.rs` and `runtime/Cargo.toml`, and is the binary + wasm
  that get released together. A node uses its native runtime only when
  `spec_name`, `spec_version`, and `authoring_version` all match the
  on-chain Wasm, so a `main` binary with an unreleased verifier does not
  silently replace the live runtime.
- **`transaction_version`** is not touched by CI. Bump it only when the
  *signed extrinsic encoding* changes (the `TxExtension` set or payload
  layout). A change to how signatures are produced or verified does not
  require it.

---

## TL;DR

```bash
# 1. Endpoints + signing wallets (see tables below)
export HEISENBERG_WS="wss://a1-heisenberg.quantus.cat"
export PLANCK_WS="wss://a1-planck.quantus.cat"

# 2. Get the runtime wasm (from the GitHub release, or the srtool CI artifact)
export WASM=/path/to/quantus_runtime.compact.compressed.wasm

# 3. Heisenberg: submit -> deposit -> vote (all 3 members) -> wait -> apply -> verify
# 4. Run the exercise suite:  quantus exercise --skip governance --node-url $HEISENBERG_WS
# 5. Planck: repeat the governance flow with the planck wallets
```

Full step-by-step is below. Each upgrade is a multi-step governance flow whose
referendum index is only known **after** you submit — capture it from the `list` step.

---

## Endpoints

The chain-spec (`node/src/chain-specs/{heisenberg,planck}.json`) only defines **p2p boot
nodes**, not RPC URLs — but each network also exposes the Substrate RPC over **wss://**
(TLS, port 443 — no port number needed).

| Testnet | RPC endpoint (`--node-url`) | Backup |
|---|---|---|
| **Heisenberg** | `wss://a1-heisenberg.quantus.cat` | `wss://a2-heisenberg.quantus.cat` |
| **Planck** | `wss://a1-planck.quantus.cat` | `wss://a2-planck.quantus.cat` |

> These are secure `wss://` endpoints, so all commands — including `quantus system
> --runtime`, whose stricter "chainHead" client refuses a remote plain `ws://` with
> `InsecureUrl` — work directly against them.

---

## Tech Collective members

Both testnets have a **3-member** Tech Collective. The `fast_upgrade` track needs
**≥ 80 % approval and ≥ 80 % support**, so **all 3 members** must vote aye. Confirm the
live membership any time with:

```bash
quantus tech-collective list-members --node-url <endpoint>
```

| Testnet | Local wallet names (all present in your keystore) | Submit `--from` |
|---|---|---|
| **Heisenberg** | `crystal_alice`, `crystal_bob`, `crystal_charlie` | `crystal_alice` |
| **Planck** | `planck-tc-1` (= `s1`), `planck-tc-2` (= `s2`), `s3` | `planck-tc-1` |

If any wallet is password-protected, add `-p/--password` or `--password-file <file>` to
every signing command (`runtime update`, `place-decision-deposit`, `vote`).

---

## Prerequisites

- `quantus` CLI on your `PATH` (`quantus --version`).
- The Tech Collective wallets above in your keystore (`quantus wallet list`).
- The new runtime **wasm** (see next section).
- Each submit/vote wallet needs a little balance to pay fees + the decision deposit
  (the deposit is refundable after the referendum ends).

---

## Step 0 — Get the runtime wasm

The canonical artifact is the compressed runtime from the release:
`quantus-runtime-v<specVersion>.compact.compressed.wasm`, built by srtool in CI.

**Normal path — download from the published GitHub release:**

```bash
cd /path/to/chain
gh release download <release-tag> --pattern '*.compact.compressed.wasm' --dir ./release-wasm
export WASM="$(find ./release-wasm -name '*.compact.compressed.wasm' | head -1)"
```

**If the release didn't publish (fallback — pull the srtool CI artifact):**

The `Release Tag & Publish` workflow builds the runtime (srtool) *before* it builds the
node binaries and publishes the GitHub release. If a later matrix job fails (e.g. the
Windows node build), the release — and its wasm asset — is **skipped**, but the srtool
wasm still exists as a workflow **artifact** named `runtime`.

```bash
# Find the run (or use the run id from `gh run list --workflow=quantus-release.yml`)
gh run download <run-id> -n runtime -D ./ci-wasm
export WASM="$(find ./ci-wasm -name '*.compact.compressed.wasm' | head -1)"
echo "Using wasm: $WASM"
```

> Workflow artifacts expire (~90 days); release assets are permanent. If you hit the
> fallback, also fix + re-publish the release (`gh run rerun <run-id> --failed`).

**Or build it locally from the tagged commit:**

```bash
git checkout <release-tag>
cargo build --release -p quantus-runtime   # or: cargo build --release
export WASM="$(pwd)/target/release/wbuild/quantus-runtime/quantus_runtime.compact.compressed.wasm"
```

---

## Step 1 — Update the runtime on Heisenberg

```bash
export HEISENBERG_WS="wss://a1-heisenberg.quantus.cat"
export HEISENBERG_TC="crystal_alice"

# a) Diff the new wasm against the live runtime (confirms it's actually an upgrade and
#    prints both spec_versions)
quantus runtime compare --wasm-file "$WASM" --node-url "$HEISENBERG_WS"

# b) Confirm the collective membership (who you'll need aye votes from)
quantus tech-collective list-members --node-url "$HEISENBERG_WS"

# c) Submit the proposal: notes the authorize_upgrade(code_hash) preimage, then opens
#    the referendum on the fast_upgrade track. Add --force to skip the interactive confirm.
quantus runtime update \
  --wasm-file "$WASM" \
  --from "$HEISENBERG_TC" \
  --node-url "$HEISENBERG_WS"

# d) Find the new referendum index
quantus tech-referenda list --node-url "$HEISENBERG_WS"
export HREF=<referendum_index_from_list>

# e) Place the decision deposit (moves it Preparing -> Deciding; refundable later)
quantus tech-referenda place-decision-deposit \
  --index "$HREF" \
  --from "$HEISENBERG_TC" \
  --node-url "$HEISENBERG_WS"

# f) Vote aye with all three members (fast_upgrade needs 80%/80% — all 3 of 3)
quantus tech-collective vote --referendum-index "$HREF" --vote aye --from crystal_alice   --node-url "$HEISENBERG_WS"
quantus tech-collective vote --referendum-index "$HREF" --vote aye --from crystal_bob     --node-url "$HEISENBERG_WS"
quantus tech-collective vote --referendum-index "$HREF" --vote aye --from crystal_charlie --node-url "$HEISENBERG_WS"

# g) Monitor until it passes -> confirms -> enacts (authorize_upgrade executes)
quantus tech-referenda status --index "$HREF" --node-url "$HEISENBERG_WS"

# h) Apply the authorized wasm (permissionless — any funded wallet)
quantus runtime apply --wasm-file "$WASM" --from "$HEISENBERG_TC" --node-url "$HEISENBERG_WS"

# i) Verify the new runtime is live (compare should now show equal versions)
quantus runtime compare --wasm-file "$WASM" --node-url "$HEISENBERG_WS"
```

> ⏱️ **Timing.** The `fast_upgrade` track has 10-minute prepare, confirm, and enactment
> windows (~30 minutes end to end plus voting time). If a testnet runs with
> `fast-governance`, it enacts in minutes. Check the live periods with
> `quantus tech-referenda config --node-url "$HEISENBERG_WS"`.

---

## Step 2 — Run the chain tests (exercise suite)

`quantus exercise` runs the chain exercise suite against a live node (balances,
reversible, multisig, preimage, negative, fuzz, wormhole, …). It uses ephemeral
accounts plus the `crystal_alice/bob/charlie` dev accounts.

```bash
# Full suite. Skip 'governance' on a non-fast-governance node (that phase needs one).
# The 'upgrade' phase is already off by default.
quantus exercise --skip governance --node-url "$HEISENBERG_WS"

# Minimal subset:
quantus exercise --phases reads,balances --node-url "$HEISENBERG_WS"

# Machine-readable report / fail on first error:
quantus exercise --skip governance --json --fail-fast --node-url "$HEISENBERG_WS"

# After Planck is upgraded, run it there too:
quantus exercise --skip governance --node-url "$PLANCK_WS"
```

Available phases: `reads, balances, reversible, multisig, preimage, governance,
negative, fuzz, wormhole, upgrade` (default = all except `upgrade`).

**In-repo shell smoke test** (single Alice→Bob transfer + tx-pool watch), as an
alternative minimal check:

```bash
# terminal 1 — watch the pool
node scripts/testing/test_txwatch.mjs
# terminal 2 — submit a transfer (NODE_URL overrides the ws://127.0.0.1:9944 default)
NODE_URL="$HEISENBERG_WS" ./scripts/testing/submit_transfer.sh 5
```

---

## Step 3 — Update the runtime on Planck

Identical flow, pointed at Planck, using the Planck wallets.

```bash
export PLANCK_WS="wss://a1-planck.quantus.cat"

# a) diff vs live
quantus runtime compare --wasm-file "$WASM" --node-url "$PLANCK_WS"

# b) confirm members
quantus tech-collective list-members --node-url "$PLANCK_WS"

# c) submit proposal
quantus runtime update --wasm-file "$WASM" --from planck-tc-1 --node-url "$PLANCK_WS"

# d) find the referendum index
quantus tech-referenda list --node-url "$PLANCK_WS"
export PREF=<referendum_index_from_list>

# e) place decision deposit
quantus tech-referenda place-decision-deposit --index "$PREF" --from planck-tc-1 --node-url "$PLANCK_WS"

# f) vote aye with all three members (all 3 of 3 required)
quantus tech-collective vote --referendum-index "$PREF" --vote aye --from planck-tc-1 --node-url "$PLANCK_WS"
quantus tech-collective vote --referendum-index "$PREF" --vote aye --from planck-tc-2 --node-url "$PLANCK_WS"
quantus tech-collective vote --referendum-index "$PREF" --vote aye --from s3          --node-url "$PLANCK_WS"

# g) monitor to enactment
quantus tech-referenda status --index "$PREF" --node-url "$PLANCK_WS"

# h) apply the authorized wasm
quantus runtime apply --wasm-file "$WASM" --from planck-tc-1 --node-url "$PLANCK_WS"

# i) verify new version live
quantus runtime compare --wasm-file "$WASM" --node-url "$PLANCK_WS"
```

---

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| Referendum never leaves "Preparing" | You skipped the **decision deposit** (Step e). Place it, then voting/deciding starts. |
| Referendum rejected / not enough support | `fast_upgrade` needs ≥ 80 % approval / 80 % support → all 3 members must vote aye. |
| Referendum enacted but version unchanged | Enactment only authorizes the hash — run `quantus runtime apply` with the same wasm. |
| No wasm asset on the GitHub release | The release publish job failed after srtool. Pull the `runtime` workflow artifact (Step 0 fallback) and re-run the release. |
| Wallet prompts for a password | Add `-p <password>` or `--password-file <file>` to the signing command. |

## Command reference

| Action | Command |
|---|---|
| On-chain runtime version | `quantus runtime compare --wasm-file <wasm> --node-url <ws>` |
| List collective members | `quantus tech-collective list-members --node-url <ws>` |
| Submit upgrade (authorize referendum) | `quantus runtime update --wasm-file <wasm> --from <member> --node-url <ws>` |
| Apply authorized wasm (after enactment) | `quantus runtime apply --wasm-file <wasm> --from <wallet> --node-url <ws>` |
| List referenda | `quantus tech-referenda list --node-url <ws>` |
| Referendum detail | `quantus tech-referenda get --index <i> --node-url <ws>` |
| Place decision deposit | `quantus tech-referenda place-decision-deposit --index <i> --from <wallet> --node-url <ws>` |
| Vote | `quantus tech-collective vote --referendum-index <i> --vote aye --from <member> --node-url <ws>` |
| Voting status / tally | `quantus tech-referenda status --index <i> --node-url <ws>` |
| Track config (periods, deposits) | `quantus tech-referenda config --node-url <ws>` |
| Run the test suite | `quantus exercise --skip governance --node-url <ws>` |
| Refund deposits (after it ends) | `quantus tech-referenda refund-decision-deposit --index <i> --from <wallet> --node-url <ws>` |
