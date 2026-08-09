# Security review follow-ups: event-scan metering, wormhole fee rounding

Addresses a batch of security-review findings. Two are fixed with red→green
regression tests, one is documented as an accepted design trade-off, and four
were assessed as not actionable (rationale below, for the review response).

## Fixes

### 1. Size-aware event-scan charge (`2e33dbe3`)

The proof-recorder extension's post-dispatch event scan was priced at a flat
1µs per `EventRecord`, based on a ~300-byte record assumption. The scan
stream-decodes every record present (`skip()` still decodes the prefix it
discards), and a legitimate `Multisig::ProposalExecuted` record can carry a
10 KiB stored call plus up to 100 approvers — an order of magnitude past the
assumption — with the charge landing via `register_extra_weight_unchecked`,
unbounded by block admission.

- Added `frame_system::Pallet::event_bytes()`: reads the encoded length of the
  `Events` storage value without decoding it.
- `event_scan_weight` now charges a per-record structural term **plus** a
  per-byte payload term (10ns/byte, ~10× ceiling over pure decode).
- New test: `wormhole_proof_recorder_scan_weight_scales_with_event_bytes`.

**Review follow-up: made the scan itself linear.** The linear charge could not
bound the previous implementation: `Events::stream_iter()` refills a 2 KiB
buffer via `sp_io::storage::read`, and each such host call materializes the
complete overlay-resident `Events` value before slicing out the window —
O(bytes²) total copying for a full scan (review measured 16× bytes → ~69×
time). Added `frame_system::Pallet::read_events_no_consensus_single_copy()`,
which fetches the encoded value exactly once (`sp_io::storage::get`) and
decodes records from the in-memory buffer; the scan now uses it and meters the
per-byte charge from the very buffer it decoded. New tests:
`single_copy_event_reader_matches_the_streaming_reader` (decode equivalence)
and `wormhole_proof_recorder_scan_of_many_small_records_registers_formula_weight`
(20,000-record workload pins the formula at scale).

### 2. Wormhole volume fee: quantized-ceiling settlement (`7d7c4bd9`)

`process_exit_bundle` recomputed the volume fee in base units with truncating
division, while the circuit's integer relation over quantized amounts
(`out·10000 ≤ input·(10000−bps)`) forces every nonzero exit to lock at least
one full quantum (0.01 QUAN) of fee. A one-quantum exit therefore settled
4,001,600 base units instead of 10^10; the shortfall silently vanished from
burn, miner, and aggregator allocation.

- New `Pallet::volume_fee_for_exit`: `fee = ceil(exit_quanta · bps / (10000 −
  bps))` quanta, scaled back to base units. Exact (minted totals are always
  whole quanta), reproduces the proof-side minimum, and over the bundle total
  never exceeds what the proofs locked (`ceil(Σ) ≤ Σ ceil`).
- Updated `docs/wormhole-zk.md` and the six test sites that mirrored the old
  formula (now via an independently derived `ceil_volume_fee` test helper).
- New tests: `process_exit_bundle_settles_the_one_quantum_minimum_fee`
  (red: 4,001,600 vs 10,000,000,000),
  `process_exit_bundle_rounds_the_volume_fee_up_to_whole_quanta`
  (red: 20,008,003,201 vs 30,000,000,000).

## Accepted trade-off, now documented

### 3. Post-hoc, not-fee-charged event scan (`a522d340`)

Finding: the event scan and proof-recording shortfall are registered against
the block after dispatch and never priced into the transaction fee.

Accepted as designed, with the rationale now recorded next to the code:

- It **cannot** be fee-charged: the scan cost depends on how many events
  earlier transactions in the same block emitted, unknowable at fee time.
  `TransactionExtension::weight()` is static, and fee correction only refunds
  downward. Reserving the worst-case whole-block scan per transaction would
  collapse throughput.
- Block capacity stays sound: the extra weight accrues into `BlockWeight`
  before the next transaction's `CheckWeight` admission; worst case is a
  bounded one-transaction overshoot at the block boundary (the same pattern
  FRAME accepts for `on_initialize`).
- The economics don't invert: emitting events is fully fee-charged through the
  emitting calls' benchmarked weights, orders of magnitude above the ~1µs/record
  + ~10ns/byte decode cost imposed on the scan.

## Findings assessed as not actionable

### Treasury setter accepts reserved protocol accounts — rejected

`set_treasury_account` is root-only, and *any* wrong address (not just the
vesting pot or minting sentinel) strands future treasury rewards identically,
with no migration back. Denylisting two of 2^256 wrong values adds no real
protection; the safeguard is governance verifying the address it proposes.
The existing zero-address check covers the one structurally detectable case.

### Vesting `payout_weight` tree-model double count — deferred / accepted overcharge

The PoV term charges per raw read+write operation (`4d+9` keys) where the
distinct-key union is `4d+4` (`LeafCount`/`Depth`/`Root` appear in both sets),
a flat ~13 KB proof-size overcharge per payout; the benchmarked baseline's
tree footprint also stays under the added model (small, not safely
subtractable without regenerating baselines). Every error is in the
conservative direction (overcharge only — slightly higher fees, slightly
fewer payouts per block; no undercharge, no security impact). The weight
model for this code is being consolidated on the `illuzen/v12-vesting`
branch, so any correction belongs there, not here.

### `retarget_schedule` skips `treasury_and_pot()` — rejected

`treasury_and_pot()` is not a lifecycle-wide invariant; it is a lookup for the
two calls that move funds to/from the treasury (`create_schedule` funds the
pot from it, `end_schedule` refunds to it). `retarget_schedule` touches the
treasury in no way: it settles what any permissionless `claim` could already
force and swaps the beneficiary (validating `new_beneficiary != pot` and
`!= current`). Adding the check would block rotating a compromised
beneficiary key exactly when the treasury is misconfigured, for no safety
gain.
