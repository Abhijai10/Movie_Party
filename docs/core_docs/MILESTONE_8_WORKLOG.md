# Milestone 8 Worklog — Resilience + Final Cert

## Status: 🟨 IN PROGRESS

## Architecture

### Disconnect Recovery (`src-tauri/src/app_runtime.rs`)
- `apply_disconnect()` → `recovery_plan(FailureEvent::GuestCrash)` → `apply_recovery_to_state()`
- Sets `last_recovery` in snapshot (visible to UI)
- Pauses both participants
- Clears pending operations

### Transfer Stall Watcher (`src-tauri/src/app_runtime.rs`)
- `spawn_transfer_stall_watcher()`: background task polling every 5s
- Monitors `transfer.bytes_available` for changes
- After 30s with no change: fires `FailureEvent::TransferInterrupted`
- Runs automatically from `create_local_party()`
- Cleanup via `transfer_stall_watcher_task` abort in `leave_party()`

### Error Codes
- MP-NET-001/002/003: Network failures
- MP-MEDIA-001 through MP-MEDIA-007: Player failures
- MP-CALL-001: Invalid call signal
- MP-CAPTURE-001/002: Capture failures
- MP-PROVIDER-001/002/003: Provider failures
- MP-SYNC-001: Sync error
- MP-CTRL-001/002: Control errors

### Recovery Plans (15 failure types)
- GuestCrash → PauseBoth
- TransferInterrupted → RestoreTransferAndRebuildBuffer
- TransferResumed → ResumePlayback
- TailscaleDisconnect → PauseAndReconnect
- ChromeCrash → RelaunchChrome
- PlayerFailure → ReportError
- MissingLocalFile → RequireUserAction
- etc.

### Telemetry Sanitization
- `redact_log_line()` exists with tests
- Never logs passwords, cookies, auth headers, private keys

### Packaging
- `pnpm tauri build` → macOS .app bundle
- bundle.active enabled in tauri.conf.json
- 16.7MB binary

## What's Wired
- ✅ Disconnect → RecoveryPlan → last_recovery in snapshot
- ✅ Transfer stall watcher (30s threshold)
- ✅ Error codes in state/error field
- ✅ .app bundle builds successfully

## What's Not Wired
- ❌ Chrome crash watcher (Chrome process exit detection)
- ❌ Player failure watcher (mpv error detection loop)
- ❌ Cache corruption watcher
- ❌ SQLite failure detection
- ❌ Call track/device failure detection

## Files
- `src-tauri/src/app_runtime.rs` — apply_disconnect, spawn_transfer_stall_watcher
- `src-tauri/src/resilience/mod.rs` — FailureEvent, RecoveryPlan, recovery_plan()

## External Verification Needed
1. Real Tailscale disconnect → verify pause + recovery
2. Real transfer stall → verify TransferInterrupted fires
3. Run .app bundle → verify packaging works
