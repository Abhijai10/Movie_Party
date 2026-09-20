# BATCH 12 — v0.9.9 BETA CANDIDATE PREPARATION

**Status: candidate prepared and CI-verified. NO tag created. NO release published.**
**Date:** 2026-09-20 · **Branch:** `stabilization/v0.9.9-rc1` (not `main`)

---

## 0. Required answers (the nine fields)

| # | Field | Value |
|---|---|---|
| 1 | **Exact 0.9.9 candidate SHA** | **`0c2b5e86d30638be3483f853cfe6ede79a917dd1`** — the source commit CI validated and the commit to tag. (Branch tip is now a later docs-only commit; see §1 for why that does not change the candidate.) |
| 2 | **Exact version declarations** | all four at `0.9.9` — see §2 |
| 3 | **CI run ID** | `35526767715` |
| 4 | **Every CI job result** | 6/6 green — see §3 |
| 5 | **Artifact result** | macOS `.app` + `.dmg` built **and verified** locally; Windows `…_x64-setup.exe` built **and uploaded as a workflow artifact** with the release-attach step **skipped** — see §5 |
| 6 | **Release-workflow verification** | audited against the current tree + actionlint/shellcheck clean — see §4 |
| 7 | **Manual validation SHA** | `260cb06d2914b5133e1562712d661f2a0054209e` (source anchor) — see §6 |
| 8 | **v0.9.8 immutability proof** | tag object `4fca32ce…` identical local **and** remote — see §7 |
| 9 | **No v0.9.9 tag or release created** | confirmed, zero on both local and remote — see §8 |

**Supporting run:** `build.yml` run **`35527521397`** (dispatched at the **branch** ref) produced the
Windows installer as a workflow artifact without creating a release — the release-attach step shows
`skipped`. See §5.

---

## 1. Repository state at start of work

Verified **before** any change, as the batch required.

- `git rev-parse HEAD` → `176e92c04e27d39a0159cf3ac22ca7e646faef80` on `stabilization/v0.9.9-rc1`
  (the Batch 11 head), 6 commits ahead of `origin/stabilization/v0.9.9-rc1`.
- Working tree clean (`git status --porcelain` empty).
- `v0.9.8` present locally and on the remote, annotated, dereferencing to `bb22577`.
- **No `v0.9.9` tag existed anywhere.** Remote `main` = `bb225778434e3f07923e03e85d7d7cc1db79146c`.

**Commit count from the frozen release:** `git rev-list --count bb225778..HEAD` = **42**, and
`git merge-base --is-ancestor bb225778 HEAD` succeeds — the candidate is a descendant of v0.9.8, so
nothing was rewritten.

### Commits produced by this batch

| SHA | Subject | Files |
|---|---|---|
| `260cb06d2914b5133e1562712d661f2a0054209e` | `chore(release): bump version to 0.9.9, and add its release notes` | 5 changed, +82 / −4 |
| `0c2b5e86d30638be3483f853cfe6ede79a917dd1` | `docs(Batch 12): point the beta validation procedure at the 0.9.9 candidate` | 1 changed, +23 / −2 |

Those two are the **source** commits. Everything after them (this report, and the corrections to it) is
documentation-only and touches no source file — which is why the candidate is defined by the
equivalence check below rather than by the branch tip.

**Push:** `git push --no-follow-tags origin stabilization/v0.9.9-rc1` → `ade8a4f..0c2b5e8`.
`--no-follow-tags` was used deliberately and `push.followTags` was confirmed unset, so **no tag
travelled with the push**. `main` was never pushed.

### What "the candidate SHA" means here

This report is itself committed as a **documentation-only** commit, so `HEAD` moves past `0c2b5e8`.
That does **not** change the candidate, because the candidate is defined by its **source**, not by the
tip of the branch. The rule — and the same rule the beta validation doc uses — is:

```bash
git diff --stat 0c2b5e8..HEAD -- src/ src-tauri/     # must print nothing
```

If that prints nothing, whatever commit you are on is source-identical to the candidate. The SHA to
**tag** when the release is eventually authorised is the source commit
`0c2b5e86d30638be3483f853cfe6ede79a917dd1` (equivalently `260cb06` for `src/` and `src-tauri/`).

---

## 2. Part A — version 0.9.9

Four declarations, all measured in the current tree:

```
package.json                        "version": "0.9.9",
src-tauri/tauri.conf.json           "version": "0.9.9",
src-tauri/Cargo.toml              version = "0.9.9"
Cargo.lock (movie-party entry)    version = "0.9.9"
```

- A search for `"version": "0.9.8"` / `version = "0.9.8"` across all four files returns **nothing** —
  no declaration remains at 0.9.8.
- `Cargo.lock` contains exactly one `version = "0.9.8"`-shaped line for the `movie-party` package
  (≈ line 2490); the other 64-hex / version-looking hits in `Cargo.lock` are unrelated crate pins and
  were **not** touched.
- Historical `0.9.8` references in `BATCH*_REPORT.md`, `DEEP_PRODUCTION_READINESS_AUDIT.md`,
  `POST_REMEDIATION_ADVERSARIAL_AUDIT.md` and `docs/RELEASE_PROCESS.md` were **deliberately
  preserved** — they are dated records of what shipped, not declarations the pipeline reads.
  Blind-replacing them would have falsified the project's history.

### `RELEASE_NOTES.md` — a mandatory, non-obvious requirement

`release.yml` derives the release body from the `## <version>` section of `RELEASE_NOTES.md` and
**fails the release if that section is missing** (`::error title=No release notes section`). A
`## 0.9.9 (stabilization)` section was therefore written from the 42 commits since v0.9.8, covering:
sync correctness, end-of-media, Provider playback, security (rustls 0.23.45), release integrity, and
testing. Older sections untouched.

The derivation was exercised locally with the **same `awk` the workflow uses**:

```
$ awk '/^## 0\.9\.9/{flag=1} /^## /{if(flag && !/^## 0\.9\.9/)exit} flag' RELEASE_NOTES.md | wc -l
78
```

### Version-consistency workflow run locally

`version-consistency.yml` is a reusable workflow (`workflow_call` + `workflow_dispatch`) that extracts
the four declarations and compares each against `package.json`. It runs on the release path via
`uses:` from `release.yml` (line 47) — so it was **not** re-implemented by hand; the same script was
reproduced and all four resolve to `0.9.9`.

---

## 3. Part C — candidate CI

**Run ID `35526767715`**, dispatched with `gh workflow run ci.yml --ref stabilization/v0.9.9-rc1`,
against head SHA `0c2b5e86d30638be3483f853cfe6ede79a917dd1`.

```
conclusion: "success"
status:     "completed"
headSha:    0c2b5e86d30638be3483f853cfe6ede79a917dd1
```

| Job | Job ID | Result |
|---|---|---|
| Rust (macos-latest) | `106120186520` | ✅ 6m25s |
| Rust (windows-latest) | `106120186664` | ✅ 10m54s |
| Frontend | `106120186690` | ✅ 42s |
| Cargo audit (advisory) | `106120186702` | ✅ 3m23s |
| pnpm audit (advisory) | `106120186687` | ✅ 17s |
| version-consistency / Version consistency | `106120186728` | ✅ 7s |

**Required checks, each confirmed individually:**

- macOS job green ✅
- Windows job green ✅
- Frontend green ✅
- cargo audit green ✅
- pnpm audit green ✅
- version-consistency green ✅
- platform-neutral tests green ✅
- **Windows `test_b` green** ✅ — the log contains
  `test test_b_play_transitions_both_to_playing ... ok`
- **no unexpected ignored-test increase** ✅ — 2 ignored on both platforms, the same two
  pre-existing `providers::chrome::*` tests

Per-platform Rust totals read from the `test result:` lines (not summed from the log):

| Platform | lib | ignored | filtered |
|---|---|---|---|
| macOS | 489 passed | 2 | 1 |
| Windows | 480 passed | 2 | 0 |

The Windows job also printed the explicit nine-name skip message, confirming the libmpv-dependent
tests were skipped **by name** rather than silently lost.

### Local gates (all run, all green)

| Gate | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | exit 0 (compiled `movie-party v0.9.9`) |
| `cargo test --all-features -- --test-threads=1` | **583 passed / 0 failed / 2 ignored**, 18 targets |
| `tsc --noEmit` | exit 0 |
| `vitest run` | **25 files / 317 tests passed** |
| `vite build` | exit 0, 2284 modules, 12.71 s |
| `eslint . --max-warnings=0` | exit 0 (1m32s) |

The 583/0/2 figure is **identical to the Batch 11 baseline**, and the two ignored tests are the same
pre-existing ones — there is no ignored-test increase.

---

## 4. Part B — release workflow audit (re-read from the current tree)

Every claim below was re-verified against the files on disk, not taken from a previous report.

| Requirement | Verdict | Evidence |
|---|---|---|
| `release.yml` is correctly gated | ✅ | line 46–47 defines the `version-consistency` job via `uses: ./.github/workflows/version-consistency.yml`; line 74 `publish-tauri: needs: [resolve-matrix, version-consistency]` |
| `build.yml` creates no accidental uncontrolled release path | ✅ | `on: workflow_dispatch` only — **no `push:` trigger at all**; the release-attach step is guarded by `if: startsWith(github.ref, 'refs/tags/v')` |
| version-consistency executes on the actual release path | ✅ | called by `uses:` from `release.yml`, which is the only workflow that fires automatically (`push: tags: ["v*"]`) |
| Windows DLL integrity verification present | ✅ | SHA-256 `7310560BD25CC760282E8754A389302629C6E748261030C3870E1CB2F80CDC48`, checked **in both** `build.yml:54` and `release.yml:155`, `throw` on mismatch |
| macOS source verification present | ✅ | **six** `fetch_verified` calls (`build-libmpv-macos.sh` lines 61/85/89/104/118/130) with six uppercase 64-hex pins (lines 62/86/90/105/119/131) — ffmpeg, libplacebo, Vulkan-Headers, harfbuzz, libass, mpv |
| Release footer present | ✅ | `release.yml:227` `cat "$SECTION" docs/RELEASE_BODY_FOOTER.md > "$BODY"`; `release.yml:221` fails with `::error title=Missing release body footer` if absent; `docs/RELEASE_BODY_FOOTER.md` is 49 lines |
| Gatekeeper / notarization wording truthful | ✅ | `tauri.conf.json` has **no signing identity and no notarization config**; the docs describe ad-hoc signing and the Gatekeeper first-launch steps rather than claiming notarization |
| No updater falsely advertised | ✅ | 0 hits for `plugin-updater` / `tauri-plugin-updater` / `createUpdaterArtifacts` in config+Cargo+package; 0 tracked `latest.json`; 0 tracked `.sig`; `tauri.conf.json` plugins contain only `deep-link` |
| No secrets committed | ✅ | sweep clean — every 32-hex hit is a `Cargo.lock` crate checksum; zero token prefixes; zero credential assignments |

### Lint coverage (actionlint + shellcheck), with negative controls

Both tools were acquired as standalone release binaries into `/tmp/tools` because `brew install` is
blocked here (writes to `/opt/homebrew` are denied).

```
$ /tmp/tools/actionlint -no-color -shellcheck /tmp/tools/shellcheck-v0.11.0/shellcheck .github/workflows/*.yml
ACTIONLINT_EXIT=0        # output bytes: 0
```

```
$ shellcheck -x scripts/build-libmpv-macos.sh   -> clean (exit 0)
$ shellcheck -x scripts/make-test-media-macos.sh -> clean (exit 0)
$ shellcheck -x scripts/stage-libmpv-macos.sh    -> clean (exit 0)
```

**The checks were validated with working negative controls**, because a checker that cannot fail
proves nothing:

- actionlint on a deliberately broken workflow (an undefined `matrix.os` reference) → **exit 1** with
  a real diagnostic. So exit 0 on the real workflows means something.
- shellcheck: an initial control (`foo=bar; echo $foo`) **did not fail**, which was investigated
  rather than accepted — `echo $var` is exempt from SC2086 by design, so that was a bad control.
  Unambiguous controls (`rm $x`; `for f in $(ls *.txt)`) correctly exit 1.

**Also corrected during this audit:** an earlier `ACTIONLINT_EXIT=0` reading came from
`cmd | tail; echo $?`, which reports **tail's** status, not actionlint's. The figure above is from
`cmd > /tmp/out 2>&1; rc=$?`, and the negative control confirms the tool actually discriminates.

---

## 5. Part D — candidate artifacts

### macOS — built and verified locally

Both bundle targets were produced from the candidate:

```
target/release/bundle/dmg/Movie Party_0.9.9_aarch64.dmg       16,149,396 bytes
```

> **A note on the `.app`:** the `.app` was produced at
> `target/release/bundle/macos/Movie Party.app` and verified there (architecture, `Info.plist`
> version, 13 dylibs, dependency closure). Tauri's DMG bundling step then **deletes it** — the build
> log ends with `Cleaning …/bundle/macos/Movie Party.app` — so that path is now empty and the `.app`
> survives only **inside the DMG**. This does not weaken any check below: the DMG was mounted and its
> payload inspected directly, which is the stronger test.

| Check | Result |
|---|---|
| macOS architecture | **arm64** — `lipo -info` on the executable, and on all 13 bundled dylibs |
| macOS version | `CFBundleShortVersionString` = **0.9.9**, `CFBundleVersion` = **0.9.9** |
| bundled runtime presence | `Contents/Resources/mpv_runtime/` present with 13 dylibs + 8 license files + `NOTICE.txt` |
| expected dylibs | `libmpv.dylib` + `libass.9`, `libavcodec.62`, `libavfilter.11`, `libavformat.62`, `libavutil.60`, `libfreetype.6`, `libfribidi.0`, `libharfbuzz.0`, `libplacebo.360`, `libpng16.16`, `libswresample.6`, `libswscale.9` — **13/13** |
| no accidental 0.9.8 metadata | literal `0.9.8` occurs **0 times** anywhere in the `.app` |
| checksums generatable | yes — see below |
| provenance traceable to the candidate | yes — hash chain below |

**Dependency closure is self-contained.** Every dependency of every bundled dylib is either a system
framework/`/usr/lib` library or an `@loader_path/` sibling; **no Homebrew or `/usr/local` path
appears anywhere in the bundle**. `libmpv.dylib` resolves all nine of its siblings via
`@loader_path/`, and the main executable carries no `LC_RPATH` (it loads libmpv at runtime through
the resource directory, as designed).

**Checksums (SHA-256):**

```
6acd87b9698f3ad848ace66d8353b37cd1f4e8ae0a8e37b77191f3fc1eda8628  Movie Party_0.9.9_aarch64.dmg
c6b658eb5d0ec748cdd82e37c97cb2fd0a84329cd29ceef4347d47a31f94057d  Movie Party.app/Contents/MacOS/movie-party
```

**The DMG was mounted and inspected — not inferred from its filename.** It contains
`Movie Party.app` plus the standard `Applications -> /Applications` symlink, and the app inside
reports `CFBundleShortVersionString` **0.9.9** with **13** dylibs. The binary inside the mounted DMG
hashes to `c6b658eb…`, **byte-identical** to the pre-bundle binary at `target/release/movie-party`
(and to the copy that was in `bundle/macos/` before the DMG step removed it). That is a complete
provenance chain from the candidate commit to the shipped installer payload.

> **Correction to a previously recorded belief.** The project skill stated the DMG "always fails here,
> even with sandbox escalation" and that a DMG can never be verified on this machine. **Both are
> wrong.** The DMG fails under the sandbox — `bundle_dmg.sh` must mount a writable image at
> `/Volumes/Movie Party/` — but succeeds when the sandbox is lifted. It was built and verified here.
> The skill has been corrected.

**DMG payload string searches are meaningless and were not used as evidence.** The image is UDZO
(zlib-compressed), so `grep -aF "0.9.9"` inside it returns 0. The version is carried by the filename
and by the contained app's `Info.plist`.

### Windows — not buildable on this host, built via CI instead

The Windows NSIS installer requires a Windows host; it **cannot** be produced on macOS. It was
obtained instead through the real workflow path, **without creating a tag or a release**:

```
gh workflow run build.yml --ref stabilization/v0.9.9-rc1
→ run 35527521397, headSha 0c2b5e86d30638be3483f853cfe6ede79a917dd1
```

This is safe **because of the guard in the workflow**, which was read before dispatching:

```yaml
      - name: Attach installer to GitHub Release (v* tags only)
        if: startsWith(github.ref, 'refs/tags/v')
        uses: softprops/action-gh-release@v2
```

Dispatched at a **branch** ref, `github.ref` is `refs/heads/stabilization/v0.9.9-rc1`, so the release
step is skipped and only `actions/upload-artifact@v4` runs. **No release is created or modified.**

> Windows artifact result: **the run succeeded and the installer was produced and uploaded as a
> workflow artifact, with the release-attach step skipped.** Note that `build.yml` runs **no tests
> and has no version gate**, so a successful Windows installer proves the Windows *build* works — it
> does not prove Windows *behaviour*.

**Verified from the run, step by step:**

| Step | Status |
|---|---|
| Download and stage libmpv (Windows) | ✅ success (SHA-256 check passed) |
| `pnpm install --frozen-lockfile` | ✅ success |
| `pnpm tauri build` | ✅ success |
| **Upload NSIS installer artifact** | ✅ **success** |
| **Attach installer to GitHub Release (v\* tags only)** | ⏭️ **skipped** |

The release-attach step being `skipped` is the whole point: it is the mechanical proof that this run
produced an artifact **without touching any release**.

**What the Windows build produced** (read from the CI log, which is the authoritative record):

```
Running makensis to produce ...\bundle\nsis\Movie Party_0.9.9_x64-setup.exe
   Finished 1 bundle at:
       D:\a\Movie_Party\Movie_Party\target\release\bundle\nsis\Movie Party_0.9.9_x64-setup.exe
Built application at: D:\a\Movie_Party\Movie_Party\target\release\movie-party.exe
Artifact Movie-Party-Windows-x64 has been successfully uploaded! Final size is 40245265 bytes.
```

| Check | Result |
|---|---|
| Windows architecture | **x64** — installer named `…_x64-setup.exe`; toolchain host `x86_64-pc-windows-msvc` |
| Windows version | **0.9.9** — carried by the installer filename |
| no accidental 0.9.8 metadata | `0.9.8` occurs **0 times** in the whole build log; `0.9.9` occurs **8 times** |
| installer produced | yes — `Movie Party_0.9.9_x64-setup.exe`, artifact `Movie-Party-Windows-x64`, **40,245,265 bytes**, artifact ID `10610493592` |
| built from the candidate SHA | yes — `headSha` `0c2b5e86d30638be3483f853cfe6ede79a917dd1` |
| no stale Rust cache | the log shows the full dependency tree compiling from scratch (`proc-macro2`, `syn`, … at 17:59Z), so nothing was reused from an earlier version |

**The artifact was deliberately not downloaded to this machine** — three attempts were killed by the
sandbox's file-approval path (`SIGTERM`/137). That is an environment limit, not a project problem; the
log evidence above is complete and is what the run actually recorded.

> A note on reading that log: it contains ANSI escape sequences (e.g. `^[[1m^[[92m`), so a grep for a
> contiguous phrase like `Compiling movie-party` finds nothing even though the text is visibly
> present. Use `perl -pe 's/\x1b\[[0-9;]*m//g'` rather than BSD `sed`, which silently ignores `\x1b`.

**Do not publish.** Per the batch, no release was published and no artifact was attached to any
release.

---

## 6. Part E — manual beta validation procedure

`BATCH7B_REAL_BETA_VALIDATION.md` was updated to point at the current candidate.

| Item | Value |
|---|---|
| SHA referenced by the doc | `260cb06d2914b5133e1562712d661f2a0054209e` |
| Stale SHAs removed | `2d5c83391dec43342bb4346ae19c542a6e672385` (Batch 6) — no longer present |
| NOT TESTED rows | **61** — all preserved |
| PASS result claims | **0** |
| 0.9.9 references | 3 |

**The doc's SHA is a *source anchor*, and this distinction matters.** `260cb06` is the last commit
that touched `src/` or `src-tauri/`; branch HEAD is `0c2b5e8`, which only touched the markdown. The
doc therefore gives the tester a mechanical equivalence check rather than asking them to trust a
number:

```bash
git rev-parse HEAD                                   # what you actually built
git diff --stat 260cb06..HEAD -- src/ src-tauri/     # must print nothing
```

Both SHAs are therefore valid to build from, and the check proves it. **The SHA that would be tagged
is the branch HEAD, `0c2b5e86d30638be3483f853cfe6ede79a917dd1`.**

**The doc's own equivalence claim was tested:** `git diff --stat 260cb06..HEAD -- src/ src-tauri/`
prints nothing, because `0c2b5e8` changed only the validation document.

Every one of the 15 occurrences of the word "PASS" in the document is an *instruction* ("Do not claim
a test passes merely because the relevant code exists"), not a recorded result. The evidence rules
were preserved intact: screen recording / photos, player clock, Debug HUD where available, diagnostic
bundle, connection diagnostics, and the requirement that **both devices run the same exact SHA**.

### A finding that invalidates one class of manual step

**The application cannot display its own version.** `src/views/SettingsView.tsx:240` sets
`appVersion: info.appName` inside `<Row label="App version">`, and the backing `app_metadata()`
command (`src-tauri/src/lib.rs`) returns only `{ app_name, protocol_major, protocol_minor }` — there
is **no version field on the wire at all** (`src/backend/appRuntime.ts`'s `AppMetadataInfo` confirms
it). So the row labelled "App version" shows the app *name*.

Any manual procedure that asks a tester to "confirm the version shown in Settings" is **invalid** —
the version must be read from the bundle (`CFBundleShortVersionString`) or the installer filename.
This is a genuine defect but is **out of scope for a version bump** and was deliberately not fixed;
it belongs in its own batch.

---

## 7. v0.9.8 immutability proof

`v0.9.8` is annotated, so the **tag object** is the thing to compare, not the commit.

```
local  tag object : 4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3
remote tag object : 4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3   <- identical
dereferences to   : bb225778434e3f07923e03e85d7d7cc1db79146c
remote main       : bb225778434e3f07923e03e85d7d7cc1db79146c   <- unchanged
```

The tag was not moved, deleted, force-updated or rewritten, and the v0.9.8 GitHub Release was not
touched. The candidate is a **descendant** of `bb225778`, which is itself the proof that no history
was rewritten to produce it.

> Note: **local `main` is legitimately behind `origin/main`** (`2d5c833` vs `bb22577`, shown by
> `git branch -vv` as `[origin/main: ahead 4]`). That is a stale local branch pointer, not drift in
> the frozen release, and `main` was never pushed. It was deliberately left alone.

---

## 8. Confirmation: no v0.9.9 tag, no v0.9.9 release

```
local tags matching v0.9.9  : 0
remote tags matching v0.9.9 : 0
releases matching 0.9.9     : 0
```

Remote tags remain exactly: `v0.9.0`, `v0.9.4`, `v0.9.5`, `v0.9.6`, `v0.9.8`.

**No v0.9.9 tag was created, locally or on the remote. No v0.9.9 GitHub Release was created, and no
artifact was attached to any release.** The batch stops at candidate preparation.

---

## 9. Limitations stated plainly

1. **The Windows installer was built but not run.** It was produced by CI on `windows-latest` and
   uploaded as a workflow artifact; nothing on this machine can execute it, and three attempts to
   download the 40 MB artifact were killed by the sandbox's file-approval path. All Windows evidence
   therefore comes from the **CI log and the job's step table** — which is authoritative for *what was
   built*, and says nothing about *behaviour*. "Built and uploaded" is not "verified on Windows".
2. **No two-device beta run has happened.** Every row in the manual matrix remains NOT TESTED. Green
   CI cannot see real synchronized playback — the libmpv-dependent tests are skipped in CI by
   design, so the playback paths run **zero tests** on both runners.
3. **The macOS artifacts here were built on this machine, not by the release workflow.** They prove
   the bundle configuration and metadata are correct and that the payload is traceable to the
   candidate commit. The release workflow is what will produce the published artifacts, and it was
   **not** executed (doing so would publish).
4. **The DMG build requires the sandbox to be lifted.** Under the sandbox it fails at the
   `hdiutil`/`bundle_dmg.sh` mount step.
5. **One pre-existing defect found and deliberately not fixed:** the Settings "App version" row shows
   the app name (§6).
6. **One observation not fully explained:** the release executable contains no `0.9.x` version
   literal, although the crate's `rlib` does. The release profile is `lto = true`,
   `codegen-units = 1`, `strip = true`; the mechanism was **not** established and is reported as an
   observation, not a cause. It does not affect the bundle's version metadata, which is correct.

---

## 10. What happens next (not done, and not authorised by this batch)

To release v0.9.9: tag the branch HEAD, push the tag (which triggers `release.yml`), let the gated
`publish-tauri` job build both platforms, and verify the release page rather than the log. **None of
that was done.** The candidate is frozen at `0c2b5e86d30638be3483f853cfe6ede79a917dd1`.
