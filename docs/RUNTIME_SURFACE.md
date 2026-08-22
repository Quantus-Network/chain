# Quantus Runtime Surface

Complete inventory of every piece of code that gets compiled into the on-chain
runtime WASM (`quantus-runtime`). This is the authoritative map of the runtime's
attack/audit surface: the runtime crate's own modules, the pallets composed into
the runtime, their dispatchable calls, the runtime APIs, transaction extensions,
genesis logic, and the workspace primitive crates pulled in.

- **Crate:** `quantus-runtime` (`runtime/`), version `0.7.1-q-day-2`
- **Spec:** `spec_name = quantus-runtime`, `spec_version = 147`, `transaction_version = 6`, `authoring_version = 1`
- **Build:** `no_std` WASM via `substrate-wasm-builder` (`runtime/build.rs`); native `std` build for the node/client
- **Block time target:** 12s (`TARGET_BLOCK_TIME_MS = 12_000`)
- **Consensus:** QPoW (quantum-resistant Proof of Work, Poseidon2-based)
- **Signatures:** Dilithium post-quantum signature schemes (ML-DSA-87 and ML-DSA-65)
- **SS58 prefix:** 189

---

## 1. Runtime crate source files (`runtime/src/`)

| File | Responsibility |
| --- | --- |
| `lib.rs` | Crate root. Core type aliases, `RuntimeVersion`, opaque types, `TxExtension`, `UncheckedExtrinsic`, `Executive`, and the `#[frame_support::runtime]` pallet composition (indices 0–21). |
| `configs/mod.rs` | All `impl pallet::Config for Runtime` blocks, `parameter_types!`, fee model, `HighSecurityConfig`, and `TryFrom<RuntimeCall>` impls. |
| `apis.rs` | `impl_runtime_apis!` — every runtime API exposed to the client/RPC. |
| `transaction_extensions.rs` | Custom transaction extensions: `ReversibleTransactionExtension`, `WormholeProofRecorderExtension`. |
| `governance/mod.rs` + `governance/definitions.rs` | Referenda tracks (`CommunityTracksInfo`, `TechCollectiveTracksInfo`), preimage deposit model, custom origins, rank converters. |
| `genesis_config_presets.rs` | Genesis presets: `dev`, `heisenberg`, `planck`; treasury/tech-collective seeding; wormhole endowment. |
| `benchmarks.rs` | `define_benchmarks!` list (only under `runtime-benchmarks`). |

### Core type aliases (`lib.rs`)

| Type | Definition |
| --- | --- |
| `Signature` | `DilithiumSignatureScheme` (post-quantum) |
| `AccountId` | Derived from the Dilithium signer (`AccountId32`) |
| `Balance` | `u128` |
| `AssetId` | `u32` |
| `Nonce` | `u32` |
| `BlockNumber` | `u32` |
| `Hash` | `sp_core::H256` |
| `Difficulty` | `U512` |
| `Address` | `MultiAddress<AccountId, ()>` |
| `Header` | `qp_header::Header<BlockNumber, BlakeTwo256>` (Poseidon block hash, Blake2 state trie) |
| `Block` | `generic::Block<Header, UncheckedExtrinsic>` |
| `Executive` | `frame_executive::Executive<Runtime, Block, ChainContext, Runtime, AllPalletsWithSystem, ()>` |
| `SessionKeys` | empty (`impl_opaque_keys!` — no session keys; PoW chain) |

### Economic constants (`lib.rs`)

`UNIT = 10^12`, `MILLI_UNIT = 10^9`, `MICRO_UNIT = 10^6`, `EXISTENTIAL_DEPOSIT = MILLI_UNIT`,
`MINUTES/HOURS/DAYS` derived from block time, `BLOCK_HASH_COUNT = 2400`.

---

## 2. Pallet composition (`#[frame_support::runtime]`)

The runtime derives `RuntimeCall`, `RuntimeEvent`, `RuntimeError`, `RuntimeOrigin`,
`RuntimeFreezeReason`, `RuntimeHoldReason`, `RuntimeSlashReason`, `RuntimeLockId`, `RuntimeTask`.

| Index | Alias | Source crate | Origin | Calls? |
| --- | --- | --- | --- | --- |
| 0 | `System` | `frame-system` `45.0.0` | **Local fork** (`pallets/frame-system`) | yes |
| 1 | `Timestamp` | `pallet-timestamp` `44.0.0` | **Inlined** (`pallets/timestamp`) | yes |
| 2 | `Balances` | `pallet-balances` `46.0.0` | **Inlined** (`pallets/balances`) | yes |
| 3 | `TransactionPayment` | `pallet-transaction-payment` `45.0.0` | **Inlined** (`pallets/transaction-payment`) | (no extrinsics) |
| 4 | — | *(vacant; was `pallet-sudo`)* | — | — |
| 5 | `QPoW` | `pallet-qpow` | **Local** (`pallets/qpow`) | no |
| 6 | `MiningRewards` | `pallet-mining-rewards` | **Local** (`pallets/mining-rewards`) | no |
| 7 | `Preimage` | `pallet-preimage` `45.0.0` | **Inlined** (`pallets/preimage`) | yes |
| 8 | `Scheduler` | `pallet-scheduler` | **Local fork** (`pallets/scheduler`) | **calls disabled** (`#[runtime::disable_call]`) |
| 9 | `Utility` | `pallet-utility` `45.0.0` | **Inlined** (`pallets/utility`) | yes |
| 10 | — | *(vacant; was community `Referenda`)* | — | — |
| 11 | `ReversibleTransfers` | `pallet-reversible-transfers` | **Local** (`pallets/reversible-transfers`) | yes |
| 12 | — | *(vacant; was `ConvictionVoting`)* | — | — |
| 13 | `TechCollective` | `pallet-ranked-collective` `45.0.0` | **Inlined** (`pallets/ranked-collective`) | yes |
| 14 | `TechReferenda` | `pallet-referenda::Pallet<Runtime, Instance1>` `45.0.0` | **Inlined** (2nd instance) | yes |
| 15 | `TreasuryPallet` | `pallet-treasury` | **Local** (`pallets/treasury`) | yes |
| 16 | — | *(vacant; was `pallet-recovery`)* | — | — |
| 17 | — | *(vacant; was `pallet-assets`)* | — | — |
| 18 | — | *(vacant; was `pallet-assets-holder`)* | — | — |
| 19 | `Multisig` | `pallet-multisig` | **Local** (`pallets/multisig`) | yes |
| 20 | `Wormhole` | `pallet-wormhole` | **Local** (`pallets/wormhole`) | yes |
| 21 | `ZkTree` | `pallet-zk-tree` | **Local** (`pallets/zk-tree`) | no |
| 22 | `Vesting` | `pallet-vesting` | **Local** (`pallets/vesting`) | yes |

> Indices 4, 10, 12, 16, 17, and 18 are intentionally left vacant after pallet removals so downstream indices stay stable.

---

## 3. Pallet configuration & dispatchable surface

All `Config` impls live in `runtime/src/configs/mod.rs` unless noted.

### Index 0 — `System` (`frame-system`, local fork)
- Config via `#[derive_impl(SolochainDefaultConfig)]`. `Block = Block`, `Hashing = BlakeTwo256`, `AccountData = pallet_balances::AccountData<Balance>`, `SS58Prefix = 189`, `MaxConsumers = 16`, `BlockHashCount = 4096`.
- `RuntimeBlockWeights`: 6s ref_time, `proof_size = u64::MAX` (uncapped — solo PoW chain).
- `RuntimeBlockLength`: 5 MB, normal dispatch ratio 75%.
- **Local fork additions:** `ZkTreeRoot` storage + `set_zk_tree_root` / `deposit_log` helpers; intra-block entropy.
- **Calls (call_index):** `remark`(0), `set_heap_pages`(1), `set_code`(2), `set_code_without_checks`(3), `set_storage`(4), `kill_storage`(5), `kill_prefix`(6), `remark_with_event`(7), `do_task`(8), `authorize_upgrade`(9), `authorize_upgrade_without_checks`(10), `apply_authorized_upgrade`(11).

### Index 1 — `Timestamp` (`pallet-timestamp`)
- `Moment = u64`, `MinimumPeriod = 100`, `OnTimestampSet = ()`.
- Provides the timestamp inherent.

### Index 2 — `Balances` (`pallet-balances`)
- `Balance = u128`, `ExistentialDeposit = MILLI_UNIT`, `AccountStore = System`, `MaxLocks = 50`, `MaxFreezes = VariantCountOf<RuntimeFreezeReason>`, hold/freeze reasons wired to runtime enums.

### Index 3 — `TransactionPayment` (`pallet-transaction-payment`)
- `OnChargeTransaction = FungibleAdapter<Balances, pallet_mining_rewards::TransactionFeesCollector<Runtime>>` (100% of fees routed to the block miner).
- `WeightToFee = ScaledIdentityFee` (identity mapping × `FEE_SCALE`; 1s compute ≈ `FEE_SCALE` UNIT).
- `LengthToFee = LengthToFeeMultiplier` (custom, `LENGTH_FEE_MULTIPLIER = 10^6` × `FEE_SCALE`; 1 MB ≈ `FEE_SCALE` UNIT).
- `FeeMultiplierUpdate = ConstFeeMultiplier` (multiplier fixed at 1), `OperationalFeeMultiplier = 5`.
- Every absolute-QUAN price (fees, deposits, the high-security fee cap) derives from the `FEE_SCALE_NUM/DEN` dial in `runtime/src/lib.rs` via `scale_fee`; percentage rates, the existential deposit, and the leaf quantum are deliberately not scaled.

### Index 5 — `QPoW` (`pallet-qpow`, local)
- `InitialDifficulty = U512([4_000_000, 0, …])`, `TargetBlockTime = 12_000ms`, `MaxReorgDepth = 180`, `WeightInfo = ()`.
- No dispatchable calls. Implements `Hooks` (`on_initialize`/`on_finalize`) to track block timing and recompute difficulty. Powers the `QPoWApi` runtime API.

### Index 6 — `MiningRewards` (`pallet-mining-rewards`, local)
- `Currency = Balances`, `ProofRecorder = Wormhole`, `MaxSupply = 21_000_000 * UNIT`, `EmissionDivisor = 15_163_560`, `MintingAccount`, `Unit = UNIT`. Miner credits are aligned to the ZK-tree leaf quantum (`AMOUNT_SCALE_DOWN_FACTOR` = 10^10).
- No dispatchable calls. Exposes `TransactionFeesCollector` + `collect_transaction_fees`. `on_finalize` requires a miner in the digest, combines transaction fees and the block reward into one miner credit, and rounds it down to the wormhole leaf quantum. A missing miner or a sub-quantum remainder stays in `CollectedFees` for the next miner — nothing is minted to treasury.

### Index 7 — `Preimage` (`pallet-preimage`)
- `ManagerOrigin = EnsureRoot`, `Consideration = PreimageDeposit` (custom: 0.1 UNIT base + 0.0001 UNIT/byte, × `FEE_SCALE`, see `governance/definitions.rs`).
- **Calls:** `note_preimage`, `unnote_preimage`, `request_preimage`, `unrequest_preimage`, `ensure_updated` (upstream).

### Index 8 — `Scheduler` (`pallet-scheduler`, local) — **calls disabled**
- `RuntimeCall`, `MaximumWeight = 80% max block`, `MaxScheduledPerBlock = 50`, `ScheduleOrigin = EnsureRoot`, `Preimages = Preimage`, `TimeProvider = Timestamp`, `Moment = u64`, `TimestampBucketSize = 2 * block time`.
- Calls exist (`schedule`(0), `cancel`(1), `schedule_named`(2), `cancel_named`(3), `schedule_after`(4), `schedule_named_after`(5), `set_retry`(6), `set_retry_named`(7), `cancel_retry`(8), `cancel_retry_named`(9)) but are **disabled at the runtime level** so users cannot enqueue arbitrary calls. Used internally by reversible-transfers and governance via the `ScheduleNamed` trait. Local fork adds block-number-or-timestamp scheduling.
- **Priority-reserved headroom:** tasks scheduled at `LOWEST_PRIORITY` (the permissionless scheduling surface — reversible transfers) may occupy at most ~80% of a block's agenda (40 of 50 slots). This reserves ~20% per block — at least one slot even if `MaxScheduledPerBlock < 5` — so a permissionless caller cannot cheaply pre-fill a referendum's deterministic enactment block with priority-255 tasks. Mid-priority tasks (e.g. referendum alarms at 128) can still occupy reserved slots, but `schedule_enactment` retries at `when + 1` for up to 16 blocks on `Exhausted` before logging failure.

### Index 9 — `Utility` (`pallet-utility`)
- `RuntimeCall`, `PalletsOrigin = OriginCaller`.
- **Calls:** `batch_all` only (`call_index` 2). Other FRAME utility combinators (`batch`, `as_derivative`, `dispatch_as`, `force_batch`, `with_weight`, `if_else`, `dispatch_as_fallible`) are omitted.

### Index 10 — `Referenda` (`pallet-referenda`, community instance)
- `Tracks = CommunityTracksInfo` (single "signed" track), `Tally = pallet_conviction_voting::Tally<Balance, DynamicMaxTurnout>`, `SubmitOrigin = EnsureSigned`, `Cancel/KillOrigin = EnsureRoot`, `SubmissionDeposit = 100 UNIT`, `UndecidingTimeout = 45 DAYS`, `Preimages = Preimage`.
- **Calls:** `submit`, `place_decision_deposit`, `refund_decision_deposit`, `cancel`, `kill`, `nudge_referendum`, `one_fewer_deciding`, `refund_submission_deposit`, `set_metadata`.

### Index 11 — `ReversibleTransfers` (`pallet-reversible-transfers`, local)
- `AssetId = u32` (retained for wire-format compatibility; asset transfers are rejected), `Scheduler = Scheduler`, `DefaultDelay = 1 DAY`, `MinDelayPeriodBlocks = 2`, `MaxPendingPerAccount = 16`, `VolumeFee = 1%` (high-security reversals, burned), `ProofRecorder = Wormhole`, `PalletId = "rtpallet"`.
- **Calls:** `set_high_security`(0), `cancel`(1), `execute_transfer`(2), `schedule_transfer`(3), `schedule_transfer_with_delay`(4), `recover_funds`(7). Call indices 5/6 were `schedule_asset_transfer` / `schedule_asset_transfer_with_delay` (removed with assets); kept vacant so `recover_funds` stays at 7. Pending transfers with `Some(asset_id)` fail with `AssetsNotSupported`.
- There is deliberately no on-chain guardian → protected-accounts index (a bounded one could be filled by strangers to grief a popular guardian; enrollment needs no guardian consent since guardianship grants only passive powers). Guardianship is authoritative in `HighSecurityAccounts`; offchain indexers (Subsquid) reconstruct the reverse mapping from `HighSecuritySet` events.
- The guardian holds instant, total seizure power (`recover_funds` sweeps all holds plus the whole free balance to it, no delay, no second approver, immutable relationship), so the recommended guardian is a **multisig address**: `pallet_multisig` dispatches as its derived address, and the cancel/recover lifecycle under a multisig guardian is pinned by an integration test.
- Backs `HighSecurityConfig` (account whitelist/guardian logic).

### Index 12 — `ConvictionVoting` (`pallet-conviction-voting`)
- `Currency = Balances`, `Polls = Referenda`, `MaxTurnout = DynamicMaxTurnout` (scales with total issuance), `VoteLockingPeriod = 7 DAYS`, `MaxVotes = 4096`.
- **Calls:** `vote`, `delegate`, `undelegate`, `unlock`, `remove_vote`, `remove_other_vote`.

### Index 13 — `TechCollective` (`pallet-ranked-collective`)
- `AddOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>` (Root-only, i.e. a passed TechReferenda vote; #91267), `RemoveOrigin = EnsureRootRemoveKeepsMemberFloor` (Root-only **and** refuses removals that would leave fewer than `MIN_TECH_COLLECTIVE_MEMBERS` members — the floor that keeps the tech-referenda lane live), `Promote/Demote/ExchangeOrigin = NeverEnsureOrigin`, `Polls = TechReferenda (Instance1)`, `VoteWeight = Linear`, `MaxMemberCount = 13` (via `GlobalMaxMembers`).
- **Calls:** `add_member`, `promote_member`, `demote_member`, `remove_member`, `vote`, `cleanup_poll`, `exchange_member`. Removal intentionally leaves the member's votes in ongoing tallies (upstream behavior); `support` clamps at 100% so the shrunken electorate cannot overflow the curve.

### Index 14 — `TechReferenda` (`pallet-referenda`, `Instance1`)
- `SubmitOrigin = RootOrMemberForTechReferendaOrigin`, `Tracks = TechCollectiveTracksInfo` (single track, 61% approval / 60% support constant curves), `Tally = pallet_ranked_collective::TallyOf<Runtime>`, `MaxActive = 128` / `MaxActivePerAccount = 8` (global + per-submitter caps on `Ongoing` referenda; storage `ActiveReferendaCount` / `ActiveSubmissionCount`; errors `TooManyActive` / `TooManyActiveBySubmitter`), `MaxProposalSize = 64 KiB`.
- **Calls:** same set as `Referenda` (separate instance/storage).

### Index 15 — `TreasuryPallet` (`pallet-treasury`, local)
- Minimal local treasury. Config only sets `WeightInfo`.
- **Calls:** `set_treasury_account`(0, root). Exposes `account_id()`. Treasury is not paid from mining rewards.

### Index 16 — `Recovery` (`pallet-recovery`) — **removed**
- Social recovery (`as_recovered`, `create_recovery`, …) was removed. High-security fund recovery remains `ReversibleTransfers::recover_funds`.

### Index 19 — `Multisig` (`pallet-multisig`, local)
- `MaxSigners = 100`, `MaxTotalProposalsInStorage = 200`, `MaxCallSize = 10 KB`, `MultisigFee = 0.6 × FEE_SCALE UNIT` (burned), `ProposalDeposit = 1 × FEE_SCALE UNIT`, `ProposalFee = 1 × FEE_SCALE UNIT`, `MaxExpiryDuration ≈ 2 weeks`, `MaxInnerCallWeight = (10^12, 2.5 MB)`, `HighSecurity = HighSecurityConfig`, `PalletId = "py/mltsg"`.
- **Calls:** `create_multisig`(0), `propose`(1), `approve`(2), `cancel`(3), `remove_expired`(4), `claim_deposits`(5), `execute`(6). Exposes `derive_multisig_address`.

### Index 20 — `Wormhole` (`pallet-wormhole`, local)
- `Currency = Balances`, `AssetId = u32` (native leaves tagged as asset id 0 internally; non-native exits unsupported), `VolumeFeeRateBps = 4` (0.04%; circuit ceil-rounds to ≥0.01 QUAN per exit), `VolumeFeesBurnRate = 50%`, `MintingAccount`, `WormholeAccountId = AccountId32`, `ZkTree = ZkTree`. No separate minimum exit amount. No `pallet-assets` dependency.
- **Calls:** `verify_private_batch`(2) — verifies a private-batch ZK proof and processes batched transfers; `verify_public_batch`(3) — verifies a public-batch proof with per-segment denial and aggregator fee rebate.
- Implements `TransferProofRecorder` (`record_transfer`) consumed by mining-rewards, reversible-transfers, and the wormhole tx-extension. `on_initialize` emits genesis endowment proofs at block 1. Loads a static aggregated verifier (`get_aggregated_verifier`).

### Index 21 — `ZkTree` (`pallet-zk-tree`, local)
- `AssetId = u32`, `Balance = u128`. No dispatchable calls.
- **Storage:** `Leaves`, `Nodes`, `LeafCount`, `Depth`, `Root`. Types `ZkLeaf`, `ZkMerkleProof`, `ZkMerkleProofRpc`, `Hash256`.
- `on_finalize` commits the merkle root. Backs the `ZkTreeApi` runtime API.

### Index 22 — `Vesting` (`pallet-vesting`, local)
- Pull-based "vesting wallet": the pallet's sovereign pot (`PalletId(*b"qvesting")`, keyless) holds the entire unclaimed allocation; beneficiaries are paid by plain keep-alive transfers only when a payout is due. **No locks, freezes, or holds ever touch a beneficiary account**, so wormhole addresses can be beneficiaries.
- Config: `Currency = Balances` (`fungible::{Inspect, Mutate}`), `TimeProvider = Timestamp` (ms since epoch), `AdminOrigin = EitherOfDiverse<EnsureRoot, EnsureTreasury>` (`EnsureTreasury` = signed by the configured treasury account; the treasury multisig executes proposals as a plain signed origin), `TreasuryAccount = TreasuryAccountOption` (Option-returning storage read, never panics), `ProofRecorder = Wormhole`, `PayoutQuantum = SCALE_DOWN_FACTOR` (10^10), `MinimumPayout = UNIT` (1 QUAN), `MinClaimInterval = 86,400,000 ms` (24 hours). Non-final claims are further aligned to `pallet_vesting::NON_FINAL_PAYOUT_QUANTA` (2,500) leaf quanta = 25 QUAN, the smallest 4 bps fee-exact multiple.
- **Storage:** `Schedules: schedule_id (u64) → { beneficiary, start, cliff, end, total, claimed, last_claim_at }` (ids sequential, never reused; a beneficiary may hold any number of schedules), `NextScheduleId`. Storage version 0 has no migration: an in-place upgrade with no schedules may leave the pot unfunded, and `create_schedule` then fails with `PotUnderfunded` until the treasury sends it one ED.
- Vesting math: `vested(t) = 0` before `cliff`, `total` from `end`, else `⌊total·(t−start)/(end−start)⌋` (256-bit rational, floor; the `end` branch guarantees exactness).
- **Payout policy:** wormhole leaves commit `amount / 10^10`, so a sub-quantum payout would create a zero-value leaf and strand funds on a keyless beneficiary. Schedule totals must be at least `MinimumPayout` and multiples of `PayoutQuantum`; payouts are quantized and `claimed` stays aligned. A successful claim must pay at least 1 QUAN and be at least 24 hours after that schedule's previous payout. Non-final claims additionally round down to 25 QUAN (`NON_FINAL_PAYOUT_QUANTA` leaf quanta) so each intermediate leaf has an exact 4 bps Wormhole fee; leftover dust stays on the schedule. They also reserve a complete minimum-sized final payout; a claim that cannot avoid a sub-minimum remainder fails with `ClaimWouldLeaveDust` until the full remainder vests. The final claim pays the exact remainder. `end_schedule` pays the unpaid vested part rounded to the nearest `PayoutQuantum` to the beneficiary only when that amount meets `MinimumPayout`; otherwise the sliver is refunded with every leftover planck to the signature-controlled treasury. The refund is not quantized and never blocks ending.
- **Proof recording:** the pallet records each pot → beneficiary payout via `TransferProofRecorder` itself (`pay_out` fuses transfer + record and fails with `PayoutProofNotRecorded` if the recorder drops the credit, rolling the transfer back), so scheduler-enacted Root calls — invisible to the event-scanning extension — still create leaves; the extension skips pot-touching transfer events and charges no static weight for vesting calls.
- **Calls:** `claim`(0) — **permissionless**; pays the largest valid claim from the pot to the schedule's stored beneficiary (never the caller); the only claim path for keyless/high-security beneficiaries. `create_schedule`(1) — admin; validates the schedule and funds the pot from the treasury in the same call. `end_schedule`(2) — admin; unpaid vested part rounded to the nearest quantum → beneficiary if it meets `MinimumPayout`, otherwise the whole remainder → treasury; schedule removed. `retarget_schedule`(3) — admin; changes the beneficiary and pays nothing out. A retarget replaces the *same* grantee's lost/stolen/abandoned wallet, so settling the old address would burn funds or pay a thief; everything vested but unclaimed stays on the schedule and reaches the new wallet at its next claim. (A permissionless claim landing before the retarget still pays the old address, so rotations should happen promptly.)
- Genesis build validates every schedule (`start ≤ cliff ≤ end`, `start < end`, `total ≥ MinimumPayout`, `total % PayoutQuantum = 0`, beneficiary ≠ pot) and, for a non-empty table, asserts the pot holds exactly `Σ schedule totals + ED`; a misconfigured chain refuses to start. `try_state` validates stored schedules, aligned claims, dust-safe remaining obligations, and—when any schedule exists—`pot balance ≥ Σ(total − claimed) + ED`; an empty schedule table is valid with an unfunded pot.

---

## 4. Runtime APIs (`apis.rs`, `impl_runtime_apis!`)

| API | Methods |
| --- | --- |
| `sp_api::Core` | `version`, `execute_block`, `initialize_block` |
| `sp_api::Metadata` | `metadata`, `metadata_at_version`, `metadata_versions` |
| `sp_block_builder::BlockBuilder` | `apply_extrinsic`, `finalize_block`, `inherent_extrinsics`, `check_inherents` |
| `sp_transaction_pool::TaggedTransactionQueue` | `validate_transaction` |
| `sp_offchain::OffchainWorkerApi` | `offchain_worker` |
| `sp_session::SessionKeys` | `generate_session_keys`, `decode_session_keys` (empty — no session keys) |
| `sp_consensus_qpow::QPoWApi` | `verify_nonce_on_import_block`, `verify_nonce_local_mining`, `get_max_reorg_depth`, `get_difficulty`, `get_last_block_time`, `get_last_block_duration`, `get_chain_height`, `get_max_difficulty`, `verify_and_get_achieved_difficulty` |
| `pallet_zk_tree::ZkTreeApi` | `get_root`, `get_leaf_count`, `get_depth`, `get_merkle_proof` |
| `frame_system_rpc_runtime_api::AccountNonceApi` | `account_nonce` |
| `pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi` | `query_info`, `query_fee_details`, `query_weight_to_fee`, `query_length_to_fee` |
| `pallet_transaction_payment_rpc_runtime_api::TransactionPaymentCallApi` | `query_call_info`, `query_call_fee_details`, `query_weight_to_fee`, `query_length_to_fee` |
| `sp_genesis_builder::GenesisBuilder` | `build_state`, `get_preset`, `preset_names` |
| `frame_benchmarking::Benchmark` | `benchmark_metadata`, `dispatch_benchmark` *(only `runtime-benchmarks`)* |
| `frame_try_runtime::TryRuntime` | `on_runtime_upgrade`, `execute_block` *(only `try-runtime`)* |

---

## 5. Transaction extensions (`TxExtension` in `lib.rs`)

Signed-extension pipeline applied to every extrinsic, in order:

1. `frame_system::CheckNonZeroSender`
2. `frame_system::CheckSpecVersion`
3. `frame_system::CheckTxVersion`
4. `frame_system::CheckGenesis`
5. `frame_system::CheckEra`
6. `frame_system::CheckNonce`
7. `frame_system::CheckWeight`
8. `transaction_extensions::ReversibleTransactionExtension` — **custom**: blocks non-whitelisted calls from high-security accounts, and rejects a high-security signer that has already included 16 extrinsics in the rolling 24h window (`HighSecurityTxQuota`: ring of 16 block numbers, O(1) update).
9. `transaction_extensions::WormholeProofRecorderExtension` — **custom**: in `post_dispatch`, scans emitted native `Balances::Transfer` / `Balances::Minted` / `Balances::TransferOnHold` / `Balances::ReserveRepatriated` events and records transfer proofs into the ZK tree (event-based, covers direct/batch/multisig native transfers). Statically pre-charged calls (`count_transfers`): `Balances` transfers, `Utility::batch_all`, `ReversibleTransfers::{cancel, recover_funds}`, and `Multisig::execute` (walks the stored proposal); uncounted paths are reconciled via `register_extra_weight_unchecked`. Transfers touching the **vesting pot** are skipped: the vesting pallet records its own payouts (covering scheduler-enacted Root calls the extension never sees) and carries that cost in its benchmarked weights. A statically-counted transfer *into* the pot is charged for a leaf insert the scan then skips — an accepted overcharge on a rare bootstrap operation. Per-transfer recording weight is flat, priced at the circuit depth ceiling (`pallet_zk_tree::INSERT_LEAF_*` constants: DB ops, Poseidon path hashing, and per-key PoV). Statically over-charged reservations (failed transfers, short batches, `recover_funds` below the worst case) are returned in `post_dispatch_details`; the extension sits **before** `ChargeTransactionPayment` so the refund reaches the payer's fee, and the trailing `WeightReclaim` returns it to block capacity.
10. `pallet_transaction_payment::ChargeTransactionPayment` — **stock**. The high-security zero-tip policy is not on this extension: it is enforced by `transaction_extensions::HighSecurityFungibleAdapter`, the configured `OnChargeTransaction`, which sees both the signer and the tip on every fee path (`can_withdraw_fee` during mempool and consensus validation, `withdraw_fee` at inclusion) and rejects any non-zero tip from a high-security signer — so no refactor of the extension tuple can silently reopen the tip channel.
11. `frame_metadata_hash_extension::CheckMetadataHash`
12. `frame_system::WeightReclaim` — re-runs the block-weight reclaim so refunds made by earlier extensions (which `CheckWeight`'s own reclaim runs too early to see) are returned to block capacity.

The high-security whitelist (`HighSecurityConfig::is_whitelisted`, extension 8) admits `ReversibleTransfers::{schedule_transfer, cancel, recover_funds}` and a flat `Utility::batch_all` of 1..=`MaxHighSecurityBatchLen` (16, deliberately decoupled from `MaxPendingPerAccount`) of those leaf calls. Nested or empty batches are rejected so a packed wrapper cannot inflate the inclusion fee. `schedule_transfer` is admitted only when `dest` is `MultiAddress::Id` — other `MultiAddress` variants (in particular unbounded `Raw`) are rejected so a stolen key cannot inflate the length fee. `Vesting::claim` is permissionless (any signer, payout always to the stored beneficiary), so a high-security account does not need it on the list. Beyond the shape rules, extension 8 enforces two blanket bounds on every high-security extrinsic: at most `MAX_HIGH_SECURITY_EXTRINSIC_LEN` (10 KiB) encoded bytes, and a zero-tip inclusion fee of at most `MAX_HIGH_SECURITY_INCLUSION_FEE` (`FEE_SCALE` UNIT — scaled in lockstep with the fees it bounds, ~10x the costliest legitimate call) — so a future whitelisted call with an unforeseen length or weight surface cannot reopen the fee-drain channel. Combined with the 16-per-day quota, the worst-case drain from a stolen key is 16 × `FEE_SCALE` UNIT per rolling day. The quota keys on the outer signer, so a single-key high-security guardian shares it with its own traffic and can be locked out of `cancel`/`recover_funds` for up to a day — a documented limitation; the recommended multisig guardian is immune because the derived address never signs extrinsics (its signers do), even when the multisig itself is enrolled as high-security.

---

## 6. Governance definitions (`governance/definitions.rs`)

- `PreimageDeposit` — custom `Consideration` fee model for preimages.
- `CommunityTracksInfo` — public referenda; single "signed" track (max_deciding 5, 500 UNIT decision deposit, 7-day decision, linear-decreasing approval 70→55% / support 25→5%).
- `TechCollectiveTracksInfo` — tech-collective referenda; single track (1000 × `FEE_SCALE` UNIT deposit, 61% approval / 60% support constant curves, 1-day decision/confirm/enactment).
- `MinRankOfClassConverter`, `GlobalMaxMembers` — rank/membership converters.
- `RootOrMemberForTechReferendaOrigin` — custom origin for TechReferenda submission (Root or ranked-collective member).
- `apply_test_timing` — compiled only under `fast-governance` (collapses all timing windows to 2 blocks for CI).

---

## 7. Genesis presets (`genesis_config_presets.rs`)

- **Presets:** `dev` (`DEV_RUNTIME_PRESET`), `heisenberg`, `planck`.
- **Network roles:**
  - `dev` — local development.
  - `heisenberg` — **internal integration testnet**, not mainnet. Tokens have no monetary value; the network may be reset.
  - `planck` — public testnet (live treasury signers + faucet).
- **Vesting genesis:** every preset endows the vesting pot with `Σ schedule totals + ED` (ED alone when the table is empty, as on `planck`). Because the pot is part of the balances genesis endowment, standard genesis proof generation creates a block-1 Wormhole leaf for it; that leaf is unspendable because the pot is keyless. `dev`/`heisenberg` seed example schedules (one account with two schedules; `dev` also vests the keyless test wormhole address, claimable only via third-party ping). A mainnet preset (4-of-6 treasury multisig, launch-gated allocation table) is planned as a separate PR.
- Dilithium well-known accounts: `crystal_alice`, `dilithium_bob`, `crystal_charlie` (public seeds `[0]` / `[1]` / `[2]`). Used by `dev` and **intentionally also by `heisenberg`** so integrators and CI can exercise governance, treasury, and transfer flows without distributing secrets. Those private keys are public by design; do **not** reuse this pattern on a mainnet or any value-bearing chain (Planck already uses distinct live treasury signers).
- Treasury = 2-of-3 multisig of the three signers for `dev`/`heisenberg` (distinct nonce per preset); no dedicated treasury genesis balance (endowments are a separate list).
- Tech-collective seeded via the chain-spec-only `tech_collective_seed_members` JSON field (`prepare_genesis_build_input` + `seed_tech_collective`).
- Endows all genesis balances with wormhole transfer proofs (ZK-spendable). `dev` also endows `TEST_WORMHOLE_SECRET`'s address.

---

## 8. In-tree FRAME core (`frame/`)

All FRAME runtime glue compiled into the WASM is now vendored in-tree (copied from
polkadot-sdk, with `[patch.crates-io]` ensuring transitive resolution):

| Crate | Path | Role in runtime |
| --- | --- | --- |
| `frame-support-procedural-tools-derive` | `frame/support-procedural-tools-derive` | Proc-macro helper for parsing struct fields. |
| `frame-support-procedural-tools` | `frame/support-procedural-tools` | Proc-macro utilities shared by `frame-support-procedural`. |
| `frame-support-procedural` | `frame/support-procedural` | `#[pallet::…]` and `#[frame_support::runtime]` proc macros. |
| `frame-support` `45.1.0` | `frame/support` | Storage, dispatch, origins, pallet traits, runtime composition. |
| `frame-metadata` `23.0.1` | `frame/metadata` | Metadata type definitions consumed by `frame-support`. |
| `frame-executive` `45.0.1` | `frame/executive` | Block execution engine (`Executive`). |
| `frame-metadata-hash-extension` `0.13.0` | `frame/metadata-hash-extension` | `CheckMetadataHash` signed extension. |
| `frame-system-rpc-runtime-api` `40.0.0` | `frame/system-rpc-runtime-api` | `AccountNonceApi` runtime API. |
| `frame-try-runtime` `0.51.0` | `frame/try-runtime` | Try-runtime helpers *(only `try-runtime` feature)*. |
| `frame-benchmarking` `45.0.3` | `frame/benchmarking` | Benchmark harness *(only `runtime-benchmarks` feature)*. |
| `frame-system-benchmarking` `45.0.0` | `frame/system-benchmarking` | System pallet benchmarks *(only `runtime-benchmarks` feature)*. |

Related transaction-payment RPC surface (patched for WASM + node builds):

| Crate | Path | In WASM? |
| --- | --- | --- |
| `pallet-transaction-payment-rpc-runtime-api` `45.0.0` | `pallets/transaction-payment-rpc-runtime-api` | **yes** — runtime API declarations |
| `pallet-transaction-payment-rpc` `48.0.0` | `pallets/transaction-payment-rpc` | no — node RPC only; patched so the family stays in-tree |

---

## 9. Workspace primitive crates compiled into the runtime

| Crate | Path | Role in runtime |
| --- | --- | --- |
| `qp-dilithium-crypto` | `primitives/dilithium-crypto` | ML-DSA-87/ML-DSA-65 post-quantum signatures; `DilithiumSignatureScheme` = the chain's `Signature`/`AccountId`. |
| `qp-header` | `primitives/header` | Custom block `Header` (Poseidon block hash + Blake2 state trie); `ZkTreeRootProvider` trait. |
| `qp-high-security` | `primitives/high-security` | `HighSecurityInspector` trait shared by multisig, reversible-transfers, tx-extensions (breaks circular dep). |
| `qp-scheduler` | `primitives/scheduler` | `BlockNumberOrTimestamp`, `DispatchTime`, `ScheduleNamed` trait for delayed dispatch. |
| `qp-wormhole` | `primitives/wormhole` | `TransferProofRecorder` trait, wormhole address derivation, author extraction. |
| `sp-consensus-qpow` | `primitives/consensus/qpow` | `QPoWApi` runtime API declaration, `POW_ENGINE_ID`, `Seal`. |
| `qpow-math` | `qpow-math` | Poseidon2 PoW nonce hashing & difficulty/target math used by `pallet-qpow`. |

External Quantus crates (crates.io, used by wormhole/zk-tree): `qp-plonky2`,
`qp-poseidon-core`, `qp-rusty-crystals-dilithium`, `qp-wormhole-*` (aggregator,
circuit, circuit-builder, inputs, prover, verifier), `qp-zk-circuits-common`.

**Still external (crates.io):** the `sp-*` Substrate primitives (`sp-api`,
`sp-runtime`, `sp-core`, `sp-io`, `sp-state-machine`, `sp-trie`, …), plus codec
layer crates (`parity-scale-codec`, `scale-info`, `primitive-types`,
`binary-merkle-tree`, `bounded-collections`). See `runtime/Cargo.toml` / root
`Cargo.toml` for exact versions.

> All runtime pallets, FRAME core crates, and the transaction-payment family are
> **in-tree** via workspace path deps and `[patch.crates-io]`. Client-only patches
> (`sc-cli`, `sc-network*`, `sc-informant`, `litep2p`) are not compiled into
> the runtime WASM.

---

## 10. Cargo features affecting the compiled runtime (`runtime/Cargo.toml`)

| Feature | Effect on compiled runtime |
| --- | --- |
| `default = ["std"]` | Native build; enables `std` across all deps + `substrate-wasm-builder`. WASM build is `no_std`. |
| `runtime-benchmarks` | Compiles `benchmarks.rs`, benchmark `Config` impls, and `Benchmark` API; adds benchmark-only genesis (reversible-transfers HS account). |
| `try-runtime` | Compiles `TryRuntime` API and migration checks. |
| `metadata-hash` | Enables `CheckMetadataHash` metadata generation (double WASM compile). |
| `fast-governance` | **Test/CI only.** Collapses every referenda timing window to 2 blocks (`apply_test_timing`). Must be OFF for production. |
| `on-chain-release-build` | `metadata-hash` + `sp-api/disable-logging` for release WASM. |

---

## 11. Lifecycle hooks (per-block execution surface)

| Pallet | Hooks implemented |
| --- | --- |
| `frame-system` (local) | `integrity_test` |
| `QPoW` | `on_initialize`, `on_finalize` (difficulty + block timing) |
| `MiningRewards` | `integrity_test`, `on_initialize`, `on_finalize` (block reward mint + fee distribution) |
| `Wormhole` | `on_initialize` (genesis proof emission at block 1) |
| `Scheduler` (local) | `on_initialize` (executes due agenda items) |
| `ZkTree` | `on_finalize` (commit merkle root) |
| `ReversibleTransfers` | `integrity_test` |

Plus the upstream pallets' own hooks, all driven through
`Executive` over `AllPalletsWithSystem`.
