# Movie Party — Master Remaining Work & Production Roadmap

> **Purpose:** This is the living source of truth for everything still required to finish Movie Party V1.  
> **Scope:** Two-person private desktop watch parties on macOS and Windows.  
> **Current branch:** `emergent-ui-integration-complete`  
> **Latest checkpoint discussed:** `0e8f387` — Windows deep links + provider-mode separation  
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

## Highest-priority current items

1. 🔴 **Native libmpv video presentation inside the Cinema surface**
2. 🔮 **First-run Tailscale onboarding + partner connectivity setup**
3. 🔴 **Video-call local/remote state separation and Lobby/Cinema call UX fixes**
4. 🔴 **Chat size/translucency and Lobby layout cleanup**
5. 🔴 **Streaming-provider selector visual bug**
6. 🧪 **Real local playback validation**
7. 🧪 **Two-device macOS ↔ Windows validation**
8. 🔮 **Scheduled Local Perfect preload completion**
9. 🟡 **Provider Sync login/preparation UX**
10. 🟡 **Provider Shared capture → encode → QUIC → guest presentation**
11. 🔮 **Ghost Mode final implementation**
12. 🎨 **Hero reel redesign by another AI**
13. 🎨 **Cinema curtain/countdown visual polish by another AI**
14. ⚠️ **Re-check unresolved audit/security/reliability findings against the current branch**
15. 🔮 **Packaging/release hardening**

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
  `moveparty://join/<room_id>#<descriptor>`
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
- 🧪 Real provider login/navigation/control remains to be verified manually.

---

# 4. CRITICAL BLOCKER — Native Movie Rendering

## Status

🔴 **Confirmed during manual testing**

The app currently reaches Cinema but reports that libmpv is not attached to a native renderer host.

Observed behavior:

> libmpv is configured not to create a second window; attach the native render host to present video.  
> NSView/CALayer host for libmpv render context.

This means Movie Party can reach the playback stage without actually presenting the movie inside the Cinema surface.

## 4.1 Required macOS architecture

```text
React Cinema surface
        ↓
Tauri native presentation host
        ↓
NSView / CALayer
        ↓
libmpv render context
        ↓
actual movie frames
```

## 4.2 Acceptance criteria

- Movie renders inside the Movie Party Cinema surface.
- No detached mpv window is used.
- Movie canvas resizes correctly with the Tauri window.
- Movie remains behind overlays.
- Play/Pause/Seek affect the real decoder/player.
- Position and duration reflect the real player.
- Player errors cannot leave the backend/frontend claiming false `PLAYING`.
- Video does not autoplay before the authoritative synchronized start.
- Cleanup works when ending/leaving/reloading a party.

## 4.3 Windows

🟡 The equivalent native presentation host must also exist for Windows before cross-platform Local Perfect can be considered complete.

## 4.4 Manual proof required

🧪 Test a real local movie on macOS first, then Windows.

---

# 5. Tailscale First-Run Onboarding and Partner Connectivity

## Status

🔮 **Required V1 onboarding feature**

A fresh Movie Party install must not assume that Tailscale is ready.

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

- 🔴 Native player presentation must be fixed first.
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

🟡 Code-level wiring exists; real provider operation is not yet production-proven.

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
moveparty://join/<room_id>#<descriptor>
```

A bare short room code is insufficient without adding a rendezvous/lookup service, which is not part of the current serverless architecture.

## 12.2 macOS

- ✅ code/config plumbing exists
- 🧪 test installed/bundled app:
  - app closed
  - app already running

## 12.3 Windows

- ✅ official Tauri deep-link/single-instance plumbing exists
- 🧪 verify Windows installer actually registers `moveparty://`
- 🧪 verify:
  - cold start
  - running app
  - full invite preserved
  - Join Party prefill
  - successful Lobby transition

---

# 13. Video Call — Correct State Model

## Status

🔴 Current manual testing exposed a likely local/remote state-model/UI mismatch.

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

🔴 Confirmed visually/manual.

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

🔴 Needs a focused cleanup after the critical player work.

Current Lobby is visually cluttered when call/chat are present.

## Desired hierarchy

1. Movie / lobby purpose
2. Participants / connection state
3. Ready state
4. Invite
5. Social controls
6. Floating chat/call only when requested

Chat and call must not become a permanent sidebar.

## 15.1 Ready Check overflow

Confirmed issue:

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

🔴 Current panel remains smaller than the intended design.

The requirement is **not** “make chat smaller so it interrupts less.”

Correct requirement:

```text
useful previous/larger size
+
translucent background
+
strong backdrop blur
+
movie/lobby visible behind it
```

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

---

# 17. Provider Selector Visual Bug

## Status

🔴 Confirmed in Create Party.

The Provider field currently renders as a white blank/native-looking area even when a provider is selected.

Fix only the provider selector/component/theme behavior.

Acceptance:

- dark Emergent styling;
- selected provider text visible;
- no white native rectangle;
- focus/hover/disabled states;
- keyboard accessible;
- no source-flow redesign required.

---

# 18. Ghost Mode

## Status

🔮 Required.

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

Privacy Mode remains distinct from Ghost Mode.

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

⚠️ Before implementing changes, verify whether these are still true:

- drift-correction helper has no runtime caller;
- no continuous drift measurement/correction loop;
- reconnect/backoff may be incomplete;
- host crash detection may depend mainly on QUIC idle timeout;
- heartbeat exists but may not be fully scheduled;
- overlapping operation cancellation/abort semantics may be incomplete;
- some coordinator state may still depend on large `app_runtime.rs` synchronization blocks.

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

⚠️ Check current branch for:

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

## Future release-hardening work

### libmpv

- decide/bundle required native runtime correctly;
- macOS dylib/runtime loading;
- Windows DLL/runtime loading;
- startup detection;
- user-safe unavailable state;
- installer verification.

### Tailscale

- first-run dependency/onboarding;
- installer/setup handoff;
- sign-in handoff;
- partner connectivity setup;
- runtime health checks.

### macOS

- app bundle;
- signing;
- notarization when release-ready;
- protocol-handler verification;
- permissions/privacy strings.

### Windows

- installer;
- protocol-handler registration;
- native renderer;
- camera/mic permissions;
- required runtime/native libraries.

### Security/release

- CSP;
- release Rust profile;
- dependency audit;
- privacy/data-handling notice;
- versioning/release notes;
- reproducible build checks.

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

## Phase 1 — Critical playback gate

1. 🔴 Native libmpv presentation on macOS
2. 🧪 Real single-device local playback
3. 🟡 Windows native presentation path
4. 🧪 Windows local playback

## Phase 2 — First-run connectivity

5. 🔮 Tailscale installation detection
6. 🔮 Tailscale sign-in handoff/status
7. 🔮 partner/tailnet/share setup guidance
8. 🔮 reachability validation
9. 🔮 onboarding persistence/recovery

## Phase 3 — Confirmed social/UI correctness issues

10. 🔴 Separate local vs remote call states
11. 🔴 Fix call-tile dragging
12. 🔴 Prevent text selection while dragging
13. 🔴 Fix call-tile z-order
14. 🔴 Implement real minimize semantics
15. 🔴 Implement close/hide semantics
16. 🔴 Remove unnecessary remote tile device buttons
17. 🔴 Add small remote mute indicator
18. 🔴 Fix Lobby Ready Check overflow only
19. 🔴 Restore larger translucent chat in Lobby
20. 🔴 Restore larger translucent chat in Cinema
21. 🔴 Fix provider selector white-field bug
22. 🔴 Reduce Lobby clutter

## Phase 4 — Ghost / privacy UX

23. 🔮 Ghost Mode final state-preserving implementation
24. 🔮 Privacy Mode verification
25. 🧪 Verify camera continues during Ghost Mode
26. 🧪 Verify Privacy exit does not reactivate devices

## Phase 5 — Real two-device Local Perfect

27. 🧪 macOS ↔ Windows
28. 🧪 strict sync
29. 🧪 buffering
30. 🧪 disconnect/reconnect
31. Fix only evidence-driven failures

## Phase 6 — Provider Sync UX

32. 🟡 provider sign-in via managed browser
33. 🟡 provider session status
34. 🟡 browse/select title on provider
35. 🟡 prepare provider party
36. 🧪 provider-specific compatibility tests

## Phase 7 — Scheduling / preload

37. 🧪 Local Perfect scheduled pre-transfer
38. 🧪 online/offline notification flow
39. 🧪 preload percentage/readiness
40. 🧪 retention flow proof
41. 🔮 Provider Sync scheduled preflight

## Phase 8 — Provider Shared

42. capture
43. audio capture
44. encode
45. timestamp sync
46. dedicated QUIC stream
47. guest buffering
48. decode
49. guest presentation
50. protected-video failure/fallback
51. 🧪 macOS/Windows/provider tests

## Phase 9 — Reliability/security revalidation

52. Re-run production audit on current branch
53. Fix only confirmed remaining findings
54. reconnect/backoff
55. drift loop
56. bounds/limits
57. CSP
58. logging/diagnostics
59. release hardening

## Phase 10 — External visual polish

60. 🎨 Hero reel redesign by another AI
61. 🎨 Curtain/spotlight/countdown polish by another AI
62. Other small visual polish after functionality freezes

## Phase 11 — Final release gate

63. complete cross-platform matrix
64. package native dependencies
65. installer tests
66. privacy/security review
67. final production audit
68. V1 release candidate

---

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
