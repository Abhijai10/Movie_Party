# Milestone 4 Worklog — SQLite Persistence

## Status: 🟨 IN PROGRESS

## Architecture

### SQLite Database (`src-tauri/src/storage/sqlite.rs`)
- rusqlite with bundled feature
- WAL journal mode, foreign keys enabled
- Idempotent schema migrations via PRAGMA user_version
- Schema v1: device_identity, schedules, cache_entries, chat_messages tables

### Production DB Path (`src-tauri/src/app_runtime.rs`)
- macOS: `~/Library/Application Support/Move Party/move_party.db`
- Windows: `%LOCALAPPDATA%/Move Party/move_party.db`
- Linux: `~/.local/share/move-party/move_party.db`
- Opened on Tauri setup via `init_db()`

### Identity Persistence
- Device identity (device_id, display_name, public_key, platform) persisted on startup
- Survives application restart (tested with file-based DB)

### Preload Calculation
- `calculate_preload_start(remaining_bytes, goodput_bps, scheduled_start_utc_ms)`
- Formula: remaining_bytes / goodput × 1.4 + 15min margin
- Returns UTC ms at which preloading should begin
- Capped at now if result is in the past

### Overdue Detection
- `overdue_schedules()` returns Planned schedules past their preload time
- Useful for detecting missed preloads after restart

### Cache Retention
- `retention_keep(media_id)`: no-op, cache remains
- `retention_remove(media_id)`: deletes cache entry from DB

## Tests (11 total)

### Original 7
1. creates_database_and_runs_migrations
2. persists_and_retrieves_identity
3. persists_and_lists_schedules
4. persists_and_manages_cache_entries
5. persists_and_retrieves_chat_messages
6. duplicate_schedule_id_is_idempotent
7. restart_persistence_with_file

### New 4 (this run)
8. preload_calculation_basic
9. preload_calculation_zero_goodput_returns_scheduled_time
10. overdue_schedules_returns_planned_past_preload
11. retention_remove_deletes_cache_entry

## Files
- `src-tauri/src/storage/sqlite.rs` — MovePartyDb, migrations, CRUD
- `src-tauri/src/storage/mod.rs` — Sqlite error variant
- `src-tauri/src/app_runtime.rs` — init_db(), default_db_path()
- `src-tauri/src/lib.rs` — Tauri setup hook

## External Verification Needed
1. Run app → verify DB created at platform data dir
2. Quit and restart → verify identity persists
3. Create schedule → restart → verify schedule persists
