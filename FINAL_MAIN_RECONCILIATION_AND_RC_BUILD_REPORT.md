# FINAL MAIN RECONCILIATION AND RC BUILD REPORT

**Task:** Final state reconciliation + pre-physical-beta preparation for Movie Party.
**Date:** 2026-10-04 (IST)
**Outcome classification:** see §13.

**`FINAL_MAIN_SHA` = `fe2754ffd3cc56389d9ef4edd3699c57c6b18178`**

> **This file is intentionally left untracked.** It names the SHA that CI and the artifact builds
> ran against. Committing it would move the tip of `main` past that SHA, and the report's central
> claim — *the SHA that was built and tested is the SHA on `main`* — would stop being true. The
> previous batch's report was left untracked for the same reason and was committed later, by this
> task, as a separate historical commit.

---

## 1. Starting state

Measured before any change was made.

| Field | Value |
|---|---|
| Starting branch | `stabilization/v0.9.9-rc1` |
| Starting HEAD | `f04731a5324ff239843d623ba400a2b021070b5a` |
| `origin/main` | `bb225778434e3f07923e03e85d7d7cc1db79146c` |
| Local `main` | `2d5c83391dec43342bb4346ae19c542a6e672385` |
| `v0.9.8` tag object | `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3` |
| `v0.9.8` dereferences to | `bb225778434e3f07923e03e85d7d7cc1db79146c` |
| Ahead/behind — `origin/main...stabilization` | **0 behind, 50 ahead** |
| Ahead/behind — `origin/main...local main` | **0 behind, 4 ahead** |
| Uncommitted changes | none |
| Untracked files | `BATCH14_REPORT.md` (one file) |
| Remote tags | `v0.9.0`, `v0.9.4`, `v0.9.5`, `v0.9.6`, `v0.9.8` — no `v0.9.9` |

### 1.1 Two corrections to the brief's stated starting state

Both were checked rather than assumed, and both change what the reconciliation had to do.

**(a) `origin/main` IS `v0.9.8`.** The brief asked whether `main` had independent commits that must be
preserved. It does not: `origin/main` is exactly `bb225778`, which is the `v0.9.8` tag's commit. There
is no independent remote work to reconcile.

**(b) The active candidate was not `746775d`.** The brief names `746775d` as the previous stabilization
candidate. It is a real commit and it *is* an ancestor of the branch tip, but the branch had already
moved **three commits past it** (all documentation: `7c76296`, `8a92e4f`, `f04731a` — the Batch 13
NO-GO report). So the starting tip was `f04731a`, not `746775d`. This is recorded because it affects
the "is the SHA honest" question: `746775d` was already superseded before this task began.

A third discrepancy in the historical record, noted for accuracy: `BATCH12_REPORT.md` §7 states that
local `main` was "behind `origin/main`". It was **ahead by 4** (`git rev-list --left-right --count
origin/main...main` → `0  4`). The four commits are the Batch 4/5/6 work. `main` was never pushed, so
nothing was affected — but the report's direction was wrong.

---

## 2. Reconciliation

**Result: a clean fast-forward. No conflicts. Nothing was discarded.**

| Question | Answer | Evidence |
|---|---|---|
| Are the local and remote histories divergent? | **No** | `git merge-base --is-ancestor origin/main stabilization/v0.9.9-rc1` → true (exit 0) |
| Does `origin/main` contain work not in the stabilization branch? | **No** | `git log --oneline stabilization/v0.9.9-rc1..origin/main` → empty |
| Does local `main` contain work not in the stabilization branch? | **No** | `git log --oneline stabilization/v0.9.9-rc1..main` → empty (local `main` is a strict ancestor) |
| Conflicts encountered | **0** | `git merge --ff-only` succeeded without a merge commit |
| Was any remote work preserved? | **Vacuously yes** — there was none to preserve; `origin/main` is an ancestor of the result, so nothing could be dropped |
| Was a force push used? | **No** | `git push --no-follow-tags origin main` reported `bb22577..fe2754f  main -> main` — a fast-forward |
| Were any tags pushed? | **No** | `--no-follow-tags` was used explicitly |

### 2.1 Commits brought into `main`

`main` advanced from `2d5c833` to `fe2754f`: **56 commits, 0 behind.** That is the whole
`v0.9.8` → candidate body of work (Batches 4–14) plus the six commits this task added.

Commits added **by this task**:

| SHA | Subject |
|---|---|
| `179067b` | `docs(Batch 14): record the cross-platform pre-two-device validation` |
| `0f5ada3` | `fix(settings): report the real application version, not the app name` |
| `bf00504` | `ci: add an RC artifact workflow, and make the beta pre-release default explicit` |
| `e3abb29` | `docs(release): document the RC path, the gating sequence, and the TMDB decision` |
| `939f869` | `docs: state plainly that the TMDB credential is exposed in the client build` |
| `fe2754f` | `docs(beta): point the physical validation procedure at FINAL_MAIN_SHA` |

`BATCH14_REPORT.md` was untracked at the start. It was committed as `179067b` rather than left loose,
because it is the evidence base for this task and every other batch report in this project is tracked.
It is a dated record and was **not** edited to match the present — the brief's rule is that `746775d`
must stop being treated as the *active* candidate, not that history must be rewritten.

---

## 3. Audit

One targeted audit, scoped to what could invalidate physical beta testing or installer generation.

### 3.1 Confirmed findings

| # | Finding | Severity | Status |
|---|---|---|---|
| F1 | Settings → "App version" displayed the application **name**. `app_metadata()` carried no version field, so `SettingsView.tsx` bound `info.appName` into a row labelled "App version". | **Blocker for a manual step** — every procedure telling a tester to "confirm the version in Settings" was invalid | **FIXED** (§4.1) |
| F2 | No workflow could build **both** platforms without the possibility of publishing. `build.yml` is Windows-only and attaches to a release on a `v*` ref; `release.yml` publishes. There was no way to produce a testable macOS `.dmg` + Windows `.exe` pair from `main`. | **Blocker for physical testing** — the brief's whole purpose is private RC artifacts | **FIXED** (§4.2) |
| F3 | `release.yml` hardcoded `prerelease: false`. `v0.9.9` is a **beta**, so the eventual release would have published as **stable**. | **Release-configuration defect** | **FIXED** (§4.3) |
| F4 | `release.yml`'s manual-dispatch `tag` default was the stale `v0.9.4`. | Hygiene — a dispatch without an explicit tag would target the wrong release | **FIXED** (§4.3) |
| F5 | `VITE_TMDB_TOKEN` was described as "a repo secret, never committed" in five places, which is true of the repository and misleading about the product. | **Honesty defect** — implies a protection that does not exist | **FIXED** (§4.4) |
| F6 | The physical-beta procedure named `260cb06` and stated the app could not display its own version (both stale after F1). | Documentation | **FIXED** (§4.5) |
| F7 | **New upstream advisory:** `pnpm audit --audit-level high` now reports 6 findings (2 moderate, 4 high), all `brace-expansion` reached through `eslint`/`typescript-eslint`. | **Non-blocking** — advisory job is `continue-on-error: true`; **dev-only** | Reported, not fixed (§3.4) |

### 3.2 False positives / claims that did not hold

| Claim under test | Verdict | Evidence |
|---|---|---|
| "The workflow list contains a candidate build workflow that is Windows-only" | **Partly false** | There is no *candidate* workflow at all before this task. `build.yml` is Windows-only; `ci.yml` runs no installer build. F2 is the accurate statement. |
| "`main` has independent commits that must be preserved" | **False** | `origin/main` == `v0.9.8` == an ancestor of the branch. Nothing to preserve. |
| "`746775d` is the active candidate" | **False** | It was superseded by three documentation commits before this task started (§1.1b). |
| "A `v*` tag is safe once version consistency passes" | **False, and now documented as false** | `docs/RELEASE_PROCESS.md` §"Gating" states it explicitly. |

### 3.3 Invariants verified (audit A)

All verified against source at `FINAL_MAIN_SHA`, not accepted from prior reports.

| Invariant | Result | Evidence |
|---|---|---|
| Provider Shared still disabled | **PASS** | `providers/sync.rs:121` `shared_available: false`; asserted for **every** provider at `sync.rs:646` (`.all(\|capability\| !capability.shared_available)`) |
| No DRM bypass / extraction in stable scope | **PASS** | No capture/extraction path is reachable: Provider Shared is the only capture consumer and it is off |
| Two-user limit enforced | **PASS** | Architectural: the room holds one local participant plus a single `peer_participant: Option<ParticipantSnapshot>` slot (`app_runtime.rs:512`). `snapshot_from_state` pushes at most one peer, so a second remote cannot be represented |
| Strict sync enabled/default as designed | **PASS** | `sync/state_machine.rs:1` `STRICT_SYNC_DEFAULT: bool = true`; `snapshot_from_state` reports `strict_sync: true` unconditionally |
| Guest buffering/disconnect pauses the room | **PASS** | `sync/local.rs` sets `paused_by_strict_sync` on `PauseCause::BufferLow`; `media/local_perfect.rs:157` gates on it; end-of-media is deliberately excluded (ADV-05) |
| Real WebRTC path exists | **PASS** | `src/call/webrtc.ts` constructs real `RTCPeerConnection`s (10 references), `callSession.ts` drives `createOffer`/`setRemoteDescription` |
| Managed Chrome / CDP path exists | **PASS** | `providers/chrome/` — real launch plan with no `--headless`, CDP over a loopback socket |
| Local-file transport moves original bytes, no transcoding | **PASS** | Zero hits for `transcode` / `re-encode` / `reencode` across `src-tauri/src/`; the transfer layer is chunked byte movement keyed on a full-file hash (`media/manifest/mod.rs`) |
| No demo/fake production state | **PASS** | Every `fake_*` / `mock_*` hit is inside a `mod tests` block or a `#[cfg(test)]` helper; none is production-reachable |

### 3.4 Release/build integrity (audit B)

| Item | Result |
|---|---|
| `package.json` version | `0.9.9` |
| `src-tauri/tauri.conf.json` version | `0.9.9` |
| `src-tauri/Cargo.toml` version | `0.9.9` |
| `Cargo.lock` (`movie-party`) version | `0.9.9` |
| `RELEASE_NOTES.md` version section | `## 0.9.9 (stabilization)` present — required, because `release.yml` fails the release without it |
| Rust toolchain pin | `1.98.0` — identical in `ci.yml`, `build.yml`, `release.yml`, `rc-build.yml` |
| Node / pnpm pins | Node `26`, pnpm `11.10.0` — identical in all four |
| Windows libmpv hash | `7310560BD25CC760282E8754A389302629C6E748261030C3870E1CB2F80CDC48`, verified before extraction in `build.yml`, `release.yml`, `rc-build.yml` |
| macOS libmpv source hashes | 6 `fetch_verified` calls with 6 SHA-256 pins in `scripts/build-libmpv-macos.sh` (ADV-09) |
| Artifact paths | Windows `target/release/bundle/nsis/*.exe` (verified: root Cargo workspace emits to `<repo>/target`); macOS `target/aarch64-apple-darwin/release/bundle/dmg/*.dmg`. `rc-build.yml` **discovers** these at runtime rather than hardcoding them |
| Runner targets | `macos-latest`, `windows-latest` |
| macOS architecture | `aarch64-apple-darwin` (Apple Silicon) — unchanged from `release.yml` |
| Windows architecture | `x86_64-pc-windows-msvc` (x64) |
| Release workflow behaviour | Gated on `version-consistency`; body derived from `RELEASE_NOTES.md`; `prerelease` now resolved, default `true` |
| Candidate workflow behaviour | New `rc-build.yml`: manual-only, both platforms, artifacts only, `permissions: contents: read` |

### 3.5 Security / release hygiene (audit C)

| Check | Result | Evidence |
|---|---|---|
| No secrets committed | **PASS** | `git grep -E 'ghp_\|gho_\|ghs_\|github_pat_\|sk-[A-Za-z0-9]{20,}\|AKIA[0-9A-Z]{16}\|xox[baprs]-\|BEGIN [A-Z ]*PRIVATE KEY'` returns exactly one hit — the line in `BATCH13_REPORT.md` that *describes* this scan. No real credential. |
| No provider credentials stored | **PASS** | No provider password/token handling in source; the managed Chrome profile holds the session, and nothing is written to the repo |
| Invite secrets not logged | **PASS** | Invite links carry no `authkey`/`tskey-`/`TS_AUTHKEY`; zero hits across `src/` + `src-tauri/` |
| Diagnostic export still redacts | **PASS** | `telemetry::redact_log_line` emits `[REDACTED]`; covered by `diagnostic_bundle_redacts_common_secret_tokens` and `diagnostic_export_redacts_secret_values_and_keeps_stable_metadata`. **Residual noted:** the Settings-side export (`SettingsView.tsx`) includes `snapshot.provider.url`, which for a provider session can carry a session URL. It is local-only and never uploaded; recorded as a residual, not a blocker. |
| Chrome CDP remains localhost-only | **PASS** | `providers/chrome/mod.rs:24` `CDP_BIND_HOST: &str = "127.0.0.1"`; the listener binds `(CDP_BIND_HOST, 0)` |
| No broad filesystem/network permission changes | **PASS** | `tauri.conf.json` CSP unchanged; `bundle.resources` still only `mpv_runtime/`; no capability additions |
| No new unsafe production behaviour | **PASS** | Production `unsafe` is confined to `providers/chrome/process.rs` and one `kill(pid, 0)` probe, all with `// SAFETY:` comments. This task added none. |
| No dependency issue reintroduced | **PASS** | This task changed **no** dependency: `git diff f04731a..HEAD -- package.json pnpm-lock.yaml pnpm-workspace.yaml` is empty. See F7 for the new upstream advisory. |

### 3.6 The six known findings, verified individually

| # | Known finding | Verified verdict |
|---|---|---|
| 1 | Settings shows the app name in "App version" | **CONFIRMED, and now FIXED.** Was `SettingsView.tsx:240` `appVersion: info.appName`; `app_metadata()` returned only `app_name`/`protocol_*` |
| 2 | Default parallel `cargo test` has known process-test races; the serialized CI command is the trusted gate | **CONFIRMED, unchanged.** Two process-global-state tests (`LIVE_CHROME_GROUP`, an 800 ms pid-file race) fail in parallel and pass with `--test-threads=1`. Test-design defect, not a product defect. Not fixed — out of scope, and the trusted gate is unaffected |
| 3 | Some test-only `unsafe` blocks lack `SAFETY` comments | **CONFIRMED, unchanged.** 18 undocumented blocks in `real_*` test files; `undocumented_unsafe_blocks` is not enabled so clippy cannot see them. Hygiene only; they do not ship. Not fixed — own batch |
| 4 | Provider Shared remains intentionally disabled | **CONFIRMED.** `sync.rs:121`, asserted at `sync.rs:646` (§3.3) |
| 5 | Physical Mac↔Windows verification remains external | **CONFIRMED.** No Windows device was reachable (§11). Nothing in this task claims otherwise |
| 6 | The final `v0.9.9` release must NOT be produced here | **HONOURED.** No tag, no release, no artifact attached to any release (§10) |

### 3.7 Release blockers

| Blocker | Disposition |
|---|---|
| No installer pair existed for any current SHA | **CLEARED** by `rc-build.yml` + the run in §9 |
| The app could not display its own version, invalidating a manual provenance step | **CLEARED** (§4.1) |
| `v0.9.9` would have published as stable, not beta | **CLEARED** (§4.3) |
| **Two-device physical validation has never been performed on any SHA** | **REMAINS — and cannot be cleared by this task.** It requires two physical machines and a human at each. This is *not* a blocker to readiness (§13) — it is the beta test itself, which is the next step. It **is** the reason `v0.9.9` must not be tagged |

### 3.8 Deferred physical-only items

Not defects, and not addressable from this environment:

* whether audio is **audible** and whether A/V stays in sync over a feature-length film;
* whether video is **visible and correct** on the physical display;
* whether two peers actually **converge** on one position (the beta promise);
* camera quality, and UI correctness on the physical display;
* Windows **runtime** behaviour — installation, launch, Tailscale detection, playback, Chrome
  cleanup, uninstall/reinstall. CI proves the Windows *build*; it proves nothing about behaviour.

---

## 4. Code and workflow fixes

### 4.1 F1 — Settings reports the real version

**Files:** `src-tauri/src/lib.rs`, `src/backend/appRuntime.ts`, `src/views/SettingsView.tsx`,
`src/views/settingsMetadata.ts` *(new)*, `src/views/settingsMetadata.test.ts` *(new)*

* `AppMetadata` gains `app_version`, populated from `env!("CARGO_PKG_VERSION")`.
  **Why this source:** it is the compile-time value of the declaration the `version-consistency` gate
  already compares against `package.json`, `tauri.conf.json` and `Cargo.lock`. Deriving from it means
  one place to bump and no possibility of drift — as opposed to writing `"0.9.9"` into a fifth file,
  which the brief explicitly rules out.
* `AppMetadataInfo` gains `appVersion`.
* The mapping moves to `settingsMetadata.ts` as a pure function. **Why:** `vitest` runs in `node` with
  no DOM, so a component cannot be rendered; the repo's established pattern is to extract the decision
  (`cinemaEndState.ts`, `reconnectLatchAfter`, `cinemaDockInteractionClass`).
* A failed `app_metadata` call now falls back to the `—` placeholder instead of silently retaining a
  stale value.

### 4.2 F2 — RC artifact workflow

**File:** `.github/workflows/rc-build.yml` *(new)*

* `workflow_dispatch` only; `permissions: contents: read`.
* Builds **both** platforms: Windows x64 NSIS `.exe` and macOS Apple Silicon `.dmg`.
* Same pins as `ci.yml`/`release.yml` (Rust 1.98.0, Node 26, pnpm 11.10.0).
* Same libmpv staging **and the same pinned SHA-256** for the Windows DLL; the same
  `build-libmpv-macos.sh` + glad2 venv path for macOS, including the SC2155 `MPV_PYTHONPATH` guard.
* `pnpm install --frozen-lockfile`, then `pnpm tauri build` (macOS with `--target aarch64-apple-darwin`,
  matching `release.yml`).
* Writes `SHA256SUMS.txt` per platform and uploads it with the artifact.
* `if-no-files-found: error`.
* **Artifact paths are discovered at runtime**, not hardcoded: a `Locate the installer/DMG` step finds
  the bundle under `target/**/release/bundle/{nsis,dmg}` and fails loudly if there is none.
* **Cannot publish:** no `tagName`, no `tauri-action`, no `action-gh-release`, and a read-only token.
* Artifact names: `Movie-Party-v<version>-RC-Windows-x64`, `Movie-Party-v<version>-RC-macOS-arm64`,
  where `<version>` is read from `package.json` rather than hardcoded.

### 4.3 F3 + F4 — beta pre-release semantics and the stale default

**File:** `.github/workflows/release.yml`

* New `prerelease` dispatch input (`type: boolean`, default `true`).
* The flag is resolved **once** in the `resolve-matrix` job and passed to both legs, so they cannot
  disagree. A tag push carries no inputs and takes the same safe default.
  **Why default `true`:** a release accidentally published as *stable* is far harder to walk back than
  one accidentally published as a *pre-release* — and the previous hardcoded `false` made every release
  stable by default.
* `tauri-action` now receives `prerelease: ${{ needs.resolve-matrix.outputs.prerelease }}`.
* Dispatch `tag` default `v0.9.4` → `v0.9.9`.

### 4.4 F5 — TMDB credential honesty

**Files:** `src/home/tmdbFeed.ts`, `src/vite-env.d.ts`, `.env.example`, `README.md`,
`.github/workflows/release.yml` (comment), `docs/RELEASE_PROCESS.md`

Each now states the same thing: the value is inlined into the shipped bundle, is recoverable from any
installer, must be safe to expose, and anything that genuinely must stay secret cannot be shipped this
way. See §5 for the decision and the experiment that establishes it.

### 4.5 F6 — beta procedure

**File:** `BATCH7B_REAL_BETA_VALIDATION.md`

* Option A is now the RC artifact workflow; the local build is the fallback.
* The candidate is referenced as **`FINAL_MAIN_SHA`**, obtainable with `git rev-parse origin/main` and
  pinned in §7 of this report. **Why the token rather than the literal hex:** writing the hex into a
  tracked file would move the tip of `main`, so the SHA in the report would stop matching the SHA CI
  and the artifact builds ran against — the one thing this task must not do.
* The superseded candidates (`746775d`, `260cb06`) are named explicitly as must-not-use.
* The equivalence check now also covers `.github/`.
* §7 gains the ordered two-device runbook and restates that Provider Shared is excluded.

### 4.6 Workflow validation

`actionlint` 1.7.7 + `shellcheck` 0.11.0 over all five workflows: **exit 0, zero bytes of output.**

**A negative control was run first**, because a checker that cannot fail proves nothing: a deliberately
broken workflow (undefined `matrix.os`, `with:` on a `run:` step) produced two real diagnostics and
exit 1 under the same invocation. `shellcheck` was likewise controlled with a script using `rm $1` and
`for f in $(ls *.txt)`, which it correctly rejected.

---

## 5. TMDB credential conclusion

**The credential is exposed in the client build. This is stated as fact, not as risk.**

`VITE_TMDB_TOKEN` is passed to Vite at build time. Vite performs **static substitution**: every
`import.meta.env.VITE_*` reference is replaced by the literal value, so the token ends up verbatim in
the shipped JavaScript. "Not committed to the repo" prevents a *source* leak (forks, archives,
`git log`); it does **not** make the value secret in the product, and the project previously implied
that it did.

**Established experimentally, not asserted.** A build was run with
`VITE_TMDB_TOKEN=SENTINEL_TMDB_TOKEN_9f3c1a7b2e` and the output searched:

```
$ grep -rl "SENTINEL_TMDB_TOKEN_9f3c1a7b2e" dist/
dist/assets/index-Bsl4KoMe.js          # the sentinel is in the shipped bundle
```

The tree was then rebuilt without the sentinel and re-searched: 0 occurrences.

**Decision: it is a client-side credential, and it is acceptable as one for this private RC.**

* It is documented as **public and rotatable** in all five places it could drift between.
* It is **not** hidden, and no claim is made that the installed app protects it.
* Redesigning the application to conceal a TMDB key — a backend proxy, or shipping no default and
  requiring a per-device paste — would be a large change for no real gain, and the brief explicitly
  forbids redesigning the app for this. The honest documentation is the correct fix.
* The per-device override in Settings still works, so a retired key never strands an install.
* **Consequence to accept knowingly:** anyone holding an installer can read this key. It must remain a
  key whose exposure is not damaging, and it must be revocable on the TMDB side.

---

## 6. Verification

All gates run locally at `FINAL_MAIN_SHA`, on the pinned toolchain.

### 6.1 Frontend

| Gate | Command | Result |
|---|---|---|
| Install | `pnpm install --frozen-lockfile` | **ok** — "Already up to date", lockfile satisfied |
| Lint | `pnpm lint` (`eslint . --max-warnings=0`) | **exit 0** |
| Tests | `pnpm test` (`vitest run`) | **26 files / 321 tests passed, 0 failures** |
| Build | `pnpm build` (`tsc --noEmit && vite build`) | **exit 0** — 2284 modules, built in 29.25 s |

The previous baseline was 25 files / 317 tests. The delta is the new `settingsMetadata.test.ts`
(+1 file, +4 tests).

### 6.2 Rust

| Gate | Command | Result |
|---|---|---|
| Format | `cargo +1.98.0 fmt --check` | **exit 0** |
| Lint | `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` | **exit 0, zero warnings** |
| Tests | `cargo +1.98.0 test -- --test-threads=1` | **exit 0 — 494 tests, 492 passed, 0 failed, 2 ignored** |

The `libmpv` runtime **is staged locally** (`src-tauri/mpv_runtime/`, 13 dylibs), so unlike CI the real
media tests genuinely executed:

```
test audio_bearing_fixture_yields_an_audio_track ... ok
test production_player_reports_completed_when_the_movie_ends ... ok
test bundled_libmpv_renders_onto_real_calayer_through_production_player ... ok
test bundled_libmpv_plays_pauses_seeks_real_video ... ok
```

The 2 ignored tests are the real-Chrome pair, which require an explicit opt-in and are ignored by
design. **No ignored-test count increase** (was 2, still 2).

### 6.3 Focused tests for the Phase-3 changes — with mutation controls

A new test that cannot fail proves nothing, so **both** new guards were verified by reintroducing the
exact defect and confirming the test goes red. Each mutated file was backed up first and restored
afterwards, with the SHA-256 compared on both sides.

**Control 1 — frontend.** `settingsMetadata.ts` changed to return `info.appName`:

```
FAIL  src/views/settingsMetadata.test.ts > settings metadata rows
      > shows the application version, not the application name
AssertionError: expected 'Movie Party' to be '0.9.9'
Tests  1 failed | 3 passed (4)
```

Restored: `0faffdccfd191605fd5ff81ae50dca91944e09cb529db8bd282036f416f452f1` before and after.

**Control 2 — Rust.** `app_version: env!("CARGO_PKG_VERSION")` changed to `app_version: APP_NAME`:

```
test tests::metadata_reports_the_package_version_not_the_app_name ... FAILED
assertion `left == right` failed
  left: "Movie Party"
 right: "0.9.9"
test result: FAILED. 0 passed; 1 failed
```

Restored: `0d47629a3377371b559929671774dd1ccab5feed06c59d6f09c93804452a4305` before and after.

**The control was re-run after restoration** as part of the full suites in §6.1/§6.2, both green.

### 6.4 What these results do and do not establish

**Established:** the code compiles, is formatted, is lint-clean on both the pinned toolchain and the
full `--all-targets --all-features` surface, and its test suites pass on macOS locally **including the
real-libmpv media tests**.

**Not established, and not claimed:** anything about runtime behaviour on Windows; anything about two
peers; anything about audible audio or visible video on a physical display. Local tests cannot produce
those results, and no attempt was made to infer them.

---

## 7. Final `main` state

| Field | Value |
|---|---|
| **`FINAL_MAIN_SHA`** | **`fe2754ffd3cc56389d9ef4edd3699c57c6b18178`** |
| `origin/main` after push | `fe2754ffd3cc56389d9ef4edd3699c57c6b18178` |
| Match? | **YES — identical** (`git rev-parse HEAD` == `git ls-remote origin refs/heads/main`) |
| Push form | `bb22577..fe2754f  main -> main` — fast-forward, `--no-follow-tags`, **no force** |
| Local `main` | `fe2754f` — same commit |
| Ahead/behind vs `origin/main` | 0 / 0 |

`FINAL_MAIN_SHA` **supersedes `746775d`**, which superseded `0c2b5e86`. All future physical tests must
use `FINAL_MAIN_SHA`.

---

## 8. GitHub CI

| Field | Value |
|---|---|
| Run ID | **`37201442858`** |
| URL | https://github.com/Abhijai10/Movie_Party/actions/runs/37201442858 |
| Trigger | `workflow_dispatch` on `main` |
| **`headSha` tested** | **`fe2754ffd3cc56389d9ef4edd3699c57c6b18178`** |
| **SHA == `FINAL_MAIN_SHA`?** | **YES** |
| Overall conclusion | **`success`** |

| Job | Result | Duration |
|---|---|---|
| `version-consistency` / Version consistency | **success** | 5 s |
| Frontend | **success** | 29 s |
| Rust (macos-latest) | **success** | 6 m 33 s |
| Rust (windows-latest) | **success** | 9 m 07 s |
| Cargo audit (advisory) | **success** | 3 m 11 s |
| pnpm audit (advisory) | **failure** — advisory, `continue-on-error: true`; did **not** fail the run | 20 s |

### 8.1 The tests actually executed — this is not a green-by-skip

A green job can be a skip, so the logs were read rather than the conclusion trusted.

* **macOS:** lib target `running 493 tests` → `491 passed; 0 failed; 2 ignored; 1 filtered out`.
* **Windows:** ran the full integration set (28 / 18 / 15 / 9 / 7 / 3 / 2 tests, all `0 failed`) plus
  the lib target — so the Windows leg compiled **and executed** the suite, including
  `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`.
* **Both legs took the explicit-skip branch for the 9 libmpv-dependent tests** and printed
  `EXPLICIT SKIP: the 9 libmpv-dependent tests above did not execute (no libmpv runtime in this job)`.
  That is the designed behaviour — `src-tauri/mpv_runtime/*` is gitignored and CI does not stage it —
  and it is an *announced* skip, not a silent pass. Those tests were run for real locally (§6.2).

**What CI does not establish:** it ran no real playback and opened no window, so it says nothing about
audio, video, sync or convergence. The Windows job proves the Windows **build and test suite**, not
Windows **behaviour**.

### 8.2 The advisory `pnpm audit` failure (F7)

`pnpm audit --audit-level high` reports **6 findings (2 moderate, 4 high)**, all `brace-expansion`
reached through `eslint` / `typescript-eslint` (`GHSA-6j4f-fj2g-mc7p` and two older ranges).

Established rather than assumed:

* **Not introduced by this task.** `git diff f04731a..HEAD -- package.json pnpm-lock.yaml
  pnpm-workspace.yaml` is empty; the only `package.json` change since `v0.9.8` is the version bump.
* **Not reachable from shipped code.** Of 46 distinct vulnerable dependency paths, **0** lie under a
  runtime dependency — all are under build-time tooling (`eslint`, `typescript-eslint`, `vitest`,
  `vite`, `prettier`, `@types`). The bundled app does not contain them.
* **Non-blocking by design.** The job carries `continue-on-error: true`; the run concluded `success`.

It is a newly published upstream advisory in a dev-only transitive dependency. Reported, not fixed —
patching it means bumping the eslint/typescript-eslint toolchain, which is unrelated to physical beta
readiness and would invalidate the CI evidence for a cosmetic gain.

---

## 9. RC artifact workflow

| Field | Value |
|---|---|
| Run ID | **`37201464926`** |
| URL | https://github.com/Abhijai10/Movie_Party/actions/runs/37201464926 |
| Trigger | `workflow_dispatch` on `main` |
| **`headSha` built** | **`fe2754ffd3cc56389d9ef4edd3699c57c6b18178`** |
| **SHA == `FINAL_MAIN_SHA`?** | **YES** |
| GitHub Release created? | **NO** |
| Tag created? | **NO** |
| Anything attached to `v0.9.8`? | **NO** |

### 9.1 Job results

| Job | Result | Duration |
|---|---|---|
| Resolve version | **success** | 6 s |
| **Windows x64 RC installer** | **success** | **12 m 13 s** |
| **macOS arm64 RC DMG** | **success** | **11 m 05 s** |

Both platform jobs built a real Tauri bundle. Neither job had a release step, and neither could have
created one: the workflow has no `tagName`, no `tauri-action`, no `action-gh-release`, and runs with
`permissions: contents: read`.

### 9.2 Windows x64

| Field | Value |
|---|---|
| Artifact name | `Movie-Party-v0.9.9-RC-Windows-x64` |
| Artifact ID | `11303391962` |
| Installer filename | **`Movie Party_0.9.9_x64-setup.exe`** |
| Installer location in CI | `target\release\bundle\nsis\Movie Party_0.9.9_x64-setup.exe` |
| **SHA-256** | **`606fe2b47d4b92e84744f89fb086d16f82badb5eae8d45f4ec4a42cc99ab9c19`** |
| Architecture | **x64** (`x86_64-pc-windows-msvc`; NSIS `_x64-setup.exe`) |
| Artifact archive size | **40,241,656 bytes** |
| Actions job passed | **YES** |
| Built from | `fe2754ffd3cc56389d9ef4edd3699c57c6b18178` |

Verified from the run log, not inferred:

```
Running makensis to produce ...\bundle\nsis\Movie Party_0.9.9_x64-setup.exe
    Finished 1 bundle at:
        D:\a\Movie_Party\Movie_Party\target\release\bundle\nsis\Movie Party_0.9.9_x64-setup.exe
Found installer: D:\a\Movie_Party\Movie_Party\target\release\bundle\nsis\Movie Party_0.9.9_x64-setup.exe
606fe2b47d4b92e84744f89fb086d16f82badb5eae8d45f4ec4a42cc99ab9c19  Movie Party_0.9.9_x64-setup.exe
Artifact Movie-Party-v0.9.9-RC-Windows-x64 has been successfully uploaded! Final size is 40241656 bytes.
```

The libmpv DLL integrity check passed before staging (the pinned `7310560B…` SHA-256), so the runtime
bundled into this installer is the verified one.

### 9.3 macOS arm64

| Field | Value |
|---|---|
| Artifact name | `Movie-Party-v0.9.9-RC-macOS-arm64` |
| Artifact ID | `11303461638` |
| DMG filename | **`Movie Party_0.9.9_aarch64.dmg`** |
| DMG location in CI | `target/aarch64-apple-darwin/release/bundle/dmg/Movie Party_0.9.9_aarch64.dmg` |
| **SHA-256** | **`e2dafdfddbc7f3546e26540f6ad95a244af742f70c67502be1e701de844c99f7`** |
| Architecture | **arm64 / Apple Silicon** (`aarch64-apple-darwin`) |
| Artifact archive size | **16,134,698 bytes** |
| Actions job passed | **YES** |
| Built from | `fe2754ffd3cc56389d9ef4edd3699c57c6b18178` |

Verified from the run log:

```
Bundling Movie Party_0.9.9_aarch64.dmg (.../target/aarch64-apple-darwin/release/bundle/dmg/Movie Party_0.9.9_aarch64.dmg)
Found DMG: target/aarch64-apple-darwin/release/bundle/dmg/Movie Party_0.9.9_aarch64.dmg
e2dafdfddbc7f3546e26540f6ad95a244af742f70c67502be1e701de844c99f7  ./Movie Party_0.9.9_aarch64.dmg
Artifact Movie-Party-v0.9.9-RC-macOS-arm64 has been successfully uploaded! Final size is 16134698 bytes.
```

The `Locate the DMG` step discovered the real path at runtime rather than trusting a hardcoded one,
and the `SHA-256 checksums` step wrote `SHA256SUMS.txt` next to the DMG and uploaded it with the
artifact — so each artifact carries its own checksum manifest.

### 9.4 Limitation: the installer file sizes were not separately measured

The table above reports the **artifact archive** size, which is what the API and the run record. The
individual installer byte sizes were **not** independently captured, for two concrete reasons:

1. **The workflow logs the filename and the checksum, but not the file size.** Tauri's build output
   prints the bundle path; nothing prints the byte count.
2. **The artifacts could not be downloaded to this machine.** `gh run download 37201464926` was
   killed twice — once inside the sandbox and once with the sandbox disabled — with exit 137
   (`SIGTERM`), by this environment's file-approval path. That is an environment limit, not a project
   problem, and it matches what `BATCH12_REPORT.md` §5 recorded.

Because the upload uses `compression-level: 0` (store, no deflate), the archive is the installer plus a
75-byte `SHA256SUMS.txt` member and zip overhead — so the archive size is a close **upper bound** on the
installer size, not the exact figure. The **SHA-256 values above are exact** and come from the run
itself, which is the authoritative record of what was built.

Anyone with access to the Actions page can download both artifacts and confirm the checksums against
the published `SHA256SUMS.txt`. That is §12 step 3, and it is deliberately left unchecked here.

---

## 10. Release safety

| Requirement | Result | Evidence |
|---|---|---|
| `v0.9.8` unchanged | **PASS** | Tag object `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3` — identical local and remote; dereferences to `bb225778434e3f07923e03e85d7d7cc1db79146c` |
| No `v0.9.9` tag | **PASS** | `git tag -l 'v0.9.9*'` empty; `git ls-remote --tags origin 'refs/tags/v0.9.9*'` empty |
| No `v0.9.9` GitHub Release | **PASS** | No release was created by any workflow run in this task |
| No artifact attached to any release | **PASS** | `rc-build.yml` has no release step and a read-only token |
| `v0.9.8` not deleted, moved, recreated, force-updated or retagged | **PASS** | Tag object byte-identical; the push was `--no-follow-tags` |
| Provider Shared still disabled | **PASS** | `providers/sync.rs:121`; asserted for every provider at `:646` |
| Provider Shared not enabled in any build | **PASS** | Not touched by any change in this task |
| Remote tags at end of task | `v0.9.0`, `v0.9.4`, `v0.9.5`, `v0.9.6`, `v0.9.8` — **no `v0.9.9`** |

**No tag was created. No release was created. The final `v0.9.9` release was not produced.**

---

## 11. Remote Windows testing

### 11.1 What is actually available

| Fact | Value | Evidence |
|---|---|---|
| macOS node | `abhijais-macbook-air`, `100.114.120.114`, `BackendState: Running` | `tailscale status --json` |
| Tailnet | `tailc930b7.ts.net` | `MagicDNSSuffix` |
| Tailscale version | 1.102.4 | `tailscale version` |
| SSH client on the Mac | OpenSSH_10.2p1 | `ssh -V` |
| Windows nodes on this tailnet | **exactly one** — `saksheepc`, `100.98.24.70` | `tailscale status \| grep -i windows` |
| `saksheepc` state | **`offline, last seen 53d ago`** | `tailscale status` |
| `saksheepc` reachability | **unreachable** — `tailscale ping saksheepc` → **`peer's node key has expired`** | live command |
| `saksheepc` SSH port | **closed** — `nc -z -G 3 100.98.24.70 22` → exit 1 | live command |
| Parallels VM | not queryable from this sandbox (`/bin/ps: Operation not permitted`); Batch 14 recorded it as a **separate tailnet** and **ARM64** | `prlctl list -a` |

**Conclusion: no Windows device is reachable from this host.** The only Windows peer on the correct
tailnet is offline with an expired node key, and its SSH port does not answer. No authorized
non-destructive setup or verification could therefore be performed, and none was attempted. The
remaining content of this section is the precise procedure for the owner, and an honest statement of
what it can and cannot prove.

### 11.2 Is Mac → Tailscale → Windows OpenSSH viable? **Yes, technically — and it is the right approach.**

The mechanism is sound and entirely within the tailnet: macOS ships an OpenSSH client, Windows ships an
OpenSSH **server** as an optional feature, and Tailscale provides the private network. No port needs to
be exposed to the public internet, and nothing needs to be enabled on a router.

**Prerequisites the owner must satisfy (all one-time, all on the Windows box):**

1. Windows 11 Pro/Enterprise (the OpenSSH *Server* capability is not available on Home).
2. Install it: `Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0`.
3. Start and enable it: `Start-Service sshd; Set-Service -Name sshd -StartupType Automatic`.
4. Confirm the firewall rule for port 22 exists (the installer normally creates it).
5. Sign in to Tailscale **on the same tailnet as the Mac** — this is the blocker Batch 14 hit; the Mac
   is on `tailc930b7` / `abhijairaghuvanshi@`, so the Windows node must join that tailnet, not another.
6. Confirm from the Mac: `tailscale ping <windows-node>` must answer, then `ssh <user>@<windows-node>`.

**Once that is in place, these become automatable** from the Mac with no GUI:

| Capability | How |
|---|---|
| SSH from macOS to Windows over the tailnet | `ssh user@<tailscale-name>` |
| Remote PowerShell | `ssh user@<node> 'powershell -NoProfile -Command "..."'` |
| Copy the installer across | `scp 'Movie Party_0.9.9_x64-setup.exe' user@<node>:C:/Users/<user>/Downloads/` |
| Verify the installer arrived intact | `Get-FileHash -Algorithm SHA256` and compare with `SHA256SUMS.txt` from the artifact |
| Install Movie Party silently | `Start-Process -Wait <installer> -ArgumentList '/S'` (NSIS; `installMode: currentUser`, so no elevation needed) |
| Launch it | `Start-Process "$env:LOCALAPPDATA\Programs\Movie Party\Movie Party.exe"` |
| Stop / restart it | `Stop-Process -Name 'Movie Party' -Force`, then relaunch |
| Inspect process state | `Get-Process -Name 'Movie Party' \| Select Id,CPU,WS,StartTime` |
| Inspect Tailscale state | `tailscale status --json`, `tailscale ip -4` |
| Run `tailscale ping` from Windows | `tailscale ping <mac-node>` — the direction the Mac cannot test alone |
| Inspect application diagnostics | `Get-ChildItem "$env:APPDATA\app.movieparty.desktop"`, plus the app's own log output |
| Collect crash / process info | `Get-WinEvent -LogName Application -MaxEvents 50`; `Get-Process` snapshots |
| **Inspect managed-Chrome cleanup** | `Get-Process chrome -ErrorAction SilentlyContinue` **before and after** app exit; the product must leave **zero** orphans, and must never kill a user's own Chrome |

### 11.3 Is Windows Device Portal useful? **Marginally — and it needs a caveat.**

Device Portal (WDP) is a real web UI on the Windows box, reachable over the tailnet at
`https://<node>:50443`. It gives a browser-based view of processes, installed apps, event logs and
performance counters, which is a reasonable *secondary diagnostic* when SSH is unavailable.

**It is not the primary mechanism, and two rules apply:**

* **Never expose it to the public internet.** It is an authenticated administrative surface. Over
  Tailscale it is private to the tailnet; published on a router it is a serious exposure. It must stay
  tailnet-only.
* **Never enable unauthenticated remote debugging.** No `--remote-debugging-port` bound to a
  non-loopback interface, no disabled authentication, no "trusted network" shortcuts. Movie Party's own
  Chrome CDP is already correctly bound to `127.0.0.1` and must stay that way.

For the specific checks in §11.2, SSH + PowerShell is strictly better: scriptable, diffable, and
capturable as evidence. WDP is a fallback for when a human needs to *look* at something.

### 11.4 What remote automation can never certify

This is the important limitation, and it is why remote access does not unblock the beta:

* **visible movie quality** — whether the picture is correct, sharp, and free of corruption;
* **audible audio correctness** — whether sound actually comes out of the speakers;
* **human-perceived A/V synchronisation** — whether the two devices *look and sound* together;
* **camera quality**;
* **UI correctness on the physical display.**

A remote shell can confirm a process is alive, a file is installed, a log line appeared and a hash
matched. It cannot hear a speaker or see a screen. Those require a person at each machine, or an
explicit evidence-capture procedure (screen recording with a shared external time reference — the
`E1` channel in `BATCH7B_REAL_BETA_VALIDATION.md`).

### 11.5 Recommended setup for the owner

1. Put the Windows machine on the **same tailnet** as the Mac (`tailc930b7`). This is the #1 blocker and
   nothing else works until it is done. `saksheepc` is already on that tailnet — it needs to come
   online and be re-authenticated (its node key has expired).
2. Enable OpenSSH Server on it (§11.2).
3. Verify from the Mac: `tailscale ping <node>`, then `ssh <user>@<node> hostname`.
4. Only then run the §11.2 table, and treat the §11.4 rows as requiring a human.

---

## 12. Remaining path to beta

A strict checklist. Each step consumes the output of the previous one. **The same `FINAL_MAIN_SHA` must
survive all of it** — if any source or workflow change lands, the SHA moves and steps 2–4 must be
repeated.

- [x] **1. `FINAL_MAIN_SHA` built** — `fe2754ffd3cc56389d9ef4edd3699c57c6b18178` on `main`
- [x] **2. CI green on that SHA** — run `37201442858`, `success`, `headSha` == `FINAL_MAIN_SHA` (§8)
- [x] **2b. Both RC installers built from that SHA** — run `37201464926`, `success`, Windows `.exe` and
      macOS `.dmg` produced with recorded SHA-256 values (§9)
- [ ] **3. Artifacts downloaded from the run page and their SHA-256 re-verified** against the
      `SHA256SUMS.txt` inside each artifact (could not be done from this environment — §9.4)
- [ ] **4. Windows installer physically installed** on the Windows test device
- [ ] **5. macOS DMG physically installed** on the Mac test device (expect the Gatekeeper first-launch step)
- [ ] **6. Two-device movie with audio tested** — both devices on the same tailnet, one tailnet only
- [ ] **7. Sync / reconnect / late-join / EOF tested** — the ordered runbook in
      `BATCH7B_REAL_BETA_VALIDATION.md` §7.1, with recorded evidence
- [ ] **8. Provider Sync tested** — YouTube first, then authorized Netflix/Prime/JioHotstar where
      available. **Provider Shared stays excluded**
- [ ] **9. Final read-only GO audit on that SHA** — no P0/P1, no unresolved stable-scope P2, and the
      tested SHA confirmed equal to the SHA to be tagged
- [ ] **10. Create the immutable `v0.9.9` beta tag/release** — only after step 9, with the release
      published as a **pre-release** (the default now resolved by `release.yml`)

**A `v*` tag must never be considered safe merely because version consistency passed.** That check
proves four declarations agree; it says nothing about whether the code works. Steps 6–8 are the beta
evidence, and they are the ones still outstanding.

---

## 13. Outcome classification

### **A. READY FOR PHYSICAL BETA TESTING**

**Read the scope of that word "ready" carefully, because it is narrow.**

It means: *a human can now sit down with two machines and run the beta.* The candidate is frozen, CI is
green on that exact SHA, and **both installers exist, built from that same SHA, with recorded
SHA-256 checksums.** Every blocker that was inside the project's control has been cleared.

It does **not** mean the beta has passed, and it does not mean `v0.9.9` is releasable. **The physical
beta has never been run, on any SHA, and this task did not run it.** The beta result is unknown. The
brief's stop condition — *do not say "beta ready" merely because builds are green* — is honoured
precisely by saying this: the builds are green, the artifacts exist, and **the beta is still unproven.**

### 13.1 Artifact information (required under A)

| | Windows | macOS |
|---|---|---|
| Artifact name | `Movie-Party-v0.9.9-RC-Windows-x64` | `Movie-Party-v0.9.9-RC-macOS-arm64` |
| Installer | `Movie Party_0.9.9_x64-setup.exe` | `Movie Party_0.9.9_aarch64.dmg` |
| Architecture | x64 | Apple Silicon (arm64) |
| SHA-256 | `606fe2b47d4b92e84744f89fb086d16f82badb5eae8d45f4ec4a42cc99ab9c19` | `e2dafdfddbc7f3546e26540f6ad95a244af742f70c67502be1e701de844c99f7` |
| Artifact archive size | 40,241,656 bytes | 16,134,698 bytes |
| Built from | `fe2754ffd3cc56389d9ef4edd3699c57c6b18178` | `fe2754ffd3cc56389d9ef4edd3699c57c6b18178` |
| Source run | [37201464926](https://github.com/Abhijai10/Movie_Party/actions/runs/37201464926) | same run |

**`FINAL_MAIN_SHA` = `fe2754ffd3cc56389d9ef4edd3699c57c6b18178`** (== `origin/main`).

Both artifacts also carry a `SHA256SUMS.txt` manifest inside the artifact. Download them from the run
page above. **The exact installer byte sizes are not reported** — see §9.4 for why.

### 13.2 What was verified, and by what

| Claim | How it was established |
|---|---|
| `FINAL_MAIN_SHA` is on `main` and is the frozen candidate | `git rev-parse HEAD` == `git ls-remote origin refs/heads/main` |
| CI is green on that exact SHA | run `37201442858`, `success`, `headSha` == `FINAL_MAIN_SHA`; all 6 jobs read individually |
| Both installers were built from that exact SHA | run `37201464926`, `success`, `headSha` == `FINAL_MAIN_SHA`; bundle paths and checksums read from the logs |
| No tag, no release, nothing attached to `v0.9.8` | `gh release view v0.9.9` → not found; `git ls-remote --tags` unchanged; `rc-build.yml` has no release step and a read-only token |
| Provider Shared is still disabled | `providers/sync.rs:121`, asserted for every provider at `:646` |
| The new version-display fix really works | mutation control: reintroducing the defect turned both new tests red (§6.3) |
| The TMDB token really is in the client bundle | sentinel build: the sentinel appeared verbatim in `dist/assets/*.js` (§5) |
| The workflows are syntactically sound | actionlint + shellcheck, exit 0, against a negative control that they do reject (§4.6) |

### 13.3 Residual limitations — stated plainly, not buried

These do not make the state B; they are the honest boundary of what this task could establish.

1. **The physical beta has not been run.** No two-device session, no real movie, no observed audio, no
   measured drift, no two-peer provider convergence. This is the whole point of the next step, and it
   is unproven.
2. **No Windows device is reachable from this environment**, so no remote setup or verification was
   performed. The mechanism is viable (SSH over the tailnet) and the procedure is in §11; the only
   Windows node on the correct tailnet is offline with an expired node key.
3. **The installer byte sizes were not independently measured** (§9.4), and the artifacts could not be
   downloaded here to re-verify the checksums. The hashes come from the run itself.
4. **`pnpm audit` is red (advisory, dev-only)** — 6 findings in `brace-expansion` via
   `eslint`/`typescript-eslint`, newly published upstream, not introduced by this task, not shipped
   (§8.2).
5. **`cargo test` remains red in parallel** — a test-design defect, unchanged, with the serialized CI
   command as the trusted gate (§3.6).
6. **18 test-only `unsafe` blocks still lack `SAFETY` comments** — hygiene, does not ship (§3.6).
7. **The TMDB credential is exposed in every build** — accepted, documented, and it must be treated as
   rotatable (§5).

### 13.4 What must not be inferred from this report

* That `v0.9.9` is released or releasable. **No tag and no release exist.**
* That the beta passed. **It has not been run.**
* That Windows runtime behaviour is verified. **CI built the installer; it ran nothing on Windows.**
* That two peers stay in sync. **Nobody has watched two peers stay in sync.**

**No tag was created. No release was created. The final `v0.9.9` release was not produced.**
