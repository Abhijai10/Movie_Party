# Milestone 4 Worklog — SQLite Persistence

## Status: 🟩 LOCALLY COMPLETE (external OS notification verification pending)

## Architecture

### SQLite Database (`src-tauri/src/storage/sqlite.rs`)
- rusqlite with bundled feature
- WAL journal mode, foreign keys enabled
- Idempotent schema migrations via PRAGMA user_version
- Schema v2: device_identity (now with signing_key_seed), schedules,
  cache_entries, chat_messages tables

### Production DB Path (`src-tauri/src/app_runtime.rs`)
- macOS: `~/Library/Application Support/Move Party/move_party.db`
- Windows: `%LOCALAPPDATA%/Move Party/move_party.db`
- Linux: `~/.local/share/move-party/move_party.db`
- Test override: `init_db_at_path()` — tests never rely on environment
  variables (they raced across parallel test binaries).
- Opened on Tauri setup via `init_db()`

### Identity Persistence (M4.1)
- v2 migration persists the 32-byte ed25519 `signing_key_seed` alongside the
  device id / public key.
- `init_db()`: open DB → load existing identity → if present restore it
  (`DeviceIdentity::from_seed`) and use it for the whole process lifetime;
  otherwise create exactly one identity and persist it.
- The runtime identity is now `Mutex<DeviceIdentity>` and is replaced by the
  restored identity — the ephemeral identity is never used after restore.
- AppRuntime-level restart test: Runtime A init → identity → drop → Runtime B
  same DB → exact same device id + same signing key (public key matches).

### Schedule CRUD (M4.2)
- AppRuntime methods + Tauri commands:
  `create_schedule`, `list_schedules`, `update_schedule_media`,
  `update_schedule_preload`, `delete_schedule`.
- DTO validation (MP-SCHEDULE-002): empty room/media/guest ids rejected,
  preload deadline must precede scheduled start.
- Schedules persist real room/media/guest/time data and survive restart.

### Real Scheduler Worker (M4.3)
- `spawn_scheduler_worker()` on production startup (Tauri setup hook).
- Restores pending schedules, waits efficiently for the next canonical preload
  deadline, executes preload when due (status → Transferring + notification),
  reschedules after updates, never double-executes (exactly-once via the
  persisted Planned→Transferring transition), and recovers overdue schedules
  after restart.
- Injectable/poll-interval test hooks for deterministic tests.
- Tests: future schedule does not run early; due executes exactly once;
  overdue executes after restart; update shifts the deadline; delete prevents
  execution.

### Notifications (M4.4)
- `Notifier` trait seam: `NativeNotifier` (macOS osascript / Windows
  PowerShell toast), `FakeNotifier` (in-memory, for tests — no OS
  notification is ever displayed in unit tests), `FailingNotifier`
  (always errors, proving failure is recoverable and never crashes the
  scheduler).
- `AppRuntime::new_with_notifier()` injects the seam.

### Retention (M4.5)
- Runtime paths + Tauri commands:
  - Keep: retains Move Party cache.
  - Remove: deletes ONLY Move Party cache dir + metadata.
  - Save As: exports cached media to a chosen destination safely.
- All three verified to never delete or overwrite the host's original source.

## Tests (249 Rust total; M4 added 13)

- storage/sqlite.rs: identity_seed_round_trips_blob, updated migration tests
- tests/m4_closure.rs (10):
  - appruntime_identity_restart_matches_exact_device_id
  - appruntime_identity_is_created_exactly_once
  - appruntime_schedule_crud_persists_and_survives_restart
  - appruntime_schedule_create_validates_dto
  - scheduler_future_schedule_does_not_run_early
  - scheduler_due_schedule_executes_once
  - scheduler_overdue_schedule_executes_after_restart
  - scheduler_update_changes_deadline_and_delete_prevents_execution
  - scheduler_notifications_use_mock_and_recover_on_failure
  - retention_runtime_keep_remove_save_as_preserve_host_source
- notifications/mod.rs: fake_records, failing_is_recoverable

## Files

- `src-tauri/src/storage/sqlite.rs` — v2 migration, due/next-deadline helpers,
  schedule media/preload updates
- `src-tauri/src/identity/mod.rs` — from_seed / seed accessors
- `src-tauri/src/app_runtime.rs` — identity restore, schedule CRUD, scheduler
  worker, retention paths, notifier seam
- `src-tauri/src/notifications/mod.rs` — Notifier abstraction
- `src-tauri/src/lib.rs` — Tauri commands + scheduler startup
- `src-tauri/tests/m4_closure.rs` — M4 integration tests

## External Verification Needed

1. Run app → verify DB created at platform data dir
2. Quit and restart → verify identity persists
3. Create schedule → restart → verify schedule persists + preload fires
4. macOS notification appears (osascript dispatch)
5. Windows toast appears (PowerShell dispatch)
