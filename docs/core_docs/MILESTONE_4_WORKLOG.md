# Milestone 4 Worklog — SQLite Persistence

## Status: 🟩 LOCALLY COMPLETE (external OS notification/Keychain verification pending)

---

## Architecture

### SQLite Database (`src-tauri/src/storage/sqlite.rs`)
- rusqlite with bundled feature, WAL, foreign keys, idempotent migrations.
- Tables: device_identity (metadata ONLY — never private key material),
  schedules, cache_entries, chat_messages.
- `StoredIdentity` carries `key_label` — a reference to the OS-protected
  secret entry, never raw key material.

### Secure Device Private Key (M4.3)
- New `SecureKeyStore` abstraction (`src-tauri/src/secure/mod.rs`).
- macOS: login Keychain via `security add/find/delete-generic-password`.
- Windows: Credential Manager via WinRT `PasswordVault` through PowerShell.
- Tests use `FakeKeyStore`; unit tests never touch the real OS secret store.
- The 32-byte Ed25519 signing seed is NEVER stored in SQLite.

### Identity Persistence (M4.1)
- `init_db()`: open DB → load identity metadata → load private key from the
  OS-protected store → if both match, restore the exact same identity
  (device id + signing key) for the whole process lifetime; otherwise create
  exactly one identity (key → protected store, metadata → SQLite).
- AppRuntime-level restart test: Runtime A → Runtime B on the same DB +
  shared key store → exact same device id + public key.

### Identity Error Handling (M4.4)
- `Ok(None)` (first run) and `Err` (read failure) are handled separately.
  `Err` surfaces MP-STORE-001 — it does NOT silently rotate the identity.
- `upsert_identity` failures propagate: `persist_identity` records
  MP-STORE-001 instead of ignoring the result.

### Pre-v2 / Missing-Key Migration (M4.5)
- If the private key cannot be recovered (missing entry, corrupt seed, or
  public-key mismatch), `rotate_identity` performs an explicit coherent
  rotation: brand-new device id + brand-new keypair; the old metadata row and
  old protected entry are removed. An old device id is NEVER paired with
  unrelated key material.
- `from_seed_for_tests` is not used by any production/migration code.

### Real Scheduler Worker (M4.3 / M4.1)
- `PreloadExecutor` seam (`src-tauri/src/scheduling/preload.rs`):
  - `AppRuntimePreloadExecutor` (production): checks a QUIC peer is online,
    then spawns the actual `guest_fetch_media` Local Perfect preload.
  - `FakePreloadExecutor` (tests): records invocations; deterministic
    outcomes prove the executor is invoked — not merely a counter.
- Scheduler flow per due schedule: atomically claim (Planned → Claimed) →
  invoke executor →
  - Started → persist Transferring + notify
  - WaitingForPrerequisites → persist WaitingForPeer + notify user + retry
    (due_schedules includes WaitingForPeer)
  - Err → persist PreloadFailed (recoverable; scheduler keeps running)
- Tests prove: due executes exactly once via executor invocation; waiting
  persists and retries; executor failure is recoverable.

### Production Overdue-Startup Bug (M4.2)
- `check_overdue_schedules` is now notification-only — it NEVER mutates
  schedule status. The scheduler worker is the single authority that claims
  and executes due schedules, so an overdue schedule can no longer be made
  invisible to the scheduler.
- Integration test uses the REAL production startup ordering
  (init_db → check_overdue_schedules → setup_real_preload_executor →
  spawn_scheduler_worker) with an overdue Planned schedule: executor invoked
  exactly once, status Transferring.

### Notifications (M4.4 / M4.6)
- `Notifier` trait: `NativeNotifier`, `FakeNotifier` (unit tests — no OS
  notification), `FailingNotifier` (failure is recoverable).
- macOS dispatch: text travels as real `on run argv` arguments — never
  interpolated into AppleScript source.
- Windows dispatch: toast XML is XML-escaped in Rust and passed to PowerShell
  as base64 — hostile characters cannot become PowerShell code.
- Tests cover quotes, apostrophes, `&`, `<`, `>`, newlines.

### Schedule CRUD (M4.2)
- AppRuntime + Tauri commands: create, list, update (media + preload
  deadline), delete with MP-SCHEDULE-002 DTO validation. Schedules survive
  restart.

### Retention (M4.5)
- Runtime + Tauri paths for Keep / Remove / Save As; verified to never delete
  or overwrite the host's original source.

---

## Tests (263 Rust total)

- secure/mod.rs: fake store round-trip, missing label
- scheduling/preload.rs: fake executor records invocations
- notifications/mod.rs: xml_escape, toast_xml, native dispatch args,
  fake/failing notifier
- storage/sqlite.rs: identity key-label round-trip, migrations, CRUD,
  retention, overdue/due/next-deadline
- tests/m4_closure.rs (18):
  - appruntime_identity_restart_matches_exact_device_id
  - appruntime_identity_is_created_exactly_once
  - appruntime_identity_rotates_coherently_when_key_missing
  - appruntime_identity_store_read_failure_is_surfaced_not_rotated
  - appruntime_schedule_crud_persists_and_survives_restart
  - appruntime_schedule_create_validates_dto
  - scheduler_future_schedule_does_not_run_early
  - scheduler_due_schedule_executes_once
  - scheduler_overdue_schedule_executes_after_restart
  - scheduler_update_changes_deadline_and_delete_prevents_execution
  - scheduler_waiting_for_peer_persists_and_retries
  - scheduler_executor_failure_is_recoverable
  - production_startup_executes_overdue_schedule_exactly_once
  - scheduler_notifications_use_mock_and_recover_on_failure
  - retention_runtime_keep_remove_save_as_preserve_host_source

---

## Files

- `src-tauri/src/secure/mod.rs` — SecureKeyStore (Keychain / Credential Manager / fake)
- `src-tauri/src/scheduling/preload.rs` — PreloadExecutor + fakes
- `src-tauri/src/storage/sqlite.rs` — identity metadata, scheduler queries
- `src-tauri/src/app_runtime.rs` — identity restore/rotation, scheduler with
  real executor, retention, notifier/key-store seams
- `src-tauri/src/notifications/mod.rs` — hardened native dispatch
- `src-tauri/src/lib.rs` — Tauri commands, startup ordering
- `src-tauri/tests/m4_closure.rs` — integration tests

---

## External Verification Needed

1. Run app → verify DB created at platform data dir; quit + restart → identity
   persists via the real Keychain/Credential Manager.
2. Create schedule → restart → preload fires via the real executor.
3. macOS notification appears; Windows toast appears.
4. First launch on Windows/macOS → Keychain/Credential Manager prompts.
