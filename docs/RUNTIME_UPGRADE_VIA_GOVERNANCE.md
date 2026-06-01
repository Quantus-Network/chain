# Runtime Upgrade via Governance (Quantus CLI only)

Polkadot JS Apps and the standard `@polkadot/api` **do not support** this chain's signature schemes. All governance actions must be done with the Quantus CLI.

## Prerequisites

- A working `quantus` CLI binary (build from https://github.com/Quantus-Network/quantus-cli if needed).
- At least one Tech Collective member wallet (created and managed inside the CLI). On Planck these are the treasury signers.
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

3. Submit the upgrade proposal (must be run by a Tech Collective member). This notes the preimage and submits the referendum on the Tech track with Root origin. It asks for an interactive `yes/no` confirmation — add `--force` to skip it (e.g. for scripts):
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

6. Tech Collective members vote:
   ```bash
   quantus tech-collective vote \
     --referendum-index <referendum_index> \
     --vote aye \
     --from <member-wallet-name> \
     --node-url <endpoint>
   ```

7. Monitor until it passes and enacts:
   ```bash
   quantus tech-referenda status --index <referendum_index> --node-url <endpoint>
   ```

When the referendum passes, confirms, and enacts, `system.set_code` executes with Root origin and the new runtime is live immediately. No node restart is required.

## Gotchas

- Only Tech Collective members can submit or vote on the Tech track.
- The decision deposit must be placed before the referendum can move to the deciding phase.
- The upgrade block is very heavy (consumes the entire block weight).
- Test on a local dev chain first. There is no automatic binary distribution — operators pull new releases on their own schedule.

All commands support `--help` and `--verbose`. Use `--finalized-tx` on important governance transactions if you want to wait for deeper finality on this PoW chain.