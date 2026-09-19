# Batch 6 — Release and installation documentation hygiene

**Status:** complete. All four findings addressed — three in-repo, and the fourth (the manual beta
checklist) located outside the repo and corrected there. Documentation and workflow only — no
application behaviour changed. `v0.9.8` tag untouched, the existing v0.9.8 GitHub Release **not**
edited, no retag, no force-push. Provider Shared untouched.

---

## 1. Files changed

| File | Change |
|---|---|
| `.github/workflows/release.yml` | new *Derive the release body from RELEASE_NOTES.md* step; `releaseBody` is now derived, not hardcoded |
| `docs/RELEASE_BODY_FOOTER.md` | **new** — the release-invariant part of the body (downloads, macOS first-launch, no-auto-update) |
| `docs/RELEASE_PROCESS.md` | **new** — the release process, signing reality, artifact decisions, updater status, verification checklist |
| `README.md` | new *Installing the macOS build (first launch)* section; new *Updates* section; corrected a stale build claim |
| `~/Downloads/Movie_Party_v0.9.8_MANUAL_BETA_CHECKLIST.md` | **outside the repo, untracked** — release-availability claims corrected to the actual v0.9.8 state (§2) |

---

## 2. The manual beta checklist — located outside the repo, and fixed

The brief described *"the v0.9.8 manual beta checklist"* containing stale statements that the release
was not available because GitHub runners were blocked.

**Where it is:** `~/Downloads/Movie_Party_v0.9.8_MANUAL_BETA_CHECKLIST.md` — **not in the repository
and not on GitHub**, which is why the first search pass (repo + release bodies + issues) found nothing.
The location was recorded in this project's own daily memory logs from 2026-09-17/18 all along.

**What was stale, and what it now says.** Three places claimed the release did not exist:

| Line | Was | Now |
|---|---|---|
| 7 | *"`v0.9.8` IS tagged … but **NOT released**. No DMG/installer exists yet because GitHub Actions cannot allocate runners"* | ✅ tagged **and released** 2026-09-17, naming the three attached assets |
| 14 | *"**Resolve the GitHub billing block** … Until then CI cannot start and no release artifact exists. This has blocked seven attempts"* | struck through as **DONE** — runners were allocated, the `Release` workflow completed successfully; "do not go looking for a billing problem" |
| 15–19 | *"Then produce the artifacts … re-run the release"* with a `gh workflow run release.yml` recipe | **download** them instead — `gh release view` / `gh release download`; nothing to build |
| 217 | *"**CI green** on GitHub (blocked on billing)"* | **not green, and not a billing block** — CI ran on 2026-09-17 and **failed on the Windows leg** (`test_b_play_transitions_both_to_playing`, *"host predicate not satisfied within 30s"*); macOS passed, and no CI run has happened since |

A line was also added to Precondition 2 noting that `Movie.Party_aarch64.app.tar.gz` is a plain archive
of the `.app` and **not** an update artifact, matching §6.

Verified against ground truth rather than memory: `gh release view v0.9.8` reports `publishedAt
2026-09-17T17:00:00Z`, `draft=false`, `prerelease=false`, three assets all `uploaded`; `gh run list`
shows `Release → success` at 16:47Z and `CI → failure` at 16:45Z on the same day.

**Change is 4 edits, 230 → 238 lines, nothing else touched** (verified with `diff` against a
`shasum`-checked backup at `/tmp/checklist_v098_backup.md`). The checklist is an untracked personal
file, so it is **not** part of the commit.

**Process lesson recorded:** before concluding that a named artifact does not exist, read this project's
recent daily memory logs — they name where the out-of-repo docs live. Searching only the repo and
GitHub cost a wasted pass.

---

## 3. Stale claims removed

1. **`release.yml`'s hardcoded release body.** It was a string matching the **v0.9.5** section of
   `RELEASE_NOTES.md` *verbatim*, and it was reused unchanged for v0.9.5, v0.9.6 **and** v0.9.8:

   | Release | Body length | Content |
   |---|---|---|
   | v0.9.4 | 1543 | hand-written highlights + downloads table |
   | v0.9.5 | 385 | the hardcoded string |
   | v0.9.6 | 385 | **identical** |
   | v0.9.8 | 385 | **identical** |

   So three consecutive releases advertised v0.9.5's changes ("shared TMDB key", "compact ready
   buttons", "macOS call permissions") and said nothing about their own. The published v0.9.8 body
   described work that shipped two releases earlier.

2. **README: "macOS: `pnpm tauri build` produces a `.app` and `.dmg` (requires `mpv` available on the
   system)."** Wrong on both counts. Release builds use the **bundled** runtime compiled by the
   workflow (`scripts/build-libmpv-macos.sh`, LGPL, pinned sources); a system-wide `mpv` is never used
   and does not help. Replaced with an accurate description plus a note that a *local* release build
   must stage the runtime first or the bundle ships with no player at all.

3. **README: no macOS install instructions existed.** There was a Windows install section and nothing
   for macOS — for an artifact that Gatekeeper refuses on first launch.

---

## 4. Release process improved

The body is now **generated**:

1. the version comes from the tag (`v0.9.8` → `0.9.8`);
2. that version's `## <version>` section is extracted from `RELEASE_NOTES.md` (awk, with a boundary
   check so `0.9.8` cannot match `0.9.80`);
3. `docs/RELEASE_BODY_FOOTER.md` is appended — the parts that are identical for every release;
4. the result is emitted as a multiline `GITHUB_OUTPUT` and passed to `tauri-action` as `releaseBody`.

**A missing section fails the release** with an `::error` annotation and a non-zero exit, instead of
publishing an empty or stale body. Publishing notes for the wrong version is worse than publishing
none.

The workflow carries a comment telling the next person not to paste a body back into `releaseBody`, and
`docs/RELEASE_PROCESS.md` §2 records *why* — so the v0.9.5–v0.9.8 failure mode is not repeated.

Version-specific prose lives in `RELEASE_NOTES.md`; release mechanics live in
`docs/RELEASE_BODY_FOOTER.md`; the process itself is documented in `docs/RELEASE_PROCESS.md`.

---

## 5. macOS signing / Gatekeeper, explained honestly

The README and the release body now state plainly that the macOS build is **ad-hoc signed and NOT
notarized** — no Developer ID signature, no notarization ticket — and that **Gatekeeper will refuse the
first launch**. Both name the actual messages a user will see, including the misleading
*"Movie Party is damaged and can't be opened"*, and say explicitly that it does **not** mean the
download is corrupt or unsafe. Both give the real fix (Control-click → Open → Open, or
`xattr -d com.apple.quarantine`), and both warn against disabling Gatekeeper system-wide.

The docs never claim notarization, and never suggest the artifact is anything other than what it is.

**Spec basis:** `MASTER_PRD.md` "PHASE 32 — OPTIONAL RELEASE HARDENING" (status: *Architecture Locked
for V1*) lists `signed macOS build`, `code signing`, `auto update`, `crash reporting` and
`proper onboarding` as deferred — "only if the project proves worthwhile". So the missing signing is
**spec-sanctioned, not an oversight**. The PRD is locked, so it is cited rather than edited.

---

## 6. Updater status, and the `.app.tar.gz` decision

**There is no updater.** Verified absent from the repository:

| Required for auto-update | Present? |
|---|---|
| `tauri-plugin-updater` in `src-tauri/Cargo.toml` | no |
| `@tauri-apps/plugin-updater` in `package.json` | no |
| `plugins.updater` (`pubkey` + `endpoints`) in `tauri.conf.json` | no |
| update signing keypair / `.sig` files | no |
| `latest.json` manifest | no |
| `bundle.createUpdaterArtifacts` | no |

The app never checks for or installs an update. Nothing in the README, the release body or the process
doc advertises the tarball as an update artifact.

**Decision: keep publishing `Movie.Party_aarch64.app.tar.gz`, and label it.** It exists because
`bundle.targets` includes `"app"`, not because of any updater feature. It is kept because:

* it is the only way to obtain the raw `.app` without mounting a `.dmg`;
* removing it means editing `bundle.targets`, and the `dmg` target is built from the `app` output — a
  build-configuration change with real risk and no user-facing benefit.

The actual defect was never its existence but the **silence**: an unexplained `*.app.tar.gz` sitting
next to a `.dmg` reads exactly like a Tauri updater artifact, and a user could reasonably wait for an
auto-update that will never arrive. It is now described in both the README and the release body as a
portable archive of the `.app`, explicitly **not** an update artifact and explicitly unsigned.

`docs/RELEASE_PROCESS.md` §4 also records the exact change to stop publishing it (drop `"app"` from
`bundle.targets` and confirm the `.dmg` still builds), so the maintainer can decide the other way
without re-deriving it — and notes it must be verified on a real release, not in CI, because of the
`dmg`→`app` dependency.

What enabling a real updater would require is written out in §5 of that doc, including the point that
it realistically needs macOS signing/notarization **first**, since an updater that swaps the `.app` in
place cannot work against an adhoc-signed bundle Gatekeeper refuses.

---

## 7. Validation

Lightweight and appropriate for a documentation batch:

* **The workflow's derivation script was run for real**, extracted verbatim from the YAML rather than
  retyped:
  * `TAG_OVERRIDE=v0.9.8` → exit **0**, 145-line body, correctly delimited in `GITHUB_OUTPUT`;
  * `TAG_OVERRIDE=v9.9.9` → exit **1** with the `::error` annotation — the guard works, so a release
    with no matching notes section is blocked rather than shipped with stale notes.
* `release.yml` parses as YAML; the notes step precedes `tauri-action`; `releaseBody` is
  `${{ steps.notes.outputs.body }}`; the stale v0.9.5 text is absent from the file.
* Every path referenced by the new docs exists.
* The three removed claims are confirmed absent with `git grep -F`.

No Rust or frontend gate is affected: this batch changes one workflow and three documents, no code.

*Note for the next session:* PyYAML parses the `on:` key as boolean `True` (YAML 1.1), so a naive
`d['on']` raises `KeyError: 'on'`. Read it as `d.get('on', d.get(True))`.

---

## 8. Confirmation of the freeze

* `v0.9.8` → **`bb22577`** — unchanged.
* The **existing v0.9.8 GitHub Release was not modified.** Its body still carries the stale v0.9.5
  text. That is deliberate: the brief says not to modify the existing Release, and the fix is for
  *future* releases. If the owner wants the published v0.9.8 body corrected, that is a separate,
  explicitly-authorised edit.
* All five tags intact: `v0.9.0`, `v0.9.4`, `v0.9.5`, `v0.9.6`, `v0.9.8`. No retag, no force-push,
  nothing pushed.
* **Provider Shared untouched** — no file under `src/` was modified by this batch.
