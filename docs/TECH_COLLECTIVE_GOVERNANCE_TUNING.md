# Tech Collective Governance Tuning (Mainnet Parameters)

Scope: the runtime-upgrade track (`TechReferenda` = `pallet_referenda::Pallet<Runtime, Instance1>` + `TechCollective` = `pallet_ranked_collective`). Sources verified against the runtime and crate sources `pallet-referenda-45.0.0` / `pallet-ranked-collective-45.0.0` (cargo registry; not vendored in `pallets/`).

## 1. Timing

All periods are in blocks. `runtime/src/lib.rs:84-89`: `TARGET_BLOCK_TIME_MS = 12_000`, so `MINUTES = 5`, `HOURS = 300`, `DAYS = 7200` blocks.

Track definition: `runtime/src/governance/definitions.rs:165-192` (`TechCollectiveTracksInfo::create_tech_collective_tracks`):

```rust
max_deciding: 1,
decision_deposit: 1000 * UNIT,
prepare_period: 20,
decision_period: DAYS,
confirm_period: DAYS,
min_enactment_period: DAYS,
```

| Parameter | Current (blocks) | Wall clock | Meaning |
|---|---|---|---|
| `prepare_period` | 20 | 4 min | Delay between submission and decision start |
| `decision_period` | 7200 (`DAYS`) | 24 h | Window in which the referendum must reach passing state |
| `confirm_period` | 7200 (`DAYS`) | 24 h | Must remain continuously passing this long to be approved |
| `min_enactment_period` | 7200 (`DAYS`) | 24 h | Min delay between approval and dispatch of the upgrade |

To change approval duration, edit these four fields in `definitions.rs:174-177`. Any change is a runtime upgrade: it only takes effect once shipped via this same track (the release workflow bumps `spec_version`, `runtime/src/lib.rs:76`, currently 131).

Test override: with the `fast-governance` cargo feature, `apply_test_timing` (`definitions.rs:87-95`) forces all four periods to 2 blocks. Production builds never compile it.

Instance1 `Config` (`runtime/src/configs/mod.rs:335-377`) also uses, from `configs/mod.rs:263-264`:

- `UndecidingTimeout = 45 * DAYS` (note: the comment at line 262 says "90 days" — the value is 45). If a referendum never enters deciding within this window (no decision deposit, or no free `max_deciding` slot), it is rejected as `TimedOut` (`pallet-referenda-45.0.0/src/lib.rs:1164-1177`).
- `AlarmInterval = 1`: granularity of scheduler wake-ups that re-service referenda state. 1 = state transitions (begin/abort confirmation, approve, reject) can happen on any block.

Both constants are shared with the community-track instance (`impl pallet_referenda::Config for Runtime`, `configs/mod.rs:267-309`; tech track is the `Config<TechReferendaInstance>` impl at 335).

## 2. Thresholds for a 5-member collective

### Verified tally semantics

`pallet-ranked-collective-45.0.0/src/lib.rs:97-136`:

```rust
pub struct Tally<T, I, M: GetMaxVoters> { bare_ayes: MemberIndex, ayes: Votes, nays: Votes, ... }
fn support(&self, class) -> Perbill { Perbill::from_rational(self.bare_ayes, M::get_max_voters(class)) }
fn approval(&self, _) -> Perbill { Perbill::from_rational(self.ayes, 1.max(self.ayes + self.nays)) }
```

- **approval** = weighted ayes / (ayes + nays) — abstainers excluded.
- **support** = `bare_ayes` (head-count of aye voters, unweighted) / total members of the class. `get_max_voters` returns `MemberCount[MinRankOfClass]` (lib.rs:266-271); `MinRankOfClassConverter` always returns rank 0 (`definitions.rs:238-243`), so the denominator is the full membership. Nay votes do not add support; abstention counts against support.
- **vote weight**: `type VoteWeight = Linear` (`configs/mod.rs:326`) = `excess_rank + 1` votes (lib.rs:236-241). `PromoteOrigin = NeverEnsureOrigin` (`configs/mod.rs:320`), so every member stays at rank 0 → exactly 1 vote each, and weighted `ayes == bare_ayes`.
- **passing is inclusive**: `y >= self.threshold(x)` (`pallet-referenda-45.0.0/src/types.rs:637-639`). Therefore a threshold of exactly 60% would let 3 aye / 2 nay pass (60% ≥ 60%). `min_approval` must be strictly above 3/5.

### Current curves (constant; `floor == ceil` makes `LinearDecreasing` flat)

Implemented at `definitions.rs:178-187`:

```rust
min_approval: pallet_referenda::Curve::LinearDecreasing {
    length: Perbill::from_percent(100),
    floor: Perbill::from_percent(61),   // strictly above 3/5
    ceil: Perbill::from_percent(61),
},
min_support: pallet_referenda::Curve::LinearDecreasing {
    length: Perbill::from_percent(100),
    floor: Perbill::from_percent(60),   // 3 of 5 members must actively vote aye
    ceil: Perbill::from_percent(60),
},
```

(2/3 ≈ 66.7% for `min_approval` works identically for n=5; 61% is the loosest safe value. `Perbill::from_rational(3,5)` is exactly 600,000,000 < 610,000,000, so the comparison is exact, no rounding hazard.)

### Verification, 5 members, all rank 0

| Ayes | Nays | Approval = a/(a+n) | ≥61%? | Support = ayes/5 | ≥60%? | Result |
|---|---|---|---|---|---|---|
| 3 | 0 | 100% | yes | 60% | yes (inclusive) | **PASS** |
| 3 | 1 | 75% | yes | 60% | yes | **PASS** |
| 3 | 2 | 60% | **no** | 60% | yes | **FAIL** |
| 4 | 1 | 80% | yes | 80% | yes | **PASS** |
| 2 | 0 | 100% | yes | 40% | **no** | **FAIL** |

Requirements: (a) 3/5 ayes execute ✓ (rows 1–2); (b) 2 nays block ✓ (row 3); (c) 1 bad member cannot block ✓ (row 2: passes despite 1 nay) nor push through alone (1 aye = 20% support); (d) against 1 honest nay, 3 compromised ayes fail (row 3 generalizes: 3a/2n fails, and 3a/1n passes — so blocking needs 2 honest nays; forcing through 2 honest nays needs 4 ayes, row 4) ✓.

### Confirm/decision periods are security parameters

A referendum must be *continuously* passing for the whole `confirm_period`; any nay that drops it below threshold aborts confirmation (`ConfirmAborted`, `pallet-referenda-45.0.0/src/lib.rs:1235-1240`) and confirmation must restart. Approval only happens at `lib.rs:1190-1208` after the confirm deadline elapses while still passing. So `confirm_period` is the honest members' reaction window: at the current 24 h, even if all ayes land in the first block, approval cannot conclude before a full day has passed — dissenting nays always have that window. Worst case (ayes arrive at the end of the decision window) approval takes up to ~48 h; if the referendum is not passing when `decision_period` ends and is not confirming, it is rejected. `prepare_period` (currently 4 min) bounds advance notice before deciding starts and could be raised to hours on mainnet.

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

The first vote on a poll is fee-free (`Pays::No`); changes pay a fee. Voting on `Completed`/missing polls fails with `NotPolling` (lib.rs:646-647). Consequence: an aye cast early can be flipped to nay during confirmation to abort it — this is what makes `confirm_period` an effective defense window.

## 4. Cancellation and incident response

Tech track Instance1 config, `runtime/src/configs/mod.rs:348-351`:

```rust
type CancelOrigin = EnsureRoot<AccountId>;
type KillOrigin = EnsureRoot<AccountId>;
```

(The community instance is identical, `configs/mod.rs:280-283`.)

- `cancel` (`pallet-referenda-45.0.0/src/lib.rs:591-606`): stops an ongoing referendum, **refunds** submission + decision deposits.
- `kill` (`lib.rs:616-630`): stops it and **slashes** both deposits (`Slash = ()` → burned, `configs/mod.rs:356`).

Both are Root-only. Root is only reachable via a passed referendum, so cancelling a malicious tech referendum requires winning *another* referendum on the same track before the first one enacts — a chicken-and-egg problem, made worse by `max_deciding: 1` (`definitions.rs:172`): a second referendum cannot even enter deciding until the first leaves it. During an attack the practical defense is votes (2 honest nays), not cancellation. Recommendation: give `CancelOrigin` to a smaller quorum (e.g. `EnsureRoot` OR a 2-of-5 ranked-collective origin via `EitherOf<EnsureRoot<...>, EnsureRankedMember<...>>`-style construct, or a dedicated fast cancel track), keep `kill` Root-only.

Member removal mid-flight: `remove_member` (`pallet-ranked-collective-45.0.0/src/lib.rs:600-617`) requires `RemoveOrigin`, which this runtime sets to `RootOrMemberForCollectiveOrigin` (`configs/mod.rs:319`) — **Root or any single collective member** (`definitions.rs:266-287`). `AddOrigin` (line 318) is the same. This is currently the weakest link: one compromised member can unilaterally remove the other four (all rank 0, so the `max_rank >= rank` check at lib.rs:609 always passes) or stuff the collective up to `MaxMemberCount = 13`, invalidating all threshold math above. For mainnet, membership changes must require Root (i.e. a passed referendum) or an equivalent quorum.

Removal does **not** touch ongoing tallies: `do_remove_member_from_rank` (lib.rs:886-892) clears member indices only — cast votes stay counted, but the support denominator `MemberCount[0]` shrinks immediately, *raising* the support percentage of remaining ayes (e.g. after removing 2 of 5 members, 3 ayes = 100% support). Membership changes during a live referendum therefore shift its outcome.

## 5. Security summary (current 5-member config: approval 61%, support 60%)

Assumes membership management is fixed to Root-only (see §4); with the current any-member `RemoveOrigin`, none of the rows below hold.

| Compromised members | Can block upgrades? | Can force an upgrade? |
|---|---|---|
| 1 | No (3a/1n passes) | No (support 20% < 60%) |
| 2 | **Yes** — 2 nays hold approval ≤60% < 61% (availability risk only) | No (support 40%) |
| 3 | Yes | **Only if** fewer than 2 honest nays land within decision + confirm window (3a/0n and 3a/1n pass; 3a/2n fails) |
| 4 | Yes | **Yes, always** (4a/1n = 80% approval) |

Design assumption: at least 2 honest members are online and able to vote nay within `decision_period + confirm_period`. That window is 24 h + 24 h: even in the fastest case (all ayes in the first block of deciding), honest members have a full 24 h confirm window to abort.
