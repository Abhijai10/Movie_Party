# Movie Party — Master Remaining Work & Production Roadmap

> **Purpose:** This is the living source of truth for everything still required to finish Movie Party V1.  
> **Scope:** Two-person private desktop watch parties on macOS and Windows.  
> **Current branch:** `main`  
> **Latest checkpoint:** 2026-09-05 full-project audit → V1 completion plan (Batches 11–23); Batch 10 closed desktop/runtime + social UX/privacy (code-level)  
> **Rule:** “Code exists” is not the same as “production-ready.” Items that need real devices, real accounts, real media, or OS behavior remain pending until manually verified.

---

## Status Legend

| Status | Meaning |
|---|---|
| ✅ | Implemented and code-level validation completed |
| 🟡 | Partially implemented, incomplete, or still needs integration |
| 🔴 | Confirmed blocker / confirmed bug |
| 🧪 | Manual or cross-device verification still required |
| 🔮 | Planned future implementation |
| 🎨 | Visual/design task intentionally delegated to another AI |
| ⚠️ | Audit finding that must be re-validated against the current branch before fixing |

---

# 1. Executive Snapshot

Movie Party has progressed beyond the prototype/UI stage. The room flow, QUIC architecture, strict-sync foundations, local media transfer/cache foundations, provider-mode separation, deep-link plumbing, chat/reaction foundations, call UI foundations, scheduling core, and major runtime hardening work all exist.

The current highest-value work is no longer broad feature creation. It is **closing the remaining production gaps and proving the real vertical slices**.

Batch 6 (2026-08-30) hardened the room lifecycle at the code level:

- `leave_party` now clears stale media, transfer, buffer, sync, provider,
  chat, reactions, and player state so a new room never inherits them.
- `create_local_party` aborts an existing host server before binding a new
  one, preventing duplicate QUIC listeners on repeated create.
- `join_party` validates the invite first, then tears down any previous
  room's server/workers/chrome/media/provider state before connecting, so a
  deep-link join while already in a room does not leak the old session.
- The ReadyCheckView "Enter Cinema" button is now gated on
  `everyoneReady` (participants media-ready + media/provider present +
  network connected), so the UI does not let the user start Cinema before
  readiness consensus.

Batch 7 (2026-08-30) removed fake readiness paths at the code level (no
real-device failure observations were supplied, so a code audit against the
V1 correctness rules was performed):

- `set_ready` no longer fabricates `media_ready = true` /
  `buffer.guest_buffer_ahead_ms = 5_000` / `peer.media_ready = true`
  unconditionally. The local participant is only marked ready when the
  player/media/provider is genuinely usable (`local_media_genuinely_ready`);
  otherwise a stable `MP-MEDIA-001` is returned and readiness is not advanced.
- The host now applies `GuestReadyState` so the guest's genuine readiness
  reaches the host's participant snapshot.
- `CoordinatorStateUpdate` now mirrors the peer's readiness from the
  coordinator's `host_ready`/`guest_ready` flags on both sides.
- m2 protocol tests were updated to wait for genuine guest media readiness and
  report genuine buffer before pressing Ready.

Remaining V1 work is dominated by **real-device and real-account
verification** plus closing the 2026-09-05 audit findings (P1–P18, below).

## 2026-09-05 full-project audit — new findings (P1–P18)

Every core doc was verified against the current code (main @ 64bf79a)
and cross-checked with MASTER_PRD §113, UI_UX_SPEC, and PROTOCOL_SPEC.
The batch plan, estimates, and decision points live in
`docs/core_docs/V1_COMPLETION_PLAN.md`. The stale milestone-era docs
(worklogs, closure audits, runbook, integration status) were deleted the
same day. Highest-impact findings:

1. 🔴 **P1 — Wire protocol violates the locked PROTOCOL_SPEC**: production
   uses JSON + a 2 MiB limit + string serde tags; the spec mandates
   Canonical CBOR + 256 KiB + numeric IDs, and `MAX_CONTROL_MESSAGE_BYTES`
   (256 KiB) sits unused in `protocol/mod.rs`. The wire envelope also lacks
   v_major/v_minor/room_id, and message IDs collide with the spec registry
   (`ReadyState=200 / BufferStatus=201 / ControlRequest=202` redefine the
   SCHEDULE_CREATE/ACCEPT/UPDATE range — §68 forbids redefining message
   IDs). Resolution requires ADR-0001 (migrate to CBOR or amend the spec)
   — Batch 11.
2. 🔴 **P2 — The video call is a local loopback, not cross-device**: each
   device creates two local RTCPeerConnections talking to each other;
   real getUserMedia exists but only feeds the self-test; received remote
   signals are stored but never applied; the call tile renders a
   placeholder icon — no `<video>`/`<audio>` element exists in the app.
   Real offer/answer/ICE over the existing QUIC relay — Batch 12.
3. 🔴 **P3 — Provider Sync never dispatches canonical playback ops**:
   coordinator commits do not reach provider adapters (play/pause/seek
   commands exist with no runtime callers); provider position/buffer never
   feed the sync engine — Batch 14.
4. 🟡 **P4/P5/P7 — Spec-vs-implementation drift in the social UI**:
   Ready Check countdown is a frontend timer (spec: sync-engine-driven);
   chat layout contradicts UI_UX_SPEC §34–36 (see the note in §16 below —
   the two core docs disagree, decision D2 required); camera card is
   24vw/260px vs spec 220px default, 120–360 clamp, locally persisted
   position, 48px minimized circle — Batch 17.
5. 🟡 **P6 — Missing spec'd screens**: Settings (§55–62), Schedule form
   (§18–19), First Run + prerequisite checks (§10–11), Home Upcoming
   (§53), QR invite (§16), Error screen with MP codes + Technical Details
   (§64), Debug HUD (§63), window-close-during-party prompt (§69), guest
   media retention prompt (§52) — Batches 15–16.
6. 🟡 **P8 — Scheduling has no frontend and no protocol messages**
   (SCHEDULE_CREATE/ACCEPT/UPDATE/CANCEL and PRELOAD_STATE absent from the
   wire); notification-permission flow not surfaced — Batch 16.
7. 🟡 **P9 — SQLite has 4 of 9 PRD §85 tables** (device_identity,
   schedules, cache_entries, chat_messages; missing trusted_peers, rooms,
   media_items, providers, network_history) — tracked with Batch 16.
8. 🟡 **P10 — Provider Shared pipeline is policy-only**:
   capture/macos + capture/windows are constant-string plans,
   encode/macos + encode/windows are 1-line files, and the pipeline
   "proof" is an #[ignore] test that shells out to ffmpeg. Phases 20–26
   have no production code — Batches 19–21 (diagnostic-first, honest DRM
   reality per PRD §109).
9. 🟡 **P11 — Zero ADRs exist** (only the template) while multiple locked
   decisions deviate undocumented (wire format, chat sizes) — Batch 11.
10. 🟡 **P12/P13 — Release/watcher gaps**: no CSP, no release cargo
    profile, no dependency audits in CI, macOS signing/notarization
    absent; Chrome-crash + player-failure watchers still unwired
    (recovery plans exist) — Batches 18, 22.
11. 🔴 **P14 — Preload units bug (verify-then-fix)**: two duplicate
    preload-start implementations disagree — `scheduling/mod.rs` is
    bits-correct, but `storage/sqlite.rs::calculate_preload_start`
    divides bytes by goodput_bps without ×8 (treats bits as bytes — 8×
    off at the call site) — Batch 16.
12. 🧪 **P18 — the entire §31 manual verification matrix remains
    pending** (by design; agents must never fake it).

Verified-correct highlights from the same audit (do NOT redo): sync math
is spec-exact (drift bands / p95 RTT / lead-time formula), drift
correction IS runtime-wired (stale claim retired — see §23), chat and
reaction limits, the preload formula in `scheduling/mod.rs`, Ed25519
identity in the real Keychain/Credential Manager, local-only telemetry,
the Phase-28 recovery table, CDP provider adapters, macOS/Windows frame
presentation, heartbeat/buffer cadence, and reconnect semantics.

---

## Highest-priority current items

1. ✅ **Native libmpv video presentation inside the Cinema surface** —
   macOS (NSView/CALayer + CGImage) and Windows (child HWND + GDI
   `StretchDIBits`) both present real SW-rendered frames in code as of
   Batch 10. 🧪 Manual playback proof on each OS remains pending.
2. ✅ **First-run Tailscale onboarding + partner connectivity setup** —
   implemented (four-state detection, setup view, launch official
   installer/sign-in/share flows, reachability verification). Revalidated
   in Batch 10; previously mislabeled as future work. 🧪 Real two-device
   tailnet verification remains pending.
3. ✅ **Video-call local/remote state separation and Lobby/Cinema call UX fixes**
   — closed at code level in Batch 10 (social UX pass): local controls
   toggle only my outgoing devices; the peer tile renders genuine remote
   participant state (display-only, unobtrusive mute indicator); the tile
   is draggable with pointer capture + viewport clamping, sits above
   Lobby/Ready Check/Cinema content, minimizes to a draggable video stage,
   closes locally without hanging up (Call button restores), and now also
   renders on the Ready Check screen. Backend separation is enforced by
   `local_device_toggles_never_mutate_peer_snapshot`. 🧪 Real two-device
   WebRTC/camera/mic verification remains pending.
4. ✅ **Chat size/translucency and Lobby layout cleanup** — the chat overlay
   already met the target family (min(420px, viewport) × min(500px,
   viewport), strong backdrop blur, floating, never a sidebar, Lobby and
   Cinema share the component); Batch 10 centralized the behavior:
   incoming messages reveal a ~5 s transient preview, never reveal while
   Ghost/Privacy hide social UI, set the unread badge that survives the
   modes, manual open/close always wins over the transient timer, and
   typing is never destroyed by auto-hide (the composer only exists in
   manual mode; the preview close path preserves it). The Lobby ready
   footer now wraps so the READY CHECK button can no longer overflow.
   🧪 Visual confirmation on each OS remains pending.
5. ✅ **Streaming-provider selector visual bug** — the select uses
   `color-scheme: dark` with explicit dark colors on the element and its
   options (no white native rectangle, selected text visible), the label
   is associated with the control, and the option list plus visible-value
   resolution come from a tested pure model, so the visible selection is
   never blank while capabilities load. 🧪 Dropdown appearance
   (hover/selected/keyboard) on macOS and Windows remains a manual check.
6. 🧪 **Real local playback validation** (run the app with a real movie on
   macOS, then Windows — the code path is complete; the manual proof is not)
7. 🧪 **Two-device macOS ↔ Windows validation**
8. 🔮 **Scheduled Local Perfect preload completion**
9. 🧪 **Provider Sync login/preparation real-account verification** (code path now wired through the provider readiness state machine)
10. 🟡 **Provider Shared capture → encode → QUIC → guest presentation**
11. ✅ **Ghost Mode final implementation** — Ghost Mode is complete at the
    code level: it hides the local social layer (chat, reactions, call
    tile, controls, diagnostics) via the `social-hidden` class, never
    touches outgoing camera/mic or playback/sync/session, gates the
    chat shortcuts so the keyboard cannot leak the overlay, keeps the
    unread flag alive during the mode, and restores the exact prior UI
    state on exit (manually-opened chat returns; call tile reappears at
    its own saved position/visibility). Privacy Mode stays distinct:
    devices off on entry, never auto-reactivated on exit. 🧪 Real
    camera/mic behavior with OS permission prompts remains pending.
12. 🎨 **Hero reel redesign by another AI**
13. 🎨 **Cinema curtain/countdown visual polish by another AI**
14. ✅ **Audit/security/reliability findings revalidated against the current branch (Batch 7 + Batch 9 + Batch 10)**: deep-link lifecycle, Tailscale boundary, Local Perfect / Cinema / Provider Sync / Call / Ghost / Privacy lifecycles, worker duplication, error/recovery consistency, and security items were all revalidated; only concrete current defects were fixed. Remaining items (CSP, reconnect media rebuild, Chrome crash watchers, release packaging) are documented as release-hardening/manual items, not silent production claims.
15. 🔮 **Packaging/release hardening**

## Batch 10 desktop + runtime closure status (2026-09-05)

Code-level only; no real-device claims:

- ✅ **Windows native presentation implemented**: `display_frame` now
  presents real frames into the existing child HWND via GDI
  `StretchDIBits` (32bpp BI_RGB top-down DIB — byte-identical to mpv
  bgr0, no conversion; mpv stride expressed as the DIB width, mirroring
  the macOS bytesPerRow, so 64-byte-aligned strides render correctly).
  The ungated macOS msg helper that broke the Windows build is fixed.
  Deterministic Windows unit tests + a Windows e2e harness were added.
  🧪 External verification pending: run the app on Windows with a real
  movie.
- ✅ **macOS presentation revalidated**: bgr0→CGImage byte order, layer
  ownership, resize, cleanup all verified correct. Bridge strings updated
  to the truthful "via libmpv SW render" wording.
  🧪 External verification pending: real movie playback.
- ✅ **Guest BUFFER_STATUS cadence wired**: periodic 500 ms worker while
  Playing (PRD §21 / PROTOCOL_SPEC §28), single replace+abort worker,
  cancelled by leave/create/join teardowns, paused/ended rooms silent.
- ✅ **Heartbeat liveness wired**: application-level detection (2 s
  interval, 5-missed ≈10 s threshold per §16), role-aware recovery
  events (guest records HostCrash; host records GuestCrash), recovery
  never auto-resumes — readiness consensus is required again.
- ✅ **Reconnect/lifecycle revalidated**: bounded backoff, single worker,
  cache survives with no completed-chunk refetch, no auto-resume; all
  session workers (including the new buffer-status worker) aborted on
  leave/create/join.
- ✅ **Structured logging wired**: local-only tracing subscriber at INFO
  default (PRD §87), `RUST_LOG` override, no cloud sink, no secrets
  logged.
- ✅ **Chat kept ephemeral per PRD** (V1 is session-scoped chat; the
  SQLite chat functions stay dormant by design).
- ✅ **Readiness revalidated truthful** (no fabricated PLAYING/buffer;
  transfer percent stays separate from playback headroom).
- ⚠ **Protocol reconciliation follow-up documented in the tracker**:
  PROTOCOL_SPEC says CBOR + 256 KiB + numeric IDs; the implementation
  uses JSON + serde tags + a 2 MiB transport limit. The wire format is
  the intentional V1 implementation; reconciling requires a versioned
  protocol batch, not a silent change.

## Batch 7 readiness-honesty status (2026-08-30)

No real-device failure observations were supplied for Batch 7, so a code-level
audit against the V1 correctness rules was run instead. Results:

- ✅ `set_ready` no longer fabricates media readiness (local participant,
  guest buffer, or peer readiness). Readiness now requires genuine
  player/media/provider state (`local_media_genuinely_ready`), with a stable
  `MP-MEDIA-001` on failure.
- ✅ Host applies `GuestReadyState`; both sides mirror peer readiness from
  `CoordinatorStateUpdate`.
- ✅ Regression tests added (5) and existing m2 helpers now wait for genuine
  guest readiness + genuine buffer reporting.
- 🧪 The real two-device READY consensus flow (both devices genuinely ready →
  READYCHECK → Cinema) still requires physical verification.
- 🧪 Real-player buffer reporting through the player event loop →
  `report_buffer_status` still requires physical verification.

## Batch 7 release-hardening audit status (2026-08-30)

The current branch was revalidated against the remaining V1 release risks.
Old audit findings were revalidated against current code, not blindly applied.
Results:

- ✅ **Running-app deep-link join is safe**: `join_party` validates the invite
  before any teardown (an invalid deep link cannot destroy the current room),
  then aborts every owned worker and clears stale media/provider/player/chat
  state before connecting. New m2 test proves a guest already in room A joins
  room B cleanly with no stale state.
- ✅ **Worker duplication fix**: `spawn_player_event_loop` now aborts the
  previous task on replacement (it was the only worker spawn that did not),
  and `create_local_party`'s duplicate-create hygiene block now aborts all
  owned background workers and closes the stale player instead of leaking them.
- ✅ **Call tile session reset on deep link**: `openJoinWithInvite` now resets
  the call tile session like `goJoinParty`, so a join from a deep link cannot
  carry the previous room's call tile UI state into the new room.
- ✅ **Tailscale boundary confirmed**: NOT_INSTALLED / SIGNED_OUT / CONNECTED /
  UNAVAILABLE plus host-create-requires-only-own-readiness and guest
  own-readiness + reachability all confirmed; setup surface is not a permanent
  gate; no rendezvous server / API credentials added.
- ✅ **Local Perfect / Cinema / Provider Sync / Call / Ghost / Privacy
  lifecycles confirmed** with no newly confirmed release blocker at the code
  level (details in IMPLEMENTATION_TRACKER Batch 7 section).
- ✅ **Security revalidation**: QUIC request bounds, range-server bounds,
  malformed-UUID rejection, secret-safe error sanitization, AppleDouble
  hygiene (`.gitignore` covers `._*`/`.DS_Store`; working tree cleaned) all
  confirmed.
- ⚠️ **Reconnect scope (documented)**: reconnect restores transport/peer/clock/
  heartbeat but does not rebuild a fresh media session; the sparse cache and
  transfer worker survive, so reconnect continues the existing transfer. A
  fresh manifest/cache session requires an explicit rejoin.
- ⚠️ **CSP (documented)**: still no explicit CSP; treated as release-hardening,
  not silently applied because it risks breaking the running app.
- 🧪 Everything requiring real devices/accounts/network remains
  MANUAL VERIFICATION REQUIRED; nothing above is PRODUCTION VERIFIED.

---

# 2. Locked V1 Product Definition

The following decisions are considered locked unless an explicit architecture decision record changes them.

## 2.1 Platform and party size

- Exactly **2 users** in V1: Host + Guest.
- Desktop only.
- macOS + Windows only.
- No browser extension.
- No Linux runtime requirement for V1.
- Music support comes later; movie/video V1 comes first.

## 2.2 Core architecture

- Tauri 2 desktop application.
- React + TypeScript frontend.
- Rust + Tokio native core.
- QUIC/Quinn peer transport.
- Tailscale for private network reachability.
- SQLite for local persistent state.
- libmpv for Local Perfect playback.
- Managed Chrome + CDP for provider sessions.
- No rented Movie Party media server.
- No paid TURN/SFU requirement in the locked architecture.
- No migration to WebSockets merely for convenience.

## 2.3 Synchronization

- Host is authoritative by default.
- Shared Controls is OFF by default.
- Guest actions become requests; Host/coordinator produces canonical commits.
- Playback synchronization uses monotonic clocks.
- If either participant cannot continue, **both pause**.
- Guest buffer starvation, player failure, meaningful desync, or disconnect must not allow the host to continue silently.
- Recovery returns through consensus/Ready Check rather than independent autoplay.

## 2.4 Cinema UX

- Movie-first design.
- No permanent sidebar.
- Chat, call, reactions, settings, diagnostics, etc. appear as overlays.
- Camera OFF by default.
- Microphone OFF by default.
- Ghost Mode and Privacy Mode remain separate behaviors.

## 2.5 Provider/security rules

Movie Party must never:

- ask users to type Netflix/Prime/JioHotstar/etc. passwords into Movie Party UI;
- store provider passwords;
- transfer provider cookies between users;
- log auth headers/tokens;
- extract DRM keys;
- hook Widevine/CDMs;
- bypass HDCP/protected surfaces;
- redistribute encrypted provider packets in a way that requires copied decryption credentials.

Provider authentication remains inside the provider’s own page in the dedicated managed-browser profile.

---

# 3. Current Implementation Checkpoints

These are considered implemented at the code level unless later manual testing disproves them.

## 3.1 UI / navigation

- ✅ Home
- ✅ Create Party
- ✅ Join Party
- ✅ Lobby
- ✅ Ready Check
- ✅ Cinema
- ✅ End Party
- ✅ Emergent visual identity integrated
- ✅ Floating chat architecture
- ✅ Reactions
- ✅ Call tile/presence foundation
- ✅ React ErrorBoundary
- ✅ Cinema UI remains overlay-first rather than sidebar-first

## 3.2 Runtime hardening

- ✅ Poison-tolerant `sync_coordinator` mutex handling was added.
- ✅ Major player command failures no longer silently disappear.
- ✅ Stable `MP-MEDIA-*` errors were added around player availability/load/commands.
- ✅ Frontend/backend command error propagation was improved.
- ✅ Player preparation was changed so loading does not intentionally autoplay ahead of synchronization.
- ✅ mpv lifecycle cleanup was hardened.
- ✅ Provider Shared is no longer allowed to falsely present itself as ready.

## 3.3 Invites / deep links

- ✅ Full invite format preserved:
  `movieparty://join/<room_id>#<descriptor>`
- ✅ Shared frontend invite parser.
- ✅ macOS deep-link plumbing.
- ✅ Windows deep-link plumbing through official Tauri mechanisms.
- ✅ Cold-start pending-link path.
- ✅ Already-running deep-link event path.
- ✅ Malformed/incomplete invite validation.
- 🧪 Real installed/bundled macOS deep-link test pending.
- 🧪 Real Windows installer/protocol activation test pending.

## 3.4 Provider source separation

- ✅ `Local Movie`, `Streaming Provider`, and `Third-party Link` are conceptually separated.
- ✅ Provider Sync is represented explicitly.
- ✅ Provider Shared is represented explicitly as Experimental.
- ✅ Unsupported Shared mode is blocked rather than pretending to work.
- ✅ Generic links no longer masquerade as named provider sessions.
- ✅ Provider readiness states (`NOT_STARTED` → `LOGIN_REQUIRED` → `READY` →
  `NAVIGATING` → `PLAYBACK_READY`) drive the Create-Party provider wizard.
- ✅ Login happens on the provider's own page; Movie Party never asks for or
  stores provider passwords/tokens/cookies.
- ✅ Provider Sync room creation is gated on valid provider readiness.
- 🧪 Real provider login/navigation/control remains to be verified manually.

---

# 4. CRITICAL BLOCKER — Native Movie Rendering

## Status

✅ **Code-complete on macOS and Windows (Batch 10)** — 🧪 manual playback proof pending

The previous manual finding (Cinema reachable without a native render
host attached) was fixed in the SW-render architecture: libmpv renders
into an application-owned buffer via the render API
(`MPV_RENDER_API_TYPE_SW`, `bgr0`), and the runtime presents each frame
into a native child surface behind the transparent WebView.

- **macOS**: NSView/CALayer child host; frames become CGImages
  (`kCGImageAlphaNoneSkipFirst | kCGBitmapByteOrder32Little` — the exact
  byte order of mpv bgr0) and update the layer each render tick.
- **Windows**: child HWND (created `WS_CHILD|WS_VISIBLE`, positioned
  `HWND_BOTTOM` behind the WebView); frames are blitted into its client
  rect via GDI `StretchDIBits` with a 32bpp `BI_RGB` top-down DIB header
  (negative `biHeight` — byte-identical to mpv bgr0, no per-pixel
  conversion). Previously `display_frame` was a Windows no-op; that gap
  is closed.

There is exactly one rendering architecture (the SW render path) and one
native presentation surface per platform; no detached mpv window, no
browser video element, no D3D11 layer.

## 4.1 Required macOS architecture

```text
React Cinema surface
        ↓
Tauri native presentation host (NSView/CALayer)
        ↓
libmpv SW render context (bgr0)
        ↓
actual movie frames
```

Implemented exactly as above; revalidated in Batch 10 with no defects
found (byte order, stride-as-bytesPerRow, layer ownership, resize,
cleanup).

## 4.2 Acceptance criteria

Code-level:

- ✅ Movie renders into the Cinema surface architecture (frames presented
  to the native child surface each render tick).
- ✅ No detached mpv window is used (`force-window=no`; `wid` never set).
- ✅ Movie canvas resizes with the Tauri window (bounds validated, finite,
  ≥1px; macOS `setFrame`, Windows `SetWindowPos`).
- ✅ Movie remains behind overlays (macOS `addSubview:...relativeTo:`
  below; Windows `HWND_BOTTOM`; WebView transparent).
- ✅ Play/Pause/Seek drive the real decoder/player (production
  `MpvPlayer` commands; pause-holds-position, seek-lands, resume-advances
  are asserted by the macOS/Windows e2e harnesses when the runtime is
  present).
- ✅ Position and duration reflect the real player (player snapshot).
- ✅ Player errors cannot leave false PLAYING (Batch 9 honesty guards).
- ✅ Video does not autoplay before the authoritative synchronized start
  (host-authoritative commit flow).
- ✅ Cleanup works when ending/leaving/reloading a party (detach +
  DestroyWindow / removeFromSuperview; worker aborts).

Manual (🧪 external verification pending):

- Real movie visibly renders inside Cinema on macOS.
- Real movie visibly renders inside Cinema on Windows.
- Resize keeps the video correctly positioned/scaled on both OSes.

## 4.3 Windows

✅ Code-complete (Batch 10): child HWND + GDI `StretchDIBits` presentation
of the SW-rendered bgr0 buffer; deterministic validation unit tests and a
Windows e2e harness (`tests/windows_native_surface_e2e.rs`, skips without
the bundled runtime/test video).
🧪 External verification pending: run the app on Windows with a real
movie before considering cross-platform Local Perfect complete.

## 4.4 Manual proof required

🧪 Test a real local movie on macOS first, then Windows. Not yet
performed — recorded as EXTERNAL VERIFICATION PENDING, not claimed.

---

# 5. Tailscale First-Run Onboarding and Partner Connectivity

## Status

✅ **Implemented (revalidated Batch 10)** — 🧪 real two-device verification pending

This was previously mislabeled as future work. Current code implements
the four-state onboarding: `TailscaleState` detection
(NotInstalled / SignedOut / Connected / Unavailable), the
`TailscaleSetupView` first-run surface, launching the official installer
/app (`open_tailscale_setup`), and reachability verification before
Create/Join is allowed. Movie Party never collects Tailscale credentials
and never silently installs software.

A fresh Movie Party install does not assume Tailscale is ready.

The onboarding must distinguish four states.

## 5.1 State A — Tailscale not installed

Show a first-run setup screen.

Desired flow:

```text
Movie Party needs a private connection
        ↓
[Set up Tailscale]
        ↓
Open/download official installer
        ↓
OS installation/permission flow
        ↓
Movie Party detects installation
```

Requirements:

- Never silently install software without user consent.
- Provide a secondary **Manual setup** option.
- Do not collect Tailscale credentials inside Movie Party.

## 5.2 State B — Installed but signed out

```text
Tailscale is installed
        ↓
[Sign in to Tailscale]
        ↓
Tailscale/browser handles authentication
        ↓
Movie Party detects signed-in state
```

## 5.3 State C — Signed in but partner is not reachable

This is a distinct requirement.

Simply having two separately signed-in Tailscale installations does not guarantee that the devices can communicate. The users need a valid connectivity relationship, such as being in the same usable tailnet context or using an appropriate device-sharing/invite flow.

Desired Movie Party state:

```text
Tailscale connected
        ↓
Partner not reachable
        ↓
[Connect movie partner]
        ↓
Guide/open official Tailscale share/invite flow
        ↓
Partner accepts
        ↓
Movie Party verifies reachability
```

### V1 principle

Movie Party should automate **detection, guidance, launching official setup pages/tools, and reachability checks**, but should avoid introducing Tailscale admin API keys/OAuth complexity unless a later design explicitly requires it.

## 5.4 State D — Ready

```text
Tailscale installed
+ signed in
+ valid address
+ partner reachable / session path usable
        ↓
Home
```

Once setup succeeds, do not show onboarding again unless the prerequisite fails.

## 5.5 Failure handling

Create/Join should distinguish:

- Tailscale missing
- Tailscale signed out
- no usable Tailscale IP
- partner unreachable
- invite target unreachable
- network timeout
- permission/setup failure

Map failures to stable `MP-NET-*` states rather than generic “Create Party failed.”

---

# 6. Local Perfect Media Pipeline

## 6.1 Intended flow

```text
Host local file
        ↓
manifest + identity
        ↓
QUIC chunk transfer
        ↓
Guest sparse cache
        ↓
range/local source
        ↓
libmpv
        ↓
strict synchronized playback
```

## 6.2 Current priorities

- ✅ Native player presentation is code-complete on both OSes (Batch 10) —
  the old "fix first" blocker is retired; only manual playback proof remains.
- 🧪 Real host playback must be tested.
- 🧪 Real guest cache/range playback must be tested.
- 🧪 Pause/Play/Seek must be tested against actual player state.
- 🧪 Buffer starvation must pause both participants.
- 🧪 Disconnect/recovery must be tested.

## 6.3 Media readiness rule

A participant must not become media-ready simply because metadata exists.

Readiness should require the actual player/source to be usable according to the mode’s contract.

## 6.4 Transfer correctness

Retain:

- full media identity based on strong hash + size;
- per-chunk integrity verification;
- guest-generated cache paths;
- no trust in remote absolute paths;
- sparse/resumable cache behavior.

## 6.5 Audit items to re-check

🧪 Code-level validation is complete for binary QUIC chunks, manifest/cache
validation, sparse-cache resume, bounded range serving, real cache-derived
transfer progress, and runtime drift correction. Real two-device playback,
buffering, disconnect/recovery, and stale-worker behavior remain manual
verification items.

---

# 7. Scheduling and Preload

## Status

🧪 The scheduler now invokes Local Perfect preparation with opening-range
priority and background continuation. Real scheduled two-device validation and
OS notification delivery remain pending.

## 7.1 Local Perfect scheduled preload

This remains a core V1 feature.

Example:

```text
Movie scheduled for 9:00 PM
        ↓
estimate file size + goodput + safety margin
        ↓
calculate preload start
        ↓
notify users/devices must be online
        ↓
transfer starts before party time
        ↓
first playback ranges/chunks prioritized
        ↓
guest cache builds
        ↓
party begins with large buffer headroom
```

Requirements:

- Persist schedule.
- Determine preload-start time.
- Detect whether both devices are online.
- Notify offline users to bring device online.
- Prioritize beginning of movie.
- Continue background transfer.
- Show meaningful preload progress.
- Do not claim Ready until required buffer threshold is met.
- Allow post-party retention:
  - Keep
  - Remove
  - Save As

## 7.2 Provider Sync scheduled preparation

Do **not** try to cache the provider’s encrypted network packets in Movie Party.

Instead scheduled preflight can:

- confirm device online;
- start/check managed provider browser;
- check provider login/session health;
- navigate to the selected movie/page;
- let each authorized provider session use its normal buffering behavior;
- perform Ready Check shortly before start.

## 7.3 Provider Shared scheduled buffering

Once Provider Shared is real:

- capture legitimate rendered output;
- encode it;
- produce **Movie Party’s own encoded stream packets**;
- maintain a small rolling/pre-start buffer where technically sensible.

Do not attempt to turn encrypted Netflix/Prime network traffic into a downloadable movie cache.

---

# 8. Streaming Providers

## 8.1 Provider selection UX

Current concept:

```text
Streaming Provider
        ↓
Select provider
        ↓
Provider Sync
or
Provider Shared (Experimental)
```

The user should not normally need to paste a generic provider URL for a provider that has a dedicated adapter.

## 8.2 Provider login UX

Recommended V1 behavior:

```text
Select Netflix / Prime / supported provider
        ↓
[Sign in]
        ↓
open provider inside dedicated managed Chrome profile
        ↓
provider's real login page handles credentials/MFA
        ↓
Movie Party observes session/navigation state
```

Movie Party itself must never present a password field for provider credentials.

## 8.3 Movie selection — V1 recommendation

Do **not** add a universal Movie Party search field yet.

Lower-complexity V1:

```text
Select provider
        ↓
open provider browser
        ↓
user searches/selects movie on provider itself
        ↓
Movie Party detects/attaches to the selected title/page
        ↓
Prepare Party
```

Automated title search inside Movie Party can be reconsidered later if provider-specific DOM maintenance is worth it.

## 8.4 Provider status labels

Do not claim provider support without real tests.

Use truthful statuses such as:

- `SUPPORTED`
- `SYNC_ONLY`
- `EXPERIMENTAL`
- `UNSUPPORTED`
- `EXTERNAL_VERIFICATION_PENDING`

---

# 9. Provider Sync

## Status

🟡 Code-level production path wired; real provider operation is not yet production-proven.

Batch 5 added the Provider Playback Production Path at the code level:

- A `ProviderReadiness` state machine (`NOT_STARTED`, `LAUNCHING`,
  `LOGIN_REQUIRED`, `READY`, `NAVIGATING`, `PLAYBACK_READY`, `UNAVAILABLE`,
  `ERROR`) is serialized into the provider snapshot and driven by the UI.
- The managed browser is opened/reused per provider; provider switch replaces
  the old session so stale state is never reused.
- Login happens only on the provider's own page; Movie Party never collects
  or exposes credentials.
- Title entry navigates the provider's own search page via the existing
  adapter/CDP boundary (YouTube = DIRECT_URL, others = PROVIDER_SEARCH).
- Playback readiness requires a real media-element detection over CDP, not
  Chrome launch alone.
- Room creation / Provider Sync start is gated on valid readiness
  (`validate_provider_ready_for_room`).

## Required behavior

- Both users authenticate separately in their own managed provider session.
- Host remains canonical controller.
- Provider-specific behavior stays inside provider adapters.
- Generic sync engine must not contain provider-specific DOM selectors.
- Movie Party never copies provider credentials/cookies between peers.
- Provider state must return truthful readiness/error information to the frontend.
- Unsupported provider/mode combinations are rejected explicitly.

## Manual verification

🧪 For every supported provider:

- browser launch;
- dedicated profile;
- login persistence;
- movie/page navigation;
- play;
- pause;
- seek;
- readiness;
- reconnect/session expiry;
- macOS;
- Windows.

---

# 10. Provider Shared / Custom Streamer

## Status

🟡 Experimental components exist; full production vertical slice is not yet proven.

## 10.1 Correct packet model

The goal is **not** to forward the provider’s original encrypted HTTP/media packets to the guest.

The valid design is:

```text
Authorized host provider session
        ↓
normal rendered frames/audio
        ↓
legitimate OS capture
        ↓
H.264 encode
        ↓
Movie Party QUIC stream packets
        ↓
Guest jitter/buffer
        ↓
decode
        ↓
Cinema presentation
```

These Movie Party-produced packets can be transmitted and buffered.

## 10.2 Intended macOS stack

- ScreenCaptureKit
- VideoToolbox H.264

## 10.3 Intended Windows stack

- Windows.Graphics.Capture
- WASAPI app loopback
- Media Foundation H.264

## 10.4 Remaining integration

- capture readiness contract;
- actual frame/audio production;
- timestamp synchronization;
- encoder lifecycle;
- dedicated QUIC media stream;
- bitrate/adaptation policy;
- guest buffer/jitter handling;
- decoder;
- guest Cinema presentation;
- backpressure;
- teardown/reconnect;
- protected-surface detection.

## 10.5 Protected content behavior

If legitimate OS capture returns black/static/protected output:

- return `MP-CAPTURE-*`;
- mark Shared unavailable;
- offer explicit Provider Sync fallback;
- never silently switch modes;
- never attempt DRM circumvention.

---

# 11. Third-Party Link Mode

## Status

✅ Conceptually separated from provider sessions.

Requirements:

- Accept only supported generic URL inputs.
- Do not identify generic URLs as Netflix/Prime/etc.
- Do not claim Provider Sync semantics.
- Do not claim Provider Shared semantics.
- Validate input safely.
- Surface unsupported URL/media types clearly.
- 🧪 Real generic-link playback still needs manual proof.

---

# 12. Deep Links / Invite UX

## 12.1 Invite format

The current architecture requires the full descriptor-bearing invite:

```text
movieparty://join/<room_id>#<descriptor>
```

A bare short room code is insufficient without adding a rendezvous/lookup service, which is not part of the current serverless architecture.

## 12.2 macOS

- ✅ code/config plumbing exists
- 🧪 test installed/bundled app:
  - app closed
  - app already running

## 12.3 Windows

- ✅ official Tauri deep-link/single-instance plumbing exists
- 🧪 verify Windows installer actually registers `movieparty://`
- 🧪 verify:
  - cold start
  - running app
  - full invite preserved
  - Join Party prefill
  - successful Lobby transition

---

# 13. Video Call — Correct State Model

## Status

✅ Local/remote state separation closed (Batch 10). 🔴 The deeper 2026-09-05
audit finding (P2): the call itself is a local loopback — real cross-device
offer/answer/ICE plus real remote video rendering is the remaining work
(Batch 12).

## 13.1 Required separation

The app needs clear distinction between:

```text
LOCAL PARTICIPANT
- myCameraEnabled
- myMicEnabled
- myOutgoingVideoTrack
- myOutgoingAudioTrack

REMOTE PARTICIPANT
- remoteCameraEnabled
- remoteMicEnabled
- remoteVideoTrack
- remoteAudioTrack
```

Bottom call controls must operate on **local outgoing devices**, not the peer’s tile.

## 13.2 Camera/mic defaults

Locked:

- Camera OFF by default.
- Mic OFF by default.
- No permission prompt until user explicitly enables a device.
- Privacy Mode exit must not auto-enable devices.

## 13.3 Peer tile

The peer tile should be display-focused.

If remote video ON:

```text
[ remote video ]
```

If remote video OFF:

```text
[ avatar / purple placeholder ]
```

If remote mic muted:

- show a small transparent mute icon on the tile;
- no large bottom “Mic muted” button.

Do not place local camera/mic toggle buttons inside the remote peer tile.

---

# 14. Video Call Tile Interaction Bugs

## Status

✅ Closed at code level (Batch 10 social UX pass). The tile surface
drags with pointer capture, non-control targets only, `user-select:none`
during drag (restored on release), viewport clamping including
shrink/resize, a z-45 overlay layer above Lobby/Ready Check/Cinema
content, close = local hide with Call-button restore (no hang-up), and
minimize = compact draggable video stage with restore. Rules live in
`src/overlays/callTileState.ts` with focused tests. 🧪 Manual drag /
bounds / z-order confirmation on each OS remains pending.

## 14.1 Dragging

Current problem:

- only a narrow part is draggable;
- dragging selects text/content behind the tile.

Required:

- almost the whole video/card surface can initiate drag;
- interactive controls are excluded from drag initiation;
- use pointer capture;
- prevent default text selection while dragging;
- apply `user-select: none` during drag;
- restore normal selection after release.

## 14.2 Z-order

The call tile must not disappear behind:

- Ready to Roll / Dim the Lights card;
- other Lobby content;
- movie content.

Use a dedicated overlay layer with controlled z-index.

## 14.3 Bounds

- Tile cannot be lost outside the window.
- Tile cannot become permanently inaccessible.
- Tile should remain movable away from critical movie content.

## 14.4 Close semantics

**Close** should hide the tile locally, not terminate the call.

The Call button/icon should restore the tile.

A real Hang Up action, if needed, should be explicit and separate.

## 14.5 Minimize semantics

Minimize should create a smaller floating video-only tile.

Minimized state:

- keep video/avatar visible;
- hide title/name;
- hide header;
- hide extra state text;
- hide controls;
- remain draggable;
- remain restorable.

---

# 15. Lobby UX Issues

## Status

✅ Closed at code level (Batch 10 social UX pass). Chat and call remain
floating overlays on request (never a sidebar), the lobby keeps the
purpose → participants → readiness → invite → social controls
hierarchy, and the Ready Check footer now wraps so its button stays
inside the window at narrow desktop widths. 🧪 Visual confirmation on
each OS remains pending.

## Desired hierarchy

1. Movie / lobby purpose
2. Participants / connection state
3. Ready state
4. Invite
5. Social controls
6. Floating chat/call only when requested

Chat and call must not become a permanent sidebar.

## 15.1 Ready Check overflow

✅ Fixed (Batch 10 social UX pass): `.lobby-ready-actions` now wraps
(status row and action buttons stack at narrow widths) on top of the
already-wrapped button row; the global purple button system was not
touched. The Ready Check button stays fully inside the available width
and the preview stays visible. 🧪 Manual check at narrow window sizes
remains pending.

Original report:

- Lobby Ready Check button can extend out of the window.

Fix **this layout only**.

Do not modify the global purple button system again.

Expected:

- Preview remains visible;
- Ready Check remains fully inside available width;
- footer adapts to current desktop window size;
- stack or flex-wrap if required.

---

# 16. Chat UX — Lobby and Cinema

## Status

✅ Closed at code level (Batch 10 social UX pass). The shared overlay
meets the size family (min(420px, viewport-32px) wide × min(500px,
viewport-150px) high), renders translucent with a strong backdrop blur,
floats over the movie/lobby (never a sidebar), and the behavioral rules
are centralized and tested: manual open/close, ~5 s transient preview on
incoming messages, unread badge while hidden, Ghost/Privacy suppression
(no reveal, badge survives), and typing is never destroyed by the
transient timer (the composer exists only in manual mode and the preview
close path preserves a manual session). 🧪 Visual confirmation on each
OS remains pending.

The earlier "smaller than intended" panel note was already resolved by
the shared larger translucent overlay; this pass closed the behavior
gaps around it.

## 16.1 Target behavior

- Floating overlay, never permanent sidebar.
- Approximately roomy 360–420 px width depending on viewport.
- Approximately 420–520 px height depending on viewport.
- Responsive maximum sizes.
- Manual show/hide.
- Auto-hide transient Cinema presentation after ~5 seconds.
- Incoming message can reveal chat.
- Green/unread indicator when message arrives while hidden.
- Active typing should not be unexpectedly destroyed by the transient timer.
- Lobby and Cinema use the same visual family.

> **2026-09-05 audit note (finding P5 / decision D2):** the sizes above
> match the implementation and this document, but UI_UX_SPEC §34–36
> specifies a different family — compose `min(560px, 70vw)` bottom-center;
> history `min(620px, 75vw) × min(560px, 70vh)` centered; lower-third
> ephemeral 5 s bubbles, max 3 visible. The two core docs disagree; one
> must be amended via ADR before the Batch 17 chat work.

---

# 17. Provider Selector Visual Bug

## Status

✅ Closed at code level (Batch 10 social UX pass): the select uses
`color-scheme: dark` with explicit dark colors on the element and its
options (no white native rectangle; selected text visible), focus/hover/
disabled states exist, the label is associated with the control for
keyboard/screen-reader access, and the option list plus visible-value
resolution come from a tested pure model (`providerSelectLabels` /
`resolveProviderSelectValue`) so the visible selection is never blank
while availability loads. The source flow was not redesigned. 🧪 Native
dropdown appearance (popup list hover/selected states) on macOS and
Windows remains a manual check — native `<select>` popup chrome follows
the OS, which is why `color-scheme: dark` is the fix mechanism.

Original report:

- The Provider field rendered as a white blank/native-looking area even
  when a provider was selected.

---

# 18. Ghost Mode

## Status

✅ Implemented (Batch 10 social UX pass, backend semantics since the
state-model batch). Ghost hides the local social layer via the
`social-hidden` class (chat, history, indicators, reactions, tray, call
tile, social controls, diagnostics overlays) in Cinema, Lobby, and the
Ready Check screen; the keyboard shortcuts that could reveal chat
(Enter/"c") are gated; incoming messages set the unread flag instead of
revealing the overlay. Outgoing camera/microphone, playback,
synchronization, and the network session are untouched — verified by
`ghost_mode_keeps_local_call_devices_unchanged`. On exit the prior UI
state is restored: a manually-opened chat returns open, and the call
tile reappears at its own saved position/visibility (its session lives
in AppShell and is only visually suppressed). 🧪 Real camera/mic
behavior with OS permission prompts remains pending.

Purpose: instantly hide Movie Party/social context locally when the user wants the screen to look like normal movie playback.

## Ghost Mode ON

Hide locally:

- chat;
- chat history;
- chat indicators;
- reactions;
- reaction tray;
- call tile;
- participant labels;
- social controls;
- party diagnostics;
- other Movie Party overlays as appropriate.

Keep:

- movie playback;
- synchronization;
- network session;
- outgoing camera exactly as it was;
- outgoing microphone exactly as it was.

**Important updated rule:** Ghost Mode does not stop the camera. The remote participant may still see the user if the camera was already ON.

## Ghost Mode OFF

Restore the prior local UI state:

- call tile visibility;
- call tile position;
- chat state;
- overlay visibility;
- reaction/social controls.

---

# 19. Privacy Mode

✅ Implemented (backend semantics since the state-model batch; UI
restore finalized in the Batch 10 social UX pass). Privacy stays distinct
from Ghost: entry hides the overlays AND disables camera + microphone
(`privacy_mode_disables_real_call_state`), blocks call-mode/device
commands from re-enabling while active
(`privacy_mode_blocks_call_mode_from_reenabling_devices`), and on exit
restores the UI state (same snapshot/restore path as Ghost) while
leaving both devices off until the user re-enables them explicitly
(`leaving_privacy_does_not_reenable_devices_or_stick_ghost_mode`). The
frontend surfaces "Privacy Mode ended. Camera and microphone remain
disabled." 🧪 Real device-off behavior with OS permission prompts
remains pending.

## Privacy Mode ON

- hide overlays;
- stop/disable outgoing camera;
- mute outgoing microphone.

## Privacy Mode OFF

- restore UI visibility as appropriate;
- **do not automatically reactivate camera or microphone**;
- user must explicitly re-enable devices.

---

# 20. Reactions

## Status

✅ Core reaction infrastructure exists.

Remaining work is primarily UI/manual validation:

- reactions should remain lightweight;
- never obscure important subtitles/content for long periods;
- Ghost Mode hides them;
- movie bandwidth takes priority over reaction traffic.

---

# 21. Cinema Entrance / Curtain Sequence

## Status

🎨 **Delegate visual polish to another AI.**

Current code establishes the concept but does not yet produce the desired theatrical feeling.

Final desired sequence:

```text
both participants ready
        ↓
cinema darkens
        ↓
curtains close
        ↓
spotlight at center
        ↓
3
2
1
START
        ↓
curtains visibly open
        ↓
movie is revealed
```

Engineering constraint:

- visual sequence must respect the authoritative synchronization start;
- animation must not visually begin real playback early;
- another AI should focus on CSS/assets/animation rather than changing readiness/network logic.

---

# 22. Hero Reel

## Status

🎨 **Delegate to another AI.**

Current reel is not the desired final visual.

Final concept:

- predominantly front-facing;
- slight perspective/top depth acceptable;
- believable film reel proportions;
- film strip physically emerges from reel;
- individual film frames contain different movie-like imagery;
- subtle cinematic movement;
- slow reel rotation;
- believable film motion;
- lightweight enough for the Home screen;
- avoid unnecessary GPU load.

This should be treated as a visual asset/component task rather than a core product blocker.

---

# 23. Strict Sync / Drift / Recovery

## Locked behavior

If either participant cannot continue:

```text
BOTH PAUSE
```

This includes:

- buffer underrun;
- transfer starvation;
- meaningful player failure;
- significant desynchronization;
- relevant peer disconnect.

Recovery:

```text
condition clears
        ↓
Ready/consensus path
        ↓
Host-authoritative resume
```

## Audit findings requiring current-branch revalidation

Revalidated 2026-09-05 (old claims retired against main @ 64bf79a):

- ✅ drift correction IS runtime-wired: the guest player event loop calls
  `apply_drift_correction` (rate nudge / micro-seek / hard-seek / restore
  1.0×). The old "no runtime caller" claim is stale (also wired in Batch 9B).
- ✅ reconnect/backoff is bounded, single-worker, cache-preserving, with
  no auto-resume (Batch 7 revalidation).
- ✅ heartbeat is fully scheduled (2 s interval, ≈10 s threshold,
  role-aware recovery events — Batch 10).
- ✅ host/guest crash detection is application-level (heartbeat), not
  dependent on QUIC idle timeout.
- ⚠️ Still true: coordinator state depends on large `app_runtime.rs`
  synchronization blocks (maintainability, not correctness).
- 🧪 Real two-device drift measurement (p95 < 100 ms target) still pending.

Do not “fix” stale audit findings without confirming current code.

---

# 24. Disconnect / Reconnect

## Desired behavior

### Guest disconnect

- host pauses;
- room shows reconnecting state;
- host may receive **Continue Without Guest** if this remains an accepted V1 behavior.

### Guest reconnect

- authenticated reconnect;
- media/cache resume;
- canonical state reconciliation;
- Ready Check;
- no independent guest autoplay.

### Host disconnect

Guest must receive a clear recoverable/end state rather than hanging indefinitely.

## Audit item

⚠️ Automatic reconnect/retry/backoff was previously identified as incomplete. Re-check current branch before scheduling implementation.

---

# 25. Bandwidth Policy

Movie remains highest priority.

When constrained, degrade in this order:

1. camera bitrate;
2. camera resolution;
3. camera FPS;
4. camera off if required;
5. preserve voice/sync;
6. preserve movie continuity wherever possible.

The goodput estimator should drive preload/transfer decisions rather than cosmetic quality assumptions.

---

# 26. Notifications

## Intended use

- scheduled party reminders;
- preload needs device online;
- partner/offline setup guidance where appropriate;
- completed/failed preload state where useful.

## Status

🟡 Native notification code exists, but real macOS/Windows delivery must still be manually verified.

Do not consider notifications complete based solely on unit tests.

---

# 27. Chat Persistence

The original product requirement mainly focuses on live shared chat.

An earlier audit claimed SQLite chat APIs existed but runtime chat persistence was incomplete.

## Decision needed

Before implementing persistence, decide whether V1 actually requires:

- chat surviving app restart;
- chat surviving reconnect;
- only current-session chat.

⚠️ Revalidate current code first; do not add persistence solely because an old audit listed it.

---

# 28. Security / Reliability Audit Backlog

The uploaded audit contains many findings from an older code state. Several have already been fixed in later hardening passes. The remaining entries should be treated as a **revalidation backlog**, not blindly as current bugs.

## 28.1 Known later fixes

The following old audit findings are no longer assumed current:

- ✅ poisoned `sync_coordinator.lock().unwrap()` paths were hardened;
- ✅ major Tauri error swallowing was improved;
- ✅ React ErrorBoundary was added;
- ✅ player errors were no longer intentionally swallowed;
- ✅ provider Chrome/Sync wiring is no longer accurately described by the earliest “empty stub” audit;
- ✅ deep links were implemented after the audit;
- ✅ provider/generic-link mode separation was implemented after the audit.

## 28.2 Revalidate before production

⚠️ Revalidated 2026-09-05 — most items below are now ✅ (QUIC retry/backoff, reconnect, heartbeat scheduling, range-server bounds, malformed-UUID rejection, error propagation, snapshot typing, structured logging, telemetry, diagnostics export, bandwidth measurement, native notification wiring, path canonicalization; chat persistence stays dormant by design). Open items: stream/data limits (see P1), CSP, release profile, dependency audits, graceful-shutdown sweep, `app_runtime.rs` monolith. Original checklist retained for reference:

- QUIC transport retry/backoff;
- reconnect behavior;
- heartbeat scheduling;
- stream/data limits;
- range-server connection bounds;
- malformed UUID fallback to nil;
- raw internal error leakage;
- Rust↔TypeScript snapshot contract typing;
- stale frontend event/listener recovery;
- stale Ghost/Privacy keyboard closure;
- CSS override accumulation;
- provider feature flags;
- strict CSP;
- structured logging coverage;
- telemetry wiring;
- diagnostics export wiring;
- bandwidth measurement wiring;
- chat persistence;
- release profile hardening;
- dependency vulnerability scanning (`cargo audit`, JS dependency audit);
- path canonicalization and traversal checks;
- secure Windows credential-helper quoting/escaping;
- TLS verifier/minimum-version assumptions;
- native notification wiring;
- graceful shutdown/JoinHandle cleanup;
- monolithic `app_runtime.rs` maintainability.

## 28.3 Do not over-prioritize irrelevant findings

The following are not automatically V1 blockers:

- lack of Linux runtime support;
- no public marketing/release system;
- no payment/account SaaS layer;
- no app-store work before release hardening.

---

# 29. Repository / External-Drive Hygiene

The external SSD has previously created AppleDouble (`._*`) and `.DS_Store` files and caused Cargo cache/hardlink warnings.

Keep enforcing:

```bash
find . -name "._*" -delete
find . -name ".DS_Store" -delete
```

where safe/appropriate before builds or commits.

Also ensure:

- generated Apple metadata is ignored by Git;
- build caches are not committed;
- mode-bit churn from the external filesystem is not accidentally committed;
- unrelated audit files do not pollute production commits;
- large audit material belongs under a dedicated docs/audits location if retained.

---

# 30. Packaging / Distribution

## Status (2026-08-30 — Windows release-candidate packaging complete)

- ✅ Windows 11 x64 NSIS installer pipeline (`.github/workflows/build.yml`): builds on
  `windows-latest`, bundles the frontend and a self-contained `libmpv` runtime
  (`mpv_runtime/mpv-2.dll`) into the installer, and uploads the NSIS `.exe` as an Actions
  artifact (and attaches it to GitHub Releases for `v*` tags).
- ✅ `movieparty://` protocol registration is wired through `tauri-plugin-deep-link`
  (NSIS registers the scheme at install).
- ✅ Installer does not require Git/Rust/Node/pnpm/repository cloning; Tailscale remains a
  separately installed prerequisite.
- 🧪 Physical Windows install + two-device verification still required (see
  `0_Remaining_Things.md` §31 manual matrix).

## Remaining release-hardening work

### libmpv

- ✅ Windows DLL/runtime loading is bundled and self-contained (`mpv-2.dll` staged in CI).
- 🧪 macOS dylib/runtime loading and installer verification remain manual.
- 🧪 startup detection and user-safe unavailable state remain manual.

### Tailscale

- 🔮 first-run dependency/onboarding;
- 🔮 installer/setup handoff;
- 🔮 sign-in handoff;
- 🔮 partner connectivity setup;
- 🔮 runtime health checks.

### macOS

- ✅ app bundle;
- ⬜ signing;
- ⬜ notarization when release-ready;
- 🧪 protocol-handler verification;
- ⬜ permissions/privacy strings.

### Windows

- ✅ installer (NSIS x64);
- ✅ protocol-handler registration;
- 🧪 native renderer (real device);
- 🧪 camera/mic permissions;
- ✅ required runtime/native libraries (libmpv bundled).

### Security/release

- ⬜ CSP;
- ⬜ release Rust profile;
- ⬜ dependency audit;
- ⬜ privacy/data-handling notice;
- ⬜ versioning/release notes;
- ⬜ reproducible build checks.

---

# 31. Manual Verification Matrix

No cross-platform feature is complete solely because it works on one Mac.

## 31.1 Required combinations

| Host | Guest | Local Perfect | Chat | Call | Deep Link | Provider Sync |
|---|---|---:|---:|---:|---:|---:|
| macOS | macOS | 🧪 | 🧪 | 🧪 | 🧪 | 🧪 |
| macOS | Windows | 🧪 | 🧪 | 🧪 | 🧪 | 🧪 |
| Windows | macOS | 🧪 | 🧪 | 🧪 | 🧪 | 🧪 |
| Windows | Windows | 🧪 | 🧪 | 🧪 | 🧪 | 🧪 |

## 31.2 Local playback test

Verify:

- actual image;
- audio;
- pause;
- play;
- seek;
- duration;
- progress;
- no premature autoplay;
- end-of-media;
- window resize;
- overlay composition.

## 31.3 Two-device strict-sync test

Verify:

- invite;
- join;
- media preparation;
- ready consensus;
- synchronized start;
- pause;
- seek;
- buffer-low host pause;
- recovery;
- disconnect;
- reconnect;
- long-duration drift.

## 31.4 Network test

Test:

- good connection;
- restrictive college network;
- Tailscale path;
- temporary Wi-Fi outage;
- peer sleep/wake;
- packet loss/jitter;
- low throughput.

## 31.5 Provider test

Per provider:

- login;
- session persistence;
- title selection;
- sync operations;
- provider page changes;
- account/session expiry;
- unsupported/protected behavior.

## 31.6 Call test

Verify:

- camera/mic permission only on explicit enable;
- camera OFF default;
- mic OFF default;
- local controls only change local outgoing media;
- remote mute/camera indicators;
- cross-device video/audio;
- disconnect/reconnect;
- Ghost Mode behavior;
- Privacy Mode behavior.

---

# 32. Recommended Implementation Order

Superseded 2026-09-05: the previous 68-item phase list described the
pre-Batch-10 state (its playback, social-UI, Ghost/Privacy, and drift
items are done at the code level). The current order is the batch plan
from the full-project audit; full detail, estimates, and decision points
D1–D4 live in `docs/core_docs/V1_COMPLETION_PLAN.md`.

1. **Batch 11 — Protocol truth & ADR foundation** (P1/P11): decide D1
   (CBOR vs ADR-amended JSON), fix message-ID collisions, envelope
   version/room fields, size-limit enforcement, malformed-input tests.
2. **Batch 12 — Real cross-device call** (P2): true offer/answer/ICE over
   the existing QUIC relay; real remote video/audio rendering in the tile.
3. **Batch 13 — Adaptive camera ladder** (P17): 480p20 → 240p10 → frozen,
   once-per-event degradation notice, movie-first priority policy.
4. **Batch 14 — Provider Sync runtime completion** (P3): canonical commits
   dispatch to provider adapters; provider position/buffer feed the
   coordinator; YouTube first.
5. **Batch 15 — Missing screens I** (P6): Settings, First Run,
   Error screen, Debug HUD, window-close-during-party prompt.
6. **Batch 16 — Scheduling frontend + protocol** (P8/P14): Schedule form,
   Home Upcoming, offline warning, SCHEDULE_*/PRELOAD_STATE messages,
   fix the preload units duplication.
7. **Batch 17 — Chat & cinema spec alignment** (P4/P5/P7): lower-third
   ephemeral bubbles, backend-driven countdown, camera card spec values.
8. **Batch 18 — Resilience watchers + recovery UX** (P13): Chrome-crash
   and player-failure watchers, disconnect overlay with host-only
   Continue Without Guest.
9. **Batch 19 — macOS Provider Shared capture spike** (P10):
   diagnostic-only capture/encode/30 s sample with honest DRM
   classification (black-frame detection exists; no circumvention).
10. **Batch 20 — Provider Shared transport** (conditional on Batch 19 +
    decision D3): QUIC media stream, 5 s presentation buffer, host
    loopback, strict sync, automatic quality ladder.
11. **Batch 21 — Windows Provider Shared spike** (needs a physical
    Windows machine).
12. **Batch 22 — Release hardening** (P12): CSP, release profile,
    dependency audits, macOS libmpv bundling verification.
13. **Batch 23 — Regression + beta prep**: chaos tests where feasible,
    the full §31 manual matrix, beta build cut.

Out-of-V1 items are enumerated in MASTER_PRD §111/§112 and must not be
built (music, mobile, Linux, >2 users, cloud accounts, manual quality
selector, 1080p webcam, auto-updater, public marketing).

# 33. Definition of V1 Complete

Movie Party V1 is complete only when the following real flow works:

```text
Fresh install
        ↓
Tailscale onboarding
        ↓
partner connectivity established
        ↓
Home
        ↓
Host chooses media/provider
        ↓
Host creates room
        ↓
Guest clicks invite
        ↓
Lobby
        ↓
optional chat/call
        ↓
scheduled preload/preparation where applicable
        ↓
Ready Check
        ↓
cinematic transition
        ↓
actual movie renders
        ↓
strict synchronized playback
        ↓
buffer/disconnect recovery
        ↓
Ghost Mode / Privacy Mode work correctly
        ↓
party ends
        ↓
retention decision
```

For Provider Sync, supported providers must additionally pass their own authentication/navigation/playback tests.

Provider Shared may remain explicitly **Experimental** in V1 if it has not passed the full capture/encode/transport/presentation verification matrix. It must never be presented as production-ready merely because components exist.

---

# 34. Agent / Coding Workflow Rules

To protect credits and avoid regressions:

1. Do not tell coding agents to audit the whole repository every run.
2. Give them exact subsystem scope and relevant files.
3. Do not ask agents to launch the app for manual UI/playback verification unless absolutely necessary.
4. The user performs real visual/device/network/provider verification.
5. Coding agents should:
   - inspect targeted code;
   - implement;
   - run focused automated tests;
   - report manual verification required.
6. Avoid screenshots/browser automation unless specifically needed.
7. Do not redesign approved Emergent UI while fixing backend issues.
8. Do not “fix” stale audit findings before checking whether later commits already resolved them.
9. Prefer small commits aligned with architectural units.
10. Keep this document updated whenever a new blocker, decision, or completed milestone changes the roadmap.

---

# 35. Document Maintenance Rule

Whenever a new issue is discovered:

1. classify it as confirmed, partial, manual verification, future, or delegated visual;
2. place it in the appropriate subsystem section;
3. add acceptance criteria;
4. update implementation priority if necessary;
5. do not delete historical decisions silently;
6. move resolved items into the implemented/checkpoint section or mark them completed;
7. revalidate old audit findings against the current branch before treating them as current bugs.

This file should remain the **master remaining-work roadmap**, while `IMPLEMENTATION_TRACKER.md` remains the coding-session/status log.
