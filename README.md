# Movie Party

Private two-person desktop cinema app for synchronized local media and provider playback.

Movie Party V1 is governed by the locked specifications in `docs/core_docs/`.
Implementation proceeds phase by phase according to `docs/core_docs/IMPLEMENTATION_TRACKER.md`.

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

## Release builds

- **macOS**: `pnpm tauri build` produces a `.app` and `.dmg` (requires `mpv` available on the
  system; see `0_Remaining_Things.md` for the known native-presentation gap).
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
