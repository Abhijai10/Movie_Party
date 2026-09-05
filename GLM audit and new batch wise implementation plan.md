# GLM Audit and New Batch-Wise Implementation Plan — Movie Party V1

**Audit date:** 2026-09-05
**Auditor:** GLM coding agent (DeepSeek Harness session)
**Base commit:** `main` @ `64bf79a` (Batch 10 — Social UX + Ghost/Privacy Closure)
**Companion docs:** `docs/core_docs/V1_COMPLETION_PLAN.md` (canonical plan location), `0_Remaining_Things.md` (findings merged), `docs/core_docs/IMPLEMENTATION_TRACKER.md` (status log)

This document captures the complete output of the 2026-09-05 full-project audit —
every core doc was read (~16,000 lines: MASTER_PRD, PROTOCOL_SPEC, UI_UX_SPEC,
all milestone worklogs, closure audits, runbook, integration status, remaining-work
roadmap, implementation tracker) and every claim was then **verified directly against
the current code** rather than trusted. It contains: (1) what was verified correct,
(2) all 18 findings (P1–P18) with evidence, (3) doc contradictions, (4) decision
points, (5) the batch-wise implementation plan (Batches 11–23), (6) time estimates,
(7) the manual verification matrix, and (8) explicit non-goals.

---

# PART 1 — WHAT THE AUDIT VERIFIED AS CORRECT (do not redo)

These were checked against the code at `64bf79a` and are code-level correct with
focused tests. Any future work in these areas should verify first, not rebuild:

1. **Sync math is spec-exact** (`src-tauri/src/sync/`):
   - Drift bands exactly per MASTER_PRD §20: 0–80 ms ignore / 80–250 ms playback-rate
     0.97–1.03× / 250–700 ms micro-seek / >700 ms hard-seek (`sync/drift.rs`).
   - Clock offset estimation: median-of-best filter (drops worst 20% by RTT),
     quality classification (Excellent/Good/Poor/Unusable) (`sync/clock.rs`).
   - **p95 RTT is real**, not an average (`p95_rtt_us`).
   - Playback lead time = max(750 ms, 2×p95 + 250 ms) clamped to 3 s, per PRD §19.
2. **Drift correction IS runtime-wired** — the old audit claim "helper has no
   runtime caller" is STALE: the guest player event loop calls
   `apply_drift_correction` (rate nudge / micro-seek / hard-seek / restore 1.0×)
   at `app_runtime.rs` ~3960–4010.
3. **Consensus + room state machine**: `all_participants_ready` requires host AND
   guest AND minimum buffer; invalid state transitions rejected (`sync/`).
4. **Chat/reaction protocol limits** (`chat/mod.rs`): 2000-byte body cap
   (MP-CHAT-002), empty-body rejection, exact 6-emoji V1 set, **5 reactions per
   3 s per participant** rate limiter (MP-CHAT-004), unknown reaction ignored
   (MP-CHAT-003).
5. **Preload-start formula** (`scheduling/mod.rs`): transfer time × 1.4 safety
   multiplier + 15-minute margin, exactly per PRD §34. (But see finding P14 — a
   second, bytes-vs-bits-inconsistent duplicate exists in `storage/sqlite.rs`.)
6. **Notifications**: real native dispatch (macOS osascript / Windows PowerShell),
   injectable notifier seam, overdue-schedule scan wired at runtime.
   Delivery on real OS remains manual verification.
7. **Identity**: Ed25519 signing key with the 32-byte seed stored in the REAL
   platform secure store (macOS Keychain via `security` CLI / Windows Credential
   Manager), UUIDv7 device IDs, versioned SQLite migration for `key_label`
   (`secure/mod.rs`, `identity/mod.rs`).
8. **Telemetry**: local-only stdout tracing subscriber, no network sink, log
   redaction (`redact_log_line`), certification/beta-readiness models.
9. **Resilience policy table** (`resilience/mod.rs`): every Phase 28 failure event
   (ChromeCrash, HostCrash, GuestCrash, TransferInterrupted, Tailscale×2,
   NetworkChange, WifiDisconnect, SleepWake, ProviderLogout, ProviderPageClosed,
   PlayerFailure, CacheCorruption, MissingLocalFile) has an explicit recovery
   plan with pause-both semantics. Disconnect + transfer-interrupt are runtime-wired.
10. **Local Perfect pipeline**: BLAKE3 per-chunk hashing, sparse cache with
    resume, manifest validation, 127.0.0.1 range server with token gating,
    software-render (bgr0) native presentation on macOS (CGImage/CALayer) and
    Windows (child HWND + GDI StretchDIBits), loopback bind checks (loopback or
    Tailscale IPv4 only).
11. **Provider adapters**: real CDP command builders for YouTube, Netflix, Prime,
    JioHotstar + generic; real managed-Chrome launch with dedicated profile and
    localhost-only CDP bind verification.
12. **Liveness**: heartbeat 2 s interval / ≈10 s threshold, guest BUFFER_STATUS on
    the 500 ms cadence while Playing, bounded reconnect backoff, cache survives
    reconnect with no completed-chunk refetch, no auto-resume (consensus required).
13. **Batch 10 closures verified holding**: Ghost/Privacy UI-state restore, chat
    arrival semantics (transient reveal, never during modes, unread badge
    survives), call-tile session model, provider select dark scheme + pure model,
    Ready Check footer wrap, local-vs-remote call state separation test.
14. **CI + gates**: macOS + Windows matrix (fmt, clippy -D warnings, tests);
    Windows NSIS installer with bundled `mpv-2.dll`; frontend gates green at
    audit time (92 FE tests, tsc clean, build clean; 300 Rust lib tests).

---

# PART 2 — FINDINGS (P1–P18), PRIORITY ORDER

## High severity

**P1 — Wire protocol violates the locked PROTOCOL_SPEC.**
- Spec: Canonical CBOR, 256 KiB control-message cap (MP-PROTO-004), numeric
  message IDs, envelope with v_major/v_minor/room_id/seq/sender/type.
- Code: `serde_json` production serialization, 2 MiB `MAX_REQUEST_BYTES`,
  string serde tags (`#[serde(tag = "type", content = "payload")]`), envelope
  missing version + room fields. The correct 256 KiB constant
  (`MAX_CONTROL_MESSAGE_BYTES` in `protocol/mod.rs`) is **dead code**.
- **Worse — ID collisions**: code assigns `ReadyState = 200`,
  `BufferStatus = 201`, `ControlRequest = 202`, but the spec registry reserves
  200/201/202/203 for SCHEDULE_CREATE/ACCEPT/UPDATE/CANCEL. §68: "Never redefine
  the meaning of an existing message ID."
- Evidence: `src-tauri/src/network/quic.rs` (write_json/read_json_bytes,
  MAX_REQUEST_BYTES, serde tags), `src-tauri/src/protocol/mod.rs`.
- Resolution requires **ADR-0001** (either migrate to CBOR/numeric IDs or amend
  the spec) — Batch 11.

**P2 — The video call is a local loopback, not a cross-device call.**
- Each device runs `runLocalPeerConnectionLoopback` — TWO local
  RTCPeerConnections talking to each other on the same machine.
- Real `getUserMedia` exists (`acquireRealCallMedia`) but only feeds the
  self-test; received remote signals are stored in snapshots but **never
  applied** to a peer connection; no component consumes `callSignals`.
- The call tile renders a **placeholder icon** — no `<video>`/`<audio>` element
  exists anywhere in `src/`.
- Evidence: `src/call/webrtc.ts`, `src/views/CinemaView.tsx` ~263,
  `src/overlays/CallTile.tsx` ~167–176.
- Fix: true per-device offer/answer/ICE over the existing QUIC relay — Batch 12.

**P3 — Provider Sync never dispatches canonical playback operations.**
- Coordinator commits (play/pause/seek) never reach provider adapters;
  `command_for_action` / play/pause/seek/position/buffer command builders have
  **no runtime callers**; provider position/buffer never feed the sync engine.
- Provider Sync today = managed browser + readiness UI + navigation, NOT
  synchronized playback.
- Evidence: grep of `app_runtime.rs` — only login-check/detect/navigate paths.
- Fix: Batch 14 (YouTube first, per PRD Phase 18/19 order).

**P6 — Missing spec'd screens/components** (V1 checklist blockers):
- Settings (UI_UX_SPEC §55–62: General/Playback/Network/Call/Storage/Providers/
  Privacy/Diagnostics), Schedule form (§18–19 + offline warning §19), First Run
  welcome + prerequisite checks (§10–11), Home "Upcoming" (§53), QR invite (§16),
  Error screen with stable MP codes + actionable options + collapsible Technical
  Details (§64), Debug HUD (§63), window-close-during-party prompt (§69),
  guest media retention prompt Keep/Remove/Save-As (§52 — storage functions
  exist, no UI).
- Evidence: `src/views/` inventory (10 views; no Settings/schedule/error
  components), grep finds no QR/close-prompt/first-run UI.

**P8 — Scheduling has no frontend and no protocol messages.**
- Backend: CRUD, preload calc, overdue scan, notifier seam exist. Frontend:
  zero schedule UI; protocol: SCHEDULE_CREATE/ACCEPT/UPDATE/CANCEL and
  PRELOAD_STATE absent from the wire; notification permission never requested.

**P10 — Provider Shared pipeline is policy-only.**
- `capture/macos/mod.rs` and `capture/windows/mod.rs` are constant-string
  diagnostic plans; `encode/macos/mod.rs` and `encode/windows/mod.rs` are
  **1-line files**; `shared_pipeline.rs` "proof" is an `#[ignore]` test that
  shells out to the ffmpeg CLI. Phases 20–26 (capture → encode → dedicated QUIC
  stream → jitter buffer → decode → present → shared strict sync) have **no
  production code**. Classification policy (black-frame/static detection,
  1-retry-then-Sync-fallback) IS implemented and tested.

## Medium severity

**P4 — Ready Check countdown is a frontend `setTimeout`** (900 ms/step +
620 ms start delay). Spec §25: countdown timing must come from the sync engine;
frontend must not create its own unsynchronized countdown.

**P5 — Chat UI contradicts UI_UX_SPEC §34–36.** Implemented: single 420×500 px
right-anchored panel (`.cinema-chat-overlay`: width min(420px, 100vw−32px),
height min(500px, …)). Spec: compose `min(560px, 70vw)` bottom-center above the
dock; history `min(620px, 75vw) × min(560px, 70vh)` centered translucent;
messages as lower-third ephemeral bubbles (5 s lifetime, max 3 visible, queue,
older fade sooner, subtitle-zone avoidance). **The two core docs contradict
each other** — `0_Remaining_Things.md` §16 codified the 360–420 family.
Decision D2 required; loser doc needs an ADR amendment.

**P7 — Camera card off-spec.** Implemented: width min(24vw, 260px), min-width
210 px, top clamp(22px, 3vw, 44px). Spec §28–29: default 220 px, clamp
120–360 px, 16:9, top-right 24 px margin, position persisted locally
(localStorage — currently session-only), minimized = ~48 px avatar circle
(currently a draggable video stage).

**P9 — SQLite has 4 of 9 PRD §85 tables** (device_identity, schedules,
cache_entries, chat_messages; missing: trusted_peers, rooms, media_items,
providers, network_history). Migrations ARE versioned (001/002 style) ✅.

**P11 — Zero ADRs exist** (only `ADR_TEMPLATE.md`). Multiple locked-decision
deviations (wire format, chat sizes) are undocumented — standing AGENTS §32
process violation. Fix: ADR-0001+ in Batch 11.

**P12 — Release hardening gaps:** no explicit CSP, no `[profile.release]` in
Cargo.toml, no cargo/pnpm audit in CI, macOS signing/notarization absent, macOS
libmpv bundling unverified at runtime.

**P13 — Watchers unwired:** Chrome-crash watcher + player-failure watcher
absent (FINAL_LIVE_INTEGRATION_AUDIT M8 said "NO" — still true); recovery plans
exist for both. Also missing: sleep/wake + network-change revalidation hooks,
media-file-moved (AskHostToLocateFile) prompt.

**P14 — Preload units bug (verify-then-fix):** two duplicate implementations.
`scheduling/mod.rs` is bits-correct (`bytes × 8 / bps`); `storage/sqlite.rs::
calculate_preload_start` divides bytes by goodput_bps **without ×8** — treats
bits as bytes, 8× off at its call site.

**P17 — Adaptive camera ladder not runtime-wired:** tier types + policy exist
(`call::CameraTier`, `encode::` ladder 5.5/4.0/3.0/2.0 Mbps); runtime sender
constraint application, renegotiation on tier change, once-per-event
degradation notice, and the 5 Mbps movie-priority test are missing.

## Low severity / hygiene

**P15 — Delegated visual work** (explicitly non-core, another AI): hero reel
redesign, cinema curtain/entrance sequence.

**P16 — Hygiene:** 405 `._*` AppleDouble files on disk (gitignored, untracked —
safe); UI_UX_SPEC ends with a stray code fence (L1526–29).

**P18 — Manual verification matrix** (0_Remaining §31.1): all 4 OS pairings ×
Local Perfect/chat/call/deep-link/provider-sync + college network + provider
accounts remain pending. By design NOT agent-completable; must never be faked.

---

# PART 3 — DOC CONTRADICTIONS FOUND

1. **Chat sizing:** UI_UX_SPEC §34–36 vs 0_Remaining_Things §16 (P5/D2).
2. **Wire format:** PROTOCOL_SPEC §3/§5/§10/§11 vs implementation (P1/D1).
3. **Camera minimized:** spec §29 (48 px circle) vs implemented draggable stage.
4. **Stale status docs** (resolved this audit): the 2026-08-19/21-era milestone
   worklogs, FINAL_LIVE_INTEGRATION_AUDIT, REMAINING_MILESTONES_EXECUTION,
   INTEGRATION_RUNBOOK, INTEGRATION_STATUS contradicted current code (claimed
   scheduling/notifications unwired, no drift caller, etc.) — **deleted**
   2026-09-05 (commit `bf705b1`), findings merged into the living docs
   (commit `2a0cd2e`). IMPLEMENTATION_TRACKER + 0_Remaining_Things are the only
   status authorities; the four core specs + V1_COMPLETION_PLAN + this doc are
   the planning set.

---

# PART 4 — DECISION POINTS (user input needed before Batch 11)

- **D1 — Protocol (P1):**
  - Option A: migrate to CBOR + numeric IDs + 256 KiB (spec-faithful; ~2–3 days;
    touches all wire code + tests + doc updates on both sides).
  - Option B (recommended): ADR-0001 amends the spec to JSON + tagged envelope
    as the V1 wire (both endpoints ship in lockstep; encoding is not
    interop-visible across the network), while fixing the real hazards NOW:
    ID collisions out of the 200–204 scheduling range, envelope gains
    v_major/v_minor/room_id, 2 MiB documented, CBOR deferred to V2.
  - Per AGENTS §1: no silent change; ADR either way.
- **D2 — Chat layout (P5):** A: follow UI_UX_SPEC (compose/history split +
  lower-third bubbles). B: keep the current 420-family panel as the history
  surface and add only the missing lower-third transient bubbles. Either way one
  core doc gets an ADR amendment.
- **D3 — Provider Shared scope (P10):** A: full V1 transport build. B
  (recommended): V1 ships Shared as EXPERIMENTAL diagnostic (capture + encode +
  30 s sample + black-frame classification + explicit Sync fallback offer);
  full transport only if the real-DRM spike passes — matches PRD §109 release
  rule and Phase 20/21 spike-first intent.
- **D4 — Batch cadence:** one batch per agent session with user review + push
  between; or two related small batches per session.

---

# PART 5 — BATCH-WISE IMPLEMENTATION PLAN (Batches 11–23)

Rules for every batch (AGENTS §20/§33/§34): `pnpm lint` + `tsc --noEmit` +
`pnpm test` + `pnpm build` + (Rust `cargo fmt --check` / `clippy -D warnings` /
`test` when src-tauri changes) all green; focused tests for new logic; tracker
+ remaining-work docs updated truthfully; commit aligned to the architectural
unit; no redesign of locked decisions; no purple-button/Emergent redesign; no
silent fallbacks; manual items recorded as ⚠ EXTERNAL VERIFICATION PENDING.

### Batch 11 — Protocol Truth & ADR Foundation (P1, P11) — ~1 session
- Decide D1; write ADR-0001 (wire format) + ADR-0002 (message-ID registry:
  move ReadyState/BufferStatus/ControlRequest out of the 200–204 scheduling
  range or formally reserve it) + ADR-0003 if D2 chosen.
- Implement: envelope version + room_id fields; size-limit enforcement at the
  spec'd value for control messages; ID fix; malformed-input property tests
  (oversized, unknown-type, missing-field, bad-seq per PROTOCOL_SPEC §67);
  make `protocol/mod.rs` constants actually used.
- Accept: round-trip + malformed-input tests pass; collisions eliminated;
  protocol docs match implementation (§69 DoD).
- Est: 0.5–1 focused day.

### Batch 12 — Real Cross-Device Call (P2; PRD Phase 14) — ~1–2 sessions
- Replace loopback with a true per-device RTCPeerConnection driven by relayed
  signals: host sends OFFER on call start → guest ANSWERs → ICE both ways (the
  QUIC relay already exists via `submit_call_signal`).
- `<video>`/`<audio>` elements in the CallTile (remote) + small self-view;
  `track.enabled` toggles stay local-only; teardown + mode downgrades
  (V+V → voice-only → off) with explicit re-enable.
- Keep the loopback path as a dev-only self-test behind a flag.
- Tests: signal-application state machine (offer/answer/ICE ordering,
  duplicate/late ICE), constraint mapping per mode.
- Est: 1–2 focused days.

### Batch 13 — Adaptive Camera + Call Polish (P17; Phase 15) — ~1 session
- Camera tier ladder (480p20 / 360p15 / 240p10–15 / frozen per PRD §41) driven
  by goodput/buffer/RTT; renegotiation on tier change; once-per-event
  "camera quality reduced to protect movie playback" notice; camera degrades
  before movie (policy unit tests with a 5 Mbps cap).
- Est: 0.5–1 focused day.

### Batch 14 — Provider Sync Runtime Completion (P3; Phase 19) — ~1–2 sessions
- Canonical commit dispatch → provider adapter CDP commands (play/pause/seek on
  host; guest via existing CONTROL_REQUEST path); provider position/buffer
  polling feeding the coordinator (PLAYER_STATE / BUFFER_LOW equivalents);
  readiness gate = PLAYBACK_READY before start; provider-mode badge
  (lobby-only, per §44); YouTube first, then Netflix/Prime/Hotstar selectors
  behind the same adapter interface; Shared stays EXPERIMENTAL/blocked.
- Tests: adapter dispatch per provider, readiness gating, provider-error
  mapping (MP-PROVIDER-003/004), no-silent-fallback tests.
- Est: 1–2 focused days (YouTube locally testable; DRM providers manual).

### Batch 15 — Frontend Screens I: Settings, First Run, Error (P6) — ~2 sessions
- Settings shell + sections (General/Playback/Network/Call/Storage/Providers/
  Privacy/Diagnostics) mostly wired to existing backend state (display name,
  cache stats + clear, provider cards + reset-with-confirm, diagnostics export,
  Strict Sync visible but not disableable per §56).
- First Run welcome + prerequisite checks (Tailscale/libmpv/Chrome/permissions)
  with truthful statuses and NO premature permission prompts (§11 rule).
- Error screen component: stable MP code + human message + actionable options +
  collapsible Technical Details, wired to snapshot errors.
- Debug HUD (dev-gated, §63); window-close-during-party prompt (Leave vs
  End-For-Everyone host distinction, §69).
- Tests: pure state models for settings sections, error mapping, first-run
  check state.
- Est: 1–2 focused days.

### Batch 16 — Scheduling Frontend + Protocol (P8, P14; Phase 9) — ~1–2 sessions
- Fix the preload units duplication: one canonical implementation, delete the
  other, test both call sites (P14 — 8× bug).
- Schedule form in Create Party (date/time/media/guest/call mode; estimated
  transfer; recommended preload start; earlier-only adjustment with warning);
  Home "Upcoming" cards with preload %; offline warning (§19); notification
  permission request; SCHEDULE_CREATE/ACCEPT/UPDATE/CANCEL + PRELOAD_STATE
  wire messages per the (post-Batch-11) registry; guest persists schedule +
  registers reminders.
- Est: 1–2 focused days.

### Batch 17 — Chat & Cinema Spec Alignment (P4, P5, P7) — ~1 session
- Lower-third ephemeral message bubbles (5 s, max 3 visible, queue, older fade
  sooner, subtitle-zone bottom-15% avoidance, reduced-motion respected).
- Backend-driven Ready-Check/resume countdown: coordinator emits the 3-2-1
  execute_at schedule; the frontend animates from the received commit, never a
  local-only timer.
- Camera card spec values: 220 px default, 120–360 clamp, 16:9, localStorage
  position persistence, 48 px minimized circle; compose/history split if D2=A.
- Est: 1 focused day.

### Batch 18 — Resilience Watchers + Recovery UX (P13; Phase 28) — ~1 session
- Chrome-crash watcher (session liveness poll → ChromeCrash → relaunch +
  readiness requirement); player-failure watcher (→ reopen + readiness).
- Disconnect overlay: "<peer> disconnected / movie paused / Reconnecting…" +
  grace period + host-only Continue Without Guest (§40).
- Sleep/wake + network-change revalidation hooks; media-file-moved prompt
  (AskHostToLocateFile).
- Est: 1 focused day.

### Batch 19 — macOS Provider Shared Spike (P10; Phases 20–22 macOS) — ~2 sessions
- Real ScreenCaptureKit app capture + app audio (SCK bindings, or a documented
  ffmpeg-CLI bridge as the V1 diagnostic mechanism); VideoToolbox encode (or
  ffmpeg h264_videotoolbox diagnostic); 30 s sample → classification (existing
  black-frame/static detection); capture-permission flow + MP-CAPTURE-001/002;
  failure policy = 1 diagnostic retry → offer Sync Mode explicitly, no
  auto-restart (§69); "Shared unavailable" + fallback UI. Diagnostic-only
  per D3-B.
- Accept: proof artifact + per-provider classification (YouTube non-DRM vs
  Netflix/Prime/Hotstar DRM reality recorded honestly, per OS).
- Est: 1–2 focused days + mandatory manual capture runs.

### Batch 20 — Provider Shared Transport + Sync (Phases 23–26) — CONDITIONAL
- Gated on Batch 19 passing on real DRM content + D3=A.
- Dedicated QUIC media stream (models exist in `shared_stream.rs`), 5 s
  presentation buffer, host loopback consumption (same PTS both sides), guest
  decode + present, source-pause + strict-sync integration, automatic quality
  ladder runtime (5.5/4.0/3.0/2.0 Mbps, 30 fps, no manual selector), three-
  timeline bookkeeping (Source/Encoded/Presentation).
- Est: 2–3 focused days.

### Batch 21 — Windows Provider Shared Spike (Phases 20–22 Windows) — ~1–2 sessions
- Windows.Graphics.Capture + WASAPI app loopback + Media Foundation encode,
  same diagnostic pattern as Batch 19. Requires a physical Windows machine.
- Est: 1–2 focused days + manual.

### Batch 22 — Release Hardening (P12; Phase 32 subset) — ~1 session
- Explicit CSP (documented, tested against the running app); `[profile.release]`;
  cargo audit + pnpm audit wired into CI (advisory, non-blocking first); macOS
  libmpv bundling verification + startup detection + user-safe unavailable
  state; version + release-notes hygiene; privacy/data-handling notice;
  diagnostics-export UI hook.
- Est: 1 focused day.

### Batch 23 — Regression + Beta Prep (Phases 29–31) — user-driven, agent-assisted
- Automated chaos tests where feasible (drop packets mid-seek, RTT change
  mid-movie, disconnect during buffer recovery — extend the m2 simulator);
  10 Mbps throttled runs; the full §31.1 manual matrix executed with results
  recorded per cell; beta build cut.

---

# PART 6 — TIME ESTIMATE

Assumptions: one agent session per batch (a few focused hours) + user
review/push between; manual verification runs happen alongside; hardware (a
Windows machine + a second device or the partner's machine) available for the
manual gates.

| Track | Batches | Focused agent-days | Elapsed (part-time cadence) |
|---|---|---|---|
| Core closure (Local Perfect + real call + Provider Sync + missing UI + resilience + hardening) | 11–18, 22 | ~8–11 | ~2–3 weeks |
| Provider Shared experimental (diagnostic per D3-B) | 19, 21 | ~2–4 | ~1–2 weeks |
| Provider Shared full transport (only if spike passes, D3-A) | 20 | ~2–3 | +1–2 weeks |
| Manual verification matrix (user-driven) | 23 | n/a | ~1–2 weeks, parallel |
| **V1 code-complete (D3-B: Shared = experimental)** | | **~10–15 focused days** | **~4–6 weeks** |
| **V1 code-complete incl. full Shared transport** | | **~13–18 focused days** | **~5–8 weeks** |

Honest variance: Provider Shared is the PRD's own highest-risk unknown (§116
Risk B). If DRM capture is black on both OSes, V1 ships with Shared =
EXPERIMENTAL/UNSUPPORTED and Sync Mode as the provider story — the plan already
treats that as a valid V1 outcome (PRD §109). The single biggest schedule risk
is not code; it is access to: a Windows machine, real provider accounts, and
the college/restrictive network for Phase 29.

---

# PART 7 — MANUAL VERIFICATION MATRIX (user checklist — never faked by agents)

1. **Local playback:** real movie visible + audio inside Cinema on macOS and
   Windows; resize; pause/play/seek/duration/progress; no premature autoplay;
   end-of-media; overlay composition.
2. **Two-device strict sync (all 4 OS pairings):** invite → join → ready
   consensus → synchronized start → pause/seek → guest buffer-low pauses host →
   recovery consensus → disconnect → reconnect → long-duration drift < 100 ms p95.
3. **Call:** camera/mic permission only on explicit enable; defaults off; local
   toggles affect only local outgoing media; remote indicators; cross-device
   video/audio; Ghost/Privacy live behavior with real OS prompts.
4. **Deep links:** installed macOS app (cold + running), Windows installer
   protocol registration, full invite preserved, Lobby transition.
5. **Providers:** per-provider login on the provider's own page, session
   persistence, title navigation, sync operations, expiry handling, both OSes.
6. **Notifications:** macOS + Windows delivery + permission prompts.
7. **Network:** Tailscale path (DIRECT vs DERP), restrictive college network,
   Wi-Fi outage, sleep/wake, packet loss.
8. **Shared spike (Batches 19/21):** real capture permission dialog, DRM
   black-frame reality per provider, 30 s samples recorded.

---

# PART 8 — EXPLICITLY NOT IN V1 (do not build)

Per MASTER_PRD §111/§112: music/Spotify/Apple Music, mobile apps, Linux,
smart TV, >2 users, cloud accounts, public matchmaking, cloud movie storage,
payments, public rooms, AI recommendations, watch-history sync, social
profiles, P2P swarm, shared playlists, voice activation, watchlists, multiple
camera layouts, manual quality selector, 1080p webcam, auto-updater,
notarization before release-readiness, public marketing.

---

# PART 9 — MAINTENANCE RULES

- Update IMPLEMENTATION_TRACKER after every batch; 0_Remaining_Things stays the
  master roadmap; mark manual items ⚠ EXTERNAL VERIFICATION PENDING; never
  claim unrun tests.
- ADRs accumulate in `docs/architecture/adr/` (ADR-0001+). Every deviation
  discovered later gets an ADR, not a silent code change.
- Before fixing any old audit claim, revalidate against the current branch —
  several historical findings proved stale this audit (drift caller, heartbeat
  scheduling, scheduling/notifications wiring).
