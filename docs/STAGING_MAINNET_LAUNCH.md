# Staging-Mainnet Launch

Mainnet dress rehearsal. Genesis comes from `mainnet_config_genesis`
(`runtime/src/genesis_config_presets.rs`) — identical to the eventual mainnet's
except the treasury multisig nonce (staging 1, mainnet 0), so the genesis hash
differs while everything else stays 1:1.

- Treasury: 6-of-10 multisig of `MAINNET_TREASURY_SIGNERS_SS58`, nonce 1 →
  `qzpjP5r4NSeWDrbHvboYcychsmCCaJrbRixvghVjnaRzjRb5i`
- Tech collective: the same ten accounts (referenda curves are runtime constants)
- Balances: no fixed figure is written down. Each signer's liquid endowment is
  computed at genesis by `governance_treasury_signer_seed()` from the live
  runtime constants — treasury multisig bootstrap (multisig fee, proposal fee,
  refundable proposal deposit, inclusion-fee prepay, ED), the referenda
  submission and decision bonds, a maximum-size preimage deposit, and a small
  scaled fee headroom. It is deliberately the minimum that lets any single
  treasurer act from genesis: bootstrap the multisig and carry one referendum
  end to end. Because it is derived, changing `FEE_SCALE` or any deposit
  re-derives the endowment instead of stranding governance, and a genesis test
  pins every signer balance to the formula. The 20 HD rehearsal accounts
  (`staging-0`..`staging-19`) each get `STAGING_REHEARSAL_LIQUID` so they can
  submit transactions. Everything else sits in the vesting pot. Read the
  exact per-signer and total figures for a given build out of the generated
  spec's `balances` section rather than restating them here.
- Vesting: `mainnet_vesting_schedules` is a DUMMY scaffold (team / early-backer /
  ecosystem entries with stand-in beneficiaries T1, T2, and the treasury, plus
  20 HD rehearsal accounts with distinct grants summing to 100_000 UNIT,
  5-minute cliff / 10-day vest from 2026-09-01 14:00 UTC). Replace it with the
  real allocation table and flip `MAINNET_VESTING_FINALIZED` before the mainnet
  preset is added — until then only staging-mainnet builds.

## Generate the chain spec

Follow `docs/CHAINSPEC_CREATION.md` with profile `staging_mainnet`: cut a
release tag, run `./scripts/genesis_generate_spec.sh <tag> staging_mainnet`
(writes `node/src/chain-specs/staging-mainnet.json` with the released wasm),
add the `"staging_mainnet"` arm to `load_spec`, record the genesis hash,
commit, cut the node release.

## First server

```sh
quantus-node key generate-node-key --file ~/.quantus/node_key.p2p
quantus-node key quantus --scheme wormhole   # note the printed "Inner Hash"
quantus-node --chain staging_mainnet --node-key-file ~/.quantus/node_key.p2p \
  --rewards-inner-hash <0x-inner-hash> --validator --force-authoring \
  --port 30333 --rpc-port 9944
```

`--force-authoring` is required on this first miner: it bypasses the two
authoring gates (no connected peers, and a best block older than
`--max-tip-age` — which the genesis block always is), so without it the chain
never starts. Drop the flag once other miners have joined: with it a restarted
node mines on a stale tip while catching up instead of pausing.

Blocks only start once a miner (`--validator`) runs. Point DNS at the server,
then add `/dns/<host>/tcp/30333/p2p/<peer-id>` to the JSON's `bootNodes` —
that field is outside genesis, so the hash is unchanged.

## Post-launch

One signer calls `multisig.createMultisig(signers, 6, 1)`; the derived address
must equal the genesis treasury account above. Treasury then operates via
`proposeTransaction` / `approveProposal` at 6-of-10.
