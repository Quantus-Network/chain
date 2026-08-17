# Close high-security value leaks outside `schedule_transfer`

High-security accounts are meant to move value only through delayed, guardian-watched `schedule_transfer`. The call whitelist never sees the tip, the inclusion fee, or several ways to inflate that fee. A compromised signing key could therefore drain free balance immediately, or grind it out as fees to the block author.

This PR closes the unbounded paths and rate-limits the rest.

## Problem

The whitelist gates `RuntimeCall`. Anything that is not the call is unguarded:

1. **Tip** — lives on `ChargeTransactionPayment`, not on the call. Unbounded. Withdrawn immediately (`Preservation::Preserve`) and reminted 100% to the QPoW author. A no-op or failed whitelisted call was enough.
2. **Inclusion fee** — length (1 UNIT/MB) and weight (1 UNIT/s) are partly attacker-controlled. 100% to the miner. A fat or heavy *whitelisted* call still pays.
3. **Padded `MultiAddress::Raw` dest** on `schedule_transfer` (and inside `batch_all`) — same length-fee path, no tip required. Already landed on this branch as #8; kept and applied to batch children.

## Changes

### Zero tip for high-security signers

`transaction_extensions::HighSecurityFungibleAdapter` (the configured `OnChargeTransaction`, wrapping `FungibleAdapter`) rejects a non-zero tip from an HS signer (`InvalidTransaction::Custom(2)`) in both `can_withdraw_fee` and `withdraw_fee`. The extension tuple keeps the stock `ChargeTransactionPayment`, whose every fee path goes through the adapter — so the policy is consensus-enforced (a self-mining attacker cannot skip it) and survives any refactor of the extension tuple.

Normal accounts can still tip.

### 16 signed extrinsics per rolling day

`HighSecurityTxQuota` is a ring of 16 block numbers (oldest at index 0). Update is O(1): if the ring is not full, push; if it is full, replace the head only when `now - oldest >= DAYS` (7200 blocks), otherwise reject (`Custom(3)`).

- Counts **included extrinsics**, not inner calls. A flat batch of 16 transfers is 1 quota slot.
- Independent of `MaxPendingPerAccount` (in-flight delayed holds).
- `validate` is read-only; `prepare` re-checks and records so same-block spam cannot sneak through the mempool.
- Failed included calls count. Guardian `cancel` / `recover_funds` are signed by the guardian, so a non-HS guardian is not under this cap.
- **Documented limitation:** a *single-key high-security* guardian shares this quota with its own traffic; with 16 slots used it cannot intervene until one ages out. Accepted rather than special-cased: a quota exemption for "live guardian interventions" is farmable (enrollment needs no guardian consent, so a stolen key can manufacture wards and unbounded exempt calls), and the recommended multisig guardian is immune by construction — the derived address never signs extrinsics, so it is never quota-gated even when itself enrolled as high-security (pinned by `high_security_multisig_guardian_is_immune_to_quota_lockout`). Multisig signers must not be HS accounts (`Multisig::propose` is not whitelisted).

### Tighter whitelist

- **`Vesting::claim` removed.** Permissionless: anyone may claim any schedule; payout always goes to the stored beneficiary. An HS signer does not need it.
- **`batch_all` is a flat wrapper only:** non-empty, no nesting, at most `MaxPendingPerAccount` (16) **leaf** children (`schedule_transfer` / `cancel` / `recover_funds`). Nested empty batches were the remaining length/weight amplifier.
- **`schedule_transfer` dest must be `MultiAddress::Id`.** Same check on batch children, so `Raw` padding cannot inflate the length fee. Other `MultiAddress` variants are rejected at the gate, before fees are withdrawn.

`schedule_transfer_with_delay` stays off the list: it is the one-time path for *normal* accounts (caller-chosen delay). The pallet already rejects it for HS (`AccountAlreadyReversibleCannotScheduleOneTime`).

`cancel` and `recover_funds` stay: they are immediate guardian actions (the delay is a window for the guardian, not a cooldown on those calls). An HS guardian still needs them, and a flat batch of cancels so a full pending set does not burn the whole daily quota.

The batch arity cap is its own constant (`MaxHighSecurityBatchLen`), deliberately decoupled from `MaxPendingPerAccount` so a future bump of pending-transfer capacity does not silently widen the HS fee surface.

### Blanket bounds on every HS extrinsic (defense in depth)

Beyond the per-call shape rules, `ReversibleTransactionExtension::validate` enforces two bounds that no future whitelist addition can escape:

- **Length cap** — at most `MAX_HIGH_SECURITY_EXTRINSIC_LEN` (10 KiB) encoded bytes (`Custom(4)`). The worst legitimate extrinsic (16-leaf batch + Dilithium sig) is ~8.1 KiB.
- **Fee ceiling** — the zero-tip inclusion fee (`compute_fee(len, info, 0)`) must not exceed `MAX_HIGH_SECURITY_INCLUSION_FEE` (1 UNIT, `Custom(5)`). This bounds the *fee itself*, closing the weight half of the padding class the length cap cannot see. The costliest legitimate call (`recover_funds`, ~0.098 UNIT) clears it with ~10x headroom (asserted by a test). Deterministic: the fee multiplier is a constant one.

Combined with the daily quota, a stolen key can grind out at most **16 UNIT per rolling day**, hard-capped regardless of future whitelist changes.

### Guardian index removed (fill-the-slots griefing)

`GuardianIndex` was a bounded (32-slot) on-chain map from guardian to protected accounts, consumed by `set_high_security` without the guardian's consent — a stranger could fill a well-known guardian's slots with throwaway enrollments so legitimate users could no longer choose it. Nothing on-chain ever read the index; it existed only for UI discovery.

The index is gone (along with `MaxGuardianAccounts` and `TooManyGuardianAccounts`), which removes the griefing surface outright: there is nothing left to fill. Guardianship stays authoritative in `HighSecurityAccounts`, and offchain indexers (Subsquid) reconstruct "which accounts do I guard?" from `HighSecuritySet` events. Enrollment needs no guardian consent because being named guardian grants only passive powers (cancel, recover-to-guardian) and carries no liability.

### Multisig guardians (documented + pinned)

The guardian holds instant, total seizure power: `recover_funds` sweeps every hold plus the whole free balance to it, with no delay, no second approver, and an immutable relationship. A single-key guardian is a single point of failure, so the docs now recommend a **multisig guardian**, and a runtime integration test pins the lifecycle with a 2-of-2 `pallet_multisig` guardian: cancelling a pending transfer during the delay window and recovering funds — each dispatched as the multisig via propose/approve/execute.

## Residual

An included HS extrinsic is now ~6–7 KB (Dilithium sig + pubkey + a flat batch of 16 `Id` transfers) → about **0.007 UNIT** of length fee. Sixteen of those in a rolling day is well under 0.2 UNIT.

A failed `cancel` still *declares* ~0.9 s of wormhole-insert weight (refunded after dispatch). That is a weight-fee / block-weight leftover, not a length pad. A batch of 16 cancels may exceed the normal-class weight cap and fail inclusion.

The signer address is also `MultiAddress`, but `AccountIdLookup` only accepts `Id`. A padded signer fails lookup before payment.

## Test plan

- `runtime/tests/transactions/high_security_tip.rs` — tip rejected on signed extrinsics; padded dest rejected before fees; fee ceiling admits worst-case legitimate extrinsics with 2x headroom.
- `runtime/tests/transactions/high_security_quota.rs` — 16 included txs, 17th rejected, slot frees after `DAYS`; normal accounts uncapped.
- Pallet: rolling-window ring (full, one-block-short, expire-and-replace); zero-capacity window rejects instead of panicking.
- Extension unit tests: empty / nested / oversized `batch_all`; `Raw` / `Address32` dest; `Raw` dest as a batch child; vesting claim rejected; oversized and overweight HS extrinsics rejected while normal signers are unaffected; fee adapter rejects HS tips on both fee paths.
