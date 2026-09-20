# Batch 8 — Deep-Audit Remediation

**Scope:** remediate the verified P0/P1/P2 findings from `DEEP_PRODUCTION_READINESS_AUDIT.md`.
**Method:** re-open the source, re-prove each finding, confirm it is not already fixed, confirm severity,
apply the smallest safe fix, add a regression test, add a negative control, then run the gates.
**Date:** 2026-09-20
**Baseline HEAD at start:** `e19c2eb` (branch `stabilization/v0.9.9-rc1`)
**Verification target:** **`7f2d59a`** — the full code + test state, **CI-verified green on both
platforms** (run `35502292851`). Earlier commits in the batch are listed in §9.

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
| **AUD-01** | **P0** | Yes — reproduced | No | **FIXED** (source + regression + control); two-device verify outstanding | Derive canonical position from the PLAY commit anchor | `guest_in_sync_is_never_seeked_by_the_player_event_loop` |
| **AUD-02** | **P1** | Yes | No | **FIXED** | Remove the dead `host_relay_ready_state`; anchor redefined | covered by AUD-01 test |
| **AUD-03** | **P1** | Yes | No | **FIXED — verified at runtime on real libmpv** (§4b) | `PlayerState::Completed` + `keep-open` + host `ended()` | `host_reaching_end_of_media_ends_playback`, `production_player_reports_completed_when_the_movie_ends` |
| **AUD-04** | P2 | Yes | No | **FIXED** | Decision extracted to `drift_correction_for_player` | AUD-01 test drives the real loop |
| **AUD-05** | P2 | Yes | No | **FIXED** | Tests now exercise the call site, not the helper | AUD-01 + control |
| **AUD-06** | P2 | Yes | No | **FIXED** — fixture + runtime-verified decode & AO init (§4c); audible output unverified | Explicit `audio`/`aid`/`ao` options + a new audio-bearing fixture | `player_requests_audio_output_explicitly`, `audio_bearing_fixture_yields_an_audio_track` |
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

**Scope note — narrowed after CI caught an over-reach (§4d).** The first implementation *also* wrote
the projection into `state.sync.position_ms`. That was wider than the defect and broke
`m2_integration`'s exact-position assertions on the macOS runner. The projection is now used **only**
as the drift reference; that field keeps its commit-derived value. The net effect is that the only
observable difference from pre-fix code is *which value the drift comparison reads* — which is exactly
the defect, and nothing more.

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

**And this is the one finding I was able to verify on real hardware rather than by reading code** —
`libmpv.dylib` turned out to be present on this machine, so the production player was driven against
a real 3-second file. It reports `Completed` at EOF, pinned to the duration, and clears it on seek
back. The `keep-open=yes` half of the fix was proven load-bearing by mutation. See §4b.

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

**Also verified the way CI will actually run it.** CI executes the step under `bash` on
`ubuntu-latest`, but my first check was in `zsh`. The script was extracted verbatim from `ci.yml` and
run with `bash`, exit 0:

```
package.json                 0.9.8
src-tauri/tauri.conf.json    0.9.8
src-tauri/Cargo.toml         0.9.8
Cargo.lock                   0.9.8
OK: all four version declarations agree (0.9.8)
```

Worth doing because the job is new: a shell-dialect difference between my interactive shell and the
runner is exactly the kind of thing that would have turned the first CI run red for no reason.

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

**Ten in total: six unit tests, plus two new macOS-only integration targets — three real-libmpv
end-of-media tests (§4b) and one audio test (§4c).** The six were verified by diffing
`#[test]`/`#[tokio::test]` attributes against HEAD (`+6`, exactly):

| Test | Proves |
|---|---|
| `guest_in_sync_is_never_seeked_by_the_player_event_loop` | An in-sync guest is **never seeked**, and the canonical position advances at **1×** (window 1000–2000 ms over a 1.4 s run). Drives the real `spawn_player_event_loop`. |
| `negative_control_frozen_anchor_reports_elapsed_time_as_drift` | The projection is load-bearing: without it, elapsed-since-commit *is* reported as drift and lands in the hard-seek band. |
| `host_reaching_end_of_media_ends_playback` | The host reaching EOF leaves `Playing`, stops drift correction, and the **frontend-visible** `sync.room_state` reads `ENDED`. |
| `negative_control_end_of_media_requires_a_playing_host` | Three ways the transition must **not** fire: a guest reporting `Completed`, a paused room, a merely-paused player. |
| `completed_player_state_has_a_distinct_wire_name` | `Completed` → `"COMPLETED"`, and is not confused with `PLAYER_ERROR`. |
| `player_requests_audio_output_explicitly` | `audio`/`aid`/`ao` are set explicitly, and `keep-open` (which AUD-03 depends on) is present. |

Plus two new integration targets driving **real libmpv** (both macOS-only):

- `tests/real_eof_detection_test.rs` — three tests through the **production** `MpvPlayer`:
  `production_player_reports_completed_when_the_movie_ends`,
  `production_player_is_not_completed_mid_playback` (negative control),
  `production_player_leaves_completed_after_seeking_back`. Detail in §4b.
- `tests/real_audio_test.rs` — `audio_bearing_fixture_yields_an_audio_track`, which asserts both that
  the audio fixture produces a decodable audio track *and* that the video-only fixture does not.
  Detail in §4c.

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

## 4b. Runtime verification on real libmpv — AUD-03 closed end-to-end

**This is the one place where I stopped reasoning from source and actually ran the real thing.**

The bundled `mpv_runtime/libmpv.dylib` (4.5 MB) **is present on this machine** — it is gitignored, so
CI can never have it. That means the real-playback path, which every gate in this repository is
structurally blind to, can be exercised here. I used it to convert AUD-03 from "source-level only"
into runtime evidence.

### What was observed (raw mpv, fixture = 3 s)

Two runs of the same file, differing only in `keep-open`:

| t | `keep-open=yes` | `keep-open=no` (the default) |
|---|---|---|
| 1.5 s | `eof-reached=false` `duration=3.0` `time-pos=2.600` | `eof-reached=false` `duration=3.0` `time-pos=2.633` |
| **2.0 s** | **`eof-reached=true`** `idle-active=false` `duration=3.0` `time-pos=2.967` | **`eof-reached=None`** `idle-active=true` `duration=None` `time-pos=None` |
| 2.5 s | `eof-reached=true` (stable) | `eof-reached=None` (stable) |
| after seek to 0.5 s | **`eof-reached=false`** `time-pos=0.500` | seek **fails** (`error running command`, −12) |

Read the middle row twice. **Without `keep-open`, `eof-reached` is not merely false — it is
unreadable.** The property read *errors* once mpv unloads the file, so
`mpv_get_flag(handle, fns, "eof-reached") == Some(true)` could never be true and `Completed` could
never be reported. `keep-open=yes` is therefore **load-bearing**, not cosmetic — and the code comment
that claimed this is now backed by a measurement rather than an assertion.

The last row also confirms the recovery path: seeking away from the end clears `eof-reached`, so
`Completed` is a live reading rather than a latch — which is what makes "seek back and watch it again"
work.

### Production-code verification: `tests/real_eof_detection_test.rs` (new)

The raw observation above proves *mpv's* behaviour. It does not prove *my* code. So the same ground is
covered through `MpvPlayer` — the type the runtime actually holds — in a new macOS-only integration
target:

| Test | What it establishes |
|---|---|
| `production_player_reports_completed_when_the_movie_ends` | Real decode → real EOF → `PlayerState::Completed`, and `position_ms` **pinned to** `duration_ms` (mpv's own last `time-pos` is 2967 ms; the code pins 3000). |
| `production_player_is_not_completed_mid_playback` | Negative control: every sample taken >500 ms before the end must be non-terminal, and the test **fails as vacuous** if it never observed such a sample. |
| `production_player_leaves_completed_after_seeking_back` | Seeking back to 500 ms clears `Completed` and moves the playhead — the recovery path. |

All three pass against real libmpv (13.6 s, real decoding).

**Note on the path:** `MpvPlayer` loads libmpv *only* in `attach_native_surface`; before that,
`play()` takes a documented simulation path that reports `Playing` without decoding anything. So the
test builds a real `CALayer` through the same minimal Objective-C bridge `real_native_surface_e2e.rs`
uses. A test that skipped that step would pass while decoding nothing — precisely the false-green this
suite exists to avoid.

### Negative control for this section: `keep-open` removed

`keep-open=yes` was mutated to `no` in `player_init_options()` and the new target re-run:

```
production_player_reports_completed_when_the_movie_ends ... FAILED
  the production player must report Completed at end of media;
  it reported Playing at position 0/Some(3000) ms.
production_player_leaves_completed_after_seeking_back ... FAILED
  precondition: the movie must reach Completed first
production_player_is_not_completed_mid_playback ... ok
```

Two of three go red, and the failure is **the pre-fix symptom itself**: the state never leaves
`Playing`. (The third passes correctly — it only fails if `Completed` appears *spuriously*.) The
mutation was reverted and verified **by hash**
(`a6e17a72140130bbf5d5bcb4df630f2784296e7cd2e10a8d3ed09f62e8adb8d6`, byte-identical to the backup),
after which all three pass again.

So the AUD-03 fix has a control at the player layer as well as at the runtime layer, and the control
reproduces the original defect on demand.

### CI must skip these by name — and now does

`tests/common/mod.rs` deliberately **fails loudly** when a prerequisite is missing (a BATCH5 policy:
no silent skips). CI has no libmpv, so the new tests would panic there and turn the build red. CI
therefore skips libmpv-dependent tests **by name**, and the three new names were added to that list
(5 → 9 across both new targets), along with the warning message. Verified: the YAML parses, the skip
list reads 9 entries, and a CI-style local run still reports **568 passed / 0 failed** with the new
targets showing `0 passed; 3 filtered out` and `0 passed; 1 filtered out`.

**This is the part that would have broken CI if I had not checked.** Adding a real-hardware test to a
suite whose prerequisites fail loudly is only safe if the CI skip list is updated in the same change.

---

## 4c. Audio verified against a real audio-bearing fixture (AUD-06)

**I was wrong when I said this was impossible.** I had recorded "no audio fixture exists and none can
be built here" because `ffmpeg` is absent. But the repository's own fixture generator uses
**Swift/AVFoundation**, which ships with macOS and needs no ffmpeg at all. The blocker was my
assumption, not the environment.

### What was built

`scripts/make-test-media-macos.swift` gained an **opt-in** `--with-audio` flag that adds a mono
440 Hz AAC track, fed as LPCM and encoded by AVFoundation. Opt-in on purpose: the committed
video-only fixture is depended on by the render tests (duration and frame content), so regenerating
*it* with audio would be a silent behaviour change for them. A **new** fixture was added instead:

| Fixture | Size | `hdlr` handlers | Codecs |
|---|---|---|---|
| `movie_party_test_320x240.mp4` (committed, untouched) | 53,203 B | `['vide']` | `['avc1']` |
| `movie_party_test_with_audio_320x240.mp4` (**new**) | 92,203 B | `['vide', 'soun']` | `['avc1', 'mp4a']` |

Verified by parsing the MP4 boxes directly — so the audio fixture provably has an audio track, and the
original provably still does not. The tone is **not silence**: an all-zero track could not distinguish
"the audio decoded" from "the audio decoded to nothing", which is the entire point of having it.

### The test, and what it observed

`tests/real_audio_test.rs` runs libmpv with the **same audio options production sets** and asserts both
directions:

```
AUDIO FIXTURE:      track-list/count=Some(2)  aid=Some(1)  audio-params/channel-count=Some(1)  audio-out-params/channel-count=Some(1)
VIDEO-ONLY FIXTURE: track-list/count=Some(1)  aid=None     audio-params/channel-count=None
```

- **Positive:** the audio fixture presents 2 tracks, mpv *selects* the audio track (`aid=1`), and the
  track resolves real channel parameters.
- **`audio-out-params/channel-count=Some(1)`** — the **audio output initialised**. That is the
  strongest available evidence that audio actually flows toward an output, not merely that a track
  exists.
- **Negative control:** the video-only fixture yields no `aid` and no audio parameters. Without this,
  "mpv reported an audio track" would be a property of mpv rather than of the media.

Raw mpv API rather than `MpvPlayer`, deliberately: `PlayerSnapshot` carries no track information, so
the production type cannot answer the question. What is verified here is the **media and the runtime**;
the app's *decision* to request audio is pinned separately by
`player_requests_audio_output_explicitly`.

### A trap worth recording: two-input AVAssetWriter deadlocks silently

My first version hung with a partially written file and **no error**. The cause: feeding video
unconditionally (it was checked first each iteration) let video race to the end of the file while audio
was still at ~1 s, after which AVAssetWriter reported `isReadyForMoreMediaData == false` on **both**
inputs forever. Two fixes were needed together:

1. each iteration feeds whichever input is **behind in time**, keeping them in step; and
2. each input is marked finished **as soon as its last sample is appended**, not after the loop.

I also replaced the unbounded `finishWriting` wait with a 60 s bounded one, because a silent
`finishWriting` hang is indistinguishable from "still encoding" — which is exactly how this first
presented.

### What this does and does not close

**Closed:** an audio-bearing fixture exists and is committed; libmpv finds, selects and initialises
output for its audio track; and the repository can no longer be described as never having decoded a
sound.

**Not closed:** this ran headless with `ao=null`, so **no sound reached a speaker**. Audible output and
A-V sync across two devices remain manual (M2).

---

## 4d. The push found a real regression — and it was mine

**CI run `35501328187` @ `117e797`.**

| Job | Result |
|---|---|
| Frontend | ✅ |
| **Rust (windows-latest)** | ✅ **553-class pass, 0 failed** — the point of the push |
| **Rust (macos-latest)** | ❌ **failed at the Tests step** |
| Version consistency | ✅ (first ever execution) |
| Cargo audit / pnpm audit | ✅ |

Windows green, macOS red — the **exact inverse** of Batch 7A, which was Windows-only and macOS-green.
Worth noting for its own sake: neither platform's green implies the other's, and this batch produced a
counter-example in both directions.

### The failure

```
test_d_seek_sets_canonical_position_on_both ... FAILED
  thread panicked at src-tauri/tests/m2_integration.rs:69:13:
  guest predicate not satisfied within 30s
```

That is the *same message* as the original Batch 1 defect, so it deserved real suspicion rather than a
re-run.

### Root cause: my AUD-01 fix was too broad

The first implementation also wrote the wall-clock projection into the guest's
`state.sync.position_ms`:

```rust
} else if let Some(canonical) = Self::projected_host_position_ms(&state) {
    state.sync.position_ms = canonical;     // <-- this line was the regression
}
```

`test_d` asserts the guest's canonical position is **exactly** the seek target (`== 42_000`). But:

- the projection advances at 1× wall-clock time, and
- the event loop only writes it inside the `position_changed || state_changed` block — which, with the
  suite's non-advancing test player, means **only on state transitions**.

So the field is set to `target + (host_now − commit_deadline)`. A PLAY commit's deadline is ~750 ms in
the future, so if the transition lands inside that window the field is set to exactly `42_000` and the
predicate is satisfied. If it lands **after** it, the field overshoots, is never rewritten (no further
transitions), and the exact-equality predicate can never be satisfied again → 30 s timeout.

**Why every local run passed:** the transition happened to land inside the deadline window. This was a
race I introduced — my local runs got lucky, twice, and CI did not.

### The fix: narrow it to the defect

The projection is needed as the **drift reference**, not as the *value* of `sync.position_ms`. Removing
the field write restores that field's exact prior semantics — it is written only at commits — so
nothing else changes observably, while the drift comparison still uses the projection.

That is a strictly better outcome than the original: the only behavioural difference from pre-fix code
is *which value the drift comparison reads*, which is precisely the defect.

### Re-verification after the narrowing

- **Mutation control re-run** (the test's assertions changed, so the earlier proof was invalidated):
  with the projection disabled the regression still goes **red** with the same signature —
  `seeked to [1000000, 1000000, 1000000]`. Reverted, verified by hash.
- `m2_integration` **28 passed / 0 failed, three consecutive runs** (the suite whose flakiness this was).
- `cargo fmt --check` clean; `clippy --all-targets --all-features -D warnings` exit 0 / zero warnings.
- The AUD-01 test gained an explicit assertion that the loop **must not** overwrite
  `sync.position_ms` with a projection — so this specific over-reach cannot come back silently.

### Re-run: green on both platforms

**CI run `35502292851` @ `7f2d59a` → SUCCESS, all six jobs.**

| Job | Result |
|---|---|
| Frontend | ✅ |
| **Rust (macos-latest)** | ✅ **568 passed / 0 failed / 2 ignored / 8 filtered** across 17 targets |
| **Rust (windows-latest)** | ✅ **559 passed / 0 failed / 2 ignored / 1 filtered** across 17 targets |
| Version consistency | ✅ — first execution; printed `OK: all four version declarations agree (0.9.8)` |
| Cargo audit / pnpm audit | ✅ |

Both platforms gained exactly **+6** over the Batch 7A baseline (562 → 568, 553 → 559), which is the
six new unit tests. The macOS-only integration tests correctly report 0 tests on Windows.

### A flaky test of my own, found by the full local run

The full local suite then caught `production_player_leaves_completed_after_seeking_back` failing —
**my own new test**, and the same *class* of mistake as the CI race: its wait predicate required only
`state != Completed`, and mpv clears `eof-reached` **before** `time-pos` has moved. So the wait could
return while the position was still pinned at the duration, and the following
`position_ms < 1_500` assertion failed.

Fixed by requiring **both** conditions in the predicate. Verified stable: **3 consecutive runs, 3/3
green.**

Worth stating plainly: I introduced two timing-dependent tests in this batch — one broke CI, one
broke my local suite — and both were *predicates satisfied by a transient state*. That is the single
most common way to write a test that passes by luck.

### The lesson, recorded

A fix that changes a **field's meaning across a whole subsystem** is a bigger change than the defect
warrants. The audit's own prescription was "redefine the drift anchor" — I did that, and then did more
by making the field live, and the "more" is what broke. The narrower change is both smaller and safer,
and the audit's wording was already pointing at it.

---

## 5. Full validation results

| Gate | Result |
|---|---|
| **CI run `35502292851` @ `7f2d59a`** | **✅ SUCCESS, all six jobs — macOS 568/0, Windows 559/0, Version consistency ✅** |
| `cargo fmt --check` | **clean** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **exit 0, zero warnings** |
| `cargo test` (all targets, CI's skip list) | **568 passed / 0 failed / 2 ignored / 8 filtered** across 17 targets |
| `cargo test` (all targets, **real libmpv present, no skips**) | **576 passed / 0 failed / 2 ignored** across 17 targets |
| `eslint . --max-warnings=0` | **exit 0**, zero output |
| `tsc --noEmit` | **exit 0** |
| `vite build` | **exit 0** (12.79 s; pre-existing chunk-size advisory) |
| `vitest run` | **287 passed / 23 files** |

**The Rust totals reconcile exactly, two ways.**

*Against CI.* The last CI-verified macOS total was **562**. The CI-comparable local run is **568** —
the same 562 plus the 6 new unit tests. My 4 new integration tests are skipped by name in CI (they
need the runtime), so they add nothing to that figure; the new targets correctly show
`0 passed; 3 filtered out` and `0 passed; 1 filtered out`. Per-target: lib 484 (one filtered),
dep_audit 3, host_guest 2, **m2_integration 28**, m3_closure 7, m3_integration 18, m3_m4_e2e 9,
m4_closure 15, tailscale 2; `real_native`/`real_playback`/`real_sw_render`/`windows_native`/doc-tests
each 0. All 17 result lines read `0 failed`.

*Against the runtime.* With `libmpv.dylib` present and no skips, the total is **576** —
`576 − 568 = 8`, which is exactly the 8 skipped tests that exist on macOS (`bundled_runtime_is_loadable`
in the lib, one each in `real_native_surface_e2e`, `real_playback_smoke_test` and `real_sw_render_test`,
three in `real_eof_detection_test`, one in `real_audio_test`). The 9th skip,
`bundled_libmpv_renders_onto_real_child_hwnd_through_production_player`, is Windows-only, so it
contributes 0 tests here — which is why the delta is 8 and not 9.

**Every libmpv-dependent test passes on this machine**, including `real_sw_render_test` (2.65 s) and
`real_native_surface_e2e` (4.45 s), which CI can never execute.

**Two environment notes, stated rather than glossed:**

- `pnpm lint` / `pnpm test` cannot run directly: pnpm's dependency pre-check tries to create a
  symlink in `/Volumes/T7 Shield/.pnpm-store`, which the sandbox denies (`EEXIST`). I ran the
  underlying binaries (`node_modules/.bin/eslint`, `vitest`, `tsc`, `vite`) instead. Same tools,
  same config.
- The `cargo test` shell exits non-zero because a test touches the macOS keychain, which the sandbox
  blocks. `CARGO TEST EXIT: 0` and all 16 `test result:` lines are `ok`. Unrelated to these changes.

**No unrelated behaviour changed.** `dist/` is gitignored, so the build dirtied nothing tracked.
The change touches: `app_runtime.rs`, `media/mod.rs`, `media/player/mod.rs`,
`media/player/mpv_backend.rs`, `media/shared_pipeline.rs` (comments only),
`providers/chrome/mod.rs` (test only), `sync/local.rs`, `.github/workflows/ci.yml`,
`tests/common/mod.rs` (added an audio-fixture prerequisite helper), two new test targets
(`real_eof_detection_test.rs`, `real_audio_test.rs`), the fixture generator
(`scripts/make-test-media-macos.{sh,swift}` — an **opt-in** `--with-audio` flag; the default path is
unchanged, and the committed video-only fixture was **not** regenerated), and one new committed
fixture (`movie_party_test_with_audio_320x240.mp4`, 92 KB).

---

## 6. Remaining findings

### AUD-08 — NOT remediated (P2, open)
`shared_pipeline.rs` is stale and uncompiled. Repairing it is Shared work (out of scope); deleting it
is the owner's call. Now recorded loudly in the file and `media/mod.rs` so it cannot mislead.

### AUD-06 — CLOSED except for audible output (P2)
The audit asked for "an audio-bearing fixture" as the regression test. I had recorded that as
impossible here because `ffmpeg` is missing — **that was wrong.** The repository's own generator uses
Swift/AVFoundation, which needs no ffmpeg. §4c records what was built and measured.

**Now closed:** a committed audio-bearing fixture exists; libmpv finds, selects and initialises output
for its audio track (`aid=1`, `audio-out-params/channel-count=1`); and the video-only fixture is
asserted to have no audio track at all, so the positive result is about the *media* rather than about
mpv. The repository can no longer be described as never having decoded a sound.

**Still open:** the test runs headless with `ao=null`, so **no sound reached a speaker**. Audible
output and A-V sync across two real devices remain manual (M2).

**One stale comment disproved while verifying.** `tests/real_native_surface_e2e.rs:113-116` states
that "load_current_media inside the player sets vo=null/ao=null". It does not. `load_current_media`
sets exactly two things — `pause=1` and `loadfile` — and the only `vo`/`ao` assignment anywhere in
`mpv_backend.rs` is the one AUD-06 added. So the comment is wrong, and wrong in a direction that
matters: it implies production audio is deliberately disabled by the player, which would make AUD-06
look closed when it is not. Worth correcting so nobody reasons from it. The observable truth is
narrower and less comfortable: the player *requests* audio, and the fixture has none to play.

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

**The Windows gap is closed. What remains is real-hardware behaviour, not platform coverage.**

`7f2d59a` **has been through CI and is green on both platforms** (run `35502292851`) — see §4d, which
also records that the *first* push went red on macOS and why. The remote tip is now `7f2d59a`, not
`a44542a`.

| Item | Status |
|---|---|
| Rust on macOS, **without** a runtime (CI's configuration) | **CI-verified** — 568 passed / 0 failed |
| Rust on Windows | **CI-verified** — 559 passed / 0 failed |
| Windows clippy (`-D warnings`) | **CI-verified** — the step passed |
| Rust on macOS, **with** the real libmpv runtime | **Locally verified** — the 8 libmpv-dependent tests execute |
| AUD-03 end-of-media on real libmpv | **Verified** (§4b) — real decode, real EOF, `Completed` |
| AUD-06 audio decode + AO init on real libmpv | **Verified** (§4c) — but headless `ao=null`, so no audible output |
| Frontend | CI-verified (287 tests) + locally (lint/tsc/build clean) |
| `version-consistency` job | **CI-verified** — executed and printed all four declarations agreeing |

**One Windows-specific limitation that remains, stated plainly:** the new
`tests/real_eof_detection_test.rs` and `real_audio_test.rs` are `#![cfg(target_os = "macos")]`, so on
Windows they compile to **zero tests** — correct behaviour, matching the other `real_*` targets, but it
means the Windows job does not exercise end-of-media detection or audio at all. Windows EOF behaviour
rests on `mpv_get_flag("eof-reached")` and `keep-open`, and Windows audio on `ao=auto` selecting
WASAPI — none of which I have observed on that platform. Green Windows CI means the code compiles and
its platform-neutral suites pass; it is not evidence about Windows playback.

---

## 8. Manual validation requirements

| # | Requirement | Why it cannot be automated here |
|---|---|---|
| M1 | **Two real devices, one real movie, watch it to the end.** | The only thing that confirms AUD-01's fix. My evidence is a scripted player driving the real loop; nobody has watched a real decoded frame stay in sync. |
| M2 | **A movie with an audio track**, confirming sound and A-V sync. | **Narrowed, not closed.** An audio-bearing fixture now exists and libmpv is verified to select and initialise output for its audio track (§4c). What is unverified is *audible* output — the test ran headless with `ao=null`, so no sound reached a speaker — and A-V sync, which needs two devices. |
| M3 | **A 2-hour (ideally 8-hour) session**, watching drift. | The projection depends on the continuously re-calibrated clock offset. Whether the offset stays accurate enough over hours is unverified. |
| M4 | **End-of-media on both sides** — host and guest both leave `PLAYING`; seek back and resume works. | **Narrowed, not closed.** The player layer is now verified on real libmpv (§4b): `Completed` is reported at EOF, pinned to the duration, cleared by seeking back. What remains is the *coordination* — that the host's `Ended` reaches a real guest and both sides stop correcting drift. That needs two devices. |
| M5 | **Pause / seek / buffering / reconnect** during playback on both sides. | Needs two devices. |
| M6 | **Windows build**, at minimum the CI run; ideally a real Windows machine. | No Windows toolchain here. Windows will not run the new end-of-media tests at all (they are macOS-gated), so Windows EOF behaviour is unobserved. |

Batches 7B's `BATCH7B_REAL_BETA_VALIDATION.md` (28 areas, T01–T28) remains the procedure for M1–M5.
**Every row in it is still NOT TESTED** — and note that §4b does *not* change that: it verifies the
player, not the party.

---

## 9. Commit SHA

| | SHA | Pushed? | CI? |
|---|---|---|---|
| Baseline HEAD (audit target) | `e19c2eb` | yes | — |
| This batch — remediation code | `f280b6c` | yes | ✅ (as part of `117e797`) |
| This batch — end-of-media runtime test + CI skips | `f2a9b2f` | yes | ✅ |
| This batch — audio fixture, generator flag, audio test | `1424116` | yes | ✅ |
| This batch — docs (report, + amendments) | `33e0e4a`, `117e797` | yes | ❌ `117e797` **failed on macOS** (§4d) |
| **This batch — narrowed AUD-01 + report** | **`7f2d59a`** | **yes** | **✅ both platforms** |
| Previously CI-verified (Batch 7A) | `a44542a` | yes | ✅ |

**`7f2d59a` is the SHA that matters for verification** — the complete code, test and fixture state,
green on macOS and Windows. Note the honest path: the first push (`117e797`) failed on macOS, which is
why the SHA moved. `f280b6c` is the remediation itself (9 files, +1635 / −57, including the audit
report); the commits after it add tests, fixtures, documentation, and the AUD-01 narrowing.

Unchanged and confirmed after the commit: `main` = `2d5c833` (local) / `bb22577` (remote), all five
tags, `v0.9.8` → `bb22577`, and no release created or modified.

---

## 10. Is the code ready for the final regression audit?

**Yes. The condition that was outstanding is now met.**

No **P0 or P1 remains as an open code defect.** AUD-01 (P0), AUD-02 and AUD-03 (P1) are fixed and
regression-tested; AUD-01's fix was proven non-vacuous by reproducing the original defect on demand
(and re-proven after it was narrowed); AUD-03 and AUD-06 are verified against real libmpv.

**`7f2d59a` is CI-verified green on both platforms** — macOS 568 passed / 0 failed, Windows 559
passed / 0 failed, plus the new version-consistency job. The path there was not clean: the first push
(`117e797`) went **red on macOS** because my AUD-01 fix was too broad, and §4d records the failure, the
diagnosis, the narrowing and the re-verification. Windows passed on the first push and every push
since.

So the "STOP before release preparation if any P0/P1 remains" condition is not triggered, and the
Windows-coverage gap that made this unanswerable earlier is closed.

**What is still not closed is not a code question:**

1. **M1–M3 remain the real gate** — two real devices, audible sound, and a multi-hour session. No
   automated gate in this repository has ever proven two devices staying in sync, and none of my
   changes alter that.
2. **Windows playback behaviour is unobserved**, not merely untested: the new real-hardware tests are
   macOS-gated, so Windows CI proves compilation and the platform-neutral suites, nothing more.
3. **AUD-08 is unremediated** by your instruction (Shared work is out of scope), and **AUD-15 is
   flagged but unchanged**.

**What changed since the first draft of this report.** Two findings moved because two assumptions of
mine turned out to be wrong — both in the direction of "more verifiable than I claimed":

- **AUD-03** went from "fixed at source, runtime verification outstanding" to **verified on real
  hardware**, because the bundled `libmpv.dylib` turned out to be present on this machine. That let the
  real-playback path finally be *executed* rather than reasoned about, and it produced the mutation
  proof that `keep-open` is load-bearing.
- **AUD-06** went from "cannot be done here" to **closed except for audible output**, because the
  fixture generator uses Swift/AVFoundation and never needed the `ffmpeg` whose absence I had treated
  as decisive. It also produced the audio fixture the audit asked for.

Both are the same lesson, and it is worth stating: **I had written off two verifications on
environmental grounds without checking the environment.** The libmpv runtime was on disk; the
generator had no ffmpeg dependency. Neither blocker was real.

**What I still would not claim.** I have not verified that two people can watch a movie together. The
honest statement is narrower and I will keep it narrow:

- the guest is **no longer seeked backwards every second** (regression-tested, with the original
  defect reproduced as a control);
- the end of a film is **now detectable and coordinated** (verified on real libmpv at the player
  layer; the host→guest coordination is tested but not observed across two machines);
- **an audio track is found, selected and routed to an initialised output** — but I have not heard it,
  and neither has anyone else, because the test is headless;
- and **no automated gate in this repository has ever proven two devices staying in sync** — none of
  my changes alter that, and only M1 closes it.

That is a fix to the mechanism, demonstrated by test and by execution. It is a materially different
claim from "it works", and the difference is exactly what M1 exists to close.
