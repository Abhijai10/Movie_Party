# Batch 7B — Real Beta Validation Procedure (two devices, real media)

**Status:** procedure prepared, **NOT executed**. No application code was modified in this batch.
**Scope:** manual, two-physical-device validation of whether Movie Party can genuinely play real
movies together.
**Companion:** `BATCH7A_CI_VERIFICATION.md` (automated CI evidence for the same code).

---

## 0. The rule this document is written under

> Do not claim a test passes merely because the relevant code exists.

Every test below is therefore specified as an **observation with an artefact**, not as an
inspection. Where a capability cannot be exercised in the available environment, the test is
classified **BLOCKED** or **NOT TESTED** — never inferred as passing from source code.

Corroborating evidence that this distinction matters: `RELEASE_NOTES.md` §0.9.0 states outright that
*"The §31.1 manual matrix (two devices, real providers, real DRM content) is pending physical
hardware verification."* As of this document, that has not changed. **The real two-device beta is
unproven.**

---

## 1. Build under test — provenance must be recorded per device

The stabilized code exists only on the candidate branch; there is no released installer for it.
Pick **one** of the two options below and record which one was used on **both** devices.

| Option | Build | SHA | Notes |
|---|---|---|---|
| **A (recommended)** | Local build from `stabilization/v0.9.9-rc1` | `2d5c83391dec43342bb4346ae19c542a6e672385` | Contains Batches 1–6. Requires toolchain + libmpv staging on macOS. |
| **B (fallback)** | GitHub Release **v0.9.8** installer | `bb225778434e3f07923e03e85d7d7cc1db79146c` | **Does NOT contain the Batch 1 sequence-reorder fix.** A run on Option B validates v0.9.8, not the stabilized code, and must be labelled as such in the sign-off. |

**Record before starting:**

```
Device A (host) : OS + version ..............  Build option (A/B): ....  SHA: ............
Device B (guest): OS + version ..............  Build option (A/B): ....  SHA: ............
```

Both devices **must** run the same SHA. A mixed-version run is not a valid beta result.

### macOS libmpv prerequisite (Option A)

A clean checkout has no bundled libmpv (`src-tauri/mpv_runtime/*` is gitignored). On macOS the
runtime must be built and staged first:

```
scripts/build-libmpv-macos.sh      # ~long; needs Homebrew + build deps
scripts/stage-libmpv-macos.sh
```

**If libmpv is not staged, every local-media playback test (T05, T06, T07, T09–T14, T20–T24) is
BLOCKED on that device.** This is not a workaround-able condition — without the runtime there is no
decoder. Confirm with `ls src-tauri/mpv_runtime/libmpv.dylib` (macOS) or `libmpv.dll` (Windows)
before starting.

### Test media

`scripts/make-test-media-macos.sh` (backed by `scripts/make-test-media-macos.swift`) generates the
synthetic clips used by the automated suite. For a *real beta* these are **insufficient on their
own** — T21/T22 require genuine feature-length and multi-codec content. See §3.

---

## 2. Evidence conventions — what counts as proof

A test is only as good as its artefact. Three evidence channels exist; know their limits.

| Channel | What it gives | Limit |
|---|---|---|
| **E1 — Screen recording, both devices** | The primary artefact for anything user-visible (countdown, playback, drift, recovery UI) | Requires a shared external time reference to compare the two streams (a phone filming both screens, or a clap/second-device clock). |
| **E2 — On-screen player clock** (`CinemaView`, `formatMs(player.positionMs)` + duration) | Position, duration, `(Buffering…)`, player error text | **1-second resolution.** Fine for "did they stay together", too coarse to measure sub-second drift. |
| **E3 — Debug HUD** (`src/components/DebugHud.tsx`) | Room state, Media, **Position to ms**, RTT, Path, Goodput, Guest buffer, Camera tier, Call status, Strict-sync | **Dev-gated.** Enabled only when `window.location.hostname` is `localhost`/`127.0.0.1` **and** `?debug` is in the URL. A packaged build has no address bar, so this is reachable only from a dev preview (`pnpm tauri dev`). **Verify reachability empirically per platform; do not assume it.** |

**Additionally:**

- **E4 — Diagnostic bundle** (Settings → Diagnostics → *Export diagnostic bundle*). Writes
  `movie-party-diagnostics-<date>.json`, local-only. Contains `network`, `room`
  (`role`/`state`/`strictSync`), `media` (`filename`/`fileSize`), `provider`, `call`
  (`mode`/`status`/`cameraTier`).
  **Important limitation: the bundle does NOT contain playback position.** It cannot be used to
  evidence drift or A/V sync. Use E1/E2/E3 for those.

- **E5 — Settings → Diagnostics → "Connection diagnostics"** (host measures RTT on the live
  session). Use for T02 and T27.

**Every classification must cite which channel produced the evidence.** "It worked" without an
artefact is a NOT TESTED.

---

## 3. Environment prerequisites

| Prerequisite | Needed by | If absent |
|---|---|---|
| Two physical devices (one Windows, one macOS ideally — cross-platform is the real risk) | everything | all tests BLOCKED |
| Tailscale installed and **signed in to the same tailnet** on both devices | T01–T28 | T02, T03, T04, T16, T17, T27 BLOCKED |
| Real media: ≥1 feature-length file (~2 h), ≥1 large file (≥4 GB), and multiple containers/codecs (MP4/H.264, MKV/H.265, and at least one audio-only or 5.1 track) | T05, T21, T22, T23 | those tests BLOCKED |
| libmpv staged (macOS, Option A) | all local playback | all local playback BLOCKED |
| Camera + microphone, with OS permission granted to the app | T26 | T26 BLOCKED |
| Provider accounts (Netflix / Prime / JioHotstar) signed in on the **managed Chrome** profile, on both devices | T25 | T25 BLOCKED (sync) |
| Working DRM playback in the managed browser | T25 | T25 BLOCKED — note that **Provider Shared is deliberately unavailable** (§5) |
| A second person (or a second pair of hands) to drive device B | all interactive tests | cannot be run solo |

---

## 4. Test matrix

Classification column is left as **NOT TESTED** for all rows. Nothing here has been executed.
`BLOCKED`-on-prerequisite is noted per test. The operator replaces each with PASS / FAIL / BLOCKED /
NOT TESTED and cites the evidence channel.

### Group A — Connectivity and party formation

---

**T01 — Two physical devices**
- **Setup:** Device A and Device B, both powered, both on the same LAN, both running the same SHA.
- **Action:** Confirm distinct machine identities: Settings → General on each device shows a
  different device id. Confirm both are reachable to each other.
- **Expected:** Two genuinely separate machines with separate identities and separate media files.
- **Evidence:** E1 (photo/video of both screens side by side), plus each device's identity.
- **Classification:** NOT TESTED — *this is the gate for the entire matrix.*
- **Note:** A single machine running two instances does **not** satisfy this test; it shares the
  identity store and the network stack.

---

**T02 — Real Tailscale connectivity**
- **Setup:** Tailscale signed in on both devices to the same tailnet; note each device's MagicDNS
  name.
- **Action:** In Movie Party, open the Tailscale readiness surface; then use Friends → *Verify* /
  *Connect* against the peer. This runs a real `tailscale ping`.
- **Expected:** Readiness reports Running; verification reports a real path and latency
  (e.g. "Connection verified · direct · 23 ms"). `network.path` and `network.rttMs` are populated.
- **Evidence:** E5 (Connection diagnostics), E1 (screenshot of the verified state), and the
  `tailscale ping` output.
- **Classification:** NOT TESTED
- **Honest failure modes to distinguish:** "Online — not verified yet" is *not* a pass. "Offline"
  is *not* a pass. Only a probed path counts.

---

**T03 — Host creation**
- **Setup:** Both devices verified (T02).
- **Action:** On Device A: Home → Create Party → select a real movie file → Create cinema room.
- **Expected:** Room created; host lands in Lobby/Ready Check with `room.role = host`; an invite
  (link / short code / QR) is offered.
- **Evidence:** E1 (host screen), E4 (`room.role`, `room.state`).
- **Classification:** NOT TESTED

---

**T04 — Guest joining**
- **Setup:** Host room open (T03).
- **Action:** On Device B: paste the invite link (or scan the QR / enter the short code) → Join.
- **Expected:** Guest reaches the lobby; **both** devices list two participants, each
  `connected: true`. Host observes the guest joining.
- **Evidence:** E1 (both screens simultaneously), E4 from both devices.
- **Classification:** NOT TESTED

---

### Group B — Real media playback

---

**T05 — Real movie files**
- **Setup:** A genuine feature-length file on the **host** device (not the synthetic fixture).
- **Action:** Create the party with that file; let the transfer/availability path resolve.
- **Expected:** The real filename and real size appear in the media manifest on both devices;
  playback uses the real file, not a placeholder.
- **Evidence:** E1 (Cinema shows the correct title/duration), E4 (`media.filename`, `media.fileSize`).
- **Classification:** NOT TESTED

---

**T06 — Video + audio playback**
- **Setup:** T05 complete, both sides in Cinema.
- **Action:** Play. Observe picture and listen for audio on **both** devices.
- **Expected:** Real video frames render on both; real audio is audible on both; no black screen, no
  silent track, no audio-only fallback.
- **Evidence:** E1 — recording capturing **both** screens *and* both audio outputs (or a separate
  recording of each device's audio).
- **Classification:** NOT TESTED
- **Note:** The automated suite has never proven real decoded frames on a real device in CI (see
  §5, libmpv skip). This is the first test that can actually establish it.

---

**T07 — Synchronized start**
- **Setup:** T06 complete.
- **Action:** Host starts the movie.
- **Expected:** Both devices begin playing the same content at the same nominal position.
- **Evidence:** E1 with a shared time reference; E3 (`sync.positionMs` on both) if the HUD is
  reachable.
- **Classification:** NOT TESTED

---

**T08 — Countdown**
- **Setup:** Both ready.
- **Action:** Host triggers the start countdown.
- **Expected:** A **backend-driven** 3-2-1 countdown (backend constant `COUNTDOWN_LEAD_US` =
  3 s) appears and both devices hand off into Cinema once. The hand-off must not fire twice.
- **Evidence:** E1 (countdown visible and counted on both, once).
- **Classification:** NOT TESTED
- **Regression to watch:** the v0.9.8 note "the countdown starts once" — a double hand-off is a
  FAIL.

---

### Group C — Transport control

---

**T09 — Pause / resume**
- **Setup:** Playing (T07).
- **Action:** Host pauses; observe both. Then host resumes; observe both.
- **Expected:** Pause propagates to both; both show PAUSED. On resume both return to PLAYING.
  Host-only control by default (`room.hostOnlyControls`), with the guest's intent routed through
  the coordinator.
- **Evidence:** E1 (both screens), E3 (`sync.roomState` = PAUSING → PAUSED → PLAYING on both).
- **Classification:** NOT TESTED

---

**T10 — Repeated pause / resume**
- **Setup:** Playing (T07).
- **Action:** Perform pause/resume **at least 10 times in quick succession**, alternating host- and
  guest-initiated if shared controls are enabled.
- **Expected:** Every cycle converges on both devices; no lost transition, no device stuck in
  PAUSING/PLAYING, no desync. **This is the manual analogue of the Batch 1 reorder defect** — the
  failure mode it guards against is a control message being dropped and never retried.
- **Evidence:** E1 (continuous recording), E3 (`sync.roomState` sampled per cycle).
- **Classification:** NOT TESTED
- **Note:** This is the highest-value manual test in the document. If any cycle silently does
  nothing on one side, capture it — that is the same class of defect Batch 1 fixed.

---

**T11 — Forward seek**
- **Setup:** Playing.
- **Action:** Host seeks forward (e.g. +60 s) via the control bar.
- **Expected:** Both devices land on the same new position; playback resumes together.
- **Evidence:** E1; E2/E3 for position on both.
- **Classification:** NOT TESTED

---

**T12 — Backward seek**
- **Setup:** Playing.
- **Action:** Host seeks backward (e.g. −60 s).
- **Expected:** Both devices land on the same new position; playback resumes together.
- **Evidence:** E1; E2/E3 for position on both.
- **Classification:** NOT TESTED

---

**T13 — Repeated seeks**
- **Setup:** Playing.
- **Action:** Seek forward and backward **rapidly, ≥10 times**, including seeking while a previous
  seek is still settling.
- **Expected:** The room converges on a single position; no device is left in SEEKING; no device
  plays from a stale position. Rapid re-seek is the second-most-likely place for a dropped control
  message.
- **Evidence:** E1 (continuous recording), E3 (`sync.roomState` must return to PLAYING each time).
- **Classification:** NOT TESTED

---

### Group D — Duration, degradation, recovery

---

**T14 — Long-duration playback**
- **Setup:** Feature-length file (≥90 min).
- **Action:** Play continuously for **≥30 minutes** without intervention.
- **Expected:** Playback continues on both; no unbounded drift; no memory/decoder failure; no
  spontaneous pause.
- **Evidence:** E1 (time-lapse or periodic screenshots at 0/10/20/30 min), E3 if available.
- **Classification:** NOT TESTED
- **Limit:** "Long" here is 30 minutes. A full 2-hour soak is a stronger test and is recommended if
  time allows; record which duration was actually run.

---

**T15 — Buffering / network degradation**
- **Setup:** Playing (T07).
- **Action:** Degrade the guest's network (throttle the link, or move one device to a congested
  Wi-Fi). Optionally force a low-buffer condition.
- **Expected:** Strict-sync pauses the room rather than letting the two drift
  (`sync.strictSyncPaused = true`); `buffer.bufferingParticipant` names the slow side; the room
  resumes when the buffer recovers. **The design intent is "we pause together" — not "one side
  keeps playing".**
- **Evidence:** E1 (both screens showing the sync pause), E3 (`Strict` = PAUSED,
  `Guest Buf`), E4 (`room.strictSync`).
- **Classification:** NOT TESTED

---

**T16 — Disconnect / reconnect**
- **Setup:** Playing (T07).
- **Action:** Break the guest's connection (disable Tailscale or pull the network). Wait past the
  ~4 s overlay grace. Then restore it.
- **Expected:** The movie pauses; the reconnect overlay appears ("<peer> disconnected. The movie has
  been paused. Reconnecting…"). After the grace, host sees **[Keep Waiting]** and **[Continue
  Without <peer>]**; guest sees **[Keep Waiting]** only. On restore, the session recovers and
  playback resumes.
- **Evidence:** E1 (overlay text and buttons on both), E4 (`room.state` = RECONNECTING), E3.
- **Classification:** NOT TESTED
- **Regression to watch:** v0.9.8 fixed "dismissing Keep Waiting once disabled the overlay
  permanently". Dismiss it, then cause a **second** disconnect — the overlay must appear again.

---

**T17 — Late join / rejoin**
- **Setup:** A party already playing (T07).
- **Action:** Bring a device in that was not present at formation (or rejoin after leaving). Observe
  how it is admitted and how it catches up.
- **Expected:** Record the actual behaviour. If the stable feature set does not support mid-playback
  join, that is the finding — report it as unsupported rather than as a failure of a feature that
  does not exist.
- **Evidence:** E1, E4 from all devices.
- **Classification:** NOT TESTED

---

**T18 — Guest leaving**
- **Setup:** Playing (T07).
- **Action:** Guest presses Leave.
- **Expected:** Guest sees guest-appropriate wording (v0.9.8 fixed the host-only "End Movie Party for
  everyone?" text appearing for guests). Host is informed and the room does not silently continue as
  if nothing happened.
- **Evidence:** E1 (both screens).
- **Classification:** NOT TESTED

---

**T19 — Host leaving**
- **Setup:** Playing (T07).
- **Action:** Host presses Leave / End Party for everyone.
- **Expected:** The host's confirmation is host-scoped; the guest is told the party ended rather than
  being left in a phantom room. Post-party retention prompt (§52) behaviour recorded.
- **Evidence:** E1 (both screens), E4 (`retention_prompt` if applicable).
- **Classification:** NOT TESTED

---

### Group E — Media variety

---

**T20 — Second movie**
- **Setup:** First movie played to completion or stopped (T06–T14).
- **Action:** Use "Change movie" / pick a different file and start again.
- **Expected:** The **new** movie is actually the one that plays. (v0.9.8 fixed a bug where the
  active party's media took precedence over an explicit pick, so the new choice was silently
  discarded.) Both devices play the same new file.
- **Evidence:** E1 (the new title is visibly playing on both), E4 (`media.filename` changes on both).
- **Classification:** NOT TESTED

---

**T21 — Different media formats**
- **Setup:** A set of files: MP4/H.264, MKV/H.265, and at least one with a non-stereo (e.g. 5.1)
  audio track. One file may be audio-only if the feature set supports it.
- **Action:** Play each container/codec combination in turn, on both devices.
- **Expected:** Record which combinations decode and play on **each** platform. Cross-platform codec
  support is a genuine risk: a codec available on macOS may be missing on Windows.
- **Evidence:** E1 per file per device, E2 (`player.errorMessage` if a file fails).
- **Classification:** NOT TESTED — **this is a per-format matrix, not one pass/fail.** Tabulate it.
- **Honest expectation:** it is entirely plausible that some formats fail on one platform. That is a
  finding to report, not a test to retry until green.

---

**T22 — Large media files**
- **Setup:** A file ≥4 GB.
- **Action:** Create the party with it and start playback; observe transfer/preload behaviour and
  time-to-first-frame.
- **Expected:** The file is handled without truncation or path-length failure; record actual
  time-to-first-frame and any transfer progress (`transfer`).
- **Evidence:** E1, E4 (`media.fileSize`), E2.
- **Classification:** NOT TESTED

---

**T23 — Audio/video synchronization**
- **Setup:** A file with clearly synchronised content (speech, or a clap/percussion segment).
- **Action:** Play on both devices; watch for lip-sync error on each independently.
- **Expected:** A/V stays in sync **within each device** (this is a different property from
  device-to-device sync in T24).
- **Evidence:** E1 (close-up recording of each screen with its own audio).
- **Classification:** NOT TESTED

---

**T24 — Actual observed playback drift**
- **Setup:** Playing (T07), both devices visible in one frame.
- **Action:** Measure the **actual** position difference between the two devices — at start, then
  after 5, 15, and 30 minutes of playback.
- **Expected:** Record the number. There is no pre-agreed pass threshold in the code; the honest
  output is a measured value with its resolution stated.
- **Evidence:** E2 gives 1-second resolution (sufficient for "seconds apart"). For sub-second drift,
  E3 (Debug HUD, ms) is required and is **dev-gated** — if the HUD is unreachable in the packaged
  build, say so and report drift at 1-second resolution instead. Do **not** report millisecond drift
  you could not measure.
- **Classification:** NOT TESTED
- **Note:** `sync.positionMs` is the authoritative synchronized position; `player.positionMs` is
  what is actually being rendered. Drift between the two devices is visible in the **player**
  position. Capture both if the HUD is available.

---

### Group F — Provider, call, network transitions, recovery

---

**T25 — Real provider / Chrome playback**
- **Setup:** Provider accounts signed in on the managed Chrome profile on both devices.
- **Action:** Launch a provider title on both (YouTube via direct URL; Netflix / Prime / JioHotstar
  via provider search). Attempt sync playback.
- **Expected:** Record what actually happens. Known, documented state:
  - The four providers are declared `sync_available: true`, `shared_available: false`.
  - **Provider Shared is deliberately unavailable** — its reason string is *"Provider Shared is
    experimental and unavailable until capture is verified on this device."*
  - Provider capability `verification` is `EXTERNAL_VERIFICATION_PENDING`.
  So **Provider Shared tests are BLOCKED by design**, and that is the correct outcome — not a
  defect. Provider **Sync** mode may be exercised, subject to real accounts and DRM.
- **Evidence:** E1 (both screens showing the same title), E4 (`provider` block).
- **Classification:** NOT TESTED (Shared: BLOCKED by design)
- **Do not** enable experimental capture/streaming to make this pass. Provider Shared must remain
  untouched.

---

**T26 — Camera / microphone / call flows**
- **Setup:** Camera and microphone present; OS permission granted (macOS declares
  `NSCameraUsageDescription` / `NSMicrophoneUsageDescription`).
- **Action:** Start the call. Toggle microphone. Toggle camera **off then on again**. Observe the OS
  camera-in-use indicator. If the bandwidth ladder engages, record the camera tier.
- **Expected:** Camera and mic capture on both; audio and video flow both ways. **Turning the camera
  off must release the device** — the OS indicator must go out (v0.9.8 fixed a bug where off only
  muted the track, leaving the device open). Re-enabling re-acquires it. A failed call must be able
  to retry.
- **Evidence:** E1 (call tile on both), the macOS camera-in-use indicator, E4
  (`call.mode`/`status`/`cameraTier`), E3 (`Camera` tier).
- **Classification:** NOT TESTED
- **Environment warning:** on a machine where the OS has not granted device access to the app, this
  test cannot pass — it is BLOCKED, not failed.

---

**T27 — Tailscale online / offline transitions**
- **Setup:** Party formed and playing.
- **Action:** Take Tailscale offline on one device mid-session, then bring it back. Optionally switch
  networks (Wi-Fi → hotspot).
- **Expected:** The app detects the loss and recovers per the reconnect design; readiness surfaces
  reflect reality rather than a stale "Running". Friends' online state must not claim a connection
  that was not probed.
- **Evidence:** E1, E5, E4 (`network.connected`, `network.path`).
- **Classification:** NOT TESTED

---

**T28 — Error / recovery paths**
- **Setup:** A healthy session, plus the ability to induce faults.
- **Action:** Induce and record each of: (a) movie file moved/deleted mid-session; (b) provider
  failure / browser closed; (c) player failure; (d) sleep/wake of one device; (e) host crash.
- **Expected:** Each produces a clear, non-silent failure and a genuine recovery path — no silent
  black screen, no stuck state, no error that disables the feature permanently. (v0.9.8 addressed
  several of these: sticky libmpv-unavailable flag, dead "Back to lobby", unrecoverable call.)
- **Evidence:** E1 per scenario, E4 (`error`, `lastRecovery`), E2 (`player.errorMessage`).
- **Classification:** NOT TESTED

---

## 5. Known structural limitations (established from code and CI, not speculation)

These are **not** test outcomes; they are constraints the beta run operates inside. Each is
verifiable independently of the manual run.

1. **Provider Shared is unavailable by design.** `provider_capabilities()` returns
   `shared_available: false` for all four providers, with a fixed reason string. The stable path
   refuses anything that is not Provider Sync. No beta run can legitimately pass a Provider Shared
   test.
2. **CI cannot validate real decoding.** `src-tauri/mpv_runtime/*` is gitignored, so a clean
   checkout has no libmpv. CI therefore **skips five tests explicitly** and emits a `::warning`:
   `bundled_libmpv_plays_pauses_seeks_real_video`,
   `bundled_libmpv_renders_onto_real_calayer_through_production_player`,
   `bundled_libmpv_sw_render_api_produces_decoded_frames`,
   `bundled_libmpv_renders_onto_real_child_hwnd_through_production_player`,
   `bundled_runtime_is_loadable`. Since Batch 5 these tests **fail loudly** when the runtime is
   missing, so the skip is explicit rather than a false green.
   **Consequence: no automated gate anywhere has ever proven real decoded video frames on real
   hardware.** Only this manual run can.
3. **The diagnostic bundle cannot evidence drift.** It omits `positionMs` entirely (§2/E4).
4. **The Debug HUD is dev-gated.** Reachable only on a `localhost`/`127.0.0.1` origin with `?debug`.
   Sub-second drift measurement may therefore be unavailable in a packaged build.
5. **macOS hairpin/firewall caveat.** The real-Tailscale self-join integration test reports an
   explicit skip when the macOS application firewall blocks the ephemeral test binary's hairpin UDP.
   A first genuine incoming connection may raise the macOS "accept incoming network connections"
   prompt — **Allow** it. This is normal macOS behaviour.
6. **Two ignored tests.** The two real-Chrome CDP tests are `#[ignore]`d in the automated suite;
   they require a real Chrome. Any provider claim rests on this manual run, not on CI.

---

## 6. Sign-off matrix

**Nothing in Group A–F has been executed.** The matrix below records the true state at the time of
writing. Every row must be re-stamped with PASS / FAIL / BLOCKED / NOT TESTED plus an evidence
reference by the operator.

| # | Test | Automated evidence? | Manual status | Blocked by |
|---|---|---|---|---|
| T01 | Two physical devices | ✗ | **NOT TESTED** | requires 2 devices |
| T02 | Real Tailscale connectivity | ✗ | **NOT TESTED** | 2 devices + tailnet |
| T03 | Host creation | partial (loopback QUIC) | **NOT TESTED** | 2 devices |
| T04 | Guest joining | partial (loopback QUIC) | **NOT TESTED** | 2 devices |
| T05 | Real movie files | ✗ (fixture only) | **NOT TESTED** | 2 devices + real media |
| T06 | Video + audio playback | ✗ (**CI skips libmpv**) | **NOT TESTED** | libmpv + 2 devices |
| T07 | Synchronized start | ✗ | **NOT TESTED** | 2 devices |
| T08 | Countdown | ✗ | **NOT TESTED** | 2 devices |
| T09 | Pause / resume | ✗ | **NOT TESTED** | 2 devices |
| T10 | Repeated pause / resume | ✗ | **NOT TESTED** | 2 devices |
| T11 | Forward seek | ✗ | **NOT TESTED** | 2 devices |
| T12 | Backward seek | ✗ | **NOT TESTED** | 2 devices |
| T13 | Repeated seeks | ✗ | **NOT TESTED** | 2 devices |
| T14 | Long-duration playback | ✗ | **NOT TESTED** | 2 devices + long media |
| T15 | Buffering / degradation | ✗ | **NOT TESTED** | 2 devices |
| T16 | Disconnect / reconnect | ✗ | **NOT TESTED** | 2 devices |
| T17 | Late join / rejoin | ✗ | **NOT TESTED** | 2 devices |
| T18 | Guest leaving | ✗ | **NOT TESTED** | 2 devices |
| T19 | Host leaving | ✗ | **NOT TESTED** | 2 devices |
| T20 | Second movie | ✗ | **NOT TESTED** | 2 devices |
| T21 | Different media formats | ✗ | **NOT TESTED** | 2 devices + codec set |
| T22 | Large media files | ✗ | **NOT TESTED** | ≥4 GB file |
| T23 | A/V sync (per device) | ✗ | **NOT TESTED** | 2 devices + media |
| T24 | Observed playback drift | ✗ | **NOT TESTED** | 2 devices; ms needs dev HUD |
| T25 | Provider / Chrome playback | ✗ | **NOT TESTED** | accounts + DRM (**Shared BLOCKED by design**) |
| T26 | Camera / mic / call | ✗ | **NOT TESTED** | devices + OS permission |
| T27 | Tailscale online/offline | ✗ | **NOT TESTED** | 2 devices + tailnet |
| T28 | Error / recovery paths | ✗ | **NOT TESTED** | 2 devices |

### What is actually proven, and what is not

**Proven (by automated gates, on the candidate SHA):**
- The code compiles, is `rustfmt`-clean, and passes `clippy -D warnings` on the pinned 1.98.0
  toolchain.
- The Rust unit + integration suites pass on the CI runner (per-target counts in
  `BATCH7A_CI_VERIFICATION.md`).
- The frontend builds, lints, and its test suite passes.
- The Batch 1 sequence-reorder defect is **fixed in source** and its behaviour is covered by unit
  tests (`SequenceTracker` window semantics).

**Not proven — and not provable by any automated gate in this repo:**
- That two real devices on a real Tailscale network can form a party.
- That real movies play, with real video and real audio, on real hardware.
- That the two devices actually stay in sync, and by how much they drift.
- Anything about provider/DRM playback.
- Anything about camera/microphone capture.
- That the Batch 1 fix holds **on a real Windows device** — CI is a runner, not the user's machine.

**The single most important unproven claim: no one has yet watched a real movie synchronised across
two real devices with this codebase.**

---

## 7. Exact next action

1. **Execute T01–T02 first.** If two devices on a shared tailnet cannot be arranged, the entire
   matrix stays BLOCKED and the beta remains unproven — report that plainly rather than substituting
   a loopback or single-machine run.
2. **Record build provenance per device** (§1) before anything else, and keep both devices on the
   same SHA.
3. **Run T10 and T13 early.** They are the manual analogue of the Batch 1 defect and the most
   likely place for a real-world recurrence.
4. **Capture E1 continuously** for the whole session — the two-screen recording is the artefact that
   makes every other classification defensible.
5. **Report failures as findings, not as tests to retry.** A format that fails on Windows, or a
   drift figure that exceeds expectation, is the result.
6. **Do not modify Provider Shared**, and do not enable experimental capture to make T25 pass.

---

*Batch 7B — procedure only. No application code was modified. Provider Shared untouched.*
