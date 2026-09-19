# Batch 5 — Make the automated test suite truthful

**Status:** complete, verified. `v0.9.8` (`bb22577`) untouched — no tag created or moved, no release
edited, no force-push. Provider Shared untouched and still not compiled into the build. No unrelated
production refactors.

**Headline:** four tests could report `PASS` without executing a single assertion (the three named in
the audit, plus a fourth found during this work). All four now **fail loudly** instead. The media
fixture is committed to the repository. `real_sw_render_test`'s failure was diagnosed and classified
as a **test parameter defect**, not a product defect. `real_native_surface_e2e`'s long-standing flake
was diagnosed and **eliminated** (8/8 after the fix, 2/6 failing before).

---

## 1. Exact effective test count

Full suite, macOS, Rust `1.98.0`, with the bundled libmpv runtime staged (so nothing is skipped):

| Target | Result |
|---|---|
| `movie_party_lib` (lib) | **479 passed**, 0 failed, 2 ignored |
| `movie_party` (bin) | 0 |
| `dependency_audit` | 3 |
| `host_guest_wiring` | 2 |
| `m2_integration` | 28 |
| `m3_closure` | 7 |
| `m3_integration` | 18 |
| `m3_m4_e2e` | 9 |
| `m4_closure` | 15 |
| `real_native_surface_e2e` | **1** (genuinely executes) |
| `real_playback_smoke_test` | **1** (genuinely executes) |
| `real_sw_render_test` | **1** (genuinely executes) |
| `tailscale_probe` | 2 |
| `windows_native_surface_e2e` | 0 (macOS: `#![cfg(windows)]`) |
| doc-tests | 0 |
| **Total** | **566 passed, 0 failed, 2 ignored** |

So the suite contains **568 test functions**, of which **566 execute** and **2 are `#[ignore]`d**:

* `providers::chrome::tests::real_chrome_launches_and_evaluates_cdp`
* `providers::chrome::tests::real_youtube_provider_sync_uses_chrome_cdp`

Both are `#[ignore]`d with a stated reason ("launches real Chrome…"), so `cargo test` reports them as
**ignored** — an explicit skip, not a silent pass. Verified with `cargo test --lib -- --ignored --list`.

### Reconciling the reported 515

The 515 figure is the **sum of the `ok. N passed` lines** in a `cargo test` log. That sum is not a count
of test functions, and it was not a count of tests that executed:

1. **It excluded `#[ignore]`d tests**, so it undercounted the suite by the ignored count (2 today).
2. **It included tests that could pass without executing.** At the time it was measured, the four
   `bundled_libmpv_*` tests each began with `if !<prerequisite>.exists() { eprintln!("SKIP: …"); return; }`
   and were reported as `ok` in 0.00 s. In any clean checkout — including every CI run — those four
   contributed 4 to the "passed" sum while asserting nothing.
3. **`--test-threads=1` without `--no-fail-fast` aborts at the first failing target**, so on Windows the
   run silently omitted every target after `m2_integration` (see §5). A sum taken from such a log is a
   partial count presented as a total.

The current truthful figures are the table above: **566 executing, 2 explicitly ignored**. The growth
from 515 is accounted for by tests added in Batches 1–4, which the project memory records per batch.

---

## 2. Which tests genuinely execute

Every `bundled_libmpv_*` test now prints its prerequisites and its own evidence. Representative
`--nocapture` output from this machine:

```
PREREQUISITE OK: libmpv runtime at ".../src-tauri/mpv_runtime/libmpv.dylib"
PREREQUISITE OK: fixture at ".../src-tauri/tests/fixtures/movie_party_test_320x240.mp4"
CHECKPOINT: playback started (position 33ms)
RENDER PASS: 26 frames in 1.2s (21 fps)
PASS: pause holds position (1233ms → 1233ms)
PASS: seek moved to ~1500ms (target 1500ms)
PASS: resume advances position (1500ms → 1966ms)
```

```
fixture duration 3.000s (>= 2.0s required)
SW render: 10 renders in 0.38s — 10 with pixel data, 0 black, 0 with no new frame (buffer 320x240 stride 1280)
Frame pixels verified: up to 303537/307200 non-zero bytes, first24=[21, 0, 26, 255, 22, 0, 26, 255, …]
```

**Cannot execute on any platform available here:** `windows_native_surface_e2e`. It needs
`mpv_runtime/libmpv.dll`, and this repository has **no way to produce one** — `scripts/` builds and
stages macOS dylibs only, and `build.yml`/`release.yml` contain no Windows libmpv step. On Windows CI
the test therefore FAILS (as intended) rather than passing silently, and CI reports the skip
explicitly. It is not counted as executing.

---

## 3. `real_sw_render_test` — root cause and classification

### Classification: **test parameter/configuration defect.** Not a product defect. Not an environment limitation.

The evidence, in the order it was obtained:

1. **Reproduced** the failure with the fixture present: `RENDER PASS: 66 frames produced in 3.0s`,
   then `rendered frame buffer must contain non-zero pixel data`.
2. **Ruled out the obvious hypothesis.** mpv's render API is sometimes said to require `vo=libmpv`
   before `mpv_initialize`. Setting it changed nothing — same 65 frames, same zeroed buffer.
3. **Asked whether mpv wrote anything at all**, by pre-filling the target buffer with a non-zero
   sentinel (`0xAB`). Result: `sentinel_left=0` — mpv had written **all 307 200 bytes**. So the buffer
   was not untouched; it was *written with zeros*. That is a black frame, not a no-op.
4. **Correlated with mpv's own frame-availability flag.** Instrumenting
   `mpv_render_context_update` (which the old test never called) gave:

   ```
   rendered=54 update_frame_flag=46 wrote_frames=54
   first8=[(1, 306659), (1, 306650), (1, 306681), …]   <- flag set: real pixels
   last8 =[(0, 307200), (0, 307200), (0, 307200), …]   <- flag clear: a black frame
   ```

   When a new frame is available the render produces **real pixel data** (~306 659 non-zero bytes).
   When none is available — i.e. past the end of the media — mpv writes a **fully zeroed frame**.

5. **Found the mismatch.** Measured `duration` = **3.000 s**, and the render loop ran for **3.0 s**
   starting ~1 s into playback. The loop therefore always finished *after the file ended*, and the
   assertion sampled one of the trailing black frames. Measured on the old code: **47 renders carried
   pixels, the last 3 were black**, and the assertion read the buffer after all 3.

6. **Controlled it.** Changing only the render window from 3.0 s to 1.0 s made the test **pass**, with
   no other change: `frames_with_pixels=17 frames_written_black=0`.

That is a test defect in two compounding parts — the render window was not shorter than the media, and
the assertion inspected an arbitrary *final* buffer instead of frames the renderer actually produced.
**The rendering pipeline was correct throughout**, which is why this must not be reported as a product
rendering defect.

### Fix

The test now states its requirement directly instead of sampling a moment:

* renders until it has collected `MIN_FRAMES_WITH_PIXELS` (10) renders that carry real pixel data, or
  hits a 10 s ceiling;
* classifies every render — wrote-nothing / wrote-black / wrote-pixels — using the sentinel and
  `mpv_render_context_update`, and reports the tally;
* asserts on the pixel-bearing frames and keeps the best frame as evidence;
* validates the fixture's duration up front (`>= 2.0 s`), so a too-short fixture produces a clear
  message instead of the old confusing symptom.

Because the loop stops as soon as it has enough real frames, it no longer depends on the fixture's
length at all.

### Product question this surfaced — measured, and **not a defect**

Production's `MpvPlayer::render_next_frame` (`src-tauri/src/media/player/mpv_backend.rs:709`) has the
**same shape** as the old test: it calls `mpv_render_context_render` without consulting
`mpv_render_context_update`, and returns the buffer whenever `rc == 0`. `spawn_player_render_loop`
(`app_runtime.rs:5888`) then renders unconditionally every ~30 ms and presents whatever comes back, and
`display_frame` sets the CALayer `contents` unconditionally. Since a render with no frame available
yields an all-zero frame (§3), that looked like it could present black frames between real ones.

**It does not.** I instrumented the production path — `MpvPlayer` + `display_frame` at the production
30 ms cadence, classifying every presented frame by pixel content:

```
PROBE presented: 23 frames — 23 with pixels, 0 ALL-ZERO (non-zero byte range 296592..304196)
PROBE paused:    10 with pixels,  0 ALL-ZERO
```

**Every presented frame carried real pixel data, mid-playback and while paused.** So:

* the trailing black frames in §3 occur only **past end-of-media**, where a black picture is the
  expected result for a finished video — not a rendering defect;
* pausing does **not** blank the picture (mpv redraws the current frame);
* the production render loop is fine as written.

The probe was temporary and has been reverted; the test file is byte-identical to its committed state
(`sha256 4c56e6a3…`). No production change was made or is needed. This closes the question rather than
leaving a suggestive "possible defect" note behind.

---

## 4. CI fixture strategy

### The fixture is committed

`src-tauri/tests/fixtures/movie_party_test_320x240.mp4` — 53 203 bytes,
`sha256 b56095304eb84aea0d26099d3783da8fb8219f7a7cab70fda8adec4386de648a`.

* H.264 (`avc1`) in an ISO-MP4 container, **no audio track**, 320×240, 15 fps, **3.000 s**.
* Tracked in git. `.gitignore` excludes `*.mp4` globally, so an explicit negation was added
  (`!src-tauri/tests/fixtures/*.mp4`) — verified with `git ls-files --others --exclude-standard`.
* This replaces the hardcoded `/tmp/movie_party_test.mp4` (macOS) and
  `C:\Windows\Temp\movie_party_test.mp4` (Windows) that **nothing in the repository created**. That
  is the root of the false greens: on a clean checkout the file never existed.

### Regeneration

`scripts/make-test-media-macos.swift` (+ `scripts/make-test-media-macos.sh`) generates an equivalent
fixture using **AVFoundation only** — no ffmpeg, which is not a dependency of this repository and is
not installed on this machine. Verified working: it produced a valid 6 s fixture
(165 230 bytes, 90 frames, `avc1`), and **both fixed tests pass against that independently generated
file** (SW render: 10/10 renders with pixels, 307 200/307 200 non-zero bytes). The pattern is
deliberately bright and non-uniform, with a sweeping bar, so no frame is black and consecutive frames
differ — otherwise a "non-zero pixels" assertion would be vacuous or wrong.

### libmpv in CI — the structural limitation

**CI cannot obtain the libmpv runtime, and this is not fixable within the repository as it stands:**

* `src-tauri/mpv_runtime/*` is gitignored (13 dylibs, ~15 MB, LGPL — deliberately not committed).
* `scripts/stage-libmpv-macos.sh` copies from `/tmp/mpv-build-workspace/stage` and Homebrew paths that
  only exist after `scripts/build-libmpv-macos.sh` has built ffmpeg+mpv locally. Neither is available
  on a hosted runner.
* There is no prebuilt artifact to download, and substituting Homebrew's libmpv would defeat the
  point — these tests exist to exercise *the dylib the packaged app ships*.

**Therefore CI does not run these tests, and now says so.** `.github/workflows/ci.yml` gained a
`libmpv runtime state` step and a conditional `Tests` step:

* runtime present → `cargo test -- --test-threads=1`, so the tests execute for real;
* runtime absent → `::warning` annotation naming every skipped test, an explicit
  `EXPLICIT SKIP: … did not execute` line, and a run with exactly those tests filtered out.

The skip is by **name, not by prefix** — `bundled_libmpv_path_resolves_correctly_inside_app_bundle`
is pure path-string logic and still runs. Verified locally: 478 lib tests pass with 1 filtered.

**Net effect:** on CI the real tests are reported as an explicit skip; they can no longer contribute a
pass they did not earn. On a machine with the runtime staged they execute and fail loudly if anything
is wrong.

---

## 5. Windows `test_b` — revisited, and the honest answer

**Result after Batch 1: UNVERIFIED. No CI run has ever exercised the Batch 1 fix.**

Evidence from the GitHub API (`gh`), which is available on this machine:

* The most recent CI run is **2026-09-17T16:45Z** (run `35248588394`, `v0.9.8`).
* Batches 1–3 were committed **2026-09-19T13:36Z** (`c842fa7`). Batch 4 was committed later still.
* CI is `workflow_dispatch`-only, and **no run has been dispatched since**. So the
  `SEQUENCE_REORDER_WINDOW = 64` reorder-window fix has never been through CI.

What the last executing run did show — and it is exactly the failure the fix targets:

```
Rust (macos-latest)   ✓ success (6m39s)
Rust (windows-latest) ✗ failure (8m55s)
    test test_b_play_transitions_both_to_playing ... FAILED
    panicked at src-tauri\tests\m2_integration.rs:41:13:
    host predicate not satisfied within 30s
    test result: FAILED. 27 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 87.50s
```

So: the Windows failure **was** `test_b`, it was Windows-only (macOS passed the same run), and the
Batch 1 root-cause analysis matches it. The fix is present in the working tree and passes locally —
but **macOS ≠ Windows**, and I have no evidence it resolves the Windows behaviour. It must be
re-verified by dispatching `ci.yml` against a commit containing Batch 1.

That run also shows the fail-fast trap: `cargo test` aborted at `m2_integration`, so `m3_closure`,
`m4_closure`, `real_*`, `tailscale_probe` and `windows_native_surface_e2e` were **silently omitted**
from the log. On Windows, `real_playback_smoke_test` and `windows_native_surface_e2e` would have
contributed further false greens had the run continued.

Windows lib test count for reference: **432 passed, 2 ignored** (vs 481 on macOS) — 49 tests are
macOS-only.

---

## 6. The five silent-pass instances removed

All five now **fail loudly** with an actionable message. There is no environment variable, feature
flag or CI setting that converts a missing prerequisite back into a pass.

| # | Location | Old behaviour |
|---|---|---|
| 1 | `tests/real_playback_smoke_test.rs` | `return` if `mpv_runtime/libmpv.dylib` or `/tmp/movie_party_test.mp4` absent |
| 2 | `tests/real_native_surface_e2e.rs` | same |
| 3 | `tests/real_sw_render_test.rs` | same, **plus** a third `return` if the dylib failed to load |
| 4 | `tests/windows_native_surface_e2e.rs` | same, against `libmpv.dll` and `C:\Windows\Temp\…` |
| 5 | `src/media/player/mpv_backend.rs` (`bundled_runtime_is_loadable`) | `return` if the bundled runtime was absent |

Shared policy lives in `src-tauri/tests/common/mod.rs`. `real_playback_smoke_test` additionally gained
`#![cfg(target_os = "macos")]`: it loads a `.dylib` by name, so on Windows it is now honestly reported
as zero tests rather than "passing". Instance 5 was renamed from
`…_when_present` and macOS-gated for the same reason.

### Negative controls (a test that cannot fail proves nothing)

* **Fixture hidden** (`cp` out, `rm`, then restore and re-verify by `sha256`): all three macOS real
  tests **FAILED** in 0.00 s with
  `PREREQUISITE MISSING: the committed media fixture is absent at …` and the regeneration command.
  Before this batch the same condition produced `ok` in 0.00 s.
* **libmpv hidden** (hash recorded, file removed, restored, hash re-verified, 13 dylibs intact): both
  tests **FAILED** in 0.00 s naming `stage-libmpv-macos.sh` and `build-libmpv-macos.sh`.

---

## 7. `real_native_surface_e2e` — the "flake" was a defect, and it is gone

The project memory recorded this test as flaky ("2 fail / 1 pass"). A/B on the **unmodified** file,
before any change of mine, measured **2 failures in 6 runs**. The failing profile is always the same:

```
RENDER PASS: 64 frames in 3.0s (21 fps)
PASS: CALayer contents is non-nil
PASS: pause holds position (0ms → 0ms)          <- position never advanced
panicked: seek: PlaybackError { message: "error running command" }
```

Root cause is the **same defect as `real_sw_render_test`**: the 3.0 s render window consumed the whole
3.000 s fixture, so the pause/seek/resume steps ran against ended playback, `time-pos` read 0, and
`seek` failed. It looked random only because whether playback finished inside the window depends on
scheduling.

Fix, in the test only — **assertions unchanged**:

* a bounded wait for playback to actually start (position > 0) before rendering, so the failure mode
  surfaces as a clear message rather than a confusing seek error;
* render window 3.0 s → **1.2 s**, leaving media for the later steps.

Result: **8/8 consecutive passes** (and it is faster: ~4.4 s vs ~6.5 s). This is a fix, not a mask —
the mechanism is understood and the timing window is now deterministic rather than marginal.

---

## 8. Validation

Rust toolchain **pinned to `1.98.0`** to match CI.

| Gate | Result |
|---|---|
| `cargo +1.98.0 fmt --all --check` | **clean** |
| `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` | **clean** |
| `cargo +1.98.0 test --no-fail-fast -- --test-threads=1` | **566 passed / 0 failed / 2 ignored** |
| `real_*` with `--nocapture` | all three execute; evidence in §2 |
| `tsc --noEmit` | **clean** (0 lines of output) |
| `vitest run` | **287 passed / 23 files** |
| `vite build` | **exit 0**, 4.92 s |
| `eslint . --max-warnings=0` | **clean** (0 output, 12 s) |

All four frontend gates are green in this run. Note this environment's `node-brokered-fs-shim` had
been blocking `@tauri-apps/api/core` earlier in the session (a 125 s approval timeout that then caches
a denial); it cleared before these gates ran, so the earlier block is **not** present in these numbers.
The frontend is untouched by Batch 5 in any case.

CI-equivalent command verified locally: `cargo test --no-fail-fast -- --test-threads=1` plus the
exact `--skip` list used by the new CI step (478 lib pass, 1 filtered; each real target reports
`1 filtered out`).

`dist/` was rebuilt by the `vite build` gate and produces the same asset hashes as the pre-existing
build (`index-ChtOoUDC.js`, `index-C5Q8PFJI.css`) — the frontend build is deterministic.

---

## 9. Environment limitations

1. **CI cannot stage libmpv** — structural, described in §4. Consequence: on CI these five tests are
   skipped, explicitly and loudly. They are *not* verified in CI.
2. **The Windows real tests cannot execute anywhere** — no Windows libmpv exists in or buildable by
   this repository. The Windows test fails loudly when run without it.
3. **Windows behaviour after Batch 1 is unverified** (§5). No CI run exists.
4. **`src/media/shared_pipeline.rs` is dead code.** It is referenced nowhere in `src/` — `grep` for
   `mod shared_pipeline` and `shared_pipeline::` returns no matches — so it is never compiled, and its
   `#[ignore]`d test is not even reported as ignored. Relevant to "Provider Shared untouched": it is
   not merely disabled, it is **not in the build at all**.
5. **`eslint` is normally slow here** (the project memory records 3–11 min) but took **12 s** in this
   run once the fs-shim block cleared. Do not treat the old duration as fixed.
6. **`cargo test` without `--no-fail-fast` hides targets.** Used `--no-fail-fast` throughout.

---

## 10. Confirmation of the release freeze

* `v0.9.8` → **`bb22577`** — unchanged.
* All five tags intact: `v0.9.0`, `v0.9.4`, `v0.9.5`, `v0.9.6`, `v0.9.8`.
* `origin/main` → **`bb22577`**. **Nothing was pushed.**
* No force-push, no reset.
* **Provider Shared untouched**: `provider_mode_gate`, the experimental `PROVIDER_SHARED` path and
  `media/shared_pipeline.rs` are unmodified, and per §9.4 the shared pipeline is not compiled at all.
  Nothing was implemented or enabled.

### Files changed by Batch 5

```
 .github/workflows/ci.yml                        | explicit libmpv state + skip reporting
 .gitignore                                      | un-ignore the committed test fixture
 scripts/make-test-media-macos.swift             | new — AVFoundation fixture generator
 scripts/make-test-media-macos.sh                | new — wrapper
 src-tauri/tests/common/mod.rs                   | new — shared prerequisite policy
 src-tauri/tests/fixtures/movie_party_test_320x240.mp4 | new — committed H.264 fixture
 src-tauri/tests/real_playback_smoke_test.rs     | no silent skip; macOS-gated
 src-tauri/tests/real_native_surface_e2e.rs      | no silent skip; flake fixed
 src-tauri/tests/real_sw_render_test.rs          | no silent skip; window/assertion defect fixed
 src-tauri/tests/windows_native_surface_e2e.rs   | no silent skip; committed fixture
 src-tauri/src/media/player/mpv_backend.rs       | lib test: no silent skip; macOS-gated
 BATCH5_REPORT.md                                | new
```

### Side effects

* `/tmp/fixture_hidden.mp4`, `/tmp/libmpv_backup.dylib`, `/tmp/fixture_committed_backup.mp4`,
  `/tmp/gen_test.mp4`, `/tmp/gen6.mp4`, `/tmp/sw_orig.rs`, `/tmp/sw_probe.rs`, `/tmp/e2e_mine.rs`,
  `/tmp/batch5_full.txt` — scratch artifacts from the controls and A/B runs. Every restored file was
  re-verified by `sha256`; the fixture and all 13 dylibs are byte-identical to their originals.
* The committed fixture's origin is the file the audit supplied at `/tmp/movie_party_test.mp4`; the
  committed copy is byte-identical (`sha256 b5609530…`). It was **not** regenerated or altered to make
  any test pass — the SW render fix stands on its own, and was independently verified against a
  freshly generated fixture as well.
* `pyyaml` was installed into the isolated venv at
  `~/.workbuddy-ai/binaries/python/envs/default` to validate the workflow YAML. No global installs.
* `dist/` was rebuilt by the `vite build` gate (untracked/gitignored).
* `~/.workbuddy-ai/skills/movie-party-verify/SKILL.md` was corrected: its §4b claimed the
  `@tauri-apps/api/core` block was "gone" and its pre-existing-failure table listed
  `real_native_surface_e2e` as flaky — both were re-measured here and updated.
