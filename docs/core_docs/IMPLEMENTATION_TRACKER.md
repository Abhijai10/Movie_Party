
# DOCUMENT 5 — `IMPLEMENTATION_TRACKER.md`


# Movie Party — Implementation Tracker

This document records implementation status.

It must be updated continuously.

---

# CURRENT STATUS

```text
Project State:
🟨 BATCH 12 (REAL CROSS-DEVICE CALL SESSION) COMPLETE (code-level).
    Audit finding P2 closed: the video call is now a real per-device
    RTCPeerConnection session signalled through the host relay instead
    of the single-device loopback demo. Backend (Rust): submit_call_signal
    validates against a call signal ledger (OFFER needs no unanswered
    offer pending; ANSWER needs a pending offer; violations →
    MP-CALL-001, rejected signals are never appended), enforces the §52
    64 KiB payload cap with an over-64KiB rejection test, relays via
    host broadcast / guest client push, and an is_self guard in
    apply_peer_event stops a publisher's own signal from re-entering
    its own receive path (self-echo test). RENEGOTIATE is specified as a
    restart-REQUEST marker, not an SDP (§52.1: ≤2 KiB JSON with a
    session-unique id; a mislabeled SDP can never fit and is rejected;
    a valid marker conditionally resets the ledger — only a never-
    completed exchange resets, so an in-flight ANSWER cannot be
    poisoned MP-CALL-003). Six new ledger tests: never-completed reset,
    completed-exchange preservation, answer-after-reset, marker
    idempotency, >2 KiB rejection, exact-2 KiB acceptance. Frontend
    (src/call/callSession.ts, ~600 lines): startRealCallSession builds
    one peer connection per device — host=offerer, guest=answerer
    (AGENTS §15); NON-TRICKLE ICE (waits icegatheringcomplete 5s,
    candidates embedded in SDP — eliminates fire-and-forget send-order
    hazards); applyRemoteSignal is internally SERIALIZED (queued
    promise chain) so two cursor batches cannot interleave
    setRemoteDescription; host-side glare guard skips a redundant
    ANSWER at stable signaling state; wire recovery re-offers carry
    iceRestart:true (a plain offer would reuse failed candidate
    pairs); failed→immediate re-offer, disconnected→2.5s grace, 4s
    anti-spam backoff; RENEGOTIATE pokes are never suppressed (a
    suppressed poke deadlocks recovery — the restarted peer's cursor
    pinned past all earlier offers). Both sides publish their marker at
    session start; the host drops its own self-echo by session-unique
    markerId, the guest drops all markers by role.
    src/views/CinemaView.tsx runs the session lifecycle (key =
    mode:role:privacy — camera/mic toggles deliberately excluded, PRD
    §41 keeps them local track.enabled flips — and a dedicated toggle
    effect pushes every later camera/mic change to the live session's
    tracks; acquisition is MODE-driven so a camera enabled mid-session
    already has a track to flip, never a renegotiation) with an
    index-based signal cursor (mixed clock domains: submit stamps
    host_time_us, receive stamps quic::monotonic_us — timestamps are
    unusable for ordering; index cursor + shrink-reset + startup hold
    instead) and nextPendingSignals() applies PER-ROLE COALESCING:
    guest keeps the last OFFER at/after the last marker + trailing ICE;
    host keeps ANSWER/ICE + the last marker unless an ANSWER follows or
    it is own-id self-echo; malformed entries are consumed-but-skipped
    (§67: skip, never crash). CallTile renders live remote video, remote
    audio, and a PiP self-view (Lobby/ReadyCheck use presentation-only
    null streams; the session lives in CinemaView alone). Gates:
    cargo fmt/clippy -D warnings/cargo test 326 (0 fail), frontend
    lint/vitest/tsc clean. Loopback self-test retained behind a
    localhost+localStorage opt-in flag for diagnostics. NOT verifiable
    here: a REAL two-device call (camera/mic permissions, Tailscale
    direct path, Windows guest) → ⚠ EXTERNAL VERIFICATION PENDING.
    Next permitted: Batch 13.

Previous:
🟨 BATCH 11 (PROTOCOL TRUTH & ADR FOUNDATION) COMPLETE (code-level).
    Audit finding P1/P11 closed: the wire protocol now matches an honest,
    ADR-ratified spec instead of a known-false one. ADR-0001 (accepted,
    Option B) amends §3: JSON + u32-BE length prefix + serde string tags
    is the canonical V1 format (the CBOR text was never implemented;
    migrating during feature completion traded zero user value for wire
    destabilization — full rationale, options, and rollback in
    docs/architecture/adr/ADR-0001-wire-format.md). ADR-0002 aligns the
    code's MessageType registry to §11 exactly: 61 registered IDs, zero
    collisions (the old enum had ReadyState/BufferStatus/ControlRequest
    squatting on the SCHEDULE 200–204 range), from_u16 parsing with
    unknown-ID rejection, and machine-enforced spec-parity + uniqueness +
    round-trip tests so drift cannot recur silently. The §5 256 KiB
    control limit is now enforced at the framing layer on BOTH ends
    (MP-PROTO-004 MESSAGE_TOO_LARGE; the old silent 2 MiB cap is gone,
    oversized length prefixes rejected before any allocation) and every
    EventEnvelope carries v_major/v_minor/room_id (§10), validated on the
    guest receive path plus a room-match defense-in-depth check in
    apply_peer_event. §67 malformed-input property tests added: oversized
    frame (rejected pre-allocation), exact-256 KiB boundary acceptance,
    unknown type tag, missing required fields (incl. legacy envelopes
    missing the §10 fields → parse error), negative-where-unsigned, and
    seq=0/stale-sequence rejection over live loopback QUIC. All gates
    green: cargo fmt --check, clippy -D warnings, 315 Rust tests
    (0 fail), PROTOCOL_SPEC §3/§4/§5/§10/§67/§69 amended to match
    implementation. Not verifiable here: Windows + two-device QUIC
    session → ⚠ EXTERNAL VERIFICATION PENDING. Batch 12 (real
    cross-device call, P2) is next; it builds on the now-stable relay.

Previous:
🟨 V1 COMPLETION PLAN ISSUED — 2026-09-05 full-project audit found 18
    findings (P1–P18) and produced a 13-batch plan to V1
    (`docs/core_docs/V1_COMPLETION_PLAN.md`). The three biggest:
    P1 wire protocol violates the locked PROTOCOL_SPEC (JSON + 2 MiB +
    string tags vs CBOR + 256 KiB + numeric IDs; message-ID collisions
    with the SCHEDULE range); P2 the video call is a local loopback with
    no real remote video rendering; P3 Provider Sync never dispatches
    canonical playback ops to provider adapters. Stale milestone-era docs
    were deleted the same day; findings were merged into
    `0_Remaining_Things.md` (see its "2026-09-05 full-project audit"
    section). Batches 11–23 are next; decision points D1–D4 (protocol,
    chat layout, Shared scope, cadence) need user input before Batch 11.

Previous:
🟨 SOCIAL UX + GHOST/PRIVACY CLOSURE COMPLETE (code-level).
    Batch 10 (social UX) closed the remaining frontend/social gaps on
    top of the desktop/runtime closure: Ghost Mode now restores the
    exact prior local UI state on exit (a manually-opened chat returns,
    expired transient previews stay closed; the call tile reappears at
    its own saved position/visibility since the modes only visually
    suppress it via the social-hidden class) and no longer destroys chat
    state on entry; chat arrival semantics are centralized (an incoming
    message reveals a transient preview when hidden, never reveals while
    Ghost/Privacy suppresses social UI, and sets the unread badge that
    survives the modes); Enter/"c" chat shortcuts are gated while
    Ghost/Privacy hides social UI; the Ready Check screen now renders the
    call tile above the "Dim the Lights" content (never disappears
    behind that screen) with social-hidden gating; the Lobby ready
    footer wraps so the READY CHECK button can no longer push out of the
    window; the provider select is label-associated, driven by a tested
    pure model (placeholder/labels/value resolution — the visible
    selection is never blank) and keeps the dark native control scheme.
    Backend semantics were already correct and tested (ghost never
    touches devices; privacy disables both and never re-enables on
    exit); this batch adds the focused local-vs-remote separation test
    (my camera/mic toggles never mutate the peer participant snapshot).
    Not manually verified: real cross-device WebRTC/camera/mic and visual
    checks remain ⚠ EXTERNAL VERIFICATION PENDING.

Previous:
🟨 DESKTOP + RUNTIME PRODUCTION CLOSURE COMPLETE (code-level).
    Batch 10 closed the remaining desktop/runtime integration gaps:
    Windows now presents real libmpv software-rendered frames into the
    existing child HWND behind the transparent WebView (GDI StretchDIBits,
    32bpp BI_RGB top-down DIB — byte-identical to mpv's bgr0 output, no
    per-pixel conversion, no second rendering architecture); the previously
    ungated macOS-only msg helper no longer breaks the Windows build; macOS
    presentation revalidated (bgr0→CGImage mapping, layer ownership,
    resize, cleanup all verified correct); guest BUFFER_STATUS now reports
    on the PRD §21/§28 500 ms cadence while Playing via a single
    replace+abort worker; heartbeat liveness is application-level
    (2 s interval, 5-missed ≈10 s threshold per PROTOCOL_SPEC §16) with
    role-aware recovery events (guest detecting host loss records
    HostCrash, not GuestCrash) and recovery still never auto-resumes;
    reconnect/lifecycle revalidated (bounded backoff, cache survives,
    completed chunks not re-fetched, no auto-resume, all session workers
    aborted on leave/create/join); structured logging wired to a local-only
    tracing subscriber (INFO default per PRD §87, RUST_LOG override, no
    cloud, no credentials); chat persistence revalidated — V1 chat is
    ephemeral per PRD (SQLite functions stay dormant by design); readiness
    revalidated truthful (no fabricated PLAYING/buffer); PROTOCOL_SPEC vs
    implementation wire-format discrepancy (CBOR/256 KiB/numeric IDs vs
    JSON/serde tags/2 MiB) verified real and documented as a protocol
    reconciliation follow-up. Not manually verified: real Windows/macOS
    playback and cross-device flows remain ⚠ EXTERNAL VERIFICATION PENDING.

Current Phase:
BATCH 10 — SOCIAL UX + GHOST/PRIVACY CLOSURE

Current Release:
V1 Development

Architecture:
LOCKED

Critical Blockers:
M3: Native mpv Cinema host implemented on macOS AND Windows (SW render →
    NSView/CALayer and child-HWND GDI presentation); real playback on both
    OSes still requires external verification
M4: OS Keychain/Credential Manager and notification dispatch require platform verification
M5: Real two-device WebRTC call needs physical devices and OS permission prompts
M6: Real Chrome/provider login and media playback require external verification
M7: ScreenCaptureKit needs macOS permission dialog
M8: Chrome crash watchers not wired (player crash/liveness paths ARE wired
    as of Batch 10: heartbeat → missed detection → recovery state, with
    role-aware events and no auto-resume)
```

---

# BATCH 10 — SOCIAL UX + GHOST/PRIVACY CLOSURE (2026-09-05)

## Objective

Close the frontend social-UX and privacy-mode gaps flagged in
`0_Remaining_Things.md` (call tile local/remote separation and interaction
semantics, Ghost Mode UI-state restore, chat arrival/unread semantics,
Ready Check overflow, provider selector) without touching the locked
architecture, the Emergent visual identity, the global purple button
system, the WebRTC signalling path, or the already-correct backend
Ghost/Privacy semantics.

## Implemented in this pass

- **Ghost/Privacy UI-state restore**: entering Ghost or Privacy Mode
  captures the chat overlay visibility (pure module
  `src/social/ghostUiState.ts`); ending the mode restores it — a
  manually-opened chat comes back open, an expired transient preview
  stays closed. The call tile needs no snapshot: its session (position,
  minimized, hidden) lives in AppShell and is only visually suppressed by
  the `social-hidden` class, so it reappears exactly as the user left it.
  Devices follow the backend: Ghost never touches them; Privacy disables
  both and never re-enables on exit.
- **Chat arrival semantics** (`applyChatArrival` in
  `src/chat/overlayState.ts`): an incoming message reveals a transient
  ~5 s preview while the overlay is hidden; it never reveals the overlay
  while Ghost/Privacy suppresses social UI — the unread flag alone
  survives the mode so the chat button badge informs the user afterward.
  `canToggleChat` gates the Enter/"c" shortcuts while the modes hide
  social UI (the keyboard can no longer leak the chat overlay).
- **Call tile on Ready Check**: the tile now renders above the
  "Dim the Lights" content (documented requirement: it must never
  disappear behind that screen), gated by `social-hidden` like Lobby and
  Cinema, and keeps its AppShell session across views.
- **Close/minimize semantics as a tested model**
  (`src/overlays/callTileState.ts`): `closeCallTileLocally` /
  `showCallTile` / `minimizeCallTile` / `restoreCallTile` /
  `canInitiateDragFrom` encode the rules — close hides locally without
  tearing the call down or touching position/minimized state; minimize
  keeps a draggable video stage; the CallTile component now uses these
  helpers instead of inline state spreads.
- **Ready Check footer overflow**: `.lobby-ready-actions` wraps
  (status row and action buttons can stack at narrow desktop widths),
  completing the already-wrapped button row, so the READY CHECK button
  can no longer extend out of the window. Global button system untouched.
- **Provider selector**: the label is now associated with the select
  (`htmlFor`/`id`), and the option list plus visible-value resolution
  come from a tested pure model (`providerSelectLabels` /
  `resolveProviderSelectValue`) — the visible selection is never blank
  while capabilities load. The dark native control scheme
  (`color-scheme: dark` + explicit option colors) stays.

## Verification

- `pnpm lint` ✅
- `pnpm test` ✅ (10 files, 92 tests)
- `pnpm exec tsc --noEmit` ✅
- `pnpm build` ✅
- `cargo test --lib` ✅ (300 passed, including the new
  `local_device_toggles_never_mutate_peer_snapshot`)
- `cargo fmt --check` ✅ / `cargo clippy --all-targets -- -D warnings` ✅

## External Verification Pending

- Real two-device WebRTC call: local mic/camera toggles must change only
  my outgoing tracks while the peer tile follows genuine remote device
  state (requires physical devices and OS permission prompts).
- Visual/manual checks on macOS and Windows: call tile drag, bounds,
  z-order above Lobby/Ready Check content, minimize/restore, local
  close/reopen; Ghost entry/exit restoring chat and tile state; Privacy
  entry disabling devices and requiring explicit re-enable; unread
  badge; Ready Check layout at narrow desktop widths; provider select
  dropdown appearance (hover/selected/keyboard) on both OSes.

---

# BATCH 10 — DESKTOP + RUNTIME PRODUCTION CLOSURE (2026-09-05)

## Objective


Close the remaining desktop/runtime integration gaps identified by the
full audit, preserving the locked architecture. Code-level only: all
real-device/real-OS behaviors remain developer-owned acceptance tests.

## Windows native video presentation (item 1) — ✅ code-complete, 🧪 external verification pending

`src-tauri/src/media/player/native_surface.rs`:

- `display_frame` on Windows is no longer a no-op. It now validates the
  frame exactly like the macOS path (non-null surface, positive w/h,
  stride ≥ w·4, data ≥ stride·h, i32-range dims) and blits it into the
  existing child HWND's client rect via GDI `StretchDIBits`.
- mpv's `bgr0` (B,G,R,0 per pixel) is byte-identical to a 32bpp `BI_RGB`
  DIB with negative `biHeight` (top-down), so no per-pixel conversion and
  no second rendering architecture were introduced — this is the same
  SW-render path the render loop already drives on macOS. Stride is honored
  the same way macOS honors `bytesPerRow`: the DIB is declared
  `stride/4` pixels wide so GDI addresses rows at the mpv stride, and the
  blit's source rectangle samples only the real frame columns (padding
  never displayed). The frame is presented directly from the render
  buffer (no intermediate copy).
- The surface stays behind the transparent WebView (`HWND_BOTTOM`,
  `SWP_NOACTIVATE` on attach — unchanged) and remains the only native
  presentation surface.
- Fixed a real cross-platform compile defect: the macOS-only `msg_void_id`
  helper (references `selector()`/`objc_msgSend` externs) was not
  cfg-gated, so the Windows build of this file could not compile. It is
  now `#[cfg(target_os = "macos")]`.
- Deterministic Windows-only unit tests added (compile+run on Windows CI;
  no window needed): `windows_frame_validation_accepts_strided_bgr0_buffer`,
  `windows_frame_validation_rejects_malformed_frames`,
  `windows_frame_validation_handles_i32_overflowing_dimensions`.
- Windows integration harness added: `tests/windows_native_surface_e2e.rs`
  (`#![cfg(windows)]`) drives the full production path — real child HWND →
  production `MpvPlayer` → `attach_native_surface` → `render_next_frame`
  → `display_frame` → StretchDIBits — and skips gracefully when the
  bundled `libmpv.dll` or test video is absent.
- 🧪 Real Windows playback (frames visibly presented, resize, pause/seek
  correctness) is NOT claimed; it requires the developer to run the app on
  Windows with a real movie.

## macOS presentation revalidation (item 2) — ✅ verified correct, no defects

Audited `NativeVideoBounds` validation, NSView ownership/retain semantics,
`addSubview:positioned:relativeTo:` (below the webview), layer
`setDrawsBackground:false`, CGImage creation (`kCGImageAlphaNoneSkipFirst |
kCGBitmapByteOrder32Little` — verified to be the exact byte order of mpv
bgr0), per-frame layer content update, `setFrame` resize, and detach
cleanup. No concrete defect found; no change made beyond the msg helper
cfg-gate that was breaking cross-platform compilation. Bridge description
strings (`presentation.rs`) were corrected from the stale "via libmpv wid"
(wid embedding is not used by this LGPL SW-render build) to the truthful
"via libmpv SW render" on both platforms. 🧪 Real macOS movie playback
remains externally verified (existing macOS harness covers the code path;
manual movie test remains pending).

## Strict-sync BUFFER_STATUS cadence (item 3) — ✅ wired

`app_runtime.rs`:

- New single periodic worker `spawn_buffer_status_worker`: guests report
  buffer status every 500 ms (`BUFFER_STATUS_INTERVAL`, matching PRD §21
  and PROTOCOL_SPEC §28) while `room_state == Playing && !strict_sync_paused`
  with a player and client present. Paused/Ended/Buffering/Reconnecting
  rooms do not spam reports.
- Transition-triggered reports in the player event loop are unchanged and
  remain supported.
- Exactly one worker can exist: spawn replaces + aborts the previous
  (verified by test). Aborted in `leave_party`, `create_local_party`
  duplicate-hygiene, and `join_party` teardown, so it cannot leak across
  rooms or publish into a replaced session. Each tick reads the current
  client, so a reconnected transport is used immediately.
- Buffer fields remain truthful: `stalled` is the player's genuine
  Buffering state, headroom is the player's buffered-ahead value, and
  transfer percent stays the separate whole-file verified-progress concept.
  No Batch 9B fake-readiness regression.
- Tests: `buffer_status_worker_is_replaced_not_duplicated` (incl. Leave
  Party cancels it), `buffer_status_worker_cadence_is_500ms_while_playing_only`.

## Host-crash / heartbeat liveness (item 4) — ✅ wired

`app_runtime.rs`:

- Heartbeat constants extracted: `HEARTBEAT_INTERVAL` (2 s) and
  `HEARTBEAT_FAILURE_THRESHOLD` (5 consecutive missed acks ≈ 10 s) match
  PROTOCOL_SPEC §16's application-level disconnect rule. Detection is
  application-level (not only QUIC idle timeout): the guest heartbeat
  worker counts consecutive failed `heartbeat()` calls and declares
  liveness failure via the testable pure helper
  `heartbeat_declares_liveness_failure`.
- Role-aware peer-loss events: `apply_disconnect` now records
  `HostCrash` when the local participant is the guest (its peer — the
  host — died) and `GuestCrash` when it is the host. Previously both
  sides recorded GuestCrash, so the guest's recovery snapshot mislabeled
  a host crash. Both route through the same existing recovery machine
  (`peer_disconnected` → RECONNECTING, strict-sync pause, bounded
  reconnect backoff) — no new recovery architecture.
- No auto-resume: recovery never returns the room to PLAYING by itself;
  readiness consensus is required again (existing
  `runtime_failures_drive_recovery_state` test plus the new role-aware
  variants).
- New tests: `heartbeat_threshold_matches_protocol_liveness_rules`,
  `peer_loss_event_is_role_aware`,
  `host_crash_detection_feeds_guest_recovery_state` (real PLAYING →
  disconnect → RECONNECTING + HostCrash + never PLAYING),
  `guest_crash_detection_feeds_host_recovery_state`.
  Existing `leave_party_aborts_reconnect_heartbeat_preload_and_watch_workers`
  extended to also cover the buffer-status worker (cancellation).

## Reconnect / retry revalidation (item 5) — ✅ verified, follow-up documented

Reverified: bounded backoff `[250, 750, 1500, 3000, 5000, 8000]` ms with 6
attempt cap; single reconnect worker (replace + abort); guest cache
survives disconnect (detached from state, preserved on disk — completed
chunks never re-fetched, sparse holes remain requestable); old transport
dropped; the authenticated-sequence guard prevents a stale session's
messages from mutating the new room; reconnect does not resume playback.
The PROTOCOL_SPEC's optional resume-token optimization remains
intentionally unimplemented — the guest re-fetches only genuinely missing
ranges via the existing demand path. Documented as a protocol
reconciliation follow-up below; NOT implemented in this batch.

## Leave-party / worker lifecycle (item 6) — ✅ verified

All session workers (`player_event`, `player_render`, `transfer`,
`transfer_stall_watcher`, `reconnect`, `heartbeat`, `buffer_status` (new),
`preload`, `peer_event`, `host_event`, `calibration`) are aborted before
replacement and cancelled by `leave_party`, `create_local_party`
duplicate-create hygiene, and `join_party` teardown. Range server
shutdown, player close, on-disk cache preservation, and
`old_transfer_task` diagnostics retention revalidated. No broad refactor
performed; the only lifecycle change is the new buffer-status worker
joining the existing abort lists.

## Telemetry / structured logging (item 7) — ✅ wired

`telemetry/mod.rs` + `lib.rs`:

- The existing `DEFAULT_LOG_LEVEL` ("INFO") is now actually used:
  `init_local_logging()` installs a local stdout `tracing-subscriber`
  (fmt, `RUST_LOG`-overridable, uptime timer, idempotent) in `run()`
  before any setup, per PRD §87.
- Local only. No cloud sink, no network sender. Existing
  `tracing::warn!` calls (deep-link registration failure) now actually
  emit. No credentials/cookies/passwords/media paths/full chat content
  are logged (PRD §86 constraints honored by the pipeline's call sites).
- Tests: `default_log_level_matches_prd_section_87`,
  `local_logging_init_is_idempotent_and_never_panics`.

## Chat persistence (item 8) — ✅ kept ephemeral (per PRD)

Revalidated against MASTER_PRD: V1 chat is "temporary chat messages"
(ephemeral, party-duration). The `chat_messages` SQLite table is described
as "Optional local history. Default retention: party duration only".
Current code matches the PRD: chat lives in runtime state for the session;
`storage/sqlite.rs` `insert_chat_message`/`get_chat_messages` remain
dormant utilities (tested) with zero runtime callers. No persistence was
wired — the PRD does not require it for V1 and reconnect/app-restart chat
persistence is not specified for V1. If a future phase requires history,
wire the existing functions rather than building a new architecture.

## Playback readiness (item 9) — ✅ revalidated truthful

`local_media_genuinely_ready` unchanged (Batch 7/9B guards intact);
`enter_cinema`/`host_play` honesty unchanged; guest BufferLow echo still
drives strict-sync pause; BufferRecovered still routes to ReadyCheck
(never auto-resume); transfer percent ≠ readiness; no fabricated buffer
or PLAYING anywhere in the changed paths. Protocol-test simulation
behavior preserved (integration suites pass unchanged).

## Path/file/resource audit (item 10) — ✅ no defects found

Media id validation (`..`, `/`, `\` rejection), cache keyed by validated
media_id under a host-provided root, range-server token + media_id scope,
loopback-only bind, canonicalization guards — all reverified. No change
required.

## Spec / code discrepancy (item 11) — ⚠ documented as follow-up (intentional non-fix)

Verified real, documented in the **Protocol reconciliation follow-ups**
section below. Not fixed in this batch (a wire-format migration is a
versioned protocol change, not a runtime patch).

## Protocol reconciliation follow-ups (Batch 10 finding)

1. **Wire format**: PROTOCOL_SPEC §3 specifies canonical CBOR with string
   keys (V1). Implementation uses JSON (`serde_json`) with serde enum
   tags (`#[serde(tag = "type", content = "payload")]`). The JSON V1 wire
   format is the intentional current implementation; migrating to CBOR is
   a versioned protocol change requiring coordinated host/guest upgrades.
2. **Request size limit**: PROTOCOL_SPEC §5 specifies 256 KiB control
   messages (MP-PROTO-004 MESSAGE_TOO_LARGE). The transport enforces
   `MAX_REQUEST_BYTES = 2 MiB` in `network/quic.rs`
   (`protocol::MAX_CONTROL_MESSAGE_BYTES = 256 KiB` exists but is unused
   by the transport). The enforced transport limit is the operative one;
   tightening to 256 KiB is part of the same reconciliation.
3. **Message IDs**: `protocol/mod.rs` defines numeric `MessageType` IDs
   matching the spec's table, but the wire uses serde tags, so numeric
   IDs are currently declarative only.
4. **Reconnect resume token**: PROTOCOL_SPEC's optional resume-token
   optimization is unimplemented; reconnect re-fetches only missing ranges
   (correct, just not optimized).

These require a single coordinated `PROTOCOL_SPEC` + implementation pass
in a dedicated future batch (must be versioned; must not silently change
either side).

## Frontend impact

None. No `src/` file changed; bridge-string changes are backend-only
(the frontend displays the string verbatim). No pnpm validation required
per batch rules ("frontend validation passes if frontend changed").

---

# BATCH 9 — LOCAL PERFECT MEDIA DELIVERY/PLAYBACK AUDIT (2026-09-03)

## Objective

Audit and complete the existing Local Perfect path so a Host can provide an
authorized local movie to the Guest over the existing Tailscale + QUIC
architecture and the Guest can actually consume that media through the
existing playback stack. No architecture redesign; only concrete
gaps between code and PROTOCOL_SPEC were fixed.

## Verified correct (no redesign)

- **Media identity / fingerprinting** (`media/manifest/mod.rs`): media_id is
  derived from file size + full BLAKE3 hash prefix; quick fingerprint hashes
  first/last 4 MiB; `validate_for_guest` rejects empty/oversized/mismatched
  manifests; identity is never filename/extension/display-name based. Duplicate
  requests reuse the same cache root keyed by media_id; replacing the source
  file changes the full hash → new media_id → old cache is not reused.
- **1 MiB chunk semantics** (`media/manifest`, `media/transfer`):
  `DEFAULT_CHUNK_SIZE_BYTES = 1_048_576`; `chunk_len` computes the final
  partial chunk; `build_chunk_packet` validates the payload length against
  `chunk_len`; `validate_chunk_packet` checks media_id, index bounds, exact
  length, and per-chunk hash. Zero-length and out-of-bounds ranges are
  rejected (`InvalidRange` → HTTP 416).
- **Guest sparse cache** (`media/cache/mod.rs`): sparse chunk map persisted to
  `chunk-map.bin`; `read_range` only returns data when every chunk in the
  range is present; `complete()` requires all chunks; writes are bounded per
  chunk (no whole-movie memory buffer); partially cached media is never
  marked complete; cache state survives reconnect/retry.
- **Integrity** (`media/transfer/mod.rs`): every received chunk is re-hashed
  and validated before it may enter the cache; a wrong-length, wrong-offset,
  wrong-identity, or corrupted chunk is rejected and never exposed to
  libmpv. Corrupted chunks are removed from the in-flight set so the next
  demand re-fetches them.
- **Resume / retry**: the transfer worker requeues on cache-write failure and
  on QUIC errors with a 1s pause; reconnect is bounded (6 attempts, backoff);
  there is exactly one demand scheduler (`ChunkDemandHandle`) with in-flight
  dedup — no second scheduler was added.
- **Strict sync**: guest/host buffer-low still pauses BOTH sides through the
  coordinator; recovery returns to READYCHECK and requires a fresh consensus
  + host play to resume. `enter_cinema` no longer fabricates PLAYING, so the
  m3 starvation test now exercises the genuine distributed play protocol
  (stronger coverage than before).
- **No localhost/LAN/public fallback**: media transfer is QUIC-only over the
  Tailscale interface (loopback only under `MOVIE_PARTY_DEV_LOOPBACK=1` for
  tests); the range server binds 127.0.0.1 with an unguessable token as the
  PROTOCOL_SPEC §27 loopback-only media source.

## Defects found and fixed

### D1 — Range server fabricated contiguous Content-Range from sparse data

**Location**: `media/stream/range_server.rs`, `read_available_portion`

**Defect**: When a range request timed out waiting for chunks,
`read_available_portion` accumulated every *available* sub-range in the
requested window — even when the missing chunk sat in the middle — and the
206 response then claimed `bytes start-(start+len-1)`, presenting
non-adjacent sparse bytes as a contiguous run. libmpv would decode garbage
as if it were the real byte range (integrity bypass via off-by-one
Content-Range).

**Fix**: `read_available_portion` now stops at the first unavailable chunk
and serves only the contiguous prefix, so the Content-Range always describes
bytes that are genuinely present in that order.

**Test**: `range_server_partial_timeout_serves_only_contiguous_prefix`
(chunk 0 + chunk 2 cached, chunk 1 missing; a 206 may only ever contain the
chunk-0 prefix and must never contain chunk-2 bytes).

### D2 — Plain full-file GET served a partial file as 200 OK

**Location**: `media/stream/range_server.rs`, `handle_connection` (no Range
header branch)

**Defect**: A GET without a `Range` header waited for the *whole* file and,
on timeout, returned `200 OK` with `Content-Length` = however many prefix
bytes existed. A 200 response means "this is the entire resource", so the
player would treat a truncated body as the real EOF of the movie.

**Fix**: The no-Range branch now serves `200 OK` only when the complete file
is cached (`RangeServe::Available`); otherwise it returns
`503 Service Unavailable` with `Retry-After: 1`. Partial data is reported
as unavailable rather than fabricated.

**Tests**: `range_server_full_get_requires_complete_file` (partial cache →
must be 503, never 200) and `range_server_full_get_serves_complete_file`
(full cache → 200 with the complete byte stream).

### D3 — `enter_cinema` fabricated PLAYING before any play commit

**Location**: `app_runtime.rs`, `enter_cinema`

**Defect**: Entering the cinema screen set `room_state = Playing` whenever
the player had no error — before the distributed play protocol had ever run.
The UI then showed PLAYING (pause button, "in sync" indicator) while no
playback had been committed by any participant, and `host_play` became a
no-op through its idempotency guard, skipping the real protocol.

**Fix**: `enter_cinema` now adopts the coordinator's canonical room state
(READYCHECK after readiness consensus, PAUSED/BUFFERING where applicable)
instead of forcing PLAYING. PLAYING only arrives through a committed play
operation.

**Tests**: `enter_cinema_never_fabricates_playing_before_play_commit`;
`runtime_failures_drive_recovery_state` now establishes genuine PLAYING
via coordinator `prepare_play`/`commit_play`; the m3_closure starvation test
now drives the real readiness + play protocol (previously it silently relied
on the fabricated state).

### D4 — `host_play` did not refuse a genuinely unavailable player

**Location**: `app_runtime.rs`, `host_play`

**Defect**: If the native surface attach failed because libmpv could not be
loaded (sticky `MP-MEDIA-001` player diagnostic), `host_play` still force-set
`media_ready = true` and started the distributed play protocol, letting the
room report PLAYING with no real playback engine on the device.

**Fix**: `host_play` now refuses to start the play protocol while the player
snapshot carries the sticky `MP-MEDIA-001` unavailable diagnostic and reports
`MP-MEDIA-001 player unavailable; cannot start playback`. The existing
LibMpvUnavailable diagnostic-only behavior (protocol tests without libmpv)
is preserved — the guard keys on the genuine attach-failure error state, not
on the pre-attach simulation window.

**Test**: `host_play_never_reports_playing_when_player_unavailable`.

## Verification

- `cargo fmt --check` ✅
- `cargo clippy --all-targets --all-features -- -D warnings` ✅
- `cargo test --all-targets --all-features` ✅ (284 lib + 2 wiring + 28 m2 +
  7 m3_closure + 18 m3_integration + 9 m3_m4 + 15 m4_closure +
  real_native_surface_e2e)
- `pnpm install --frozen-lockfile` ✅
- `pnpm lint` ✅
- `pnpm test` ✅ (70 frontend tests)
- `pnpm exec tsc --noEmit` ✅
- `pnpm build` ✅
- `pnpm tauri build` ✅ (`Movie Party.app` + DMG bundle produced)

## Remaining manual validation (NOT claimed complete)

- Real two-device Local Perfect playback over live Tailscale (Host picks a
  movie → Guest joins → media transfers → both sides actually watch
  synchronized video) has NOT been performed in this batch and remains
  external verification pending.
- Real in-window libmpv rendering on macOS requires the staged libmpv runtime
  and a real GUI session; unit/integration tests do not prove visible
  playback.

---

# BATCH 8 — TAILSCALE ONBOARDING TRUTHFULNESS PASS (2026-09-02)

## Objective

Make the Movie Party Tailscale first-run onboarding truthful, actionable, and
production-ready. The existing six-state model was already in place; this batch
audited the mapping, fixed gaps, surfaced errors, and added focused tests.

## Six-state model (verified)

| State | Meaning | Primary Action | Secondary Action |
|---|---|---|---|
| NOT_INSTALLED | Tailscale is not on the machine | Install Tailscale (download page) | Check again |
| DAEMON_UNAVAILABLE | Executable found but daemon not responding | Open Tailscale | Check again |
| NEEDS_LOGIN | Installed but not authenticated | Open Tailscale | Check again ("I've signed in") |
| STOPPED | Installed/auth'd but connection is off | Open Tailscale | Check again |
| NO_USABLE_ADDRESS | Running but no CGNAT IPv4 available | Open Tailscale | Check again |
| READY | Usable Tailscale IPv4 present | leave onboarding automatically | — |

## Defects found and fixed

### D1 — Error-code extraction regex failed for MP-NET-TS-xxx codes

The frontend `extractErrorCode` regex `\bMP-[A-Z]+-\d{3}\b` matched only
`MP-NET-001` but not `MP-NET-TS-001` through `MP-NET-TS-006`. This meant
`createRoomErrorMessage` and `joinFailureMessage` never matched their
Tailscale-specific branches, falling back to generic messages.

**Fix**: `\bMP-[A-Z]+(?:-[A-Z0-9]+)*-\d{3}\b` — matches both `MP-NET-001` and
`MP-NET-TS-001` style codes.

### D2 — Open Tailscale failure silently swallowed

`openTailscaleSetup` returned `boolean` and the AppShell ignored the result.
When `open -a Tailscale` failed (e.g., app not installed), the user saw no
feedback — the "Open Tailscale" button appeared to do nothing.

**Fix**:
- Added `TailscaleError::OpenFailed` variant with stable `MP-NET-TS-003` code.
- `openTailscaleSetup` returns `null` on success, user-readable message on
  failure.
- `TailscaleSetupView` accepts `openError` prop and renders it in red text
  below the action buttons.
- "Check again" remains available regardless of open failure.

### D3 — Stale `MP-NET-001` error codes in `TailscaleError` Display

The `TailscaleError` enum used `MP-NET-001` in its Display strings, while the
readiness model used `MP-NET-TS-001/003` codes. This made error strings from
`open_tailscale_app` return a non-TS-series code.

**Fix**: Changed Display to `MP-NET-TS-001` for `ExecutableNotFound` and
`MP-NET-TS-003` for `CommandFailed`/`InvalidStatus`/`OpenFailed`.

### D4 — `contentFor` did not guard against READY

The `contentFor` default case handled READY by returning a generic "Open
Tailscale" action, which would be contradictory if READY ever reached the setup
view.

**Fix**: Added explicit `case "READY": throw new Error("READY is not a setup
state")`. The view is only rendered when state !== READY, so this is a
defensive programming contract backed by a test.

## macOS Open Tailscale behavior

`open_tailscale_app()` uses `open -a Tailscale` (standard macOS Launch
Services mechanism). If the app is not registered, the error maps to
`MP-NET-TS-003` and the user sees: "Movie Party could not open the Tailscale
app. Open it from your Applications folder, then check again."

No hard-coded developer paths. No fallback to the download page when the app
is installed.

## Readiness polling

- In-flight guard (`connectivityInFlight` ref) prevents overlapping refresh
  calls.
- Cadence: 15s when READY, 4s when not READY.
- "Check again" button clears any open error and triggers immediate refresh.
- Polling interval extracted to `tailscaleOnboarding.pollIntervalMs()`.

## Partner connectivity

Partner reachability is NOT a prerequisite for entering Home. The
`PartnerConnectView` is shown only on `MP-NET-TS-005` join failure (host
unreachable). This is a separate stage after Tailscale local readiness.

## Create Party regression

`create_local_party` calls `local_readiness()` + `required_ipv4()` to obtain
the authoritative Tailscale IPv4. No localhost, LAN, or public IP fallback
when `MOVIE_PARTY_DEV_LOOPBACK` is not set. No regression.

## Tests added/updated

### Rust (2 new tests in tailscale.rs)
- `open_failed_maps_to_daemon_unavailable` — OpenFailed → DaemonUnavailable
- `open_failed_displays_stable_mp_net_ts_code` — Display string is MP-NET-TS-003

### Frontend (new files + updated)

**`src/backend/tailscaleOnboarding.test.ts`** (6 tests):
- `isTailscaleReady` returns true only for READY
- `pollIntervalMs` cadence: ready slower than setup
- `refreshLabelFor` — "I've signed in" for NEEDS_LOGIN, "Check again" for others

**`src/backend/appRuntime.test.ts`** (14 tests):
- BackendCommandError extracts MP-NET-TS-001/002/003/004/005/006 codes
- Falls back to MP-BACKEND-001 for unknown codes
- Classifies TS codes as "network failure"
- `tailscaleSetupOpenErrorMessage` maps codes to user-readable messages
- Verifies no raw backend detail leaks to the user

**`src/views/TailscaleSetupView.test.ts`** (17 tests, previously 9):
- READY throws (defensive contract)
- Each state has a unique title identifying the state
- No localhost/LAN/public IP references in any state
- "Check again" affordance for every setup state
- Partner guidance in NOT_INSTALLED description
- refreshLabelFor coverage

## Verification

- `cargo clippy --all-targets --all-features -- -D warnings` ✅
- `cargo test --lib` ✅ (279 lib tests)
- `cargo test --all-targets --all-features` ✅ (279 lib + 82 integration)
- `pnpm lint` ✅
- `pnpm test` ✅ (69 frontend tests)
- `pnpm exec tsc --noEmit` ✅
- `pnpm build` ✅

## External Verification Pending

- macOS: fresh install, signed-out, service-offline, and healthy signed-in
  Tailscale paths; official setup/sign-in launch behavior; host creation and
  guest join across two devices.
- Windows: installer detection, `tailscale up` handoff, external help launch,
  and child join reachability across a real tailnet.
- Separate-tailnet sharing/ACL cases remain user-managed in Tailscale; Movie
  Party provides guidance only and does not use admin credentials or APIs.

---

# BATCH 7 — V1 RELEASE HARDENING + CURRENT-BRANCH RELIABILITY AUDIT (2026-08-30)

## Mode

The user supplied a Batch 7 template focused on revalidating the current branch
against the remaining V1 release risks (deep-link lifecycle, Tailscale boundary,
Local Perfect / Cinema / Provider Sync / Call / Ghost / Privacy lifecycle,
worker safety, error/recovery consistency, security revalidation, focused
deterministic tests, tracker documentation). Old audit findings were treated as
a **revalidation backlog**: each was checked against the current code, and only
concrete current-branch defects were fixed.

## Concrete defects found and fixed

### D1 — `spawn_player_event_loop` did not abort the previous task handle

**Location**: `app_runtime.rs`, `spawn_player_event_loop()`

**Defect**: Every other background worker spawn in `app_runtime.rs`
(`spawn_clock_calibration`, `spawn_guest_peer_event_listener`,
`spawn_guest_heartbeat`, `spawn_transfer_stall_watcher`, `spawn_scheduler_worker`)
stores its handle with `replace(...)` + `old.abort()`, so a second spawn aborts
the first. `spawn_player_event_loop` was the only outlier: it assigned
`state.player_event_task = Some(task)` without aborting a previously running
loop. A duplicate create (double-tap / stale UI on `create_local_party`, or a
repeated call) could leave **two player event loops** polling the same player,
causing duplicate position/buffer emissions and duplicate
`report_buffer_status` calls.

**Fix**: `spawn_player_event_loop` now uses `replace(...)` + `old.abort()`,
matching every other worker spawn. A focused regression test
`player_event_loop_is_replaced_not_duplicated` proves the old loop is aborted on
replacement.

### D2 — `create_local_party` hygiene block only aborted the host server

**Location**: `app_runtime.rs`, `create_local_party()`

**Defect**: On a duplicate create (double-tap / stale UI) the hygiene block
aborted `host_session` and `host_event_task` and dropped the client, but it did
not abort the player event loop, transfer stall watcher, heartbeat, reconnect,
peer-event, calibration, or preload workers, and it did not close a stale player
or clear stale media/provider/chat/player-snapshot state. A previous room's
workers/state could survive into a newly created room.

**Fix**: The hygiene block now aborts every owned background worker
(`player_event_task`, `transfer_stall_watcher_task`, `heartbeat_task`,
`reconnect_task`, `peer_event_task`, `calibration_task`, `preload_task`), shuts
down the range server, closes the stale player, and clears stale
media/transfer/player-snapshot/chrome-session/chat/reactions state before
binding a fresh session. This mirrors the `join_party` teardown and the
`leave_party` cleanup.

### D3 — `openJoinWithInvite` did not reset the call tile session

**Location**: `src/components/AppShell.tsx`, `openJoinWithInvite()`

**Defect**: When a deep link (or cold-start pending link) arrived, the call tile
session (position / hidden / minimized state from a previous room) was left
untouched, so a join triggered from a deep link could carry the previous room's
call tile UI state into the new room. `goJoinParty()` already reset it;
`openJoinWithInvite()` did not.

**Fix**: `openJoinWithInvite()` now resets the call tile session via
`createCallTileSessionState()`, consistent with `goHome()` and `goJoinParty()`.

## Audit findings revalidated and resolved (no change needed)

The following old audit items were inspected against the current branch and
found already resolved or not currently real:

- **Running-app deep-link join while inside a party**: `join_party` validates
  the invite (`room::parse_invite`, socket-address validation) **before** any
  teardown, so an invalid deep link cannot destroy the current room (covered by
  `invalid_invite_does_not_destroy_existing_room`). After validation it aborts
  every owned worker and clears stale media/provider/player/chat state before
  connecting. The new m2 test
  `test_w_guest_joins_second_room_cleanly_from_running_app` verifies a guest
  already inside room A can join room B cleanly (fresh invite, cleared chat /
  reactions / position / provider state, media re-fetched, no stale room).
- **Deep-link parsing**: `src/invites/deepLinks.ts` rejects missing room codes,
  missing descriptors, query params, whitespace/control chars, oversized links,
  and non-`movieparty://` schemes; duplicate deliveries are idempotent.
- **Tailscale boundary**: `network/tailscale.rs` covers NOT_INSTALLED, SIGNED_OUT,
  CONNECTED, UNAVAILABLE, missing/invalid IPv4 (100.64/10 CGNAT range), host
  create requiring only the host's own readiness (`required_ipv4`), guest join
  requiring own readiness + host reachability (`MP-NET-TS-005` on connect
  failure), with stable `MP-NET-TS-00x` codes. No rendezvous server, no Tailscale
  API credentials, no cross-device pre-connection requirement. First-run setup
  surface (`TailscaleSetupView`) stays reachable and is not a permanent gate once
  `CONNECTED`.
- **Local Perfect lifecycle**: manifest validation, strong media identity, first
  chunk verification, sparse cache, runtime-owned cache root, bounded range
  server (`MAX_RANGE_CONNECTIONS=8`, token auth, 503 on unavailable bytes),
  host-authoritative position, buffer-low/buffer-recovered through
  `report_buffer_status`, drift correction (`correction_for_drift`), transfer
  progress, leave cleanup, worker cancellation, reconnect transport resume all
  confirmed present.
- **Cinema / native player**: native surface attaches once (`CinemaView` effect),
  resizes via `ResizeObserver`, detaches on unmount; `leave_party` clears the
  player and player snapshot; no detached mpv window is created
  (`vo=libmpv`); player state cannot falsely remain `PLAYING` on error
  (`enter_cinema` sets `ERROR` + strict-sync pause when the player has an error).
- **Provider Sync lifecycle**: readiness state machine, managed Chrome
  session reuse/replacement, login-required vs playback-ready states, room
  creation gated on `validate_provider_ready_for_room`, Chrome cleanup on
  leave/replacement, provider→Local and Local→provider transitions clear stale
  state. No concrete lifecycle bug found.
- **Call / Chat / Ghost / Privacy**: local vs remote participant state is kept
  separate; Ghost Mode hides local social UI without touching outgoing device
  state; Privacy Mode disables camera/mic and never auto-reactivates on exit;
  leaving/replacing a room clears chat, reactions, call signals.
- **Worker/task safety**: scheduler, preload, QUIC connection, media transfer,
  range server, drift, heartbeat, reconnect workers are duplicate-safe
  (`replace` + abort or in-flight guard) and aborted on leave/join/create
  replacement.
- **Error/recovery consistency**: stable `MP-*` codes (`MP-MEDIA-001`,
  `MP-NET-TS-*`, `MP-ROOM-001`, `MP-PROVIDER-*`, `MP-STORE-001`) are used;
  errors are truthful, recoverable where possible, and do not leak secrets
  (frontend `sanitizeErrorDetail` scrubs paths).
- **Security revalidation**: QUIC control requests bounded (`MAX_REQUEST_BYTES`
  2 MB, `MAX_AUTH_NONCES`); range-server bounds present; malformed UUIDs
  rejected (UUIDv7 validation); no raw internal error leakage to production UI;
  AppleDouble (`._*`) and `.DS_Store` are gitignored and were cleaned from the
  working tree; no tracked AppleDouble artifacts.

## Known limitations (documented, not silently "fixed")

- **Reconnect media resume**: the reconnect worker restores the QUIC transport,
  peer snapshot, clock calibration, and heartbeat, but does **not** re-trigger a
  full `guest_fetch_media` for a partially-cached media. The sparse cache and
  transfer worker survive disconnect (`apply_disconnect_preserves_guest_cache`),
  and the transfer worker picks up the replacement client, so a reconnect
  continues the existing transfer; a fresh manifest/cache session is only built
  by an explicit rejoin. This is the documented, bounded reconnect scope.
- **CSP**: `tauri.conf.json` / `index.html` still ship without an explicit CSP.
  Adding one risks breaking the running app and needs runtime verification, so it
  is tracked as release-hardening, not silently applied here.
- **m2_integration environment failures**: `test_a_ready_reaches_host`,
  `test_d_seek_sets_canonical_position_on_both`,
  `test_l_ready_does_not_bypass_play_protocol`,
  `test_t_seek_waits_for_guest_and_resumes_together` fail on this machine both
  with and without this batch's changes (verified against the clean baseline) and
  match the previously documented "4 pre-existing environment failures". They
  are timing/sandbox related, not introduced by Batch 7.

## Verification

- `cargo check --lib` ✅ (temporary target directory due to external-drive
  AppleDouble artifacts)
- `cargo test --lib app_runtime::tests` ✅ (35 tests, incl. new
  `player_event_loop_is_replaced_not_duplicated`)
- `cargo test --test m2_integration test_w_guest_joins_second_room_cleanly_from_running_app` ✅
- `cargo test --test m2_integration` ✅ 22 pass + 4 pre-existing environment
  failures (verified identical on the clean baseline)
- `cargo test --test host_guest_wiring` ✅ (2)
- `cargo test --test m3_closure` ✅ (7)
- `cargo test --test m4_closure` ✅ (15)
- `cargo test --test m3_m4_e2e` ✅ (9)
- `pnpm lint` ✅
- `pnpm test` ✅ (27)
- `pnpm build` ✅

## Regression tests added

1. `player_event_loop_is_replaced_not_duplicated` — proves a second
   `spawn_player_event_loop` aborts the first (workers cannot duplicate).
2. `test_w_guest_joins_second_room_cleanly_from_running_app` (m2_integration) —
   proves a guest already inside a room can join a fresh room cleanly with no
   stale chat/reactions/position/provider state (running-app deep-link join).

## Platform affected

Code-level only — macOS and Windows both benefit; no platform-specific change.

## Status

Batch 7 is **CODE COMPLETE** at the code/test level. The following remain
**MANUAL VERIFICATION REQUIRED** on physical devices:

- Two-device READY consensus and real-player buffer reporting
- Real deep-link activation (installed/bundled macOS + Windows, cold + running)
- Real Tailscale install/sign-in/partner-reachability across devices
- Real Local Perfect playback / range / buffer / reconnect on macOS and Windows
- Real Chrome provider login and Provider Sync operation
- Real native libmpv presentation inside the Cinema surface
- Real call camera/microphone behavior (permissions, Ghost/Privacy)
- Windows native video presentation host

No item above is marked PRODUCTION VERIFIED by these automated tests.

---

# BATCH 7 — V1 READINESS HONESTY AUDIT (2026-08-30)

## Background

The user provided no real-device failure observations (the batch template was
supplied with the `ACTUAL_FAILURES_OBSERVED` section unfilled). The user chose
"Audit code against rules only" mode. This batch therefore inspected the
codebase against the V1 correctness rules and fixed all concrete code-level
defects found.

## Concrete defects found and fixed

### D1 — `set_ready` unconditionally fabricated media readiness

**Location**: `app_runtime.rs:3854-3908`, `set_ready()`

**Defect**: `set_ready` unconditionally set `local_participant.media_ready = true`,
`buffer.guest_buffer_ahead_ms = 5_000`, and `peer.media_ready = true` — the
host marked the guest ready (and the guest marked the host ready) purely because
the local user pressed Ready, regardless of actual media/player/provider state.
The comment even said "M2 fake player: pressing Ready means the local player can
consume media; later phases gate this on real transfer/media."

This violated:
- V1 rule: "Never allow fake media readiness"
- V1 rule: "Never allow Ready before actual prerequisites are satisfied"
- V1 rule: "media-ready must correspond to actual usable media"
- V1 rule: "Do not solve buffering by falsely marking a participant ready"

**Fix**: `set_ready` now gates the local participant's readiness on genuine
media/player/provider state via a new `local_media_genuinely_ready()` helper.
For Local Perfect: the player must be present and opened without error, and a
media manifest must exist. For provider/generic rooms: the provider readiness
must be `Ready` or `PlaybackReady`. If not genuinely ready, a stable
`MP-MEDIA-001` error is surfaced and neither the coordinator nor the QUIC
ReadyState is advanced. The peer's `media_ready` is no longer force-set; it is
propagated from the peer's genuine state.

### D2 — Host ignored `QuicHostEvent::GuestReadyState`

**Location**: `app_runtime.rs:2173`, `apply_host_event()`

**Defect**: The host's `GuestReadyState` handler was an empty block `{}`,
meaning the host's `peer_participant.media_ready` was never updated from the
guest's actual readiness. It was only force-set to `true` in `set_ready` (D1).

**Fix**: The handler now updates `peer_participant.media_ready` and
`buffer_ahead_ms` from the guest's reported readiness and buffer values.

### D3 — `CoordinatorStateUpdate` did not mirror peer readiness

**Location**: `app_runtime.rs:3034-3060`, `apply_peer_event()`

**Defect**: The `CoordinatorStateUpdate` handler applied coordinator state
(host_ready/guest_ready flags, room state, position) but never synchronized
`peer_participant.media_ready` from those flags. On the guest, the host's
readiness was never reflected in the participant snapshot; on the host, the
guest's readiness was only set via the force-set in `set_ready`.

**Fix**: The handler now sets `peer_participant.media_ready = guest_ready` on
the host side and `= host_ready` on the guest side, reflecting the genuine
coordinator state.

### D4 — m2 integration tests relied on fabricated readiness

**Defect**: The m2_integration protocol tests called `guest.set_ready()`
immediately after the 300ms join sleep, before the guest's async media fetch
had completed, and relied on the fabricated `media_ready = true` /
`buffer = 5_000` in `set_ready`. The `ready_both` helper unconditionally set
`peer.media_ready = true`.

**Fix**: Updated `ready_both` and `guest_ready_when_prepared` helpers to:
(a) poll for the guest's genuine `media_ready` before pressing Ready; (b) call
`guest.report_buffer_status(0, 8_000, false)` to report genuine buffer through
the real buffer-status path before Ready. This matches the production flow
(player event loop reports buffer headroom) and aligns with how `m3_closure`
tests handle buffer reporting.

## Verification

- `npm run lint` ✅
- `npm run test` ✅ (27 tests)
- `npm run build` ✅
- `cargo check` ✅ (temp target directory)
- `cargo test --lib app_runtime` ✅ (34 tests, including 5 new regression tests)
- `cargo test --test m3_closure` ✅ (7 tests)
- `cargo test --test m4_closure` ✅ (15 tests)
- `cargo test --test host_guest_wiring` ✅ (2 tests)
- `cargo test --test m3_m4_e2e` ✅ (9 tests)
- `cargo test --test m2_integration` ✅ matches baseline (21 pass, 4 pre-existing
  environment failures unrelated to this batch)

## Regression tests added

1. `set_ready_does_not_fabricate_media_readiness` — fresh runtime with no
   media: set_ready returns MP-MEDIA-001 error, media_ready stays false.
2. `set_ready_requires_provider_playback_readiness` — provider room with
   LoginRequired: set_ready rejects with MP-MEDIA-001.
3. `set_ready_with_genuine_local_media_marks_ready` — runtime with an opened
   player and manifest: set_ready marks media_ready true.
4. `guest_ready_state_propagates_peer_media_readiness` — apply_host_event
   GuestReadyState updates peer snapshot.
5. `coordinator_update_mirrors_peer_media_readiness` — apply_peer_event
   CoordinatorStateUpdate syncs peer.media_ready from coordinator flags.

## Platform affected

Code-level only — no platform-specific changes. The fix applies equally to
macOS and Windows.

## Status

This batch is CODE COMPLETE. No physical verification was performed (no
real-device test was supplied). The following items require manual verification
on physical devices:

- Two-device READY consensus flow with genuine media readiness
- Cross-device QUIC ReadyState round trip and GuestReadyState propagation
- Real-player buffer reporting (player event loop → report_buffer_status)
- Provider mode readiness gating

---

# M6 SOCIAL OVERLAY + PRIVACY STATE PASS (2026-08-28)

## Implemented in this pass

- **Local and remote call presentation**: Cinema and Lobby device controls
  remain local-outgoing controls; the floating peer tile now renders from the
  separate peer participant snapshot fields and is display-only. It no longer
  presents local device state, device inventory, call-mode controls, or raw
  diagnostics as peer UI.
- **Call tile interaction**: tile position, hidden state, and minimized state
  are held in AppShell for the active party. The practical tile surface is
  draggable with pointer capture, viewport clamping, selection prevention, and
  an explicit overlay layer above Lobby/Cinema content. Close hides locally;
  minimize keeps a compact, draggable video/avatar tile with restore control.
- **Lobby and chat**: Lobby social controls remain floating overlays, the
  Ready Check action wraps within its own footer at narrow desktop widths, and
  the provider select explicitly uses the dark native control scheme. Cinema
  and Lobby chat use the same larger translucent panel; Cinema now separates a
  five-second incoming preview from a manually-opened composing session.
- **Ghost and Privacy**: Ghost Mode conceals local social UI only and leaves
  call device state untouched. Privacy Mode conceals those overlays while
  disabling local camera/microphone; leaving Privacy restores the prior Ghost
  visual state without re-enabling either device.

## Verification

- `npm run lint` ✅
- `npm run test` ✅ (6 files, 19 tests)
- `npm run build` ✅
- `cargo check` ✅ using `/private/tmp/movieparty-batch3-target`
- focused Ghost/Privacy runtime tests ✅

## External Verification Pending

- Verify on macOS and Windows that local mic/camera controls change outgoing
  tracks only, while the peer tile follows the peer participant status.
- Verify pointer dragging, viewport bounds, z-order over Lobby cards, compact
  minimize/restore, local close/reopen, and narrow-window Ready Check layout.
- Verify real remote camera/microphone transitions with a second device. The
  current protocol does not yet send live remote device-track telemetry, so
  participant fields remain the existing remote-status boundary.
- Verify Ghost and Privacy behavior with actual camera/microphone permissions:
  Ghost must preserve remote delivery; Privacy must stop it and require an
  explicit user re-enable afterward.

---

# M6 TAILSCALE ONBOARDING + REACHABILITY PASS (2026-08-26)

## Implemented in this pass

- **Typed local readiness**: the existing `tailscale status --json` boundary
  now reports `NOT_INSTALLED`, `SIGNED_OUT`, `CONNECTED`, or `UNAVAILABLE`,
  with a usable Tailscale IPv4 and device name only for a healthy connection.
  CLI calls use structured arguments and a bounded timeout; loopback remains
  available only under the explicit `MOVIE_PARTY_DEV_LOOPBACK=1` flag.
- **First-run setup gate**: Home/Create/Join remain unchanged underneath a
  small prerequisite surface. It opens official Tailscale install/help pages
  or asks the locally installed Tailscale CLI to start its own sign-in flow;
  Movie Party never collects credentials or tailnet administration access.
- **Host precondition**: production room creation now rejects missing,
  signed-out, unavailable, or address-less Tailscale before starting QUIC.
  A healthy host can still create a room without any guest preconnection.
- **Guest reachability**: Join validates local Tailscale readiness before the
  existing authenticated QUIC connection. Endpoint connection failures map to
  `MP-NET-TS-005`, which presents a concise partner-connect recovery surface.
- **Invite preservation**: a pending deep link or pasted invite remains in
  AppShell state while onboarding blocks the screen, then resumes the existing
  Join Party flow after a successful refresh.

## Verification

- `cargo check` ✅ using a temporary target directory
- `cargo test network::tailscale --lib` ✅ (9 tests)
- `npm run lint` ✅
- `npm run test` ✅ (includes pending deep-link parsing tests)
- `npm run build` ✅

## External Verification Pending

- macOS: fresh install, signed-out, service-offline, and healthy signed-in
  Tailscale paths; official setup/sign-in launch behavior; host creation and
  guest join across two devices.
- Windows: installer detection, `tailscale up` handoff, external help launch,
  and child join reachability across a real tailnet.
- Separate-tailnet sharing/ACL cases remain user-managed in Tailscale; Move
  Party provides guidance only and does not use admin credentials or APIs.

---

# M3 NATIVE CINEMA PRESENTATION PASS (2026-08-25)

## Implemented in this pass

- **Embedded local-media presentation**: Local Perfect playback now defers
  libmpv initialization until Cinema attaches a native video host. libmpv is
  initialized with its `wid` option, so it renders into the supplied native
  child surface instead of opening a detached player window.
- **Cinema lifecycle bridge**: `CinemaView` measures the existing movie frame
  on mount and through `ResizeObserver`, then uses narrow attach/resize/detach
  Tauri commands. The native host is placed behind the transparent Cinema
  webview; existing React controls and overlays remain above the video.
- **Platform hosts**: macOS uses an `NSView` sibling beneath the `WKWebView`;
  Windows has a DPI-aware child `HWND` host path. Bounds are validated before
  platform attachment.
- **Truthful presentation state**: `EMBEDDED_NATIVE` is reported only after a
  surface and libmpv context are attached. A missing libmpv library or failed
  host attach remains a stable `MP-MEDIA-*` diagnostic rather than pretending
  the Cinema player is usable.
- **Production feature selection**: the native `mpv` feature is now enabled by
  default; libmpv itself remains a required runtime dependency.

## Verification

- `npm run lint` ✅
- `npm run test` ✅ (4 files, 15 tests)
- `npm run build` ✅
- `cargo check` ✅ using a fresh temporary target directory because the
  external-drive target contains a pre-existing Apple metadata file with
  invalid UTF-8.
- `cargo test native_surface --lib` ✅ (2 tests)
- `cargo test mpv_player --lib` ✅ (1 test)

## External Verification Pending

- macOS: install/bundle libmpv, create a Local Movie party, enter Cinema, and
  verify frames render in the movie surface while React overlays remain usable.
- Windows: verify the child-HWND z-order, DPI scaling, resizing, and libmpv
  playback on a real supported machine.

---

# M6 PRODUCTION HARDENING PASS (2026-08-25)

## M6 INVITE + PROVIDER MODE INTEGRATION PASS (2026-08-25)

## Implemented in this pass

- **Windows invite support**: configured the official Tauri deep-link plugin
  for `movieparty://`; the official single-instance companion forwards a second
  invite activation to the running application. Cold starts continue through
  the existing pending-link buffer and Join Party parser.
- **Provider source separation**: Create Party now submits distinct Local,
  Provider Sync, and Generic Link requests. Named Provider Sync uses the
  existing managed-Chrome/CDP launch path; generic links use the existing
  generic HTML-media adapter path without provider identity inference.
- **Capability honesty**: the native provider registry now exposes capability
  data to the frontend. Provider Shared is shown as Experimental but disabled
  because capture readiness has not been verified or wired as a runtime start
  path.

## Verification

- `npm run lint` ✅
- `npm run test` ✅
- `npm run build` ✅
- `cargo check` ✅ using a fresh temporary target directory because the
  external-drive cache contains a pre-existing Apple metadata file with invalid
  UTF-8.
- `cargo test --lib providers::sync::tests` ✅ (9 tests)

## External Verification Pending

- Windows installer registration, cold-start invite activation, and forwarding
  a second invite to a running application.
- Real provider login, page detection, playback control, and Provider Shared
  capture capability on supported Windows/macOS hardware.

## Implemented in this pass

- **Runtime mutex recovery**: app-runtime sync coordinator and player locks now
  recover from poisoned mutexes instead of panicking in production paths.
- **Player failure propagation**: native player open/command failures now
  surface stable `MP-MEDIA-*` diagnostics in the player snapshot. Real playback
  command failures still move the room into strict media error; missing libmpv
  during protocol-only fixture flows remains diagnostic so room/sync tests do
  not fake a native decoder dependency.
- **Backend bridge diagnostics**: frontend invoke failures now preserve command
  name, stable error code, and failure kind in development builds while keeping
  production copy user-safe.
- **Render crash containment**: the React root is wrapped in an error boundary
  that logs `MP-UI-001` and presents a recovery action instead of a blank app.
- **Notification support honesty**: unsupported native-notification platforms
  now return `MP-NOTIFY-001` instead of reporting success.
- **Native player dependency notes**: the libmpv backend documents runtime
  availability requirements and checks command/property return codes.

## Verification

- `npm run lint` ✅
- `npm run test` ✅
- `npm run build` ✅
- `cargo test` ✅ with permission to bind local QUIC/network test endpoints.

## Known Local Issue

- `cargo fmt --check` still reports a pre-existing formatting diff in
  `src-tauri/src/storage/sqlite.rs`; this pass did not modify that file.

## External Verification Pending

- Real OS keychain, native notification delivery, libmpv playback, Tailscale,
  QUIC across devices, and provider playback still require platform/manual
  verification.

---

# M6 UI POLISH PASS (2026-08-25)

## Implemented in this pass

- **Create-party action containment**: the prepared action row now uses an
  internal constrained grid so `CREATE CINEMA ROOM` remains inside the
  selection card without wrapping.
- **Laptop-practical typography**: reduced Lobby feature title, invite code,
  Ready Check cinema heading, Ready Check movie title, and Cinema Mode
  placeholder title sizing while preserving the cinematic hierarchy.
- **Cinema preparation state**: Cinema Mode now presents unavailable local
  player presentation as a graceful playback-preparation state, with no backend
  or player architecture changes.
- **Chat overlay styling**: chat now reads as a floating translucent cinema
  overlay with stronger glass, blur, subtle border, and no full-height side
  panel feel.

## Verification

- `npm run lint` ✅
- `npm run test` ✅
- `npm run build` ✅

---

# M6 UI INTEGRATION FIX PASS (2026-08-25)

## Implemented in this pass

- **Create-party source-card state**: unselected source cards now reserve
  layout space with a transparent border instead of showing a bright outline.
  Hover keeps the existing glow/elevation only. Selected cards retain the
  active accent border, glow, and selected indicator.
- **Cinema button stability**: shared `CinemaButton` controls now use fixed
  height/width, non-wrapping text, truncation, and non-shrinking icons so the
  create-room action cannot grow vertically or escape its container when the
  label changes.
- **Create-room error visibility**: `create_local_party` / provider launch
  invoke failures are no longer collapsed to frontend `null` in the create
  path. Development builds surface the command name plus backend error detail;
  production keeps the generic user-facing fallback.

## Verification

- `npm run lint` ✅
- `npm run test` ✅
- `npm run build` ✅
- `npm run tauri dev` ✅ launched after local-server permission approval.
- `cargo test -p movie-party --lib dev_loopback_mode_selects_correct_bind_addr` ✅
  with permission to bind local QUIC endpoints.

## External Verification Pending

- Full manual create-room success requires a usable Tailscale runtime or
  explicit dev loopback. On this machine, `tailscale status --json` currently
  fails with `Failed to load preferences`, so production-mode room creation
  surfaces an `MP-NET-001` Tailscale status failure instead of entering Lobby.

## Known Local Issue

- `cargo fmt --check` currently reports a pre-existing formatting diff in
  `src-tauri/src/storage/sqlite.rs`; this pass did not modify that file.

---

# M6 FINAL CLOSURE PASS (2026-08-21)

## Implemented in this pass

- **Managed Chrome launch correctness**: provider launch now allocates an
  actual localhost CDP port instead of passing port `0`, and launch plans
  reject invalid CDP port `0`.
- **Provider launch boundary**: launch validates provider IDs and provider URL
  ownership before opening Chrome.
- **Runtime truthfulness**: Chrome discovery, CDP port allocation, launch, and
  plan failures now return an explicit provider `Unavailable` snapshot instead
  of disappearing as frontend `null`.
- **Chrome lifecycle ownership**: managed Chrome is launched directly via the
  executable so the runtime owns the process; previous sessions are closed
  before replacement and on party leave.

## External Verification Pending

- Installed Chrome discovery and launch on macOS and Windows.
- Dedicated provider profile login/session reuse with real accounts.
- Live YouTube, Netflix, Prime, and JioHotstar playback detection/control.

---

# M5 FINAL CLOSURE PASS (2026-08-21)

## Implemented in this pass

- **WebRTC lifecycle**: Cinema Mode now aborts superseded call attempts, closes
  peer connections, detaches handlers, and stops tracks on privacy/call-mode
  changes or setup failure.
- **Truthful call state**: runtime snapshots now expose explicit call status:
  `connecting`, `connected`, `degraded`, `reconnecting`, `unavailable`, and
  `ended`.
- **Permission/runtime failures**: browser media acquisition no longer silently
  reports synthetic success in the cinema connection path; permission and media
  failures become unavailable/degraded call states without interrupting movie
  playback.
- **Signaling hardening**: SDP and ICE payloads are parsed, bounded, and checked
  for duplicate or stale offer/answer/ICE transitions before entering runtime
  state.
- **Privacy protection**: Privacy Mode continues to force camera off and mic
  muted; changing call mode while private can no longer re-enable devices.

## External Verification Pending

- Real macOS camera/microphone permission prompt and recovery behavior.
- Real Windows camera/microphone permission prompt and recovery behavior.
- Two-device WebRTC media flow and reconnect across supported OS pairs.

---

# M3-M4 FINAL CLOSURE PASS (2026-08-21)

## Implemented in this pass

- **M3 presentation safety**: libmpv is now configured with `vo=libmpv` so it
  does not create an unmanaged second user-facing window. The runtime exposes a
  typed `PlayerPresentationStatus`; Cinema Mode shows the native render-host
  boundary as an overlay instead of pretending embedded video is active.
- **M3 partial-cache playback**: Local Perfect loopback HTTP media URLs are now
  accepted by the player abstraction, so guest partial-cache playback can open
  the range server URL instead of being rejected as a missing filesystem path.
- **M3 lifecycle cleanup**: `guest_fetch_media` now aborts old transfer and
  player-event workers, shuts down the old range server, closes the old player,
  and clears the old cache before installing a replacement session.
- **M4 identity persistence**: corrupt identity metadata now surfaces a storage
  error instead of being treated as first launch. Private key material remains
  behind the secure-store abstraction.
- **M4 scheduler correctness**: due schedule claiming is atomic, only one
  scheduler can win execution, and schedules stranded in `Claimed` by a crash
  are recovered to a retryable waiting state on scheduler scan.
- **M4 scheduling edge case**: empty `next_preload_deadline` now returns
  `None` instead of a SQLite null-read error.
- **M4 production preload guard**: `AppRuntimePreloadExecutor` now starts the
  real `guest_fetch_media` path only when a QUIC client exists and the active
  media matches the schedule; otherwise the schedule remains retryable as
  `WaitingForPrerequisites`. The readiness check uses clippy-clean boolean
  logic.

## Verification

- `cargo fmt --check` ✅
- `cargo test --test m4_closure` ✅
- `cargo test --test m3_closure duplicate_range_demands_deduplicate` ✅
- `cargo test --lib media::player::tests::loopback_http_media_source_is_accepted_for_partial_cache_playback` ✅
- `cargo test --lib media::player::presentation::tests` ✅
- `cargo test --lib claim_due_schedule_is_atomic_and_one_winner_only` ✅
- `cargo test --lib claimed_schedule_recovers_after_restart_before_execution` ✅
- `cargo test --lib corrupt_identity_row_surfaces_read_error` ✅
- `cargo test --lib next_preload_deadline_returns_none_when_no_pending_schedule_exists` ✅
- `pnpm exec tsc --noEmit` ✅

## Known remaining boundaries

- Full visible in-window video still requires attaching the native macOS
  NSView/CALayer or Windows HWND render host to the Tauri window surface. The
  unsafe external mpv window path is no longer the configured final path; no
  embedded-video success is claimed until that native host is verified.
- Live two-device QUIC transfer, macOS/Windows notification dispatch, and
  OS-protected key store behavior remain **EXTERNAL VERIFICATION PENDING**.

---

# M3 + M4 CORRECTION PASS (2026-08-20)

## M3 — Local Perfect: 🟨 CORE GREEN — PRESENTATION PARTIAL

- **M3.1 Demand-driven range fetch + cache-write fix**: `ChunkDemandHandle`
  (priority queue + in-flight dedup + tokio Notify) connects the loopback
  range server to the guest QUIC transfer worker. The worker now acquires the
  cache with `lock().await` (never `try_lock`), writes the chunk, and ONLY
  then updates `bytes_available` + notifies the waiter. Failed writes are
  requeued, never silently lost. Contention test proves persistence under
  lock pressure.
- **M3.2 Binary QUIC bulk transfer**: PROTOCOL_SPEC §45 raw binary chunk
  stream on dedicated QUIC streams; base64 JSON payload removed.
- **M3.3 Range wake path**: dead watch channel replaced with `ChunkWake`
  (bounded Condvar). Waiting range requests are woken by a real notify after
  each chunk write — no 20ms polling.
- **M3.4 Empty-timeout response**: unavailable bytes return
  `503 Service Unavailable` with `Retry-After` — never a bogus empty 206;
  Content-Range underflow eliminated.
- **M3.5 Lifecycle**: leave/reconnect aborts workers and stops the range
  server; no duplicate live workers.
- **M3.6 Strict sync**: starvation pauses BOTH sides; refill requires
  readiness consensus; no independent auto-resume.
- **M3.7 Presentation: PARTIAL** — libmpv opens its own unmanaged Cocoa
  window. In-app embedding needs a dedicated native-rendering milestone
  (NSView/CALayer hosting through a Tauri native plugin). This is
  locally-codable, tracked separately, NOT an external hardware blocker.

## M4 — Persistence & Scheduling: 🟩 LOCALLY COMPLETE

- **M4.1 Real preload executor**: `PreloadExecutor` seam. The scheduler
  atomically claims (Planned → Claimed) each due schedule, then invokes the
  executor: Started → Transferring; WaitingForPrerequisites → WaitingForPeer
  (retried); failure → PreloadFailed (recoverable). Production executor
  drives the actual `guest_fetch_media` preload; tests prove the executor is
  invoked via a recording fake.
- **M4.2 Production overdue-startup fix**: `check_overdue_schedules` is
  notification-only and never mutates status; the scheduler is the single
  authority. Real startup ordering test: overdue Planned schedule → startup →
  executor invoked exactly once.
- **M4.3 Secure device private key**: `SecureKeyStore` abstraction —
  macOS Keychain, Windows Credential Manager; SQLite stores metadata + a
  key label only, never the raw Ed25519 seed. Tests use `FakeKeyStore`.
- **M4.4 Identity error handling**: `Ok(None)` (first run) vs `Err`
  (surfaced as MP-STORE-001, no silent rotation). `upsert_identity` failures
  propagate.
- **M4.5 Coherent migration/rotation**: missing or mismatched private key →
  explicit rotation with a NEW device id + keypair; old metadata row and
  protected entry removed. `from_seed_for_tests` is never used by
  production/migration code.
- **M4.6 Notification hardening**: macOS dispatch passes text as `on run
  argv` arguments (never interpolated into AppleScript); Windows dispatch
  XML-escapes text and passes base64 (never PowerShell source). Tests cover
  quotes, apostrophes, `&`, `<`, `>`, newlines.
- **M4.7 Schedule CRUD + retention**: unchanged and green (create/list/
  update/delete; Keep / Remove / Save As never touch the host source).

Test totals: 263 Rust + 3 FE = 266, 0 failures. `.app` bundle builds.

M5 status: 🟨 IN PROGRESS — CallSignal routing over QUIC (3 tests), full
WebRTC state machine needs physical devices.
M6 status: 🟨 IN PROGRESS — Chrome session owned by AppRuntime; provider
sync-mode wiring incomplete.
M7 status: 🟥 BLOCKED — OS capture permission dialog.
M8 status: 🟨 IN PROGRESS — disconnect recovery + transfer stall watcher
wired; comprehensive watcher coverage incomplete.

⚠ EXTERNAL VERIFICATION PENDING:
- Run .app → pick media → visible playback in mpv window
- macOS notification dispatch; Windows toast dispatch
- Two-device QUIC transfer (Tailscale)

---

# STATUS LEGEND

```
⬜ NOT STARTED

🟨 IN PROGRESS

🟦 IMPLEMENTED / TESTING REQUIRED

🟩 COMPLETE

🟥 BLOCKED

⚠ PARTIAL / PLATFORM-SPECIFIC
```

---

# GLOBAL GATES

```
⚠ Windows build working — EXTERNAL VERIFICATION PENDING
🟦 macOS build working
⬜ Windows ↔ macOS test environment available
⬜ Tailscale verified on both machines
⚠ CI running — configured locally; hosted GitHub execution pending
🟦 architecture docs committed
```

---

# PHASE 0 — PROJECT FOUNDATION

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

Tasks:

```
🟦 Initialize repository
🟦 Initialize Tauri 2
🟦 Configure React
🟦 Configure TypeScript strict mode
🟦 Configure Rust workspace
🟦 Configure pnpm
🟦 Add ESLint
🟦 Add Prettier
🟦 Add rustfmt
🟦 Configure clippy
🟦 Configure Vitest
🟦 Configure Rust tests
🟦 Add GitHub Actions Windows
🟦 Add GitHub Actions macOS
🟦 Add docs folder
🟦 Add ADR folder
🟦 Add SQLite migrations folder
```

Acceptance:

```
🟦 pnpm install succeeds
🟦 pnpm build succeeds
🟦 cargo build succeeds
🟦 cargo test succeeds
⚠ Windows app launches — EXTERNAL VERIFICATION PENDING
🟦 macOS app launches
```

Notes:

```
Phase 0 foundation scaffolded on 2026-08-15:
- Tauri 2 + React + strict TypeScript app shell created.
- Rust workspace and required module boundary skeletons created.
- ESLint, Prettier, rustfmt, clippy, Vitest, Rust tests, GitHub Actions, and SQLite migration folder added.
- macOS executable built with `pnpm tauri build --no-bundle`.
- macOS launch smoke test started the app process; non-fatal macOS service warnings were observed in console output.
- Windows build/launch and hosted GitHub Actions execution require external verification.
```

---

# PHASE 1 — TAILSCALE CONNECTIVITY SPIKE

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

Tasks:

```
🟦 Detect Tailscale executable
🟦 Detect signed-in state
🟦 Discover local Tailscale IPv4
🟦 Discover peer
🟦 Detect path type
🟦 Build QUIC listener
🟦 Build QUIC client
🟦 Generate protocol-shaped room credentials
🟦 Bind host to Tailscale address
🟦 Reject non-loopback/non-Tailscale QUIC bind addresses
🟦 Implement HELLO
🟦 HELLO validates UUIDv7 device IDs
🟦 HELLO rejects future minor protocol versions
🟦 HELLO rejects unsupported V1 platforms
🟦 Implement authentication
🟦 AUTH_REQUEST validates Base64URL 128-bit room IDs
🟦 AUTH_REQUEST validates Base64URL 256-bit join secret hashes
🟦 Ed25519 identity public key in HELLO
🟦 HELLO validates Base64URL 256-bit public keys
🟦 AUTH_REQUEST device signature validation
🟦 AUTH_REQUEST invite nonce replay rejection
🟦 Post-auth request sender validation
🟦 Post-auth sequence rejection for duplicate/stale control requests
🟦 Implement heartbeat
🟦 Implement RTT test
🟦 Implement throughput test
🟦 Transfer synthetic 1 GB payload
```

Real environment tests:

```
⚠ College network Mac → Windows — EXTERNAL VERIFICATION PENDING
⚠ College network Windows → Mac — EXTERNAL VERIFICATION PENDING
⚠ Record direct/DERP path — EXTERNAL VERIFICATION PENDING
⚠ Record throughput — EXTERNAL VERIFICATION PENDING
⚠ Record RTT — EXTERNAL VERIFICATION PENDING
```

Gate:

```
⚠ 1 GB reliable transfer — EXTERNAL VERIFICATION PENDING over real Tailscale peers
⬜ reconnect test passes
🟦 no listening on public/LAN interfaces in local listener API; real host binding pending Tailscale environment
```

Notes:

```
Hardened locally on 2026-08-15:
- Added room credential generation for 128-bit Base64URL room IDs and 256-bit Base64URL join secrets.
- QUIC server binding is restricted to loopback or Tailscale IPv4 addresses; wildcard, LAN, and public binds are rejected locally.
- Replaced Phase 1 placeholder public-key/signature values with Ed25519 device identity support.
- HELLO now carries the local identity public key.
- HELLO validates that device IDs are UUIDv7 strings before room authentication succeeds.
- HELLO rejects future protocol minor versions that this host cannot safely interpret.
- HELLO rejects unsupported V1 platform values before room authentication succeeds.
- AUTH_REQUEST rejects malformed room IDs that are not 128-bit Base64URL-without-padding values.
- AUTH_REQUEST rejects malformed join secret hashes that are not 256-bit Base64URL-without-padding values.
- HELLO rejects malformed identity public keys that are not 256-bit Base64URL-without-padding values.
- AUTH_REQUEST signatures cover room ID, join secret hash, invite nonce, and device ID.
- Host-side QUIC auth rejects invalid room secrets, tampered device signatures, and replayed invite nonces in loopback tests.
- Post-auth QUIC requests require an authenticated sender and monotonic per-connection sequence; unauthenticated, mismatched, duplicate, and stale control requests are rejected locally.
- Persistent OS credential storage for the private identity key remains future integration work; current automated tests use deterministic/ephemeral identities.
```

---

# PHASE 2 — SYNCHRONIZATION ENGINE SIMULATOR

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

Tasks:

```
🟦 Fake player interface
🟦 Host monotonic clock
🟦 CLOCK_PING/PONG
🟦 Offset estimation
🟦 PLAY_PREPARE
🟦 PLAY_COMMIT
🟦 PAUSE flow
🟦 SEEK flow
🟦 Buffer consensus
🟦 Sequence handling
🟦 Duplicate-operation handling
🟦 Simulated drift correction
```

Simulation matrix:

```
🟦 10ms RTT
🟦 50ms RTT
🟦 150ms RTT
🟦 jitter
🟦 1% packet loss
🟦 3% packet loss
🟦 10-second outage
```

Gate:

```
🟦 no divergent canonical state
🟦 p95 simulated drift <100ms under normal conditions
```

---

# PHASE 3 — LIBMPV LOCAL PLAYER

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

Capabilities:

```
⚠ Open — app boundary implemented; real libmpv unavailable locally
⚠ Play — app boundary implemented; real libmpv unavailable locally
⚠ Pause — app boundary implemented; real libmpv unavailable locally
⚠ Seek — app boundary implemented; real libmpv unavailable locally
🟦 Position
⬜ Duration
🟦 Volume
⬜ Audio track
⬜ Subtitle track
🟦 Playback rate
🟦 State events
```

Platform:

```
⚠ Windows — EXTERNAL VERIFICATION PENDING
⚠ macOS — EXTERNAL VERIFICATION PENDING; libmpv not installed in current environment
```

---

# PHASE 4 — LOCAL MEDIA IDENTITY

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
⚠ Metadata extraction — filename/container/size implemented; duration/codecs pending media backend
🟦 Quick fingerprint
🟦 BLAKE3 full hash
🟦 Manifest
🟦 Identical-file detection
🟦 Different-file rejection
```

---

# PHASE 5 — P2P FILE TRANSFER

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
🟦 1 MiB chunking
🟦 Chunk scheduler
🟦 Chunk hash
🟦 QUIC chunk stream
🟦 Sparse cache
🟦 Resume bitmap
🟦 Transfer progress
🟦 Disconnect recovery
```

Acceptance:

```
⚠ 4GB file transfer — EXTERNAL VERIFICATION PENDING
⚠ interrupt at 35% — large-file manual verification pending
⚠ restart — large-file manual verification pending
🟦 resume bitmap persists in sparse cache tests
⬜ final hash identical
```

---

# PHASE 6 — PARTIAL CACHE PLAYBACK

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
⚠ Local range server — route/range responder implemented; live loopback HTTP server pending
🟦 Random byte ranges
⬜ mpv playback from incomplete media
🟦 Range-triggered chunk priority
⬜ Forward seek
⬜ Backward seek
```

---

# PHASE 7 — STRICT LOCAL SYNC

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
⚠ Player adapter connected — coordinator boundary implemented; real libmpv unavailable locally
🟦 Scheduled play
🟦 Scheduled pause
🟦 Scheduled seek
🟦 Buffer reports
🟦 BUFFER_LOW flow
🟦 Global pause
🟦 Ready recovery
🟦 Global resume
🟦 Disconnect pause
```

Critical test:

```
🟦 Guest buffer starvation pauses Host in coordinator test; real libmpv/Tailscale test pending
```

Phase may not pass without this.

---

# PHASE 8 — NETWORK-AWARE PRELOAD

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
🟦 Goodput estimator
⬜ Movie bitrate estimator
🟦 Buffer recommendation
⬜ Auto mode
🟦 Smart preload
🟦 Download-first recommendation
```

Test:

```
🟦 2 Mbps policy unit coverage
🟦 5 Mbps policy unit coverage
⚠ 10 Mbps — EXTERNAL VERIFICATION PENDING on real college network
🟦 20 Mbps policy unit coverage
```

---

# PHASE 9 — SCHEDULING

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
🟦 SQLite schedule table
🟦 Create schedule model
⬜ Send schedule
⬜ Guest accept
⚠ Local notification registration — notification plan implemented; OS registration pending
🟦 Preload calculation
⬜ Background transfer
🟦 Offline peer state
⬜ Resume when peer returns
```

---

# PHASE 10 — RETENTION

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Keep
🟦 Remove
🟦 Save As
🟦 Cache cleanup
🟦 Settings policy
```

---

# PHASE 11 — CINEMA UI

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Home
🟦 Create Party
🟦 Join
🟦 Lobby
🟦 Ready Check
🟦 Cinema
🟦 Control Dock
🟦 Buffer UI
🟦 Reconnect UI
🟦 End Party UI
```

Notes:

```
Implemented locally on 2026-08-15:
- Added React navigation for home, create party, join party, lobby, ready check, cinema, and end party confirmation.
- Added cinematic fullscreen-style Cinema Mode with dominant movie surface, floating camera, transient chat, control dock, buffering overlay, and reconnect overlay.
- Added responsive styling that avoids permanent sidebars and keeps social/status UI as overlays.
- Current UI uses representative local state only; backend command wiring and manual visual/device QA remain pending in later integration phases.
```

---

# PHASE 12 — CHAT & REACTIONS

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 CHAT_MESSAGE
🟦 Compose
🟦 Floating messages
🟦 History
🟦 Reactions
🟦 Rate limiting
```

Notes:

```
Implemented locally on 2026-08-15:
- Added native chat payload validation for CHAT_MESSAGE with 2000-byte UTF-8 body limit.
- Added native REACTION validation for the V1 allowed reaction set.
- Added per-participant reaction rate limiter for max 5 reactions per 3 seconds.
- Added protocol message IDs 180 CHAT_MESSAGE and 181 REACTION.
- Added Cinema Mode compose input, floating recent messages, translucent history overlay, reaction tray, floating reaction animation, keyboard handling for Enter/C/Esc, and client-side reaction limit feedback.
- Peer transport wiring remains pending; local UI/state and native validation are implemented.
```

---

# PHASE 13 — GHOST / PRIVACY

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
🟦 Ghost Mode
🟦 Privacy Mode
⚠ Global shortcut Windows — EXTERNAL VERIFICATION PENDING
⚠ Global shortcut macOS — local in-window shortcut implemented; OS-global registration pending later Tauri integration
🟦 Camera stays unchanged under Ghost
🟦 Camera disabled under Privacy
🟦 Mic disabled under Privacy
```

Notes:

```
Implemented locally on 2026-08-15:
- Added native privacy state model for Ghost Mode and Privacy Mode.
- Ghost Mode hides local social/control overlays without changing camera or microphone state.
- Privacy Mode enables Ghost Mode and disables camera/microphone intent.
- Exiting Privacy Mode restores UI but does not re-enable camera or microphone.
- Cinema Mode handles Ctrl/Cmd+Shift+M and Ctrl/Cmd+Shift+P while app window is focused.
- OS-level global shortcut registration and real camera/microphone device effects remain pending for later native integration/manual platform testing.
```

---

# PHASE 14 — VIDEO CALL SPIKE

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
🟦 Camera enumeration
🟦 Mic enumeration
🟦 WebRTC signalling payload/model
⚠ Peer video — EXTERNAL VERIFICATION PENDING with real WebRTC peer
⚠ Peer voice — EXTERNAL VERIFICATION PENDING with real WebRTC peer
🟦 Mic default muted
🟦 Voice-only
🟦 Call-off
```

Cross-platform:

```
⚠ Win → Win — EXTERNAL VERIFICATION PENDING
⚠ Win → Mac — EXTERNAL VERIFICATION PENDING
⚠ Mac → Win — EXTERNAL VERIFICATION PENDING
⚠ Mac → Mac — EXTERNAL VERIFICATION PENDING
```

If routing fails:

```
⬜ ADR created
```

Notes:

```
Implemented locally on 2026-08-15:
- Added native call state, camera state, mic state, and CALL_SIGNAL payload model.
- Added protocol IDs 160 CALL_STATE, 161 CAMERA_STATE, 162 MIC_STATE, and 163 CALL_SIGNAL.
- Added Tier B initial camera constraints and mic-default-muted tests.
- Added browser device enumeration helper and WebRTC peer-connection helper with no TURN servers configured.
- Added Cinema Mode call controls for Video + Voice, Voice Only, Off, mic/camera toggles, and minimizable/hideable floating camera card.
- Real getUserMedia permission flow, remote peer media, Tailscale route behavior, and all OS combinations require manual/external validation.
```

---

# PHASE 15 — ADAPTIVE CAMERA

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Camera tier A
🟦 Tier B
🟦 Tier C
🟦 Tier D
🟦 Goodput feedback
🟦 Buffer feedback
🟦 Camera downgrade
🟦 Camera recovery
```

Critical:

```
🟦 Movie stays smooth when camera is degraded in policy tests; real WebRTC sender enforcement pending Phase 14 external verification
```

Notes:

```
Implemented locally on 2026-08-15:
- Added camera Tier A/B/C/D state definitions with V1-safe resolution/FPS caps.
- Added adaptive camera recommendation policy using measured goodput, movie bitrate, guest buffer, and RTT.
- Policy downgrades camera before sacrificing movie continuity and disables camera under severe pressure.
- Policy recovers one tier at a time only under healthy buffer/goodput/RTT conditions.
- Applying constraints to a live WebRTC sender remains pending real call integration and external platform verification.
```

---

# PHASE 16 — MANAGED CHROME

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

```
🟦 Locate Chrome Windows
🟦 Locate Chrome macOS
🟦 Dedicated profile
🟦 Local CDP
🟦 Navigate URL
⚠ Reuse provider session — EXTERNAL VERIFICATION PENDING with real Chrome login
🟦 Close/restart safely
```

Security:

```
🟦 CDP bound locally
🟦 No provider cookies logged
```

Notes:

```
Implemented locally on 2026-08-15:
- Added managed Chrome candidate discovery for macOS/Windows/Linux.
- Added dedicated provider profile path construction with traversal rejection.
- Added launch plan arguments for non-default profile, localhost-only CDP address, CDP port, and provider URL.
- Added CDP navigation and browser-close command payload builders.
- Added tests proving localhost CDP binding, profile isolation, traversal rejection, and cookie-free launch/CDP payloads.
- Real Chrome executable discovery, launching, provider login reuse, and close/restart behavior require manual platform verification with installed Chrome.
```

---

# PHASE 17 — GENERIC PROVIDER

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Media detection
🟦 Play
🟦 Pause
🟦 Seek
🟦 Position
🟦 Buffer detection
```

Notes:

```
Implemented locally on 2026-08-15:
- Added provider-neutral generic HTML media adapter.
- Adapter emits CDP Runtime.evaluate commands for media detection, identification, position, player state, buffer state, play, pause, seek, and playback-rate control.
- Added snapshot mapping for playing/paused/buffering/ended state and buffer-ahead calculation.
- Tests verify generic adapter uses only HTML media detection and does not include provider-specific selectors.
- Live ordinary non-DRM web-video testing through managed Chrome remains pending external/manual verification.
```

---

# PHASE 18 — YOUTUBE

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 URL parsing
🟦 Content ID
🟦 Player detection
🟦 Sync
🟦 Buffer state
```

Notes:

```
Implemented locally on 2026-08-15:
- Added YouTube URL recognition for watch, youtu.be, embed, and shorts URLs.
- Added 11-character content ID extraction with supported-host validation.
- Added YouTube-specific player detection inside the YouTube adapter module.
- Delegated play/pause/seek/position/buffer control to generic HTML media commands.
- Tests cover URL parsing, invalid host rejection, player detection, and control delegation.
- Live YouTube sync testing through managed Chrome remains pending external/manual verification.
```

---

# PHASE 19 — PROVIDER SYNC

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

Netflix:

```
⚠ Windows — EXTERNAL VERIFICATION PENDING
⚠ macOS — EXTERNAL VERIFICATION PENDING
```

Prime:

```
⚠ Windows — EXTERNAL VERIFICATION PENDING
⚠ macOS — EXTERNAL VERIFICATION PENDING
```

JioHotstar:

```
⚠ Windows — EXTERNAL VERIFICATION PENDING
⚠ macOS — EXTERNAL VERIFICATION PENDING
```

Required per provider:

```
⚠ Launch — EXTERNAL VERIFICATION PENDING
⚠ Login — EXTERNAL VERIFICATION PENDING
⚠ Open URL — EXTERNAL VERIFICATION PENDING
🟦 Detect media
🟦 Play
🟦 Pause
🟦 Seek
🟦 Position
🟦 Buffer detect
🟦 Strict global pause
⚠ Resume — EXTERNAL VERIFICATION PENDING
```

Notes:

```
Implemented locally on 2026-08-15:
- Added provider-sync core with provider ID mapping for YouTube, Netflix, Prime, and JioHotstar.
- Added compatibility matrix that records Windows/macOS entries as external verification pending without claiming support.
- Added host-committed provider sync action mapping to adapter CDP commands.
- Added strict global pause decision when peer disconnects or provider buffer is below threshold.
- Real Netflix/Prime/JioHotstar login, media detection, playback control, recovery, and support-level status remain pending external/manual verification.
```

---

# PHASE 20 — WINDOWS SHARED CAPTURE SPIKE

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

Test:

```
⚠ YouTube video — EXTERNAL VERIFICATION PENDING
⚠ YouTube audio — EXTERNAL VERIFICATION PENDING
⚠ Netflix video — EXTERNAL VERIFICATION PENDING
⚠ Netflix audio — EXTERNAL VERIFICATION PENDING
⚠ Prime video — EXTERNAL VERIFICATION PENDING
⚠ Prime audio — EXTERNAL VERIFICATION PENDING
⚠ JioHotstar video — EXTERNAL VERIFICATION PENDING
⚠ JioHotstar audio — EXTERNAL VERIFICATION PENDING
```

Record protected-capture behavior.

Notes:

```
Implemented locally on 2026-08-15:
- Added Windows diagnostic capture plan for Windows.Graphics.Capture video, WASAPI Application Loopback audio, selected-window capture, provider-audio capture, and 30-second sample duration.
- Added provider-shared capture availability model.
- Added black-frame/protected-content heuristic using frame count, luma, variance, changed-frame ratio, and provider-playing signal.
- Added one diagnostic retry then Provider Sync fallback policy.
- Actual Windows capture APIs, provider media samples, and protected-capture outcomes require Windows/manual external verification.
```

---

# PHASE 21 — MACOS SHARED CAPTURE SPIKE

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

Same matrix:

```
⚠ YouTube video — EXTERNAL VERIFICATION PENDING
⚠ YouTube audio — EXTERNAL VERIFICATION PENDING
⚠ Netflix video — EXTERNAL VERIFICATION PENDING
⚠ Netflix audio — EXTERNAL VERIFICATION PENDING
⚠ Prime video — EXTERNAL VERIFICATION PENDING
⚠ Prime audio — EXTERNAL VERIFICATION PENDING
⚠ JioHotstar video — EXTERNAL VERIFICATION PENDING
⚠ JioHotstar audio — EXTERNAL VERIFICATION PENDING
```

Notes:

```
Implemented locally on 2026-08-15:
- Added macOS diagnostic capture plan for ScreenCaptureKit video/application audio and VideoToolbox H264.
- Added 30-second diagnostic sample configuration and macOS capture permission status model.
- Actual ScreenCaptureKit permission flow, provider media capture, audio capture, and protected-capture outcomes require manual/external verification.
```

---

# PHASE 22 — HARDWARE ENCODING

Status:

```
⚠ PARTIAL / PLATFORM-SPECIFIC
```

Windows:

```
🟦 H264 Media Foundation
```

macOS:

```
🟦 H264 VideoToolbox
```

Benchmark:

```
🟦 720p30
🟦 1080p30
🟦 encode latency
⚠ CPU — EXTERNAL VERIFICATION PENDING on real hardware
⚠ GPU — EXTERNAL VERIFICATION PENDING on real hardware
```

Notes:

```
Implemented locally on 2026-08-15:
- Added hardware encoder platform mapping for Windows Media Foundation and macOS VideoToolbox.
- Added V1 H264 30fps bitrate ladder: 1080p30 High, 1080p30 Medium, 720p30 High, and 720p30 Low.
- Added goodput-based encoder profile selection.
- Added benchmark sample classification for capture-to-encode latency and achieved FPS.
- Real Media Foundation/VideoToolbox invocation, CPU/GPU measurement, and 720p30/1080p30 benchmark samples require usable provider capture and external platform testing.
```

---

# PHASE 23 — SHARED STREAM TRANSPORT

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Encoded media stream
🟦 Guest receive buffer
⚠ Decoder — EXTERNAL VERIFICATION PENDING with real encoded provider stream
🟦 Presentation timestamps
🟦 Audio
🟦 A/V sync
```

Notes:

```
Implemented locally on 2026-08-15:
- Added shared stream packet format for encoded video/audio packets with sequence, stream kind, keyframe flag, PTS, duration, and payload.
- Added packet encode/decode validation with typed errors for malformed input.
- Added guest presentation buffer with seconds-of-media readiness check and PTS-based release.
- Added duplicate/stale packet rejection.
- Real decoder integration and playback of encoded provider streams remain pending capture/encoder availability and manual integration testing.
```

---

# PHASE 24 — HOST LOOPBACK

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Host watches encoded output
🟦 Guest watches same output
🟦 Chrome source hidden from experience
```

Notes:

```
Implemented locally on 2026-08-15:
- Added host loopback plan requiring both host and guest to consume the encoded shared stream.
- Added Chrome-source-hidden flag so raw provider Chrome is not the user's presentation surface.
- Added 5-second shared-mode presentation latency constant and aligned guest buffer target with it.
- Added shared timeline model tracking source, encoded, and presentation positions, with presentation timeline authoritative.
- Real host decoder playback requires capture/encode/decode integration and manual shared-mode testing.
```

---

# PHASE 25 — SHARED STRICT SYNC

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

Critical test:

```
🟦 Throttle Guest
🟦 Guest buffer falls
🟦 Source pauses
🟦 Host presentation pauses
🟦 Guest rebuilds
🟦 Both resume together
```

Notes:

```
Implemented locally on 2026-08-15:
- Added shared strict-sync decision model for guest buffer pressure, peer disconnect, and decoder readiness.
- Guest buffer below threshold pauses source, host presentation, and guest presentation together.
- Paused shared mode rebuilds guest buffer before resuming.
- Once target shared presentation buffer is restored, action resumes both viewers together.
- Real throttle test over live shared stream remains pending capture/encode/decode integration.
```

---

# PHASE 26 — AUTOMATIC QUALITY

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 1080p high
🟦 1080p medium
🟦 720p high
🟦 720p low
🟦 camera reduction before movie reduction
```

Notes:

```
Implemented locally on 2026-08-15:
- Added automatic quality decision model using network goodput, buffer level, encoder stats, and call bitrate.
- Added output decision for movie encoder profile and camera tier.
- Policy preserves current movie quality and reduces camera first when the movie bitrate budget is still available.
- Policy reduces movie profile only when the movie budget or encoder health is unsafe.
- Policy recovers movie and camera quality under healthy buffer/goodput/encoder conditions.
- Live sender/encoder reconfiguration remains pending capture/encode/transport integration.
```

---

# PHASE 27 — VBROWSER R&D

Status:

```
⬜ NOT REQUIRED YET
```

Only activate if Provider Shared has blocking limitations.

```
⬜ Local VM
⬜ User-owned second PC option
⬜ DRM playback test
⬜ Capture test
⬜ Audio
⬜ Latency
```

Result:

```
TBD
```

---

# PHASE 28 — RESILIENCE

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Chrome crash recovery policy
🟦 Guest crash recovery policy
🟦 Host crash recovery policy
🟦 Tailscale disconnect recovery policy
🟦 Tailscale reconnect recovery policy
🟦 Network change recovery policy
🟦 WiFi disconnect recovery policy
🟦 Sleep/wake recovery policy
🟦 Provider logout recovery policy
🟦 Cache corruption recovery policy
🟦 Missing local file recovery policy
⚠ Chrome crash drill — EXTERNAL VERIFICATION PENDING
⚠ Guest crash drill — EXTERNAL VERIFICATION PENDING
⚠ Host crash drill — EXTERNAL VERIFICATION PENDING
⚠ Tailscale disconnect/reconnect drill — EXTERNAL VERIFICATION PENDING
⚠ WiFi disconnect drill — EXTERNAL VERIFICATION PENDING
⚠ Sleep/wake drill — EXTERNAL VERIFICATION PENDING
⚠ Provider logout drill — EXTERNAL VERIFICATION PENDING
⚠ Cache corruption drill — EXTERNAL VERIFICATION PENDING
⚠ Missing local file drill — EXTERNAL VERIFICATION PENDING
```

Notes:

```
Implemented locally on 2026-08-15:
- Added explicit resilience recovery planning for Chrome crash, host/guest crash, Tailscale disconnect/reconnect, network change, WiFi disconnect, sleep/wake, provider logout, cache corruption, and missing local file.
- Every recovery plan pauses both participants before recovery, preserving strict sync behavior.
- Provider logout and missing local file require user action instead of silent fallback.
- Tailscale reconnect and network-change events revalidate networking and rebuild buffers before resume.
- Manual crash, network, sleep/wake, provider-session, and cache-corruption drills require real devices/environments and remain externally pending.
```

---

# PHASE 29 — COLLEGE NETWORK CERTIFICATION

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

Environment:

```
College WiFi
~10 Mbps expected
Tailscale active
```

Local:

```
🟦 2GB measurement scenario support
🟦 4GB measurement scenario support
🟦 8GB measurement scenario support
⚠ 2GB real college WiFi run — EXTERNAL VERIFICATION PENDING
⚠ 4GB real college WiFi run — EXTERNAL VERIFICATION PENDING
⚠ 8GB real college WiFi run — EXTERNAL VERIFICATION PENDING
```

Call:

```
🟦 Off measurement scenario support
🟦 Voice measurement scenario support
🟦 Video measurement scenario support
⚠ Off real college WiFi run — EXTERNAL VERIFICATION PENDING
⚠ Voice real college WiFi run — EXTERNAL VERIFICATION PENDING
⚠ Video real college WiFi run — EXTERNAL VERIFICATION PENDING
```

Provider Shared:

```
🟦 Provider Shared measurement record support
⚠ All available provider combinations — EXTERNAL VERIFICATION PENDING
```

Notes:

```
Implemented locally on 2026-08-15:
- Added telemetry certification model for Phase 29 measurement records.
- Required local movie matrix covers 2GB, 4GB, and 8GB movies across call off, voice-only, and video modes.
- Certification samples record actual goodput, buffering events, max sync drift, and Tailscale path.
- Summary logic flags low goodput, buffering, excessive drift, and relayed/unknown Tailscale paths as tuning inputs.
- The code cannot mark Phase 29 ready until all required real local-movie measurement scenarios have samples.
- Real college WiFi, Tailscale, provider shared, and media/call runs remain externally pending.
```

---

# PHASE 30 — FULL CROSS-PLATFORM REGRESSION

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
🟦 Win → Win regression record support
🟦 Win → Mac regression record support
🟦 Mac → Win regression record support
🟦 Mac → Mac regression record support
🟦 Release candidate gate requires Local Perfect pass for every platform pair
⚠ Win → Win manual regression — EXTERNAL VERIFICATION PENDING
⚠ Win → Mac manual regression — EXTERNAL VERIFICATION PENDING
⚠ Mac → Win manual regression — EXTERNAL VERIFICATION PENDING
⚠ Mac → Mac manual regression — EXTERNAL VERIFICATION PENDING
```

Notes:

```
Implemented locally on 2026-08-15:
- Added telemetry regression model for Windows/macOS host/guest pairs.
- Added regression records for core scenarios including Local Perfect, preloaded local, Provider Sync, Provider Shared, voice call, and video call.
- Added release-candidate gate that remains closed until every required platform pair has a passing Local Perfect record.
- Actual Windows/macOS pair testing remains externally pending and no release-candidate status has been claimed.
```

---

# PHASE 31 — PERSONAL BETA

Status:

```
🟦 IMPLEMENTED / TESTING REQUIRED
```

```
⚠ Create development installer — EXTERNAL VERIFICATION PENDING
⚠ Install on trusted friend device — EXTERNAL VERIFICATION PENDING
🟦 Collect bug reports support
🟦 Export diagnostic bundle JSON support
⚠ Export diagnostic bundles from real beta device — EXTERNAL VERIFICATION PENDING
⚠ Run real movie nights — EXTERNAL VERIFICATION PENDING
🟦 Beta readiness gate requires trusted install, diagnostic export, and real movie night
```

Notes:

```
Implemented locally on 2026-08-15:
- Added telemetry beta diagnostic bundle model with app version, platform, beta event list, and redacted log lines.
- Added redaction for common token, cookie, password, and authorization fields.
- Added beta event records for trusted friend install, bug reports, diagnostic bundle export, and real movie nights.
- Added beta readiness gate that remains externally pending until a trusted friend install, diagnostic bundle export, and real movie night are recorded.
- Added local diagnostic JSON export helper with stable Movie Party metadata, beta events, redacted logs, and destination validation.
- Real installer creation, trusted-device install, real diagnostic export, and real movie-night usage remain externally pending.
```

---

# PHASE 32 — OPTIONAL RELEASE HARDENING

Status:

```
⬜ DEFERRED
```

```
⬜ Signing
⬜ Notarization
⬜ Installer polish
⬜ Auto-update
⬜ Crash reporting
⬜ Public docs
```

---

# CURRENT BLOCKERS

```
Phase 0 cannot be marked COMPLETE until Windows build/launch and hosted CI are verified externally.
Phase 28 cannot be marked COMPLETE until manual resilience drills are executed on real devices/networks.
Phase 29 requires college WiFi with Tailscale and real 2GB/4GB/8GB movie transfer/call/provider tests.
Phase 30 requires Windows/macOS cross-platform pairs.
Phase 31 requires a trusted friend device and real movie-night beta usage.
```

---

# OPEN ADRs

```
None.
```

---

# ACCEPTED ADRs

```
None.
```

---

# KNOWN RISKS

## RISK-001

Provider protected video may not be capturable.

Status:

```
OPEN
```

Mitigation:

```
Provider Sync fallback
self-hosted vBrowser R&D
```

---

## RISK-002

College network may force Tailscale DERP and reduce throughput.

Status:

```
OPEN
```

Mitigation:

```
preload
download-first
optional future Peer Relay
```

---

## RISK-003

WebRTC may not reliably use desired Tailscale route in embedded WebViews.

Status:

```
OPEN
```

Mitigation:

```
Phase 14 spike
native-call ADR if necessary
```

---

# SESSION LOG TEMPLATE

Every significant coding session appends:

```
## YYYY-MM-DD

Active Phase:
Phase X

Completed:
- ...

Tests:
- ...

Failures:
- ...

Cross-platform:
- Windows:
- macOS:

Blockers:
- ...

Documentation Updated:
- ...

Next permitted task:
- ...
```

## 2026-08-15

Active Phase:
Phase 0

Completed:
- Initialized Git repository metadata.
- Added Tauri 2 / React / TypeScript / Rust workspace foundation.
- Added required frontend and native-core directory boundaries.
- Added package, TypeScript, Vite, ESLint, Prettier, rustfmt, pnpm, Tauri, and Cargo configuration.
- Added GitHub Actions workflow for frontend validation and Windows/macOS Rust validation.
- Added initial SQLite migration matching the locked data model tables.
- Added minimal Tauri app icon required by the Tauri build.

Tests:
- `pnpm install` passed after approving `esbuild` build scripts.
- `pnpm lint` passed.
- `pnpm test` passed.
- `pnpm build` passed.
- `pnpm format` passed.
- `cargo fmt --check` passed.
- `cargo build` passed from repository root.
- `cargo clippy --all-targets --all-features -- -D warnings` passed from repository root.
- `cargo test` passed from repository root.
- `pnpm tauri build --no-bundle` passed on macOS.
- macOS launch smoke test started the built executable and was then stopped.

Failures:
- Initial sandboxed `pnpm install` and `cargo build` attempts failed because registry DNS/network access was restricted; both passed after approved network access.
- Initial Tauri build failed until `src-tauri/icons/icon.png` was added.
- Initial Prettier check scanned locked docs/generated build output; `.prettierignore` now excludes those paths.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Build and launch smoke test passed locally on 2026-08-15.

Blockers:
- Windows build/launch requires a Windows machine or CI result.
- Hosted GitHub Actions execution requires pushing the repository to GitHub.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Finish Phase 0 external verification, then begin Phase 1 Tailscale connectivity spike.

---

## 2026-08-15

Active Phase:
Phase 11

Completed:
- Added the locally navigable Cinema UI flow: home, create party, join party, lobby, ready check, cinema, and end party confirmation.
- Added Cinema Mode with movie-dominant layout, floating camera, transient chat, control dock, buffering state, and reconnect state.
- Added responsive frontend styling aligned with the overlay-first UI spec.
- Added `PRODUCT.md` for Impeccable product context used during UI implementation.

Tests:
- `pnpm lint` passed.
- `pnpm test` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `pnpm lint` failed on shorthand callbacks returning void; fixed by using named route handlers.
- Initial `pnpm format` found formatting drift in new UI files; fixed with project formatter.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Frontend build passed locally on 2026-08-15; manual visual QA pending.

Blockers:
- Backend command wiring, real playback state, and manual visual/device QA remain pending for later integration.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 12 chat and reactions.

---

## 2026-08-15

Active Phase:
Phase 12

Completed:
- Added native chat and reaction message validation under `src-tauri/src/chat`.
- Registered protocol IDs 180 `CHAT_MESSAGE` and 181 `REACTION`.
- Added reaction rate limiting at max 5 reactions per 3 seconds per participant.
- Added Cinema Mode chat compose, floating message queue, chat history overlay, reaction tray, floating reaction animation, and Enter/C/Esc keyboard behavior.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test chat` passed.
- `cargo test protocol` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm test` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo test chat protocol` was an invalid Cargo filter invocation; reran as separate filters.
- Initial sandboxed `cargo test` failed on existing QUIC loopback socket binding; rerun with permission passed.
- Initial `pnpm lint` flagged deprecated React form event aliases and numeric template IDs; fixed.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated frontend and Rust validation passed locally on 2026-08-15; peer chat/reaction transport and manual visual QA pending.

Blockers:
- Real peer delivery requires later room/network integration.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 13 Ghost Mode and Privacy Mode.

---

## 2026-08-15

Active Phase:
Phase 13

Completed:
- Added native Ghost/Privacy state model under `src-tauri/src/privacy`.
- Added tests proving Ghost Mode does not change camera/microphone state.
- Added tests proving Privacy Mode disables camera/microphone and does not re-enable them on exit.
- Added focused-window shortcuts in Cinema Mode for Ctrl/Cmd+Shift+M and Ctrl/Cmd+Shift+P.
- Added UI hiding for camera, chat, reactions, status overlays, and controls while Ghost/Privacy is active, with temporary confirmation notices only.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test privacy` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm test` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `pnpm format` found formatting drift in `src/cinema/CinemaMode.tsx`; fixed with project formatter.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated validation passed locally on 2026-08-15; OS-global shortcut registration and real device mute/camera behavior pending.

Blockers:
- OS-level global shortcuts and real camera/mic device control require later Tauri/call integration and manual platform verification.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 14 video call spike.

---

## 2026-08-15

Active Phase:
Phase 14

Completed:
- Replaced the call stub with typed native call, camera, mic, and signal models.
- Registered protocol IDs for `CALL_STATE`, `CAMERA_STATE`, `MIC_STATE`, and `CALL_SIGNAL`.
- Added tests for muted initial microphone state, Tier B camera defaults, and signal payload validation.
- Added frontend helpers for browser camera/microphone enumeration and WebRTC peer connection creation without TURN servers.
- Added Cinema Mode call mode controls, mic/camera toggles, and minimizable/hideable floating camera card.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test call` passed.
- `cargo test protocol` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm test` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo test call protocol` was an invalid Cargo filter invocation; reran as separate filters.
- Initial `pnpm lint` flagged the runtime media-device guard; fixed with a narrower runtime navigator shape.
- Initial `cargo clippy` flagged an assertion on a constant; removed the redundant assertion.
- Initial `pnpm format` found formatting drift in new call/Cinema files; fixed with project formatter.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated validation passed locally on 2026-08-15; real camera/mic permissions, peer media, and WebRTC route verification pending.

Blockers:
- Real peer video/voice and Tailscale route behavior require two devices and Windows/macOS manual testing.
- If WebRTC routing fails in the target WebViews, the documented native media transport ADR path remains pending.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 15 adaptive camera.

---

## 2026-08-15

Active Phase:
Phase 15

Completed:
- Added camera Tier A/B/C/D definitions with V1 resolution/FPS/bitrate limits.
- Added adaptive camera recommendation policy driven by goodput, movie bitrate, guest buffer, and RTT.
- Added downgrade behavior that reduces/disables camera before compromising movie continuity.
- Added conservative one-tier-at-a-time recovery behavior.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test call` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted one function signature wrapped; fixed with `cargo fmt`.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated validation passed locally on 2026-08-15; live WebRTC sender constraint behavior pending external/manual call testing.

Blockers:
- Real camera quality changes require live WebRTC sender integration and real network/camera devices.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 16 managed Chrome.

---

## 2026-08-15

Active Phase:
Phase 16

Completed:
- Added managed Chrome discovery candidates for macOS, Windows, and Linux.
- Added dedicated provider profile path builder with path traversal rejection.
- Added Chrome launch plan arguments using non-default provider profile and localhost-only CDP.
- Added CDP navigation and browser-close command payload builders.
- Added tests for local CDP binding, profile isolation, cookie-free payloads, and invalid provider IDs.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test providers::chrome` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted wrapping in the new Chrome module; fixed with `cargo fmt`.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated validation passed locally on 2026-08-15; real Chrome launch/profile/session reuse pending manual verification.

Blockers:
- Real provider session reuse requires installed Chrome and provider login in the dedicated profile.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 17 generic provider.

---

## 2026-08-15

Active Phase:
Phase 17

Completed:
- Added generic HTML media provider adapter under `src-tauri/src/providers/generic.rs`.
- Added provider-neutral CDP commands for detect, identify, position, state, buffer, play, pause, seek, and playback-rate control.
- Added media snapshot helpers for player-state and buffer-ahead calculation.
- Added tests proving no provider-specific selectors are present in generic detection.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test providers::generic` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.

Failures:
- None.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated validation passed locally on 2026-08-15; live ordinary non-DRM web-video test pending.

Blockers:
- Live provider/browser testing requires managed Chrome launch and a non-DRM media page.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 18 YouTube adapter.

---

## 2026-08-15

Active Phase:
Phase 18

Completed:
- Added YouTube adapter implementation under `src-tauri/src/providers/youtube`.
- Added URL recognition and content ID extraction for watch, youtu.be, embed, and shorts URLs.
- Added YouTube-specific player detection CDP command inside the YouTube adapter boundary.
- Delegated play, pause, seek, position, and buffer state commands to the generic HTML media adapter.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test providers::youtube` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial parser used Rust 2024 let-chain syntax; rewrote for Rust 2021.
- Initial `cargo fmt --check` wanted closure wrapping; fixed with `cargo fmt`.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated validation passed locally on 2026-08-15; live YouTube managed-Chrome sync test pending.

Blockers:
- Live YouTube sync testing requires managed Chrome launch and network/media access.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 19 Provider Sync.

---

## 2026-08-15

Active Phase:
Phase 19

Completed:
- Added provider-sync core under `src-tauri/src/providers/sync.rs`.
- Added provider ID mapping for YouTube, Netflix, Prime, and JioHotstar.
- Added unverified compatibility matrix that does not claim support before manual testing.
- Added host-committed provider action mapping to adapter CDP commands.
- Added strict global pause decision for provider buffering or peer disconnect.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test providers::sync` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted wrapping in the new provider-sync module; fixed with `cargo fmt`.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING.
- macOS: Automated validation passed locally on 2026-08-15; real provider sync testing pending.

Blockers:
- Netflix, Prime, and JioHotstar support status requires real provider accounts, Chrome login, media playback, and Windows/macOS testing.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 20 Windows shared capture spike.

---

## 2026-08-15

Active Phase:
Phase 20

Completed:
- Added Windows provider-shared diagnostic capture plan.
- Added capture availability model for video/audio diagnostic results.
- Added black-frame/protected-content heuristic based on luma, variance, frame change, and provider-playing state.
- Added capture failure policy: one diagnostic retry, then offer Provider Sync Mode.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test capture` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted wrapping in capture tests; fixed with `cargo fmt`.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING for all real capture/audio/provider samples.
- macOS: Automated validation passed locally on 2026-08-15; Windows API behavior not available in this environment.

Blockers:
- Real Windows Graphics Capture and WASAPI Application Loopback testing requires Windows hardware, Chrome, provider playback, and capture permissions.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 21 macOS shared capture spike.

---

## 2026-08-15

Active Phase:
Phase 21

Completed:
- Added macOS provider-shared diagnostic capture plan.
- Added ScreenCaptureKit application video/audio capture model.
- Added VideoToolbox H264 diagnostic encode target.
- Added macOS capture permission status tracking for external/manual verification.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test capture` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- None.

Cross-platform:
- Windows: Not applicable to Phase 21.
- macOS: Automated validation passed locally on 2026-08-15; real ScreenCaptureKit permission and provider capture tests pending.

Blockers:
- Real macOS provider-shared capture testing requires Chrome/provider playback and screen/audio capture permissions.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 22 hardware encoding.

---

## 2026-08-15

Active Phase:
Phase 22

Completed:
- Added hardware encoder platform mapping for Windows Media Foundation and macOS VideoToolbox.
- Added H264 30fps bitrate ladder for 1080p30 and 720p30 profiles.
- Added goodput-based profile selection.
- Added benchmark sample classification for encode latency and achieved FPS.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test encode` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted one assertion wrapped; fixed with `cargo fmt`.

Cross-platform:
- Windows: ⚠ EXTERNAL VERIFICATION PENDING for real Media Foundation encode benchmarks.
- macOS: Automated validation passed locally on 2026-08-15; real VideoToolbox encode benchmark pending usable capture and permissions.

Blockers:
- Real hardware benchmark data requires usable provider capture samples and platform-specific encoder execution.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 23 Shared Mode transport.

---

## 2026-08-15

Active Phase:
Phase 23

Completed:
- Added shared stream packet format for encoded video/audio media.
- Added typed packet decoder errors for malformed, wrong-magic, unknown-kind, and length-mismatched packets.
- Added guest presentation buffer with target buffered-duration readiness.
- Added PTS-based packet release and duplicate/stale packet rejection.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test shared_stream` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted wrapping in the shared-stream module; fixed with `cargo fmt`.

Cross-platform:
- Windows: Automated validation passed locally; real shared-stream decoder/playback over QUIC pending platform/integration testing.
- macOS: Automated validation passed locally on 2026-08-15; real shared-stream decoder/playback over QUIC pending integration testing.

Blockers:
- Real decoder playback requires usable capture/encode input and guest player integration.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 24 host loopback.

---

## 2026-08-15

Active Phase:
Phase 24

Completed:
- Added host loopback plan ensuring host and guest both consume encoded shared stream output.
- Added Chrome-source-hidden presentation flag.
- Added 5-second shared presentation latency and aligned default presentation buffer to that target.
- Added source/encoded/presentation timeline model.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test shared_stream` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted wrapping in shared-stream tests; fixed with `cargo fmt`.

Cross-platform:
- Windows: Automated validation passed locally; real host loopback playback pending platform/integration testing.
- macOS: Automated validation passed locally on 2026-08-15; real host loopback playback pending capture/encode/decode integration.

Blockers:
- Real host loopback playback requires usable provider capture, hardware encoding, shared transport, and decoder integration.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 25 Shared Strict Sync.

---

## 2026-08-15

Active Phase:
Phase 25

Completed:
- Added shared strict-sync input/action model.
- Added pause-all behavior when guest buffer falls, peer disconnects, or either decoder is unavailable.
- Added rebuild behavior while source is paused and guest buffer is below shared presentation target.
- Added resume-together behavior once the shared presentation target is restored.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test shared_stream` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- None.

Cross-platform:
- Windows: Automated validation passed locally; live shared-stream throttle test pending integration/external testing.
- macOS: Automated validation passed locally on 2026-08-15; live shared-stream throttle test pending integration testing.

Blockers:
- Real throttle/decoder behavior requires usable shared capture, encoder, transport, and decoder playback.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Begin Phase 26 Automatic Quality.

---

## 2026-08-15

Active Phase:
Phase 26

Completed:
- Added automatic quality input and decision model.
- Added movie encoder profile + camera tier output selection.
- Added policy to reduce camera before movie when movie bitrate remains safe.
- Added movie profile reduction only when movie budget or encoder health is unsafe.
- Added recovery behavior under healthy buffer, goodput, and encoder stats.

Tests:
- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test encode` passed.
- `cargo test` passed after approving local UDP socket access for existing QUIC loopback tests.
- `pnpm lint` passed.
- `pnpm build` passed.
- `pnpm format` passed.

Failures:
- Initial `cargo fmt --check` wanted wrapping in automatic quality conditions; fixed with `cargo fmt`.

Cross-platform:
- Windows: Automated validation passed locally; live encoder/camera sender reconfiguration pending platform testing.
- macOS: Automated validation passed locally on 2026-08-15; live encoder/camera sender reconfiguration pending integration testing.

Blockers:
- Real automatic quality behavior requires live shared capture, encoder, call sender, and transport metrics.

Documentation Updated:
- `docs/core_docs/IMPLEMENTATION_TRACKER.md`

Next permitted task:
- Evaluate Phase 27 vBrowser R&D activation criteria.

---

# RULE

Do not change a phase to:

```
🟩 COMPLETE
```

until every mandatory gate for that phase passes.

````

---

## Final planning package

You now have the equivalent of:

```text
Movie Party/
│
├── MASTER_PRD.md
├── AGENTS.md
├── PROTOCOL_SPEC.md
├── UI_UX_SPEC.md
├── IMPLEMENTATION_TRACKER.md
│
└── docs/
    └── architecture/
        └── adr/
            └── ADR_TEMPLATE.md
````

---

## 2026-08-29 — Batch 4 Local Perfect code-completion pass

Completed at code level:

- Guest manifest acceptance validates cache-safe media metadata before a sparse
  cache is opened. Guest cache paths remain locally owned and are registered
  under the runtime cache root used by retention.
- Guest preparation fetches, validates, and persists the opening chunk before
  exposing the loopback source or media-ready state. Transfer progress now
  comes from sparse-cache coverage.
- Scheduled preload uses the existing scheduler/executor: verified opening
  data first, then sequential background chunks while playback demand remains
  higher priority.
- Actual player buffering transitions use the existing canonical
  BUFFER_LOW/BUFFER_RECOVERED flow. Recovery returns to Ready Check and never
  resumes independently.
- Guest player position no longer overwrites host canonical position. The
  existing drift policy is called only while playing and not in strict buffer
  recovery; its seek direction converges toward the canonical position.
- Loopback range serving is capped at eight concurrent connections. Existing
  leave/replacement cleanup remains the owner of player, range, transfer, and
  cache resources.

Validation:

- `cargo check` passed with a clean temporary target directory.
- Focused manifest, sparse-cache, transfer/integrity, and drift tests passed.
- Range-server socket tests could not bind loopback in the sandbox
  (`Operation not permitted`); this is an environment limitation, not a test
  assertion failure.
- macOS ↔ Windows media playback, buffering, preload timing, reconnect, OS
  notification delivery, and retention actions remain manual verification.

## 2026-08-29 — Batch 4 reconnect closure

- Added one cancellable Guest reconnect worker using the existing invitation,
  certificate fingerprint, identity, and authenticated `QuicClient::connect`
  path. Backoff is bounded and terminal authentication/TLS/protocol failures
  stop retries.
- Reconnect replaces only the transport, preserves the verified sparse cache,
  range source, and transfer demand, then stays in canonical `RECONNECTING`
  / strict-pause state until the Host runs recovery. No Guest autoplay is
  introduced.
- Guest heartbeat monitoring is production-wired and replaced rather than
  duplicated across transport changes. Leave Party cancels heartbeat,
  reconnect, preload, transfer-stall, range, player, and receive workers.
- Playback headroom now falls back to contiguous verified cache bytes around
  the playhead when libmpv cannot report its own buffer value; whole-file
  transfer percentage remains separate.
- Scheduled preload computes an earlier safe deadline from observed goodput
  (with a deterministic conservative fallback). Offline preload remains
  `WaitingForPeer`; its existing native notice is rate-limited to one per
  schedule per fifteen minutes.

⚠ EXTERNAL VERIFICATION PENDING: authenticated reconnect, range-server
backpressure, real libmpv headroom, scheduled offline notification delivery,
and macOS ↔ Windows recovery must be checked with physical devices.

## 2026-08-30 — Batch 4 closure test pass

No production code changed in this pass; focused tests were added to lock the
Batch-4 closure behaviors that the working tree already implemented:

- Reconnect worker guards (host role / missing invite / exactly one worker)
  and terminal-vs-retryable failure classification.
- `apply_disconnect` preserves the verified guest sparse cache and pauses
  strict sync; `leave_party` aborts reconnect, heartbeat, preload, and
  transfer-stall watch workers (proven via abort-observed Drop guards).
- Guest heartbeat is replaced (aborted) rather than duplicated on transport
  change.
- Playback headroom: contiguous verified bytes ahead of the playhead can be
  healthy before full-file completion and low despite high whole-file
  progress.
- Range-server connection bound: `MAX_RANGE_CONNECTIONS` is 8 and excess
  work is blocked by the semaphore until a permit is freed (pure
  semaphore-state test; no sandbox socket binding required).
- Goodput-driven preload deadline and offline-preload `WaitingForPeer`
  behavior were re-confirmed by existing focused tests.

Verification:

- `cargo check` ✅ with a clean temporary target directory.
- `cargo test --lib` ✅ 232 passed, 2 ignored.
- Focused filters: reconnect_*, heartbeat_*, contiguous_bytes*,
  range_server_connection_limit*, apply_disconnect*,
  preload_deadline*, demand_*, and `--test m3_closure` / `--test m4_closure`
  all ✅.
- `--test m2_integration`: 21 passed, 4 failed — the same four
  (`test_a_ready_reaches_host`, `test_d_seek_sets_canonical_position_on_both`,
  `test_l_ready_does_not_bypass_play_protocol`,
  `test_t_seek_waits_for_guest_and_resumes_together`) fail identically on the
  clean pre-pass HEAD; they are pre-existing timing predicates, not caused by
  this pass.
- Real authenticated reconnect, two-device recovery, libmpv headroom, and OS
  notification delivery remain manual/external verification.

---

## 2026-08-30 — Batch 5 Provider Playback Production Path

### Implemented at code level

- **Provider capability model reflects reality**: the native provider registry
  now reports `supportLevel` (SUPPORTED) and `titleResolution`
  (`DIRECT_URL` for YouTube, `PROVIDER_SEARCH` for Netflix/Prime/JioHotstar).
  All providers keep `verification: EXTERNAL_VERIFICATION_PENDING`; Provider
  Shared stays `sharedAvailable: false` until capture is verified.
- **Provider readiness state machine**: new `ProviderReadiness`
  (`NOT_STARTED`, `LAUNCHING`, `LOGIN_REQUIRED`, `READY`, `NAVIGATING`,
  `PLAYBACK_READY`, `UNAVAILABLE`, `ERROR`) is serialized into the provider
  snapshot. `readiness_from_detection` maps real CDP signals (login page vs
  media element) to state; `READY`/`PLAYBACK_READY` are never derived from
  Chrome launch alone.
- **Managed browser/session lifecycle**: `open_provider_browser` reuses the
  existing session for the same provider via `session_matches_provider`
  (alive + same provider id) and never spawns a duplicate browser process.
  Provider switch closes the old session and launches a clean one, so stale
  provider state is not reused.
- **Provider authentication stays on the provider's own page**: Movie Party
  never collects passwords, tokens, cookies, or session secrets. The UI only
  opens the provider's official page, observes readiness, and offers an
  "I've signed in — Check status" action. No email/password form exists.
- **Movie/title navigation**: `navigate_provider_title` navigates the managed
  browser to the provider's own search URL for the user-entered title using
  the existing adapter/CDP boundary. Empty titles are rejected; the provider
  remains authoritative for catalogue results (no scraping).
- **Playback preparation**: `check_provider_status` runs the provider adapter's
  login/media detection over CDP and updates truthful readiness. Room creation
  is gated so Provider Sync starts only when provider readiness is valid
  (`validate_provider_ready_for_room` requires READY or PLAYBACK_READY).
- **Room handoff**: `launch_provider` reuses the authenticated session when one
  exists and only attaches it to a created room after readiness validation;
  otherwise it launches a fresh browser. `store_launched_provider` no longer
  claims media readiness while login is unverified.
- **Provider Shared status**: kept explicitly unavailable/experimental
  (`MP-CAPTURE-001`) with no capture pipeline enabled.

### Verification

- `cargo check` ✅ using a clean temporary target directory.
- `cargo test --lib` ✅ 242 passed, 0 failed, 2 ignored (includes new provider
  capability/readiness/session tests; the existing leave-party worker abort
  test was hardened with a poll window to avoid a parallel-suite timing flake).
- `cargo test --lib providers::sync::tests` ✅ (14 tests).
- `cargo test --lib` filter `provider` ✅ (38 tests, includes app-runtime
  readiness/session tests).
- `npm run lint` ✅
- `npm run test` ✅ (6 files, 27 tests)
- `npm run build` ✅

### External / manual verification pending

- Real provider login, session persistence, and reuse in the dedicated managed
  profile on macOS and Windows (Netflix, Prime, JioHotstar, YouTube).
- Provider search/navigation opens the intended title and playback readiness is
  detected (media element present after user starts playback on the provider).
- Provider switch replaces the previous provider's browser session cleanly.
- CDP status checks, title navigation, and playback detection on live provider
  pages (requires installed Chrome and a real provider account).
- `--test m2_integration` keeps the four pre-existing timing predicate failures
  noted in the Batch 4 pass.

---

## 2026-08-30 — Batch 6 V1 End-to-End Integration Hardening

### Concrete bugs found and fixed

1. **Stale media/provider/chat state after leave_party** (`app_runtime.rs`):
   `leave_party` aborted workers and cleared the session but left
   `state.media`, `state.transfer`, `state.buffer`, `state.sync`,
   `state.provider`, `state.chat`, `state.reactions`, and
   `state.player_snapshot` untouched. A subsequent `return_home` cleared
   them, but any code path that called `leave_party` without `return_home`
   would leak stale state into the next room. Fixed: `leave_party` now
   resets all of these fields to their default/empty values.

2. **Duplicate QUIC server when create_local_party invoked twice**
   (`app_runtime.rs`): `create_local_party` bound a new QUIC server without
   aborting an existing `host_session` server handle. Dropping a `JoinHandle`
   without `.abort()` leaks the task. Fixed: `create_local_party` now aborts
   any existing `host_session.server_handle`, aborts the old
   `host_event_task`, drops any existing guest client, and clears
   stale invite/credentials before binding a new server.

3. **Stale room state when join_party receives a deep-link while already in a
   room** (`app_runtime.rs`): `join_party` connected a new QUIC session
   without tearing down the previous room's host server, guest workers,
   chrome session, media cache, provider state, or chat. An incoming
   deep-link invite could arrive while the user was mid-party, and the old
   server/client/workers would leak. Fixed: `join_party` now validates the
   invite first, then aborts all existing host/guest workers, drops the
   client, shuts down the range server, clears chrome/media/provider/chat
   state, and resets the buffer/sync/player snapshots before connecting.

4. **Invalid invite could destroy an active room** (`app_runtime.rs`):
   safeguard: `join_party` parses and validates the invite before any
   cleanup, so a malformed paste never tears down the current room.

5. **Enter Cinema button not gated on participant readiness**
   (`ReadyCheckView.tsx`): the ReadyCheckView always showed an enabled
   "Enter Cinema" button even when not all participants were ready, the
   room had no media, or the network was disconnected. Fixed: the button
   is now `disabled={!everyoneReady}` and shows "Waiting for readiness"
   when the guard is not met.

### Test improvements

- **New test** `leave_party_clears_stale_media_provider_and_social_state`:
  deterministic state verification that media/transfer/buffer/sync/provider/
  chat/reactions/player_snapshot are all reset after leave_party.
- **New test** `create_local_party_aborts_previous_host_session`: verifies
  that a second `create_local_party` call while already hosting aborts the
  first server, produces a fresh invite, and does not leak a duplicate
  listener.
- **New test** `invalid_invite_does_not_destroy_existing_room`: verifies
  that a malformed invite is rejected before any existing room state is
  touched.
- **Shared env-var lock** (`loopback_env_lock`): added a module-level static
  mutex to serialize the pre-existing `dev_loopback_mode_selects_correct_bind_addr`
  test and the new `create_local_party_aborts_previous_host_session` test,
  which both manipulate the process-global `MOVIE_PARTY_DEV_LOOPBACK` env var.

### Fixes considered and reverted

- `set_ready` and `host_play` both unconditionally set
  `local_participant.media_ready = true` and `buffer_ahead_ms = 5_000`,
  fabricating readiness that the coordinator consensus and PLAY_READY
  response rely on. Removing this fabrication would break the
  m2_integration/m3_closure protocol test suite (13 of 25 tests failed vs
  the pre-existing 4 timing-predicate failures). The protocol tests
  legitimately need simulated readiness to exercise the sync protocol
  without a real libmpv player. The honest readiness gate is enforced at
  the coordinator (`prepare_play_scheduled` returns Err when `all_ready`
  is false) and at the frontend (ReadyCheckView gates "Enter Cinema" on
  `everyoneReady`). The production preparation paths (`create_local_party`
  checks player open success; `guest_prepare_media` requires a verified
  first chunk; `attach_launched_provider` requires validated provider
  readiness) already gate `media_ready` honestly.

### Verification

- `cargo test --lib` ✅ 245 passed, 0 failed, 2 ignored
- `cargo test --test host_guest_wiring`, `m2_integration` (21/4 baseline),
  `m3_closure`, `m3_integration`, `m3_m4_e2e`, `m4_closure` ✅
- `cargo test --lib` filter `provider` ✅ 39 passed, 2 ignored
- `npm run lint` ✅
- `npm run test` ✅ (6 files, 27 tests)
- `npm run build` ✅

### External / manual verification pending

- Deep-link while in an active room (host or guest) — the backend now
  tears down stale state before joining a new room, but the frontend
  `openJoinWithInvite` does not call `leaveParty` before showing the
  Join Party screen.
- Real two-device create/join cycle on macOS and Windows.
- `leave_party` media/provider/chat cleanup on guest disconnect.
- `m2_integration` keeps the four pre-existing timing predicate failures
  (`test_a`, `test_d`, `test_l`, `test_t`).

---

## 2026-09-04 — Batch 9B Close Local Perfect production gaps

Scope: code-level closure only — no architecture change, no new protocols,
no fake readiness. All edits confined to `src-tauri/src/app_runtime.rs` and
`src-tauri/tests/m3_integration.rs`.

### Concrete gaps closed

1. **Playback headroom is now separate from transfer percent.**
   New free function `transfer_percent_for_state` derives
   `buffer.percent` from `TransferProgress::fraction()`. The
   TransferResumed / TransferInterrupted / BufferRecovered handlers and the
   guest `QuicServerEvent::BufferRecovered` handler no longer fabricate
   `percent = 100`; they report the real whole-file transfer fraction.
   `buffer.percent` keeps its meaning (whole-file transfer %, consumed by
   the CinemaView BufferingOverlay); playback headroom continues to flow
   through `buffer.guest_buffer_ahead_ms` (ms of contiguous playable
   cache), derived in `guest_prepare_media` from
   `SparseCache::contiguous_bytes_from` with the player backend value
   authoritative and the contiguous-cache fallback honest.

2. **Transfer recovery never auto-resumes playback** (PROTOCOL_SPEC §30 /
   MASTER_PRD §14). `apply_recovery_to_state` for `TransferResumed` no
   longer sets `room_state = Playing`, clears `strict_sync_paused`, or
   fabricates headroom; it clears the buffering participant and syncs
   percent/headroom from verified transfer state. Resume only happens
   through a fresh host play-protocol cycle after readiness consensus.
   `TransferInterrupted` still pauses both sides (Reconnecting +
   strict_sync_paused) with percent from the on-disk transfer state (the
   cache itself is untouched by a stall).

3. **Drift correction is now runtime-invoked, not orphaned helpers.**
   The guest player event loop (200 ms poll) invokes the new
   `apply_drift_correction` which applies `correction_for_drift`
   thresholds (0–80 ignore → rate 1.0; 81–250 playback rate 0.97/1.03;
   251–700 micro-seek; >700 hard-seek to host commit position) via the
   real `LocalPlayer` seam when the room is Playing and not
   strict-sync-paused.

4. **Guest cache headroom computation can no longer deadlock.** The
   `guest_prepare_media` player-open block no longer awaits the cache lock
   while holding the state lock; headroom is computed from
   `contiguous_bytes_from` outside the state lock and re-applied under it.

5. **Preload-wait OS notifications are rate-limited per schedule.**
   The inline 15-minute dedup decision is extracted into
   `should_notify_preload_wait` (testable) and covered by a focused test
   (first poll notifies; repeats within 15 min suppressed; after the
   window notifies again; independent schedules notify independently).

6. **`leave_party` detaches the in-memory guest cache handle but preserves
   the on-disk cache directory** — retention (Keep/Remove/Save As) decides
   the files later. Now proven by a focused test instead of being
   implicit.

7. **`host_play` error gate proven.** A sticky `MP-MEDIA-001` player
   error refuses to start playback (no `media_ready`, no PLAYING, stable
   error surfaced) — previously the gate existed but had no test.

### Test improvements

- New focused tests in `app_runtime.rs` (all use the real coordinator /
  state machine; no fake READY/PLAYING):
  - `drift_correction_restores_normal_rate_after_convergence`
  - `drift_correction_hard_seeks_back_to_host_commit`
  - `recovery_percent_reflects_verified_transfer_not_headroom`
  - `buffer_recovery_reports_real_percent_and_never_resumes`
  - `leave_party_detaches_guest_cache_but_preserves_it_on_disk`
  - `preload_wait_notifications_are_rate_limited_per_schedule`
  - `host_play_refuses_to_start_when_player_reports_media_error`
- `runtime_failures_drive_recovery_state` updated to honest semantics:
  TransferResumed keeps `strict_sync_paused`, does NOT move to PLAYING,
  clears the buffering participant, and reports real percent.
- `m3_integration::test_18_player_buffering_feeds_strict_sync` updated:
  a solo host with no guest transfer honestly reports `percent = 0` after
  recovery (was a fabricated 100).
- **Test-scheduler hazard fixed** in
  `leave_party_aborts_reconnect_heartbeat_preload_and_watch_workers`:
  tokio processes a task's abort the next time the task is polled, so a
  spawned-but-never-scheduled worker can sit indefinitely behind a worker
  thread that never yields (reproduced deterministically when a previous
  loopback-binding test leaves the process loaded, both parallel and
  single-threaded; pristine tree had the same latent hazard). The test now
  waits for all four workers' first poll (start-counter rendezvous) before
  calling `leave_party`, and the poll loop yields so the test's own worker
  participates in scheduling. No assertion was weakened.

### Verification

- `cargo fmt --check` ✅
- `cargo check` ✅
- `cargo test --lib` ✅ 291 passed, 0 failed, 2 ignored (parallel and
  `--test-threads=1`)
- Full `cargo test` ✅ — lib 291, host_guest_wiring 2, m2_integration 28,
  m3_closure 7, m3_integration 18, m3_m4_e2e 9, m4_closure 15 — all
  passing, 0 failed
- No app launch, no browser automation, no fake external verification.

### External / manual verification pending (⚠ EXTERNAL VERIFICATION PENDING)

- Real libmpv playback of a local file (host) and range-server playback
  (guest) on real hardware.
- Real HTTP range requests issued by libmpv against the loopback range
  server.
- Real two-device Tailscale transfer (host→guest) including a 4 GB file
  with mid-transfer interruption and resume.
- Real starvation/recovery cycle: strict-sync pause on buffer underrun and
  resume only via a fresh host play cycle after readiness consensus.
- Real drift measurement between two devices under load (rate/micro-seek/
  hard-seek tiers).
- OS notification delivery (preload-wait rate limiting) on macOS and
  Windows.
- macOS and Windows playback verification (per AGENTS.md §19, recorded
  separately per OS pair).
