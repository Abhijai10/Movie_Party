# Movie Party

Private two-person desktop cinema app for synchronized local media and provider playback.

Movie Party V1 is governed by the locked specifications in `docs/core_docs/`.

## Development

```bash
pnpm install
pnpm dev
pnpm build
pnpm test
pnpm lint
```

Rust validation lives in `src-tauri/`:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Shared TMDB key (build-time)

The Home hero's trending posters use TMDB. The **shared key** is baked into the app at
build time so every install (yours and your friend's) shows posters out of the box,
while **never being committed** to the repo:

1. **Local builds**: create `.env` (git-ignored) with `VITE_TMDB_TOKEN=<your key>`.
   See `.env.example`. Use either the v3 API key (32 chars) or the v4 read access
   token (starts with `eyJ`).
2. **Release builds**: add a repository secret named `VITE_TMDB_TOKEN`
   (GitHub → Settings → Secrets and variables → Actions → New repository secret).
   `.github/workflows/release.yml` passes it to the build.

**If the shared key stops working**, any device can paste a replacement key in
**Settings → General → "Trending posters (TMDB)"** — a pasted key always overrides the
bundled one on that device, no rebuild needed. With no key at all the app falls back to
the built-in gradient wall (no posters, no network).

## Release builds

Releases are cut by `.github/workflows/release.yml` — on a `v*` tag push, or by manual dispatch
against an existing tag. It builds both platforms and attaches the artifacts to a GitHub Release:

- **macOS (Apple Silicon)**: a `.dmg`, plus a `.app.tar.gz` copy of the bundle. The workflow compiles
  the LGPL `libmpv` runtime from pinned sources and stages it into the bundle, so **no system `mpv` is
  involved** — neither to build the release nor to run it.
- **Windows 11 x64**: an NSIS `.exe` installer. It embeds the frontend and a self-contained `libmpv`
  runtime (`mpv-2.dll`), so it runs without Git, Rust, Node, or pnpm on the target machine.

The release body is generated from the tag's own section of `RELEASE_NOTES.md`, never hand-written
into the workflow. See `docs/RELEASE_PROCESS.md`.

> **Local release builds need the runtime staged first.** `pnpm tauri build` on macOS produces a
> bundle that contains whatever is in `src-tauri/mpv_runtime/` at build time. That directory is
> gitignored, so a clean checkout has no runtime and the resulting app cannot play anything. Run
> `./scripts/stage-libmpv-macos.sh` first (it needs a local libmpv build —
> `./scripts/build-libmpv-macos.sh`). A system-wide `mpv` install is *not* used and does not help.

### Installing the macOS build (first launch)

**The macOS build is ad-hoc signed and NOT notarized.** It has no Apple Developer ID signature and no
notarization ticket, because notarizing requires a paid Apple Developer account. Gatekeeper therefore
refuses the first launch. Depending on the macOS version you will see one of:

* *"Movie Party" is damaged and can't be opened. You should move it to the Trash.*
* *"Movie Party" cannot be opened because Apple cannot check it for malicious software.*
* *Apple could not verify "Movie Party" is free of malware.*

**None of these mean the download is corrupt or the app is unsafe.** They mean Apple has not vetted
this build. To open it:

1. Open the `.dmg` and drag **Movie Party** into **Applications**.
2. In **Applications**, **Control-click** (or right-click) the app → **Open**.
3. Click **Open** in the dialog that appears.

That is needed **once**. Afterwards it launches normally, including by double-click.

If you prefer the terminal, clear the quarantine flag instead:

```bash
xattr -d com.apple.quarantine "/Applications/Movie Party.app"
```

Do **not** disable Gatekeeper system-wide. And do not treat the "damaged" wording as a build problem —
it is the standard Gatekeeper response to any unnotarized app.

You also need **Tailscale installed separately** (<https://tailscale.com/download>), signed in and on
the same tailnet as your partner. Movie Party does not bundle it.

### Installing the Windows testing build

On a **Windows 11 x64** machine the user needs:

1. Windows 11 x64
2. The Movie Party installer (NSIS `.exe` from the GitHub Actions run / release)
3. **Tailscale installed separately** (https://tailscale.com/download) — Movie Party does not
   bundle Tailscale
4. Tailscale signed in and connected to the appropriate tailnet (or the same tailnet as the
   partner device)

No Git, Rust, Node, pnpm, or repository clone is required to install or run the app.

> **Status note:** the release build is available and distributable, but physical two-device
> verification (Local Perfect playback, strict synchronization, deep-link join on real Windows
> and macOS machines) is **pending**. Implementation completeness is code-level; it is not yet
> production-verified end to end.

## Updates

**Movie Party has no update mechanism, by design.** There is no Tauri updater plugin, no update
signing key, and no `latest.json` manifest anywhere in this repository, and the app never checks for
or installs an update. **Updating means downloading the new release and replacing the app.**

`Movie.Party_aarch64.app.tar.gz` is published alongside the `.dmg` as a plain compressed copy of the
`.app` bundle — useful if you want the raw bundle without mounting a disk image. It is **not** an
auto-update artifact: nothing signs it, nothing consumes it, and the app never looks for it. Its
filename is not version-stamped either, so it does not identify which release it came from beyond the
release page it sits on.

See `docs/RELEASE_PROCESS.md` for the release process, the signing situation, and what enabling a
real updater would require.
