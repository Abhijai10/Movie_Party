# Move Party Integration Status

Updated: 2026-08-21

## Completed

- React boots from `get_app_snapshot` and subscribes to `app_snapshot_pushed`.
- Room creation and joining flow through Tauri commands into `AppRuntime`.
- Frontend screen transitions for Home, Join, and End confirmation are backend-owned.
- Local file party creation uses the real Local Perfect host path: manifest creation, QUIC room, player open boundary, and runtime snapshot state.
- Provider URL entry creates a real room first, then routes to the provider runtime launch path.
- Chat and reactions flow through runtime commands and protocol-backed host/guest events.
- Call controls and signaling use runtime state and Tauri commands.
- Scheduler startup is safe under real Tauri startup through `tauri::async_runtime`.
- Provider launch failures return truthful provider `Unavailable` state.

## Tests Proving Local Integration

- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all`
- `pnpm test`
- `pnpm build`
- `pnpm tauri dev` startup smoke: app window opens.

## Pending External Verification

- Two physical machines.
- Windows host and guest.
- Real Tailscale network path outside dev loopback.
- Real multi-GB movie files.
- Real libmpv installation and visible playback lifecycle.
- macOS notification permission and dispatch.
- Windows toast notification dispatch.
- macOS camera/microphone permission prompts.
- Windows camera/microphone permission prompts.
- Installed Chrome discovery and launch on macOS and Windows.
- Dedicated Chrome provider profile login/session reuse.
- Live YouTube, Netflix, Prime, and JioHotstar playback detection/control.

## Blocked

- In-window native libmpv rendering remains blocked on native `NSView`/`CALayer`
  or `HWND` render-host attachment.
- Provider DRM/shared playback support cannot be claimed without real provider
  accounts, Chrome login, and platform playback verification.
- Full cross-platform readiness cannot be claimed until the Windows and
  two-device Tailscale matrix is executed.

## Next Action

Run the external verification matrix above before starting UI redesign.
