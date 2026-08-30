# Movie Party V1 — Final Integration Runbook

## Purpose

The roadmap phases have already produced many individual modules, models,
policies, tests, and UI components.

This run is NOT for implementing more isolated abstractions.

This run exists to connect the existing implementation into real executable
end-to-end Movie Party functionality.

Do not redo completed roadmap work.

Do not perform general audits, security-hardening sweeps, telemetry expansion,
or speculative cleanup.

Work only through the gates below in order.

External verification may remain pending and must not block independent work.

---

# Gate 1 — Local Perfect Mode End to End

Produce a real host-to-guest vertical slice using production components.

Required flow:

Host chooses a real local media file
→ creates room
→ Guest joins
→ manifest exchanged
→ Guest requests real chunks
→ real file bytes travel through production QUIC transport
→ sparse cache receives chunks
→ cache-backed byte-range source serves media
→ real libmpv player consumes partial media
→ playback starts before complete transfer
→ strict synchronized play/pause works
→ Guest buffer starvation pauses Host
→ transfer catches up
→ both resume together
→ synchronized seek works
→ reconnect resumes transfer/session
→ retention flow works.

Requirements:

- use real production transport and protocol;
- use real file bytes;
- use real player integration;
- no fake alternate protocol;
- no screen sharing;
- no test-only production path.

A localhost two-instance development mode is allowed and encouraged.
It must reuse production networking/sync/media code.

If libmpv is unavailable locally, install/configure the normal development
dependency if permitted. Do not replace libmpv with another architecture.

Gate 1 complete when this full path works locally or when the only remaining
failure requires unavailable external hardware/runtime dependency.

---

# Gate 2 — Application and UI Integration

Connect the existing frontend to real Rust/backend state.

Production UI must obtain real:

- room state;
- participant state;
- media metadata;
- transfer progress;
- buffering state;
- synchronization state;
- network state;
- chat;
- reactions;
- call state;
- provider state.

Required user flow:

Home
→ choose media
→ create/join party
→ lobby
→ readiness
→ countdown
→ cinema
→ buffering/recovery
→ leave/end.

Remove hardcoded/demo production values such as fake participant names,
fake movies, fake network values, and fake connection status.

Connect host controls and Shared Controls to real canonical room operations.

Connect Chat and Reactions to the real peer protocol.

Ghost Mode and Privacy Mode must operate on real call/UI state.

Do not redesign the existing UI.

---

# Gate 3 — Real Call Path

Convert existing call models/helpers into an actual peer-media path.

Required:

- real RTCPeerConnection or selected production implementation;
- signalling through Movie Party;
- offer;
- answer;
- ICE;
- microphone track;
- optional camera track;
- microphone muted initially;
- Video + Voice;
- Voice Only;
- Off;
- camera toggle;
- microphone toggle;
- Privacy Mode disables camera/microphone;
- adaptive camera policy wired to actual sender parameters.

Use two local application instances and synthetic media where needed for
development.

Physical remote-device verification may remain external pending.

No paid TURN service.

---

# Gate 4 — Real Managed Chrome and Provider Sync

Existing provider command/model code must be connected to real Chrome/CDP.

Required real local path:

Movie Party
→ locate installed Chrome
→ launch dedicated non-default provider profile
→ connect to real localhost CDP
→ find correct target
→ navigate
→ execute JavaScript
→ read results
→ reconnect/close safely.

First prove with YouTube.

YouTube should exercise:

- URL recognition;
- media identification;
- playback position;
- play;
- pause;
- seek;
- buffering/player state;
- synchronization.

Then ensure Netflix, Prime Video, and JioHotstar adapters have production
paths for:

- login-required detection;
- media detection;
- play;
- pause;
- seek;
- position;
- buffering where available;
- provider errors;
- strict-sync integration.

Do not ask for or store provider credentials.

Real subscription-provider verification may remain:

EXTERNAL PROVIDER VERIFICATION PENDING.

---

# Gate 5 — Real Provider Shared Generic Pipeline

Convert existing shared-mode models into a functional media pipeline.

On the available platform implement a real non-DRM proof:

real capturable window
→ OS capture API
→ actual video frames
→ real H.264 encoder
→ real encoded samples
→ QUIC media transport
→ receive/presentation buffer
→ real decode
→ Host Movie Party presentation
→ Guest Movie Party presentation.

On macOS use actual ScreenCaptureKit + VideoToolbox.

Implement Windows code behind platform-specific boundaries according to the
locked architecture; Windows runtime proof can remain external.

Also connect:

- shared-mode intentional presentation buffer;
- real buffer measurements;
- strict source/Host/Guest pause when Guest falls behind;
- synchronized resume;
- automatic bitrate quality policy;
- camera degradation before movie degradation.

Use a normal non-protected window or YouTube to prove the generic pipeline.

Do not claim Netflix/Prime/JioHotstar protected capture works unless actually
verified.

Do not circumvent DRM.

Do not implement vBrowser unless a real provider capture experiment later
proves it necessary.

---

# Gate 6 — Runtime Resilience and Final Local Validation

Connect existing recovery policies to real runtime events.

Verify locally where possible:

- peer disconnect;
- reconnect;
- transfer interruption;
- transfer resume;
- corrupted chunk;
- missing media;
- player failure;
- Chrome exit;
- provider target closing;
- network interruption.

Then run one final local validation set:

Frontend:
- lint
- typecheck
- tests
- build

Rust:
- fmt
- clippy
- tests
- build

Run all integration tests created by Gates 1–6.

Do one final Tauri local build/smoke test if practical.

---

# External Verification

These may remain pending without blocking local implementation:

- real Windows execution;
- Mac ↔ Windows physical testing;
- college Wi-Fi certification;
- real Tailscale path/throughput between physical machines;
- remote friend's camera/microphone;
- Netflix authenticated playback;
- Prime authenticated playback;
- JioHotstar authenticated playback;
- protected-content capture;
- real personal movie-night beta.

Never fabricate these results.

---

# Execution Rules

1. Work through Gates 1 → 6.
2. Do not stop after a Gate.
3. A Gate completion is a checkpoint, not a stopping point.
4. Perform focused tests during implementation.
5. Run broad regression only after Gates 1, 3, 4, 5, and final Gate 6.
6. Update IMPLEMENTATION_TRACKER.md only after a Gate completes or a real
   blocker occurs.
7. Do not append repetitive minute-by-minute session logs.
8. Prefer one coherent commit per Gate, not one commit per tiny fix.
9. Do not search for unrelated improvements.
10. Do not perform another general repository audit.
11. Do not add more telemetry/certification models unless required by an
    integration Gate.
12. Do not expand scope into Phase 32 release hardening.
13. Do not mark a model/helper as complete integration without exercising it
    in the real data path.
14. If context compacts, resume from the current Gate recorded below rather
    than repeating intake.

---

# Active Integration State

Current Gate: COMPLETE
Last Completed Gate: 6
Blocker: External verification pending: libmpv runtime install, Windows execution, physical cross-device/Tailscale, subscription provider auth/capture.
Next Action: Stop integration run; do not search for additional gaps.

Update only these four lines while executing.

---

# Completion Condition

This run is complete when Gates 1–6 have been implemented as far as the
current machine permits, all locally executable integration checks pass, and
all unavailable external verification is explicitly listed.

When that condition is reached:

STOP.

Do not inspect the repository for more improvements.
