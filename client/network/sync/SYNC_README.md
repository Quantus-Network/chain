### Sync package overview (short)

This is a concise map of how sync works and where to look in the code.

## Entry points
- `service/syncing_service.rs` (handle): Async API used by consensus/RPC to query status and send commands. Forwards to the engine.
- `engine.rs` (SyncingEngine): Orchestrator. Owns peers, polls network events, runs the loop, executes strategy actions, and routes request/response results.
- `strategy/` (policies): Pluggable sync logic.
  - `strategy/polkadot.rs` (PolkadotSyncingStrategy): Wraps/dispatches to specific strategies.
  - `strategy/chain_sync.rs` (ChainSync): Main block sync (gaps, forks, justifications). Uses `BlockCollection` for planning.
  - `strategy/warp.rs`, `strategy/state.rs`, `strategy/state_sync.rs`: Warp and state sync.
- Request handlers (serve inbound): `block_request_handler.rs`, `state_request_handler.rs`, `warp_request_handler.rs`.
- Block queue/coordination: `blocks.rs` (`BlockCollection`).

## High-level flow
1) Network connects peers → engine validates handshake → registers peer → notifies active strategy via `add_peer`.
2) Strategy computes actions on ticks/events via `actions(&network_service)`:
   - `StartRequest`, `CancelRequest`, `DropPeer`, `ImportBlocks`, `ImportJustifications`, `Finished`.
3) Engine executes actions and tracks in-flight via `PendingResponses` keyed by `(PeerId, StrategyKey)`.
4) Successful responses are routed back to strategy via `on_generic_response(..)`; errors (timeouts etc.) handled in engine.
5) Blocks import via the import queue; engine reports results to strategy with `on_blocks_processed(..)`.

## Where key state lives
- Engine `peers: HashMap<PeerId, Peer>` → roles, best hash/number, small `known_blocks` LRU for re‑announce.
- Strategy (ChainSync): per‑peer sync state (`PeerSync`), request planning, and block scheduling.
- `BlockCollection`:
  - `needed_blocks(..)` picks the next range, limits `max_parallel_downloads`, respects `peer_best` and a `max_ahead` window.
  - `peer_requests` tracks in‑flight ranges per peer; `ready_blocks(..)` yields contiguous, importable blocks.

## Dedupe and limits
- Per‑(PeerId, StrategyKey) in‑flight dedupe via `PendingResponses` (obsoletes replaced when `remove_obsolete = true`).
- Per‑block/queue dedupe via `queue_blocks` in ChainSync.
- Per‑range coordination and parallel limits via `BlockCollection`.

## Major syncing and peer failures (current behavior)
- Major sync signal: `strategy.is_major_syncing()`; engine updates an atomic gauge each loop.
- Network‑level failures (timeouts, refused, connection closed, etc.) are handled in engine:
  - During major sync, peer drops are gated by a threshold obtained from the active strategy.
  - Outside major sync, engine drops fast (legacy behavior).
  - On success, engine decrements the peer’s failure counter toward 0.

## CLI flags (runtime tunables)
Set at startup and applied immediately after network build:
- `--sync-max-timeouts-before-drop <u32>`: threshold for failures during major sync.
- `--sync-disable-major-sync-gating` (bool): fast‑ban even during major sync when true.

These are stored and exposed by `PolkadotSyncingStrategy`:
- Accessors used by engine: `peer_drop_threshold()`, `relaxed_peer_drop_while_syncing()`.
- Setters used by service: `set_peer_drop_threshold(..)`, `set_relaxed_peer_drop_while_syncing(..)`.

## Pointers for debugging
- Engine timeout handling: logs `is_major_syncing`, computed `should_gate`, and effective `threshold` before deciding to drop.
- ChainSync range logging: `on_block_data` prints received ranges only when debug is enabled; request‑based estimate is used if the response lacks numbers.
- `blocks.rs::needed_blocks(..)`: trace logs explain why a candidate was accepted or rejected (out‑of‑range, too far ahead, etc.).

## Minimal glossary
- StrategyKey: routes responses to the right strategy.
- BlockAttributes/Data/Request/Response: request/response types for block relay.
- BadPeer/ReputationChange: reputation impacts used when dropping peers.


