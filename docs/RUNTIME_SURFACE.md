# Quantus Runtime Surface

Complete inventory of every piece of code that gets compiled into the on-chain
runtime WASM (`quantus-runtime`). This is the authoritative map of the runtime's
attack/audit surface: the runtime crate's own modules, the pallets composed into
the runtime, their dispatchable calls, the runtime APIs, transaction extensions,
genesis logic, and the workspace primitive crates pulled in.

- **Crate:** `quantus-runtime` (`runtime/`), version `0.7.1-q-day-2`
- **Spec:** `spec_name = quantus-runtime`, `spec_version = 131`, `transaction_version = 2`, `authoring_version = 1`
- **Build:** `no_std` WASM via `substrate-wasm-builder` (`runtime/build.rs`); native `std` build for the node/client
- **Block time target:** 12s (`TARGET_BLOCK_TIME_MS = 12_000`)
- **Consensus:** QPoW (quantum-resistant Proof of Work, Poseidon2-based)
- **Signatures:** Dilithium (ML-DSA-87) post-quantum signature scheme
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
| 10 | `Referenda` | `pallet-referenda` `45.0.0` | **Inlined** (`pallets/referenda`) | yes |
| 11 | `ReversibleTransfers` | `pallet-reversible-transfers` | **Local** (`pallets/reversible-transfers`) | yes |
| 12 | `ConvictionVoting` | `pallet-conviction-voting` `45.0.0` | **Inlined** (`pallets/conviction-voting`) | yes |
| 13 | `TechCollective` | `pallet-ranked-collective` `45.0.0` | **Inlined** (`pallets/ranked-collective`) | yes |
| 14 | `TechReferenda` | `pallet-referenda::Pallet<Runtime, Instance1>` `45.0.0` | **Inlined** (2nd instance) | yes |
| 15 | `TreasuryPallet` | `pallet-treasury` | **Local** (`pallets/treasury`) | yes |
| 16 | `Recovery` | `pallet-recovery` `45.0.0` | **Inlined** (`pallets/recovery`) | yes |
| 17 | `Assets` | `pallet-assets` `48.1.0` | **Inlined** (`pallets/assets`) | yes |
| 18 | `AssetsHolder` | `pallet-assets-holder` `0.8.0` | **Inlined** (`pallets/assets-holder`) | (no extrinsics) |
| 19 | `Multisig` | `pallet-multisig` | **Local** (`pallets/multisig`) | yes |
| 20 | `Wormhole` | `pallet-wormhole` | **Local** (`pallets/wormhole`) | yes |
| 21 | `ZkTree` | `pallet-zk-tree` | **Local** (`pallets/zk-tree`) | no |

> Index 4 is intentionally left vacant after `pallet-sudo` removal so downstream indices stay stable.

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
- `WeightToFee = IdentityFee<Balance>` (1s compute ≈ 1 UNIT).
- `LengthToFee = LengthToFeeMultiplier` (custom, `LENGTH_FEE_MULTIPLIER = 10^6`; 1 MB ≈ 1 UNIT).
- `FeeMultiplierUpdate = ConstFeeMultiplier` (multiplier fixed at 1), `OperationalFeeMultiplier = 5`.

### Index 5 — `QPoW` (`pallet-qpow`, local)
- `InitialDifficulty = U512([4_000_000, 0, …])`, `TargetBlockTime = 12_000ms`, `MaxReorgDepth = 180`, `WeightInfo = ()`.
- No dispatchable calls. Implements `Hooks` (`on_initialize`/`on_finalize`) to track block timing and recompute difficulty. Powers the `QPoWApi` runtime API.

### Index 6 — `MiningRewards` (`pallet-mining-rewards`, local)
- `Currency = Balances`, `ProofRecorder = Wormhole`, `MaxSupply = 21_000_000 * UNIT`, `EmissionDivisor = 26_280_000`, `Treasury = pallet_treasury::Pallet`, `MintingAccount`, `Unit = UNIT`.
- No dispatchable calls. Exposes `TransactionFeesCollector` + `collect_transaction_fees`. `on_finalize` mints block reward (70% miner / 30% treasury split per fee-structure docs).

### Index 7 — `Preimage` (`pallet-preimage`)
- `ManagerOrigin = EnsureRoot`, `Consideration = PreimageDeposit` (custom: 0.1 UNIT base + 0.0001 UNIT/byte, see `governance/definitions.rs`).
- **Calls:** `note_preimage`, `unnote_preimage`, `request_preimage`, `unrequest_preimage`, `ensure_updated` (upstream).

### Index 8 — `Scheduler` (`pallet-scheduler`, local) — **calls disabled**
- `RuntimeCall`, `MaximumWeight = 80% max block`, `MaxScheduledPerBlock = 50`, `ScheduleOrigin = EnsureRoot`, `Preimages = Preimage`, `TimeProvider = Timestamp`, `Moment = u64`, `TimestampBucketSize = 2 * block time`.
- Calls exist (`schedule`(0), `cancel`(1), `schedule_named`(2), `cancel_named`(3), `schedule_after`(4), `schedule_named_after`(5), `set_retry`(6), `set_retry_named`(7), `cancel_retry`(8), `cancel_retry_named`(9)) but are **disabled at the runtime level** so users cannot enqueue arbitrary calls. Used internally by reversible-transfers and governance via the `ScheduleNamed` trait. Local fork adds block-number-or-timestamp scheduling.

### Index 9 — `Utility` (`pallet-utility`)
- `RuntimeCall`, `PalletsOrigin = OriginCaller`.
- **Calls:** `batch`, `as_derivative`, `batch_all`, `dispatch_as`, `force_batch`, `with_weight`.

### Index 10 — `Referenda` (`pallet-referenda`, community instance)
- `Tracks = CommunityTracksInfo` (single "signed" track), `Tally = pallet_conviction_voting::Tally<Balance, DynamicMaxTurnout>`, `SubmitOrigin = EnsureSigned`, `Cancel/KillOrigin = EnsureRoot`, `SubmissionDeposit = 100 UNIT`, `UndecidingTimeout = 45 DAYS`, `Preimages = Preimage`.
- **Calls:** `submit`, `place_decision_deposit`, `refund_decision_deposit`, `cancel`, `kill`, `nudge_referendum`, `one_fewer_deciding`, `refund_submission_deposit`, `set_metadata`.

### Index 11 — `ReversibleTransfers` (`pallet-reversible-transfers`, local)
- `Scheduler = Scheduler`, `DefaultDelay = 1 DAY`, `MinDelayPeriodBlocks = 2`, `MaxGuardianAccounts = 32`, `MaxPendingPerAccount = 16`, `VolumeFee = 1%` (high-security reversals, burned), `ProofRecorder = Wormhole`, `PalletId = "rtpallet"`.
- **Calls:** `set_high_security`(0), `cancel`(1), `execute_transfer`(2), `schedule_transfer`(3), `schedule_transfer_with_delay`(4), `schedule_asset_transfer`(5), `schedule_asset_transfer_with_delay`(6), `recover_funds`(7).
- Backs `HighSecurityConfig` (account whitelist/guardian logic).

### Index 12 — `ConvictionVoting` (`pallet-conviction-voting`)
- `Currency = Balances`, `Polls = Referenda`, `MaxTurnout = DynamicMaxTurnout` (scales with total issuance), `VoteLockingPeriod = 7 DAYS`, `MaxVotes = 4096`.
- **Calls:** `vote`, `delegate`, `undelegate`, `unlock`, `remove_vote`, `remove_other_vote`.

### Index 13 — `TechCollective` (`pallet-ranked-collective`)
- `AddOrigin/RemoveOrigin = EnsureRootWithSuccess<AccountId, ConstU16<0>>` (Root-only, i.e. a passed TechReferenda vote; #91267), `Promote/Demote/ExchangeOrigin = NeverEnsureOrigin`, `Polls = TechReferenda (Instance1)`, `VoteWeight = Linear`, `MaxMemberCount = 13` (via `GlobalMaxMembers`).
- **Calls:** `add_member`, `promote_member`, `demote_member`, `remove_member`, `vote`, `cleanup_poll`, `exchange_member`.

### Index 14 — `TechReferenda` (`pallet-referenda`, `Instance1`)
- `SubmitOrigin = RootOrMemberForTechReferendaOrigin`, `Tracks = TechCollectiveTracksInfo` (single track, 61% approval / 60% support constant curves), `Tally = pallet_ranked_collective::TallyOf<Runtime>`.
- **Calls:** same set as `Referenda` (separate instance/storage).

### Index 15 — `TreasuryPallet` (`pallet-treasury`, local)
- Minimal local treasury. Config only sets `WeightInfo`.
- **Calls:** `set_treasury_account`(0, root), `set_treasury_portion`(1, root). Exposes `account_id()` / `portion()`.

### Index 16 — `Recovery` (`pallet-recovery`)
- `ConfigDepositBase = 10 UNIT`, `FriendDepositFactor = 1 UNIT`, `MaxFriends = 9`, `RecoveryDeposit = 10 UNIT`.
- **Calls:** `as_recovered`, `set_recovered`, `create_recovery`, `initiate_recovery`, `vouch_recovery`, `claim_recovery`, `close_recovery`, `remove_recovery`, `cancel_recovered`.

### Index 17 — `Assets` (`pallet-assets`)
- `AssetId = u32`, `CreateOrigin = AsEnsureOriginWithArg<EnsureSigned>`, `ForceOrigin = EnsureRoot`, `AssetDeposit/AccountDeposit/Metadata = MILLI_UNIT`, `StringLimit = 50`, `CallbackHandle = AutoIncAssetId`, `Holder = AssetsHolder`, `RemoveItemsLimit = 1000`.
- **Calls:** full `pallet-assets` surface (`create`, `force_create`, `mint`, `burn`, `transfer`, `transfer_keep_alive`, `force_transfer`, `freeze`/`thaw`, `set_metadata`, `approve_transfer`, `transfer_approved`, etc.).

### Index 18 — `AssetsHolder` (`pallet-assets-holder`)
- `RuntimeEvent`, `RuntimeHoldReason`. No standalone extrinsics; provides hold support to `Assets`.

### Index 19 — `Multisig` (`pallet-multisig`, local)
- `MaxSigners = 100`, `MaxTotalProposalsInStorage = 200`, `MaxCallSize = 10 KB`, `MultisigFee = 0.6 UNIT` (burned), `ProposalDeposit = 1 UNIT`, `ProposalFee = 1 UNIT`, `MaxExpiryDuration ≈ 2 weeks`, `MaxInnerCallWeight = (10^12, 2.5 MB)`, `HighSecurity = HighSecurityConfig`, `PalletId = "py/mltsg"`.
- **Calls:** `create_multisig`(0), `propose`(1), `approve`(2), `cancel`(3), `remove_expired`(4), `claim_deposits`(5), `execute`(6). Exposes `derive_multisig_address`.

### Index 20 — `Wormhole` (`pallet-wormhole`, local)
- `Currency = Balances`, `Assets = Assets`, `MinimumTransferAmount = 0.1 UNIT`, `VolumeFeeRateBps = 10` (0.1%), `VolumeFeesBurnRate = 50%`, `MintingAccount`, `WormholeAccountId = AccountId32`, `ZkTree = ZkTree`.
- **Calls:** `verify_aggregated_proof`(2) — verifies an aggregated ZK proof and processes batched transfers.
- Implements `TransferProofRecorder` (`record_transfer`) consumed by mining-rewards, reversible-transfers, and the wormhole tx-extension. `on_initialize` emits genesis endowment proofs at block 1. Loads a static aggregated verifier (`get_aggregated_verifier`).

### Index 21 — `ZkTree` (`pallet-zk-tree`, local)
- `AssetId = u32`, `Balance = u128`. No dispatchable calls.
- **Storage:** `Leaves`, `Nodes`, `LeafCount`, `Depth`, `Root`. Types `ZkLeaf`, `ZkMerkleProof`, `ZkMerkleProofRpc`, `Hash256`.
- `on_finalize` commits the merkle root. Backs the `ZkTreeApi` runtime API.

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
8. `pallet_transaction_payment::ChargeTransactionPayment`
9. `frame_metadata_hash_extension::CheckMetadataHash`
10. `transaction_extensions::ReversibleTransactionExtension` — **custom**: blocks non-whitelisted calls from high-security accounts.
11. `transaction_extensions::WormholeProofRecorderExtension` — **custom**: in `post_dispatch`, scans emitted `Transfer`/`Transferred`/`Minted`/`Issued` events and records transfer proofs into the ZK tree (event-based, covers direct/batch/multisig/recovery/scheduled transfers).

---

## 6. Governance definitions (`governance/definitions.rs`)

- `PreimageDeposit` — custom `Consideration` fee model for preimages.
- `CommunityTracksInfo` — public referenda; single "signed" track (max_deciding 5, 500 UNIT decision deposit, 7-day decision, linear-decreasing approval 70→55% / support 25→5%).
- `TechCollectiveTracksInfo` — tech-collective referenda; single track (1000 UNIT deposit, 61% approval / 60% support constant curves, 1-day decision/confirm/enactment).
- `MinRankOfClassConverter`, `GlobalMaxMembers` — rank/membership converters.
- `RootOrMemberForTechReferendaOrigin` — custom origin for TechReferenda submission (Root or ranked-collective member).
- `apply_test_timing` — compiled only under `fast-governance` (collapses all timing windows to 2 blocks for CI).

---

## 7. Genesis presets (`genesis_config_presets.rs`)

- **Presets:** `dev` (`DEV_RUNTIME_PRESET`), `heisenberg`, `planck`.
- Dilithium dev accounts: `crystal_alice`, `dilithium_bob`, `crystal_charlie`.
- Treasury = 2-of-3 multisig of the three signers (distinct nonce per preset); receives 30% of the 21M genesis supply.
- Tech-collective seeded via the chain-spec-only `tech_collective_seed_members` JSON field (`prepare_genesis_build_input` + `seed_tech_collective`).
- Reserves asset id 0 for the native-token-in-assets wormhole path.
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
| `qp-dilithium-crypto` | `primitives/dilithium-crypto` | ML-DSA-87 post-quantum signatures; `DilithiumSignatureScheme` = the chain's `Signature`/`AccountId`. |
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
