# Movie Party — V1 Completion Plan (Audit + Batched Implementation Roadmap)

Generated: 2026-09-05, after a full audit of every core doc (MASTER_PRD, PROTOCOL_SPEC,
UI_UX_SPEC, all milestone worklogs, FINAL_LIVE_INTEGRATION_AUDIT, INTEGRATION_RUNBOOK,
0_Remaining_Things, IMPLEMENTATION_TRACKER) verified directly against the current code
on `main` @ `64bf79a`.

This is the working plan for reaching V1 completion as defined by MASTER_PRD §113
(Final V1 Functional Checklist) and §117 (Success Definition).

---

# PART 1 — AUDIT RESULTS

## 1.1 Verified correct (code-level, tested — do not redo)

- **Sync math is spec-exact**: drift bands 0–80/80–250/250–700/>700 ms with
  0.97–1.03 rate correction (sync/drift.rs); clock offset median-of-best-80%-RTT
  filter; p95 RTT; lead time max(750ms, 2×p95+250ms) clamped 3s (sync/clock.rs).
- **Drift correction is runtime-wired** (old audit claim "no runtime caller" is STALE):
  the player event loop calls `apply_drift_correction` → rate nudge / micro-seek /
  hard-seek / restore 1.0x (app_runtime.rs ~3960–4010).
- **Consensus + state machine**: `all_participants_ready` = host AND guest AND
  min-buffer; RoomState machine rejects invalid transitions.
- **Chat/reactions limits**: 2000-byte body cap, empty-body rejection, exact 6-emoji
  set, 5-per-3s rate limiter (chat/mod.rs), unknown reaction → MP-CHAT-003.
- **Preload formula** (scheduling/mod.rs): bits-correct transfer time × 1.4 + 15-min
  margin per PRD §34. ⚠ but see finding P14 (duplicate implementation in
  storage/sqlite.rs is bytes/bits inconsistent).
- **Notifications**: real native dispatch (osascript / PowerShell), notifier seam,
  overdue-schedule scan wired (app_runtime.rs 1109+). Delivery unverified on real OS.
- **Identity**: Ed25519 seed in real Keychain/Credential Manager (secure/mod.rs),
  key_label schema migration, UUIDv7.
- **Telemetry**: local-only tracing subscriber, log redaction, certification/beta
  readiness models; no network sink.
- **Resilience policy table**: all Phase 28 failure events have explicit recovery plans
  (resilience/mod.rs); disconnect + transfer-interrupt are runtime-wired.
- **Local Perfect pipeline**: BLAKE3 chunk hashing, sparse cache, resume, manifest,
  127.0.0.1 token-gated range server, SW-render (bgr0) presentation on macOS
  (CGImage) and Windows (GDI StretchDIBits).
- **Provider adapters**: real CDP command builders for YouTube/Netflix/Prime/JioHotstar
  + generic; real Chrome launch with dedicated profile + localhost-only CDP bind check.
- **Liveness**: heartbeat 2s/10s, BUFFER_STATUS 500ms cadence, bounded reconnect
  backoff, cache survives reconnect, no auto-resume (consensus required).
- **Ghost/Privacy, call-tile UX, chat arrival semantics, provider select** — closed in
  Batch 10 (this branch, commit 64bf79a).
- **CI**: macOS + Windows matrix (fmt, clippy -D warnings, tests); Windows NSIS
  installer with bundled mpv-2.dll; frontend gates green (92 FE tests, tsc clean,
  build clean, 300 Rust lib tests).

## 1.2 Findings — violations, gaps, bugs (priority order)

| ID | Finding | Evidence | Severity |
|----|---------|----------|----------|
| P1 | **Wire protocol violates locked PROTOCOL_SPEC**: production uses JSON + 2 MiB limit + string serde tags; spec mandates Canonical CBOR + 256 KiB + numeric IDs. Envelope lacks v_major/v_minor/room_id. `MAX_CONTROL_MESSAGE_BYTES` (256 KiB) is a dead constant. **ID collisions**: code ReadyState=200/BufferStatus=201/ControlRequest=202 redefine the spec's SCHEDULE_CREATE/ACCEPT/UPDATE range (violates §68 "never redefine an existing message ID"). | network/quic.rs (write_json/read_json_bytes, MAX_REQUEST_BYTES=2MiB, `#[serde(tag="type")]`), protocol/mod.rs (dead 256KiB + colliding IDs) | HIGH |
| P2 | **Video call is a local loopback, not cross-device**: each device creates two local RTCPeerConnections talking to each other; real `getUserMedia` exists but only feeds the self-test; received remote signals are stored but never applied; CallTile renders a placeholder icon — no `<video>`/`<audio>` element exists anywhere in src/. | src/call/webrtc.ts runLocalPeerConnectionLoopback, CinemaView 263, CallTile 167–176; grep: zero srcObject/video elements | HIGH |
| P3 | **Provider Sync canonical ops not wired**: coordinator commits never dispatch play/pause/seek to provider adapters; provider position/buffer never feed the sync engine. Runtime only does login-check/detect/navigate. Sync mode is currently "browser + readiness UI", not synchronized playback. | grep: no callers of command_for_action/play_command/pause/seek in app_runtime.rs | HIGH |
| P4 | **Ready Check countdown is a frontend setTimeout** (900ms/step), spec §25 requires sync-engine-driven countdown. | ReadyCheckView.tsx 38–54 | MED |
| P5 | **Chat UI contradicts UI_UX_SPEC §34–36**: implemented = one 420×500 right-anchored panel; spec = compose min(560px,70vw) bottom-center + history min(620px,75vw)×min(560px,70vh) centered + lower-third ephemeral bubbles (5s life, max 3, queue, subtitle-zone avoidance). **Docs also contradict each other** (0_Remaining_Things §16 specifies the 360–420 family). | styles.css .cinema-chat-overlay; ChatOverlay.tsx | MED (needs decision) |
| P6 | **Missing spec'd screens/components**: Settings (§55–62: General/Playback/Network/Call/Storage/Providers/Privacy/Diagnostics), Schedule form (§18–19), First Run + prerequisite checks (§10–11), Home "Upcoming" (§53), QR invite (§16), Error screen with MP codes + Technical Details + actionable options (§64), Debug HUD (§63), window-close-during-party prompt (§69), guest retention prompt Keep/Remove/Save-As (§52; storage functions exist, no UI). | src/views inventory (10 views only); no Settings/schedule/error components | HIGH (V1 checklist blockers) |
| P7 | **Camera card off-spec**: width min(24vw,260px)/min-210px vs spec 220px default, 120–360 clamp, 16:9; position persists only per session (spec: persist locally); minimized shows a draggable stage, spec wants ~48px avatar circle. | styles.css .camera-card; CallTile.tsx | LOW-MED |
| P8 | **Scheduling has no frontend and no protocol messages**: SCHEDULE_CREATE/ACCEPT/UPDATE/CANCEL/PRELOAD_STATE absent from wire; notification-permission flow not surfaced in UI. | quic.rs message enums; no schedule UI in src/views | HIGH |
| P9 | **SQLite has 4 of 9 PRD §85 tables** (device_identity, schedules, cache_entries, chat_messages): missing trusted_peers, rooms, media_items, providers, network_history. | storage/sqlite.rs MIGRATION_001 | MED |
| P10 | **Provider Shared pipeline is policy-only**: capture/macos + capture/windows are constant strings; encode/macos + encode/windows are 1-line files; shared_pipeline.rs "proof" is an #[ignore] test invoking ffmpeg CLI. Phases 20–26 (capture → encode → QUIC stream → jitter buffer → decode → present → shared strict-sync) have no production code. | capture/*, encode/*, shared_pipeline.rs | HIGH (biggest remaining build) |
| P11 | **Zero ADRs exist** (only the template). Multiple locked-decision deviations (JSON wire, 2MiB, reconnect scope, chat sizes) are undocumented — a standing AGENTS §32 process violation. | docs/architecture/adr/ contains ADR_TEMPLATE.md only | MED (process) |
| P12 | **Release hardening gaps**: no CSP, no release cargo profile, no cargo/pnpm audit in CI, macOS signing/notarization absent, macOS libmpv bundling unverified. | tauri.conf.json security block empty; Cargo.toml no [profile.release]; ci.yml | MED |
| P13 | **Watchers unwired**: Chrome crash watcher + player failure watcher (FINAL_LIVE_INTEGRATION_AUDIT M8 "NO") still absent; recovery_plan entries exist for both. | resilience/mod.rs has plans; no spawn sites | MED |
| P14 | **Preload units bug (verify)**: storage/sqlite.rs `calculate_preload_start` divides bytes by goodput_bps without ×8 (treats bits as bytes); scheduling/mod.rs is bits-correct. One of the two duplicate implementations is 8× wrong at the call site. | sqlite.rs 480–508 vs scheduling/mod.rs 49–62 | MED |
| P15 | Delegated visual work (explicitly non-core): Hero reel redesign, cinema curtain/entrance sequence. | 0_Remaining §21–22 | LOW |
| P16 | Hygiene: 405 `._*` AppleDouble files on disk (gitignored, untracked); UI_UX_SPEC has a stray code fence at EOF. | find; spec L1526–29 | LOW |
| P17 | Adaptive-camera ladder (PRD §41/§62 tiers, once-per-event degradation notice, 5 Mbps movie-priority test) — policy + tier types exist; runtime sender wiring external. | call/mod.rs CameraTier; encode/mod.rs | MED |
| P18 | Everything in the manual verification matrix (§31.1: 4 OS pairs × 6 feature families; real provider accounts; college network) remains pending — by design not agent-completable. | 0_Remaining §31 | USER-DRIVEN |

## 1.3 Doc contradictions to resolve while implementing

1. Chat sizing: UI_UX_SPEC §34–36 vs 0_Remaining_Things §16 (P5).
2. Wire format: PROTOCOL_SPEC §3/§5/§10/§11 vs implementation (P1) — ADR required either way.
3. Camera minimized: spec §29 (48px circle) vs implemented draggable stage (P7).
4. FINAL_LIVE_INTEGRATION_AUDIT/REMAINING_MILESTONES_EXECUTION are stale (2026-08-19; predate Batches 1–10) — treat as history, not status. IMPLEMENTATION_TRACKER + 0_Remaining are authoritative.

---

# PART 2 — DECISION POINTS (need user input before/early in execution)

- **D1 Protocol (P1)** — Option A: migrate to CBOR + numeric IDs + 256 KiB (spec-faithful;
  ~2–3 days, touches all wire code + tests + both docs). Option B (recommended): ADR-0001
  amends the spec to JSON + tagged envelope as V1 wire (both endpoints ship in lockstep,
  encoding is not interoperability-visible), while fixing the real compat hazards now:
  ID collisions out of the 200–204 scheduling range, envelope gains v_major/v_minor/room_id,
  2 MiB documented, CBOR deferred to V2. Per AGENTS §1: no silent change; ADR either way.
- **D2 Chat layout (P5)** — A: follow UI_UX_SPEC (560/620 bottom-center + lower-third
  ephemeral bubbles). B: keep the current 420 family as the history surface and add only
  the missing lower-third transient bubbles. Either way one doc must be amended via ADR.
- **D3 Provider Shared scope (P10)** — A: full V1 vertical slice (Batches 19–21 complete
  transport). B (recommended): V1 ships Shared as EXPERIMENTAL diagnostic (capture +
  encode + 30s sample + black-frame classification + explicit Sync fallback offer), full
  transport only if the spike passes on real DRM content — this matches PRD §109's release
  rule and Phase 20/21 spike-first intent.
- **D4 Batch cadence** — one batch per agent session (a few hours of focused work) with
  user review + push between batches; or 2 related batches per session when small.

---

# PART 3 — BATCHED IMPLEMENTATION PLAN

Rules for every batch (AGENTS §20/§33/§34): lint + tsc + pnpm test + build + (Rust
fmt/clippy/test when src-tauri changes) all green; focused tests for new logic;
IMPLEMENTATION_TRACKER + 0_Remaining_Things updated truthfully; commit aligned to the
architectural unit; no redesign of locked decisions; no purple-button/Emergent redesign;
no silent fallbacks; manual items recorded as ⚠ EXTERNAL VERIFICATION PENDING.

## Batch 11 — Protocol Truth & ADR Foundation (P1, P11) — ~1 session
Scope: decide D1; write ADR-0001 (wire format) + ADR-0002 (message-ID registry
reservation: move ReadyState/BufferStatus/ControlRequest out of 200–204 or reserve
scheduling range) + ADR-0003 if D2 chosen; implement: envelope version + room_id fields,
size-limit enforcement at the spec'd value for control messages, ID fix, malformed-input
property tests (oversized, unknown-type, missing-field, bad-seq per §67), protocol/mod.rs
constants actually used; update PROTOCOL_SPEC/0_Remaining.
Accept: round-trip tests; malformed-input tests; collisions eliminated; docs match code
(PROTOCOL_SPEC §69 DoD bullet).
Est: 0.5–1 focused day.

## Batch 12 — Real Cross-Device Call (P2; PRD Phase 14) — ~1–2 sessions
Scope: replace loopback with true per-device RTCPeerConnection driven by relayed signals
(offer host→guest on call start, guest answers, ICE both ways — the QUIC relay already
exists); <video>/<audio> elements in CallTile (remote) + small self-view (local);
track.enabled toggles stay local-only; call teardown + mode downgrades (V+V→voice→off);
keep the loopback path as a dev-only self-test behind a flag.
Tests: signal-application state machine (offer/answer/ICE ordering, duplicate/late ICE),
constraint mapping per mode; WebRTC loopback dev test retained.
Accept: two machines can hold a call (manual), code-level: both signal directions
applied to a real PC, tracks rendered.
Est: 1–2 focused days.

## Batch 13 — Adaptive Camera + Call Polish (P17; Phase 15) — ~1 session
Scope: camera tier ladder (480p20/360p15/240p10–15/frozen per PRD §41) driven by
goodput/buffer/RTT; renegotiation on tier change; once-per-event "camera quality reduced"
notice; camera degrades before movie (priority policy unit tests with 5 Mbps cap).
Est: 0.5–1 focused day.

## Batch 14 — Provider Sync Runtime Completion (P3; Phase 19) — ~1–2 sessions
Scope: canonical commit dispatch → provider adapter commands (play/pause/seek via CDP
on host; guest requests via existing CONTROL_REQUEST path); provider position/buffer
polling → PLAYER_STATE/BUFFER_LOW equivalents feeding the coordinator; readiness =
PLAYBACK_READY gate for start; provider-mode badge (lobby-only); YouTube first, then
Netflix/Prime/Hotstar selectors behind the same adapter interface; Shared mode stays
explicitly EXPERIMENTAL/blocked until Batch 19+.
Tests: adapter command dispatch per provider, readiness gating, provider-error mapping
(MP-PROVIDER-003/004), no-silent-fallback tests.
Est: 1–2 focused days (YouTube locally testable; DRM providers = manual).

## Batch 15 — Frontend Screens I: Settings, First Run, Error (P6; Phases 16–17/§55–64) — ~2 sessions
Scope: Settings shell + sections (General/Playback/Network/Call/Storage/Providers/
Privacy/Diagnostics) — most fields wired to existing backend state (name, cache stats +
clear, provider cards + reset-with-confirm, diagnostics export button, Strict Sync shown
non-disableable); First Run welcome + prerequisite checks (Tailscale/libmpv/Chrome/
permissions with truthful statuses, no premature permission prompts); Error screen
component (stable MP code, human message, actionable options, collapsible technical
details) wired to snapshot errors; Debug HUD overlay (dev-gated); window-close-during-
party prompt (Leave vs End-for-everyone host distinction).
Tests: pure state models for settings sections, error mapping, first-run check state.
Est: 1–2 focused days.

## Batch 16 — Scheduling Frontend + Protocol (P8, P14; Phase 9) — ~1–2 sessions
Scope: fix the preload units duplication (one canonical implementation, delete the
other, test both call sites); Schedule form in Create Party (date/time/media/guest/call
mode, estimated transfer, recommended preload start, earlier-only adjustment with
warning); Home "Upcoming" cards with preload %; offline warning; notification permission
request; wire SCHEDULE_CREATE/ACCEPT/UPDATE/CANCEL + PRELOAD_STATE messages per spec
IDs (post-Batch-11 registry); guest persists schedule + registers reminders.
Est: 1–2 focused days.

## Batch 17 — Chat & Cinema Spec Alignment (P4, P5, P7) — ~1 session
Scope per D2: lower-third ephemeral message bubbles (5s, max 3 visible, queue, older
fade sooner, subtitle-zone bottom-15% avoidance, reduced-motion respected); backend-
driven Ready-Check/resume countdown (coordinator emits 3-2-1 execute_at schedule;
frontend animates from received commit, never a local-only timer); camera card 220px
default + 120–360 clamp + 16:9 + localStorage persistence + 48px minimized circle
option; chat compose/history split if D2=A.
Est: 1 focused day.

## Batch 18 — Resilience Watchers + Recovery UX (P13; Phase 28) — ~1 session
Scope: Chrome crash watcher (session liveness poll → ChromeCrash → relaunch + readiness
require), player failure watcher (error states → PlayerFailure → reopen + readiness);
disconnect overlay ("<peer> disconnected / movie paused / Reconnecting…" + grace +
host-only Continue Without Guest); sleep/wake + network-change revalidation hooks;
media-file-moved prompt (AskHostToLocateFile).
Est: 1 focused day.

## Batch 19 — macOS Provider Shared Spike (P10; Phases 20–22 macOS) — ~2 sessions
Scope: real ScreenCaptureKit app capture + app-audio (screencapturetool/SCK bindings or
documented ffmpeg-CLI bridge as the V1 diagnostic mechanism), VideoToolbox encode
(or ffmpeg h264_videotoolbox as diagnostic), 30s sample → classify (black-frame/static
detection already exists), capture permission flow + MP-CAPTURE-001/002 errors,
capture-failure policy (1 diagnostic retry → offer Sync Mode, no auto-restart), Shared
unavailable + explicit fallback UI. Diagnostic-only per D3-B.
Accept: proof artifact + classification per provider (YouTube non-DRM vs Netflix/Prime/
Hotstar DRM reality recorded honestly, per OS).
Est: 1–2 focused days + mandatory manual capture runs.

## Batch 20 — Provider Shared Transport + Sync (Phases 23–26) — CONDITIONAL on Batch 19 + D3
Scope: dedicated QUIC media stream (models exist in shared_stream.rs), presentation
buffer (5s intentional), host loopback consumption (same PTS both sides), guest decode +
present, source-pause + strict-sync integration, automatic quality ladder runtime
(5.5/4.0/3.0/2.0 Mbps, 30fps, no manual selector), three-timeline bookkeeping.
Est: 2–3 focused days. Gated: only if capture spike passes on real DRM content.

## Batch 21 — Windows Shared Spike (Phases 20–22 Windows) — ~1–2 sessions
Scope: Windows.Graphics.Capture + WASAPI loopback + Media Foundation encode, same
diagnostic pattern as Batch 19. Requires a physical Windows machine.
Est: 1–2 focused days + manual.

## Batch 22 — Release Hardening (P12; Phase 32 subset) — ~1 session
Scope: explicit CSP (documented, tested against the running app), [profile.release],
cargo audit + pnpm audit wired into CI (advisory, non-blocking first), macOS libmpv
bundling verification + startup detection + user-safe unavailable state, version +
release-notes hygiene, privacy/data-handling notice, diagnostics-export UI hook.
Est: 1 focused day.

## Batch 23 — Regression + Beta Prep (Phases 29–31) — user-driven, agent-assisted
Scope: automated chaos tests where feasible (drop packets mid-seek, RTT change,
disconnect-during-recovery — extend the m2 simulator), 10 Mbps throttled runs, the full
§31.1 manual matrix execution with results recorded per cell, beta build cut.
This batch is mostly manual: 4 OS pairs × Local Perfect/chat/call/deep-link/provider
+ college network + provider accounts.

## Explicitly NOT in V1 (per PRD §111/§112 — do not build)
Music/Spotify, mobile, Linux, >2 users, cloud accounts/payments, matchmaking, watch
history sync, social profiles, P2P swarm, voice activation, manual quality selector,
1080p webcam, public marketing, auto-updater, notarization before release-readiness.

---

# PART 4 — TIME ESTIMATE

Assumptions: one agent session per batch (few focused hours) + user review/push between;
manual verification runs happen alongside; hardware (a Windows machine + second Mac or
the partner's device) available for the manual gates.

| Track | Batches | Focused agent-days | Elapsed (part-time cadence) |
|---|---|---|---|
| Core closure (Local Perfect + real call + Provider Sync + all missing UI + resilience + hardening) | 11–18, 22 | ~8–11 | ~2–3 weeks |
| Provider Shared experimental (diagnostic per D3-B) | 19, 21 | ~2–4 | ~1–2 weeks |
| Provider Shared full transport (only if spike passes, D3-A) | 20 | ~2–3 | +1–2 weeks |
| Manual verification matrix (user-driven) | 23 | n/a | ~1–2 weeks, parallel |
| **V1 code-complete (D3-B: Shared=experimental)** | | **~10–15 focused days** | **~4–6 weeks elapsed** |
| **V1 code-complete incl. full Shared transport** | | **~13–18 focused days** | **~5–8 weeks elapsed** |

Honest variance: Provider Shared is the PRD's own highest-risk unknown (§116 Risk B);
if DRM capture is black on both OSes, V1 ships with Shared=EXPERIMENTAL/UNSUPPORTED and
Sync Mode as the provider story — the plan already treats that as a valid V1 outcome
(PRD §109). The single biggest schedule risk is not code; it is access to: a Windows
machine, real provider accounts, and the college/restrictive network for Phase 29.

---

# PART 5 — MANUAL VERIFICATION MATRIX (user checklist; agents must never fake these)

1. Local playback: real movie visible+audio inside Cinema on macOS and Windows; resize;
   pause/play/seek/duration/progress; no premature autoplay; end-of-media; overlays compose.
2. Two-device strict sync (all 4 OS pairings): invite → join → ready consensus →
   synchronized start → pause/seek → guest buffer-low pauses host → recovery consensus →
   disconnect → reconnect → long-duration drift <100 ms p95.
3. Call: camera/mic permission only on explicit enable; defaults off; local toggles
   affect only local outgoing media; remote indicators; cross-device video/audio;
   Ghost/Privacy live behavior with real prompts.
4. Deep links: installed macOS app (cold + running), Windows installer protocol
   registration, full invite preserved, Lobby transition.
5. Providers: per-provider login on provider's own page, session persistence, title
   navigation, sync operations, expiry handling, both OSes.
6. Notifications: macOS + Windows delivery, permission prompts.
7. Network: Tailscale path (DIRECT vs DERP), restrictive college network, Wi-Fi outage,
   sleep/wake, packet loss.
8. Shared spike (Batch 19/21): real capture permission dialog, DRM black-frame reality
   per provider, 30s samples recorded.

# PART 6 — MAINTENANCE

- Update IMPLEMENTATION_TRACKER after every batch; keep 0_Remaining_Things as the master
  roadmap; mark manual items ⚠ EXTERNAL VERIFICATION PENDING; never claim unrun tests.
- Archive FINAL_LIVE_INTEGRATION_AUDIT + REMAINING_MILESTONES_EXECUTION as historical
  (2026-08-19) or refresh them at Batch 23.
- ADRs accumulate in docs/architecture/adr/ (ADR-0001+). Every deviation discovered later
  gets an ADR, not a silent code change.
