//! Real SQLite persistence for Movie Party.
//!
//! Provides actual database storage for device identity, trusted peers,
//! room history, schedules, cache metadata, and chat messages.
//!
//! Uses rusqlite with the bundled feature for cross-platform support.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};

use super::StorageError;

const CURRENT_SCHEMA_VERSION: i32 = 5;

/// Helper to lock a Mutex, converting PoisonError to StorageError.
fn lock_mutex<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, StorageError> {
    mutex
        .lock()
        .map_err(|e| StorageError::Sqlite(format!("lock poisoned: {e}")))
}

/// Movie Party database backed by real SQLite.
pub struct MoviePartyDb {
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
/// camelCase on the wire — the TS contract (StoredSchedule)
/// declares camelCase fields; snake_case here would deliver undefined at
/// runtime (same class of bug as the CameraState serde fix).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// A persisted Provider Shared capture diagnostic.
/// camelCase on the wire — the TS contract declares camelCase fields.
/// §38: `shared_available` reflects a REAL diagnostic on THIS
/// device, never a guess.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredProviderDiagnostic {
    pub provider_id: String,
    pub display_name: String,
    pub shared_available: bool,
    pub shared_reason: String,
    pub verified_at_ms: Option<i64>,
    pub sample_seconds: u16,
}

/// A saved movie-partner friend: a chosen Tailscale tailnet peer.
/// `peer_key` is the peer's full MagicDNS name (stable across IP changes);
/// the last verified probe result is kept so the Home panel can show honest
/// connection health without re-pinging on every render.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredFriend {
    /// Full Tailscale DNS name, e.g. "rahul-mac.tailc930b7.ts.net."
    pub peer_key: String,
    pub display_name: String,
    /// Last usable Tailscale CGNAT IPv4 observed for the peer.
    pub ip: Option<String>,
    pub added_at_ms: i64,
    /// Timestamp of the last successful `tailscale ping` verification.
    pub last_verified_at_ms: Option<i64>,
    /// Path reported by the last verification (direct / relay …).
    pub last_path: Option<String>,
    /// Latency (ms) reported by the last verification.
    pub last_latency_ms: Option<u32>,
    /// Where the friend is in the external-user invitation flow:
    /// INVITED | TAILSCALE_JOINED | MOVIE_PARTY_VERIFIED. ONLINE/OFFLINE
    /// is derived live from tailnet status, never stored.
    pub connection_state: FriendConnectionState,
}

/// The persisted stage of the Tailscale friend architecture.
/// Order: INVITED → TAILSCALE_JOINED → MOVIE_PARTY_VERIFIED; ONLINE and
/// OFFLINE are transient observations derived from the live tailnet
/// status and are intentionally NOT part of this persisted enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FriendConnectionState {
    /// Accepted from a movieparty://friend invite; the friend's device
    /// has not been observed in this device's tailnet status yet.
    Invited,
    /// The expected peer answered `tailscale status` with a usable
    /// address — the friend authenticated with their own Tailscale
    /// identity and the device joined the tailnet.
    TailscaleJoined,
    /// A real `tailscale ping` verified the tunnel end-to-end.
    MoviePartyVerified,
}

impl FriendConnectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FriendConnectionState::Invited => "INVITED",
            FriendConnectionState::TailscaleJoined => "TAILSCALE_JOINED",
            FriendConnectionState::MoviePartyVerified => "MOVIE_PARTY_VERIFIED",
        }
    }

    /// Parse the persisted label; unknown values fall back to INVITED
    /// (the honest earliest state, never a fabricated later one).
    pub fn from_persisted(value: &str) -> Self {
        match value {
            "TAILSCALE_JOINED" => FriendConnectionState::TailscaleJoined,
            "MOVIE_PARTY_VERIFIED" => FriendConnectionState::MoviePartyVerified,
            _ => FriendConnectionState::Invited,
        }
    }

    /// True once the friend's device joined the tailnet (state ≥ JOINED).
    pub fn joined(&self) -> bool {
        !matches!(self, FriendConnectionState::Invited)
    }
}

impl MoviePartyDb {
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

    /// Run arbitrary SQL against the open connection.
    ///
    /// Test-only, and deliberately so: it is the only way to induce a *genuine*
    /// storage failure (drop a table, break a constraint) so a failure-path
    /// test asserts on a real error instead of a happy path dressed up as one.
    #[cfg(test)]
    pub fn execute_sql_for_test(&self, sql: &str) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute_batch(sql)
            .map_err(|e| StorageError::Sqlite(e.to_string()))
    }

    /// Get the current schema version from the database.
    pub fn schema_version(&self) -> Result<i32, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        read_schema_version(&conn)
    }

    /// Run all pending migrations.
    ///
    /// Safety properties (F51 — the previous runner could brick a database
    /// permanently, reproduced against a scratch DB):
    ///
    /// * **Atomic per step.** Each migration body *and* its `user_version`
    ///   stamp commit together in one transaction. SQLite journals both DDL
    ///   and the `user_version` header write, so an interrupt mid-step rolls
    ///   the whole step back instead of leaving a half-applied schema.
    /// * **Resumable.** The version is stamped per migration, so a crash
    ///   during step N resumes at step N rather than replaying earlier ones.
    /// * **Idempotent `ADD COLUMN`.** A database that was physically altered
    ///   but still reports the old `user_version` — precisely the
    ///   interrupted-F51 state — is detected via [`column_exists`] and
    ///   resumed, instead of failing with `duplicate column name`.
    /// * **Never downgraded.** A database written by a newer build keeps its
    ///   version; we refuse to touch it rather than rewriting it backwards.
    /// * **Errors are surfaced**, never swallowed.
    fn run_migrations(&self) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;

        // Propagate the read failure — never default it to 0. A `unwrap_or(0)`
        // here is worse than it looks: 0 is *below* every supported version, so
        // a database whose version could not be read would sail straight past
        // the `SchemaTooNew` guard below and have migrations run against a
        // schema this build does not understand. Failing loudly keeps the guard
        // meaningful.
        let current = read_schema_version(&conn)?;

        if current > CURRENT_SCHEMA_VERSION {
            return Err(StorageError::SchemaTooNew {
                found: current,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }

        for version in (current + 1)..=CURRENT_SCHEMA_VERSION {
            Self::apply_migration(&conn, version)?;
        }

        Ok(())
    }

    /// Apply exactly one forward migration and stamp its version, atomically.
    ///
    /// On failure the transaction is rolled back so the next launch retries
    /// this step from a known-good state, and the original error propagates.
    fn apply_migration(conn: &Connection, version: i32) -> Result<(), StorageError> {
        exec_sql(conn, "BEGIN IMMEDIATE")?;

        let outcome = Self::apply_migration_body(conn, version)
            .and_then(|()| exec_sql(conn, &format!("PRAGMA user_version={version};")));

        match outcome {
            Ok(()) => exec_sql(conn, "COMMIT"),
            Err(error) => {
                // Discard the rollback error: the migration error is the
                // one that explains the failure.
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// The statements for a single migration version, without the version
    /// stamp or transaction handling.
    fn apply_migration_body(conn: &Connection, version: i32) -> Result<(), StorageError> {
        match version {
            1 => exec_sql(conn, MIGRATION_001),
            2 => {
                // The v1 schema declaration recorded `user_version = 1`
                // without this column, so it is added only when genuinely
                // absent (a fresh v1 table already carries it).
                if column_exists(conn, "device_identity", "key_label")? {
                    Ok(())
                } else {
                    exec_sql(conn, MIGRATION_002)
                }
            }
            3 => exec_sql(conn, MIGRATION_003),
            4 => exec_sql(conn, MIGRATION_004),
            5 => {
                // ADD COLUMN is not idempotent: guard it so an interrupted
                // step (column present, version still 4) resumes cleanly.
                if !column_exists(conn, "friends", "connection_state")? {
                    exec_sql(conn, MIGRATION_005_ADD_COLUMN)?;
                }
                // The backfill must run even when the column was already
                // present — that IS the interrupted state, where rows are
                // still on the column default.
                exec_sql(conn, MIGRATION_005_BACKFILL)
            }
            other => Err(StorageError::Sqlite(format!(
                "MP-STORE-003 unknown migration version {other}"
            ))),
        }
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
    ///
    /// Deterministic: the table is meant to hold exactly one row, but a failed
    /// rotation delete can leave two, and an unordered `LIMIT 1` then returns
    /// whichever row SQLite happens to visit first. That is how a stale identity
    /// could be silently resurrected after a rotation. Newest first, with
    /// `device_id` as a stable tie-break for two rows written in the same
    /// millisecond.
    pub fn get_identity(&self) -> Result<Option<StoredIdentity>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT device_id, display_name, public_key, platform, created_at_ms, key_label
                 FROM device_identity
                 ORDER BY created_at_ms DESC, device_id ASC
                 LIMIT 1",
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

    /// Upsert a Provider Shared capture diagnostic. The row is
    /// the empirical record for §38 — `shared_available` is true
    /// ONLY when a real diagnostic verified capture on this device.
    pub fn upsert_provider_diagnostic(
        &self,
        diagnostic: &StoredProviderDiagnostic,
    ) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "INSERT INTO providers (provider_id, display_name, shared_available, shared_reason, verified_at_ms, sample_seconds)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(provider_id) DO UPDATE SET
                display_name = excluded.display_name,
                shared_available = excluded.shared_available,
                shared_reason = excluded.shared_reason,
                verified_at_ms = excluded.verified_at_ms,
                sample_seconds = excluded.sample_seconds",
            rusqlite::params![
                diagnostic.provider_id,
                diagnostic.display_name,
                i64::from(diagnostic.shared_available),
                diagnostic.shared_reason,
                diagnostic.verified_at_ms,
                i64::from(diagnostic.sample_seconds),
            ],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// All persisted diagnostics keyed by provider ID.
    pub fn list_provider_diagnostics(&self) -> Result<Vec<StoredProviderDiagnostic>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, display_name, shared_available, shared_reason,
                        verified_at_ms, sample_seconds
                 FROM providers ORDER BY provider_id",
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(StoredProviderDiagnostic {
                    provider_id: row.get(0)?,
                    display_name: row.get(1)?,
                    shared_available: row.get::<_, i64>(2)? != 0,
                    shared_reason: row.get(3)?,
                    verified_at_ms: row.get(4)?,
                    sample_seconds: row.get::<_, i64>(5)?.clamp(0, u16::MAX as i64) as u16,
                })
            })
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let mut diagnostics = Vec::new();
        for row in rows {
            diagnostics.push(row.map_err(|e| StorageError::Sqlite(e.to_string()))?);
        }
        Ok(diagnostics)
    }

    // ── Friends (saved movie partners) ─────────────────────────────────────

    /// Upsert a friend. The peer key (full MagicDNS name) is the identity;
    /// re-adding the same peer refreshes its cached observations. The
    /// persisted connection_state follows the incoming record (invites
    /// write INVITED; a live tailnet observation writes TAILSCALE_JOINED).
    pub fn upsert_friend(&self, friend: &StoredFriend) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "INSERT INTO friends (peer_key, display_name, ip, added_at_ms, last_verified_at_ms, last_path, last_latency_ms, connection_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(peer_key) DO UPDATE SET
                display_name = excluded.display_name,
                ip = excluded.ip,
                last_verified_at_ms = excluded.last_verified_at_ms,
                last_path = excluded.last_path,
                last_latency_ms = excluded.last_latency_ms,
                connection_state = excluded.connection_state",
            rusqlite::params![
                friend.peer_key,
                friend.display_name,
                friend.ip,
                friend.added_at_ms,
                friend.last_verified_at_ms,
                friend.last_path,
                friend.last_latency_ms.map(|v| v as i64),
                friend.connection_state.as_str(),
            ],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Update only the cached verification result of a saved friend.
    /// A successful verification (verified_at_ms present) also promotes
    /// the persisted state to MOVIE_PARTY_VERIFIED; a failed one keeps
    /// TAILSCALE_JOINED (the peer joined, the tunnel just didn't answer).
    pub fn update_friend_verification(
        &self,
        peer_key: &str,
        ip: Option<&str>,
        verified_at_ms: Option<i64>,
        path: Option<&str>,
        latency_ms: Option<u32>,
    ) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "UPDATE friends
             SET ip = COALESCE(?2, ip),
                 last_verified_at_ms = ?3,
                 last_path = ?4,
                 last_latency_ms = ?5,
                 connection_state = CASE
                     WHEN ?3 IS NOT NULL THEN 'MOVIE_PARTY_VERIFIED'
                     WHEN COALESCE(?2, ip) IS NOT NULL AND connection_state = 'INVITED' THEN 'TAILSCALE_JOINED'
                     ELSE connection_state END
             WHERE peer_key = ?1",
            rusqlite::params![
                peer_key,
                ip,
                verified_at_ms,
                path,
                latency_ms.map(|v| v as i64),
            ],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Promote a friend's persisted state after a live tailnet
    /// observation (the friend's device joined the tailnet). Never
    /// demotes MOVIE_PARTY_VERIFIED and refreshes the cached IP.
    pub fn mark_friend_joined(&self, peer_key: &str, ip: &str) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "UPDATE friends
             SET ip = ?2,
                 connection_state = CASE
                     WHEN connection_state = 'MOVIE_PARTY_VERIFIED' THEN 'MOVIE_PARTY_VERIFIED'
                     ELSE 'TAILSCALE_JOINED' END
             WHERE peer_key = ?1",
            rusqlite::params![peer_key, ip],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// All saved friends, ordered by display name.
    pub fn list_friends(&self) -> Result<Vec<StoredFriend>, StorageError> {
        let conn = lock_mutex(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT peer_key, display_name, ip, added_at_ms,
                        last_verified_at_ms, last_path, last_latency_ms, connection_state
                 FROM friends ORDER BY display_name",
            )
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(StoredFriend {
                    peer_key: row.get(0)?,
                    display_name: row.get(1)?,
                    ip: row.get(2)?,
                    added_at_ms: row.get(3)?,
                    last_verified_at_ms: row.get(4)?,
                    last_path: row.get(5)?,
                    last_latency_ms: row
                        .get::<_, Option<i64>>(6)?
                        .map(|v| v.clamp(0, u32::MAX as i64) as u32),
                    connection_state: row
                        .get::<_, String>(7)
                        .map(|value| FriendConnectionState::from_persisted(&value))?,
                })
            })
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        let mut friends = Vec::new();
        for row in rows {
            friends.push(row.map_err(|e| StorageError::Sqlite(e.to_string()))?);
        }
        Ok(friends)
    }

    /// Remove a saved friend by peer key.
    pub fn delete_friend(&self, peer_key: &str) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute("DELETE FROM friends WHERE peer_key = ?1", [peer_key])
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
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
    /// (§55): update a schedule's wall-clock start time. Wall-clock
    /// is the legal domain for scheduled movie time (§16).
    pub fn update_schedule_start(
        &self,
        schedule_id: &str,
        scheduled_start_utc_ms: i64,
    ) -> Result<(), StorageError> {
        let conn = lock_mutex(&self.conn)?;
        conn.execute(
            "UPDATE schedules SET scheduled_start_utc_ms = ?1 WHERE schedule_id = ?2",
            rusqlite::params![scheduled_start_utc_ms, schedule_id],
        )
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        Ok(())
    }

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

    // NOTE: the preload-deadline formula
    // previously lived here as a DUPLICATE that divided bytes by a
    // bits-per-second goodput without ×8 (reading the transfer as 8×
    // faster than reality and scheduling preload 8× too late). There is
    // exactly ONE canonical implementation now:
    // `scheduling::calculate_preload_start` (bits-correct). Callers and
    // tests route through it; this storage layer never recomputes the
    // formula.

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

    /// Retention action: Remove deletes only the Movie Party cache entry and
    /// any associated cached data on disk.  Never deletes the host's
    /// original media file.
    pub fn retention_remove(&self, media_id: &str) -> Result<(), StorageError> {
        // Delete the cache entry from the database
        self.delete_cache_entry(media_id)?;
        Ok(())
    }
}

/// Run one SQL batch, mapping rusqlite failures into [`StorageError`].
///
/// The `sql` is always an internal `MIGRATION_*` literal — never user input.
fn exec_sql(conn: &Connection, sql: &str) -> Result<(), StorageError> {
    conn.execute_batch(sql)
        .map_err(|e| StorageError::Sqlite(e.to_string()))
}

/// True when `table` has a column named `column`.
///
/// Lets the runner make `ALTER TABLE ... ADD COLUMN` steps idempotent: a
/// database interrupted between the `ALTER` and the `user_version` stamp
/// still reports the old version, so the step is re-entered — and without
/// this check it would die on `duplicate column name` (F51).
///
/// `table`/`column` are always internal literals, never user input, so the
/// interpolated `PRAGMA table_info` cannot be influenced from outside.
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, StorageError> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;

    for name in columns {
        if name.map_err(|e| StorageError::Sqlite(e.to_string()))? == column {
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

/// v3: persistent per-provider Provider Shared capture
/// diagnostics — the empirical classification record (§38:
/// provider support is empirical, never guessed). `shared_available`
/// flips only when a real diagnostic on this device verified capture.
const MIGRATION_003: &str = "
CREATE TABLE IF NOT EXISTS providers (
    provider_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    shared_available INTEGER NOT NULL DEFAULT 0,
    shared_reason TEXT NOT NULL DEFAULT '',
    verified_at_ms INTEGER,
    sample_seconds INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_providers_recent ON providers(provider_id, verified_at_ms);
";

/// v4: saved movie-partner friends — the chosen Tailscale tailnet peers.
/// `peer_key` (full MagicDNS name) is the stable identity; IP and probe
/// results are cached observations that refresh on each verification.
const MIGRATION_004: &str = "
CREATE TABLE IF NOT EXISTS friends (
    peer_key TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    ip TEXT,
    added_at_ms INTEGER NOT NULL,
    last_verified_at_ms INTEGER,
    last_path TEXT,
    last_latency_ms INTEGER
);
";

/// v5: explicit friend connection states for the Tailscale friend
/// architecture. The Movie Party friend identity (peer_key + editable
/// display name) stays separate from the Tailnet node identity; this
/// column records WHERE the friend is in the external-user invitation
/// flow:
///   INVITED            — accepted from a movieparty://friend link; the
///                        friend's device has not been observed in this
///                        device's tailnet status yet.
///   TAILSCALE_JOINED   — the expected peer IS in the live tailnet
///                        status with a usable address (the friend
///                        authenticated with THEIR OWN Tailscale identity
///                        and joined/authorized on the tailnet).
///   MOVIE_PARTY_VERIFIED — a real `tailscale ping` answered through
///                        the tunnel (Movie Party-level verification).
/// ONLINE/OFFLINE is derived at read time from the live status and is
/// never persisted — it is an observation, not an identity fact.
/// `pending` is a transient label meaning "INVITED and not yet joined".
///
/// The step is split in two so the runner can make the `ADD COLUMN` half
/// conditional: `ALTER TABLE ... ADD COLUMN` is NOT idempotent in SQLite,
/// and re-running it is exactly what bricked databases (F51 —
/// `duplicate column name: connection_state`).
const MIGRATION_005_ADD_COLUMN: &str = "
ALTER TABLE friends ADD COLUMN connection_state TEXT NOT NULL DEFAULT 'INVITED';
";

/// v5 (part 2): map v4 rows onto the new states.
///
/// Idempotent by construction, which matters because this runs again on a
/// database that was physically altered but still reports `user_version`
/// 4 (the interrupted-F51 state). The first statement only lifts rows
/// still sitting on the column default, so a resumed run can never demote
/// a row that a later code path already promoted; the second only ever
/// promotes.
const MIGRATION_005_BACKFILL: &str = "
UPDATE friends SET connection_state = 'TAILSCALE_JOINED'
    WHERE connection_state = 'INVITED' AND ip IS NOT NULL;
UPDATE friends SET connection_state = 'MOVIE_PARTY_VERIFIED'
    WHERE last_verified_at_ms IS NOT NULL;
";

// ── Tests ───────────────────────────────────────────────────────────────────

/// Read the `user_version` schema header, propagating a read failure.
///
/// Deliberately NOT `.unwrap_or(0)`. `0` means "no schema yet" and is below
/// every supported version, so coercing an unreadable header to 0 would let a
/// database this build cannot understand pass the `SchemaTooNew` guard in
/// [`MoviePartyDb::run_migrations`] and have migrations applied to it. This is
/// a separate function so that contract is directly testable.
fn read_schema_version(conn: &Connection) -> Result<i32, StorageError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
        .map_err(|e| StorageError::Sqlite(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_database_and_runs_migrations() {
        let db = MoviePartyDb::open_in_memory().expect("open");
        let version = db.schema_version().expect("version");
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn friends_crud_round_trip() {
        let db = MoviePartyDb::open_in_memory().expect("open");
        assert!(db.list_friends().expect("empty list").is_empty());

        let friend = StoredFriend {
            peer_key: "rahul-mac.tailc930b7.ts.net.".to_string(),
            display_name: "rahul-mac".to_string(),
            ip: Some("100.64.0.42".to_string()),
            added_at_ms: 1_000,
            last_verified_at_ms: None,
            last_path: None,
            last_latency_ms: None,
            connection_state: FriendConnectionState::TailscaleJoined,
        };
        db.upsert_friend(&friend).expect("insert");

        // Re-add with fresh verification data → upsert refreshes, no dupes.
        db.update_friend_verification(
            "rahul-mac.tailc930b7.ts.net.",
            Some("100.64.0.43"),
            Some(2_000),
            Some("direct"),
            Some(23),
        )
        .expect("verify update");

        let friends = db.list_friends().expect("list");
        assert_eq!(friends.len(), 1);
        assert_eq!(friends[0].peer_key, "rahul-mac.tailc930b7.ts.net.");
        assert_eq!(friends[0].ip.as_deref(), Some("100.64.0.43"));
        assert_eq!(friends[0].last_verified_at_ms, Some(2_000));
        assert_eq!(friends[0].last_path.as_deref(), Some("direct"));
        assert_eq!(friends[0].last_latency_ms, Some(23));
        assert_eq!(
            friends[0].connection_state,
            FriendConnectionState::MoviePartyVerified,
            "a successful ping promotes the persisted state"
        );

        db.delete_friend("rahul-mac.tailc930b7.ts.net.")
            .expect("delete");
        assert!(db.list_friends().expect("list after delete").is_empty());
        // Deleting an unknown peer key is a no-op, not an error.
        db.delete_friend("never-saved").expect("idempotent delete");
    }

    #[test]
    fn friends_upsert_replaces_cached_observations() {
        let db = MoviePartyDb::open_in_memory().expect("open");
        db.upsert_friend(&StoredFriend {
            peer_key: "pc.tailnet.ts.net.".to_string(),
            display_name: "pc".to_string(),
            ip: Some("100.64.0.9".to_string()),
            added_at_ms: 1,
            last_verified_at_ms: Some(50),
            last_path: Some("direct".to_string()),
            last_latency_ms: Some(10),
            connection_state: FriendConnectionState::MoviePartyVerified,
        })
        .expect("first add");
        // Second add of the same peer with no verification yet clears the
        // stale verification but keeps exactly one row.
        db.upsert_friend(&StoredFriend {
            peer_key: "pc.tailnet.ts.net.".to_string(),
            display_name: "pc-renamed".to_string(),
            ip: None,
            added_at_ms: 2,
            last_verified_at_ms: None,
            last_path: None,
            last_latency_ms: None,
            connection_state: FriendConnectionState::Invited,
        })
        .expect("second add");
        let friends = db.list_friends().expect("list");
        assert_eq!(friends.len(), 1);
        assert_eq!(friends[0].display_name, "pc-renamed");
        assert_eq!(
            friends[0].added_at_ms, 1,
            "original added_at is kept by ON CONFLICT"
        );
        assert!(friends[0].ip.is_none());
        assert!(friends[0].last_verified_at_ms.is_none());
    }

    #[test]
    fn migration_v4_upgrades_v3_database() {
        // Build a v3 database (no friends table), then reopen through
        // MoviePartyDb::open so the migration path runs against a real file.
        let dir = std::env::temp_dir().join(format!("mp-friends-mig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("friends.db");
        {
            let conn = Connection::open(&db_path).expect("open raw");
            conn.execute_batch(MIGRATION_001).expect("v1 schema");
            conn.execute_batch(MIGRATION_003).expect("v3 schema");
            conn.execute_batch("PRAGMA user_version=3;")
                .expect("set v3");
        }
        let db = MoviePartyDb::open(&db_path).expect("migrate to v5");
        assert_eq!(
            db.schema_version().expect("version"),
            CURRENT_SCHEMA_VERSION
        );
        // The friends table is usable immediately after migration.
        db.upsert_friend(&StoredFriend {
            peer_key: "k".to_string(),
            display_name: "k".to_string(),
            ip: None,
            added_at_ms: 1,
            last_verified_at_ms: None,
            last_path: None,
            last_latency_ms: None,
            connection_state: FriendConnectionState::Invited,
        })
        .expect("insert post-migration");
        assert_eq!(db.list_friends().expect("list").len(), 1);
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn migration_v5_backfills_existing_friends_states() {
        // Build a v4 friends database with one verified friend and one
        // joined-but-unverified friend, then migrate to v5 and confirm
        // the connection_state backfill preserves their progress.
        let dir = std::env::temp_dir().join(format!("mp-friends-mig5-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("friends.db");
        {
            let conn = Connection::open(&db_path).expect("open raw");
            conn.execute_batch(MIGRATION_001).expect("v1 schema");
            conn.execute_batch(MIGRATION_003).expect("v3 schema");
            conn.execute_batch(MIGRATION_004).expect("v4 friends");
            conn.execute_batch(
                "INSERT INTO friends (peer_key, display_name, ip, added_at_ms, last_verified_at_ms, last_path, last_latency_ms)
                 VALUES
                    ('verified.tailnet.ts.net.', 'V', '100.64.0.1', 1, 500, 'direct', 12),
                    ('joined.tailnet.ts.net.',   'J', '100.64.0.2', 2, NULL, NULL, NULL),
                    ('bare.tailnet.ts.net.',     'B', NULL, 3, NULL, NULL, NULL);",
            )
            .expect("seed v4 friends");
            conn.execute_batch("PRAGMA user_version=4;")
                .expect("set v4");
        }
        let db = MoviePartyDb::open(&db_path).expect("migrate to v5");
        let friends = db.list_friends().expect("list");
        let by_key = |k: &str| {
            friends
                .iter()
                .find(|f| f.peer_key == k)
                .unwrap_or_else(|| panic!("missing {k}"))
                .connection_state
        };
        assert_eq!(
            by_key("verified.tailnet.ts.net."),
            FriendConnectionState::MoviePartyVerified
        );
        assert_eq!(
            by_key("joined.tailnet.ts.net."),
            FriendConnectionState::TailscaleJoined
        );
        assert_eq!(
            by_key("bare.tailnet.ts.net."),
            FriendConnectionState::Invited
        );
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&dir);
    }

    // ── F51 regression suite: migration interruption safety ──────────────
    //
    // The previous runner stamped `user_version` only after all batches had
    // run, and MIGRATION_005's `ALTER TABLE ADD COLUMN` is not idempotent.
    // An interrupt between the two left a database that failed on EVERY
    // subsequent launch with `duplicate column name: connection_state`.

    /// Scratch v4 friends database with three rows covering every backfill
    /// branch: verified, joined-but-unverified, and bare/unobserved.
    fn scratch_v4_friends_db(tag: &str) -> (PathBuf, PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("mp-f51-{tag}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("movie_party.db");
        {
            let conn = Connection::open(&db_path).expect("open raw");
            conn.execute_batch(MIGRATION_001).expect("v1 schema");
            conn.execute_batch(MIGRATION_003).expect("v3 providers");
            conn.execute_batch(MIGRATION_004).expect("v4 friends");
            conn.execute_batch(
                "INSERT INTO friends (peer_key, display_name, ip, added_at_ms, last_verified_at_ms, last_path, last_latency_ms)
                 VALUES
                    ('verified.tailnet.ts.net.', 'V', '100.64.0.1', 1, 500, 'direct', 12),
                    ('joined.tailnet.ts.net.',   'J', '100.64.0.2', 2, NULL, NULL, NULL),
                    ('bare.tailnet.ts.net.',     'B', NULL, 3, NULL, NULL, NULL);",
            )
            .expect("seed v4 friends");
            conn.execute_batch("PRAGMA user_version=4;")
                .expect("stamp v4");
        }
        (dir, db_path)
    }

    fn friend_state(friends: &[StoredFriend], peer_key: &str) -> FriendConnectionState {
        friends
            .iter()
            .find(|friend| friend.peer_key == peer_key)
            .unwrap_or_else(|| panic!("missing {peer_key}"))
            .connection_state
    }

    fn raw_user_version(db_path: &Path) -> i32 {
        let conn = Connection::open(db_path).expect("open raw");
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
            .expect("user_version")
    }

    /// MP-11: a `PRAGMA user_version` read that FAILS must surface as an error,
    /// never be coerced to 0.
    ///
    /// 0 is below every supported version, so a swallowed read failure would
    /// let an unreadable database sail past the `SchemaTooNew` guard and have
    /// migrations run against it. Asserting on `read_schema_version` directly
    /// is what makes this test discriminating: a reintroduced `.unwrap_or(0)`
    /// returns `Ok(0)` here and fails the assertion.
    #[test]
    fn schema_version_read_failure_is_propagated_not_defaulted_to_zero() {
        let dir = std::env::temp_dir().join(format!("mp-notadb-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("garbage.db");
        // Not a SQLite file: the header read fails with SQLITE_NOTADB.
        std::fs::write(&db_path, b"definitely not a sqlite database").expect("write");

        // Opening is lazy, so this succeeds; the read is what must fail.
        let conn = Connection::open(&db_path).expect("lazy open");

        let result = read_schema_version(&conn);

        assert!(
            result.is_err(),
            "an unreadable schema header must be an error, not version 0 (got {result:?})"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MP-13: the identity lookup must be deterministic and must not let a
    /// stale row win.
    ///
    /// The table is meant to hold exactly one row, but a failed rotation delete
    /// can leave two. An unordered `LIMIT 1` then returns whichever row SQLite
    /// visits first, which is how a rotated-away identity could be silently
    /// resurrected on the next launch. Newest must win, repeatably.
    #[test]
    fn identity_lookup_is_deterministic_and_prefers_the_newest_row() {
        let db = MoviePartyDb::open_in_memory().expect("open");

        let stale = StoredIdentity {
            device_id: "device-stale".to_string(),
            display_name: "Old Name".to_string(),
            public_key: "old-key".to_string(),
            platform: "macos".to_string(),
            created_at_ms: 1_000,
            key_label: "label-stale".to_string(),
        };
        let fresh = StoredIdentity {
            device_id: "device-fresh".to_string(),
            display_name: "New Name".to_string(),
            public_key: "new-key".to_string(),
            platform: "macos".to_string(),
            created_at_ms: 2_000,
            key_label: "label-fresh".to_string(),
        };

        // Reproduces the two-row state a failed rotation delete would leave.
        db.upsert_identity(&stale).expect("stale row");
        db.upsert_identity(&fresh).expect("fresh row");

        let first = db.get_identity().expect("lookup").expect("some row");
        assert_eq!(
            first.device_id, "device-fresh",
            "the newest identity must win, not an arbitrary row"
        );
        // Repeatable: no dependence on physical row order.
        for _ in 0..5 {
            let again = db.get_identity().expect("lookup").expect("some row");
            assert_eq!(again.device_id, first.device_id);
        }

        // With the stale row gone the answer is unchanged, and an empty table
        // still reports "no identity".
        db.delete_identity().expect("delete");
        assert!(db.get_identity().expect("lookup").is_none());
    }

    /// A. Normal v4 → v5 upgrade.
    #[test]
    fn f51_a_normal_v4_to_v5_upgrade() {
        let (dir, db_path) = scratch_v4_friends_db("a");
        let db = MoviePartyDb::open(&db_path).expect("v4 -> v5 upgrade");

        assert_eq!(
            db.schema_version().expect("version"),
            CURRENT_SCHEMA_VERSION
        );
        let friends = db.list_friends().expect("list");
        assert_eq!(friends.len(), 3);
        assert_eq!(
            friend_state(&friends, "verified.tailnet.ts.net."),
            FriendConnectionState::MoviePartyVerified
        );
        assert_eq!(
            friend_state(&friends, "joined.tailnet.ts.net."),
            FriendConnectionState::TailscaleJoined
        );
        assert_eq!(
            friend_state(&friends, "bare.tailnet.ts.net."),
            FriendConnectionState::Invited
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// B. Schema already fully upgraded, but `user_version` still at 4.
    #[test]
    fn f51_b_already_upgraded_schema_with_stale_user_version() {
        let (dir, db_path) = scratch_v4_friends_db("b");
        {
            let conn = Connection::open(&db_path).expect("open raw");
            // ALTER *and* backfill completed; the process died before the
            // version stamp was written.
            conn.execute_batch(MIGRATION_005_ADD_COLUMN).expect("alter");
            conn.execute_batch(MIGRATION_005_BACKFILL)
                .expect("backfill");
        }
        assert_eq!(raw_user_version(&db_path), 4, "version deliberately stale");

        let db = MoviePartyDb::open(&db_path).expect("stale-version upgrade must succeed");
        assert_eq!(
            db.schema_version().expect("version"),
            CURRENT_SCHEMA_VERSION
        );

        let friends = db.list_friends().expect("list");
        assert_eq!(
            friend_state(&friends, "verified.tailnet.ts.net."),
            FriendConnectionState::MoviePartyVerified,
            "a re-run must not demote an already-verified friend"
        );
        assert_eq!(
            friend_state(&friends, "joined.tailnet.ts.net."),
            FriendConnectionState::TailscaleJoined
        );
        assert_eq!(
            friend_state(&friends, "bare.tailnet.ts.net."),
            FriendConnectionState::Invited
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// C. Repeated migration execution.
    #[test]
    fn f51_c_repeated_migration_execution() {
        let (dir, db_path) = scratch_v4_friends_db("c");
        for round in 0..3 {
            let db = MoviePartyDb::open(&db_path)
                .unwrap_or_else(|e| panic!("round {round} failed: {e}"));
            assert_eq!(
                db.schema_version().expect("version"),
                CURRENT_SCHEMA_VERSION
            );
        }
        let db = MoviePartyDb::open(&db_path).expect("final open");
        assert_eq!(
            db.list_friends().expect("list").len(),
            3,
            "repeated opens must not duplicate rows"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// D. Interrupted migration — the exact reproduced F51 scenario.
    ///
    /// Before the fix this failed forever with
    /// `duplicate column name: connection_state`.
    #[test]
    fn f51_d_interrupted_migration_does_not_brick() {
        let (dir, db_path) = scratch_v4_friends_db("d");
        {
            let conn = Connection::open(&db_path).expect("open raw");
            // The ALTER committed, then the process died: the column is
            // present, the rows are still on the column default, and the
            // version still says 4.
            conn.execute_batch(MIGRATION_005_ADD_COLUMN).expect("alter");
        }
        assert_eq!(raw_user_version(&db_path), 4);

        let db = MoviePartyDb::open(&db_path)
            .expect("F51 regression: the interrupted upgrade must recover, not brick");

        assert_eq!(
            db.schema_version().expect("version"),
            CURRENT_SCHEMA_VERSION
        );
        let friends = db.list_friends().expect("list");
        assert_eq!(friends.len(), 3);
        assert_eq!(
            friend_state(&friends, "verified.tailnet.ts.net."),
            FriendConnectionState::MoviePartyVerified,
            "the interrupted backfill is completed on resume"
        );
        assert_eq!(
            friend_state(&friends, "joined.tailnet.ts.net."),
            FriendConnectionState::TailscaleJoined,
            "the interrupted backfill is completed on resume"
        );
        assert_eq!(
            friend_state(&friends, "bare.tailnet.ts.net."),
            FriendConnectionState::Invited
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E. Opening an existing v5 database is a no-op that preserves data.
    #[test]
    fn f51_e_opening_an_existing_v5_database() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("mp-f51-e-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("movie_party.db");

        {
            let db = MoviePartyDb::open(&db_path).expect("create v5");
            assert_eq!(
                db.schema_version().expect("version"),
                CURRENT_SCHEMA_VERSION
            );
            db.upsert_friend(&StoredFriend {
                peer_key: "keep.tailnet.ts.net.".to_string(),
                display_name: "Keep".to_string(),
                ip: Some("100.64.0.7".to_string()),
                added_at_ms: 42,
                last_verified_at_ms: Some(99),
                last_path: Some("direct".to_string()),
                last_latency_ms: Some(7),
                connection_state: FriendConnectionState::MoviePartyVerified,
            })
            .expect("insert");
        }

        let db = MoviePartyDb::open(&db_path).expect("reopen existing v5");
        assert_eq!(
            db.schema_version().expect("version"),
            CURRENT_SCHEMA_VERSION
        );
        let friends = db.list_friends().expect("list");
        assert_eq!(friends.len(), 1, "reopening must not disturb existing rows");
        assert_eq!(friends[0].display_name, "Keep");
        assert_eq!(
            friends[0].connection_state,
            FriendConnectionState::MoviePartyVerified
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F. A database from a NEWER build is refused, not downgraded.
    #[test]
    fn f51_f_newer_schema_version_is_refused_not_downgraded() {
        let (dir, db_path) = scratch_v4_friends_db("f");
        {
            let conn = Connection::open(&db_path).expect("open raw");
            conn.execute_batch("PRAGMA user_version=99;")
                .expect("simulate a newer build");
        }

        let error = match MoviePartyDb::open(&db_path) {
            Ok(_) => panic!("a newer schema must be refused, never silently downgraded"),
            Err(error) => error,
        };
        assert!(
            matches!(
                error,
                StorageError::SchemaTooNew {
                    found: 99,
                    supported: CURRENT_SCHEMA_VERSION
                }
            ),
            "unexpected error: {error}"
        );

        assert_eq!(
            raw_user_version(&db_path),
            99,
            "the newer build's version must be left untouched"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn friend_state_transitions_never_demote_verification() {
        let db = MoviePartyDb::open_in_memory().expect("open");
        db.upsert_friend(&StoredFriend {
            peer_key: "v.tailnet.ts.net.".to_string(),
            display_name: "v".to_string(),
            ip: None,
            added_at_ms: 1,
            last_verified_at_ms: None,
            last_path: None,
            last_latency_ms: None,
            connection_state: FriendConnectionState::Invited,
        })
        .expect("insert invited");

        // INVITED → TAILSCALE_JOINED via a live observation.
        db.mark_friend_joined("v.tailnet.ts.net.", "100.64.0.5")
            .expect("mark joined");
        let friend = &db.list_friends().expect("list")[0];
        assert_eq!(
            friend.connection_state,
            FriendConnectionState::TailscaleJoined
        );
        assert_eq!(friend.ip.as_deref(), Some("100.64.0.5"));

        // JOINED → VERIFIED via a successful ping.
        db.update_friend_verification(
            "v.tailnet.ts.net.",
            Some("100.64.0.5"),
            Some(9_000),
            Some("direct"),
            Some(20),
        )
        .expect("verify");
        let friend = &db.list_friends().expect("list")[0];
        assert_eq!(
            friend.connection_state,
            FriendConnectionState::MoviePartyVerified
        );

        // A later joined-observation or failed ping must NOT demote.
        db.mark_friend_joined("v.tailnet.ts.net.", "100.64.0.6")
            .expect("observe again");
        db.update_friend_verification("v.tailnet.ts.net.", Some("100.64.0.6"), None, None, None)
            .expect("failed probe");
        let friend = &db.list_friends().expect("list")[0];
        assert_eq!(
            friend.connection_state,
            FriendConnectionState::MoviePartyVerified,
            "verification is sticky across later observations"
        );
        assert_eq!(friend.ip.as_deref(), Some("100.64.0.6"));
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
        let db = MoviePartyDb {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        };

        db.run_migrations().expect("upgrade");

        let conn = lock_mutex(&db.conn).expect("lock");
        let has_key_label = column_exists(&conn, "device_identity", "key_label").expect("columns");
        assert!(has_key_label);
        drop(conn);
        assert_eq!(
            db.schema_version().expect("version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn persists_and_retrieves_identity() {
        let db = MoviePartyDb::open_in_memory().expect("open");
        let identity = StoredIdentity {
            device_id: "test-device-id".to_string(),
            display_name: "Test Host".to_string(),
            public_key: "base64key".to_string(),
            platform: "macos".to_string(),
            created_at_ms: 1_000_000,
            key_label: "movie-party-device-signing-key".to_string(),
        };

        db.upsert_identity(&identity).expect("upsert");
        let retrieved = db.get_identity().expect("get").expect("some");
        assert_eq!(retrieved, identity);
    }

    #[test]
    fn identity_key_label_round_trips() {
        let db = MoviePartyDb::open_in_memory().expect("open");
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
        let db = MoviePartyDb::open_in_memory().expect("open");
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
        let db = MoviePartyDb::open_in_memory().expect("open");
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
        let db = MoviePartyDb::open_in_memory().expect("open");
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
        let db = MoviePartyDb::open_in_memory().expect("open");
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
        let db = MoviePartyDb::open_in_memory().expect("open");
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
        let db = MoviePartyDb::open_in_memory().expect("open");
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
            let db = MoviePartyDb::open(&db_path).expect("open1");
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
            let db = MoviePartyDb::open(&db_path).expect("open2");
            let identity = db.get_identity().expect("get").expect("exists");
            assert_eq!(identity.device_id, "persistent-device");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_identity_row_surfaces_read_error() {
        let db = MoviePartyDb::open_in_memory().expect("open");
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

    /// Regression: the storage-layer duplicate divided BYTES by
    /// a BITS-per-second goodput (8× optimistic). The canonical
    /// scheduling::calculate_preload_start is bits-correct: 1 GB at
    /// 10 Mbps takes 800 s, ×1.4 = 1120 s ≈ 1_120_000 ms before the
    /// 15-min preparation margin.
    #[test]
    fn preload_deadline_uses_the_canonical_bits_correct_formula() {
        let start = crate::scheduling::calculate_preload_start(crate::scheduling::PreloadInputs {
            remaining_bytes: 1_000_000_000,
            conservative_goodput_bps: 10_000_000,
            scheduled_start_utc_ms: 1_000_000_000,
        })
        .expect("valid inputs");
        // 1e9 bytes × 8 / 1e7 bps = 800 s; ×1.4 = 1120 s = 1_120_000 ms;
        // − 15 min margin (900_000 ms).
        assert_eq!(start, 1_000_000_000 - 1_120_000 - 900_000);
    }

    #[test]
    fn preload_deadline_zero_goodput_is_an_error_not_a_guess() {
        // The canonical function refuses to guess on unknown throughput;
        // the caller (adaptive_preload_deadline) supplies the fallback.
        let result = crate::scheduling::calculate_preload_start(crate::scheduling::PreloadInputs {
            remaining_bytes: 1_000,
            conservative_goodput_bps: 0,
            scheduled_start_utc_ms: 1_000,
        });
        assert_eq!(
            result,
            Err(crate::scheduling::SchedulingError::MissingGoodput)
        );
    }

    #[test]
    fn overdue_schedules_returns_planned_past_preload() {
        let db = MoviePartyDb::open_in_memory().expect("open");
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
        let db = MoviePartyDb::open_in_memory().expect("open");

        assert_eq!(db.next_preload_deadline(100).expect("deadline"), None);
    }

    #[test]
    fn retention_remove_deletes_cache_entry() {
        let db = MoviePartyDb::open_in_memory().expect("open");
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
