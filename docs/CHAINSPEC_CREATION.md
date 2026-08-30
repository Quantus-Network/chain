# Creating a Chain Spec (JSON)

How to produce a committed, raw JSON chain spec for a network profile
(`heisenberg`, `planck`, `staging_mainnet`). The genesis runtime in the JSON is
the **published release artifact**, not a local build — anyone can reproduce
and verify it.

## Naming convention

For a profile `<profile>` (e.g. `staging_mainnet`):

| Name | Meaning |
| --- | --- |
| `<profile>_live_spec` | `--chain` id that builds genesis from the preset compiled into the node (used only during generation) |
| `node/src/chain-specs/<profile-with-dashes>.json` | the committed raw spec |
| `<profile>` | `--chain` id that loads the committed JSON (what operators use) |

## Prerequisites

- The runtime preset exists in `runtime/src/genesis_config_presets.rs` and is
  wired up in `node/src/chain_spec.rs` (`<profile>_chain_spec()`) and in
  `load_spec` (`node/src/command.rs`) under `"<profile>_live_spec"`.
- Clean git working tree (the script refuses otherwise).
- `jq`, `curl`, `xxd`, and a Rust toolchain that builds the node.

## Steps

1. **Cut a release tag.** CI publishes the runtime WASM
   (`quantus-runtime-vNNN.compact.compressed.wasm`) as a release asset on
   `Quantus-Network/chain`.

2. **Generate the spec from the tag:**

   ```sh
   ./scripts/genesis_generate_spec.sh <release-tag> <profile>
   ```

   The script:
   - creates branch `genesis/<profile>/<release-tag>` at the tag and builds
     the node there, so genesis state comes from exactly the released code;
   - runs `build-spec --chain <profile>_live_spec --raw
     --disable-default-bootnode` into
     `node/src/chain-specs/<profile-with-dashes>.json` (without
     `--disable-default-bootnode`, a spec with no bootnodes gets a throwaway
     `/ip4/127.0.0.1` bootnode injected);
   - downloads the release's compressed runtime WASM and splices it into the
     JSON's `:code` key (`.genesis.raw.top."0x3a636f6465"`), so the genesis
     runtime is byte-identical to the published artifact.

3. **Wire the JSON into the node.** Add a `"<profile>"` arm to `load_spec` in
   `node/src/command.rs`:

   ```rust
   "staging_mainnet" => Box::new(chain_spec::ChainSpec::from_json_bytes(include_bytes!(
       "chain-specs/staging-mainnet.json"
   ))?) as Box<dyn sc_service::ChainSpec>,
   ```

4. **Record the genesis hash.** Run `quantus-node --chain <profile> --tmp` and
   note the genesis hash it prints on startup.

5. **Verify the genesis runtime against the release** (against any running
   node of the new chain):

   ```sh
   ./scripts/genesis_verification.sh --release-tag <release-tag> --node-url http://localhost:9944
   ```

6. **Add bootnodes** once the infrastructure exists: edit the JSON's top-level
   `bootNodes` array (`/dns/<host>/tcp/30333/p2p/<peer-id>`). This field lives
   outside genesis, so the genesis hash is unchanged.

7. **Commit** the JSON plus the `load_spec` arm, and cut the node release that
   embeds them.

For the staging-mainnet launch sequence around this (treasury multisig,
first server, post-launch), see `docs/STAGING_MAINNET_LAUNCH.md`.
