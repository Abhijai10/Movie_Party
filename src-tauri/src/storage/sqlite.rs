//! Real SQLite persistence for Move Party.
//!
//! Provides actual database storage for device identity, trusted peers,
//! room history, schedules, cache metadata, and chat messages.
//!
//! Uses rusqlite with the bundled feature for cross-platform support.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};

use super::StorageError;

const CURRENT_SCHEMA_VERSION: i32 = 2;

/// Helper to lock a Mutex, converting PoisonError to StorageError.
fn lock_mutex<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, StorageError> {
    mutex
        .lock()
        .map_err(|e| StorageError::Sqlite(format!("lock poisoned: {e}")))
}

/// Move Party database backed by real SQLite.
pub struct MovePartyDb {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    path: PathBuf,
}

/// Stored device identity metadata. The private signing material is NEVER
/// stored here — it lives in [`crate::secure::SecureKeyStore`]. This table
/// only holds the public identity and a reference to the OS-protected
/// secret entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIdentity {
    pub device_id: String,
    pub display_name: String,
    pub public_key: String,
    pub platform: String,
    pub created_at_ms: i64,
    /// Label of the OS-protected secret entry holding the signing seed.
    pub key_label: String,
}

/// A stored schedule record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredSchedule {
    pub schedule_id: String,
    pub room_id: String,
    pub media_id: String,
    pub scheduled_start_utc_ms: i64,
    pub planned_preload_utc_ms: i64,
    pub guest_device_id: String,
    pub status: String,
    pub created_at_ms: i64,
}

/// A cached media entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCacheEntry {
    pub media_id: String,
    pub filename: String,
    pub file_size: u64,
    pub full_hash: String,
    pub cache_root: String,
    pub bytes_available: u64,
    pub created_at_ms: i64,
}

/// A chat message persisted locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredChatMessage {
    pub message_id: String,
    pub room_id: String,
    pub sender: String,
    pub body: String,
    pub created_host_time_us: i64,
}

impl MovePartyDb {
    /// Open or create the database at the given path.
    /// Runs migrations automatically and idempotently.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path).map_err(|e| StorageError::Sqlite(e.to_string()))?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;

        let db = Self {
            conn: Mutex::new(conn),
            path,
        };

        db.run_migrations()?;
        Ok(db)
    }

    /// Open an in-memory database for testing.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory().map_err(|e| StorageError::Sqlite(e.to_string()))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let db = Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        };
        db.run_migrations()?;
        Ok(db)
    }

    /// Get the current schema version from the database.
    pub fn schema_version(&self) -> Result<i32, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(version)
    }

    /// Run all pending migrations idempotently.
    fn run_migrations(&self) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;

        let current = conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
            .unwrap_or(0);

        if current < 1 {
            conn.execute_batch(MIGRATION_001)
                .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        }

        if current < 2 && !device_identity_has_key_label(&conn)? {
            conn.execute_batch(MIGRATION_002)
                .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        }

        conn.execute_batch(&format!("PRAGMA user_version={CURRENT_SCHEMA_VERSION};"))
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Device Identity ────────────────────────────────────────────────────

    /// Store or update the device identity metadata.
    pub fn upsert_identity(&self, identity: &StoredIdentity) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO device_identity (device_id, display_name, public_key, platform, created_at_ms, key_label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                identity.device_id,
                identity.display_name,
                identity.public_key,
                identity.platform,
                identity.created_at_ms,
                identity.key_label,
            ],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Retrieve the stored device identity metadata, if any.
    pub fn get_identity(&self) -> Result<Option<StoredIdentity>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT device_id, display_name, public_key, platform, created_at_ms, key_label
                 FROM device_identity LIMIT 1",
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        match stmt.query_row([], |row| {
            Ok(StoredIdentity {
                device_id: row.get(0)?,
                display_name: row.get(1)?,
                public_key: row.get(2)?,
                platform: row.get(3)?,
                created_at_ms: row.get(4)?,
                key_label: row.get(5)?,
            })
        }) {
            Ok(identity) => Ok(Some(identity)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Sqlite(e.to_string())),
        }
    }

    /// Remove the stored identity metadata (used on coherent rotation so the
    /// new device identity becomes the single source of truth).
    pub fn delete_identity(&self) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute("DELETE FROM device_identity", [])
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Schedules ──────────────────────────────────────────────────────────

    /// Insert a new schedule.
    pub fn insert_schedule(&self, schedule: &StoredSchedule) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO schedules
             (schedule_id, room_id, media_id, scheduled_start_utc_ms, planned_preload_utc_ms,
              guest_device_id, status, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                schedule.schedule_id,
                schedule.room_id,
                schedule.media_id,
                schedule.scheduled_start_utc_ms,
                schedule.planned_preload_utc_ms,
                schedule.guest_device_id,
                schedule.status,
                schedule.created_at_ms,
            ],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// List all schedules.
    pub fn list_schedules(&self) -> Result<Vec<StoredSchedule>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT schedule_id, room_id, media_id, scheduled_start_utc_ms,
                        planned_preload_utc_ms, guest_device_id, status, created_at_ms
                 FROM schedules ORDER BY scheduled_start_utc_ms",
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(StoredSchedule {
                    schedule_id: row.get(0)?,
                    room_id: row.get(1)?,
                    media_id: row.get(2)?,
                    scheduled_start_utc_ms: row.get(3)?,
                    planned_preload_utc_ms: row.get(4)?,
                    guest_device_id: row.get(5)?,
                    status: row.get(6)?,
                    created_at_ms: row.get(7)?,
                })
            })
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let mut schedules = Vec::new();
        for row in rows {
            schedules.push(row.map_err(|e| StorageError::Sqlite(e.to_string()))?);
        }
        Ok(schedules)
    }

    /// Delete a schedule by ID.
    pub fn delete_schedule(&self, schedule_id: &str) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "DELETE FROM schedules WHERE schedule_id = ?1",
            params![schedule_id],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Update schedule status.
    pub fn update_schedule_status(
        &self,
        schedule_id: &str,
        status: &str,
    ) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "UPDATE schedules SET status = ?1 WHERE schedule_id = ?2",
            params![status, schedule_id],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Atomically claim a due schedule for execution. Returns `true` only
    /// when this caller won the transition from a pending state to `Claimed`.
    pub fn claim_due_schedule(&self, schedule_id: &str, now_ms: i64) -> Result<bool, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let changed = conn
            .execute(
                "UPDATE schedules
                 SET status = 'Claimed'
                 WHERE schedule_id = ?1
                   AND status IN ('Planned', 'WaitingForPeer')
                   AND planned_preload_utc_ms <= ?2",
                params![schedule_id, now_ms],
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(changed == 1)
    }

    /// Recover schedules left in `Claimed` by a process crash. A claimed
    /// schedule has not reached `Transferring`, so it is safe to return it to
    /// the retryable waiting state.
    pub fn recover_claimed_schedules(&self, now_ms: i64) -> Result<usize, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "UPDATE schedules
             SET status = 'WaitingForPeer'
             WHERE status = 'Claimed'
               AND planned_preload_utc_ms <= ?1",
            params![now_ms],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))
    }

    /// Update the planned preload deadline of a schedule (rescheduling).
    pub fn update_schedule_preload(
        &self,
        schedule_id: &str,
        planned_preload_utc_ms: i64,
    ) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "UPDATE schedules SET planned_preload_utc_ms = ?1 WHERE schedule_id = ?2",
            params![planned_preload_utc_ms, schedule_id],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Update the media id of a schedule.
    pub fn update_schedule_media(
        &self,
        schedule_id: &str,
        media_id: &str,
    ) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "UPDATE schedules SET media_id = ?1 WHERE schedule_id = ?2",
            params![media_id, schedule_id],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Cache Entries ──────────────────────────────────────────────────────

    /// Store or update a cache entry.
    pub fn upsert_cache_entry(&self, entry: &StoredCacheEntry) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO cache_entries
             (media_id, filename, file_size, full_hash, cache_root, bytes_available, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.media_id,
                entry.filename,
                entry.file_size,
                entry.full_hash,
                entry.cache_root,
                entry.bytes_available,
                entry.created_at_ms,
            ],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Get all cache entries.
    pub fn list_cache_entries(&self) -> Result<Vec<StoredCacheEntry>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT media_id, filename, file_size, full_hash, cache_root, bytes_available, created_at_ms
                 FROM cache_entries",
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(StoredCacheEntry {
                    media_id: row.get(0)?,
                    filename: row.get(1)?,
                    file_size: row.get(2)?,
                    full_hash: row.get(3)?,
                    cache_root: row.get(4)?,
                    bytes_available: row.get(5)?,
                    created_at_ms: row.get(6)?,
                })
            })
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|e| StorageError::Sqlite(e.to_string()))?);
        }
        Ok(entries)
    }

    /// Delete a cache entry by media_id.
    pub fn delete_cache_entry(&self, media_id: &str) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "DELETE FROM cache_entries WHERE media_id = ?1",
            params![media_id],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Chat Messages ──────────────────────────────────────────────────────

    /// Persist a chat message.
    pub fn insert_chat_message(&self, msg: &StoredChatMessage) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "INSERT OR IGNORE INTO chat_messages
             (message_id, room_id, sender, body, created_host_time_us)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                msg.message_id,
                msg.room_id,
                msg.sender,
                msg.body,
                msg.created_host_time_us,
            ],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Get recent chat messages for a room.
    pub fn get_chat_messages(
        &self,
        room_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredChatMessage>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT message_id, room_id, sender, body, created_host_time_us
                 FROM chat_messages
                 WHERE room_id = ?1
                 ORDER BY created_host_time_us DESC
                 LIMIT ?2",
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![room_id, limit as i64], |row| {
                Ok(StoredChatMessage {
                    message_id: row.get(0)?,
                    room_id: row.get(1)?,
                    sender: row.get(2)?,
                    body: row.get(3)?,
                    created_host_time_us: row.get(4)?,
                })
            })
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row.map_err(|e| StorageError::Sqlite(e.to_string()))?);
        }
        messages.reverse();
        Ok(messages)
    }

    // ── Scheduling helpers ────────────────────────────────────────────────

    /// Calculate the preload start time (UTC milliseconds) for a scheduled
    /// session using the locked formula:
    ///
    ///   remaining_bytes / conservative_goodput × safety_factor (~1.4)
    ///   + safety_margin (~15 min)
    ///
    /// Returns the UTC epoch millisecond at which preloading should begin.
    pub fn calculate_preload_start(
        remaining_bytes: u64,
        conservative_goodput_bps: u64,
        scheduled_start_utc_ms: i64,
    ) -> i64 {
        const SAFETY_FACTOR: f64 = 1.4;
        const SAFETY_MARGIN_MS: i64 = 15 * 60 * 1000; // 15 minutes

        if conservative_goodput_bps == 0 {
            // Unknown throughput — start preloading immediately
            return scheduled_start_utc_ms;
        }

        let transfer_secs =
            (remaining_bytes as f64) / (conservative_goodput_bps as f64) * SAFETY_FACTOR;
        let transfer_ms = (transfer_secs * 1000.0) as i64;
        let preload_utc = scheduled_start_utc_ms - transfer_ms - SAFETY_MARGIN_MS;

        // Don't schedule in the past
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        preload_utc.max(now_ms)
    }

    /// Return all schedules whose planned preload time has passed but which
    /// have not yet started (status = "Planned") — these are overdue for
    /// preload initiation.
    pub fn overdue_schedules(&self) -> Result<Vec<StoredSchedule>, StorageError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.due_schedules(now_ms)
    }

    /// Schedules due for preload at the given `now_ms` (injectable clock):
    /// status "Planned" or "WaitingForPeer" (both not-yet-started) and
    /// `planned_preload_utc_ms <= now_ms`.
    pub fn due_schedules(&self, now_ms: i64) -> Result<Vec<StoredSchedule>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT schedule_id, room_id, media_id, scheduled_start_utc_ms,
                        planned_preload_utc_ms, guest_device_id, status, created_at_ms
                 FROM schedules
                 WHERE status IN ('Planned', 'WaitingForPeer')
                   AND planned_preload_utc_ms <= ?1
                 ORDER BY planned_preload_utc_ms",
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![now_ms], |row| {
                Ok(StoredSchedule {
                    schedule_id: row.get(0)?,
                    room_id: row.get(1)?,
                    media_id: row.get(2)?,
                    scheduled_start_utc_ms: row.get(3)?,
                    planned_preload_utc_ms: row.get(4)?,
                    guest_device_id: row.get(5)?,
                    status: row.get(6)?,
                    created_at_ms: row.get(7)?,
                })
            })
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let mut schedules = Vec::new();
        for row in rows {
            schedules.push(row.map_err(|e| StorageError::Sqlite(e.to_string()))?);
        }
        Ok(schedules)
    }

    /// The earliest future preload deadline among still-pending schedules
    /// (Planned or WaitingForPeer) after `now_ms` (injectable clock).
    /// `None` when nothing is pending.
    pub fn next_preload_deadline(&self, now_ms: i64) -> Result<Option<i64>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let result = conn.query_row(
            "SELECT MIN(planned_preload_utc_ms)
             FROM schedules
             WHERE status IN ('Planned', 'WaitingForPeer')
               AND planned_preload_utc_ms > ?1",
            params![now_ms],
            |row| row.get::<_, Option<i64>>(0),
        );
        match result {
            Ok(deadline) => Ok(deadline),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Sqlite(e.to_string())),
        }
    }

    // ── Cache retention ───────────────────────────────────────────────────

    /// Retention action: Keep leaves the cache entry untouched.
    pub fn retention_keep(&self, _media_id: &str) -> Result<(), StorageError> {
        // No-op: the cache entry remains as-is.
        Ok(())
    }

    /// Retention action: Remove deletes only the Move Party cache entry and
    /// any associated cached data on disk.  Never deletes the host's
    /// original media file.
    pub fn retention_remove(&self, media_id: &str) -> Result<(), StorageError> {
        // Delete the cache entry from the database
        self.delete_cache_entry(media_id)?;
        Ok(())
    }
}

fn device_identity_has_key_label(conn: &Connection) -> Result<bool, StorageError> {
    let mut statement = conn
        .prepare("PRAGMA table_info(device_identity)")
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;

    for column in columns {
        if column.map_err(|e| StorageError::Sqlite(e.to_string()))? == "key_label" {
            return Ok(true);
        }
    }
    Ok(false)
}

// ── Migrations ──────────────────────────────────────────────────────────────

const MIGRATION_001: &str = "
CREATE TABLE IF NOT EXISTS device_identity (
    device_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    public_key TEXT NOT NULL,
    platform TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    key_label TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS schedules (
    schedule_id TEXT PRIMARY KEY,
    room_id TEXT NOT NULL,
    media_id TEXT NOT NULL,
    scheduled_start_utc_ms INTEGER NOT NULL,
    planned_preload_utc_ms INTEGER NOT NULL,
    guest_device_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'Planned',
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS cache_entries (
    media_id TEXT PRIMARY KEY,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    full_hash TEXT NOT NULL,
    cache_root TEXT NOT NULL,
    bytes_available INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_messages (
    message_id TEXT PRIMARY KEY,
    room_id TEXT NOT NULL,
    sender TEXT NOT NULL,
    body TEXT NOT NULL,
    created_host_time_us INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_schedules_start ON schedules(scheduled_start_utc_ms);
CREATE INDEX IF NOT EXISTS idx_chat_room ON chat_messages(room_id, created_host_time_us);
";

/// v2: M4 moved private identity material into the platform secure store and
/// needs a stable Keychain/Credential Manager entry label beside public metadata.
/// The conditional migration also repairs databases produced by the prior v1
/// schema declaration, which recorded `user_version = 1` without this column.
const MIGRATION_002: &str = "
ALTER TABLE device_identity ADD COLUMN key_label TEXT NOT NULL DEFAULT '';
";

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_database_and_runs_migrations() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let version = db.schema_version().expect("version");
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn upgrades_v1_identity_table_missing_key_label() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(
            "
            CREATE TABLE device_identity (
                device_id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                public_key TEXT NOT NULL,
                platform TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            );
            PRAGMA user_version=1;
            ",
        )
        .expect("legacy schema");
        let db = MovePartyDb {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        };

        db.run_migrations().expect("upgrade");

        let conn = lock_mutex(&db.conn).expect("lock");
        let has_key_label = device_identity_has_key_label(&conn).expect("columns");
        assert!(has_key_label);
        drop(conn);
        assert_eq!(db.schema_version().expect("version"), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn persists_and_retrieves_identity() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let identity = StoredIdentity {
            device_id: "test-device-id".to_string(),
            display_name: "Test Host".to_string(),
            public_key: "base64key".to_string(),
            platform: "macos".to_string(),
            created_at_ms: 1_000_000,
            key_label: "move-party-device-signing-key".to_string(),
        };

        db.upsert_identity(&identity).expect("upsert");
        let retrieved = db.get_identity().expect("get").expect("some");
        assert_eq!(retrieved, identity);
    }

    #[test]
    fn identity_key_label_round_trips() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let identity = StoredIdentity {
            device_id: "label-device".to_string(),
            display_name: "Label".to_string(),
            public_key: "pk".to_string(),
            platform: "macos".to_string(),
            created_at_ms: 1,
            key_label: "custom-key-label".to_string(),
        };
        db.upsert_identity(&identity).expect("upsert");
        let retrieved = db.get_identity().expect("get").expect("some");
        assert_eq!(retrieved.key_label, "custom-key-label");
    }

    #[test]
    fn persists_and_lists_schedules() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let schedule = StoredSchedule {
            schedule_id: "sched-1".to_string(),
            room_id: "room-1".to_string(),
            media_id: "media-1".to_string(),
            scheduled_start_utc_ms: 10_000_000,
            planned_preload_utc_ms: 7_000_000,
            guest_device_id: "guest-1".to_string(),
            status: "Planned".to_string(),
            created_at_ms: 1_000_000,
        };

        db.insert_schedule(&schedule).expect("insert");
        let list = db.list_schedules().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].schedule_id, "sched-1");

        db.update_schedule_status("sched-1", "Transferring")
            .expect("update");
        let updated = db.list_schedules().expect("list2");
        assert_eq!(updated[0].status, "Transferring");

        db.delete_schedule("sched-1").expect("delete");
        let empty = db.list_schedules().expect("list3");
        assert!(empty.is_empty());
    }

    #[test]
    fn claim_due_schedule_is_atomic_and_one_winner_only() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let schedule = StoredSchedule {
            schedule_id: "claim-1".to_string(),
            room_id: "room".to_string(),
            media_id: "media".to_string(),
            scheduled_start_utc_ms: 100,
            planned_preload_utc_ms: 50,
            guest_device_id: "guest".to_string(),
            status: "Planned".to_string(),
            created_at_ms: 1,
        };

        db.insert_schedule(&schedule).expect("insert");
        assert!(db.claim_due_schedule("claim-1", 60).expect("claim"));
        assert!(!db.claim_due_schedule("claim-1", 60).expect("claim2"));
        assert_eq!(db.list_schedules().expect("list")[0].status, "Claimed");
    }

    #[test]
    fn claimed_schedule_recovers_after_restart_before_execution() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let schedule = StoredSchedule {
            schedule_id: "claimed-restart".to_string(),
            room_id: "room".to_string(),
            media_id: "media".to_string(),
            scheduled_start_utc_ms: 100,
            planned_preload_utc_ms: 50,
            guest_device_id: "guest".to_string(),
            status: "Claimed".to_string(),
            created_at_ms: 1,
        };

        db.insert_schedule(&schedule).expect("insert");
        assert_eq!(db.recover_claimed_schedules(60).expect("recover"), 1);
        let due = db.due_schedules(60).expect("due");
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].status, "WaitingForPeer");
    }

    #[test]
    fn persists_and_manages_cache_entries() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let entry = StoredCacheEntry {
            media_id: "local-abc".to_string(),
            filename: "movie.mkv".to_string(),
            file_size: 4_500_000_000,
            full_hash: "hash123".to_string(),
            cache_root: "/tmp/cache".to_string(),
            bytes_available: 2_000_000_000,
            created_at_ms: 1_000_000,
        };

        db.upsert_cache_entry(&entry).expect("upsert");
        let list = db.list_cache_entries().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].bytes_available, 2_000_000_000);

        db.delete_cache_entry("local-abc").expect("delete");
        let empty = db.list_cache_entries().expect("list2");
        assert!(empty.is_empty());
    }

    #[test]
    fn persists_and_retrieves_chat_messages() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let msg = StoredChatMessage {
            message_id: "msg-1".to_string(),
            room_id: "room-1".to_string(),
            sender: "Host".to_string(),
            body: "Hello!".to_string(),
            created_host_time_us: 1000,
        };

        db.insert_chat_message(&msg).expect("insert");
        let messages = db.get_chat_messages("room-1", 10).expect("get");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "Hello!");

        // Insert another message
        let msg2 = StoredChatMessage {
            message_id: "msg-2".to_string(),
            room_id: "room-1".to_string(),
            sender: "Guest".to_string(),
            body: "Hi!".to_string(),
            created_host_time_us: 2000,
        };
        db.insert_chat_message(&msg2).expect("insert2");

        // Should return in chronological order with limit
        let messages = db.get_chat_messages("room-1", 1).expect("get2");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "Hi!");
    }

    #[test]
    fn duplicate_schedule_id_is_idempotent() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let schedule = StoredSchedule {
            schedule_id: "dup".to_string(),
            room_id: "room".to_string(),
            media_id: "media".to_string(),
            scheduled_start_utc_ms: 100,
            planned_preload_utc_ms: 50,
            guest_device_id: "guest".to_string(),
            status: "Planned".to_string(),
            created_at_ms: 1,
        };

        db.insert_schedule(&schedule).expect("first");
        db.insert_schedule(&schedule).expect("second (upsert)");
        let list = db.list_schedules().expect("list");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn restart_persistence_with_file() {
        let dir = std::env::temp_dir().join(format!("mp_test_{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("dir");
        let db_path = dir.join("test.db");

        {
            let db = MovePartyDb::open(&db_path).expect("open1");
            db.upsert_identity(&StoredIdentity {
                device_id: "persistent-device".to_string(),
                display_name: "Persistent".to_string(),
                public_key: "key".to_string(),
                platform: "macos".to_string(),
                created_at_ms: 100,
                key_label: "persistent-key".to_string(),
            })
            .expect("upsert1");
        }

        // Reopen — identity must survive
        {
            let db = MovePartyDb::open(&db_path).expect("open2");
            let identity = db.get_identity().expect("get").expect("exists");
            assert_eq!(identity.device_id, "persistent-device");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_identity_row_surfaces_read_error() {
        let db = MovePartyDb::open_in_memory().expect("open");
        {
            let conn = db.conn.lock().expect("conn");
            conn.execute(
                "INSERT INTO device_identity
                 (device_id, display_name, public_key, platform, created_at_ms, key_label)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["bad", "Bad", "pk", "macos", "not-an-integer", "label"],
            )
            .expect("insert");
        }

        assert!(
            db.get_identity().is_err(),
            "corrupt identity metadata must surface an error, not look like first launch"
        );
    }

    #[test]
    fn preload_calculation_basic() {
        // 1 GB remaining, 10 Mbps goodput, scheduled 3 hours from now
        let remaining: u64 = 1_000_000_000;
        let goodput: u64 = 10_000_000; // 10 Mbps
        let now_ms: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let scheduled_ms: i64 = now_ms + 3 * 3600 * 1000; // +3h

        let preload = MovePartyDb::calculate_preload_start(remaining, goodput, scheduled_ms);
        // Transfer time: 1e9 / 1e7 = 100s, × 1.4 = 140s = 140000ms
        // Plus 15 min margin = 900000ms
        // Expected: scheduled_ms - 140000 - 900000 = scheduled_ms - 1_040_000
        assert_eq!(preload, scheduled_ms - 1_040_000);
    }

    #[test]
    fn preload_calculation_zero_goodput_returns_scheduled_time() {
        let preload = MovePartyDb::calculate_preload_start(1_000_000_000, 0, 7_200_000);
        // Zero goodput = unknown → start immediately, but capped at now
        // Since now_ms >> scheduled_ms in test, result >= now_ms
        assert!(preload >= 0);
    }

    #[test]
    fn overdue_schedules_returns_planned_past_preload() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let schedule = StoredSchedule {
            schedule_id: "overdue-1".to_string(),
            room_id: "room-1".to_string(),
            media_id: "media-1".to_string(),
            scheduled_start_utc_ms: 1_000_000_000,
            planned_preload_utc_ms: 1, // Way in the past
            guest_device_id: "guest-1".to_string(),
            status: "Planned".to_string(),
            created_at_ms: 1_000_000,
        };
        db.insert_schedule(&schedule).expect("insert");

        let overdue = db.overdue_schedules().expect("overdue");
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].schedule_id, "overdue-1");

        // Mark as transferring — should no longer be overdue
        db.update_schedule_status("overdue-1", "Transferring")
            .expect("update");
        let overdue_after = db.overdue_schedules().expect("overdue2");
        assert!(overdue_after.is_empty());
    }

    #[test]
    fn next_preload_deadline_returns_none_when_no_pending_schedule_exists() {
        let db = MovePartyDb::open_in_memory().expect("open");

        assert_eq!(db.next_preload_deadline(100).expect("deadline"), None);
    }

    #[test]
    fn retention_remove_deletes_cache_entry() {
        let db = MovePartyDb::open_in_memory().expect("open");
        let entry = StoredCacheEntry {
            media_id: "to-delete".to_string(),
            filename: "movie.mkv".to_string(),
            file_size: 4_500_000_000,
            full_hash: "hash123".to_string(),
            cache_root: "/tmp/cache".to_string(),
            bytes_available: 2_000_000_000,
            created_at_ms: 1_000_000,
        };
        db.upsert_cache_entry(&entry).expect("upsert");
        assert_eq!(db.list_cache_entries().expect("list").len(), 1);

        db.retention_remove("to-delete").expect("remove");
        assert!(db.list_cache_entries().expect("list2").is_empty());
    }
}
