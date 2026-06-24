# Tech Referenda Feature Reference

Capability map of the tech-collective governance lane: every voting feature the
pallets expose, what is wired today, and what is dormant but available. For the
mainnet *parameter tuning* and *security analysis* (threshold math, attack
tables, incident response) see [`TECH_COLLECTIVE_GOVERNANCE_TUNING.md`](./TECH_COLLECTIVE_GOVERNANCE_TUNING.md);
this document avoids repeating those numbers and focuses on mechanics.

Sources are the **local forks** at `pallets/referenda/` and
`pallets/ranked-collective/`, plus the runtime wiring in `runtime/src/`.

---

## 1. Architecture: two pallets, one lane

| Pallet | Runtime alias / index | Role |
|---|---|---|
| `pallet-ranked-collective` | `TechCollective` (13) | Membership, ranks, vote casting, vote weighting |
| `pallet-referenda` `Instance1` | `TechReferenda` (14) | Proposal lifecycle: submit → decide → confirm → enact |

They reference each other in `runtime/src/configs/mod.rs`:

```251:267:runtime/src/configs/mod.rs
impl pallet_ranked_collective::Config for Runtime {
	...
	type Polls = pallet_referenda::Pallet<Runtime, TechReferendaInstance>;
	type MinRankOfClass = MinRankOfClassConverter<MinRankOfClassDelta>;
	type VoteWeight = Linear;
	type MaxMemberCount = GlobalMaxMembers<MaxMemberCount>;
	...
}
```

Referenda's `Tally = pallet_ranked_collective::TallyOf<Runtime>` and
`SubmitOrigin = RootOrMemberForTechReferendaOrigin` close the loop
(`configs/mod.rs:271-313`).

---

## 2. Functions (extrinsics)

### `TechCollective` (ranked-collective)
| Call | Origin in this runtime | Notes |
|---|---|---|
| `add_member` / `remove_member` | `RootOrMemberForCollectiveOrigin` | Add/remove at rank 0 |
| `promote_member` / `demote_member` | `NeverEnsureOrigin` → **disabled** | Rank changes frozen post-genesis |
| `exchange_member` | `NeverEnsureOrigin` → **disabled** | Account swap unreachable |
| `vote(poll, aye)` | member | Rank-weighted aye/nay; re-votable while Ongoing |
| `cleanup_poll` | signed | GC vote records after a poll ends |

### `TechReferenda` (referenda `Instance1`)
| Call | Origin | Notes |
|---|---|---|
| `submit` | Root or member | Create a referendum |
| `place_decision_deposit` / `refund_decision_deposit` | signed | 1000 UNIT decision bond |
| `refund_submission_deposit` | signed | 100 UNIT submission bond |
| `nudge_referendum` | **permissionless** | Force the state machine to re-evaluate now (the "fast resolution" lever, §6) |
| `cancel` | Root | Stop; refunds deposits |
| `kill` | Root | Stop; slashes deposits (`Slash = ()` → burned) |
| `one_fewer_deciding` | signed | Free a deciding slot |
| `set_metadata` | signed | Attach a preimage hash describing the proposal |

---

## 3. Vote-weight schemes (three available, `Linear` selected)

The `VoteWeight` trait maps a member's *excess rank* to a number of votes. Three
implementations exist:

```220:257:pallets/ranked-collective/src/lib.rs
pub struct Unit;        // every voter = 1 vote, rank ignored
...
pub struct Linear;      // votes = excess_rank + 1   (1,2,3,4,5,...)
...
pub struct Geometric;   // votes = (r+1)(r+2)/2      (1,3,6,10,15,...) triangular
```

Excess rank = member rank − the track's minimum rank:

```761:764:pallets/ranked-collective/src/lib.rs
fn rank_to_votes(rank: Rank, min: Rank) -> Result<Votes, DispatchError> {
	let excess = rank.checked_sub(min).ok_or(Error::<T, I>::RankTooLow)?;
	Ok(T::VoteWeight::convert(excess))
}
```

**Today this is effectively one-member-one-vote.** `MinRankOfClassConverter`
returns `0` for every track and promote/demote are disabled, so every member is
rank 0 → `excess = 0` → `Linear(0) = 1`. The ranking machinery is present and
ready but dormant until rank changes and per-track min-ranks are enabled.

> Note: `Geometric` is *super*-linear in rank — there is **no quadratic-cost
> voting option**, and no token/balance weighting in this lane (see §7).

---

## 4. Tally: two independent metrics

The tally tracks `bare_ayes` (head-count), `ayes` (weighted), `nays` (weighted)
and exposes two ratios; a referendum must satisfy **both**:

```128:136:pallets/ranked-collective/src/lib.rs
fn ayes(&self, _: ClassOf<T, I>) -> Votes { self.bare_ayes }
fn support(&self, class) -> Perbill {
	Perbill::from_rational(self.bare_ayes, M::get_max_voters(class))
}
fn approval(&self, _) -> Perbill {
	Perbill::from_rational(self.ayes, 1.max(self.ayes + self.nays))
}
```

- **Approval** ("unity") = weighted `ayes / (ayes + nays)` — abstainers excluded.
- **Support** ("quorum") = **unweighted** `bare_ayes / total_members` — a true
  turnout gate, immune to rank weighting; nays and abstentions do not add support.

---

## 5. Threshold curves (three shapes; constant today)

Each track sets `min_approval` and `min_support` as a `Curve` evaluated against
*time elapsed into the decision period*. Three shapes are available:

```424:435:pallets/referenda/src/types.rs
pub enum Curve {
	LinearDecreasing { length, floor, ceil },          // straight-line decay
	SteppedDecreasing { begin, end, step, period },     // staircase decay
	Reciprocal { factor, x_offset, y_offset },          // K/(x+S)-T hyperbola
}
```

`passing(x, y)` is simply `y >= threshold(x)` (`types.rs:640-642`), inclusive.
Decaying curves let a track demand high approval/support early and relax it over
time (the OpenGov design). The track config lives in `TrackInfo`
(`pallets/referenda/src/types.rs:189-215`).

The tech track currently uses **constant** curves (`LinearDecreasing` with
`floor == ceil`, i.e. no decay):

```111:120:runtime/src/governance/definitions.rs
min_approval: pallet_referenda::Curve::LinearDecreasing {
	length: from_percent(100), floor: from_percent(61), ceil: from_percent(61),
},
min_support: pallet_referenda::Curve::LinearDecreasing {
	length: from_percent(100), floor: from_percent(60), ceil: from_percent(60),
},
```

→ flat **61% approval + 60% support** for the entire window. Swapping in
`Reciprocal` / `SteppedDecreasing` (or a decaying linear curve) makes the
thresholds time-dependent.

---

## 6. Lifecycle and "fast resolution"

Phases: `prepare_period` → **deciding** (≤ `decision_period`) → **confirming**
(`confirm_period`) → enactment (`min_enactment_period`).

There is no dedicated fast-resolve call. Early resolution comes from the
**confirming** mechanism plus the permissionless **`nudge_referendum`** poke.
Once both curves are satisfied the referendum enters confirming; if it stays
passing for `confirm_period` it is approved **without waiting out the full
decision period**:

```1188:1217:pallets/referenda/src/lib.rs
branch = if is_passing {
	match deciding.confirming {
		Some(t) if now >= t => { /* Passed! schedule enactment, return Approved */ }
		Some(_) => ServiceBranch::ContinueConfirming,
		None => { deciding.confirming = Some(now + track.confirm_period); /* BeginConfirming */ }
	}
} else { /* if decision_period elapsed -> Rejected; else maybe abort confirming */ }
```

`is_passing` re-checks both curves at the current time fraction:

```1314:1325:pallets/referenda/src/lib.rs
let x = Perbill::from_rational(elapsed.min(period), period);
support_needed.passing(x, tally.support(id)) && approval_needed.passing(x, tally.approval(id))
```

If support drops below threshold during confirming, confirmation aborts
(`ConfirmAborted`) and must restart — so it is not a one-way ratchet, which is
what makes `confirm_period` a defense window. `nudge_referendum` lets anyone
force a re-evaluation immediately instead of waiting for the scheduled alarm.

---

## 7. Quadratic / token-based voting — answered

- **Quadratic voting?** No. Weighting is rank-based via the three schemes in §3
  (`Unit`, `Linear`, `Geometric`). `Geometric` is super-linear in rank, the
  opposite direction from quadratic-cost voting. There is no quadratic option.
- **Token-based voting?** Not in this lane. Tech votes are one-member-one-vote
  (rank-weighted), independent of balances. Tokens appear only as fixed
  *deposits* (100 UNIT submission, 1000 UNIT decision) that gate participation
  but never weight a vote. Balance-weighted conviction voting was the separate
  *community lane* (`pallet-conviction-voting` + community `Referenda`), which
  `runtime/src/governance/definitions.rs:93-94` notes has been removed, leaving
  this tech lane as the transitional fallback alongside `pallet-upgrade-gov`.

---

## 8. Capability vs. current configuration

| Knob | Available | Currently set to |
|---|---|---|
| Vote weight | `Unit` / `Linear` / `Geometric` | `Linear` (flat → 1 vote/member; ranks dormant) |
| Ranks | arbitrary 0..N, per-track min-rank | all rank 0, min-rank 0, changes disabled |
| Approval curve ("unity") | Linear / Stepped / Reciprocal, time-decaying | constant **61%** |
| Support curve ("quorum") | same three curve types | constant **60%** (head-count) |
| `max_deciding` | configurable | 1 |
| `prepare / decision / confirm / enactment` | per-track | 20 blocks / 1d / 1d / 1d |
| Deposits | submission + decision | 100 UNIT + 1000 UNIT |
| Max members | `GlobalMaxMembers` | 13 |
| Early resolution | confirm-period pass + `nudge_referendum` | enabled |
| Cancel / kill | `CancelOrigin` / `KillOrigin` | Root only |

---

## 9. Related documents

- [`TECH_COLLECTIVE_GOVERNANCE_TUNING.md`](./TECH_COLLECTIVE_GOVERNANCE_TUNING.md) — threshold math, security tables, incident response.
- [`RUNTIME_UPGRADE_VIA_GOVERNANCE.md`](./RUNTIME_UPGRADE_VIA_GOVERNANCE.md) — using this lane to authorize runtime upgrades.
- [`RUNTIME_SURFACE.md`](./RUNTIME_SURFACE.md) — full runtime pallet inventory.
- `node/src/GOVERNANCE_AUDIT_AND_REDESIGN.md` — audit findings and the `pallet-upgrade-gov` proposal.
