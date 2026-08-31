# Runtime Upgrade via Governance (Quantus CLI only)

Polkadot JS Apps and the standard `@polkadot/api` **do not support** this chain's signature schemes. All governance actions must be done with the Quantus CLI.

## How upgrades work

Tech-referenda proposals are `Lookup` preimages capped at 64 KiB
(`MaxReferendaProposalSize`), so a runtime WASM (hundreds of KiB) can never
itself be a proposal. Upgrades instead go through the hash-then-apply flow,
modeled on Polkadot's Whitelisted Caller track:

1. `quantus runtime update` notes the 34-byte preimage of
   `system.authorize_upgrade(code_hash)` and submits it as a referendum on
   track 1 (`fast_upgrade`) with the custom `Origins::FastUpgrade` proposal
   origin. That origin is honored **only** by `system.authorize_upgrade` — the
   track can publish an upgrade hash and nothing else, it cannot dispatch
   arbitrary Root calls.
2. The track uses constant 80%/80% approval/support curves — **8-of-10** ayes
   from the genesis collective (2 nays cannot block) — with 10-minute
   prepare, confirm, and enactment windows (~30 minutes end to end plus
   voting time).
3. After enactment, `quantus runtime apply` submits the permissionless,
   version-checked `system.apply_authorized_upgrade(wasm)` with the actual
   runtime bytes. It refuses to run if no authorization exists, the WASM hash
   does not match the authorized hash, version checking is disabled, or the
   WASM is already installed — and verifies after inclusion that the new code
   is live.

Track 0 (Root origin, 61%/60% curves, 1-day windows) remains for other
governance calls that fit in 64 KiB; it is not used for runtime upgrades.

## Prerequisites

- A working `quantus` CLI binary (build from https://github.com/Quantus-Network/quantus-cli if needed).
- At least one Tech Collective member wallet (created and managed inside the CLI) to submit, and 8 member wallets to vote.
- The new runtime WASM file (normally the compressed one from the release: `quantus-runtime-vNNN.compact.compressed.wasm`).
- Node endpoint (e.g. `wss://rpc.quantus.network`).

## Steps

1. Check current runtime version:
   ```bash
   quantus system --runtime --node-url <endpoint>
   ```

2. (Recommended) Sanity-check the WASM file:
   ```bash
   quantus runtime compare --wasm-file /path/to/new-runtime.wasm --node-url <endpoint>
   ```

3. Submit the authorization referendum (must be run by a Tech Collective member). This notes the `authorize_upgrade(code_hash)` preimage and submits the referendum on the `fast_upgrade` track. It asks for an interactive `yes/no` confirmation — add `--force` to skip it (e.g. for scripts):
   ```bash
   quantus runtime update \
     --wasm-file /path/to/new-runtime.wasm \
     --from <tech-collective-wallet-name> \
     --node-url <endpoint>
   ```

4. Find the new referendum index (the submit command does not print it):
   ```bash
   quantus tech-referenda list --node-url <endpoint>
   ```

5. Place the decision deposit (anyone with enough balance can do this; required before voting can decide):
   ```bash
   quantus tech-referenda place-decision-deposit \
     --index <referendum_index> \
     --from <any-funded-wallet> \
     --node-url <endpoint>
   ```

6. Tech Collective members vote — 8 of the 10 genesis members must vote aye:
   ```bash
   quantus tech-collective vote \
     --referendum-index <referendum_index> \
     --vote aye \
     --from <member-wallet-name> \
     --node-url <endpoint>
   ```

7. Monitor until it passes, confirms, and enacts (`system.authorize_upgrade` executes):
   ```bash
   quantus tech-referenda status --index <referendum_index> --node-url <endpoint>
   ```

8. Apply the authorized WASM (any funded wallet — the call is permissionless):
   ```bash
   quantus runtime apply \
     --wasm-file /path/to/new-runtime.wasm \
     --from <any-funded-wallet> \
     --node-url <endpoint>
   ```

Once `apply` succeeds the new runtime is live immediately. No node restart is required.

## Gotchas

- Only Tech Collective members can submit or vote on tech referenda.
- The decision deposit must be placed before the referendum can move to the deciding phase.
- The `apply` step must pass the **exact** bytes whose hash was authorized — use the same release artifact, not a rebuild.
- The apply block is very heavy (consumes the entire block weight).
- Test on a local dev chain first. There is no automatic binary distribution — operators pull new releases on their own schedule.

All commands support `--help` and `--verbose`. Use `--finalized-tx` on important governance transactions if you want to wait for deeper finality on this PoW chain.
