# Staging-Mainnet Launch

Mainnet dress rehearsal. Genesis comes from `mainnet_config_genesis`
(`runtime/src/genesis_config_presets.rs`). It seeds the launch tech collective
but deliberately leaves the treasury unconfigured; the collective sets the
treasury account after launch through a Root referendum.

- Genesis hash: regenerate and record it after this genesis change; the previous
  staging hash is obsolete.
- Treasury: unconfigured at genesis. `TreasuryPallet::set_treasury_account` is
  called by an approved Root referendum after the real treasury is ready.
- Tech collective: the ten accounts in
  `MAINNET_TECH_COLLECTIVE_MEMBERS_SS58` (referenda curves are runtime constants).
- Balances: no fixed figure is written down. Each member's liquid endowment is
  computed at genesis by `governance_member_seed()` from the live runtime
  constants — ED, the referenda submission and decision bonds, a maximum-size
  preimage deposit, and scaled fee headroom. It is deliberately the minimum
  that lets any member carry one referendum end to end. Because it is derived,
  changing `FEE_SCALE` or any deposit re-derives the endowment instead of
  stranding governance, and a genesis test pins every member balance to the
  formula. The 20 HD rehearsal accounts
  (`staging-0`..`staging-19`) each get `STAGING_REHEARSAL_LIQUID` so they can
  submit transactions. Everything else sits in the vesting pot. Read the
  exact per-member and total figures for a given build out of the generated
  spec's `balances` section rather than restating them here.
- Vesting: `mainnet_vesting_schedules` is a DUMMY scaffold (team / early-backer /
  ecosystem entries with stand-in beneficiaries T1, T2, and T3, plus
  20 HD rehearsal accounts with distinct grants summing to 100_000 UNIT). Every
  schedule starts on 2026-09-03 UTC and every cliff is at most 24 hours. Team and
  ecosystem grants retain their 4×365-day duration, the early-backer grant retains
  its 2×365-day duration, and rehearsal grants retain their 5-minute cliff /
  10-day duration from 14:00 UTC. Replace this with the real allocation table and
  flip `MAINNET_VESTING_FINALIZED` before the mainnet preset is added — until then
  only staging-mainnet builds.

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

Create the real treasury multisig with the chosen signers, threshold, and nonce.
Then submit and approve a normal Root referendum calling
`treasuryPallet.setTreasuryAccount(multisigAddress)`. Setting the account does
not move or create funds.

Vesting administration is also Root-only. Replace each stand-in beneficiary
through `vesting.retargetSchedule`, using an atomic `utility.batchAll` where
appropriate. Complete each retarget before its cliff: `claim` is permissionless
and pays the beneficiary stored when the claim executes.
