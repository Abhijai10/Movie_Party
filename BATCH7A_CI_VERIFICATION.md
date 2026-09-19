# Batch 7A — Candidate CI Verification Report

**Status: COMPLETE — all five CI jobs green on both platforms. The Batch 1 Windows `test_b` fix is
verified on the real Windows runner.**

Two CI runs were needed. The first exposed a *separate*, Windows-only quality-gate regression that
blocked the test step; that was fixed (with your approval) in a two-line test-only commit, and the
second run passed end to end.

| | Run 1 | Run 2 (authoritative) |
|---|---|---|
| Run ID | `35472905806` | **`35474284919`** |
| SHA | `2d5c83391dec43342bb4346ae19c542a6e672385` | **`a44542af0f0f286c9b387c23d72692a56b966a7a`** |
| Branch | `stabilization/v0.9.9-rc1` | `stabilization/v0.9.9-rc1` |
| Conclusion | ❌ failure (Windows clippy) | ✅ **success** |
| Windows `test_b` | **never ran** (Tests step skipped) | ✅ **`test_b_play_transitions_both_to_playing ... ok`** |

> **The SHA that is actually verified is `a44542a`, not `2d5c833`.** Run 2 tested one commit further
> along the candidate branch. This is stated plainly because it is a real difference: `2d5c833`
> remains CI-unverified on Windows.

**No tag was created or moved, no GitHub Release was created or modified, and `main` was never
pushed.** `v0.9.8` → `bb22577` is intact.

---

## 1. Local state

| Item | Value |
|---|---|
| Starting HEAD | `2d5c833…` (`main`, clean tree, 4 commits ahead of `origin/main`) |
| Final candidate HEAD | `a44542a…` |
| `main` (local and remote) | `bb22577…` — **unchanged throughout** |
| `origin/main` | `bb225778434e3f07923e03e85d7d7cc1db79146c` |

### Commits since v0.9.8

| Commit | Batch(es) | Subject |
|---|---|---|
| `c842fa7` | **Batches 1–3** | fix: harden Chrome process ownership, DB write honesty, and failure propagation |
| `2c9d565` | Batch 4 | harden(Batch 4): network/security hardening + rustls 0.23.45 |
| `aec8e69` | Batch 5 | test(Batch 5): make the automated test suite truthful |
| `2d5c833` | Batch 6 | docs(Batch 6): release body derives from notes; macOS install reality |
| `a44542a` | **Batch 7A** | fix(ci): gate platform-only test imports so Windows clippy passes |

Batches 1–3 were committed together in `c842fa7` (Batch 1's work was still uncommitted when Batch 2
began). There is **no `BATCH1_REPORT.md`** — Batch 1's work is inside `c842fa7`. All five batch
reports are committed.

### v0.9.8 immutability — verified

`v0.9.8` is an **annotated** tag, so `git show-ref --tags` prints the *tag object* (`4fca32c…`), not
the commit. Dereferenced, it is correct:

```
$ git rev-parse v0.9.8^{commit}
bb225778434e3f07923e03e85d7d7cc1db79146c          ✓

$ git cat-file -p v0.9.8
object bb225778434e3f07923e03e85d7d7cc1db79146c
type commit
tag v0.9.8
```

Remote refs after both pushes:

| Ref | SHA | Status |
|---|---|---|
| `refs/heads/main` | `bb22577…` | **unchanged** |
| `refs/tags/v0.9.8` | `4fca32c…` → `bb22577…` | **unchanged** |
| `refs/tags/v0.9.0 / v0.9.4 / v0.9.5 / v0.9.6` | (as before) | **unchanged** |
| `refs/heads/stabilization/v0.9.9-rc1` | `a44542a…` | the only change |

GitHub Releases: `Movie Party v0.9.8` still the latest; **no release created or modified**.
`release.yml` triggers only on `push: tags: ["v*"]`, so a branch push cannot fire a release.

### Batch 1 fix present — verified in source

`SEQUENCE_REORDER_WINDOW = 64` at `src-tauri/src/protocol/mod.rs:249`; absent at v0.9.8, which had
the strict `seq <= last_seq_received` watermark. Introduced by `c842fa7`. At HEAD the tracker is the
bounded sliding replay window that accepts a legitimate cross-stream reorder while still rejecting
duplicates and anything older than the window.

---

## 2. Branch creation, pushes, and the fix

```
$ git branch stabilization/v0.9.9-rc1 2d5c833…            # from stabilized HEAD
$ git push origin refs/heads/stabilization/v0.9.9-rc1:refs/heads/stabilization/v0.9.9-rc1
$ git push origin refs/heads/stabilization/v0.9.9-rc1:refs/heads/stabilization/v0.9.9-rc1   # after the fix
```

Explicit refspecs — **only** that one branch was pushed, no `--tags`, no `main`, no force.

### The fix — `a44542a` (2 files, +10 lines, comments only beyond the attributes)

```diff
--- a/src-tauri/src/media/player/mpv_backend.rs   (~772)
     use crate::media::player::LocalPlayer;
+    #[cfg(target_os = "macos")]
     use std::path::Path;

--- a/src-tauri/src/providers/chrome/mod.rs       (~641)
     use crate::providers::youtube::{is_youtube_url, YoutubeAdapter};
+    #[cfg(unix)]
     use std::process::{Child, Stdio};
```

**Test-only.** No production behaviour is altered; both imports remain present on the platforms that
use them. Verified locally before pushing: `cargo fmt --check` exit 0, and
`cargo clippy --all-targets --all-features -- -D warnings` **exit 0 with zero warnings**.

---

## 3. Run 2 — `35474284919` (authoritative) — ✅ SUCCESS

**SHA `a44542af0f0f286c9b387c23d72692a56b966a7a`** · 2026-09-19T22:46:41Z → 22:56:43Z (~10 min)

| Job | Result | Duration |
|---|---|---|
| **Frontend** | ✅ success | 41 s |
| **Rust (macos-latest)** | ✅ success | 5 m 26 s |
| **Rust (windows-latest)** | ✅ success | **9 m 59 s** |
| Cargo audit (advisory) | ✅ success | 3 m 11 s |
| pnpm audit (advisory) | ✅ success | 17 s |

### 3.1 Windows — PASS (the previously-skipped suite ran)

Every step green, including the two that failed or were skipped in run 1:

| # | Step | Result |
|---|---|---|
| 4 | `cargo fmt --check` | success |
| 5 | `cargo clippy --all-targets --all-features -- -D warnings` | **success** (was FAILURE) |
| 6 | libmpv runtime state | success |
| 7 | **Tests** | **success** (was SKIPPED) |

**The decisive line:**

```
test test_b_play_transitions_both_to_playing ... ok
```

**Per-target results — all 14 targets named (plus the doc-test run):**

| Target | Passed | Failed | Ignored | Filtered out |
|---|---|---|---|---|
| `unittests src/lib.rs` | 469 | 0 | 2 | 0 |
| `unittests src/main.rs` | 0 | 0 | 0 | 0 |
| `tests/dependency_audit.rs` | 3 | 0 | 0 | 0 |
| `tests/host_guest_wiring.rs` | 2 | 0 | 0 | 0 |
| **`tests/m2_integration.rs`** | **28** | **0** | 0 | 0 |
| `tests/m3_closure.rs` | 7 | 0 | 0 | 0 |
| `tests/m3_integration.rs` | 18 | 0 | 0 | 0 |
| `tests/m3_m4_e2e.rs` | 9 | 0 | 0 | 0 |
| `tests/m4_closure.rs` | 15 | 0 | 0 | 0 |
| `tests/real_native_surface_e2e.rs` | 0 | 0 | 0 | 0 |
| `tests/real_playback_smoke_test.rs` | 0 | 0 | 0 | 0 |
| `tests/real_sw_render_test.rs` | 0 | 0 | 0 | 0 |
| `tests/tailscale_probe.rs` | 2 | 0 | 0 | 0 |
| `tests/windows_native_surface_e2e.rs` | 0 | 0 | 0 | 1 |
| Doc-tests | 0 | 0 | 0 | 0 |

**Windows totals: 553 passed / 0 failed / 2 ignored / 1 filtered out.**

Note the asymmetry: on Windows the single filtered-out test is in
`tests/windows_native_surface_e2e.rs` (the test exists there and was skipped for lack of libmpv);
on macOS that target has zero tests, so the skip produces no line — see §3.2.

`m2_integration` = **28 passed / 0 failed** — including `test_a_ready_reaches_host ... ok`,
`test_b_play_transitions_both_to_playing ... ok`, `test_c_pause_is_canonical ... ok`.

### 3.2 macOS — PASS

```
test test_b_play_transitions_both_to_playing ... ok
```

**Per-target results — all 14 targets named (plus the doc-test run):**

| Target | Passed | Failed | Ignored | Filtered out |
|---|---|---|---|---|
| `unittests src/lib.rs` | 478 | 0 | 2 | 1 |
| `unittests src/main.rs` | 0 | 0 | 0 | 0 |
| `tests/dependency_audit.rs` | 3 | 0 | 0 | 0 |
| `tests/host_guest_wiring.rs` | 2 | 0 | 0 | 0 |
| **`tests/m2_integration.rs`** | **28** | **0** | 0 | 0 |
| `tests/m3_closure.rs` | 7 | 0 | 0 | 0 |
| `tests/m3_integration.rs` | 18 | 0 | 0 | 0 |
| `tests/m3_m4_e2e.rs` | 9 | 0 | 0 | 0 |
| `tests/m4_closure.rs` | 15 | 0 | 0 | 0 |
| `tests/real_native_surface_e2e.rs` | 0 | 0 | 0 | 1 |
| `tests/real_playback_smoke_test.rs` | 0 | 0 | 0 | 1 |
| `tests/real_sw_render_test.rs` | 0 | 0 | 0 | 1 |
| `tests/tailscale_probe.rs` | 2 | 0 | 0 | 0 |
| `tests/windows_native_surface_e2e.rs` | 0 | 0 | 0 | 0 |
| Doc-tests | 0 | 0 | 0 | 0 |

**macOS totals: 562 passed / 0 failed / 2 ignored / 4 filtered out.**

The 4 filtered-out tests on macOS are exactly `bundled_runtime_is_loadable` (lib, macOS-gated) plus
one each in `real_native_surface_e2e`, `real_playback_smoke_test` and `real_sw_render_test`.
macOS carries 9 more lib tests than Windows (478 vs 469) — the `#[cfg(target_os = "macos")]`-gated
tests that legitimately do not exist on Windows.

### 3.3 Frontend — PASS

`eslint . --max-warnings=0` clean · **23 test files, 287 tests passed** · `vite build` ✓ (2.64 s).
Identical to run 1 — the fix touched only Rust files.

### 3.4 Dependency / security audits

| Job | Run 2 | Run 1 | Baseline (v0.9.8) |
|---|---|---|---|
| Cargo audit (advisory) | ✅ | ✅ | ❌ "1 vulnerability found!" |
| pnpm audit (advisory) | ✅ | ✅ | ✅ |

Batch 4's rustls `0.23.45` bump cleared the advisory that was red at v0.9.8. Both jobs remain
`continue-on-error: true` (advisory by design).

---

## 4. Run 1 — `35472905806` — the Windows clippy regression

Recorded because it is a genuine finding about the stabilized code, not just a step on the way.

**SHA `2d5c833…`** → **failure.** The Windows job died at step 5 and the Tests step was **skipped**,
so `test_b` never executed. Verbatim:

```
error: unused import: `std::path::Path`
   --> src-tauri\src\media\player\mpv_backend.rs:772:9

error: unused imports: `Child` and `Stdio`
   --> src-tauri\src\providers\chrome\mod.rs:641:24

error: could not compile `movie-party` (lib test) due to 2 previous errors
```

**Classification:** new, deterministic, **Windows-only**, **test-only** regression introduced by the
stabilization work itself. Not the sequence/reordering defect, not a flake, not infrastructure, not a
timeout. At v0.9.8 the Windows job *passed* clippy and reached the tests — so this was a real
regression, and Windows test coverage for `2d5c833` is zero.

### Root cause

Both are imports in `#[cfg(test)]` modules used **only** by code Windows does not compile:

**`mpv_backend.rs:772`** — `use std::path::Path;`. Every use of the imported symbol sits inside a
`#[cfg(target_os = "macos")]` test (lines 783, 801, 840). *Introduced by `aec8e69` (Batch 5)*, which
renamed `bundled_runtime_is_loadable_when_present` → `bundled_runtime_is_loadable` and gated it to
macOS so it would stop reporting a false `ok`. That correct change is what made the import unused.

**`chrome/mod.rs:641`** — `use std::process::{Child, Stdio};`. Both symbols are used only by
`#[cfg(unix)]` helpers (`spawn_isolated` 647–653, `session_over` 660). *Introduced by `c842fa7`
(Batches 1–3)* — the import does not exist at v0.9.8.

**Why it was invisible locally:** both predicates are *true* on macOS, so macOS clippy sees the
imports as used. Only a Windows runner compiles the test target without them.

**Local reproduction — attempted, not possible.** `x86_64-pc-windows-msvc` is installed for 1.98.0,
but `cargo clippy --target x86_64-pc-windows-msvc` dies in build scripts for `ring` and
`aws-lc-sys` (no Windows C toolchain on this Mac). The CI log plus source analysis were the evidence;
that limitation is stated rather than worked around.

**Why the failure looked like a test failure:** the Windows leg finished in ~3 m 40 s versus ~9 min
when it reaches the tests. A much faster Windows leg means an early step failed — check the step
table, not just the job conclusion.

---

## 5. Skipped and ignored tests

### Explicitly skipped by CI (no bundled libmpv runtime)

`src-tauri/mpv_runtime/*` is gitignored, so a clean CI checkout has no libmpv and cannot build one.
The `libmpv runtime state` step set `BUNDLED_LIBMPV=0` and emitted a `::warning` plus
`EXPLICIT SKIP: the 5 libmpv-dependent tests above did not execute (no libmpv runtime in this job).
The committed media fixture they need IS present; only the runtime is missing.`

| Test | Target |
|---|---|
| `bundled_libmpv_plays_pauses_seeks_real_video` | `real_playback_smoke_test` |
| `bundled_libmpv_renders_onto_real_calayer_through_production_player` | `real_native_surface_e2e` |
| `bundled_libmpv_sw_render_api_produces_decoded_frames` | `real_sw_render_test` |
| `bundled_libmpv_renders_onto_real_child_hwnd_through_production_player` | `windows_native_surface_e2e` |
| `bundled_runtime_is_loadable` | lib target (macOS-only) |

Since Batch 5 these **fail loudly** when the runtime is missing, so the skip is explicit rather than
a false green. `bundled_libmpv_path_resolves_correctly_inside_app_bundle` is deliberately **not**
skipped — pure path-string logic, and it runs.

### Ignored (`#[ignore]`) — 2 tests, both platforms

| Test | Reason |
|---|---|
| `real_chrome_launches_and_evaluates_cdp` | launches real Chrome, localhost CDP session |
| `real_youtube_provider_sync_uses_chrome_cdp` | launches real Chrome, live YouTube page |

A third `#[ignore]` exists — `macos_shared_pipeline_captures_encodes_transports_and_decodes`
(`src/media/shared_pipeline.rs:222`) — but it **never compiles**: `shared_pipeline.rs` is not
declared in `media/mod.rs`, so it is dead code. That is why the count is 2, not 3.

---

## 6. Remaining CI limitations

1. **No real playback is verified in CI.** The libmpv skip is structural — CI cannot stage the
   runtime. **No automated gate in this repo has ever proven real decoded video frames on real
   hardware.** Only a manual two-device run can (see `BATCH7B_REAL_BETA_VALIDATION.md`).
2. **`2d5c833` itself remains unverified on Windows.** The green result belongs to `a44542a`.
3. **Real Chrome is never exercised** — both CDP tests are `#[ignore]`d.
4. **Advisory audits cannot fail the build** (`continue-on-error: true` by design).
5. **A runner is not the user's machine.** Green CI proves the code passes on GitHub-hosted VMs —
   not on a real Windows desktop with real Tailscale and real media. That is Batch 7B's job.
6. **`build.yml` (Windows installer) was not dispatched** — manual-only, out of scope here.

---

## 7. Exact next action

**Batch 7A is complete: the Batch 1 fix is verified on Windows.** The original failure —
`host predicate not satisfied within 30s` from the strict sequence watermark — does not reproduce
with the bounded reorder window on the actual Windows runner.

Next, in order:

1. **Batch 7B** — run the two-device validation procedure in
   `BATCH7B_REAL_BETA_VALIDATION.md`. Automated gates now prove the code compiles and its unit and
   integration suites pass on both platforms; **they still do not prove anyone can watch a movie
   together.**
2. **Decide the fate of the candidate branch.** `stabilization/v0.9.9-rc1` @ `a44542a` is green on
   both platforms. If it is to become v0.9.9, that is a *release* batch: version bump, tag, release
   — none of which was done here and none of which should be done without an explicit instruction.
3. **Consider committing `BATCH7A_CI_VERIFICATION.md` and `BATCH7B_REAL_BETA_VALIDATION.md`.** Both
   are currently **untracked** — deliberately, so the tested SHA stayed clean. The project
   convention has reports shipping inside commits.

**Do not** retag `v0.9.8`, modify its release, or push `main` — none of that was done, and none of it
is part of this batch.

---

## 8. Batch 7A compliance checklist

| Requirement | Status |
|---|---|
| Verify local HEAD, working tree, commit history | ✅ §1 |
| Confirm Batches 1–6 present | ✅ §1 |
| Confirm v0.9.8 still points to `bb22577` | ✅ §1 (annotated tag dereferenced) |
| Create `stabilization/v0.9.9-rc1` from stabilized HEAD | ✅ §2 |
| Push **only** that branch | ✅ §2 |
| Do **not** push `main` | ✅ `origin/main` = `bb22577` |
| Do **not** create or modify any tag | ✅ 5 tags unchanged |
| Do **not** create a GitHub Release | ✅ releases unchanged |
| Dispatch the existing CI workflow against the branch | ✅ §3 |
| Wait for the complete CI result | ✅ 5/5 jobs, both runs |
| **Verify Windows `test_b`** | ✅ **`... ok`** (§3.1) |
| Verify macOS CI / frontend / fmt / clippy / cargo test / all integration targets | ✅ §3.1–3.3 |
| Verify dependency/security audit jobs | ✅ §3.4 |
| Report explicit libmpv skip | ✅ §5 |
| Do not hide, reinterpret, or suppress failures | ✅ §4 reported verbatim, including the run that failed |
| Do not increase timeouts to force a pass | ✅ no timeout was ever changed |
| Production code unchanged | ✅ the only change is two test-only `cfg` attributes |
| Stop and report before fixes | ✅ run 1 was reported in full before any code was touched |

*Batch 7A — verification plus one approved test-only fix. Provider Shared untouched. No tag, release,
or `main` was modified.*
