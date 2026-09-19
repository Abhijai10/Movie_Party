# Release process

How Movie Party is released, what the artifacts are, and what is deliberately *not* in place.
Written after the v0.9.8 post-mortem, which found three consecutive releases shipping the wrong
release notes.

---

## 1. Cutting a release

`.github/workflows/release.yml` is the only release path. It runs on:

* a **`v*` tag push**, or
* **manual dispatch** (`workflow_dispatch`) with a `tag` input — the recovery path when a tag's
  workflow needed a fix after the tag was pushed. Tags are **never** re-created. The dispatch
  builds the code at the selected ref and attaches the artifacts to the existing release, because
  `tauri-action` appends to an existing release rather than replacing it.

The `resolve-matrix` job decides which legs run (`all` / `macos` / `windows`), so a macOS-only
rebuild does not pay for the Windows build again. Legs:

| Leg | Runner | Target |
|---|---|---|
| macOS | `macos-latest` | `aarch64-apple-darwin` |
| Windows | `windows-latest` | `x86_64-pc-windows-msvc` |

### The libmpv runtime is staged per platform, not committed

`src-tauri/mpv_runtime/` is gitignored (large LGPL binaries), so **each leg stages its own runtime**
before building:

* **macOS** — `scripts/build-libmpv-macos.sh` compiles LGPL `ffmpeg`/`libplacebo`/`harfbuzz`/`libass`/
  `libmpv` from pinned sources with `@loader_path`-relative install names, then
  `scripts/stage-libmpv-macos.sh` installs them. Slow (~20–40 min), but license-correct: Homebrew's
  `mpv` is GPL and cannot be bundled into this proprietary app.
* **Windows** — downloads `mpv-winbuild-cmake`'s self-contained LGPL dev archive and stages
  `libmpv-2.dll` as `mpv-2.dll`, which is what `mpv_backend.rs` loads at runtime.

A **local** macOS release build needs the runtime staged by hand first
(`./scripts/stage-libmpv-macos.sh`); otherwise the bundle simply ships without it and plays nothing.
A system-wide `mpv` install is never used.

### The TMDB secret

`VITE_TMDB_TOKEN` (repo secret, never committed) is baked into the frontend at build time so both
installs get Home posters out of the box. A key pasted in Settings overrides it per device, so a
retired shared key never strands an install.

---

## 2. Release notes and the release body

**The body is generated; it is never written into the workflow.** The `Derive the release body from
RELEASE_NOTES.md` step:

1. takes the version from the tag (`v0.9.8` → `0.9.8`);
2. extracts that version's `## <version>` section from `RELEASE_NOTES.md`, up to the next `## `
   heading;
3. appends `docs/RELEASE_BODY_FOOTER.md`, which carries the parts that are identical for every
   release (downloads, the macOS first-launch warning, the no-auto-update statement);
4. passes the result to `tauri-action` as `releaseBody`.

**A missing section fails the release.** If `RELEASE_NOTES.md` has no `## <version>` section for the
tag, the step exits non-zero with an `::error` annotation. Publishing notes for the wrong version is
worse than publishing none.

### Why this exists

The body used to be a hardcoded string in `release.yml`. It was written for **v0.9.5** and then
reused **verbatim** for v0.9.6 and v0.9.8, so all three releases advertised v0.9.5's changes
("shared TMDB key", "compact ready buttons", "macOS call permissions") and said nothing about their
own. The published v0.9.8 body described work that had shipped two releases earlier.

**Do not paste a body into `releaseBody` again.** Version-specific prose belongs in
`RELEASE_NOTES.md`; release mechanics belong in `docs/RELEASE_BODY_FOOTER.md`.

### Adding a version

Add a new `## <version> (short title)` section at the **top** of `RELEASE_NOTES.md`. Never rewrite
older sections — they are the historical record and the source for that tag's body.

---

## 3. macOS signing and Gatekeeper

**Current state: ad-hoc signed, NOT notarized.** There is no Apple Developer ID signature and no
notarization ticket, because notarization requires a paid Apple Developer account.

Consequences, and what to tell users:

* Gatekeeper **will** refuse the first launch. The wording varies by macOS version — most commonly
  *"Movie Party" is damaged and can't be opened*, or *"cannot be opened because Apple cannot check it
  for malicious software"*, or *"Apple could not verify…"*.
* **The "damaged" wording is not a build defect.** It is the standard response to any unnotarized
  app, and it is the single most likely thing a tester will report as a broken download.
* The user opens it once via **Control-click → Open → Open**, or clears the flag with
  `xattr -d com.apple.quarantine "/Applications/Movie Party.app"`.
* Users must **not** be told to disable Gatekeeper system-wide.

The README carries the user-facing walkthrough; the release body carries a short version of it.

**Notarizing would require**: an Apple Developer Program membership, a Developer ID Application
certificate, and a `codesign` + `notarytool submit --wait` + `stapler staple` pass in the macOS leg of
`release.yml` with the certificate and an app-specific password in CI secrets. Until that exists, every
macOS release needs the first-launch workaround.

**This is spec-sanctioned, not an oversight.** `docs/core_docs/MASTER_PRD.md` (status: *Architecture
Locked for V1*) carries "PHASE 32 — OPTIONAL RELEASE HARDENING", explicitly gated on "only if the
project proves worthwhile", listing `signed macOS build`, `code signing`, `auto update`, `crash
reporting` and `proper onboarding` as deferred. The PRD is a locked document — do not edit it to match
reality; update this file instead.

---

## 4. Artifacts

| Artifact | Published | Notes |
|---|---|---|
| `Movie.Party_<version>_aarch64.dmg` | yes | the macOS distribution artifact |
| `Movie.Party_<version>_x64-setup.exe` | yes | Windows 11 x64 NSIS installer |
| `Movie.Party_aarch64.app.tar.gz` | yes | see below |

### Decision: the `.app.tar.gz` stays, documented as not-an-updater-artifact

It exists because `bundle.targets` includes `"app"`; it is **not** produced by any updater feature.
It is a plain compressed copy of the `.app` bundle, and the decision is to **keep publishing it**
while saying plainly what it is, because:

* it is the only way to obtain the raw `.app` without mounting a `.dmg`;
* removing it means changing `bundle.targets`, and `dmg` is built from the `app` output — a build
  configuration change with real risk and no user-facing benefit;
* the actual problem was never its existence, it was the **silence**. An unexplained
  `*.app.tar.gz` next to a `.dmg` reads exactly like a Tauri updater artifact, and users could
  reasonably wait for an auto-update that will never come.

So it is now labelled in both the README and the release body as a portable archive, explicitly
**not** an update artifact, and explicitly unsigned.

If the maintainer would rather stop publishing it, the change is to drop `"app"` from
`bundle.targets` in `src-tauri/tauri.conf.json` and confirm the `.dmg` still builds — verify on a real
release, not in CI, because the `dmg` target depends on the `app` output.

---

## 5. Updates: there are none

**Movie Party has no update mechanism**, and per `MASTER_PRD.md` Phase 32 that is deferred by design
(see §3) rather than an unfinished feature. Verified absent from the repository:

| Required for auto-update | Present? |
|---|---|
| `tauri-plugin-updater` in `src-tauri/Cargo.toml` | no |
| `@tauri-apps/plugin-updater` in `package.json` | no |
| `plugins.updater` (`pubkey` + `endpoints`) in `tauri.conf.json` | no |
| Update signing keypair / `.sig` files | no |
| `latest.json` manifest | no |
| `bundle.createUpdaterArtifacts` | no |

The app never checks for an update and never installs one. **Updating means downloading the new
release and replacing the app**, and the release body says so.

**Enabling a real updater would require all of:** the plugin on both sides; a keypair from
`tauri signer generate` with the private key in CI secrets and the public key in `tauri.conf.json`;
`bundle.createUpdaterArtifacts: true` so `.sig` files are produced and uploaded; a `latest.json`
published at the configured endpoint; and — realistically — **macOS code signing and notarization
first**, because an updater that swaps the `.app` in place cannot work reliably against an adhoc-signed
bundle that Gatekeeper refuses.

Do not advertise the tarball as an update artifact in the meantime.

---

## 6. After publishing: verification checklist

Run these against the **release page**, not the workflow log.

- [ ] Both platform artifacts are present and `uploaded`: the `.dmg` (or `.exe`) for each leg that was
      supposed to run.
- [ ] The **release body matches the tag's version** and describes *that* release's changes. This is
      the check that v0.9.5–v0.9.8 all failed — read it, do not assume it.
- [ ] The body contains the macOS first-launch section and the no-auto-update statement (i.e. the
      footer was appended).
- [ ] `git rev-list -n1 <tag>` still points at the intended commit, and no other tag moved.

```bash
gh release view <tag> --json assets --jq '.assets[] | "\(.name)  \(.state)"'
gh release view <tag> --json body --jq '.body' | head -20
```

If a leg fails after the build succeeded (e.g. a transient `Error saving asset` on upload), re-run only
the failed job — `gh run rerun <run-id> --failed` — rather than re-dispatching the whole workflow.
