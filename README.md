# Move Party

Private two-person desktop cinema app for synchronized local media and provider playback.

Move Party V1 is governed by the locked specifications in `docs/core_docs/`.
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
