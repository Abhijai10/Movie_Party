
# DOCUMENT 5 — `IMPLEMENTATION_TRACKER.md`


# Move Party — Implementation Tracker

This document records implementation status.

It must be updated continuously.

---

# CURRENT STATUS

```text
Project State:
🟨 IN PROGRESS

Current Phase:
PHASE 6

Current Release:
V1 Development

Architecture:
LOCKED

Critical Blockers:
None for independent local implementation. External Phase 0/1 verification remains pending.
````

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
🟦 Bind host to Tailscale address
🟦 Implement HELLO
🟦 Implement authentication
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
🟦 no listening on public interfaces in local listener API; real host binding pending Tailscale environment
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
⬜
```

```
⬜ Player adapter connected
⬜ Scheduled play
⬜ Scheduled pause
⬜ Scheduled seek
⬜ Buffer reports
⬜ BUFFER_LOW flow
⬜ Global pause
⬜ Ready recovery
⬜ Global resume
⬜ Disconnect pause
```

Critical test:

```
⬜ Guest buffer starvation pauses Host
```

Phase may not pass without this.

---

# PHASE 8 — NETWORK-AWARE PRELOAD

Status:

```
⬜
```

```
⬜ Goodput estimator
⬜ Movie bitrate estimator
⬜ Buffer recommendation
⬜ Auto mode
⬜ Smart preload
⬜ Download-first recommendation
```

Test:

```
⬜ 2 Mbps
⬜ 5 Mbps
⬜ 10 Mbps
⬜ 20 Mbps
```

---

# PHASE 9 — SCHEDULING

Status:

```
⬜
```

```
⬜ SQLite schedule table
⬜ Create schedule
⬜ Send schedule
⬜ Guest accept
⬜ Local notification registration
⬜ Preload calculation
⬜ Background transfer
⬜ Offline peer state
⬜ Resume when peer returns
```

---

# PHASE 10 — RETENTION

Status:

```
⬜
```

```
⬜ Keep
⬜ Remove
⬜ Save As
⬜ Cache cleanup
⬜ Settings policy
```

---

# PHASE 11 — CINEMA UI

Status:

```
⬜
```

```
⬜ Home
⬜ Create Party
⬜ Join
⬜ Lobby
⬜ Ready Check
⬜ Cinema
⬜ Control Dock
⬜ Buffer UI
⬜ Reconnect UI
⬜ End Party UI
```

---

# PHASE 12 — CHAT & REACTIONS

Status:

```
⬜
```

```
⬜ CHAT_MESSAGE
⬜ Compose
⬜ Floating messages
⬜ History
⬜ Reactions
⬜ Rate limiting
```

---

# PHASE 13 — GHOST / PRIVACY

Status:

```
⬜
```

```
⬜ Ghost Mode
⬜ Privacy Mode
⬜ Global shortcut Windows
⬜ Global shortcut macOS
⬜ Camera stays unchanged under Ghost
⬜ Camera disabled under Privacy
⬜ Mic disabled under Privacy
```

---

# PHASE 14 — VIDEO CALL SPIKE

Status:

```
⬜
```

```
⬜ Camera enumeration
⬜ Mic enumeration
⬜ WebRTC signalling
⬜ Peer video
⬜ Peer voice
⬜ Mic default muted
⬜ Voice-only
⬜ Call-off
```

Cross-platform:

```
⬜ Win → Win
⬜ Win → Mac
⬜ Mac → Win
⬜ Mac → Mac
```

If routing fails:

```
⬜ ADR created
```

---

# PHASE 15 — ADAPTIVE CAMERA

Status:

```
⬜
```

```
⬜ Camera tier A
⬜ Tier B
⬜ Tier C
⬜ Tier D
⬜ Goodput feedback
⬜ Buffer feedback
⬜ Camera downgrade
⬜ Camera recovery
```

Critical:

```
⬜ Movie stays smooth when camera is degraded
```

---

# PHASE 16 — MANAGED CHROME

Status:

```
⬜
```

```
⬜ Locate Chrome Windows
⬜ Locate Chrome macOS
⬜ Dedicated profile
⬜ Local CDP
⬜ Navigate URL
⬜ Reuse provider session
⬜ Close/restart safely
```

Security:

```
⬜ CDP bound locally
⬜ No provider cookies logged
```

---

# PHASE 17 — GENERIC PROVIDER

Status:

```
⬜
```

```
⬜ Media detection
⬜ Play
⬜ Pause
⬜ Seek
⬜ Position
⬜ Buffer detection
```

---

# PHASE 18 — YOUTUBE

Status:

```
⬜
```

```
⬜ URL parsing
⬜ Content ID
⬜ Player detection
⬜ Sync
⬜ Buffer state
```

---

# PHASE 19 — PROVIDER SYNC

Status:

```
⬜
```

Netflix:

```
⬜ Windows
⬜ macOS
```

Prime:

```
⬜ Windows
⬜ macOS
```

JioHotstar:

```
⬜ Windows
⬜ macOS
```

Required per provider:

```
⬜ Launch
⬜ Login
⬜ Open URL
⬜ Detect media
⬜ Play
⬜ Pause
⬜ Seek
⬜ Position
⬜ Buffer detect
⬜ Strict global pause
⬜ Resume
```

---

# PHASE 20 — WINDOWS SHARED CAPTURE SPIKE

Status:

```
⬜
```

Test:

```
⬜ YouTube video
⬜ YouTube audio
⬜ Netflix video
⬜ Netflix audio
⬜ Prime video
⬜ Prime audio
⬜ JioHotstar video
⬜ JioHotstar audio
```

Record protected-capture behavior.

---

# PHASE 21 — MACOS SHARED CAPTURE SPIKE

Status:

```
⬜
```

Same matrix.

---

# PHASE 22 — HARDWARE ENCODING

Status:

```
⬜
```

Windows:

```
⬜ H264 Media Foundation
```

macOS:

```
⬜ H264 VideoToolbox
```

Benchmark:

```
⬜ 720p30
⬜ 1080p30
⬜ encode latency
⬜ CPU
⬜ GPU
```

---

# PHASE 23 — SHARED STREAM TRANSPORT

Status:

```
⬜
```

```
⬜ Encoded media stream
⬜ Guest receive buffer
⬜ Decoder
⬜ Presentation timestamps
⬜ Audio
⬜ A/V sync
```

---

# PHASE 24 — HOST LOOPBACK

Status:

```
⬜
```

```
⬜ Host watches encoded output
⬜ Guest watches same output
⬜ Chrome source hidden from experience
```

---

# PHASE 25 — SHARED STRICT SYNC

Status:

```
⬜
```

Critical test:

```
⬜ Throttle Guest
⬜ Guest buffer falls
⬜ Source pauses
⬜ Host presentation pauses
⬜ Guest rebuilds
⬜ Both resume together
```

---

# PHASE 26 — AUTOMATIC QUALITY

Status:

```
⬜
```

```
⬜ 1080p high
⬜ 1080p medium
⬜ 720p high
⬜ 720p low
⬜ camera reduction before movie reduction
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
⬜
```

```
⬜ Chrome crash
⬜ Guest crash
⬜ Host crash
⬜ Tailscale disconnect
⬜ WiFi disconnect
⬜ Sleep/wake
⬜ Provider logout
⬜ Cache corruption
⬜ Missing local file
```

---

# PHASE 29 — COLLEGE NETWORK CERTIFICATION

Status:

```
⬜
```

Environment:

```
College WiFi
~10 Mbps expected
Tailscale active
```

Local:

```
⬜ 2GB
⬜ 4GB
⬜ 8GB
```

Call:

```
⬜ Off
⬜ Voice
⬜ Video
```

Provider Shared:

```
⬜ All available provider combinations
```

---

# PHASE 30 — FULL CROSS-PLATFORM REGRESSION

Status:

```
⬜
```

```
⬜ Win → Win
⬜ Win → Mac
⬜ Mac → Win
⬜ Mac → Mac
```

---

# PHASE 31 — PERSONAL BETA

Status:

```
⬜
```

```
⬜ Create development installer
⬜ Install on trusted friend device
⬜ Collect bug reports
⬜ Export diagnostic bundles
⬜ Run real movie nights
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
Move Party/
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
