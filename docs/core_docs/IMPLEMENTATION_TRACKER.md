
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
PHASE 32

Current Release:
V1 Development

Architecture:
LOCKED

Critical Blockers:
Independent local implementation has reached the external certification/regression gates. External Phase 0/1 verification and Phase 28-31 manual/device/network verification remain pending.
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
🟦 HELLO validates UUIDv7 device IDs
🟦 HELLO rejects unsupported V1 platforms
🟦 Implement authentication
🟦 Ed25519 identity public key in HELLO
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
🟦 no listening on public interfaces in local listener API; real host binding pending Tailscale environment
```

Notes:

```
Hardened locally on 2026-08-15:
- Replaced Phase 1 placeholder public-key/signature values with Ed25519 device identity support.
- HELLO now carries the local identity public key.
- HELLO validates that device IDs are UUIDv7 strings before room authentication succeeds.
- HELLO rejects unsupported V1 platform values before room authentication succeeds.
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
- Added local diagnostic JSON export helper with stable Move Party metadata, beta events, redacted logs, and destination validation.
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
