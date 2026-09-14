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

- **macOS**: `pnpm tauri build` produces a `.app` and `.dmg` (requires `mpv` available on the
  system).
- **Windows**: a GitHub Actions workflow (`.github/workflows/build.yml`) builds a Windows 11
  x64 NSIS installer. The installer embeds the frontend and a self-contained `libmpv` runtime
  (`mpv-2.dll`) so it runs without Git, Rust, Node, or pnpm on the target machine.

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
