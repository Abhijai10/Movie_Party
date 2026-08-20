# Milestone 3 Worklog — Local Perfect E2E

## Status: 🟩 LOCALLY COMPLETE (external macOS playback verification pending)

---

## Architecture Implemented

### Demand-Driven Range Fetch (M3.1)
- `ChunkDemandHandle` (`src-tauri/src/media/transfer/mod.rs`): shared bounded
  demand channel between the loopback range server and the guest QUIC
  transfer worker.
  - Priority-ordered queue (`BinaryHeap`): Critical outranks background.
  - In-flight set prevents duplicate simultaneous fetch of the same chunk.
  - Tokio `Notify` with stored-permit semantics — no lost wakeups, no busy loop.
- Range server (`range_server.rs`) now computes the exact missing 1 MiB chunk
  indices for every uncached HTTP Range request and enqueues them as CRITICAL
  demand before waiting (configurable bounded `wait_timeout_ms`, defaults to 5s
  in the production path, then serves whatever is available).
- `guest_fetch_media()` (AppRuntime) runs a demand-driven transfer worker:
  pop → QUIC binary fetch → BLAKE3/len/index/media_id validation → write to
  SparseCache → wake range server via watch channel.
- Tests: uncached range triggers real QUIC fetch + serves exact source bytes;
  distant seek reprioritizes over background; duplicate demands deduplicate.

### Binary QUIC Bulk Transfer (M3.2)
- `ServerResponse::ChunkResponse` (base64 JSON payload) is GONE.
- Host `ChunkRequest` handler now writes the PROTOCOL_SPEC §45 binary
  chunk-stream (`MPCK` magic, version, media id, uint32 index, uint32 payload
  length, 32-byte BLAKE3, raw payload) directly on the dedicated QUIC stream.
- Guest `fetch_local_media_chunk()` reads the leading 4 bytes: MPCK magic →
  binary decode; otherwise JSON error response (ChunkUnavailable/MediaError).
- Control JSON stays small; `MAX_REQUEST_BYTES` unchanged (2 MiB).
- Tests: valid round-trip, truncated frame, oversized claimed payload, wrong
  media/index, hash corruption, raw-binary-not-base64 assertion.

### Lifecycle (M3.3)
- `guest_fetch_media()` owns manifest, sparse cache, range server, transfer
  worker, player, and player event loop.
- `leave_party()` aborts transfer worker, player event loop, transfer stall
  watcher, stops the range server (`shutdown()` now synchronous), releases
  cache and client.
- Reconnect: `guest_fetch_media()` aborts any prior transfer worker and stops
  any prior range server before installing the new session; the old task is
  retained as `old_transfer_task` so tests can confirm it terminated.
- Tests: session ownership + release; reconnect keeps exactly one live worker.

### Strict Sync Authority (M3.4)
- No changes needed to M2: PROTOCOL_SPEC §30 behavior already proven at the
  coordinator level. Added an AppRuntime E2E proof:
  guest cannot provide bytes → BUFFERING + strict_sync_paused on BOTH sides →
  refill → readiness consensus → fresh host play cycle → synchronized PLAYING.
  No independent Local Perfect auto-resume.

### Player / Presentation (M3.5)
- libmpv backend remains behind `#[cfg(feature = "mpv")]`.
- Known limitation (documented, NOT renamed as V2): libmpv opens its own
  unmanaged Cocoa window. In-app embedding of the mpv surface requires a
  platform-native view-hosting layer (NSView/CALayer hosting) that the Tauri 2
  webview API does not expose; this is the locked-architecture gap, recorded
  as a platform blocker, not silently renamed.
- External verification pending: run .app → pick media → visible playback.

---

## Tests (M3 closure added 13)

Component (transfer/mod.rs):
- validates_and_round_trips_chunk_stream
- rejects_truncated_frame
- rejects_oversized_claimed_payload
- rejects_wrong_media_id_and_index
- rejects_corrupt_chunk_hash
- binary_stream_is_raw_not_base64_json
- demand_pops_highest_priority_first
- demand_deduplicates_concurrent_requests
- demand_in_flight_chunk_is_not_popped_twice
- demand_wakes_waiter_on_new_request

Range server (range_server.rs):
- range_server_enqueues_demand_for_missing_chunks

Integration (tests/m3_closure.rs):
- uncached_http_range_triggers_real_quic_fetch_and_serves
- distant_seek_repioritizes_demand_before_background
- duplicate_range_demands_deduplicate
- guest_fetch_media_owns_session_and_leave_releases_everything
- reconnect_reuses_single_session_worker
- guest_starvation_pauses_host_and_guest_with_consensus_resume

Existing suites (m3_integration, local_perfect loopback E2E, m3_m4_e2e) all
exercise the new binary chunk-stream path.

---

## Files

- `src-tauri/src/media/transfer/mod.rs` — ChunkDemandHandle + binary stream tests
- `src-tauri/src/media/stream/range_server.rs` — demand-driven serving
- `src-tauri/src/network/quic.rs` — binary chunk stream transport
- `src-tauri/src/app_runtime.rs` — demand worker, lifecycle ownership
- `src-tauri/tests/m3_closure.rs` — new integration tests

---

## External Verification Needed

1. Run .app bundle → pick .mp4 → verify visible video + audio in mpv window
2. Verify mpv Cocoa window appears alongside Move Party cinema
3. In-app mpv surface embedding — platform blocker (Tauri 2 cannot host a
   native mpv view in the webview without a native plugin).
