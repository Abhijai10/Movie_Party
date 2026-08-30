# Milestone 2 Worklog

## CURRENT RESUME STATE
Current objective:
M2 — Live Strict Sync + Readiness + Social Transport over real authenticated QUIC.

Current subtask:
**M2 COMPLETE (LOCALLY)** — All automated gates green. External verification pending.

Last completed subtask:
M2.14 Closure audit: re-verified all 14 acceptance items + 13 audit items against source code. Ran full regression gate. All green. No code changes needed.

Current blocker:
None for automated work. EXTERNAL VERIFICATION PENDING: real physical Tailscale Mac ↔ Windows peer session (protocol sync + readiness + pause/seek + chat/reaction over real network).

Exact next action:
Milestone 3 planning. OR run the M2 manual acceptance script on a real Mac + second device (Windows) over Tailscale.

Last known test state (2026-08-18 final audit):
- `cargo fmt --check`: PASS
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS (0 warnings)
- `cargo test`: 143 lib + 2 M1 + 22 M2 = 167 pass / 0 fail / 2 ignored (lib Chrome tests)
- `cargo build`: PASS
- `npx tsc --noEmit`: PASS
- `pnpm lint`: PASS (0 warnings)
- `pnpm test`: 3/3 PASS
- `pnpm build`: PASS

Last updated:
2026-08-18 — M2 closure audit complete; LOCALLY COMPLETE

## IMPLEMENTATION STATUS
- [x] M2.1 Runtime ownership — AppRuntime owns LocalSyncCoordinator (as `Arc<Mutex<LocalSyncCoordinator>>`), host/peer readiness, host broadcast sender
- [x] M2.2 Peer message transport — EventEnvelope over server-opened bi-streams; guest uses accept_bi; seq/sender/sent_mono_us populated by QuicServer; Host also self-subscribes to its own event bus
- [x] M2.3 Ready consensus — guest `ReadyState` ClientRequest → host coordinator `guest_ready` + `host_ready` + `commit_play` on `all_ready`; both sides converge via `CoordinatorStateUpdate` broadcast
- [x] M2.4 Play prepare/ready/commit — `host_play` → `prepare_play` → `commit_play` → `PlayCommit` envelope; guest reconciles position/state
- [x] M2.5 Pause and seek — `host_pause`/`host_seek` → `commit_pause`/`update_position` + `PauseCommit`/`SeekCommit` envelopes; guest mirrors position + strict_sync_paused
- [x] M2.6 Buffer starvation/recovery — full round trip: guest `BufferStatus` ClientRequest (`QuicClient::send_buffer_status`) → host re-broadcasts `BufferLow`/`BufferRecovered` via `event_tx` → role-aware `apply_peer_event` pauses both sides at the shared position; host-only resume via new `LocalSyncCoordinator::resume_after_buffer_recovery` (never resumes manual pauses) + `report_buffer_low`/`report_buffer_recovered`/`report_buffer_status` AppRuntime APIs. Tests: test_k (guest stalls → host pauses → recovery → both PLAYING) + test_k2 (host stalls → guest pauses, buffering label correct)
- [x] M2.7 Disconnect/reconnect — QUIC connection loss fires `PeerDisconnected` → host `apply_disconnect` → `peer_disconnected()` → Reconnecting/Paused + `network.connected=false`
- [x] M2.8 Shared Controls authority — `ControlRequest` always denied with `host_only_controls` (test_g: guest alone can never reach PLAYING); host toggle via `set_shared_controls` with `AtomicBool` shared to QuicServer; granted requests become canonical host ops
- [x] M2.9 Chat transport — Host→Guest broadcast; Guest→Host `ChatMessage` ClientRequest → host echoes over event bus; dedupe by message_id
- [x] M2.10 Reaction transport — same round-trip; dedupe by reaction_id; rate limiter honored on both ends
- [x] M2.11 UI/event integration — Rust-side commands (`set_ready`, `pause_playback`, `resume_playback`, `send_chat_message`, `send_reaction`, `leave_party`, `set_shared_controls`, `report_buffer_status`) now do real QUIC transport; UI frontend commands already wired (M1 listener remains)
- [x] M2.12 AppRuntime integration tests — tests A–T + env canary all PASS (22 tests)
- [x] M2.13 Full regression — fmt/clippy/test/build + typecheck/lint/test/build all PASS
- [x] M2.14 Tracker update — this file + IMPLEMENTATION_TRACKER.md + MILESTONE_2_CLOSURE_AUDIT.md updated

## IMPORTANT DECISIONS

### 2026-08-17 — Bidirectional QUIC delivery topology
- **Decision:** Host→Guest async events use Quinn `connection.open_bi()` from the server side. Guest reads inbound event streams in a background task.
- **Reason:** Quinn allows both peers to open bi-streams. No need for persistent TCP-like channel. Minimal change to existing M1 request/response architecture. Fits the existing `handle_connection` accept loop by also spawning sender tasks.
- **Affected files:** `src-tauri/src/network/quic.rs` (new `HostEventStream` / server-initiated open), `src-tauri/src/app_runtime.rs` (peer delivery methods)
- **Protocol behavior:** New `ServerEvent` message types (PLAY_COMMIT, PAUSE_COMMIT, SEEK_COMMIT, CHAT, REACTION, BUFFER_STATUS, READY_STATE) travel over server-opened bi-streams. Sequence tracking applies. All post-auth.

### 2026-08-18 — Coordinator is the single source of truth; Host self-subscribes to its own event bus
- **Decision:** The host's `AppRuntimeState.sync_coordinator` and the QUIC server's coordinator are THE SAME `Arc<Mutex<LocalSyncCoordinator>>`; the host additionally runs a `host_event_task` that subscribes to `event_tx` and applies every envelope through `apply_peer_event` (same code path as the guest listener).
- **Reason:** Previously the host state and server coordinator were separate copies, so the host never observed its own canonical coordinator transitions (GuestReadyState callback was ignored, readiness/pause CSUs were only delivered to the guest). With the self-subscriber, the coordinator broadcast is applied to the host snapshot without inventing a second authoritative clock or a parallel state machine.
- **Affected files:** `src-tauri/src/app_runtime.rs` (create_local_party host_event_task, leave_party abort), `src-tauri/src/sync/local.rs`

### 2026-08-18 — One monotonic envelope seq counter per host coordinator
- **Decision:** `LocalSyncCoordinator.coordinator_event_seq: u64` is incremented inside `fire_coordinator_cb` AND by `send_host_event` (both under the coordinator mutex), and every host envelope (CSU, Play/Pause/SeekCommit, chat, reaction, RoomStateUpdate) carries the resulting strictly-increasing seq.
- **Reason:** The previous formula `last_peer_seq_received().wrapping_add(1)` produced seq=1 for EVERY host envelope whenever the host had never recorded a guest seq — the guest's stale-rejection (`seq <= last_peer_seq_received`) silently dropped everything after the first envelope.
- **Affected files:** `src-tauri/src/sync/local.rs`, `src-tauri/src/app_runtime.rs` (send_host_event + broadcast closure)

### 2026-08-18 — Case-consistent coordinator state payloads
- **Decision:** `CoordinatorStateUpdate.coordinator_play_state` is sent as `format!(\"{:?}\", room_state).to_ascii_uppercase()` so `apply_coordinator_state`'s `\"PLAYING\"`-style parse always matches.
- **Reason:** The payload previously used `{:?}` (e.g. \"Playing\") while the parser only matched \"PLAYING\" — guest CSUs silently no-op'd, so the guest never converged to PLAYING/PAUSED.

### 2026-08-18 — Guest actions relay through the host coordinator
- **Decision:** Guest `set_ready`, `send_chat_message`, `send_reaction` now (a) apply locally as before, and (b) fire-and-forget the matching `ClientRequest` (ReadyState / ChatMessage / Reaction) over QUIC via new `QuicClient::send_ready_state/send_chat_message/send_reaction` helpers. The host is the canonical broadcast point: chat/reaction are re-broadcast via `event_tx` (with the guest's device id + request seq in the envelope) and deduped by id on receipt.
- **Reason:** Before this, the guest path was purely local — the host never learned guest readiness, chat, or reactions (tests a–f, i failed at the host side).

### 2026-08-18 — Host detects peer loss from connection teardown
- **Decision:** `handle_connection` fires `QuicHostEvent::PeerDisconnected { device_id }` when its accept loop ends (connection closed); `apply_host_event` maps it to `apply_disconnect` (peer.connected=false, network.connected=false, `coordinator.peer_disconnected()` → Reconnecting/Paused).
- **Reason:** There was no disconnect path on the host at all — test_f (guest leaves → host pauses) could never pass.
- **Affected files:** `src-tauri/src/network/quic.rs`, `src-tauri/src/app_runtime.rs`

### 2026-08-18 — Tailscale null-safe parsing (fixes MP-NET-001)
- **Decision:** `RawNode.tailscale_ips` is `Vec<String>` with `#[serde(default, deserialize_with = \"option_vec_string_null_as_empty\")]` (rename `TailscaleIPs`); `local_ipv4` uses `map(...).and_then(first_ipv4)`. Regression test `parses_real_macos_tailscale_json_with_nulls` covers real macOS `tailscale status --json` output where TailscaleIPs is null.
- **Reason:** Real `tailscale status --json` on macOS emits `\"TailscaleIPs\": null` for unauthenticated/offline nodes, which crashed `create_local_party` with `MP-NET-001 Tailscale output could not be parsed: invalid type: null, expected a sequence`.

### 2026-08-18 — M2 tests serialize the DEV_LOOPBACK env var
- **Decision:** All access to the process-global `MOVIE_PARTY_DEV_LOOPBACK` var is serialized with a shared `tokio::sync::Mutex` (`ENV_LOCK`); `setup_host_guest` and `env_tests::dev_loopback_mode_selects_correct_bind_addr` both hold it while calling `create_local_party`.
- **Reason:** Tests run in parallel and `create_local_party` reads the var; the env canary (no-env → Tailscale error) raced against tests setting `=1`, making it fail intermittently.

### 2026-08-18 — Guest-side assertions poll (async convergence)
- **Decision:** Added `poll_guest` helper; tests b/c/d/e now poll the guest snapshot instead of asserting immediately after a host action.
- **Reason:** Guest state converges asynchronously through `peer_event_listener`; immediate `guest.snapshot()` reads raced the QUIC round trip (test_d always failed with position 0).

### 2026-08-18 — Buffer starvation is a full bidirectional transport, not a local flag
- **Decision:** Guest `BufferStatus` ClientRequests are re-broadcast by the host (like chat/reaction) as `ServerEvent::BufferLow`/`BufferRecovered` through `event_tx`, so BOTH sides apply the same message. `apply_peer_event` resolves the buffering role from `envelope.sender` vs local/peer identity (host: `PeerRole::Host`; guest relayed: `PeerRole::Guest`; cross-side: peer role). Resume is host-only: new `LocalSyncCoordinator::resume_after_buffer_recovery()` fires PLAYING only when `paused_by_strict_sync` is set (manual pauses are never resumed by this path); guests clear flags and converge via the host's canonical `CoordinatorStateUpdate`. Removed the never-constructed `QuicHostEvent::GuestBufferStatus` variant.
- **Reason:** Previously the guest→host path was a silent ack and the host side ignored starvation entirely — `apply_host_event` dropped `GuestBufferStatus` and no host path ever broadcast `BufferLow`. AGENTS.md §14 requires the HOST to stop when the guest cannot continue; nothing did.
- **Affected files:** `src-tauri/src/sync/local.rs` (resume API + unit test), `src-tauri/src/network/quic.rs` (send_buffer_status helper, BufferStatus broadcast arm, variant removal), `src-tauri/src/app_runtime.rs` (role-aware BufferLow/BufferRecovered arms, report_buffer_low/report_buffer_recovered/report_buffer_status APIs), `src-tauri/tests/m2_integration.rs` (test_k, test_k2)

### 2026-08-18 — Shared Controls default OFF + host-only toggle + QUIC enforcement
- **Decision:** `shared_controls` defaults to `false` in `AppRuntimeState`; `shared_controls_flag` is an `Arc<AtomicBool>` shared between AppRuntime and QuicServer. `set_shared_controls()` rejects guest attempts with `MP-CTRL-002`. QUIC server reads the flag on every `ControlRequest`; when off, sends `ControlDeny` event (guest surfaces `MP-CTRL-001`). When on, grants → `GuestControlRequest` host event → `on_guest_control_request` → canonical host operation.
- **Reason:** MASTER_PRD §50/51 requires host-only default; PROTOCOL_SPEC §41/42 requires guest-only CONTROL_REQUEST with host authority. Tests O/P/Q prove all three paths.
- **Affected files:** `src-tauri/src/app_runtime.rs` (set_shared_controls, on_guest_control_request, guest_control_request), `src-tauri/src/network/quic.rs` (shared_controls on QuicServer, ControlRequest grant/deny), `src-tauri/src/lib.rs` (set_shared_controls command)

## COMPLETED WORK

### 2026-08-17 — Build repair + EventEnvelope refactor
- What: Fixed compile errors; refactored peer-event transport to use `EventEnvelope` consistently.
- Why: Previous session had `EventEnvelope` defined inside an `impl` block (illegal), `monotonic_us` imported from a module that doesn't export it, `listen_for_server_event` using `open_bi()` (wrong direction), `send.finish().await` misused (finish returns Result not future), `VecDeque` missing import, and broken coordinator tests.
- Important logic:
  - `EventEnvelope { seq, sender, sent_mono_us, event: ServerEvent }` now lives at module top in quic.rs, derives Serialize/Deserialize, and is the broadcast channel payload.
  - `QuicServer::broadcast(event)` assigns `seq` and `sender` (= host_device_id) and `sent_mono_us` (= monotonic_us()). There is one canonical seq source per host.
  - The host dispatcher task in `handle_connection` now subscribes and writes fully-formed envelopes over opened bi-streams. New helper functions `write_event_envelope` / `read_event_envelope`.
  - `QuicClient::listen_for_server_event` uses `accept_bi()` (not open_bi). Trailing recv is drained with `read_to_end`.
  - `monotonic_us` is now `pub` and lives in quic.rs.
  - AppRuntimeState gained `host_event_tx: Option<Arc<broadcast::Sender<EventEnvelope>>>`.
  - Fixed `apply_peer_event` to use `coordinator.commit_play(&ScheduledPlayback{..})` and `coordinator.commit_pause(target_position_ms)`.
  - Fixed broken tests in sync/local.rs — `buffer_low` returns `Result<(), _>` and there is no `update_readiness`; tests now use `host_ready`/`guest_ready`.
- Files changed: src-tauri/src/network/quic.rs, src-tauri/src/app_runtime.rs, src-tauri/src/sync/local.rs
- Tests run: cargo build (green), cargo test --lib (139 pass), cargo test --tests (2 pass)

### 2026-08-18 — Tailscale MP-NET-001 fix + regression test
- What: Parsed real macOS `tailscale status --json` (null `TailscaleIPs`) instead of erroring.
- Why: `create_local_party` failed on every non-dev run with `MP-NET-001 Tailscale output could not be parsed: invalid type: null, expected a sequence`.
- Important logic: `option_vec_string_null_as_empty` deserializer on `RawNode.tailscale_ips: Vec<String>`; `first_ipv4` chain; `parses_real_macos_tailscale_json_with_nulls` test; restored `#[test]` on `classifies_derp_and_offline`.
- Files changed: src-tauri/src/network/tailscale.rs
- Tests run: cargo test tailscale (pass)

### 2026-08-18 — Coordinator broadcast wiring (Host)
- What: `LocalSyncCoordinator` gains `coordinator_tx`, `coordinator_local_id`, `coordinator_broadcast_fn`; `create_local_party` installs a fn-pointer closure sending `CoordinatorStateUpdate` envelopes; `fire_coordinator_cb` invokes it on every state change.
- Why: Coordinator transitions must reach the guest (and the host itself) as canonical `CoordinatorStateUpdate`.
- Files changed: src-tauri/src/sync/local.rs, src-tauri/src/app_runtime.rs
- Tests run: cargo test --lib (139 pass)

### 2026-08-18 — Arc<Mutex<LocalSyncCoordinator>> refactor
- What: `AppRuntimeState.sync_coordinator` changed from an owned `LocalSyncCoordinator` to `Arc<Mutex<LocalSyncCoordinator>>`, shared with the QUIC server (`server.run(Some(coordinator_handle))`); all call sites updated.
- Why: The server coordinator and the state copy diverged; the host snapshot never reflected server-side coordinator mutations.
- Important logic: borrow-checker-safe pattern — extract values with short-lived guard temporaries; never hold a coordinator guard across a `state.X = ...` write.
- Files changed: src-tauri/src/app_runtime.rs, src-tauri/src/network/quic.rs, src-tauri/src/sync/local.rs
- Tests run: cargo check (clean), cargo test (all green after M2 fixes)

### 2026-08-18 — M2 integration suite green (tests A–J + env canary)
- What: All 11 tests pass over real loopback QUIC.
- Root causes fixed (see IMPORTANT DECISIONS):
  1. Guest actions were local-only → added `QuicClient` send helpers + fire-and-forget transport in `set_ready`/`send_chat_message`/`send_reaction`.
  2. Host never applied its own coordinator broadcasts → `host_event_task` self-subscriber in `create_local_party`.
  3. Host envelope seq always 1 → `coordinator_event_seq` monotonic counter shared by `fire_coordinator_cb` and `send_host_event`.
  4. CSU payload case mismatch ("Playing" vs "PLAYING") → uppercase payload.
  5. No host disconnect detection → `QuicHostEvent::PeerDisconnected` fired by `handle_connection` teardown.
  6. `host.set_ready` never committed → mirror `handle_request`'s `all_ready → commit_play`.
  7. Racy guest assertions → `poll_guest` helper.
  8. Env-var race between parallel tests → `ENV_LOCK` serialization.
  9. `ConnectionSession` now stores `authenticated_display_name`; `handle_request` broadcasts guest chat/reaction with the guest's display name as sender; dedupe by message/reaction id on both ends.
- Files changed: src-tauri/src/network/quic.rs, src-tauri/src/app_runtime.rs, src-tauri/src/sync/local.rs, src-tauri/tests/m2_integration.rs
- Tests run: cargo test (140 lib + 2 + 11 all pass)

### 2026-08-18 — M2.6 buffer starvation/recovery completed (real transport)
- What: Full bidirectional buffer-starvation path implemented and proven over real loopback QUIC.
- Why: M2.6 was marked done prematurely — the guest→host path was a silent ack, `GuestBufferStatus` was ignored by `apply_host_event`, and no host path ever broadcast `BufferLow`.
- Tests added: `buffer_recovery_resumes_only_strict_sync_pauses` (unit), `test_k`, `test_k2`.
- Files changed: src-tauri/src/sync/local.rs, src-tauri/src/network/quic.rs, src-tauri/src/app_runtime.rs, src-tauri/tests/m2_integration.rs
- Tests run: cargo test (141 lib + 2 + 13 all pass), cargo clippy -D warnings (clean), cargo fmt --check (pass)

### 2026-08-18 — M2.7–M2.12 tests L–T added
- What: Tests L (ready doesn't bypass play), M (deadline honored), N (clock calibration), O (shared controls denied), P (shared controls granted), Q (guest can't toggle), R (reconnect), S (stale/duplicate op-id), T (seek waits for guest) added.
- Tests run: cargo test --test m2_integration: 22/22 pass

### 2026-08-18 — Quality gate + cleanup
- What: `cargo fmt` applied; clippy clean (`-D warnings`); removed all `[M2-DEBUG]`/`[M2-T]` eprintln instrumentation; removed unused `base64::Engine` import and `host_snap` helper from m2_integration.rs; restored missing `#[test]` on `classifies_derp_and_offline`.
- Files changed: src-tauri/src/app_runtime.rs, src-tauri/src/network/quic.rs, src-tauri/src/network/tailscale.rs, src-tauri/tests/m2_integration.rs
- Tests run: cargo fmt --check (PASS), cargo clippy (clean), cargo test (all pass)

### 2026-08-18 — M2 closure audit (FINAL)
- What: Re-read all acceptance matrix items and audit items against source code. Verified Shared Controls behavior (default OFF, host toggle, MP-CTRL-001/002, request forwarding, canonical authority). Verified buffer handling (starvation pauses, no auto-resume, fresh sync required, no fake timers). Searched for debug artifacts (clean). Ran full regression gate (167 Rust + 3 FE = 170 pass, 0 fail). Updated all three docs.
- No code changes needed.
- Status: **M2 LOCALLY COMPLETE**

## OPEN ISSUES / BLOCKERS
- ⚠ EXTERNAL VERIFICATION PENDING: real physical Tailscale Mac ↔ Windows peer session for M2 (sync agreement, readiness, pause/seek, chat/reaction over real network, disconnect pause on both platforms).
- Known transient: `host_play` when already PLAYING fires a momentary READYCHECK `CoordinatorStateUpdate` before PLAYING (prepare_play semantics); both sides converge to PLAYING; harmless to current tests, worth tightening when real RTT scheduling lands.
- `QuicServer::broadcast` + `next_event_seq: AtomicU64` on the server remain unused by app paths (host envelopes use `coordinator_event_seq`); candidate cleanup.

## RESUME INSTRUCTIONS
Milestone 2 is LOCALLY COMPLETE. Next step is either:
1. External verification: run M2 manual acceptance on real Mac + Windows over Tailscale.
2. Start Milestone 3 planning.
Read this file first. Then read AGENTS.md. Then run `git status --short` to see current modification state.
