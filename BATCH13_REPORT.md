# BATCH 13 — Real Two-Device Beta Validation + Final Read-Only Release Audit

**Date:** 2026-09-20 / 2026-09-21 (IST)
**Branch:** `stabilization/v0.9.9-rc1`
**Outcome:** **NO-GO**

---

## 0. Verdict and the one-line reason

> **NO-GO.** The release-critical beta evidence this batch exists to produce **was not produced**,
> because two of the three physical-validation parts require hardware this environment does not have.
> The read-only audit (Part D) **did** complete, and it found **no P0, no P1, and no unresolved
> stable-scope P2** — but a clean audit cannot substitute for two-device evidence that was never
> gathered. **Batch 14 must not run.**

**The candidate SHA is `746775dfc72a0b7db3c37d0e1b5fca8a14895726`.**

This is **not** the Batch 12 candidate (`0c2b5e86…`). It moved because the owner reported a real
disruption mid-batch and asked for it to be fixed; that fix is a source change. See §1.

---

## 1. The candidate SHA moved — why, and what it means

While this batch was running, the owner reported:

> "if you are using chrome so it is quiting unexpectedly and im seeing an error page on my window"

**The cause was this batch's own work.** The two `#[ignore]`d real-Chrome tests drive the *production*
launch plan, which deliberately has **no `--headless`** (the provider page has to be visible to the
user). Running them with `--ignored` opened two **real, visible Chrome windows** on the owner's
desktop — one `about:blank`, one the live YouTube page for ~13 s — which then closed on their own.
From the desktop that is indistinguishable from Chrome crashing.

### What was and was not touched

| Question | Answer | Evidence |
|---|---|---|
| Was the owner's real Chrome profile affected? | **No** | Tests use an isolated `--user-data-dir` under `$TMPDIR`; no crash marker in `~/Library/Application Support/Google/Chrome/` |
| Could teardown have killed the owner's own Chrome? | **No** | `terminate_tree` → `killpg(pgid)` on a group the child created itself via `process_group(0)`, guarded by `pgid <= 1 \|\| pgid == own_process_group()`. Windows uses a job object / `taskkill /PID <pid> /T`, explicitly not `/IM chrome.exe` |
| Is the shutdown graceful? | **Yes, graceful-first** | CDP `Browser.close` → 3 s `GRACEFUL_CLOSE_TIMEOUT` → SIGTERM to the group → `TREE_EXIT_GRACE` → SIGKILL. The hard signal is deliberate and fires *after* the browser has exited, to reap renderer/GPU children (MP-22). **It was not removed.** |
| Orphaned processes after the run? | **Zero** | `pgrep "Google Chrome"` → 0 |

### The fix — `746775d`, strictly test-only

1. `require_visible_chrome_consent()` — both real-Chrome tests now **panic** unless
   `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1`. `--ignored` alone fails loudly instead of popping windows
   onto someone's desktop (the AUD-07 rule: an asked-for test must produce a verdict).
2. `TempProfile` guard — replaces five `let _ = std::fs::remove_dir_all(root)` calls that **swallowed
   every failure** *and* only ran on the success path. A panicking test leaked its profile silently;
   that is the origin of the `movie-party-chrome-*` directories dated **2026-09-19** still in `$TMPDIR`.
3. `temp_profile_is_removed_even_when_the_test_panics` — panics *inside* the guard and asserts cleanup
   anyway. **Proven as a real control:** disabling the `Drop` impl turns it red
   (`FAILED. 0 passed; 1 failed`, exit 101, naming the leaked path). Restored, hash-verified.

**Proof it is test-only:** every diff hunk carries the `mod tests {` context and the lowest changed
line is **647**, while `mod tests` begins at **638**. The launch plan, teardown and signal handling are
untouched.

**Consequence:** `git diff --stat 0c2b5e8..HEAD -- src/ src-tauri/` is **no longer empty**
(1 file, +124/−25). Batch 12's candidate identity is superseded. Any prior audit or validation that
names `0c2b5e8` now names a superseded commit.

---

## 2. PART A — Real two-device validation: **BLOCKED**

**BLOCKED — no second physical device, and Tailscale is not running on this one.**

The batch requires "two genuinely separate physical devices", both with Tailscale installed and
authenticated, both running the candidate. What exists:

```
$ /usr/local/bin/tailscale status
Tailscale is stopped.                     ← device A's own readiness is unsatisfiable
$ uname -m ; sw_vers
arm64
ProductName:    macOS
```

**Rows that could not be attempted — all 28, marked NOT TESTED:**

| # | Test | Status |
|---|---|---|
| 1 | Tailscale readiness | **NOT TESTED** — Tailscale stopped |
| 2 | Real peer verification | **NOT TESTED** — no second device |
| 3–4 | Party creation / invite / join | **NOT TESTED** — needs two devices |
| 5 | Real movie transfer/availability | **NOT TESTED** |
| 6–7 | Real video / audio playback | **NOT TESTED** — needs two devices to be meaningful |
| 8–9 | Countdown / play synchronisation | **NOT TESTED** |
| 10 | ≥10 min continuous playback | **NOT TESTED** |
| 11 | Feature-length playback | **NOT TESTED** |
| 12–17 | Pause / resume / seek ×3 / repeated seeks | **NOT TESTED** |
| 18 | Buffering behaviour | **NOT TESTED** |
| 19 | Network disruption / reconnect | **NOT TESTED** |
| 20 | Late join | **NOT TESTED** |
| 21–24 | Watch to end / ENDED / no false buffering overlay / controls do not resume an ended movie | **NOT TESTED** |
| 25–26 | Both users hear audio / obvious A/V sync | **NOT TESTED** |
| 27 | Observed playback drift | **NOT TESTED** |
| 28 | Session stability | **NOT TESTED** |

**Zero PASS results are recorded in this report. Zero screenshots or video evidence exist.** Nothing
was inferred, approximated, or marked green on the strength of a unit test.

**Required metadata that was never gathered:** OS/version per device, movie/container/codec, duration,
audio presence, Tailscale path, observed drift, buffering events, reconnect events, end-of-media
behaviour.

**A real feature-length movie with audio was not used** — the batch forbids the 3-second synthetic
fixture, and no feature-length media was supplied.

---

## 3. PART B — Windows runtime: **BLOCKED**

**BLOCKED — no Windows device is reachable from this host.**

Every item is **NOT TESTED**: installer installation; launch; Tailscale detection; real movie playback;
audio; end-of-media; end-party; Chrome process cleanup; orphaned helper/Chrome processes;
uninstall/reinstall repeat.

This is explicitly **not** evidenced by CI. `build.yml` runs **no tests and has no version gate**; a
successful Windows *build* — which Batch 12 obtained — proves the Windows build works, not that
Windows *behaviour* works. The batch says so directly: "CI success is not sufficient evidence for
these."

---

## 4. PART C — Real provider / Chrome: **PARTIALLY VALIDATED**

This is the one part of the physical validation that could be advanced here, and it was.

Both `#[ignore]`d tests **pass** with the sandbox lifted (they fail *sandboxed* — the CDP socket dies
mid-read; that is an environment artefact, established by A/B, not a product defect):

| Test | Sandboxed | Sandbox lifted |
|---|---|---|
| `real_chrome_launches_and_evaluates_cdp` | fail (`evaluate: Io("failed to fill whole buffer")`) | **ok — 3.65 s** |
| `real_youtube_provider_sync_uses_chrome_cdp` | fail | **ok — 13.15 s** |

| Requirement | Result |
|---|---|
| Chrome launches through the managed process | **PASS** — real Chrome, real CDP session |
| Generation/reinsertion works | **PASS** — covered by the CDP test |
| Playback position is actually observed | **PASS** — `get_buffer_state` evaluated on the live YouTube page |
| Seek actually moves playback | **PASS** — exercised against the live page |
| **Two peers observe the same provider position** | **NOT TESTED — needs two devices** |
| Teardown leaves no Chrome processes behind | **PASS** — `pgrep` → 0 after both runs, and after the whole module |

**Provider Shared was not tested and remains disabled.** All four providers report
`sync_available: true` and `shared_available: false` (`providers/sync.rs`, explicit test at line 637).
No ignored test was converted into a fake PASS.

**Remaining limitation, stated plainly:** a single-machine Chrome run proves the *mechanism*. It does
not prove that two peers converge on the same position, which is the actual beta promise.

---

## 5. PART D — Final read-only audit: **COMPLETE, NO P0 / NO P1**

Audited read-only at `746775d`. No code was modified by this part. Every item was verified against
source, not accepted from the audit documents.

### 5.1 The required checks

| Requirement | Result | Evidence |
|---|---|---|
| No P0 | **PASS** | none found |
| No P1 | **PASS** | none found |
| No unresolved stable-scope P2 on the core promise | **PASS** | the open items are test-suite/behavioural, listed in §6 |
| **AUD-08 closed by deletion, not by resurrecting Shared** | **PASS** | `media/shared_pipeline.rs` deleted in `07cf4fe`; zero live references; a regression test asserts its absence; `shared_available: false` everywhere |
| **AUD-16 correctly surfaced** | **PASS** | `MovieFinishedOverlay.tsx` (`role="status"`), `cinemaEndState.ts` pure guards, `cinemaEndState.test.ts` + `endOfMediaContracts.test.ts` — **30 tests pass** |
| **ADV-01** remains fixed | **PASS** | `clock_calibrated` guard `app_runtime.rs:5437`; clamp at 5352–5355 |
| **ADV-02** remains fixed | **PASS** | `committed_playback = None` in `leave_party` (6576) and fresh-session path (3176); regression test `leaving_a_party_clears_the_play_anchor` |
| **ADV-03** remains fixed | **PASS** | shared reusable workflow called by `ci.yml:16` **and** `release.yml:47`; `release.yml:74` `needs: [resolve-matrix, version-consistency]` |
| **ADV-05** remains fixed | **PASS** | `paused_by_strict_sync` guard + regression test at 10392 |
| **ADV-06** remains fixed | **PASS** | `release.yml:128–134` assigns separately, guards with `::error` + `exit 1`, then exports (the SC2155 fix) |
| **ADV-07** remains fixed | **PASS** | `fixture_properties.rs` pins the fixture; generator header states it does not reproduce it |
| **ADV-08** remains fixed | **PASS** | Windows DLL SHA-256 `7310560B…` in **both** `build.yml:54` and `release.yml:155` |
| **ADV-09** remains fixed | **PASS** | **6** `fetch_verified` calls = **6** SHA-256 pins in `build-libmpv-macos.sh` |
| **ADV-10** | **DOES NOT EXIST** | the ADV series is **01–09**; the batch's list names a phantom item. Not "verified" — recorded as absent rather than quietly satisfied |
| Version gate protects the real release path | **PASS** | see ADV-03 — the gate is on `release.yml` and the release job `needs` it |
| Windows DLL integrity check exists | **PASS** | see ADV-08 |
| macOS source integrity checks exist | **PASS** | see ADV-09 |
| Release documentation is current | **PASS** | `RELEASE_NOTES.md` has a `## 0.9.9` section; `docs/RELEASE_BODY_FOOTER.md` documents Gatekeeper (ad-hoc signed, **not** notarized, all three wordings + the control-click walkthrough) and "Updates — there are none, by design" (no updater plugin, no key, no `latest.json`) |
| Provider Shared remains disabled | **PASS** | see AUD-08 |
| No secret leakage | **PASS** | zero hits for token prefixes (`ghp_`/`gho_`/`ghs_`/`github_pat_`/`sk-`/`AKIA`/`xox[baprs]-`/`BEGIN … PRIVATE KEY`) and for entropy-bearing secret assignments. **The pattern was proven to fire first** — a token planted inside the repo was found by the same `git grep` invocation form |
| No accidental v0.9.8 modification | **PASS** | tag object `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3` local **and** remote; dereferences to `bb225778…`; remote `main` unchanged at `bb225778434e3f07923e03e85d7d7cc1db79146c` |
| **Candidate SHA exactly matches the manually tested SHA** | **FAIL** | **this is the blocker** — no manual two-device test happened at any SHA, and the candidate moved to `746775d` mid-batch (§1) |

### 5.2 Diff audited

The complete diff from `v0.9.8` to the candidate, plus the incremental diff introduced by §1
(`0c2b5e8..746775d`, one file, test-only). No P0, no P1.

---

## 6. New findings raised by this batch (not fixed — each belongs in its own batch)

### 6.1 `cargo test` is deterministically RED in parallel

| Command | Result |
|---|---|
| `cargo test --all-features` (default parallelism) | **2 failed**, 489 passed |
| `cargo test --all-features -- --test-threads=1` | **491 passed, 0 failed** |
| `cargo test -- --test-threads=1` (**CI's exact command**) | **491 passed, 0 failed** |

Both failures are **pre-existing, not caused by §1** — the pristine tree fails the *same two* (488
passed + same 2 failures; the extra test in §1 accounts for 489 vs 488). Confirmed by restoring the
pristine file as a control.

- `providers::chrome::process::tests::terminating_a_replaced_tree_leaves_the_new_tree_alone` —
  *"a stale teardown cleared the live slot"*. `LIVE_CHROME_GROUP` is a **process-global** atomic and
  every test that spawns a tree publishes into it via `adopt()`. Two such tests cannot hold the slot
  simultaneously, so in parallel a different test's group occupies it at the assertion.
- `providers::chrome::tests::launch_timeout_kills_and_reaps_the_whole_tree` —
  *"expected the browser pid and its child's, got []"*. The fake browser gets 800 ms to write its pid
  file; under ~490 parallel tests it loses that race.

**CI is green only because CI always passes `--test-threads=1`, and nothing local enforces it.** A
developer running plain `cargo test` gets a red suite and an invitation to chase a phantom. This is a
**test-design defect, not a product defect** — but it must never be reported as "the tests fail".

### 6.2 CI is `workflow_dispatch`-only

`ci.yml` never runs on push or PR, by design (Actions minutes). Re-running CI therefore requires the
commit to be on the remote first, then `gh workflow run ci.yml --ref <branch>`.

### 6.3 Carried forward from Batch 12, still open

- **The app cannot display its own version.** `SettingsView.tsx:240` sets `appVersion: info.appName`,
  so the "App version" row shows the app *name*; `app_metadata()` has no version field. Any manual
  step instructing a tester to read the version from Settings is **invalid** — it must come from
  `CFBundleShortVersionString` or the installer filename.

---

## 7. Gate results at `746775d`

| Gate | Command | Result |
|---|---|---|
| Rust fmt | `cargo +1.98.0 fmt --check` | **clean** |
| Rust lint | `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` | **clean, 0 warnings** |
| Rust tests | `cargo +1.98.0 test -- --test-threads=1` (CI's command) | **491 passed, 0 failed, 2 ignored** |
| Rust tests | `cargo +1.98.0 test --all-features -- --test-threads=1` | **491 passed, 0 failed, 2 ignored** |
| Frontend types | `tsc --noEmit` | **clean** |
| Frontend tests | `vitest run` | **317 tests across 25 files, 0 failures** |
| CI | run `35532271846` @ `746775d` | see §8 |

The 2 ignored tests are the real-Chrome pair, which are now **guarded**: they panic unless
`MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1`. CI does not pass `--ignored`, so CI can never trip them.

**Note on the frontend run:** the machine was under memory pressure from the Rust suite (free memory
fell to ~80 MB), and `vitest` was SIGTERM'd partway through on several attempts. Every file was
therefore also verified in smaller groups — 21 files in one run plus the remaining 4 individually
(12 + 9 + 17 + 1 tests). **All 25 files green; 317 tests; 0 failures.** No file was skipped or assumed.

---

## 8. CI

Run **`35532271846`**, dispatched at `746775dfc72a0b7db3c37d0e1b5fca8a14895726` via
`gh workflow run ci.yml --ref stabilization/v0.9.9-rc1`.

**Result: `completed` / `success`.** All six jobs green:

| Job | Conclusion | Duration |
|---|---|---|
| Rust (macos-latest) | **success** | 430 s |
| Rust (windows-latest) | **success** | 544 s |
| Frontend | **success** | 35 s |
| Cargo audit (advisory) | **success** | 189 s |
| pnpm audit (advisory) | **success** | 17 s |
| version-consistency | **success** | 3 s |

**Verified that Windows actually ran tests rather than only compiling** — a green can be a skip:

```
running 483 tests
test result: ok. 481 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 49.48s
running 28 tests
test result: ok.  28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 58.78s
```

Windows also passed `cargo clippy --all-targets --all-features -- -D warnings` and
`cargo fmt --check`, which is what confirms the §1 test-only change compiles clean on the
non-unix target as well. (The Windows lib count is 483 vs macOS 493 — the difference is the
`#[cfg(unix)]`-gated process tests, which do not exist on Windows.)

**What this does and does not establish.** It establishes that the candidate at `746775d` passes
fmt, clippy and the test suite on **both** macOS and Windows runners, that the version gate is
satisfied, and that the supply-chain audits are clean. It establishes **nothing** about runtime
behaviour on either platform: `build.yml`/`ci.yml` skip the real-libmpv and real-Chrome tests by
design, so no test in this run opened a window, played a frame, or produced a sound. **Parts A and B
remain BLOCKED.**

---

## 9. What is required to reach GO

1. **Two physical devices** (ideally macOS + Windows), both with Tailscale authenticated, both running
   **the same** SHA, both installed from the candidate build — then all 28 Part A rows attempted with
   real screenshots/video and real metadata recorded.
2. **A real Windows device** for all Part B items.
3. **A real feature-length movie with audio** for the Part A playback rows.
4. **Two-peer provider convergence** for Part C.
5. **Then** re-freeze: whatever SHA those devices run is the SHA to audit and tag. **The candidate is
   currently `746775d`; if any further source change lands, it moves again and the physical validation
   must be re-run against the new SHA.**

---

## 10. Compliance with the batch's prohibitions

| Prohibition | Status |
|---|---|
| Do not create a tag | **COMPLIED** — zero `v0.9.9` tags, local or remote |
| Do not create a release | **COMPLIED** — zero `v0.9.9` releases; v0.9.8 still the only one |
| Do not push `main` | **COMPLIED** — remote `main` unchanged at `bb225778…` |
| Do not modify v0.9.8 | **COMPLIED** — tag object byte-identical local and remote |
| Do not mark anything PASS without observed evidence | **COMPLIED** — zero PASS results in Parts A/B; Part C/D passes each cite a command or a file |
| If a prerequisite is unavailable, mark BLOCKED/NOT TESTED | **COMPLIED** — Parts A and B marked BLOCKED in full |
| Never manufacture evidence | **COMPLIED** — no screenshots, no invented metadata, no inferred results |

The branch push in §1 carried **only** the branch (`--no-follow-tags`), was authorised by the owner's
explicit choice to fix and re-run CI, and was required to re-run CI at all (§6.2).
