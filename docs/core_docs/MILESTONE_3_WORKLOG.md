# Milestone 3 Worklog — Local Perfect E2E

## Status: 🟨 CORE TRANSPORT GREEN — PRESENTATION PARTIAL

- Core Local Perfect transport (demand-driven range fetch, binary QUIC
  transfer, lifecycle, strict sync) is green with tests.
- Presentation is PARTIAL: libmpv opens its own unmanaged Cocoa window.
  In-app embedding is a dedicated native-rendering milestone (NSView/CALayer
  hosting via a Tauri native plugin), NOT an external hardware blocker. See
  "Presentation" below.

---

## Architecture Implemented

### Demand-Driven Range Fetch (M3.1)
- `ChunkDemandHandle` (`src-tauri/src/media/transfer/mod.rs`): shared bounded
  demand channel between the loopback range server and the guest QUIC
  transfer worker.
  - Priority-ordered queue (`BinaryHeap`): Critical outranks background.
  - In-flight set prevents duplicate simultaneous fetch of the same chunk.
  - Tokio `Notify` with stored-permit semantics — no lost wakeups, no busy loop.
- Range server (`range_server.rs`) computes the exact missing 1 MiB chunk
  indices for every uncached HTTP Range request and enqueues them as CRITICAL
  demand before waiting (configurable bounded `wait_timeout_ms`, 5s default).
- `guest_fetch_media()` (AppRuntime) runs a demand-driven transfer worker with
  correct cache-write discipline:
  - QUIC success → validate → **acquire the cache with `lock().await`, never
    `try_lock()`** → write chunk → ONLY then update transfer progress → notify
    range waiter → finish demand.
  - If the cache write fails, the chunk is requeued (never silently lost,
    never counted in `bytes_available`).
  - Contention test proves the worker persists the chunk even when the cache
    lock is held during the write attempt.
- Tests: uncached range triggers real QUIC fetch + serves exact source bytes;
  distant seek reprioritizes over background; duplicate demands deduplicate;
  cache-write contention eventually persists.

### Range Wake Path (M3.2)
- The dead `watch`-channel wake was replaced with a real bounded condition
  variable: `ChunkWake` (`range_server.rs`).
  - Transfer worker bumps a generation counter and `notify_all`s after each
    chunk write.
  - Waiting range requests sleep on the Condvar via `wait_timeout_while`
    (lost-wakeup safe; a notify that arrives before the wait returns
    immediately).
  - No 20 ms polling. `notify_chunk_written()` is wired to the same wake.

### Empty-Timeout Response (M3.3)
- `wait_for_range` returns `RangeServe::{Available,Partial,Unavailable}`.
- `Available`/`Partial` → 206 with a correctly computed Content-Range (the
  `end = start + len - 1` underflow is impossible because an empty body never
  reaches that arithmetic).
- `Unavailable` → `503 Service Unavailable` with `Retry-After: 1` — never a
  bogus empty 206.
- Test: no-data range returns 503; wake test proves a genuine notify serves
  data well before the timeout.

### Binary QUIC Bulk Transfer (M3.4)
- `ServerResponse::ChunkResponse` (base64 JSON payload) is GONE.
- Host `ChunkRequest` handler writes the PROTOCOL_SPEC §45 binary chunk-stream
  (`MPCK` magic, version, media id, uint32 index, uint32 payload length,
  32-byte BLAKE3, raw payload) on a dedicated QUIC stream.
- Guest `fetch_local_media_chunk()` reads the leading 4 bytes: MPCK magic →
  binary decode; otherwise JSON error. Control JSON stays small;
  `MAX_REQUEST_BYTES` unchanged.
- Tests: valid round-trip, truncated frame, oversized claimed payload, wrong
  media/index, hash corruption, raw-binary-not-base64.

### Lifecycle (M3.5)
- `guest_fetch_media()` owns manifest, sparse cache, range server, transfer
  worker, player, and player event loop.
- `leave_party()` aborts transfer worker, player event loop, transfer stall
  watcher; stops the range server; releases cache and client.
- Reconnect aborts the prior worker (`old_transfer_task`) before installing a
  new one — never duplicate live workers.
- Tests: ownership + release; reconnect keeps exactly one live worker.

### Strict Sync Authority (M3.6)
- PROTOCOL_SPEC §30 behavior proven at AppRuntime level: guest starvation
  pauses BOTH sides; refill requires readiness consensus + a fresh host play
  cycle; no independent Local Perfect auto-resume.

### Presentation (M3.7) — PARTIAL
- libmpv still opens an unmanaged separate Cocoa window (mpv defaults, no
  `vo=null`).
- In-app embedding requires a dedicated native-rendering milestone: a Tauri
  native plugin that hosts the mpv surface inside the app window (NSView
  hierarchy, `wid` property, lifecycle/resize handling, IPC bridge). Tauri 2's
  public API does not expose webview NSView hosting.
- This is a locally-codable milestone, not an external hardware blocker, but
  it is substantial enough that it is tracked separately rather than
  half-implemented inside this pass. Core transport is green; presentation is
  explicitly PARTIAL.

---

## Tests (M3 closure added 15)

Component (transfer/mod.rs):
- validates_and_round_trips_chunk_stream, rejects_truncated_frame,
  rejects_oversized_claimed_payload, rejects_wrong_media_id_and_index,
  rejects_corrupt_chunk_hash, binary_stream_is_raw_not_base64_json,
  demand_pops_highest_priority_first, demand_deduplicates_concurrent_requests,
  demand_in_flight_chunk_is_not_popped_twice, demand_wakes_waiter_on_new_request

Range server (range_server.rs):
- range_server_enqueues_demand_for_missing_chunks
- range_server_returns_503_when_no_data_after_timeout
- range_server_wake_notifies_waiter_immediately

Integration (tests/m3_closure.rs):
- uncached_http_range_triggers_real_quic_fetch_and_serves
- cache_write_contention_eventually_persists_chunk
- distant_seek_repioritizes_demand_before_background
- duplicate_range_demands_deduplicate
- guest_fetch_media_owns_session_and_leave_releases_everything
- reconnect_reuses_single_session_worker
- guest_starvation_pauses_host_and_guest_with_consensus_resume

---

## Files

- `src-tauri/src/media/transfer/mod.rs` — ChunkDemandHandle + binary stream
- `src-tauri/src/media/stream/range_server.rs` — ChunkWake (Condvar), demand,
  RangeServe (503 on empty timeout)
- `src-tauri/src/network/quic.rs` — binary chunk stream transport
- `src-tauri/src/app_runtime.rs` — demand worker with correct cache-write
  discipline, lifecycle ownership
- `src-tauri/tests/m3_closure.rs` — integration tests

---

## External Verification Needed

1. Run .app bundle → pick media → visible playback in the mpv window
2. In-app mpv surface embedding — dedicated native-rendering milestone
   (locally-codable, tracked separately, currently PARTIAL)
