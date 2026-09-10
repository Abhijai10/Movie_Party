# Movie Party

## Master Product Requirements Document, System Architecture, Engineering Specification & Implementation Roadmap

**Document status:** Architecture Locked for V1
**Target:** Personal-use development build
**Primary platforms:** Windows 10/11 + macOS 11+
**Initial party size:** Exactly 2 participants
**Primary network environment:** Restrictive college network, approximately 10 Mbps Internet connectivity
**Infrastructure constraint:** ₹0 recurring Movie Party infrastructure cost; no rented VPS/cloud media server/TURN server
**Primary connectivity layer:** Tailscale
**Document date:** August 15, 2026

---

# 0. PURPOSE OF THIS DOCUMENT

This document is the single source of truth for Movie Party.

A human developer or future contributor must **not make architectural decisions independently** when an answer already exists here.

If implementation reveals that a locked assumption is technically impossible, the developer must:

1. stop work on the affected subsystem;

2. create an Architecture Decision Record describing the blocker;

3. provide reproducible evidence;

4. propose alternatives;

5. obtain approval before replacing the architecture.


Do not silently redesign Movie Party.

---

# 1. PRODUCT VISION

Movie Party is a private cross-platform desktop cinema application that allows two people to watch media together while remaining tightly synchronized.

The desired experience is:

```
Host opens Movie Party
        ↓
chooses local movie
OR
pastes Netflix / Prime / JioHotstar / YouTube URL
        ↓
creates party
        ↓
guest joins
        ↓
Movie Party prepares media
        ↓
network + buffer + call readiness check
        ↓
3
2
1
PLAY
        ↓

Both people experience:

FULLSCREEN MOVIE
+
optional floating video call
+
optional microphone
+
temporary chat messages
+
reactions
+
strict synchronization
```

The movie must remain the primary experience.

Movie Party is **not** intended to resemble:

- Discord;

- Google Meet;

- a remote-desktop application;

- a dashboard with a small movie player;

- a conventional side-panel watch-party extension.


The movie should visually occupy essentially the entire display.

---

# 2. LOCKED PRODUCT PRINCIPLES

These requirements are non-negotiable unless explicitly changed later.

## 2.1 Desktop application only

V1 shall consist of a native desktop application.

There shall be:

- no browser extension;

- no required Movie Party website;

- no hosted Movie Party web application.


Supported operating systems:

- Windows 10/11;

- macOS 11 or later.


Tauri 2 is selected for the application shell because it supports cross-platform desktop applications with a Rust backend and web-based frontend architecture. citeturn932290search23turn777396search17

---

## 2.2 Two-person V1

V1 is designed for:

```
1 Host
+
1 Guest
```

Do not prematurely optimize the core networking system for large rooms.

Protocol messages must nevertheless include participant IDs so that V2 can later support more than two users without replacing the protocol entirely.

---

## 2.3 Zero rented infrastructure

Movie Party itself shall require no:

- AWS instance;

- VPS;

- paid TURN server;

- hosted movie relay;

- hosted signalling backend;

- hosted database.


The host's computer acts as the temporary room coordinator.

Tailscale is an allowed external dependency.

As of August 2026, Tailscale's Personal plan allows up to six free users, which is sufficient for this personal-use project and the planned future small-party expansion. citeturn482310search0turn482310search1

---

# 3. NETWORKING PHILOSOPHY

Movie Party assumes both participants are members of the same Tailscale tailnet.

Tailscale currently supports:

```
Direct connection
        ↓ if unavailable

Peer Relay
        ↓ if unavailable

DERP relay
```

Direct connections provide the best throughput and latency; DERP is a fallback when direct NAT traversal fails. citeturn482310search2turn482310search8

This is especially important because the expected college network is restrictive.

Movie Party must always determine the current Tailscale path before starting high-bandwidth media.

Possible connection states:

```
DIRECT
PEER_RELAY
DERP_RELAY
UNKNOWN
OFFLINE
```

Tailscale exposes the connection type through its status tooling, including `direct`, `relay`, and `peer-relay`. citeturn482310search17

---

# 4. MEDIA MODES

Movie Party has exactly three primary media modes.

They must remain separate internally even though they share the same UI.

---

# MODE A — LOCAL PERFECT MODE

Used when the host selects a normal media file.

Examples:

```
movie.mkv
movie.mp4
movie.webm
```

The host owns or is otherwise authorized to share the media.

Architecture:

```
HOST FILE
   │
   │ original encoded bytes
   ▼
Chunk Transfer Engine
   │
   ▼
Tailscale
   │
   ▼
Guest Cache
   │
   ▼
Movie Party Player
```

There shall be:

- no screen capture;

- no movie transcoding;

- no re-encoding;

- no central media server.


The guest receives the original encoded file.

This mode should provide the highest possible quality and reliability.

---

# MODE B — PROVIDER SHARED MODE

**Experimental V1 feature.**

Target providers:

1. Netflix

2. Amazon Prime Video

3. JioHotstar

4. later: other providers


Only the host needs the provider playback session in this mode.

Architecture:

```
Provider
   │
   ▼
Managed Chrome
   │
DRM-authorized playback
   │
   ▼
OS-supported window capture
   │
   ▼
hardware video encoder
   │
   ▼
Movie Party media stream
   │
   ├────────► Host Movie Party Player
   │
   └────────► Guest Movie Party Player
```

Movie Party must **not** attempt to:

- extract Widevine keys;

- export DRM licenses;

- decrypt provider packets itself;

- bypass protected-content restrictions;

- intercept and redistribute decrypted provider media buffers.


Encrypted Media Extensions explicitly use a Content Decryption Module and associated key/session architecture for protected media. citeturn777396search6

Provider Shared Mode shall therefore operate only through legitimate OS capture surfaces.

If protected video capture is blocked:

```
PROVIDER_SHARED_UNAVAILABLE
```

must be returned.

No DRM bypass shall be attempted.

---

# MODE C — PROVIDER SYNC MODE

Reliable fallback.

Each participant has authorized provider playback.

Movie Party controls and synchronizes each provider player.

Architecture:

```
Netflix CDN
     │
     ├──────── Host Chrome
     │
     └──────── Guest Chrome

Movie Party
     │
     └──────── synchronizes both
```

This requires provider access on both machines but preserves native provider delivery and should remain the compatibility fallback.

---

# 5. SELF-HOSTED VBROWSER FALLBACK

If Provider Shared Mode proves unreliable, a self-hosted vBrowser experiment may be attempted.

It **must not require rented infrastructure**.

Possible deployment:

```
Host computer
      │
      ▼
isolated browser environment
      │
Chrome
      │
virtual desktop / isolated display
      │
capture
      │
encode
      │
Tailscale
      │
guest
```

Possible future alternative:

```
user-owned spare PC
        ↓
self-hosted vBrowser node
```

A cloud VM rented specifically for Movie Party is outside project requirements.

Important limitation:

A vBrowser does **not automatically solve DRM capture restrictions**.

If the OS/provider prevents capture of protected video, virtualizing the browser may still fail.

Therefore this is an R&D fallback, not a guaranteed DRM solution.

---

# 6. CUSTOM Movie Party PLAYER

Movie Party shall contain its own media presentation layer.

The Movie Party player is responsible for:

### Local Perfect Mode

Full playback of transferred/local files.

### Provider Shared Mode

Decoding the host-generated encoded stream.

### Provider Sync Mode

The actual DRM media remains in Chrome.

Movie Party may use an overlay/presentation window to make the provider playback visually feel integrated.

The custom player itself shall **not become a Netflix/Prime/JioHotstar DRM client**.

---

# 7. LOCAL PLAYER TECHNOLOGY

Use:

```
libmpv
```

as the initial local playback backend.

mpv explicitly recommends `libmpv` when mpv is being used as the playback backend for another application, and supports a wide variety of media formats, codecs and subtitle types. citeturn932290search0turn932290search4

Supported V1 media targets:

```
MP4
MKV
WebM

H.264
H.265/HEVC where system decoding permits
VP9
AV1 where system decoding permits

AAC
MP3
Opus
AC3/EAC3 where available

SRT subtitles
ASS subtitles
embedded subtitles

multiple audio streams
multiple subtitle streams
```

Do not write a custom codec stack.

---

# 8. APPLICATION TECHNOLOGY STACK

## Frontend

```
React
TypeScript
Tauri 2 frontend
```

## Native core

```
Rust
Tokio
```

## Desktop shell

```
Tauri 2
```

## Network transport

```
Tailscale
+
QUIC
```

Preferred Rust QUIC implementation:

```
Quinn
```

Quinn is a Rust QUIC implementation tested on Windows and macOS. citeturn932290search5

## Local database

```
SQLite
```

## Movie playback

```
libmpv
```

## Provider browser

```
installed Google Chrome
```

## Provider control

```
Chrome DevTools Protocol
```

Chrome's remote debugging security behavior changed starting with Chrome 136: remote debugging must use a non-default `--user-data-dir`. That matches Movie Party's dedicated provider-profile architecture. citeturn777396search13

## Windows capture

```
Windows.Graphics.Capture
```

Microsoft exposes Windows Graphics Capture for application-window/display capture. citeturn875678search0

## Windows provider audio

```
WASAPI application loopback
```

Microsoft provides process-specific application loopback capture using WASAPI. citeturn840012search0

## macOS capture

```
ScreenCaptureKit
```

ScreenCaptureKit supports high-performance screen and application capture including associated audio. citeturn840012search5turn840012search33

## macOS encoding

```
VideoToolbox
```

VideoToolbox provides access to hardware encoders/decoders and supports low-latency H.264 encoding. citeturn840012search3turn840012search7

## Windows encoding

Initial baseline:

```
Media Foundation H.264
```

Microsoft provides an H.264 Media Foundation encoder implementation. citeturn840012search2

---

# 9. HIGH-LEVEL PROCESS ARCHITECTURE

```
MovieParty.exe / MovieParty.app

│
├── UI PROCESS
│
│   React
│   Cinema UI
│   Lobby
│   Settings
│   Chat
│
├── RUST CORE
│
│   ├── Room Coordinator
│   ├── Network Manager
│   ├── Sync Engine
│   ├── Transfer Engine
│   ├── Cache Manager
│   ├── Scheduler
│   ├── Provider Manager
│   └── Diagnostics
│
├── LIBMPV
│
│   Local playback
│
├── PROVIDER CHROME
│
│   Netflix / Prime / JioHotstar
│
├── CAPTURE PIPELINE
│
│   Windows Graphics Capture
│   OR
│   ScreenCaptureKit
│
├── ENCODER
│
│   H264
│
└── TAILSCALE CONNECTION
```

---

# 10. REPOSITORY STRUCTURE

Required initial structure:

```
movie-party/

├── README.md
├── LICENSE
├── package.json
├── pnpm-lock.yaml
├── tsconfig.json
├── vite.config.ts
├── .github/
│   └── workflows/
│
├── docs/
│   ├── architecture/
│   │   ├── adr/
│   │   ├── protocols.md
│   │   ├── sync.md
│   │   ├── media.md
│   │   └── providers.md
│   │
│   ├── testing/
│   └── compatibility/
│
├── src/
│   ├── app/
│   ├── components/
│   ├── cinema/
│   ├── lobby/
│   ├── party/
│   ├── chat/
│   ├── call/
│   ├── schedule/
│   ├── settings/
│   ├── diagnostics/
│   └── types/
│
└── src-tauri/
    ├── Cargo.toml
    └── src/
        │
        ├── main.rs
        │
        ├── identity/
        │
        ├── room/
        │
        ├── network/
        │   ├── tailscale.rs
        │   ├── quic.rs
        │   ├── bandwidth.rs
        │   └── diagnostics.rs
        │
        ├── sync/
        │   ├── clock.rs
        │   ├── drift.rs
        │   ├── consensus.rs
        │   └── state_machine.rs
        │
        ├── media/
        │   ├── player/
        │   ├── manifest/
        │   ├── cache/
        │   ├── transfer/
        │   └── stream/
        │
        ├── providers/
        │   ├── chrome/
        │   ├── netflix/
        │   ├── prime/
        │   ├── hotstar/
        │   └── youtube/
        │
        ├── capture/
        │   ├── windows/
        │   └── macos/
        │
        ├── encode/
        │   ├── windows/
        │   └── macos/
        │
        ├── call/
        ├── scheduling/
        ├── notifications/
        ├── storage/
        └── telemetry/
```

Provider code shall never be mixed into the generic sync engine.

---

# 11. INSTALLATION IDENTITY

Every Movie Party installation generates:

```
DeviceID
Persistent identity keypair
DisplayName
Platform
AppVersion
ProtocolVersion
```

Recommended:

```
Ed25519 identity
```

Store private identity using the operating system's secure credential storage where practical.

Do not transmit private identity keys.

---

# 12. ROOM SECURITY

Every room shall generate:

```
RoomID          128-bit random
JoinSecret      256-bit random
HostDeviceID
CreatedAt
ExpiresAt
```

Invitation descriptor:

```
MoviePartyInvite {
    protocolVersion,
    roomId,
    hostDeviceId,
    hostTailscaleIp,
    hostPort,
    serverCertificateFingerprint,
    joinSecret,
    expiresAt
}
```

Encode invitation using:

```
CBOR
→ Base64URL
```

Represent as:

```
movieparty://join/<encoded-descriptor>
```

or a copyable text code.

No central URL-shortening server.

---

# 13. QUIC CONNECTION

Default Movie Party transport:

```
QUIC over Tailscale
```

Default port:

```
UDP 47821
```

Allow configuration if necessary.

The host listens only on:

```
Tailscale interface
```

not:

```
0.0.0.0
```

unless explicitly required during debugging.

QUIC connection certificate must be pinned using the fingerprint contained in the invitation.

The JoinSecret then authenticates room membership.

---

# 14. PROTOCOL ENVELOPE

All control messages must use a versioned envelope.

```
MessageEnvelope {
    protocol_version
    room_id
    sequence_number
    sender_device_id
    sent_at_monotonic_us
    message_type
    payload
}
```

Use CBOR for production messages.

JSON may be allowed only for debug tooling.

---

# 15. CORE MESSAGE TYPES

At minimum:

```
HELLO
AUTH_REQUEST
AUTH_ACCEPT
AUTH_REJECT

JOIN_REQUEST
JOIN_ACCEPT
JOIN_REJECT
LEAVE_ROOM

HEARTBEAT

CLOCK_PING
CLOCK_PONG

ROOM_STATE

MEDIA_ANNOUNCE
MEDIA_READY
MEDIA_NOT_READY

PLAY_PREPARE
PLAY_COMMIT

PAUSE_PREPARE
PAUSE_COMMIT

SEEK_PREPARE
SEEK_READY
SEEK_COMMIT

BUFFER_STATUS
BUFFER_LOW
BUFFER_RECOVERED

PLAYER_STATE

TRANSFER_MANIFEST
CHUNK_REQUEST
CHUNK_DATA
CHUNK_ACK

NETWORK_STATS

CALL_STATE
CAMERA_STATE
MIC_STATE

CHAT_MESSAGE
REACTION

SHARED_CONTROL_REQUEST
SHARED_CONTROL_GRANT
SHARED_CONTROL_REVOKE

SCHEDULE_CREATE
SCHEDULE_UPDATE
SCHEDULE_CANCEL

PROVIDER_STATE

DISCONNECT_NOTICE
RECONNECT_REQUEST
RECONNECT_COMPLETE
```

Unknown message types must be ignored safely when protocol compatibility allows.

---

# 16. ROOM STATE MACHINE

Primary states:

```
CREATED
WAITING_FOR_GUEST
LOBBY
PREPARING
READY_CHECK
PLAYING
PAUSING
PAUSED
SEEKING
BUFFERING
RECONNECTING
ENDED
ERROR
```

Invalid state transitions must be rejected.

Example:

```
WAITING_FOR_GUEST
        ↓
LOBBY
        ↓
PREPARING
        ↓
READY_CHECK
        ↓
PLAYING
```

Buffering:

```
PLAYING
   ↓
BUFFERING
   ↓
READY_CHECK
   ↓
PLAYING
```

---

# 17. STRICT SYNCHRONIZATION

Strict Sync is enabled by default.

User may not accidentally disable it.

If later a relaxed mode is added, it must be explicitly selected.

---

# 18. AUTHORITATIVE CLOCK

Host owns the canonical room clock.

Use a monotonic system clock.

Never use wall-clock timestamps for playback synchronization.

Guest estimates:

```
offset_to_host
RTT
jitter
```

using repeated clock probes.

Clock calibration procedure:

1. send 20 `CLOCK_PING` messages;

2. guest records send/receive monotonic timestamps;

3. discard highest-latency outliers;

4. calculate median offset from best samples;

5. periodically refresh while party is active.


Clock recalibration interval:

```
30 seconds
```

or sooner after reconnect/network path change.

---

# 19. SCHEDULED COMMANDS

Never synchronize by:

```
host presses play
→ guest immediately executes when packet arrives
```

Instead:

```
PLAY_COMMIT {
    targetPosition,
    executeAtHostTime
}
```

Execution lead time:

```
max(
    750 ms,
    2 × p95 RTT + 250 ms
)
```

Clamp V1 default maximum:

```
3000 ms
```

unless network diagnostics explicitly require a longer value.

---

# 20. DRIFT TARGET

Desired normal playback drift:

```
≤100 ms
```

Correction policy:

```
0–80 ms
    ignore

80–250 ms
    temporary playback-rate correction

250–700 ms
    micro-seek or stronger correction

>700 ms
    synchronized hard seek
```

Soft playback rate:

```
minimum 0.97x
maximum 1.03x
```

Never leave a corrected playback rate active indefinitely.

Restore:

```
1.0x
```

once synchronized.

---

# 21. BUFFER CONSENSUS

Every client periodically reports:

```
BufferStatus {
    playbackPosition
    bufferedAheadSeconds
    playerReady
    transportGoodput
    localCacheAhead
    stalled
}
```

Report interval while playing:

```
500 ms
```

The room may play only when all active participants are ready.

For V1:

```
AllParticipantsReady = HostReady AND GuestReady
```

---

# 22. STRICT BUFFER FAILURE

If either participant becomes unable to continue:

```
BUFFER_LOW
```

the room enters:

```
PAUSING
```

and both participants pause.

The host shall not continue watching while the guest buffers.

---

# 23. BUFFERING UI

Show:

```
Playback paused to keep everyone synchronized

Abhijai
Ready ✓

Rahul
Buffering...
███████░░░░░
```

Video call and chat remain operational.

Once everyone reaches the resume threshold:

```
Ready ✓
Ready ✓

Resuming in

3
2
1
```

---

# 24. LOCAL MEDIA TRANSFER FORMAT

When the host selects a file, create:

```
MediaManifest
```

Fields:

```
media_id
filename
filesize
duration
container
video_codec
audio_tracks[]
subtitle_tracks[]
average_bitrate
quick_fingerprint
full_hash
chunk_size
chunk_count
```

Recommended chunk size:

```
1 MiB
```

Reason:

good random-seek granularity while keeping metadata manageable.

---

# 25. FILE HASHING

Use two levels.

## Quick fingerprint

Used for immediate probable matching.

Include:

```
file size
first 4 MiB hash
last 4 MiB hash
```

## Full hash

Use:

```
BLAKE3
```

Compute asynchronously.

Two files are considered definitely identical only after:

```
size identical
AND
full hash identical
```

---

# 26. LOCAL CACHE

Guest cache structure:

```
MoviePartyCache/
  <media-id>/
      metadata.cbor
      data.part
      chunk-map.bin
```

Cache must support sparse/random ranges.

Do not assume the file downloads sequentially.

---

# 27. LOCAL PLAYBACK CACHE SOURCE

The player shall not directly open an incomplete normal filesystem file.

Instead create a loopback-only media source:

```
127.0.0.1:<random-port>/media/<media-id>
```

supporting HTTP byte-range semantics.

libmpv opens this local media endpoint.

When mpv requests:

```
Range: bytes=A-B
```

the cache layer:

```
if available:
    return range

if unavailable:
    prioritize those chunks
    wait
    return when available
```

Bind only to:

```
127.0.0.1
```

Generate an unguessable session token for the local endpoint.

---

# 28. CHUNK TRANSFER PRIORITIES

Priority 0 — Critical

```
current playback range
```

Priority 1 — Immediate future

```
next 2 minutes
```

Priority 2 — Target safety buffer

```
current position → targetBuffer
```

Priority 3 — Background completion

```
all remaining media
```

Priority 4 — speculative metadata/index ranges.

---

# 29. ADAPTIVE PRELOAD POLICY

Calculate:

```
R = measured_goodput / estimated_media_bitrate
```

Use these initial policies:

```
R >= 2.0
    minimum start buffer 60 seconds

1.5 <= R < 2.0
    120 seconds

1.2 <= R < 1.5
    300 seconds

1.05 <= R < 1.2
    900 seconds

R < 1.05
    recommend Download First
```

These are product policy constants, not universal network laws.

Make them configurable internally for later tuning.

---

# 30. LOCAL MEDIA MODES

UI exposes:

```
AUTO
STREAM SOON
SMART PRELOAD
DOWNLOAD FIRST
```

Default:

```
AUTO
```

AUTO chooses based on measured peer throughput.

---

# 31. SEEKING LOCAL MEDIA

When the host seeks to an unbuffered location:

```
HOST requests seek
        ↓
Room state = SEEKING
        ↓
Guest prioritizes requested chunks
        ↓
Host waits
        ↓
Guest reports SEEK_READY
        ↓
PLAY_COMMIT scheduled
```

The host must **not watch the destination scene before the guest is ready**.

---

# 32. KEEP / DELETE MEDIA AFTER PARTY

After a guest has received local media, show:

```
Keep this movie on this device?

[ Keep ]
[ Remove ]
```

Default button selection:

```
Remove
```

but require explicit user choice.

If `Remove`:

delete transferred media/cache after confirmation.

If `Keep`:

allow:

```
Keep in Movie Party Cache

or

Save to selected folder
```

Do not automatically export files.

---

# 33. SCHEDULED MOVIE PARTIES

A party may be scheduled:

```
movie
date
time
guest
call mode
```

Example:

```
Interstellar
Saturday 10:00 PM
```

If local media is selected, Movie Party calculates preload requirements.

---

# 34. PRELOAD START TIME

Inputs:

```
remaining bytes
last measured peer throughput
connection type
historical throughput
scheduled movie time
```

Calculate:

```
estimated_transfer_time
    =
remaining_bytes / conservative_goodput
```

Apply safety multiplier:

```
1.4
```

Add preparation margin:

```
15 minutes
```

Start earlier if possible.

---

# 35. OFFLINE PARTICIPANT SCHEDULING

When the scheduled party is accepted, both machines save the schedule locally.

Each machine registers OS notifications.

Tauri provides a native notification plugin for desktop applications. citeturn932290search3

Recommended notifications:

```
T_preload - 30 min
"Movie Party preload starts soon. Keep this device online."

T_preload
"Movie preload should start now."

T_party - 15 min
"Movie night starts in 15 minutes."

T_party - 2 min
"Movie Party is almost ready."
```

If a laptop is completely powered off, Movie Party cannot notify it until the operating system runs again.

No fake cloud push system shall be implemented.

---

# 36. BACKGROUND MODE

Movie Party may remain in:

```
system tray / menu bar
```

for:

- scheduled transfers;

- notifications;

- active preload;

- reconnect preparation.


During an active scheduled transfer, optionally prevent sleep after user permission.

Do not prevent sleep indefinitely.

---

# 37. NETWORK DIAGNOSTIC BEFORE PARTY

Every party begins with diagnostics.

Collect:

```
Tailscale reachable?
connection type?
RTT?
packet loss estimate?
upload goodput?
download goodput?
movie bitrate?
camera enabled?
estimated camera bitrate?
```

Display simplified result:

```
NETWORK

Tailscale       DIRECT ✓
Latency         18 ms
P2P speed       8.2 Mbps
Movie need      4.3 Mbps
Call            Video

Expected:
GOOD
```

Internal diagnostics retain raw data.

---

# 38. TAILSCALE PATH POLICY

## DIRECT

Preferred.

Allow normal streaming.

## PEER_RELAY

Allow streaming after throughput test.

## DERP_RELAY

Tailscale documents that DERP can add latency and throughput is rate-limited for fairness. citeturn482310search20

Therefore:

```
run full throughput test
```

before Local Stream or Provider Shared Mode.

If insufficient:

```
recommend preload
```

for local files.

For Provider Shared Mode:

```
lower quality
or
disable video camera
or
declare connection insufficient
```

---

# 39. OPTIONAL FUTURE PEER RELAY

Tailscale Peer Relays are available on the Personal plan and can use another user-owned device to relay traffic when direct connections fail. citeturn482310search4turn777396search18

Possible future setup:

```
College Mac
     ↓
Home PC
Peer Relay
     ↓
Friend laptop
```

Not required for V1.

---

# 40. VIDEO CALL REQUIREMENTS

Calling is optional.

Room options:

```
Video + Voice
Voice Only
No Call
```

Every participant may independently choose:

```
Camera On / Off
Microphone On / Off
```

Initial microphone state:

```
MUTED
```

Always.

---

# 41. CAMERA QUALITY POLICY

Movie quality has priority over camera quality.

Initial desired tiers:

```
Tier A
480p
20 fps

Tier B
360p
15 fps

Tier C
240p
10–15 fps

Tier D
camera temporarily frozen/off
```

Do not use 1080p webcam in V1.

Adaptive order:

```
network congestion
        ↓
reduce camera bitrate
        ↓
reduce camera resolution
        ↓
reduce camera FPS
        ↓
freeze camera
        ↓
disable camera
```

Do not sacrifice movie synchronization merely to preserve camera quality.

WebRTC senders can be configured with bitrate and resolution constraints, making adaptive camera quality practical if the call implementation uses WebRTC. citeturn577435search2

---

# 42. CALL IMPLEMENTATION STRATEGY

Initial implementation:

```
WebRTC peer-to-peer
```

No SFU.

No TURN service operated by Movie Party.

Because Tailscale already provides peer reachability, signalling occurs through the Movie Party QUIC connection.

Technology-validation phase must verify that WebRTC media uses a reliable route on both:

```
Windows WebView2
macOS WKWebView
```

If the browser media stack does not reliably select the Tailscale route:

replace only the call transport with a native media subsystem.

Do not redesign the room protocol.

---

# 43. ECHO PREVENTION

Provider Shared Mode must avoid transmitting captured movie audio back into the call.

Movie source audio and microphone audio are separate pipelines.

Never capture:

```
entire system output
```

when per-application capture is possible.

Windows:

use process-specific application loopback.

macOS:

filter provider application audio using ScreenCaptureKit capabilities.

---

# 44. CHAT

Chat must never shrink the movie.

No permanent sidebar.

Press:

```
Enter
```

to open compose input.

Message appears as temporary overlay.

Default display duration:

```
5 seconds
```

Then fade.

---

# 45. CHAT HISTORY

Shortcut:

```
C
```

when app is focused and no text field is active.

Open translucent overlay over movie.

Movie dimensions remain unchanged.

Close with:

```
Esc
```

---

# 46. REACTIONS

Initial reactions:

```
😂
❤️
😮
🔥
😭
👏
```

Reaction should:

- animate briefly;

- disappear automatically;

- never interrupt playback;

- use negligible bandwidth.


---

# 47. VIDEO CALL PRESENTATION

Two-person V1:

Guest camera card appears floating in a corner.

Properties:

```
draggable
resizable
minimizable
hideable
```

Default location:

```
top-right
```

Do not permanently reserve layout space.

---

# 48. GHOST MODE

Shortcut:

```
Ctrl/Cmd + Shift + M
```

Ghost Mode hides locally:

```
camera UI
chat
reactions
party controls
room indicators
Movie Party branding
```

It does **not** automatically stop:

```
camera transmission
microphone transmission
connection
sync
```

It is a visual privacy mode.

---

# 49. PRIVACY MODE

Shortcut:

```
Ctrl/Cmd + Shift + P
```

Privacy Mode performs:

```
Ghost Mode
+
Camera OFF
+
Microphone MUTED
```

Returning from Privacy Mode does **not automatically re-enable** camera or microphone.

User must restore them explicitly.

---

# 50. PLAYBACK AUTHORITY

Default:

```
HOST ONLY
```

Guest cannot directly:

```
play
pause
seek
skip
```

unless:

```
Shared Controls = ON
```

Host may enable or disable Shared Controls at any time.

---

# 51. SHARED CONTROL PROTOCOL

Guest request:

```
SHARED_CONTROL_REQUEST
```

Host can:

```
GRANT
DENY
```

When granted, guest commands still go through room coordinator.

Never allow both players to independently change media state.

---

# 52. DISCONNECTION POLICY

Strict Sync default:

```
guest disconnects
        ↓
pause movie
```

Display:

```
Rahul disconnected.

Waiting for reconnection...

[ Continue Without Rahul ]
```

Host may override manually.

Default is to wait.

---

# 53. RECONNECTION

On reconnection:

1. authenticate guest;

2. restore room state;

3. compare media identity;

4. compare local playback position;

5. restore transfer session;

6. restore buffer;

7. run ready check;

8. seek guest if needed;

9. scheduled countdown;

10. resume.


No blind resume.

---

# 54. PROVIDER BROWSER ARCHITECTURE

Each provider uses its own dedicated Chrome profile.

Example:

```
MoviePartyProfiles/
   netflix/
   prime/
   jiohotstar/
   youtube/
```

Launch Chrome with:

```
--user-data-dir=<provider-profile>
--remote-debugging-port=<local-port>
```

Remote debugging binds to localhost only.

Never expose CDP over Tailscale.

Chrome requires non-default profile directories for remote debugging under current security behavior. citeturn777396search13

---

# 55. PROVIDER AUTHENTICATION

Movie Party never displays:

```
Netflix Email
Netflix Password
```

Host logs into the actual provider page in Chrome.

Movie Party must never:

- intercept password form data;

- store passwords;

- copy authentication cookies into its SQLite database;

- transmit cookies to another participant.


Chrome maintains its normal provider session.

---

# 56. PROVIDER SUPPORT TARGETS

P0:

```
Netflix
Amazon Prime Video
JioHotstar
YouTube
```

Netflix officially supports current Chrome on Windows and macOS, with platform-dependent resolution limits. citeturn577435search0

Prime Video officially supports Chrome, Firefox, Edge and Safari for desktop playback. citeturn875678search3

JioHotstar officially lists modern Chrome, Firefox, Edge and Safari desktop browsers and requires supported protected-content configurations for playback. citeturn577435search1turn577435search23

---

# 57. PROVIDER ADAPTER INTERFACE

Every provider adapter implements:

```
ProviderAdapter {

    detect_page()

    identify_media()

    get_title()

    get_duration()

    get_position()

    get_player_state()

    get_buffer_state()

    play()

    pause()

    seek(seconds)

    set_playback_rate(rate)

    wait_until_ready()

    detect_login_required()

    detect_playback_error()

}
```

Provider-specific selectors must remain entirely inside the adapter.

---

# 58. PROVIDER DOM RESILIENCE

Provider adapters must use multiple detection strategies.

Priority:

1. direct HTML media element where available;

2. stable accessibility/semantic attributes;

3. provider-specific known player APIs;

4. DOM selectors as fallback.


Never scatter CSS selectors throughout the codebase.

---

# 59. PROVIDER ADAPTER VERSIONING

Store:

```
adapter_version
last_verified_date
provider
platform
chrome_version
```

Compatibility log example:

```
Netflix Adapter 0.4
Windows
Chrome 142
Verified 2026-08-15
```

---

# 60. PROVIDER SHARED MODE — CAPTURE PIPELINE

## Windows

```
Chrome provider window
        ↓
Windows.Graphics.Capture
        ↓
Direct3D frame
        ↓
encoder
```

Windows Graphics Capture can capture application windows and expose captured frames. citeturn875678search0turn875678search16

Audio:

```
Chrome process
        ↓
WASAPI Application Loopback
```

## macOS

```
Chrome provider window
        ↓
ScreenCaptureKit
        ↓
CMSampleBuffer
        ↓
VideoToolbox
```

Audio captured at application level through ScreenCaptureKit. citeturn840012search13

---

# 61. PROTECTED VIDEO DETECTION

Provider Shared Mode must detect unusable capture.

Possible failure:

```
normal Chrome controls captured
but
video region black
```

Implement black-frame/protected-content heuristic:

- compare expected video region;

- monitor frame variance;

- verify captured media changes while provider reports `playing`;

- allow user visual confirmation during diagnostics.


If protected video is not available:

```
Shared Mode unavailable for this provider/device.
```

Do not attempt bypass.

---

# 62. SHARED MODE ENCODING

Baseline codec:

```
H.264
```

because broad hardware decode compatibility is more important than maximum compression efficiency for V1.

Target framerate:

```
30 fps
```

Not 60 fps.

Automatic bitrate ladder:

```
1080p30 High
~5.5 Mbps target

1080p30 Medium
~4.0 Mbps

720p30 High
~3.0 Mbps

720p30 Low
~2.0 Mbps
```

Actual encoder configuration must respond to measured goodput.

No manual quality selector in V1.

Quality is automatic, per user decision.

---

# 63. SHARED MODE AUDIO

Use:

```
Opus
```

target approximately:

```
96–128 kbps stereo
```

unless implementation constraints make AAC integration substantially simpler.

Movie audio is independent from call audio.

---

# 64. SHARED MODE MOVIE TRANSPORT

Use a dedicated QUIC media channel.

Because Movie Party intentionally maintains seconds of buffer, reliable QUIC delivery is acceptable for movie content.

Do not put encoded movie bytes into chat/control streams.

Transport separation:

```
QUIC connection

├── control stream
├── chat stream
├── transfer streams
└── provider-media stream
```

---

# 65. SHARED MODE PRESENTATION ARCHITECTURE

Host must **not watch Chrome directly** while guest watches delayed Movie Party output.

That would create perceptual synchronization problems.

Instead:

```
Chrome source
    ↓
Capture
    ↓
Encoder
    ↓
Encoded stream
    ├──── Host Movie Party decoder
    └──── Guest Movie Party decoder
```

Both viewers therefore consume the same encoded representation.

---

# 66. SHARED MODE INTENTIONAL BUFFER

Introduce presentation latency deliberately.

Initial target:

```
5 seconds
```

Tune later.

Source Chrome is approximately:

```
5 seconds ahead
```

of both presentation players.

This gives room for:

- network jitter;

- retransmission;

- synchronization;

- strict pause behavior.


---

# 67. SHARED MODE BUFFER FAILURE

Suppose guest receive queue is collapsing.

Guest sends:

```
BUFFER_LOW
```

Coordinator:

1. pauses source Chrome;

2. schedules host presentation pause;

3. schedules guest presentation pause at matching PTS;

4. continues transmitting already encoded queued content;

5. guest rebuilds minimum safe queue;

6. both participants enter READY;

7. source resumes;

8. scheduled presentation resume follows.


Never allow host presentation to advance independently.

---

# 68. SOURCE LEAD

Track three timelines separately:

```
SourcePosition
EncodedPosition
PresentationPosition
```

Example:

```
Chrome source        00:12:10
Encoded              00:12:09
Host presentation    00:12:05
Guest presentation   00:12:05
```

The presentation timeline is authoritative for shared experience.

---

# 69. PROVIDER SHARED MODE FAILURE POLICY

If capture fails:

```
Attempt count: 1 diagnostic retry
```

then:

```
Offer Provider Sync Mode
```

Do not repeatedly restart capture automatically.

---

# 70. VBROWSER R&D MODE

Only start this phase after Provider Shared capture tests are complete.

Goal:

move provider browser into an isolated locally hosted environment.

Requirements:

```
No rented machine
No paid cloud GPU
No hosted browser service
```

Allowed:

```
host's own PC
user-owned spare PC
local VM
```

Test:

- DRM playback;

- hardware acceleration;

- video capture;

- audio capture;

- latency;

- 1080p capability.


If protected content remains uncapturable, close the experiment.

---

# 71. YOUTUBE

YouTube should be used as the first provider integration test because it is substantially easier to debug than subscription DRM providers.

Implement before Netflix.

It validates:

```
URL routing
managed Chrome
adapter
sync
capture
encoded shared mode
```

without DRM being the first debugging variable.

---

# 72. HOME SCREEN UX

```
┌─────────────────────────────────────┐
│             Movie Party              │
│                                     │
│ What are we watching?               │
│                                     │
│ ┌─────────────────────────────────┐ │
│ │ Paste movie / video URL...      │ │
│ └─────────────────────────────────┘ │
│                                     │
│          [ Continue ]               │
│                                     │
│               or                    │
│                                     │
│     [ Choose Downloaded Movie ]     │
│                                     │
│     [ Join Existing Party ]         │
│                                     │
│     Upcoming Parties                │
└─────────────────────────────────────┘
```

---

# 73. CREATE PARTY SCREEN

Fields:

```
Media
Guest

Call Mode
● Video + Voice
○ Voice only
○ No call

Strict Sync
ON — locked default

Controls
● Host only
○ Shared

Start
● Now
○ Schedule
```

---

# 74. LOBBY

```
INTERSTELLAR

Source:
Local File

Abhijai
Connected ✓
Movie ✓
Call ✓
Mic Muted

Rahul
Connected ✓
Movie preparing...
Camera ✓
Mic Muted

NETWORK
DIRECT
8.4 Mbps

Buffer
02:41 prepared

[ Start When Ready ]
```

---

# 75. READY CHECK

Before playback:

```
Host:
READY

Guest:
READY

Media:
READY

Network:
READY

Sync Clock:
READY
```

Only then:

```
Starting in

3
2
1
```

---

# 76. CINEMA MODE

Movie occupies the complete window.

Overlays are transient.

Mouse inactivity timeout:

```
3 seconds
```

After timeout:

hide controls.

---

# 77. CONTROL DOCK

Visible on mouse movement:

```
⏪10   ▶/⏸   10⏩

🎙
🎥
💬
❤️

Strict Sync ✓

⋮
```

---

# 78. CHAT COMPOSE

Press Enter:

```
┌─────────────────────────────┐
│ Type message...             │
└─────────────────────────────┘
```

Sending immediately returns keyboard focus to cinema.

---

# 79. BUFFERING EXPERIENCE

Never display a generic browser spinner alone.

Use Movie Party status:

```
Pausing to keep everyone together

Rahul's connection is catching up.

Buffer:
██████████░░░

Camera and chat remain available.
```

---

# 80. NETWORK QUALITY UI

Do not constantly display bitrate.

Use a discreet status icon:

```
Excellent
Good
Unstable
Buffering Risk
```

Detailed numbers available under diagnostics.

---

# 81. SETTINGS

Sections:

```
General
Playback
Network
Call
Storage
Providers
Privacy
Diagnostics
```

---

# 82. STORAGE SETTINGS

Show:

```
Movie Party cache

Used:
12.4 GB

[ Manage Cache ]

After party:
● Ask every time
○ Always remove
○ Always keep
```

V1 default:

```
Ask every time
```

---

# 83. PROVIDER SETTINGS

```
Netflix
Signed in ✓
[ Open Profile ]
[ Reset Session ]

Prime Video
Signed in ✓

JioHotstar
Not signed in
```

"Signed in" must be inferred carefully.

Never expose provider cookies.

---

# 84. KEYBOARD SHORTCUTS

When no text field is focused:

```
Space
Play/Pause
(if authorized)

Left
Seek -10s

Right
Seek +10s

Enter
Open chat

M
Toggle microphone

V
Toggle camera

C
Chat history

Esc
Close current overlay

Ctrl/Cmd + Shift + M
Ghost Mode

Ctrl/Cmd + Shift + P
Privacy Mode
```

---

# 85. SQLITE DATA MODEL

## devices

```
device_id TEXT PRIMARY KEY
display_name TEXT
platform TEXT
identity_public_key BLOB
created_at INTEGER
```

## trusted_peers

```
peer_device_id TEXT PRIMARY KEY
display_name TEXT
public_key BLOB
first_seen_at INTEGER
last_seen_at INTEGER
```

## rooms

```
room_id TEXT PRIMARY KEY
host_device_id TEXT
media_type TEXT
state TEXT
created_at INTEGER
ended_at INTEGER
```

## schedules

```
schedule_id TEXT PRIMARY KEY
room_id TEXT
media_id TEXT
scheduled_start INTEGER
planned_preload_start INTEGER
guest_device_id TEXT
status TEXT
```

## media_items

```
media_id TEXT PRIMARY KEY
media_type TEXT
title TEXT
source_uri TEXT NULL
filename TEXT NULL
file_size INTEGER NULL
duration_ms INTEGER
full_hash TEXT NULL
```

## cache_entries

```
media_id TEXT PRIMARY KEY
cache_path TEXT
bytes_available INTEGER
complete BOOLEAN
keep_policy TEXT
last_accessed INTEGER
```

## providers

```
provider_id TEXT PRIMARY KEY
adapter_version TEXT
profile_path TEXT
last_verified INTEGER
```

## network_history

```
id INTEGER PRIMARY KEY
peer_device_id TEXT
connection_type TEXT
rtt_ms REAL
goodput_bps INTEGER
timestamp INTEGER
```

## chat_messages

Optional local history:

```
message_id TEXT PRIMARY KEY
room_id TEXT
sender_device_id TEXT
body TEXT
sent_at INTEGER
```

Default retention:

```
party duration only
```

unless future setting changes it.

---

# 86. PRIVACY REQUIREMENTS

Never log:

- provider password;

- provider authentication headers;

- browser cookies;

- microphone audio;

- video call frames;

- captured movie frames;

- full personal chat content in diagnostics.


Debug logs may contain:

```
provider
adapter state
timing
bitrate
errors
```

URLs should be sanitized where they contain authentication/session parameters.

---

# 87. LOGGING

Structured logs.

Levels:

```
ERROR
WARN
INFO
DEBUG
TRACE
```

Production/dev build default:

```
INFO
```

Diagnostic bundle should contain:

```
Movie Party version
OS
architecture
Tailscale state
connection type
provider adapter version
player state transitions
sync metrics
buffer metrics
encoder metrics
errors
```

No credentials.

---

# 88. DEBUG HUD

Developer shortcut opens:

```
Room State
Host Clock Offset
RTT
Drift
Player Position
Buffer Ahead
Transfer Goodput
Tailscale Path
Encoder FPS
Encoder Bitrate
Dropped Frames
Provider State
```

This is mandatory before provider development.

---

# 89. PERFORMANCE METRICS

Record:

```
sync_error_ms
command_latency_ms
buffer_ahead_seconds
movie_goodput
call_bitrate
capture_fps
encode_fps
encode_latency
decode_latency
dropped_frames
```

---

# 90. TARGET SYNC ACCEPTANCE

Local media mode:

```
p95 drift < 100 ms
```

under stable connection.

Provider Shared Mode:

```
p95 presentation drift < 150 ms
```

Provider Sync Mode:

```
target < 250 ms
```

because provider player behavior may reduce control precision.

---

# 91. NETWORK TEST MATRIX

Every major build must test:

```
2 Mbps
5 Mbps
10 Mbps
20 Mbps
```

RTT:

```
10 ms
50 ms
150 ms
```

Packet loss:

```
0%
1%
3%
```

Temporary outage:

```
5 sec
15 sec
30 sec
```

---

# 92. CROSS-PLATFORM MATRIX

Required combinations:

```
Windows Host → Windows Guest
Windows Host → macOS Guest
macOS Host → Windows Guest
macOS Host → macOS Guest
```

No phase is complete if testing only happens:

```
Mac ↔ Mac
```

or:

```
Windows ↔ Windows
```

---

# 93. LOCAL MEDIA TEST MATRIX

Minimum files:

```
MP4 / H264 / AAC

MKV / H264 / AAC

MKV / H265

WebM / VP9

multiple audio tracks

embedded subtitles

external SRT

large >8 GB file

variable bitrate file
```

---

# 94. PROVIDER COMPATIBILITY MATRIX

For every provider/OS:

```
Launch
Login
Open URL
Detect media
Play
Pause
Seek
Read position
Detect buffering
Capture video
Capture audio
Encode
Guest decode
Strict pause
Resume
```

Example matrix:

|Provider|Windows|macOS|
|---|---|---|
|YouTube|Test|Test|
|Netflix|Test|Test|
|Prime Video|Test|Test|
|JioHotstar|Test|Test|

Never mark a provider "supported" because login merely works.

---

# 95. PROVIDER SUPPORT LEVELS

Use:

```
SUPPORTED
EXPERIMENTAL
SYNC_ONLY
UNSUPPORTED
```

Example:

```
Netflix Shared / Windows:
EXPERIMENTAL

Netflix Sync / Windows:
SUPPORTED
```

depending on actual testing.

---

# 96. ERROR MODEL

All errors must use stable codes.

Examples:

```
MP-NET-001
Tailscale unavailable

MP-NET-002
Peer unreachable

MP-NET-003
Throughput insufficient

MP-MEDIA-001
Unsupported media

MP-MEDIA-002
Missing chunk

MP-SYNC-001
Clock calibration failed

MP-PROVIDER-001
Chrome unavailable

MP-PROVIDER-002
Login required

MP-PROVIDER-003
Adapter incompatible

MP-CAPTURE-001
Capture permission denied

MP-CAPTURE-002
Protected video unavailable

MP-CALL-001
Camera permission denied
```

Never show raw stack traces to normal users.

---

# 97. DEVELOPMENT RULE

Do not begin provider Shared Mode first.

The project must prove core synchronization and networking with local media before DRM/provider complexity is introduced.

---

# 98. IMPLEMENTATION ROADMAP

---

# PHASE 0 — PROJECT FOUNDATION

## Objectives

Create a stable repository and development environment.

## Tasks

```
Initialize Tauri 2
React
TypeScript
Rust
pnpm

Configure formatting
ESLint
Prettier
rustfmt
clippy

Configure tests
Vitest
Rust unit tests

Create docs/architecture/adr

Add CI
Windows
macOS
```

## Deliverables

Application launches on:

```
Windows
macOS
```

## Gate

No feature development until both platforms build successfully.

---

# PHASE 1 — TAILSCALE CONNECTIVITY SPIKE

This is the first high-risk validation.

## Build

Host:

```
Start Room
```

Guest:

```
Join using descriptor
```

Implement:

```
Tailscale IP discovery
QUIC server
QUIC client
room authentication
heartbeat
ping
connection-state monitor
```

## Test specifically

college Wi-Fi.

Determine:

```
DIRECT?
DERP?
```

## Gate

Mac and Windows must exchange at least:

```
1 GB synthetic data
```

reliably.

Measure real throughput.

Do not proceed using assumptions about the 10 Mbps network.

---

# PHASE 2 — SYNCHRONIZATION ENGINE SIMULATOR

No movie yet.

Build two fake players exposing:

```
play
pause
seek
clock
buffer
```

Simulate:

```
latency
jitter
packet loss
buffer failure
disconnect
```

Implement:

- host clock;

- clock offset estimation;

- scheduled commands;

- strict ready consensus;

- drift correction state machine.


## Gate

Automated simulation shows no divergent playback state.

---

# PHASE 3 — LIBMPV LOCAL PLAYER

Build standalone playback.

Support:

```
open file
play
pause
seek
volume
subtitle selection
audio selection
duration
position
buffer/player state
```

## Gate

Cross-platform local playback works before networking is attached.

---

# PHASE 4 — LOCAL MEDIA IDENTIFICATION

Implement:

```
metadata
quick fingerprint
full hash
manifest
```

Guest detects if file already exists.

## Gate

Identical files are correctly detected.

Different files are never falsely accepted after full hash.

---

# PHASE 5 — P2P FILE TRANSFER

Implement:

```
chunk manifest
chunk requests
chunk transfer
chunk hash validation
resume
sparse cache
```

Implement transfer priority queue.

## Gate

Transfer survives:

```
disconnect
restart
resume
```

without restarting from zero.

---

# PHASE 6 — CACHE-BACKED STREAMING

Implement local loopback range source.

Connect libmpv to partial guest cache.

Test:

```
start playback before complete download
seek forward
seek backward
network interruption
```

## Gate

Guest can watch incomplete transferred media without corruption.

---

# PHASE 7 — STRICT LOCAL SYNC

Connect:

```
libmpv
+
sync engine
+
transfer engine
```

Implement:

```
global play
global pause
global seek
buffer consensus
disconnect pause
```

## Critical acceptance test

Artificially throttle guest until its buffer reaches zero.

Expected:

```
host pauses automatically
guest pauses
buffer recovers
both resume together
```

If host continues:

Phase fails.

---

# PHASE 8 — NETWORK-AWARE PRELOAD

Implement:

```
goodput estimator
adaptive preload
download-first recommendation
```

Use 10 Mbps college-network testing.

## Gate

App correctly warns when playback rate exceeds sustainable transfer rate.

---

# PHASE 9 — SCHEDULING

Implement:

```
schedule party
store schedule
replicate schedule to guest
calculate preload start
background transfer
```

Add OS notifications.

## Gate

Party can be scheduled hours ahead and file begins preloading automatically when both peers become available.

---

# PHASE 10 — CACHE RETENTION UX

Implement end-of-party prompt:

```
Keep
Remove
```

Verify cleanup.

---

# PHASE 11 — CINEMA UI

Only now polish presentation.

Implement:

```
fullscreen cinema
control dock
inactivity fade
buffer overlay
network state
lobby
ready countdown
```

Do not implement complex animation until functional tests pass.

---

# PHASE 12 — CHAT & REACTIONS

Implement:

```
ephemeral chat
history overlay
reactions
```

Verify no movie resize.

---

# PHASE 13 — GHOST & PRIVACY MODES

Implement global shortcuts.

Test:

```
Ghost:
UI hidden
call remains

Privacy:
UI hidden
camera off
mic muted
```

---

# PHASE 14 — CALLING SPIKE

Implement:

```
audio
camera
WebRTC signalling through room channel
```

Test all OS combinations.

## Gate

Call operates across Tailscale without paid TURN.

If this fails:

create ADR and implement native media transport.

---

# PHASE 15 — ADAPTIVE CAMERA

Measure:

```
movie buffer
network throughput
RTT
```

Automatically reduce camera quality under pressure.

## Critical test

Artificial network cap:

```
5 Mbps
```

Movie must be prioritized over camera.

---

# PHASE 16 — MANAGED CHROME

Implement provider browser manager.

Requirements:

```
find Chrome
launch dedicated profile
local CDP connection
persist provider login
navigate URL
close browser cleanly
```

No provider adapter yet.

---

# PHASE 17 — GENERIC PROVIDER ADAPTER

Implement generic HTML media control.

Test on ordinary non-DRM web video.

---

# PHASE 18 — YOUTUBE ADAPTER

Implement:

```
URL recognition
media detection
play/pause
seek
position
buffer state
```

Use it to test Provider Sync Mode.

---

# PHASE 19 — PROVIDER SYNC ENGINE

Synchronize two managed Chrome sessions.

Use YouTube first.

Then implement:

```
Netflix
Prime Video
JioHotstar
```

one at a time.

## Gate per provider

Play/pause/seek/buffer recovery must work in:

```
Windows
macOS
```

before the provider is marked supported.

---

# PHASE 20 — WINDOWS PROVIDER SHARED CAPTURE SPIKE

Do **not** immediately build a polished feature.

Build diagnostic program:

```
Launch Chrome
play provider
capture selected window
capture provider audio
save 30-second sample
```

Test:

```
YouTube
Netflix
Prime
JioHotstar
```

Record:

```
video visible?
black?
audio?
resolution?
FPS?
```

This phase determines actual feasibility.

---

# PHASE 21 — MACOS PROVIDER SHARED CAPTURE SPIKE

Repeat using:

```
ScreenCaptureKit
```

Test same providers.

Do not assume Windows results apply to macOS.

---

# PHASE 22 — HARDWARE ENCODING

Only for provider/OS combinations where capture is usable.

Implement:

Windows:

```
Media Foundation H264
```

macOS:

```
VideoToolbox H264
```

Measure:

```
capture → encode latency
CPU
GPU
FPS
bitrate
```

---

# PHASE 23 — PROVIDER SHARED TRANSPORT

Send encoded provider stream:

```
Host
→ QUIC
→ Guest
```

Implement jitter/presentation buffer.

Guest plays in Movie Party custom player.

---

# PHASE 24 — HOST LOOPBACK PLAYER

Host also consumes encoded stream.

Chrome source is no longer the user's presentation surface.

Now:

```
Host Movie Party Player
Guest Movie Party Player
```

must present the same PTS.

---

# PHASE 25 — PROVIDER SHARED STRICT SYNC

Integrate:

```
source pause
stream buffer
host presentation
guest presentation
```

Artificially throttle guest.

Expected:

```
guest falls behind
source pauses
host pauses
guest catches up
both resume
```

---

# PHASE 26 — AUTOMATIC QUALITY CONTROL

Inputs:

```
network goodput
buffer level
encoder stats
call bitrate
```

Output:

```
movie resolution
movie bitrate
camera tier
```

Priority:

```
movie
voice
sync
camera
```

---

# PHASE 27 — VBROWSER R&D

Run only if Provider Shared Mode has unacceptable limitations.

Try:

```
isolated local browser environment
local VM
user-owned second machine
```

No rented server.

Document whether DRM playback and capture work.

Close investigation if protection remains the blocker.

---

# PHASE 28 — RESILIENCE

Test:

```
Chrome crash
Movie Party crash
guest disconnect
Tailscale reconnect
network change
Wi-Fi disconnect
sleep/wake
provider logout
media file moved
cache corruption
```

Each must have explicit recovery behavior.

---

# PHASE 29 — 10 MBPS COLLEGE NETWORK CERTIFICATION

This is a dedicated real-world phase.

Test:

### Local movie

```
2 GB
4 GB
8 GB
```

### Call

```
off
voice only
video
```

### Provider Shared

all successful provider combinations.

Record:

```
actual bitrate
buffering
sync drift
Tailscale path
```

Tune constants based on real measurements.

---

# PHASE 30 — CROSS-PLATFORM REGRESSION

Run complete matrix:

```
Win → Win
Win → Mac
Mac → Win
Mac → Mac
```

No release candidate until all core Local Perfect scenarios pass.

---

# PHASE 31 — PERSONAL BETA

Use:

```
unsigned/dev builds
```

with a few trusted users.

Collect diagnostics manually.

Do not implement public accounts, payments or hosted analytics.

---

# PHASE 32 — OPTIONAL RELEASE HARDENING

Only if the project proves worthwhile.

Add:

```
signed macOS build
Windows installer
auto update
code signing
crash reporting
proper onboarding
public documentation
```

Tauri can produce Windows installer bundles; production signing/distribution is deferred until this phase. citeturn777396search16

---

# 99. PHASE COMPLETION RULE

A phase may **not be begun merely because code exists**.

A phase is complete only when:

```
implementation complete
+
unit tests pass
+
integration tests pass
+
manual acceptance test passes
+
documentation updated
+
known failures recorded
```

---

# 100. TEST-FIRST REQUIREMENT FOR RISKY SYSTEMS

The following must always be prototyped before full UI integration:

```
Tailscale transport
sync engine
partial-file playback
WebRTC call
Chrome CDP
Windows capture
macOS capture
DRM provider capture
hardware encoding
```

---

# 101. UNIT TEST REQUIREMENTS

High priority unit coverage:

```
room state machine
clock calculations
command sequencing
buffer consensus
chunk scheduler
chunk bitmap
manifest validation
hash verification
network quality classification
provider state mapping
schedule calculations
```

---

# 102. INTEGRATION TEST HARNESS

Build a headless simulator capable of running:

```
HostSimulator
GuestSimulator
```

with injectable:

```
latency
jitter
packet loss
bandwidth
disconnect
buffer underrun
```

This prevents sync testing from depending entirely on two physical machines.

---

# 103. CHAOS TESTS

Required scenarios:

```
drop every 50th control packet

pause network for 10 sec

change RTT during movie

disconnect guest during seek

disconnect host during buffer recovery

corrupt one transferred chunk

kill Chrome

restart Chrome

remove Tailscale temporarily
```

System must fail predictably rather than deadlock.

---

# 104. FILE TRANSFER ACCEPTANCE

Required:

```
4 GB file
partial start
disconnect at 35%
restart Movie Party
resume
finish
hash identical
```

Pass criterion:

```
no duplicated full transfer
no corruption
```

---

# 105. SCHEDULE ACCEPTANCE

Example:

```
Party: 10 PM
Preload: 7 PM
```

Guest offline at 7 PM.

Expected:

```
local guest notification appears if OS running

host shows:
Waiting for Rahul

guest comes online 7:30 PM

transfer starts automatically
```

---

# 106. GHOST MODE ACCEPTANCE

Activate while movie playing.

Someone observing display must see only the movie.

No visible:

```
camera
chat
room controls
Movie Party overlay
```

---

# 107. PRIVACY MODE ACCEPTANCE

Activate.

Verify:

```
camera capture stopped
remote peer receives camera-off state
mic track disabled
UI hidden
movie continues
```

---

# 108. PROVIDER AUTH ACCEPTANCE

Movie Party must pass security review demonstrating:

```
no provider passwords in logs
no provider password in SQLite
no provider cookies transmitted to guest
no login fields scraped
```

---

# 109. PROVIDER SHARED MODE RELEASE RULE

A provider may only appear in the normal Shared Mode UI after passing:

```
video capture
audio capture
stable encode
strict sync
30-minute playback test
Windows/macOS compatibility status recorded
```

Otherwise mark it:

```
EXPERIMENTAL
```

or:

```
SYNC ONLY
```

---

# 110. DEFINITION OF "SUPPORTED"

Supported does not mean:

```
worked once
```

It means:

```
repeatable
documented
tested
recoverable
```

---

# 111. OUT OF SCOPE FOR V1

Do not implement:

```
music streaming
Spotify
Apple Music
mobile apps
Linux
smart TV apps
more than two users
cloud accounts
public matchmaking
cloud movie storage
payments
public rooms
end-to-end media server
AI movie recommendation
watch history synchronization
social profiles
```

Music is explicitly postponed until movie/video functionality is complete.

---

# 112. V2 POSSIBILITIES

After V1:

```
3–6 participants
P2P swarm local-file transfer
music mode
shared playlists
voice activation
watchlists
multiple camera layouts
user-owned Tailscale Peer Relay
remote control from phone
```

Do not build these during V1.

---

# 113. FINAL V1 FUNCTIONAL CHECKLIST

V1 is complete only when:

- Windows app works.

- macOS app works.

- Windows ↔ macOS connection works.

- Two-person rooms work.

- Tailscale connectivity works on college network.

- Direct/DERP path detection works.

- Local media can be selected.

- Guest can stream partial local file.

- Original local movie bytes are transferred without transcoding.

- Full pre-download works.

- Scheduled preloading works.

- Resume transfer works.

- Identical local files avoid transfer.

- Strict play synchronization works.

- Strict pause works.

- Strict seeking works.

- Guest buffering pauses host.

- Guest disconnect pauses host.

- Reconnect restores synchronization.

- Video calling is optional.

- Voice-only works.

- No-call works.

- Mic starts muted.

- Camera quality adapts.

- Chat does not resize movie.

- Reactions work.

- Ghost Mode works.

- Privacy Mode works.

- Host-only controls are default.

- Shared Controls toggle works.

- Keep/remove downloaded media prompt works.

- Managed Chrome works.

- Provider login stays inside Chrome.

- Netflix adapter tested.

- Prime Video adapter tested.

- JioHotstar adapter tested.

- YouTube adapter tested.

- Provider Sync Mode works.

- Windows Provider Shared experiment completed.

- macOS Provider Shared experiment completed.

- Successful capture combinations stream to guest.

- Provider Shared quality adapts automatically.

- Provider Shared buffering pauses both viewers.

- Failed protected capture does not trigger circumvention.

- vBrowser R&D outcome documented if required.

- 10 Mbps real-network tests completed.

- Diagnostics bundle works.

- No provider credentials are stored by Movie Party.


---

# 114. ARCHITECTURAL DECISIONS THAT MUST NOT BE CHANGED

The following are considered **locked**:

```
Desktop app
Tauri + React + Rust
Windows + macOS
2 users V1
No rented Movie Party server
Tailscale
Host as room coordinator
QUIC control/file transport
Strict synchronization
libmpv local playback
original-byte local movie transfer
scheduled preload
guest buffering pauses host
optional video call
mic muted initially
automatic media quality
camera degrades before movie
transient chat
Ghost Mode
Privacy Mode
host-only controls by default
managed Chrome for providers
dedicated provider Chrome profiles
no provider credentials stored
three media modes
experimental Provider Shared Mode
Provider Sync fallback
no DRM key extraction
vBrowser only self-hosted/user-owned
music postponed
```

Any change requires an ADR.

---

# 115. FIRST ENGINEERING MILESTONES

The first practical target should **not** be "watch Netflix together."

The first four milestones are:

```
M1
Mac ↔ Windows Movie Party connection over Tailscale

M2
Two fake players remain synchronized under artificial lag

M3
4 GB MKV transfers partially and plays while downloading

M4
Guest buffer failure automatically pauses host
```

If these four are solid, the foundation is correct.

Only then move to:

```
video calling
UI polish
Chrome
providers
capture
```

---

# 116. THREE HIGHEST-RISK EXPERIMENTS

These must be treated as engineering research gates.

## Risk A — College-network Tailscale throughput

Known:

Tailscale can connect using direct, Peer Relay or DERP paths; direct normally provides better performance. citeturn482310search2

Unknown:

actual throughput on the college network.

Therefore measure it.

---

## Risk B — Provider protected-window capture

Windows and macOS both provide legitimate application/window capture APIs. citeturn875678search0turn875678search1

Unknown:

whether each provider/OS/GPU/DRM combination exposes usable video frames.

Therefore test rather than assume.

---

## Risk C — Call transport on Tailscale

WebRTC supports adaptive media encoding, but actual WebView/Tailscale routing must be measured on both platforms. citeturn577435search2

Therefore implement a spike before committing the entire call subsystem.

---

# 117. SUCCESS DEFINITION

Movie Party V1 succeeds if two people using Windows/macOS and a restrictive network can:

```
open Movie Party

join each other through Tailscale

choose a local movie
or supported provider source

optionally video call

watch without a side-panel UI

remain tightly synchronized

have playback automatically pause when either person cannot continue

chat/reaction without interrupting the movie

hide social UI instantly

schedule movies ahead of time

pre-transfer local media

do all of this without paying for Movie Party server infrastructure
```

The strongest supported experience should be:

```
LOCAL PERFECT MODE
```

The most ambitious feature should be:

```
PROVIDER SHARED MODE
```

The compatibility fallback should be:

```
PROVIDER SYNC MODE
```

---

# 118. FINAL SYSTEM MODEL

```
                           Movie Party

                      Windows / macOS
                             │
            ┌────────────────┼─────────────────┐
            │                │                 │
            ▼                ▼                 ▼

       MEDIA ENGINE      SOCIAL ENGINE      SYNC ENGINE
            │                │                 │
            │                ├ camera          ├ clock
            │                ├ microphone      ├ consensus
            │                ├ chat            ├ drift
            │                ├ reactions       ├ buffering
            │                ├ ghost           └ recovery
            │                └ privacy
            │
     ┌──────┼───────────────┐
     │      │               │
     ▼      ▼               ▼

   LOCAL  PROVIDER       PROVIDER
  PERFECT  SHARED         SYNC
    │       │               │
 original Chrome         Chrome
 chunks    capture       adapter
    │       │               │
    └───────┴───────────────┘
                 │
                 ▼
            NETWORK CORE
                 │
              QUIC
                 │
             TAILSCALE
                 │
          ┌──────┴──────┐
          │             │
         HOST         GUEST
```

This architecture is the V1 baseline.

Implementation proceeds **phase by phase in the exact dependency order defined above**.