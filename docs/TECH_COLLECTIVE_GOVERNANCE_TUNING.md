# Tech Collective Governance Tuning (Mainnet Parameters)

Scope: the two tech-referenda tracks (`TechReferenda` = `pallet_referenda::Pallet<Runtime, TechReferendaInstance>` + `TechCollective` = `pallet_ranked_collective`). Tracks are defined in `runtime/src/governance/definitions.rs` (`TechCollectiveTracksInfo::create_tech_collective_tracks`) and wired in `runtime/src/configs/mod.rs`. Pallet semantics verified against the crate sources `pallet-referenda-45.0.0` / `pallet-ranked-collective-45.0.0` (cargo registry; not vendored in `pallets/`).

| Track | Proposal origin | May dispatch | Used for |
|---|---|---|---|
| 0 `tech_collective_members` | `Root` | any call whose preimage fits `MaxReferendaProposalSize` (64 KiB) | membership and parameter changes, cancel/kill, slow fallback for `authorize_upgrade` |
| 1 `fast_upgrade` (`FAST_UPGRADE_TRACK_ID`) | `Origins::FastUpgrade` (`pallet_custom_origins`, runtime index 23) | `system.authorize_upgrade(code_hash)` only | **runtime upgrades** |

Runtime upgrades never go through `set_code`: a runtime WASM is hundreds of KiB, far over the 64 KiB proposal cap and over hardware-wallet signing limits. The supported route is to vote on the code *hash* on track 1, then have anyone submit the permissionless, version-checked `system.apply_authorized_upgrade(wasm)` with the exact bytes. `frame_system::Config::AuthorizeUpgradeOrigin` is `EitherOfDiverse<EnsureRoot, FastUpgrade>`, so a track-0 Root referendum can also authorize a hash (about two days slower); `set_code` and `authorize_upgrade_without_checks` stay Root-only. `TracksInfo::track_for` accepts only the `Root` and `FastUpgrade` proposal origins — `Signed(_)` is rejected (#91247/#91270), so neither track can dispatch as an arbitrary account. Operator flow: [`RUNTIME_UPGRADE_VIA_GOVERNANCE.md`](./RUNTIME_UPGRADE_VIA_GOVERNANCE.md).

## 1. Timing

All periods are in blocks. `runtime/src/lib.rs`: `TARGET_BLOCK_TIME_MS = 12_000`, so `MINUTES = 5`, `HOURS = 300`, `DAYS = 7200` blocks.

| Parameter | Track 0 | Track 1 `fast_upgrade` | Meaning |
|---|---|---|---|
| `prepare_period` | `2 * HOURS` (600) | `10 * MINUTES` (50) | Delay between submission and decision start |
| `decision_period` | `DAYS` (7200) | `DAYS` (7200) | Window in which the referendum must reach passing state |
| `confirm_period` | `DAYS` | `10 * MINUTES` | Must remain continuously passing this long to be approved |
| `min_enactment_period` | `DAYS` | `10 * MINUTES` | Min delay between approval and dispatch |
| `max_deciding` | 1 | 1 | Per track: one deciding referendum at a time on each lane |
| `decision_deposit` | `TECH_COLLECTIVE_DECISION_DEPOSIT` = `scale_fee(10 * UNIT)` | same | Bond required to enter deciding |

Fastest end to end on track 1: 10 min prepare + 10 min confirm + 10 min enactment ≈ **30 minutes** after submission (plus the time to place the decision deposit and collect 8 ayes); the WASM can be applied in the block after `authorize_upgrade` executes. Track 0: 2 h + 24 h + 24 h ≈ 50 h.

To change a track's timing, edit its `TrackInfo` in `create_tech_collective_tracks`. Any change is itself a runtime upgrade and only takes effect once shipped through the `fast_upgrade` lane (the release workflow bumps `spec_version`, `runtime/src/lib.rs`, currently 147).

Test override: with the `fast-governance` cargo feature, `apply_test_timing` forces all four periods of **both** tracks to 2 blocks. Production builds never compile it.

The `TechReferendaInstance` `Config` (`runtime/src/configs/mod.rs`) also sets:

- `ReferendumSubmissionDeposit = scale_fee(10 * UNIT)`: submission bond, sized to stay above the maximum preimage deposit of a 64 KiB proposal so noted bytes remain collateralized after `unnote`.
- `UndecidingTimeout = 45 * DAYS`: if a referendum never enters deciding within this window (no decision deposit, or no free `max_deciding` slot on its track), it is rejected as `TimedOut` (`pallet-referenda-45.0.0/src/lib.rs:1164-1177`).
- `AlarmInterval = 1`: granularity of scheduler wake-ups that re-service referenda state. 1 = state transitions (begin/abort confirmation, approve, reject) can happen on any block.

## 2. Thresholds

### Verified tally semantics

`pallet-ranked-collective-45.0.0/src/lib.rs:97-136`:

```rust
pub struct Tally<T, I, M: GetMaxVoters> { bare_ayes: MemberIndex, ayes: Votes, nays: Votes, ... }
fn support(&self, class) -> Perbill { Perbill::from_rational(self.bare_ayes, M::get_max_voters(class)) }
fn approval(&self, _) -> Perbill { Perbill::from_rational(self.ayes, 1.max(self.ayes + self.nays)) }
```

- **approval** = weighted ayes / (ayes + nays) — abstainers excluded.
- **support** = `bare_ayes` (head-count of aye voters, unweighted) / total members of the class. `get_max_voters` returns `MemberCount[MinRankOfClass]` (lib.rs:266-271); `MinRankOfClassConverter` always returns rank 0 (`definitions.rs`), so the denominator is the full membership. Nay votes do not add support; abstention counts against support.
- **vote weight**: `type VoteWeight = Linear` = `excess_rank + 1` votes (lib.rs:236-241). `PromoteOrigin = NeverEnsureOrigin`, so every member stays at rank 0 → exactly 1 vote each, and weighted `ayes == bare_ayes`.
- **passing is inclusive**: `y >= self.threshold(x)` (`pallet-referenda-45.0.0/src/types.rs:637-639`). A threshold of exactly 60% lets 3 aye / 2 nay pass, so track 0's `min_approval` is 61%, strictly above 3/5; track 1's 80% is deliberately inclusive so that 8 ayes / 2 nays passes.

### Curves (constant; `floor == ceil` makes `LinearDecreasing` flat)

Both tracks use flat curves, so the required numbers never decay over the decision period (`fast_track_curves_pin_eight_of_ten` in `runtime/tests/governance/fast_upgrade.rs` pins this for track 1):

| Track | `min_approval` | `min_support` | 10 members (staging-mainnet tech collective) | 5 members (`MIN_TECH_COLLECTIVE_MEMBERS`, dev/testnet presets) |
|---|---|---|---|---|
| 0 | 61% | 60% | 6 ayes; 4 nays block | 3 ayes; 2 nays block |
| 1 `fast_upgrade` | 80% | 80% | 8 ayes; 3 nays block | 4 ayes; 2 nays block |

(61% is the loosest value strictly above 3/5: `Perbill::from_rational(3,5)` is exactly 600,000,000 < 610,000,000, so the comparison is exact, no rounding hazard. 2/3 ≈ 66.7% would behave identically for n = 5 and n = 10.)

### Verification, 10 members, all rank 0

Track 0 (approval ≥ 61%, support ≥ 60%):

| Ayes | Nays | Approval = a/(a+n) | ≥61%? | Support = ayes/10 | ≥60%? | Result |
|---|---|---|---|---|---|---|
| 6 | 0 | 100% | yes | 60% | yes (inclusive) | **PASS** |
| 6 | 3 | 66.7% | yes | 60% | yes | **PASS** |
| 6 | 4 | 60% | **no** | 60% | yes | **FAIL** |
| 7 | 3 | 70% | yes | 70% | yes | **PASS** |
| 5 | 0 | 100% | yes | 50% | **no** | **FAIL** |

Track 1 (approval ≥ 80%, support ≥ 80%):

| Ayes | Nays | Approval = a/(a+n) | ≥80%? | Support = ayes/10 | ≥80%? | Result |
|---|---|---|---|---|---|---|
| 8 | 0 | 100% | yes | 80% | yes (inclusive) | **PASS** |
| 8 | 2 | 80% | yes (inclusive) | 80% | yes | **PASS** |
| 9 | 1 | 90% | yes | 90% | yes | **PASS** |
| 7 | 0 | 100% | yes | 70% | **no** | **FAIL** |
| 7 | 3 | 70% | **no** | 70% | **no** | **FAIL** |

Requirements, track 1: (a) 8/10 ayes authorize an upgrade even against both remaining nays ✓ (`eight_of_ten_authorizes_the_upgrade`); (b) 3 nays always block, since support cannot reach 80% ✓ (`seven_of_ten_is_not_enough`); (c) no minority can force one: 7 ayes fail regardless of nays ✓. Track 0: (a) 6/10 ayes execute with up to 3 nays ✓; (b) 4 nays block (6a/4n = 60% < 61%) ✓; (c) 3 compromised members cannot block (7a/3n passes) ✓.

With 5 members the same math gives 3-of-5 on track 0 (3a/1n passes, 3a/2n fails, 2 ayes = 40% support fails) and 4-of-5 on track 1 (4a/1n = 80% passes, 3 ayes = 60% support fails); 2 nays block either track.

### Confirm/decision periods are security parameters

A referendum must be *continuously* passing for the whole `confirm_period`; any nay that drops it below threshold aborts confirmation (`ConfirmAborted`, `pallet-referenda-45.0.0/src/lib.rs:1235-1240`) and confirmation must restart. Approval only happens at `lib.rs:1190-1208` after the confirm deadline elapses while still passing. So `confirm_period` is the honest members' reaction window:

- **Track 0: 24 h.** Even if all ayes land in the first block of deciding, approval cannot conclude before a full day has passed; worst case (ayes arrive at the end of the decision window) approval takes up to ~48 h. `prepare_period` (2 h) bounds advance notice before deciding starts.
- **Track 1: 10 minutes**, after a 10-minute `prepare_period`. From submission, the earliest an upgrade hash can be authorized is ~30 min, and a dissenting member must have voted nay within ~20 min to abort confirmation. This is the trade the fast lane makes: a much shorter window in exchange for a much larger quorum (8 of 10) and a dispatch that can only publish an upgrade hash. Members must expect to react within prepare + confirm, not within a day.

On both tracks, if the referendum is not passing when `decision_period` (1 day) ends and is not confirming, it is rejected.

## 3. Vote changing

**Yes — a member can flip their vote any time while the poll is Ongoing.** `pallet-ranked-collective-45.0.0/src/lib.rs:632-675` (`vote`): an existing vote is first subtracted from the tally, then the new vote is applied and overwrites `Voting`:

```rust
match Voting::<T, I>::get(&poll, &who) {
    Some(Aye(votes)) => { tally.bare_ayes.saturating_dec(); tally.ayes.saturating_reduce(votes); },
    Some(Nay(votes)) => tally.nays.saturating_reduce(votes),
    None => pays = Pays::No,
}
...
Voting::<T, I>::insert(&poll, &who, &vote);
```

The first vote on a poll is fee-free (`Pays::No`); changes pay a fee. Voting on `Completed`/missing polls fails with `NotPolling` (lib.rs:646-647). Consequence: an aye cast early can be flipped to nay during confirmation to abort it — this is what makes `confirm_period` an effective defense window. On track 1 that window is the 10-minute confirm period.

## 4. Cancellation and incident response

Tech track `Config` (`runtime/src/configs/mod.rs`):

```rust
type CancelOrigin = EnsureRoot<AccountId>;
type KillOrigin = EnsureRoot<AccountId>;
```

- `cancel` (`pallet-referenda-45.0.0/src/lib.rs:591-606`): stops an ongoing referendum, **refunds** submission + decision deposits.
- `kill` (`lib.rs:616-630`): stops it and **slashes** both deposits (`Slash = ()` → burned).

Both are Root-only, and Root is only reachable via a passed track-0 referendum (≥ 2 h prepare + 24 h confirm + 24 h enactment). That is a chicken-and-egg problem on track 0 — `max_deciding: 1` means a cancel referendum cannot even enter deciding while the malicious one holds the slot — and it is simply too slow for track 1: a fast-track referendum authorizes its hash ~30 min after submission, long before any Root cancel could enact. On both tracks the practical defense is votes — 4 honest nays on track 0 and 3 on track 1 with 10 members (2 on either track with 5 members) — cast or flipped before confirmation completes. `Origins::FastUpgrade` itself cannot cancel, kill, or change membership: the only call that honors it is `system.authorize_upgrade` (`fast_track_cannot_dispatch_arbitrary_root_calls`, `direct_authorize_upgrade_origin_checks` in `runtime/tests/governance/fast_upgrade.rs`). Recommendation, more pressing now that a 30-minute lane exists: give `CancelOrigin` to a smaller quorum (e.g. `EnsureRoot` OR a few-member ranked-collective origin via an `EitherOf<EnsureRoot<...>, EnsureRankedMember<...>>`-style construct, or a dedicated fast cancel track), keep `kill` Root-only.

Once `authorize_upgrade` has executed, the remaining safeguards are in the apply step: `apply_authorized_upgrade` accepts only the exact bytes whose hash was authorized and checks the runtime version, and it is permissionless, so it can land in the very next block. `AuthorizedUpgrade` is a single storage slot — a later `authorize_upgrade` (either track) overwrites a pending one, and a successful apply clears it — but an attacker who got a hash authorized will apply immediately, so authorization must be treated as final.

Member removal mid-flight: `remove_member` requires `RemoveOrigin`, which this runtime sets to `EnsureRootRemoveKeepsMemberFloor` (`governance/definitions.rs`) — **Root only**, and only while the removal leaves at least `MIN_TECH_COLLECTIVE_MEMBERS` (5, `genesis_config_presets.rs`) members. That floor is load-bearing: shrinking below it would collapse the tech-referenda curves or (at zero members) deadlock the only governance lane. `AddOrigin` remains `EnsureRootWithSuccess<AccountId, ConstU16<0>>` (#91267). Membership changes therefore require a passed track-0 referendum: a single member can no longer unilaterally remove the others or stuff the collective up to `MaxMemberCount = 13`. (Genesis seeding bypasses the origin via `do_add_member_to_rank`, as before.)

Removal intentionally does not reconcile ongoing tallies — this matches upstream `pallet-ranked-collective` (eager reconciliation would be an unbounded `Voting` scan inside a scheduler-enacted dispatch). A removed member's already-cast votes keep counting until each poll ends, while the support denominator `MemberCount[0]` shrinks immediately; the tally's `support` clamps at 100%, so the distortion is bounded and expires with the poll (at most `decision_period` + `confirm_period`). Completed-poll vote records are swept permissionlessly via `cleanup_poll`.

## 5. Security summary (10-member mainnet collective)

Assumes membership management stays Root-only with the member-floor `RemoveOrigin` (see §4) and every member at rank 0 (1 vote each).

Track 0 — Root proposals (approval 61%, support 60%, 24 h confirm):

| Compromised members | Can block? | Can force a Root call? |
|---|---|---|
| 1–3 | No (7a/3n = 70% passes) | No (support ≤ 30% < 60%) |
| 4–5 | **Yes** — 4 nays hold 6 ayes at 60% < 61% (availability risk only) | No (support ≤ 50%) |
| 6 | Yes | **Only if** fewer than 4 honest nays land within decision + confirm window (6a/3n passes, 6a/4n fails) |
| 7+ | Yes | **Yes, always** (7a/3n = 70% approval) |

Track 1 — upgrade-hash authorization (approval 80%, support 80%, 10 min confirm):

| Compromised members | Can block an upgrade? | Can force an upgrade? |
|---|---|---|
| 1–2 | No (8a/2n passes) | No (support ≤ 20%) |
| 3–7 | **Yes** — support is capped at ≤ 70% (availability risk only; fallback is a track-0 Root `authorize_upgrade`, ~2 days) | No (support ≤ 70% < 80%) |
| 8+ | Yes | **Yes, always**: 8a/2n = 80%; the hash is authorized ~30 min after submission and the WASM can be applied permissionlessly in the next block |

Design assumptions: track 0 needs 4 honest members able to vote nay within `decision_period + confirm_period` (24 h + 24 h; 2 with a 5-member collective). Track 1 needs **3 honest members able to vote nay within ~20 minutes of submission** (prepare + confirm) and stays available only while 8 members are honest and online; in exchange, fewer than 8 colluding members can never push a hash through, and even 8 can only publish an upgrade hash — never an arbitrary Root call.
