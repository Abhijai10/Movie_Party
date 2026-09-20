# Batch 11 — closing AUD-08 and AUD-16

Stable-scope work on the v0.9.9 beta candidate, branch `stabilization/v0.9.9-rc1`.
Nothing was tagged, pushed, or released. v0.9.8 is untouched.

---

## 0. Baseline, verified before anything changed

| Claim | Verified how | Result |
|---|---|---|
| HEAD is the stabilization candidate | `git rev-parse HEAD` | `ade8a4f2b2bb695dc78e5c56cae7ab2a6f113240` |
| The code is byte-identical to CI-verified `03673a3` | `git diff --stat 03673a3..HEAD` | **2 files, both markdown** (`POST_REMEDIATION_ADVERSARIAL_AUDIT.md`, `docs/RELEASE_PROCESS.md`). No source, no config, no test changed. The claim holds. |
| Pristine test baseline | `cargo test -- --test-threads=1` on a stashed tree | **581 passed / 0 failed / 2 ignored**, 18 targets |
| Frontend baseline | documented in Batch 7A / 8 | **287 tests / 23 files** |

Two tests in the *lib* target fail under the default parallel run on this machine
(`providers::chrome::*`, process-tree tests) and pass in isolation. That is load
sensitivity, not a defect: CI itself serializes (`--test-threads=1`), so every
count here — baseline and final — was taken the CI way. It is reported rather
than smoothed over because the two runs are not comparable otherwise.

---

## 1. AUD-08 — the orphaned `shared_pipeline.rs`

### What was proved first

* `src-tauri/src/media/shared_pipeline.rs` was **not declared** in `media/mod.rs`,
  so no build compiled it and its single `#[ignore]`d test could never run.
* The only references to it anywhere in `src-tauri/` were **inside itself** and in
  the `media/mod.rs` comment that documented the finding.
* `send_shared_stream_packet` existed **nowhere** in the crate — the shared-stream
  transport API was gone from `network/quic.rs` entirely. That is what made the
  file *orphaned* rather than stale: the capability it existed to prove had been
  removed.
* Repairing it would have meant either gutting the transport assertion (the only
  thing the harness proved) or rebuilding a deleted send path — Provider Shared
  implementation work, explicitly out of scope.

### Result

**Deleted.** `git rm src-tauri/src/media/shared_pipeline.rs` (276 lines). The
`media/mod.rs` comment block was rewritten from "AUD-08 (NOT remediated)" to a
short "AUD-08 (CLOSED)" note that records why deletion was the honest option and
still states plainly that **Provider Shared is not implemented and stays
disabled**. Nothing was repaired, no transport was added, `shared_available` was
not touched.

### Residual finding (new, reported not hidden)

`media/shared_stream.rs` and the `encode` module were reachable **only** through
the deleted file. They are still declared `pub` in `lib.rs` / `media/mod.rs`, so
they still compile, their own unit tests still run, and `clippy -D warnings`
stays clean — but they now have no consumer outside their own tests. They were
**deliberately not deleted**: they contain real, tested logic, and removing them
is a much larger change than this batch authorises. Flagged for a later,
deliberate decision.

---

## 2. AUD-16 — end-of-media UX

### The defect, precisely

The backend had been truthful since AUD-03 (room `ENDED`, player `COMPLETED`).
Nothing surfaced it, so the end of a film looked like a stalled player:

* no message at all — `screen` stayed `CINEMA`;
* the sync indicator read **"Syncing"**, because it was a two-branch
  `PLAYING ? In sync : Syncing` and an ended room fell into the second branch;
* the play control drew its **Play** icon, i.e. *resume*. Pressing it ran
  `roomState === "PLAYING" ? pausePlayback : resumePlayback`, which at `ENDED`
  calls `resumePlayback` → `host_play`.

### The backend hole behind the last one

`prepare_play_scheduled` validates **readiness, not the current state**, so a
play from `Ended` fires `RoomState::ReadyCheck` and schedules a play from the end
position. **Measured, not argued:** with the guard removed, the regression test
reports the room at `READYCHECK` — the room really is resurrected.

### What was built

| Requirement | Implementation |
|---|---|
| Cinema screen stays visible | unchanged |
| Final frame stays visible | unchanged; the card is a centred overlay that does not cover the frame |
| "Movie Finished" / "The movie has ended." / "Back to Lobby" | `src/overlays/MovieFinishedOverlay.tsx`, `role="status"` (an announcement, not a modal) |
| disable play/pause/seek at ENDED | real `disabled` on the play button and both seek buttons |
| no `host_play` from ENDED | two guards: the `disabled` attribute, and `playbackToggleAction(...) === "NONE"` returning before any command |
| no buffering overlay at ENDED | `shouldShowBufferingOverlay` suppresses it from **either** input |
| no "Syncing" at ENDED | `syncIndicatorFor` returns `"NONE"`; the indicator is omitted |
| no replay semantics invented | no replay affordance exists, and the copy is asserted not to imply one |
| no redesign / no dashboard | one new card in the existing cinematic language; no new utilities |

Also suppressed at `ENDED`: the `movie-prep-state` block, which would otherwise
have said **"Now screening"** directly beside "Movie Finished". A deliberate,
minimal addition in the same family as the reported defect — a contradictory
claim about the movie's state.

Decisions live in `src/views/cinemaEndState.ts` as **pure functions**, because
this repo has no DOM test environment and adding one for a single batch is not
warranted. That is the same pattern as `reconnectLatchAfter`,
`cinemaDockInteractionClass` and `overlayState`.

`styles.css` is a pre-built Tailwind output with **no Tailwind dependency or
config in the repo**, so a new utility class would simply not exist. The new
styles are hand-written, like `.buffer-overlay` and `.privacy-toast`.

---

## 3. Tests added — and the controls that prove they can fail

**2 Rust tests, 30 frontend tests (2 new files).**

Every claim below was checked by breaking the thing it guards.

| # | Test | Negative control | Mutation result |
|---|---|---|---|
| 1 | ENDED renders the finished state | no other room state is treated as finished | untouched helper stayed green — the mutation was targeted |
| 2 | ENDED does not render the buffering overlay | the overlay still works in live rooms | **RED** (2 tests) when the `ENDED` branch was removed |
| 3 | ENDED exposes no active play/resume action | the old inline expression is reproduced and asserted to yield `"PLAY"` | **RED** (2 tests) when reverted |
| 4 | Back to Lobby performs the existing path | `leave_party` is asserted to be a *different* command | mocked `invoke` distinguishes them |
| 5 | the deleted file has no live references | checker must detect a live declaration **and** ignore a comment-only mention | **RED** (2 tests), naming `media/mod.rs`, when `pub mod shared_pipeline;` was re-added |
| 6 | Provider Shared remains disabled | a synthetic activation site is caught by the same predicate | existing Rust tests already cover the gate behaviourally |
| — | sync indicator never claims to sync an ended room | old two-branch expression reproduced | **RED** (2 tests) when reverted |
| — | the **wiring** the pure decisions cannot reach | `onLeave` is asserted to exist elsewhere in the file | **RED** (1 test) when the seek buttons' `disabled` was missing |

The last row earned its place immediately. Two edits were applied in one batch
and one silently clobbered the other, so the **seek buttons were never
disabled** — a direct violation of the requirement, invisible to every pure
decision test and to `tsc`. The wiring assertions caught it, and a *second* real
problem with them: the finished card's own comment mentions `onLeave`, so the
`not.toContain("onLeave")` check was a false positive until comments were
stripped. Both are fixed.

**Not covered, stated plainly:** nothing renders CinemaView in a test. The
component-level wiring is guarded by source assertions, which is weaker than
rendering it. The alternative was adding a DOM testing framework for one batch,
which the brief ruled out.

---

## 4. Gates

| Gate | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean, exit 0 |
| `cargo test -- --test-threads=1` | **exit 0** — 583 passed / 0 failed / 2 ignored, 18 targets |
| `vitest run` | **25 files / 317 tests passed** |
| `eslint . --max-warnings=0` | clean |
| `tsc --noEmit` | clean, exit 0 |
| `vite build` | ✓ 2284 modules, built in 10.93 s |
| version consistency (all four declarations) | ✓ all `0.9.8` |

### 4a. A flake this batch introduced — found after the first commit, and fixed

The frontend row above was measured **before** the work was committed, and it was
true at the time. Re-running the full suite against the *committed* tree exposed a
load-sensitive test that this batch had added:

```
× has no live reference anywhere in the crate   6437ms
✓ its obsolete transport API has no live reference either   3ms
✓ the audit is reading real source, not an empty set   1ms
```

`has no live reference anywhere in the crate` walks all 97 `.rs` files under
`src-tauri/src`. That costs **~2.5 s idle but ~6.4 s under full-suite load**
(vitest runs files in parallel; the workspace is on an external volume), which
crosses vitest's **5 s** default per-test timeout.

Two things made this worth reporting rather than quietly patching:

* **The signature was misleading in the dangerous direction.** The test failed
  while the next three assertions — reading the *same* memoised cache — reported
  1–3 ms. That combination means the walk **completed** and the test was aborted
  for time, not that it found an offender. Read the other way, it looks like a
  live reference to `shared_pipeline` survived the deletion.
* **A single-file run hid it completely** — `17 passed` at 2555 ms. Only the full
  suite reproduced it, and it reproduced twice.

Fixed by warming the cache in a module-level `beforeAll(…, 30_000)`, so the cost
is declared rather than left to a default that does not describe it. This is not
a timeout raised to conceal a defect: the assertions are pure scans with no timing
semantics, and the failure mode is "the machine was busy".

Re-verified after the fix, full suite: **25 files / 317 tests passed**, the
affected file at 8224 ms. Test counts are unchanged — no test was added, removed
or skipped.

Because the distinction mattered here, **every remaining frontend gate was then
re-run against the committed tree** rather than left on the pre-commit numbers in
the table above:

| Gate, re-run at HEAD | Result |
|---|---|
| `tsc --noEmit` | exit 0, no output |
| `eslint . --max-warnings=0` | exit 0, no output (1 m 32 s) |
| `vite build` | exit 0, **2284 modules**, 12.61 s |
| `vitest run` | 25 files / 317 tests passed |

The Rust gates were not re-run, and do not need to be — `git diff 07cf4fe..HEAD
-- src-tauri/` is **empty**, so the Rust content at HEAD is byte-identical to what
`fmt`, `clippy` and `cargo test` measured. That is a mechanical check, not an
assumption. `dist/` is gitignored, so rebuilding it left the tree clean.

### Count reconciliation

| | Baseline | Final | Δ | Explained by |
|---|---|---|---|---|
| Rust | 581 / 0 / 2 | 583 / 0 / 2 | **+2** | exactly the two new tests (lib 488 → 490) |
| Frontend | 287 / 23 files | 317 / 25 files | **+30 / +2** | exactly the 13 + 17 new tests in the two new files |

No test was skipped, renamed, or removed to reach these numbers. Deleting the
orphan changed **no** count, as expected: its only test was in a module no build
compiled.

---

## 5. Did anything unrelated change?

**No.** Source changes are confined to the two audited items:

* `src-tauri/src/media/shared_pipeline.rs` — deleted
* `src-tauri/src/media/mod.rs` — comment only
* `src-tauri/src/app_runtime.rs` — one guard in `host_play` + two tests
* `src/components/mp/CinemaControls.tsx` — `playbackDisabled` prop
* `src/views/CinemaView.tsx` — the ENDED wiring
* `src/styles.css` — appended rules
* `src/overlays/MovieFinishedOverlay.tsx`, `src/views/cinemaEndState.ts` — new
* `src/views/cinemaEndState.test.ts`, `src/views/endOfMediaContracts.test.ts` — new

Deliberately **not** touched: `apply_end_of_media` and the ADV-05 decision not
to set `strict_sync_paused` at EOF; `provider_mode_gate`; `provider_capabilities`;
`network/quic.rs`; the `#[ignore]`d tests; `shared_stream.rs`; `encode`.

The historical audit reports (`DEEP_PRODUCTION_READINESS_AUDIT.md`,
`POST_REMEDIATION_ADVERSARIAL_AUDIT.md`, `BATCH8_AUDIT_REMEDIATION.md`) still
describe AUD-08 and AUD-16 as open. They are **dated records of what was true
when written** and were left unedited on purpose; this file is the closure
record. If they should instead be updated, that is a separate, deliberate edit.

---

## 6. Provider Shared

**Untouched and still disabled.**

* `provider_capabilities()` still returns `shared_available: false` for all four
  providers.
* `provider_mode_gate` is unchanged: `PROVIDER_SHARED` without a verified
  diagnostic → `SharedUnverified`; **and even with one → `Unsupported`**. The
  double gate still holds, so the diagnostic can never activate the mode.
* Nothing under `media/` sets `shared_available: true` or names `PROVIDER_SHARED`
  — asserted by a test.
* No capture, encoding, encryption, QUIC media streaming, browser interception
  or DRM handling was added. The `capture/diagnostic.rs` `shared_available: true`
  is the pre-existing diagnostic outcome and is unreachable as an activation.

The repository still states plainly, in `media/mod.rs`, that Provider Shared is
not implemented.

---

## 7. Remaining, stated up front

1. **No component rendering test.** The wiring is guarded by source assertions.
2. **`media/shared_stream.rs` and `encode` are now unconsumed.** Still compiled
   and tested; deleting them is a larger decision.
3. **The countdown path is unguarded at `ENDED`.** `request_play_countdown` also
   calls `prepare_play_scheduled`, so it shares the same shape of hole. It is not
   reachable while `screen` is `CINEMA` at `ENDED`, which is why the brief's
   "smallest correct guard" was scoped to `host_play` — but it is a real
   asymmetry and is recorded here rather than left to be discovered.
4. **CI has not run this.** Everything above is a local measurement on macOS with
   the libmpv runtime staged. The CI job runs without that runtime and skips nine
   named tests — but only **eight** of them exist on macOS
   (`bundled_libmpv_renders_onto_real_child_hwnd_through_production_player` is
   Windows-only), so a CI-style run here measures **575 passed / 0 failed / 2
   ignored** against the unskipped **583**. Measured, not inferred: the
   `--skip`-listed run reports exactly `8 filtered out`.
5. **Two process-tree tests are load-sensitive locally** and are only reliable
   serialized. Unchanged by this batch.

---

## 8. Where this work landed

| | |
|---|---|
| Baseline HEAD (CI-verified, unchanged) | `ade8a4f2b2bb695dc78e5c56cae7ab2a6f113240` |
| AUD-08 + AUD-16 code and tests | `07cf4fe9a235ab1c79f5a1a146f0d84142f285be` |
| This report, first version | `dae7f01a87e5aa61913f1072232776b688d13eb8` |
| Fix for the flake in §4a | `48668625bee4b9d676032c096b0258d04a9f99cb` |
| This report, with §4a added | the commit immediately following the above |

The last two entries are stated as a range rather than a single SHA on purpose:
a document cannot name the commit that contains it, so the row that describes the
amendment points at its position in the history instead of at a hash it cannot
know. The substantive work is `07cf4fe`.

Branch `stabilization/v0.9.9-rc1` only. Nothing was pushed, tagged, or released;
`main` and `v0.9.8` are untouched.
