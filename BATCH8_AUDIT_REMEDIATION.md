# Batch 8 — Deep-Audit Remediation

**Scope:** remediate the verified P0/P1/P2 findings from `DEEP_PRODUCTION_READINESS_AUDIT.md`.
**Method:** re-open the source, re-prove each finding, confirm it is not already fixed, confirm severity,
apply the smallest safe fix, add a regression test, add a negative control, then run the gates.
**Date:** 2026-09-20
**Baseline HEAD at start:** `e19c2eb` (branch `stabilization/v0.9.9-rc1`)
**Commit produced:** `f280b6c` — *local only, not pushed*

---

## 0. Baseline, before anything changed

| Fact | Value |
|---|---|
| Branch | `stabilization/v0.9.9-rc1` |
| HEAD at start | `e19c2eb729de8a2267d2110c4b31c2a1b8592bd6` |
| Working tree | clean |
| `main` (local) | `2d5c833` |
| `origin/main` | `bb22577` — **never pushed** |
| `v0.9.8` (commit) | `bb22577` — unchanged |
| `origin/stabilization/v0.9.9-rc1` | `a44542a` — the last CI-verified SHA |
| Version declarations | all four `0.9.8` |
| Provider Shared | `shared_available: false`, by design |

The audit was performed against the stabilized candidate (`e19c2eb`), not the `v0.9.8` tag.
Nothing frozen was touched in this batch: `main` is not pushed, `v0.9.8` is not retagged, the
v0.9.8 GitHub Release is not modified, and no `v0.9.9` was created.

---

## 1. Finding-by-finding verification table

| ID | Class | Re-proved? | Already fixed? | Verdict after this batch | Fix | Test |
|---|---|---|---|---|---|---|
| **AUD-01** | **P0** | Yes — reproduced | No | **FIXED (source)**, runtime-verify outstanding | Derive canonical position from the PLAY commit anchor | `guest_in_sync_is_never_seeked_by_the_player_event_loop` |
| **AUD-02** | **P1** | Yes | No | **FIXED** | Remove the dead `host_relay_ready_state`; anchor redefined | covered by AUD-01 test |
| **AUD-03** | **P1** | Yes | No | **FIXED (source)**, `eof-reached` runtime-verify outstanding | `PlayerState::Completed` + `keep-open` + host `ended()` | `host_reaching_end_of_media_ends_playback` |
| **AUD-04** | P2 | Yes | No | **FIXED** | Decision extracted to `drift_correction_for_player` | AUD-01 test drives the real loop |
| **AUD-05** | P2 | Yes | No | **FIXED** | Tests now exercise the call site, not the helper | AUD-01 + control |
| **AUD-06** | P2 | Yes | No | **PARTIAL** — code fixed, end-to-end unverified | Explicit `audio`/`aid`/`ao` options | `player_requests_audio_output_explicitly` |
| **AUD-07** | P2 | Yes | No | **FIXED (test-only)** | Silent `return` → explicit panic; assert the seek moved the playhead | the test itself |
| **AUD-08** | P2 | Yes — and **worse than reported** | No | **NOT REMEDIATED** — deliberately | Staleness recorded loudly; deletion deferred to owner | none |
| **AUD-09** | P2 | Yes | No | **FIXED** | `version-consistency` CI job | the job itself |
| AUD-10 | P3 | Yes | No | Unchanged — out of scope | — | — |
| AUD-11 | P3 | Yes | No | Unchanged — out of scope | — | — |
| AUD-12 | P3 | Yes | No | Unchanged — out of scope | — | — |
| AUD-13 | INFO | Yes | No | Unchanged — documentation matter | — | — |
| AUD-14 | INFO | Yes | No | Unchanged — deliberate and documented | — | — |
| **AUD-15** | **P3 (new)** | Yes | No | **NEW — found while fixing AUD-01**; not changed | recommended, not applied — see §6 | — |

---

## 2. Fixes implemented

### AUD-01 (P0) — the frozen drift anchor

**Re-proof.** `app_runtime.rs` computed guest drift as
`snap.position_ms − state.sync.position_ms`. The left side advances (mpv `time-pos`); the right side
was written only by `PlayCommit`/`PauseCommit`/`SeekCommit`/`BufferLow` and a one-shot
`if position_ms == 0` guard. So "drift" was really *elapsed time since the last commit*, growing
~1000 ms/s, crossing the 250 ms `MicroSeek` and 700 ms `HardSeek` bands on every poll.

**The fix — and why it is the smallest one.** The audit offered two options: wire a periodic position
broadcast, or redefine the drift anchor. The second is smaller *and* better, because the protocol
**already carried everything needed**: a PLAY commit names both a `target_position_ms` *and* the
host-monotonic instant it takes effect (`execute_at_host_mono_us`). From that instant the canonical
position advances at 1×. The runtime simply never used the second half of that pair.

So no new channel was added. `AppRuntimeState` gained one field (`committed_playback`), recorded at
the two commit sites that already existed, and two helpers:

- `host_monotonic_us(state)` — this process's monotonic clock mapped onto the host's base via the
  calibrated `clock_offset_to_host_us` (the inverse of the existing `instant_for_host_mono`).
- `projected_host_position_ms(state)` — the canonical position now, or `None` when the room is not
  `Playing`. Guarding on `Playing` means pause/buffer/seek need no extra bookkeeping.

A periodic broadcast was rejected on a concrete correctness ground, not taste: a late or replayed
position message would **yank the anchor backwards**, which is the same class of bug. A locally
derived projection cannot be reordered or duplicated.

### AUD-02 (P1) — the dead position channel

`host_relay_ready_state` was the only production sender of `QuicServerEvent::RoomStateUpdate`, and it
was `#[allow(dead_code)]` with no caller — uncalled since the initial snapshot. It was the root cause
of AUD-01. **Removed**, with the reasoning recorded in place so the next reader does not re-add it.
`RoomStateUpdate` is still *received* for wire compatibility; nothing sends it.

### AUD-03 (P1) — end of media

Three coordinated parts:

1. **`PlayerState::Completed`** added — a terminal state distinct from `Stopped` (no media) and
   `Paused` (playback may continue).
2. **`keep-open=yes`** in `player_init_options()`. Without it mpv unloads the file at EOF,
   `time-pos`/`duration` stop resolving, and "the film finished" is indistinguishable from "the
   player broke" — there is nothing stable left to detect. With it, `eof-reached` stays set and the
   final frame remains on screen.
3. **`snapshot()` sets `Completed` from `eof-reached`**, and pins the position to the duration so the
   clock reads the end rather than stalling at whatever the last poll saw.

On the runtime side, `apply_end_of_media` fires **only on the host** and only from `Playing`:
a guest must never unilaterally end a room the host may still be playing, or a shorter/truncated
local copy would end the film early for both. The host transitions the coordinator via a new
`LocalSyncCoordinator::ended()`, which broadcasts, so the guest follows and neither side keeps
correcting drift against a position that can no longer advance.

**A gap I found in my own fix and closed:** `sync.room_state` is a *copy* taken by
`sync_room_snapshot`. The first version set `state.room_state` but not the copy, so the frontend
would have kept reading `PLAYING` while the room had ended — the transition landing half-applied.
The fix now calls `sync_room_snapshot`, and the test asserts the frontend-visible field.

### AUD-04 / AUD-05 (P2) — the tests could not fail

The old drift tests called `apply_drift_correction` **directly with hand-picked literals**. They
verified the threshold mapping; the production defect was in the *argument*. The decision now lives in
`drift_correction_for_player(state, local_position_ms)`, so the thing under test is the call site.

### AUD-06 (P2) — audio

The init options were extracted into `player_init_options()`, a pure function, so the configuration is
assertable with no libmpv handle. It now sets `audio=auto`, `aid=auto`, `ao=auto` explicitly.

**Be clear about what this is.** These are mpv's documented defaults: this is the *decision made
visible*, not a behaviour change. It proves the player asks for audio. It does not prove a sound is
produced — see §6.

### AUD-07 (P2) — a test that could pass while proving nothing

The provider test printed a message and `return`ed when no HTML5 player was detected, and libtest
reports an early return as `ok`. An explicitly-requested run could therefore report success without
verifying anything. It now **panics** with an explanation that the failure is of the verification, not
the product. It also asserts the seek **moved the playhead** (position near 1.0 s afterwards) instead
of only that CDP returned `true` — the old assertions could not tell a working seek from a no-op.

### AUD-09 (P2) — version consistency

A `version-consistency` CI job asserts that `package.json`, `src-tauri/tauri.conf.json`,
`src-tauri/Cargo.toml` and `Cargo.lock` all declare the same version. Verified locally against the
real files before committing; it reports all four as `0.9.8`.

### AUD-08 (P2) — not remediated, deliberately

The audit said `shared_pipeline.rs` "will rot silently". It has already rotted. Declaring it fails in
**three** places:

- `QuicServer::run()` now requires `Option<Arc<Mutex<LocalSyncCoordinator>>>` (line ~131)
- `QuicClient::connect(..)`'s argument list changed (line ~132)
- `QuicClient::send_shared_stream_packet` no longer exists (line ~141)

Repairing it is Provider Shared implementation work, which this batch excludes. Deleting it is the
owner's call and I did not delete it. What I did instead: record the staleness **loudly** in the file
header and in `media/mod.rs`, naming the three broken call sites, so nobody reads it as a shipped
capability. Its only test is `#[ignore]`d and can never run.

---

## 3. Tests added

Six, verified by diffing `#[test]`/`#[tokio::test]` attributes against HEAD (`+6`, exactly):

| Test | Proves |
|---|---|
| `guest_in_sync_is_never_seeked_by_the_player_event_loop` | An in-sync guest is **never seeked**, and the canonical position advances at **1×** (window 1000–2000 ms over a 1.4 s run). Drives the real `spawn_player_event_loop`. |
| `negative_control_frozen_anchor_reports_elapsed_time_as_drift` | The projection is load-bearing: without it, elapsed-since-commit *is* reported as drift and lands in the hard-seek band. |
| `host_reaching_end_of_media_ends_playback` | The host reaching EOF leaves `Playing`, stops drift correction, and the **frontend-visible** `sync.room_state` reads `ENDED`. |
| `negative_control_end_of_media_requires_a_playing_host` | Three ways the transition must **not** fire: a guest reporting `Completed`, a paused room, a merely-paused player. |
| `completed_player_state_has_a_distinct_wire_name` | `Completed` → `"COMPLETED"`, and is not confused with `PLAYER_ERROR`. |
| `player_requests_audio_output_explicitly` | `audio`/`aid`/`ao` are set explicitly, and `keep-open` (which AUD-03 depends on) is present. |

Test doubles added: `AdvancingPlayer` (a player whose position genuinely advances with wall time —
`ScriptedPlayer` is static, which is *precisely* the environment in which AUD-01 was invisible),
plus `guest_playing_runtime` and `install_play_anchor` helpers.

---

## 4. Negative controls

**The regression was proven to fail before it was trusted.** With `projected_host_position_ms`
temporarily forced to return `None` — reproducing the pre-fix frozen anchor — the regression test went
red with exactly the defect's signature:

```
an in-sync guest must never be seeked; the loop seeked to [1000000, 1000000, 1000000].
A seek back to the commit target means the canonical position was compared as a frozen anchor
rather than a projected one (AUD-01).
```

Three hard-seeks back to the commit target in 1.4 s. That is AUD-01, reproduced on demand.

The mutation was reverted and verified **by hash** — `app_runtime.rs` restored byte-identical to its
pre-mutation backup (`b2fffa1b289c9b83bde124e9ef9c29ff3b2d2ac0581fbff81924c92cd39313c8`), with the
mutation marker confirmed absent — and the test passes again. (The file was edited once more after
that check, to strengthen two assertions, so that hash describes the reverted state, not the final
committed file. The committed file is `f280b6c`.)

The control was designed to disable the **whole** mechanism rather than one guard: the projection is
the single thing the fix adds, and removing it removes both assertions at once (the seek assertion
*and* the 1×-advance assertion). There is no second mechanism left holding the test green.

The two other controls are in-suite and permanent: the frozen-anchor control (drives the same call
site with no anchor) and the end-of-media control (three cases that must not transition).

---

## 5. Full validation results

| Gate | Result |
|---|---|
| `cargo fmt --check` | **clean** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **exit 0, zero warnings** |
| `cargo test` (all targets, CI's skip list) | **568 passed / 0 failed / 2 ignored** across 15 targets |
| `eslint . --max-warnings=0` | **exit 0**, zero output |
| `tsc --noEmit` | **exit 0** |
| `vite build` | **exit 0** (12.79 s; pre-existing chunk-size advisory) |
| `vitest run` | **287 passed / 23 files** |

**The Rust totals reconcile exactly.** The last CI-verified macOS total was **562**. This run is
**568** — the same 562 plus the 6 new tests. Per-target: lib 484 (one filtered by the CI skip list),
dep_audit 3, host_guest 2, **m2_integration 28**, m3_closure 7, m3_integration 18, m3_m4_e2e 9,
m4_closure 15, tailscale 2; `real_native`/`real_playback`/`real_sw_render`/`windows_native`/doc-tests
each 0. All 15 result lines read `0 failed`.

**Two environment notes, stated rather than glossed:**

- `pnpm lint` / `pnpm test` cannot run directly: pnpm's dependency pre-check tries to create a
  symlink in `/Volumes/T7 Shield/.pnpm-store`, which the sandbox denies (`EEXIST`). I ran the
  underlying binaries (`node_modules/.bin/eslint`, `vitest`, `tsc`, `vite`) instead. Same tools,
  same config.
- The `cargo test` shell exits non-zero because a test touches the macOS keychain, which the sandbox
  blocks. `CARGO TEST EXIT: 0` and all 15 `test result:` lines are `ok`. Unrelated to these changes.

**No unrelated behaviour changed.** `dist/` is gitignored, so the build dirtied nothing tracked.
The diff is 8 files + the audit report: `app_runtime.rs`, `media/mod.rs`,
`media/player/mod.rs`, `media/player/mpv_backend.rs`, `media/shared_pipeline.rs` (comments only),
`providers/chrome/mod.rs` (test only), `sync/local.rs`, `.github/workflows/ci.yml`.

---

## 6. Remaining findings

### AUD-08 — NOT remediated (P2, open)
`shared_pipeline.rs` is stale and uncompiled. Repairing it is Shared work (out of scope); deleting it
is the owner's call. Now recorded loudly in the file and `media/mod.rs` so it cannot mislead.

### AUD-06 — PARTIAL (P2)
The code decision is now explicit and pinned by a test. **End-to-end audio remains unverified.**
There is still no audio-bearing fixture in the repository: I confirmed by direct MP4 box parsing that
`movie_party_test_320x240.mp4` has **one video track and no audio track at all** (no `soun` handler,
no `mp4a`/`ac-3`). I could not create one — `ffmpeg`/`ffprobe` are not installed here and the sandbox
blocks package installs. A-V sync is a core part of "watch a movie together" and **nothing in this
repository has ever produced a sound.**

### AUD-15 — NEW, found while fixing AUD-01 (P3, not changed)
`clock_calibrated` (`app_runtime.rs:620`) is **written but never read** — dead state. It is set
together with `clock_offset_to_host_us` in both calibration paths, and read nowhere.

Why it now matters: AUD-01's fix makes drift correction **continuously** dependent on the calibrated
offset, where before the offset only affected commit timing. If calibration never succeeds (fewer than
4 of 20 probes return), the offset stays `0` and the guest's projection uses its own monotonic base as
if it were the host's — an arbitrary error that would seek the guest wildly.

I did **not** change this, because it is beyond the audit's scope and the realistic likelihood is low
(the calibration loop retries until the client is gone, and getting 4 of 20 probes on a LAN is
near-certain). It is also not a *new* failure mode: an uncalibrated offset already breaks the commit
deadline. The one-line guard, if you want it, is to return `None` from `drift_correction_for_player`
when `!state.clock_calibrated` — which would also make the dead flag meaningful. Say the word and I
will apply it with a test.

### AUD-10..AUD-14 — unchanged (P3/INFO)
Untouched, as instructed. AUD-10 (five never-invoked commands), AUD-11 (`position_ms == 0` heuristic),
AUD-12 (`buffered_ahead_ms()` always `None` for mpv), AUD-13 (unsigned/un-notarized macOS builds, no
updater — a documentation requirement, not a defect), AUD-14 (the `real_*` targets execute 0 tests).

---

## 7. Unverified Windows / platform items

**This is the largest remaining gap, and it is mine, not the audit's.**

`f280b6c` **has not been through CI.** It is a local commit on `stabilization/v0.9.9-rc1`; the remote
tip is still `a44542a`, which is the SHA CI actually verified. So:

| Item | Status |
|---|---|
| Rust on macOS | **Locally verified** — 568 passed / 0 failed |
| Rust on Windows | **NOT verified.** No CI run against `f280b6c`. |
| Windows clippy (`-D warnings`) | **NOT verified.** |
| Frontend | Locally verified (287 tests, lint/tsc/build clean) |
| `version-consistency` job | Logic verified locally; the job itself has never executed |

This matters concretely: Batch 7A's failure was a Windows-only clippy error that macOS could not see,
and it happened *before* the test step, so Windows ran no tests at all. I cannot cross-compile to
`x86_64-pc-windows-msvc` here (it dies in `ring`/`aws-lc-sys` build scripts with no Windows C
toolchain). Reviewing my changes, none is platform-conditional — `player_init_options` and
`PlayerState::Completed` are neutral, `mpv_get_flag` mirrors the existing `mpv_get_double` FFI pattern,
and the new tests use no platform-specific API — but **that is a code-reading argument, not a
verification.** Windows must be re-run before this can be called verified.

**Nothing was pushed.** Pushing the candidate branch is what would let CI run, and that is an
externally visible action, so it waits for your approval.

---

## 8. Manual validation requirements

| # | Requirement | Why it cannot be automated here |
|---|---|---|
| M1 | **Two real devices, one real movie, watch it to the end.** | The only thing that confirms AUD-01's fix. My evidence is a scripted player driving the real loop; nobody has watched a real decoded frame stay in sync. |
| M2 | **A movie with an audio track**, confirming sound and A-V sync. | No audio-bearing fixture exists and none can be built here. |
| M3 | **A 2-hour (ideally 8-hour) session**, watching drift. | The projection depends on the continuously re-calibrated clock offset. Whether the offset stays accurate enough over hours is unverified. |
| M4 | **End-of-media on both sides** — host and guest both leave `PLAYING`; seek back and resume works. | The `eof-reached` → `Completed` step needs a real libmpv runtime. |
| M5 | **Pause / seek / buffering / reconnect** during playback on both sides. | Needs two devices. |
| M6 | **Windows build**, at minimum the CI run; ideally a real Windows machine. | No Windows toolchain here. |

Batches 7B's `BATCH7B_REAL_BETA_VALIDATION.md` (28 areas, T01–T28) remains the procedure for M1–M5.
**Every row in it is still NOT TESTED.**

---

## 9. Commit SHA

| | SHA | Pushed? |
|---|---|---|
| Baseline HEAD (audit target) | `e19c2eb` | yes (candidate branch) |
| **This batch — code** | **`f280b6c`** | **no — local only** |
| This batch — this report | `4d124cd` | no — local only |
| Last CI-verified SHA | `a44542a` | yes — still the remote tip |

`f280b6c` is one commit: 9 files, +1635 / −57, and it includes
`DEEP_PRODUCTION_READINESS_AUDIT.md`. `4d124cd` adds only this report.
**`f280b6c` is the SHA that matters for verification** — it is the code state.

Unchanged and confirmed after the commit: `main` = `2d5c833` (local) / `bb22577` (remote), all five
tags, `v0.9.8` → `bb22577`, and no release created or modified.

---

## 10. Is the code ready for the final regression audit?

**Yes — with one condition that is not mine to meet.**

No **P0 or P1 remains as an open code defect.** AUD-01 (P0), AUD-02 and AUD-03 (P1) are fixed and
regression-tested, and AUD-01's fix was proven non-vacuous by reproducing the original defect on
demand. So the "STOP before release preparation if any P0/P1 remains" condition is not triggered.

The condition is this: **`f280b6c` is not CI-verified, and Windows has never run it.** Given that
Batch 7A's one and only failure was a Windows-only clippy error invisible on macOS, calling this
"ready for final regression audit" while Windows is unrun would repeat the exact mistake the audit
was written to stop. The honest sequence is:

1. **Approve the push** of `stabilization/v0.9.9-rc1` → CI runs on `f280b6c` (both platforms).
2. If Windows is green, the final regression audit can begin against that SHA.
3. M1–M3 (two real devices, audio, a long session) remain the real gate on the product promise —
   no automated gate in this repository has ever proven real decoded frames, and none of my changes
   alter that.

**What I would not claim.** I have not verified that two people can watch a movie together. I have
verified that the code no longer *actively prevents* it — the guest is no longer seeked backwards
every second, and the film ending is now detectable. That is a fix to the mechanism, demonstrated by
test, and it is a materially different claim from "it works". The difference is exactly what M1 exists
to close.
