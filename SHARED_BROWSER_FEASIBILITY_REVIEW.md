# Shared browser — feasibility and security gate review (optional experimental batch)

Date: 2026-10-26. Branch: `codex/integrated-youtube-player`.

**Decision: not started, and nothing was built, flagged, or enabled.**

The batch's own precondition is not met — *"Do this ONLY after … the physical
Provider Sync beta have passed"* — and two of its security gates cannot pass for
the content that matters. This document records the gate status, the feasibility
research that *can* be done without a prototype, the benchmark procedure that
would have to run first, and the honest answer to requirement 14.

## 1. Precondition status

| Precondition | Required by | State on this date | Evidence |
| --- | --- | --- | --- |
| Physical Provider Sync beta passed | Batch header | **Not met** | `docs/PROVIDER_VERIFICATION_CHECKLIST.md`: S1–S16 are `BLOCKED` — no authorized Netflix/Prime/JioHotstar session exists on this machine, and no two-device run has ever happened |
| Two-device rows passed | implied by "physical" | **Not met** | Checklist rows O17–O29, P5, C12 remain unrun |
| Provider Shared disabled in the stable build | requirement 11 | **Holds today, and is pinned by a test** | `ProviderCapability.shared_available == false` for every provider, asserted by `capabilities_expose_sync_without_claiming_shared_readiness` (`src-tauri/src/providers/sync.rs`); the app has no capture pipeline — only diagnostic plans |

Because the first row is unmet, requirement 12 ("produce an isolated prototype
only if feasibility and security gates pass") forbids building one. There is
therefore **no feature flag, no capture code, and no change to `shared_available`**.

## 2. Feasibility of host-authorised per-window capture

### 2.1 macOS

| Question | Answer | Source |
| --- | --- | --- |
| API | ScreenCaptureKit (macOS 12.3+), which "gives you fine-grained control to select and stream only the content you want to capture" | Apple, ScreenCaptureKit overview |
| Per-window selection | `SCContentFilter` with `SCWindow` / `SCRunningApplication`; `SCShareableContent` enumerates what may be captured | Apple, ScreenCaptureKit topics |
| Native consent UI | `SCContentSharingPicker` — "the system screen-sharing control, as the recommended approach for letting people select content sources and manage active streams" | Apple, same page |
| Permission | Screen-recording TCC, declared with `NSScreenCaptureUsageDescription` | Apple, same page |
| Audio | Application audio is captured through SCK as well | Apple framework surface; repo's own plan names "ScreenCaptureKit Application Audio" |
| Existing groundwork | `src-tauri/src/capture/macos/sck_probe.swift` (a real SCK frame probe) and `diagnostic_plan()` naming SCK + VideoToolbox H.264 + SCK application audio | this repository |

Consequence: on macOS the *capture* half is feasible with the OS's own picker as
the consent surface, and the repo has already done a probe.

### 2.2 Windows

| Question | Answer | Source |
| --- | --- | --- |
| API | Windows.Graphics.Capture, per-window item; WASAPI application loopback for audio | repo diagnostic plan; Microsoft WGC |
| Protected/opted-out windows | `WDA_EXCLUDEFROMCAPTURE` (Windows 10 2004+): "the window is displayed only on a monitor. Everywhere else, the window does not appear at all." On older versions it degrades to `WDA_MONITOR` (content hidden everywhere except the monitor) | Microsoft, `SetWindowDisplayAffinity` |
| Is that DRM? | No — Microsoft states the affinity APIs are "not a security feature or an implementation of Digital Rights Management (DRM)" | same page |

### 2.3 The part that is *not* feasible inside this app's current architecture

The app is a Tauri webview (WKWebView on macOS, WebView2 on Windows). A browser
`getDisplayMedia` call inside that webview cannot be relied on to capture another
application's window on macOS — the supported route there is native
ScreenCaptureKit. So a prototype would need a native capture → encode → transport
pipeline, not a handful of lines in the frontend. Both OS paths would also need
their own permission UX *in addition to* the app's own consent.

## 3. Protected content (requirements 5–7)

This is the finding that decides the feature's scope:

- Subscription providers (Netflix, Prime Video, JioHotstar, and YouTube's paid
  content) deliver video through platform DRM. Those surfaces are built so that
  screen capture yields black or empty frames — that is the protection working as
  designed, on both macOS and Windows.
- The OS-level opt-outs confirm the model: Windows lets a window remove itself
  from capture entirely, and Microsoft explicitly does **not** present that as a
  DRM substitute because real DRM does not depend on it.
- Requirement 7 ("do not capture or transmit DRM-protected video") therefore rules
  out exactly the content Movie Party's provider work exists for. A prototype
  could only legally and technically cover **unprotected, shareable** content.

The honest consequence: **Provider Shared cannot be made to work for the
subscription providers by any implementation of this feature.** If a capture
attempt returns blank frames, the only correct response is to stop and say so —
never to retry, adjust, or otherwise try to defeat the protection.

## 4. Benchmark plan (requirement 2) — specified, not run

Requirement 2 asks for measurements on a ~10 Mbps connection. No prototype exists,
so no measurement was taken; publishing numbers as if measured would be the
failure mode this project keeps guard-railing against. The plan, to run only after
the gate passes:

1. 30-second captures of a 1080p and a 720p **unprotected** video in a window, at
   30 and 60 fps, encoded with hardware H.264 (VideoToolbox / Media Foundation).
2. Measure: encode latency (p50/p95), frames dropped, host CPU and RSS, encoder
   session memory, and audio/video drift over 10 minutes.
3. Apply a shaped 10 Mbps / 40 ms RTT link (`pfctl`/`dnctl` on macOS, `clumsy` or
   `tc` on Windows), and record guest-side end-to-end latency (capture timestamp →
   guest display), rebuffer events, and audio sync error.
4. Compare against the same session using synchronized independent playback, same
   titles, same link.

Budget estimates to sanity-check the plan (from published codec guidance, **not**
measurements): hardware H.264 1080p30 typically lands at 4.5–6 Mbps, 720p30 at
2.5–4 Mbps, 1080p60 at 6–9 Mbps; Opus audio adds 96–128 kbps. On a 10 Mbps link,
1080p30 fits but leaves little headroom on the guest's downlink. Expected latency
budget: capture 16–33 ms + encode 10–30 ms + jitter buffer 50–200 ms + network RTT
+ decode 10–20 ms + compositor — roughly **150–400 ms**, versus a control-plane
RTT of a few tens of milliseconds for synchronized playback.

## 5. Reuse assessment (requirement 3)

| Component | Reusable? | Why |
| --- | --- | --- |
| QUIC room transport (control, chat, call signalling) | Yes, unchanged | Already carries every flow on the guest-initiated connection |
| `src/call/webrtc.ts` peer connection | Partly | It moves `MediaStreamTrack`s, but its tracks come from `getUserMedia` (camera/mic). A screen track would need native capture feeding it, and the encoder would then be the browser's, not VideoToolbox/Media Foundation directly |
| Local file streaming path | Yes, as a model | It already pushes host-owned media over the room connection; a captured stream has the same shape but very different timing |
| Companion / managed-Chrome lease | Yes, as the window-identity model | It already answers "which window is the intended one" for providers |

Reuse is real but does not make the feature small: the new work is native capture,
hardware encode, a rate-controlled stream, sync with the existing clock, and an
entire consent/indicator surface.

## 6. Security and consent gates (requirements 4–9, 13)

| Gate | Can it pass on current evidence? | Notes |
| --- | --- | --- |
| G1. Explicit, in-app consent before capture | Yes, by design | Two consents: the OS picker (SCContentSharingPicker / WGC) and an in-app confirmation naming the window |
| G2. Visible sharing indicator for both people | Yes, by design | Persistent host banner + guest-side "host is sharing" state; must be present for the whole stream |
| G3. Capture restricted to the selected window | Yes | `SCContentFilter` / WGC window item; no display-wide mode, no window list exposed to the guest |
| G4. OS and browser protected-content restrictions respected | Yes only by *not capturing DRM* | Blank-frame detection stops the stream; no retry, no workaround |
| G5. No DRM-protected video captured or transmitted | **Cannot pass for subscription providers** | Their content is DRM by definition; requirement 7 limits the feature to unprotected content |
| G6. No password dialogs or private-browsing data | **Cannot be guaranteed** | Window-scoped capture excludes other windows, but a sign-in form *inside* the shared window, or a private window chosen by the host, is in frame. Private mode is not reliably detectable across browsers. Mitigation is procedural (share only after playback starts, host confirms what is on screen) and leaves residual risk |
| G7. Remote control off by default | Yes, already true | `shared_controls: false` at runtime; a capture prototype must not flip it |
| G8. Explicit experimental flag | Would be required | Existing precedent: `MOVIE_PARTY_DEV_LOOPBACK`, `MOVIE_PARTY_ALLOW_VISIBLE_CHROME`; nothing was added |
| G9. Provider Shared stays disabled in stable | Holds, and a test enforces it | `capabilities_expose_sync_without_claiming_shared_readiness` fails if any provider claims `shared_available`; untouched by this review |

G5 and G6 are the reasons requirement 12's gate cannot pass as written for
provider content. G6 means even an unprotected-content prototype would ship with a
documented residual risk, which is only acceptable behind an explicit experimental
flag and with copy that says so.

## 7. Requirement 14 — does it improve the app?

No evidence that it does, and several reasons to think it would not:

| Dimension | Shared capture (prototype) | Synchronized independent playback (shipped) |
| --- | --- | --- |
| Subscription providers | Impossible (DRM) | Works today |
| Bandwidth | 2.5–6 Mbps of re-encoded video | A few KB/s of control |
| Quality | Encoded, resolution- and bitrate-capped | Source-native on each device |
| Latency | ~150–400 ms plus network | Control-plane RTT only |
| Pause/seek | Waits on capture and encode | Immediate; both sides observe the real player |
| Failure modes | Capture stalls, blank DRM frames, permission loss | Entitlement/region mismatch (visible and honest) |
| Privacy surface | Entire new class of risk (screen, dialogs, private windows) | None beyond tailnet traffic |
| Entitlement mismatch — capture's only real advantage | Guest sees host's copy | Fails honestly and asks for another title |

The single case capture wins — the guest cannot legitimately obtain the title —
is the case the product cannot verify, and for subscription services it is a terms
problem, not a technical one. The current design (both people play the same title
on their own accounts, synchronized) is faster, higher quality, works with DRM, and
carries none of the privacy surface.

**Recommendation: do not build the prototype.** Keep Provider Shared disabled.
Revisit only if all of the following become true:

1. the physical Provider Sync beta passes (so the comparison is measurable), and
2. a concrete, legally shareable content class appears that synchronized playback
   cannot serve, and
3. a reliable way exists to exclude private-browsing windows and in-window
   credential forms — or the product accepts that risk explicitly in copy.

## 8. What was and was not done in this batch

| Item | Status |
| --- | --- |
| Official OS API review | Done (§2, with sources) |
| Repo groundwork review | Done — SCK probe + diagnostic plans exist; no capture pipeline |
| Capability/consent gate analysis | Done (§6) |
| Benchmark | **Not run** — no prototype, precondition unmet; procedure specified (§4) |
| Prototype | **Not produced** — requirement 12's gate fails |
| Feature flag | **Not added** |
| `shared_available` change | **None** |
| Stable release promotion | **None** |
