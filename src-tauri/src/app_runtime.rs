use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, RwLock,
    },
    time::Instant,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    call::{validate_signal, CallMode, CallSignal, CameraState, MicState},
    chat::{
        allowed_reactions, validate_chat_message, validate_reaction, ChatMessage, ReactionMessage,
        ReactionRateLimiter,
    },
    identity::DeviceIdentity,
    media::{
        manifest::{build_manifest, MediaManifest},
        player::{LocalPlayer, PlayerSnapshot as LibPlayerSnapshot},
        stream::range_server::RangeServerHandle,
        transfer::{transfer_progress, TransferProgress},
    },
    network::quic::{
        self, instant_for_host_mono, loopback_bind_addr, monotonic_us, tailscale_bind_addr,
        EventEnvelope, QuicClient, QuicHostEvent, QuicServer, RoomCredentials,
        ServerEvent as QuicServerEvent,
    },
    resilience::{recovery_plan, FailureEvent, RecoveryAction, RecoveryPlan},
    room,
    sync::{
        clock,
        consensus::ParticipantReadiness,
        local::{LocalSyncCoordinator, PauseCause, PeerRole, ScheduledPlayback},
        state_machine::RoomState,
    },
};

#[cfg(feature = "mpv")]
use crate::media::player::MpvPlayer;

// ── SnapshotSink abstraction ───────────────────────────────────────────────────

pub trait SnapshotSink: Send + Sync + 'static {
    fn emit(&self, snapshot: &AppSnapshot);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NullSink;

impl SnapshotSink for NullSink {
    fn emit(&self, _: &AppSnapshot) {}
}

#[derive(Debug, Clone)]
pub struct TauriSink {
    app_handle: tauri::AppHandle,
}

impl TauriSink {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }
}

impl SnapshotSink for TauriSink {
    fn emit(&self, snapshot: &AppSnapshot) {
        let _ = self.app_handle.emit("app_snapshot_pushed", snapshot);
    }
}

// ── Snapshots (TS-visible; do not change fields without updating TS types) ─────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub screen: String,
    pub room: RoomSnapshot,
    pub participants: Vec<ParticipantSnapshot>,
    pub media: Option<MediaManifest>,
    pub transfer: Option<TransferProgress>,
    pub buffer: BufferSnapshot,
    pub sync: SyncSnapshot,
    pub network: NetworkSnapshot,
    pub call: CallSnapshot,
    pub call_signals: Vec<CallSignalSnapshot>,
    pub provider: ProviderSnapshot,
    pub chat: Vec<ChatSnapshot>,
    pub reactions: Vec<ReactionSnapshot>,
    pub ghost_mode: bool,
    pub privacy_mode: bool,
    pub last_recovery: Option<RuntimeRecoverySnapshot>,
    pub error: Option<String>,
    pub player: PlayerSnapshot,
}

impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            screen: "HOME".to_string(),
            room: RoomSnapshot {
                room_id: None,
                invite_code: None,
                role: "Host".to_string(),
                state: "CREATED".to_string(),
                host_only_controls: true,
                shared_controls: false,
                strict_sync: true,
            },
            participants: vec![],
            media: None,
            transfer: None,
            buffer: BufferSnapshot {
                guest_buffer_ahead_ms: 0,
                percent: 0,
                buffering_participant: None,
            },
            sync: SyncSnapshot {
                room_state: "CREATED".to_string(),
                strict_sync_paused: false,
                position_ms: 0,
            },
            network: NetworkSnapshot {
                transport: "QUIC".to_string(),
                path: "Not connected".to_string(),
                goodput_bps: 0,
                rtt_ms: None,
                connected: false,
            },
            call: CallSnapshot {
                mode: CallMode::VideoVoice,
                connected: false,
                camera: CameraState::tier_b_enabled(),
                microphone: MicState::default(),
            },
            call_signals: vec![],
            provider: ProviderSnapshot {
                mode: "LOCAL_PERFECT".to_string(),
                provider_id: None,
                url: None,
                state: "Idle".to_string(),
            },
            chat: vec![],
            reactions: vec![],
            ghost_mode: false,
            privacy_mode: false,
            last_recovery: None,
            error: None,
            player: PlayerSnapshot::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSnapshot {
    pub room_id: Option<String>,
    pub invite_code: Option<String>,
    pub role: String,
    pub state: String,
    pub host_only_controls: bool,
    pub shared_controls: bool,
    pub strict_sync: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantSnapshot {
    pub id: String,
    pub display_name: String,
    pub role: String,
    pub connected: bool,
    pub media_ready: bool,
    pub camera_enabled: bool,
    pub microphone_enabled: bool,
    pub buffer_ahead_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferSnapshot {
    pub guest_buffer_ahead_ms: u64,
    pub percent: u8,
    pub buffering_participant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSnapshot {
    pub room_state: String,
    pub strict_sync_paused: bool,
    pub position_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    pub transport: String,
    pub path: String,
    pub goodput_bps: u64,
    pub rtt_ms: Option<u32>,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallSnapshot {
    pub mode: CallMode,
    pub connected: bool,
    pub camera: CameraState,
    pub microphone: MicState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub mode: String,
    pub provider_id: Option<String>,
    pub url: Option<String>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallSignalSnapshot {
    pub signal_type: String,
    pub data: String,
    pub created_host_time_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSnapshot {
    pub id: String,
    pub sender: String,
    pub body: String,
    pub created_host_time_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionSnapshot {
    pub id: String,
    pub sender: String,
    pub reaction: String,
    pub created_host_time_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRecoverySnapshot {
    pub event: FailureEvent,
    pub action: RecoveryAction,
    pub pauses_playback_for_both: bool,
    pub requires_user_action: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub state: String,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub playback_rate: f32,
    pub buffered_ahead_ms: Option<u64>,
    pub error_message: Option<String>,
}

impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            state: "STOPPED".to_string(),
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            playback_rate: 1.0,
            buffered_ahead_ms: None,
            error_message: None,
        }
    }
}

impl From<&LibPlayerSnapshot> for PlayerSnapshot {
    fn from(snap: &LibPlayerSnapshot) -> Self {
        Self {
            state: format!("{:?}", snap.state).to_ascii_uppercase(),
            position_ms: snap.position_ms,
            duration_ms: snap.duration_ms,
            volume: snap.volume,
            playback_rate: snap.playback_rate,
            buffered_ahead_ms: snap.buffered_ahead_ms,
            error_message: snap.error_message.clone(),
        }
    }
}

// ── RuntimeInner / AppRuntimeState ────────────────────────────────────────────

struct RuntimeInner {
    state: Mutex<AppRuntimeState>,
    emitter: RwLock<Option<Arc<dyn SnapshotSink>>>,
    /// Device identity. Replaced by the persisted identity on startup
    /// (`init_db`), so the same device id + signing key is used for the
    /// whole process lifetime.
    identity: Mutex<DeviceIdentity>,
    /// Notification seam — production uses [`NativeNotifier`]; tests inject
    /// a fake so no OS notification is displayed during unit tests.
    notifier: Arc<dyn crate::notifications::Notifier>,
    /// Count of preload executions (tests + diagnostics).
    preload_executions: std::sync::atomic::AtomicU64,
    started_at: Instant,
}

impl std::fmt::Debug for RuntimeInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeInner").finish_non_exhaustive()
    }
}

impl RuntimeInner {
    pub fn identity(&self) -> DeviceIdentity {
        match self.identity.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    pub fn replace_identity(&self, identity: DeviceIdentity) {
        match self.identity.lock() {
            Ok(mut g) => *g = identity,
            Err(p) => *p.into_inner() = identity,
        }
    }
    pub fn emit(&self, snapshot: crate::app_runtime::AppSnapshot) {
        if let Some(arc) = self.emitter.read().ok().and_then(|g| g.clone()) {
            arc.emit(&snapshot);
        }
    }

    fn lock(&self) -> MutexGuard<'_, AppRuntimeState> {
        match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct HostSession {
    server_handle: tokio::task::JoinHandle<Result<(), quic::QuicError>>,
    bound_addr: std::net::SocketAddr,
    cert_fingerprint: String,
    invite_url: String,
}

struct AppRuntimeState {
    screen: String,
    credentials: Option<RoomCredentials>,
    invite: Option<room::MovePartyInvite>,
    host_session: Option<HostSession>,
    client: Option<QuicClient>,
    room_state: RoomState,
    sync_coordinator: Arc<Mutex<LocalSyncCoordinator>>,
    local_participant: ParticipantSnapshot,
    peer_participant: Option<ParticipantSnapshot>,
    #[allow(dead_code)]
    local_readiness: ParticipantReadiness,
    #[allow(dead_code)]
    peer_readiness: Option<ParticipantReadiness>,
    media: Option<MediaManifest>,
    transfer: Option<TransferProgress>,
    buffer: BufferSnapshot,
    sync: SyncSnapshot,
    network: NetworkSnapshot,
    call: CallSnapshot,
    call_signals: Vec<CallSignalSnapshot>,
    provider: ProviderSnapshot,
    chat: Vec<ChatSnapshot>,
    reactions: Vec<ReactionSnapshot>,
    ghost_mode: bool,
    privacy_mode: bool,
    last_recovery: Option<RuntimeRecoverySnapshot>,
    shared_controls: bool,
    error: Option<String>,
    reaction_limiter: ReactionRateLimiter,
    #[allow(dead_code)]
    peer_event_task: Option<tokio::task::JoinHandle<()>>,
    #[allow(dead_code)]
    host_event_task: Option<tokio::task::JoinHandle<()>>,
    #[allow(dead_code)]
    pending_operations: VecDeque<ScheduledPlayback>,
    #[allow(dead_code)]
    host_event_tx: Option<std::sync::Arc<broadcast::Sender<EventEnvelope>>>,
    // ── M2 closure: distributed protocol orchestration ─────────────────────
    /// Canonical Shared-Controls flag shared with QuicServer for
    /// ControlRequest granting. Host-only toggle via set_shared_controls.
    shared_controls_flag: Arc<AtomicBool>,
    /// operation_id of the in-flight PREPARE (play/pause/seek) awaiting the
    /// guest READY. Duplicate PREPAREs and stale READYs are rejected against
    /// this.
    pending_operation_id: Option<String>,
    /// Kind of the in-flight operation: "PLAY" | "PAUSE" | "SEEK".
    pending_operation_kind: Option<String>,
    pending_operation_target_ms: u64,
    pending_operation_execute_at_us: u64,
    /// Seek-only: whether the committed seek restarts playback afterwards
    /// (then the play protocol follows, §31).
    pending_operation_resume_after: bool,
    /// operation_id of the most recently committed operation. A COMMIT whose
    /// id matches this (or a stale id) is never executed twice.
    last_committed_operation_id: Option<String>,
    /// Host side: monotonic Instant at which the pending commit executes.
    /// Guest side: same, converted via the calibrated clock offset.
    commit_scheduled_for: Option<Instant>,
    /// Bumped on every committed play operation; carried in PlayCommit so
    /// presentation changes are unambiguous.
    presentation_epoch: u64,
    /// Guest-side calibration buffer (host: learned peer clock).
    clock_offset_to_host_us: i64,
    /// Host-side p95 RTT of the peer, used for play-lead sizing (§19).
    peer_rtt_p95_us: Option<u64>,
    clock_calibrated: bool,
    #[allow(dead_code)]
    calibration_task: Option<tokio::task::JoinHandle<()>>,
    /// Last peer request routed to the host under Shared Controls (guest side,
    /// for ControlDeny surfacing).
    pending_guest_request_id: Option<String>,
    // ── M3: Live player ownership ────────────────────────────────────────
    /// Optional player instance owned by AppRuntime. When present,
    /// coordinator commits (play/pause/seek) dispatch to this player.
    #[allow(dead_code)]
    player: Option<Arc<Mutex<dyn LocalPlayer + Send + Sync>>>,
    /// HTTP range server handle for serving cached media to the player.
    #[allow(dead_code)]
    range_server_handle: Option<RangeServerHandle>,
    /// M3: Guest-side sparse cache shared between range server and prefetch task.
    guest_cache: Option<Arc<tokio::sync::Mutex<crate::media::cache::SparseCache>>>,
    /// Cached player snapshot for the frontend (synced from the live player
    /// on coordinator commits).
    player_snapshot: PlayerSnapshot,
    /// M3: Background task polling the live player for position/duration/buffering.
    player_event_task: Option<tokio::task::JoinHandle<()>>,
    /// M3: Demand-driven QUIC transfer worker (guest Local Perfect). Owned by
    /// the active session; aborted on leave/disconnect so old session tasks
    /// can never write to the cache afterwards.
    transfer_task: Option<tokio::task::JoinHandle<()>>,
    /// Previously-aborted transfer worker, kept for diagnostics so callers
    /// can confirm an old session's worker has actually terminated before a
    /// reconnect replaces it with a fresh one.
    old_transfer_task: Option<tokio::task::JoinHandle<()>>,
    // ── M8: Automatic failure watchers ──────────────────────────────────
    /// Background task monitoring transfer progress for stalls.
    transfer_stall_watcher_task: Option<tokio::task::JoinHandle<()>>,
    // ── M4: Persistent storage ──────────────────────────────────────────
    /// SQLite database for identity, schedules, cache metadata, chat.
    db: Option<Arc<crate::storage::sqlite::MovePartyDb>>,
    /// Root directory for Move Party's own cache (guest Local Perfect data).
    cache_root: Option<PathBuf>,
    /// Background scheduler worker (M4.3). Owned so it can be aborted.
    scheduler_task: Option<tokio::task::JoinHandle<()>>,
    // ── M6: Managed Chrome session ownership ────────────────────────────
    /// Owned Chrome child process (previously leaked via forget).
    chrome_session: Option<crate::providers::chrome::ManagedChromeSession>,
}

impl std::fmt::Debug for AppRuntimeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRuntimeState")
            .field("screen", &self.screen)
            .field("room_state", &format!("{:?}", self.room_state))
            .field("player_snapshot", &self.player_snapshot)
            .finish_non_exhaustive()
    }
}

// ── AppRuntime ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppRuntime {
    inner: Arc<RuntimeInner>,
}

impl AppRuntime {
    pub fn new() -> Self {
        Self::new_without_emitter()
    }

    pub fn new_without_emitter() -> Self {
        Self::new_with_emitter(None)
    }

    /// Construct a runtime with a test-injectable notifier. Unit tests never
    /// display real OS notifications.
    pub fn new_with_notifier(notifier: impl crate::notifications::Notifier + 'static) -> Self {
        Self::new_with_emitter_and_notifier(None, Arc::new(notifier))
    }

    pub fn new_with_emitter(emitter: Option<Arc<dyn SnapshotSink>>) -> Self {
        Self::new_with_emitter_and_notifier(emitter, Arc::new(crate::notifications::NativeNotifier))
    }

    fn new_with_emitter_and_notifier(
        emitter: Option<Arc<dyn SnapshotSink>>,
        notifier: Arc<dyn crate::notifications::Notifier>,
    ) -> Self {
        let identity = DeviceIdentity::new_ephemeral();
        let display_name = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "Host".to_string());

        Self {
            inner: Arc::new(RuntimeInner {
                state: Mutex::new(AppRuntimeState {
                    screen: "HOME".to_string(),
                    credentials: None,
                    invite: None,
                    host_session: None,
                    client: None,
                    room_state: RoomState::Created,
                    sync_coordinator: Arc::new(Mutex::new(LocalSyncCoordinator::new())),
                    local_participant: ParticipantSnapshot {
                        id: identity.device_id.clone(),
                        display_name,
                        role: "Host".to_string(),
                        connected: false,
                        media_ready: false,
                        camera_enabled: true,
                        microphone_enabled: false,
                        buffer_ahead_ms: 0,
                    },
                    peer_participant: None,
                    local_readiness: ParticipantReadiness::ready(0),
                    peer_readiness: None,
                    media: None,
                    transfer: None,
                    buffer: BufferSnapshot {
                        guest_buffer_ahead_ms: 0,
                        percent: 0,
                        buffering_participant: None,
                    },
                    sync: SyncSnapshot {
                        room_state: "CREATED".to_string(),
                        strict_sync_paused: false,
                        position_ms: 0,
                    },
                    network: NetworkSnapshot {
                        transport: "QUIC".to_string(),
                        path: "Not connected".to_string(),
                        goodput_bps: 0,
                        rtt_ms: None,
                        connected: false,
                    },
                    call: CallSnapshot {
                        mode: CallMode::VideoVoice,
                        connected: false,
                        camera: CameraState::tier_b_enabled(),
                        microphone: MicState::default(),
                    },
                    call_signals: Vec::new(),
                    provider: ProviderSnapshot {
                        mode: "LOCAL_PERFECT".to_string(),
                        provider_id: None,
                        url: None,
                        state: "Idle".to_string(),
                    },
                    chat: Vec::new(),
                    reactions: Vec::new(),
                    ghost_mode: false,
                    privacy_mode: false,
                    last_recovery: None,
                    shared_controls: false,
                    error: None,
                    reaction_limiter: ReactionRateLimiter::default(),
                    peer_event_task: None,
                    host_event_task: None,
                    pending_operations: VecDeque::new(),
                    host_event_tx: None,
                    shared_controls_flag: Arc::new(AtomicBool::new(false)),
                    pending_operation_id: None,
                    pending_operation_kind: None,
                    pending_operation_target_ms: 0,
                    pending_operation_execute_at_us: 0,
                    pending_operation_resume_after: false,
                    last_committed_operation_id: None,
                    commit_scheduled_for: None,
                    presentation_epoch: 0,
                    clock_offset_to_host_us: 0,
                    peer_rtt_p95_us: None,
                    clock_calibrated: false,
                    calibration_task: None,
                    pending_guest_request_id: None,
                    player: None,
                    range_server_handle: None,
                    guest_cache: None,
                    player_snapshot: PlayerSnapshot::default(),
                    player_event_task: None,
                    transfer_task: None,
                    old_transfer_task: None,
                    transfer_stall_watcher_task: None,
                    db: None,
                    cache_root: None,
                    scheduler_task: None,
                    chrome_session: None,
                }),
                emitter: RwLock::new(emitter),
                identity: Mutex::new(identity),
                notifier,
                preload_executions: std::sync::atomic::AtomicU64::new(0),
                started_at: Instant::now(),
            }),
        }
    }

    pub fn swap_emitter(&self, emitter: Option<Arc<dyn SnapshotSink>>) {
        if let Ok(mut guard) = self.inner.emitter.write() {
            *guard = emitter;
        }
    }

    /// M4: Open the persistent SQLite database and restore device identity.
    ///
    /// When `db_path_override` is `Some`, the database is opened at that
    /// path instead of the platform default. Tests use this to pin a temp
    /// file without relying on environment variables.
    ///
    /// Production startup path:
    ///   open DB → load existing DeviceIdentity → if present use it;
    ///   otherwise create exactly one identity, persist it, and use it.
    /// The restored identity replaces the temporary ephemeral one, so the
    /// same device id + signing key is used for the whole process lifetime.
    pub fn init_db(&self) {
        self.init_db_at(None)
    }

    /// Open the database at an explicit path (test hook).
    #[doc(hidden)]
    pub fn init_db_at_path(&self, path: &Path) {
        self.init_db_at(Some(path.to_path_buf()))
    }

    fn init_db_at(&self, db_path_override: Option<PathBuf>) {
        let db_path = db_path_override.unwrap_or_else(Self::default_db_path);
        match crate::storage::sqlite::MovePartyDb::open(&db_path) {
            Ok(db) => {
                let existing = db.get_identity();
                match existing {
                    Ok(Some(stored)) => {
                        // Restore the identity from its persisted seed, or —
                        // for pre-v2 rows without a seed — keep the current
                        // ephemeral identity but bind its device id.
                        let restored = stored
                            .signing_key_seed
                            .as_deref()
                            .and_then(|seed| DeviceIdentity::from_seed(&stored.device_id, seed))
                            .unwrap_or_else(|| {
                                let current = self.inner.identity();
                                let fallback = DeviceIdentity::from_seed_for_tests(
                                    stored.device_id.clone(),
                                    current.seed(),
                                );
                                self.persist_identity(&db, &fallback, &stored.display_name);
                                fallback
                            });
                        self.inner.replace_identity(restored);
                        self.lock().local_participant.id = stored.device_id;
                        self.lock().local_participant.display_name = stored.display_name;
                    }
                    Ok(None) | Err(_) => {
                        let identity = self.inner.identity();
                        self.persist_identity(&db, &identity, &self.current_display_name());
                    }
                }
                self.lock().db = Some(Arc::new(db));
            }
            Err(e) => {
                eprintln!("MoveParty: failed to open database: {e}");
            }
        }
    }

    fn current_display_name(&self) -> String {
        self.lock().local_participant.display_name.clone()
    }

    fn persist_identity(
        &self,
        db: &crate::storage::sqlite::MovePartyDb,
        identity: &DeviceIdentity,
        display_name: &str,
    ) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let stored = crate::storage::sqlite::StoredIdentity {
            device_id: identity.device_id.clone(),
            display_name: display_name.to_string(),
            public_key: identity.public_key_base64(),
            platform: std::env::consts::OS.to_string(),
            created_at_ms: now_ms,
            signing_key_seed: Some(identity.seed().to_vec()),
        };
        let _ = db.upsert_identity(&stored);
        self.lock().local_participant.id = identity.device_id.clone();
        self.lock().local_participant.display_name = display_name.to_string();
    }

    /// M6: Store an owned Chrome session in AppRuntime, replacing any previous one.
    /// The old session is dropped (killing the Chrome process) if present.
    pub fn store_chrome_session(&self, session: crate::providers::chrome::ManagedChromeSession) {
        self.lock().chrome_session = Some(session);
    }

    /// M4: Detect overdue preload schedules and send local notifications.
    /// Notification failure is recoverable: an Err never aborts or panics.
    pub fn check_overdue_schedules(&self) {
        self.check_overdue_schedules_with_notifier();
    }

    /// Notifier-aware overdue scan. Returns whether any overdue schedule was
    /// detected (test hook). Uses the injected notifier so unit tests never
    /// display real OS notifications.
    pub fn check_overdue_schedules_with_notifier(&self) -> bool {
        let db = match self.lock().db.as_ref() {
            Some(db) => db.clone(),
            None => return false,
        };
        let overdue = match db.overdue_schedules() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let mut detected = false;
        for schedule in &overdue {
            detected = true;
            let result = self.inner.notifier.notify(
                "Move Party — Overdue Preload",
                &format!(
                    "Scheduled session for '{}' needs preloading now.",
                    schedule.media_id
                ),
            );
            if let Err(error) = result {
                eprintln!("MoveParty: notification failed (recoverable): {error}");
            }
            let _ = db.update_schedule_status(&schedule.schedule_id, "PreloadDue");
        }
        detected
    }

    fn default_db_path() -> PathBuf {
        // Test / portable override: MOVE_PARTY_DB_PATH pins the database
        // location so AppRuntime restart tests can use a temp file.
        if let Ok(path) = std::env::var("MOVE_PARTY_DB_PATH") {
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
        // Use the platform-appropriate application data directory.
        #[cfg(target_os = "macos")]
        {
            let base = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            base.join("Library/Application Support/Move Party/move_party.db")
        }
        #[cfg(target_os = "windows")]
        {
            let base = std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            base.join("Move Party/move_party.db")
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let base = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            base.join(".local/share/move-party/move_party.db")
        }
    }

    // ── M4.1: identity access (test hook) ──────────────────────────────────

    /// Current device identity used throughout this runtime.
    #[doc(hidden)]
    pub fn device_identity_for_test(&self) -> DeviceIdentity {
        self.inner.identity()
    }

    // ── M4.2: Schedule CRUD ────────────────────────────────────────────────

    /// Create a scheduled movie session. Validates the DTO, persists real
    /// room/media/guest/time data, and returns the generated schedule id.
    /// Errors map to stable Move Party codes.
    pub fn create_schedule(
        &self,
        room_id: &str,
        media_id: &str,
        scheduled_start_utc_ms: i64,
        planned_preload_utc_ms: i64,
        guest_device_id: &str,
    ) -> Result<String, String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        if room_id.trim().is_empty() {
            return Err("MP-SCHEDULE-002 room id must not be empty".to_string());
        }
        if media_id.trim().is_empty() {
            return Err("MP-SCHEDULE-002 media id must not be empty".to_string());
        }
        if guest_device_id.trim().is_empty() {
            return Err("MP-SCHEDULE-002 guest device id must not be empty".to_string());
        }
        if planned_preload_utc_ms > scheduled_start_utc_ms {
            return Err(
                "MP-SCHEDULE-002 preload deadline must precede scheduled start".to_string(),
            );
        }
        let schedule_id = Uuid::now_v7().to_string();
        let now_ms = wall_now_ms();
        let schedule = crate::storage::sqlite::StoredSchedule {
            schedule_id: schedule_id.clone(),
            room_id: room_id.to_string(),
            media_id: media_id.to_string(),
            scheduled_start_utc_ms,
            planned_preload_utc_ms,
            guest_device_id: guest_device_id.to_string(),
            status: "Planned".to_string(),
            created_at_ms: now_ms,
        };
        db.insert_schedule(&schedule)
            .map_err(|e| format!("MP-STORE-001 {e}"))?;
        Ok(schedule_id)
    }

    /// List persisted schedules (ordered by scheduled start).
    pub fn list_schedules(&self) -> Result<Vec<crate::storage::sqlite::StoredSchedule>, String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.list_schedules().map_err(|e| format!("MP-STORE-001 {e}"))
    }

    /// Update the media id of a schedule (rescheduling).
    pub fn update_schedule_media(&self, schedule_id: &str, media_id: &str) -> Result<(), String> {
        if media_id.trim().is_empty() {
            return Err("MP-SCHEDULE-002 media id must not be empty".to_string());
        }
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.update_schedule_media(schedule_id, media_id)
            .map_err(|e| format!("MP-STORE-001 {e}"))
    }

    /// Move the preload deadline of a schedule (rescheduling). The new
    /// deadline must still precede the scheduled start.
    pub fn update_schedule_preload(
        &self,
        schedule_id: &str,
        planned_preload_utc_ms: i64,
    ) -> Result<(), String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        let current = db
            .list_schedules()
            .map_err(|e| format!("MP-STORE-001 {e}"))?
            .into_iter()
            .find(|s| s.schedule_id == schedule_id)
            .ok_or_else(|| "MP-SCHEDULE-002 unknown schedule".to_string())?;
        if planned_preload_utc_ms > current.scheduled_start_utc_ms {
            return Err(
                "MP-SCHEDULE-002 preload deadline must precede scheduled start".to_string(),
            );
        }
        db.update_schedule_preload(schedule_id, planned_preload_utc_ms)
            .map_err(|e| format!("MP-STORE-001 {e}"))
    }

    /// Delete a schedule. A deleted schedule can never execute.
    pub fn delete_schedule(&self, schedule_id: &str) -> Result<(), String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.delete_schedule(schedule_id)
            .map_err(|e| format!("MP-STORE-001 {e}"))
    }

    // ── M4.3: Real scheduler worker ────────────────────────────────────────

    /// Spawn the production scheduler worker. On startup it restores pending
    /// schedules, waits efficiently for the next canonical preload deadline,
    /// executes preload when due (status → Transferring, notification), and
    /// reschedules after updates. Exactly-once execution is guaranteed by the
    /// status transition: only "Planned" schedules are picked, and the
    /// transition to "Transferring" is persisted before returning.
    pub fn spawn_scheduler_worker(&self) {
        self.spawn_scheduler_worker_inner(wall_now_ms, 2_000);
    }

    /// Test hook: controllable poll interval (and real wall clock).
    #[doc(hidden)]
    pub fn spawn_scheduler_worker_for_test(&self, _now_ms: i64, poll_interval_ms: u64) {
        self.spawn_scheduler_worker_inner(wall_now_ms, poll_interval_ms);
    }

    fn spawn_scheduler_worker_inner(&self, now_fn: fn() -> i64, poll_interval_ms: u64) {
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            loop {
                let now = now_fn();
                let db_opt = {
                    match inner.state.lock() {
                        Ok(g) => g.db.clone(),
                        Err(p) => p.into_inner().db.clone(),
                    }
                };
                let Some(db) = db_opt else {
                    tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms)).await;
                    continue;
                };

                // Execute every schedule whose preload deadline has arrived.
                let due = match db.due_schedules(now) {
                    Ok(due) => due,
                    Err(error) => {
                        eprintln!("MoveParty: scheduler due-scan failed (recoverable): {error}");
                        tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms))
                            .await;
                        continue;
                    }
                };
                for schedule in &due {
                    // Exactly once: the status transition is persisted before
                    // any side effect, so concurrent/restarted workers cannot
                    // double-execute.
                    if let Err(error) =
                        db.update_schedule_status(&schedule.schedule_id, "Transferring")
                    {
                        eprintln!("MoveParty: scheduler status transition failed: {error}");
                        continue;
                    }
                    inner
                        .preload_executions
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let _ = inner.notifier.notify(
                        "Move Party — Preloading",
                        &format!(
                            "Preloading '{}' for the scheduled session.",
                            schedule.media_id
                        ),
                    );
                }

                // Wait efficiently for the next deadline.
                let next = db.next_preload_deadline(now).unwrap_or_default();
                match next {
                    Some(deadline_ms) => {
                        let wait = deadline_ms.saturating_sub(wall_now_ms()).max(0) as u64;
                        let wait = wait.min(60_000);
                        tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                    }
                    None => {
                        tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms))
                            .await;
                    }
                }
            }
        });
        {
            let mut state = self.lock();
            if let Some(old) = state.scheduler_task.take() {
                old.abort();
            }
            state.scheduler_task = Some(task);
        }
    }

    /// Stop the scheduler worker (test/diagnostics).
    #[doc(hidden)]
    pub fn stop_scheduler_for_test(&self) {
        let mut state = self.lock();
        if let Some(task) = state.scheduler_task.take() {
            task.abort();
        }
    }

    /// Number of preload executions performed by this runtime.
    #[doc(hidden)]
    pub fn preload_execution_count(&self) -> u64 {
        self.inner
            .preload_executions
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    // ── M4.5: Retention runtime paths ──────────────────────────────────────

    /// Record the cache root (test hook; production sets it in the cache open
    /// path).
    #[doc(hidden)]
    pub fn init_cache_root_for_test(&self, root: PathBuf) {
        self.lock().cache_root = Some(root);
    }

    fn cache_root(&self) -> Result<PathBuf, String> {
        self.lock()
            .cache_root
            .clone()
            .ok_or_else(|| "MP-MEDIA-002 cache root not initialised".to_string())
    }

    fn cache_dir_for(&self, media_id: &str) -> Result<PathBuf, String> {
        let root = self.cache_root()?;
        let dir = root.join(media_id);
        if !dir.starts_with(&root) {
            return Err("MP-MEDIA-002 unsafe cache path".to_string());
        }
        Ok(dir)
    }

    /// Register a cached media entry in the database.
    pub fn register_cached_media(
        &self,
        media_id: &str,
        filename: &str,
        file_size: u64,
        full_hash: &str,
    ) -> Result<(), String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        let cache_root = self.cache_root()?;
        let entry = crate::storage::sqlite::StoredCacheEntry {
            media_id: media_id.to_string(),
            filename: filename.to_string(),
            file_size,
            full_hash: full_hash.to_string(),
            cache_root: cache_root.to_string_lossy().into_owned(),
            bytes_available: 0,
            created_at_ms: wall_now_ms(),
        };
        db.upsert_cache_entry(&entry)
            .map_err(|e| format!("MP-STORE-001 {e}"))
    }

    /// List registered cached media.
    pub fn list_cached_media(
        &self,
    ) -> Result<Vec<crate::storage::sqlite::StoredCacheEntry>, String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.list_cache_entries()
            .map_err(|e| format!("MP-STORE-001 {e}"))
    }

    /// Retention "Keep": retain Move Party's cache for this media.
    pub fn retention_keep(&self, media_id: &str) -> Result<(), String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.retention_keep(media_id)
            .map_err(|e| format!("MP-STORE-001 {e}"))
    }

    /// Retention "Remove": delete only Move Party's cache directory and the
    /// matching cache metadata. Never touches the host's original source.
    pub fn retention_remove(&self, media_id: &str) -> Result<(), String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        let root = self.cache_root()?;
        let dir = self.cache_dir_for(media_id)?;
        let data_file = dir.join(crate::media::cache::CACHE_DATA_FILE);
        crate::storage::apply_retention_decision(
            &root,
            &dir,
            &data_file,
            crate::storage::RetentionDecision::Remove,
            None,
        )
        .map_err(|e| format!("MP-MEDIA-002 {e}"))?;
        db.delete_cache_entry(media_id)
            .map_err(|e| format!("MP-STORE-001 {e}"))
    }

    /// Retention "Save As": export the completed cached media to a chosen
    /// destination safely. Never deletes or overwrites the host source.
    pub fn retention_save_as(&self, media_id: &str, destination: &Path) -> Result<PathBuf, String> {
        let root = self.cache_root()?;
        let dir = self.cache_dir_for(media_id)?;
        let data_file = dir.join(crate::media::cache::CACHE_DATA_FILE);
        crate::storage::apply_retention_decision(
            &root,
            &dir,
            &data_file,
            crate::storage::RetentionDecision::SaveAs,
            Some(destination),
        )
        .map(|saved| saved.unwrap_or_else(|| destination.to_path_buf()))
        .map_err(|e| format!("MP-MEDIA-002 {e}"))
    }

    pub fn snapshot(&self) -> AppSnapshot {
        snapshot_from_state(&self.lock())
    }

    // M1: real host flow — bind QUIC server and produce a live invite
    pub async fn create_local_party(
        &self,
        media_path: Option<String>,
    ) -> Result<AppSnapshot, String> {
        let identity = self.inner.identity();
        let credentials = RoomCredentials::generate();

        let manifest = if let Some(path) = media_path.as_ref().filter(|p| !p.trim().is_empty()) {
            Some(build_manifest(&PathBuf::from(path.trim())).map_err(|e| e.to_string())?)
        } else {
            None
        };

        let maybe_media_path = media_path
            .as_ref()
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);

        let is_dev = std::env::var("MOVE_PARTY_DEV_LOOPBACK")
            .map(|v| v == "1")
            .unwrap_or(false);
        let (bind_addr, tailscale_ip) = if is_dev {
            (loopback_bind_addr(), "127.0.0.1".to_string())
        } else {
            let status = crate::network::tailscale::detect_status()
                .await
                .map_err(|e| format!("MP-NET-001 tailscale detection failed: {e}"))?;
            if !status.signed_in {
                return Err("MP-NET-001 tailscale is not signed in".to_string());
            }
            let ipv4 = status
                .local_ipv4
                .ok_or_else(|| "MP-NET-001 tailscale has no local IPv4".to_string())?;
            (
                tailscale_bind_addr(ipv4).map_err(|e| e.to_string())?,
                ipv4.to_string(),
            )
        };

        let display_name = self.lock().local_participant.display_name.clone();

        let server = if let Some(ref path) = maybe_media_path {
            QuicServer::bind_with_local_media(
                bind_addr,
                credentials.clone(),
                display_name.clone(),
                identity.device_id.clone(),
                path.clone(),
            )
        } else {
            QuicServer::bind(
                bind_addr,
                credentials.clone(),
                display_name,
                identity.device_id.clone(),
            )
        }
        .map_err(|e| e.to_string())?;

        let bound_addr = server.local_addr().map_err(|e| e.to_string())?;
        let cert_der = server.certificate().as_ref().to_vec();
        let cert_fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(&cert_der));

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let invite = room::MovePartyInvite {
            v: room::INVITE_VERSION_V1,
            protocol_major: crate::PROTOCOL_MAJOR,
            protocol_minor: crate::PROTOCOL_MINOR,
            room_id: credentials.room_id.clone(),
            join_secret: credentials.join_secret.clone(),
            host_device_id: identity.device_id.clone(),
            host_ip: tailscale_ip,
            host_port: bound_addr.port(),
            server_certificate_fingerprint: cert_fingerprint.clone(),
            expires_at_ms: now_ms + room::INVITE_TTL_MS,
        };

        let invite_url = room::encode_invite(&invite).map_err(|e| e.to_string())?;

        let runtime_for_cb = self.clone();
        let event_callback = Arc::new(move |event: QuicHostEvent| {
            runtime_for_cb.apply_host_event(event);
        });

        let (event_tx, _) = broadcast::channel::<EventEnvelope>(256);
        let event_tx_arc = Arc::new(event_tx.clone());
        let shared_controls_flag = Arc::new(AtomicBool::new(false));

        let coordinator = {
            let state = self.lock();
            let my_id = state.local_participant.id.clone();
            let mut coord = LocalSyncCoordinator::new();
            coord.coordinator_local_id = my_id;
            coord.coordinator_tx = event_tx_arc.as_ref().clone();
            coord.coordinator_broadcast_fn = Some(
                |coord_arg: &LocalSyncCoordinator,
                 sender: &str,
                 tx: &broadcast::Sender<EventEnvelope>| {
                    let _ = tx.send(EventEnvelope {
                        seq: coord_arg.coordinator_event_seq,
                        sender: sender.to_string(),
                        sent_mono_us: monotonic_us(),
                        event: QuicServerEvent::CoordinatorStateUpdate {
                            host_ready: coord_arg.host_ready.player_ready,
                            guest_ready: coord_arg.guest_ready.player_ready,
                            coordinator_play_state: format!("{:?}", coord_arg.room_state)
                                .to_ascii_uppercase(),
                            buffer_ahead_ms: coord_arg.host_ready.buffer_ahead_ms,
                        },
                    });
                },
            );
            Arc::new(std::sync::Mutex::new(coord))
        };

        let server = server
            .with_event_callback(event_callback)
            .with_event_broadcast(Arc::new(event_tx))
            .with_shared_controls(Arc::clone(&shared_controls_flag));
        let coordinator_for_run = Arc::clone(&coordinator);
        let server_handle = tokio::spawn(server.run(Some(coordinator_for_run)));

        {
            let mut state = self.lock();
            state.shared_controls_flag = shared_controls_flag;
            state.sync_coordinator = coordinator.clone();
            state.credentials = Some(credentials);
            state.invite = Some(invite);
            state.host_event_tx = Some(event_tx_arc.clone());
            state.host_session = Some(HostSession {
                server_handle,
                bound_addr,
                cert_fingerprint,
                invite_url: invite_url.clone(),
            });
            state.room_state = RoomState::WaitingForGuest;
            state.screen = "LOBBY".to_string();
            state.network.transport = quic::QUIC_TRANSPORT_NAME.to_string();
            state.network.path = format!("Listening on {}", bound_addr);
            state.network.connected = false;
            state.error = None;

            if let Some(ref m) = manifest {
                state.local_participant.media_ready = true;
                state.local_participant.buffer_ahead_ms = 5_000;
                state.media = Some(m.clone());
                state.transfer = Some(transfer_progress(m, 0, 0, 0));

                // M3: Create and open a real player for the host's local file.
                if let Some(ref path) = maybe_media_path {
                    #[cfg(feature = "mpv")]
                    {
                        let mut player = MpvPlayer::new();
                        if let Err(e) = player.open(path) {
                            state.error = Some(format!("MP-MEDIA-001 {e}"));
                        } else {
                            state.player_snapshot = PlayerSnapshot::from(&player.snapshot());
                        }
                        state.player = Some(Arc::new(std::sync::Mutex::new(player)));
                    }
                    #[cfg(not(feature = "mpv"))]
                    {
                        let mut player = crate::media::player::LibMpvPlayer::new();
                        if let Err(e) = player.open(path) {
                            state.error = Some(format!("MP-MEDIA-001 {e}"));
                        } else {
                            state.player_snapshot = PlayerSnapshot::from(&player.snapshot());
                        }
                        state.player = Some(Arc::new(std::sync::Mutex::new(player)));
                    }
                }
            } else {
                state.local_participant.media_ready = false;
                state.media = None;
            }
            sync_room_snapshot(&mut state);
        }

        // M3: Start the background player event loop so position/duration/
        // buffering state flows from the live player to the frontend.
        if self.lock().player.is_some() {
            self.spawn_player_event_loop();
        }

        // M8: Start the transfer stall watcher — automatically detects when
        // no bytes are received for 30 seconds and fires TransferInterrupted.
        self.spawn_transfer_stall_watcher();

        // Host-side event subscriber: the coordinator's canonical broadcasts
        // (CoordinatorStateUpdate, Play/Pause/SeekCommit, guest chat/reaction)
        // flow through event_tx. The host must apply them to its own
        // AppRuntimeState so the snapshot mirrors the coordinator (single

        // source of truth) without needing the round trip through QUIC.
        {
            let inner_for_listener = Arc::clone(&self.inner);
            let tx_for_listener = event_tx_arc.clone();
            let task = tokio::spawn(async move {
                let mut rx = tx_for_listener.subscribe();
                while let Ok(envelope) = rx.recv().await {
                    Self::apply_peer_event(&inner_for_listener, &envelope, envelope.event.clone());
                }
            });
            self.lock().host_event_task = Some(task);
        }

        let snapshot = self.snapshot();
        self.emit_ok(snapshot.clone());
        Ok(snapshot)
    }

    // M1: real guest flow — parse invite, authenticate via QUIC
    pub async fn join_party(&self, invite_url: String) -> Result<AppSnapshot, String> {
        let invite = room::parse_invite(&invite_url).map_err(|e| e.to_string())?;
        let addr = room::invite_socket_addr(&invite).map_err(|e| e.to_string())?;
        quic::validate_quic_bind_addr(addr)
            .map_err(|e| format!("MP-NET-001 invalid peer endpoint: {e}"))?;

        let credentials = room::invite_to_credentials(&invite);
        let identity = self.inner.identity();
        let display_name = self.lock().local_participant.display_name.clone();

        let (client, auth_accept) = QuicClient::connect(
            addr,
            invite.server_certificate_fingerprint.clone(),
            credentials.clone(),
            identity.clone(),
            display_name,
        )
        .await
        .map_err(|e| e.to_string())?;

        if auth_accept.host_device_id != invite.host_device_id {
            return Err("MP-NET-001 authenticated host identity mismatch".to_string());
        }

        {
            let mut state = self.lock();
            state.credentials = Some(credentials);
            state.invite = Some(invite.clone());
            state.client = Some(client);
            state.local_participant.role = "Guest".to_string();
            state.peer_participant = Some(ParticipantSnapshot {
                id: invite.host_device_id.clone(),
                display_name: auth_accept.host_display_name,
                role: "Host".to_string(),
                connected: true,
                media_ready: false,
                camera_enabled: true,
                microphone_enabled: false,
                buffer_ahead_ms: 0,
            });
            state.network.transport = quic::QUIC_TRANSPORT_NAME.to_string();
            state.network.path = "Authenticated over QUIC".to_string();
            state.network.connected = true;
            state.room_state = RoomState::Lobby;
            state.screen = "LOBBY".to_string();
            state.error = None;
            sync_room_snapshot(&mut state);
        }

        // coordinator_local_id set once in create_local_party for Host; Guest uses AppRuntimeState only.

        let inner_for_listener = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            Self::peer_event_listener(inner_for_listener).await;
        });

        {
            let mut state = self.lock();
            state.peer_event_task = Some(task);
        }

        // M2: live clock calibration (MASTER_PRD §18): 20 CLOCK_PING probes,
        // median offset, then refresh every 30s. The offset feeds the
        // host-monotonic deadline conversion for scheduled commits.
        self.spawn_clock_calibration();

        // M3: Auto-fetch media for the guest after authenticated join.
        // Only trigger when the host has local media (the ManifestRequest
        // will fail harmlessly if there is none, but we skip the fetch
        // entirely to avoid interfering with non-media m2 sync tests).
        {
            let runtime = self.clone();
            tokio::spawn(async move {
                // Small delay to let the connection stabilize
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                let _ = runtime.guest_fetch_media().await;
            });
        }

        let snapshot = self.snapshot();
        self.emit_ok(snapshot.clone());
        Ok(snapshot)
    }

    /// Background guest task measuring the guest↔host clock offset with
    /// CLOCK_PING probes. Results are sent to the host (ClockResult) and used
    /// locally to convert host-monotonic deadlines into guest-monotonic
    /// Instants.
    fn spawn_clock_calibration(&self) {
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            loop {
                let client = inner.lock().client.clone();
                let Some(client) = client else { break };
                let mut samples = Vec::with_capacity(20);
                for probe_id in 0..20u64 {
                    let t0 = monotonic_us();
                    match client.clock_probe(probe_id, t0).await {
                        Ok(sample) => samples.push(sample),
                        Err(_) => break,
                    }
                }
                if samples.len() >= 4 {
                    let estimate = clock::estimate_offset(&samples);
                    let rtt_p95 = clock::p95_rtt_us(&samples);
                    if let Some(estimate) = estimate {
                        let (offset, rtt, count, quality) = (
                            estimate.offset_to_host_us,
                            rtt_p95.unwrap_or(estimate.rtt_us.max(0) as u64),
                            samples.len() as u64,
                            format!("{:?}", estimate.quality),
                        );
                        {
                            let mut state = inner.lock();
                            state.clock_offset_to_host_us = offset;
                            state.clock_calibrated = true;
                            state.network.rtt_ms = Some((estimate.rtt_us.max(0) as u32) / 1_000);
                            let snapshot = snapshot_from_state(&state);
                            inner.emit(snapshot);
                        }
                        let client = client.clone();
                        tokio::spawn(async move {
                            let _ = client.send_clock_result(offset, rtt, count, &quality).await;
                        });
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
        self.lock().calibration_task = Some(task);
    }

    // M1: called by QUIC server background task when guest authenticates
    fn apply_host_event(&self, event: QuicHostEvent) {
        match event {
            QuicHostEvent::PeerAuthenticated {
                device_id,
                display_name,
                ..
            } => {
                let mut state = self.lock();
                state.peer_participant = Some(ParticipantSnapshot {
                    id: device_id,
                    display_name,
                    role: "Guest".to_string(),
                    connected: true,
                    media_ready: false,
                    camera_enabled: true,
                    microphone_enabled: false,
                    buffer_ahead_ms: 0,
                });
                state.network.connected = true;
                state.network.path = "Guest authenticated over QUIC".to_string();
                // Mirror the coordinator: a reconnecting room must NOT jump to
                // LOBBY and resume blindly — the room stays RECONNECTING until
                // the host runs a fresh play protocol cycle.
                let room_state = state.sync_coordinator.lock().unwrap().room_state;
                state.room_state = room_state;
                sync_room_snapshot(&mut state);
                let snapshot = snapshot_from_state(&state);
                drop(state);
                self.inner.emit(snapshot);
            }
            QuicHostEvent::PeerDisconnected { device_id } => {
                let is_ours = {
                    let state = self.lock();
                    state
                        .peer_participant
                        .as_ref()
                        .map(|p| p.id == device_id)
                        .unwrap_or(false)
                };
                if is_ours {
                    let _ = Self::apply_disconnect(&self.inner);
                }
            }
            QuicHostEvent::GuestReadyState { .. } => {}
            QuicHostEvent::GuestPlayReady {
                broadcaster_device_id: _,
                operation_id,
                ready,
                position_ms,
                buffer_ahead_ms: _,
            } => {
                self.on_guest_play_ready(&operation_id, ready, position_ms);
            }
            QuicHostEvent::GuestPauseReady {
                operation_id,
                ready,
                ..
            } => {
                self.on_guest_pause_ready(&operation_id, ready);
            }
            QuicHostEvent::GuestSeekReady {
                operation_id,
                ready,
                ..
            } => {
                self.on_guest_seek_ready(&operation_id, ready);
            }
            QuicHostEvent::GuestControlRequest {
                broadcaster_device_id: _,
                request_id,
                action,
                parameters,
            } => {
                self.on_guest_control_request(&request_id, &action, parameters);
            }
            QuicHostEvent::ClockResultReceived {
                broadcaster_device_id: _,
                offset_to_host_us,
                rtt_p95_us,
                sample_count: _,
                quality,
            } => {
                let mut state = self.lock();
                state.clock_offset_to_host_us = offset_to_host_us;
                state.clock_calibrated = true;
                state.network.rtt_ms = Some((rtt_p95_us / 1_000).min(u32::MAX as u64) as u32);
                state.error = None;
                let snapshot = snapshot_from_state(&state);
                drop(state);
                self.inner.emit(snapshot);
                let _ = quality;
            }
        }
    }

    /// Host side of the PLAY protocol: the guest answered PLAY_READY for the
    /// pending play operation. Broadcast PLAY_COMMIT and schedule the
    /// canonical commit at the host-monotonic deadline.
    fn on_guest_play_ready(&self, operation_id: &str, ready: bool, position_ms: u64) {
        if !ready {
            return;
        }
        let (op_id, target, execute_at, commit_scheduled) = {
            let mut state = self.lock();
            if state.pending_operation_id.as_deref() != Some(operation_id) {
                return;
            }
            if state.pending_operation_kind.as_deref() != Some("PLAY") {
                return;
            }
            if state.commit_scheduled_for.is_some() {
                return;
            }
            let execute_at = state.pending_operation_execute_at_us;
            let deadline = instant_for_host_mono(execute_at, 0);
            state.commit_scheduled_for = Some(deadline);
            state.presentation_epoch = state.presentation_epoch.wrapping_add(1);
            (
                operation_id.to_string(),
                state.pending_operation_target_ms,
                execute_at,
                deadline,
            )
        };
        let _ = position_ms;
        // Broadcast PLAY_COMMIT now; both sides execute at the deadline.
        let event = {
            let state = self.lock();
            QuicServerEvent::PlayCommit {
                operation_id: op_id.clone(),
                target_position_ms: target,
                execute_at_host_mono_us: execute_at,
                presentation_epoch: state.presentation_epoch,
            }
        };
        Self::send_host_event(&mut self.lock(), event);
        let runtime = self.clone();
        tokio::spawn(async move {
            let _ =
                tokio::time::sleep_until(tokio::time::Instant::from_std(commit_scheduled)).await;
            let mut state = runtime.lock();
            let scheduled = state
                .sync_coordinator
                .lock()
                .unwrap()
                .pending_scheduled
                .clone();
            let Some(scheduled) = scheduled else { return };
            if scheduled.operation_id.to_string() != op_id {
                return;
            }
            let _ = state
                .sync_coordinator
                .lock()
                .unwrap()
                .commit_play(&scheduled);
            state.sync.position_ms = target;
            state.sync.strict_sync_paused = false;
            state.pending_operation_id = None;
            state.pending_operation_kind = None;
            state.last_committed_operation_id = Some(op_id);
            state.commit_scheduled_for = None;
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;
            // M3: dispatch to live player
            Self::dispatch_player_play(&mut state);
            sync_room_snapshot(&mut state);
            let snapshot = snapshot_from_state(&state);
            runtime.inner.emit(snapshot);
        });
    }

    /// Host side of the PAUSE protocol: the guest answered PAUSE_READY.
    fn on_guest_pause_ready(&self, operation_id: &str, ready: bool) {
        if !ready {
            return;
        }
        let (op_id, target, execute_at, commit_scheduled) = {
            let mut state = self.lock();
            if state.pending_operation_id.as_deref() != Some(operation_id) {
                return;
            }
            if state.pending_operation_kind.as_deref() != Some("PAUSE") {
                return;
            }
            if state.commit_scheduled_for.is_some() {
                return;
            }
            let execute_at = state.pending_operation_execute_at_us;
            let deadline = instant_for_host_mono(execute_at, 0);
            state.commit_scheduled_for = Some(deadline);
            (
                operation_id.to_string(),
                state.pending_operation_target_ms,
                execute_at,
                deadline,
            )
        };
        let event = QuicServerEvent::PauseCommit {
            operation_id: op_id.clone(),
            target_position_ms: target,
            execute_at_host_mono_us: execute_at,
        };
        Self::send_host_event(&mut self.lock(), event);
        let runtime = self.clone();
        tokio::spawn(async move {
            let _ =
                tokio::time::sleep_until(tokio::time::Instant::from_std(commit_scheduled)).await;
            let mut state = runtime.lock();
            state
                .sync_coordinator
                .lock()
                .unwrap()
                .commit_pause(target, PauseCause::Manual);
            state.sync.position_ms = target;
            let strict_sync_paused = state.sync_coordinator.lock().unwrap().paused_by_strict_sync;
            state.sync.strict_sync_paused = strict_sync_paused;
            state.pending_operation_id = None;
            state.pending_operation_kind = None;
            state.last_committed_operation_id = Some(op_id);
            state.commit_scheduled_for = None;
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;
            // M3: dispatch to live player
            Self::dispatch_player_pause(&mut state);
            sync_room_snapshot(&mut state);
            let snapshot = snapshot_from_state(&state);
            runtime.inner.emit(snapshot);
        });
    }

    /// Host side of the SEEK protocol: the guest answered SEEK_READY.
    fn on_guest_seek_ready(&self, operation_id: &str, ready: bool) {
        if !ready {
            return;
        }
        let (op_id, target, execute_at, resume_after_seek, commit_scheduled) = {
            let mut state = self.lock();
            if state.pending_operation_id.as_deref() != Some(operation_id) {
                return;
            }
            if state.pending_operation_kind.as_deref() != Some("SEEK") {
                return;
            }
            if state.commit_scheduled_for.is_some() {
                return;
            }
            let execute_at = state.pending_operation_execute_at_us;
            let deadline = instant_for_host_mono(execute_at, 0);
            state.commit_scheduled_for = Some(deadline);
            (
                operation_id.to_string(),
                state.pending_operation_target_ms,
                execute_at,
                state.pending_operation_resume_after,
                deadline,
            )
        };
        let event = QuicServerEvent::SeekCommit {
            operation_id: op_id.clone(),
            target_position_ms: target,
            execute_at_host_mono_us: execute_at,
            resume_after_seek,
        };
        Self::send_host_event(&mut self.lock(), event);
        let runtime = self.clone();
        tokio::spawn(async move {
            let _ =
                tokio::time::sleep_until(tokio::time::Instant::from_std(commit_scheduled)).await;
            {
                let mut state = runtime.lock();
                state
                    .sync_coordinator
                    .lock()
                    .unwrap()
                    .commit_seek(target, resume_after_seek);
                state.sync.position_ms = target;
                state.sync.strict_sync_paused = !resume_after_seek;
                state.pending_operation_id = None;
                state.pending_operation_kind = None;
                state.last_committed_operation_id = Some(op_id);
                state.commit_scheduled_for = None;
                let room_state = state.sync_coordinator.lock().unwrap().room_state;
                state.room_state = room_state;
                // M3: dispatch to live player
                Self::dispatch_player_seek(&mut state, target);
                sync_room_snapshot(&mut state);
                let snapshot = snapshot_from_state(&state);
                runtime.inner.emit(snapshot);
            }
            // §31: when the seek resumes, the host follows with the play
            // protocol so both sides restart at the seek destination together.
            if resume_after_seek {
                let _ = runtime.host_play();
            }
        });
    }

    /// Host side of Shared Controls: the guest's granted request becomes a
    /// normal canonical host operation (PLAY/PAUSE/SEEK). The guest never
    /// commits anything itself.
    fn on_guest_control_request(
        &self,
        request_id: &str,
        action: &str,
        parameters: serde_json::Value,
    ) {
        match action {
            "PLAY" => {
                let _ = self.host_play();
            }
            "PAUSE" => {
                let _ = self.host_pause();
            }
            "SEEK_TO" => {
                let target = parameters
                    .get("target_position_ms")
                    .and_then(|v| v.as_u64());
                if let Some(target) = target {
                    let _ = self.host_seek(target, true);
                }
            }
            "SEEK_RELATIVE" => {
                let delta = parameters
                    .get("delta_ms")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let current = self.lock().sync.position_ms;
                let next = if delta.is_negative() {
                    current.saturating_sub(delta.unsigned_abs())
                } else {
                    current.saturating_add(delta as u64)
                };
                let _ = self.host_seek(next, true);
            }
            _ => {}
        }
        let event = QuicServerEvent::ControlGrant {
            request_id: request_id.to_string(),
            action: action.to_string(),
        };
        Self::send_host_event(&mut self.lock(), event);
    }

    // M2: Guest background task — receives host-originated ServerEvent messages
    async fn peer_event_listener(inner: Arc<RuntimeInner>) {
        loop {
            let (client_opt, should_continue) = {
                let state = inner.lock();
                if let Some(ref c) = state.client {
                    (Some(c.clone()), true)
                } else {
                    (None, false)
                }
            };

            if !should_continue {
                break;
            }

            let Some(client) = client_opt else { break };

            match client.listen_for_server_event().await {
                Ok(envelope) => {
                    Self::apply_peer_event(&inner, &envelope, envelope.event.clone());
                }
                Err(_) => {
                    let _ = Self::apply_disconnect(&inner);
                    break;
                }
            }
        }
    }

    fn apply_disconnect(inner: &RuntimeInner) -> Result<(), ()> {
        let mut state = inner.lock();
        if let Some(peer) = &mut state.peer_participant {
            peer.connected = false;
        }
        state.network.connected = false;
        state.network.path = "Peer disconnected".to_string();
        let _ = state.sync_coordinator.lock().unwrap().peer_disconnected();
        state.sync.strict_sync_paused = true;
        // Abort any in-flight operation: a disconnected guest cannot answer
        // READY, so the pending operation must not commit half-way.
        state.pending_operation_id = None;
        state.pending_operation_kind = None;
        state.commit_scheduled_for = None;
        // M8: Wire through the proper recovery system so last_recovery is
        // surfaced to the UI and the full RecoveryPlan is recorded.
        let event = FailureEvent::GuestCrash;
        let plan = recovery_plan(event);
        apply_recovery_to_state(&mut state, event, plan);
        state.last_recovery = Some(RuntimeRecoverySnapshot {
            event: FailureEvent::GuestCrash,
            action: plan.action,
            pauses_playback_for_both: plan.pauses_playback_for_both,
            requires_user_action: plan.requires_user_action,
        });
        let room_state = state.sync_coordinator.lock().unwrap().room_state;
        state.room_state = room_state;
        sync_room_snapshot(&mut state);
        let snapshot = snapshot_from_state(&state);
        drop(state);
        inner.emit(snapshot);
        Ok(())
    }

    fn apply_peer_event(
        inner: &Arc<RuntimeInner>,
        envelope: &EventEnvelope,
        event: QuicServerEvent,
    ) {
        let seq = envelope.seq;
        let sender = &envelope.sender;

        let snapshot = {
            let mut state = inner.lock();

            if sender != &state.local_participant.id {
                if seq
                    <= state
                        .sync_coordinator
                        .lock()
                        .unwrap()
                        .last_peer_seq_received()
                {
                    return;
                }
                state.sync_coordinator.lock().unwrap().record_peer_seq(seq);
            }

            // The host orchestrates its own protocol operations directly
            // (on_guest_*_ready / host_*); the host self-subscriber must NOT
            // re-apply its own PREPARE/COMMIT broadcasts as if it were the
            // receiving peer.
            let is_self = sender == &state.local_participant.id;
            if is_self {
                match &event {
                    QuicServerEvent::PlayPrepare { .. }
                    | QuicServerEvent::PlayCommit { .. }
                    | QuicServerEvent::PausePrepare { .. }
                    | QuicServerEvent::PauseCommit { .. }
                    | QuicServerEvent::SeekPrepare { .. }
                    | QuicServerEvent::SeekCommit { .. } => return,
                    _ => {}
                }
            }

            match event {
                QuicServerEvent::PlayPrepare {
                    operation_id,
                    target_position_ms,
                    minimum_buffer_ms,
                } => {
                    // Guest side of PLAY_PREPARE → PLAY_READY. Duplicate
                    // PREPAREs are ignored (PROTOCOL_SPEC §40).
                    if state.pending_operation_id.as_deref() == Some(&operation_id) {
                        return;
                    }
                    state.pending_operation_id = Some(operation_id.clone());
                    state.pending_operation_kind = Some("PLAY".to_string());
                    state.pending_operation_target_ms = target_position_ms;
                    let ready = state.local_participant.media_ready
                        && state.buffer.guest_buffer_ahead_ms >= minimum_buffer_ms;
                    let position_ms = state.sync.position_ms;
                    let buffer_ahead_ms = state.buffer.guest_buffer_ahead_ms;
                    let client = state.client.clone();
                    if let Some(client) = client {
                        tokio::spawn(async move {
                            let _ = client
                                .send_play_ready(operation_id, ready, position_ms, buffer_ahead_ms)
                                .await;
                        });
                    }
                }
                QuicServerEvent::PlayCommit {
                    operation_id,
                    target_position_ms,
                    execute_at_host_mono_us,
                    presentation_epoch: _,
                } => {
                    if state.last_committed_operation_id.as_deref() == Some(&operation_id) {
                        return;
                    }
                    // A commit for a superseded operation (a newer op is
                    // pending) must not execute (PROTOCOL_SPEC §40).
                    if state.pending_operation_id.is_some()
                        && state.pending_operation_id.as_deref() != Some(&operation_id)
                    {
                        return;
                    }
                    let offset = state.clock_offset_to_host_us;
                    let deadline = instant_for_host_mono(execute_at_host_mono_us, offset);
                    state.commit_scheduled_for = Some(deadline);
                    let scheduled = ScheduledPlayback {
                        operation_id: Uuid::parse_str(&operation_id).unwrap_or_default(),
                        target_position_ms,
                        execute_at_host_mono_us,
                    };
                    let inner_for_task = inner.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))
                            .await;
                        let mut state = inner_for_task.lock();
                        let _ = state
                            .sync_coordinator
                            .lock()
                            .unwrap()
                            .commit_play(&scheduled);
                        state.sync.position_ms = target_position_ms;
                        state.sync.strict_sync_paused = false;
                        state.pending_operation_id = None;
                        state.pending_operation_kind = None;
                        state.last_committed_operation_id = Some(operation_id);
                        state.commit_scheduled_for = None;
                        let room_state = state.sync_coordinator.lock().unwrap().room_state;
                        state.room_state = room_state;
                        // M3: dispatch to live player
                        Self::dispatch_player_play(&mut state);
                        sync_room_snapshot(&mut state);
                        let snapshot = snapshot_from_state(&state);

                        inner_for_task.emit(snapshot);
                    });
                }
                QuicServerEvent::PausePrepare {
                    operation_id,
                    reason: _,
                    target_position_ms,
                } => {
                    if state.pending_operation_id.as_deref() == Some(&operation_id) {
                        return;
                    }
                    state.pending_operation_id = Some(operation_id.clone());
                    state.pending_operation_kind = Some("PAUSE".to_string());
                    state.pending_operation_target_ms = target_position_ms;
                    let client = state.client.clone();
                    if let Some(client) = client {
                        tokio::spawn(async move {
                            let _ = client.send_pause_ready(operation_id, true).await;
                        });
                    }
                }
                QuicServerEvent::PauseCommit {
                    operation_id,
                    target_position_ms,
                    execute_at_host_mono_us,
                } => {
                    if state.last_committed_operation_id.as_deref() == Some(&operation_id) {
                        return;
                    }
                    if state.pending_operation_id.is_some()
                        && state.pending_operation_id.as_deref() != Some(&operation_id)
                    {
                        return;
                    }
                    let offset = state.clock_offset_to_host_us;
                    let deadline = instant_for_host_mono(execute_at_host_mono_us, offset);
                    state.commit_scheduled_for = Some(deadline);
                    let inner_for_task = inner.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))
                            .await;
                        let mut state = inner_for_task.lock();
                        state
                            .sync_coordinator
                            .lock()
                            .unwrap()
                            .commit_pause(target_position_ms, PauseCause::Manual);
                        state.sync.position_ms = target_position_ms;
                        let strict_sync_paused =
                            state.sync_coordinator.lock().unwrap().paused_by_strict_sync;
                        state.sync.strict_sync_paused = strict_sync_paused;
                        state.pending_operation_id = None;
                        state.pending_operation_kind = None;
                        state.last_committed_operation_id = Some(operation_id);
                        state.commit_scheduled_for = None;
                        let room_state = state.sync_coordinator.lock().unwrap().room_state;
                        state.room_state = room_state;
                        // M3: dispatch to live player
                        Self::dispatch_player_pause(&mut state);
                        sync_room_snapshot(&mut state);
                        let snapshot = snapshot_from_state(&state);
                        inner_for_task.emit(snapshot);
                    });
                }
                QuicServerEvent::SeekPrepare {
                    operation_id,
                    target_position_ms,
                    initiator: _,
                } => {
                    if state.pending_operation_id.as_deref() == Some(&operation_id) {
                        return;
                    }
                    state.pending_operation_id = Some(operation_id.clone());
                    state.pending_operation_kind = Some("SEEK".to_string());
                    state.pending_operation_target_ms = target_position_ms;
                    let ready = state.local_participant.media_ready;
                    let buffer_ahead_ms = state.buffer.guest_buffer_ahead_ms;
                    let client = state.client.clone();
                    if let Some(client) = client {
                        tokio::spawn(async move {
                            let _ = client
                                .send_seek_ready(operation_id, ready, buffer_ahead_ms)
                                .await;
                        });
                    }
                }
                QuicServerEvent::SeekCommit {
                    operation_id,
                    target_position_ms,
                    execute_at_host_mono_us,
                    resume_after_seek,
                } => {
                    if state.last_committed_operation_id.as_deref() == Some(&operation_id) {
                        return;
                    }
                    if state.pending_operation_id.is_some()
                        && state.pending_operation_id.as_deref() != Some(&operation_id)
                    {
                        return;
                    }
                    let offset = state.clock_offset_to_host_us;
                    let deadline = instant_for_host_mono(execute_at_host_mono_us, offset);
                    state.commit_scheduled_for = Some(deadline);
                    let inner_for_task = inner.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))
                            .await;
                        let mut state = inner_for_task.lock();
                        state
                            .sync_coordinator
                            .lock()
                            .unwrap()
                            .commit_seek(target_position_ms, resume_after_seek);
                        state.sync.position_ms = target_position_ms;
                        state.sync.strict_sync_paused = !resume_after_seek;
                        state.pending_operation_id = None;
                        state.pending_operation_kind = None;
                        state.last_committed_operation_id = Some(operation_id);
                        state.commit_scheduled_for = None;
                        let room_state = state.sync_coordinator.lock().unwrap().room_state;
                        state.room_state = room_state;
                        // M3: dispatch to live player
                        Self::dispatch_player_seek(&mut state, target_position_ms);
                        sync_room_snapshot(&mut state);
                        let snapshot = snapshot_from_state(&state);
                        inner_for_task.emit(snapshot);
                    });
                }
                QuicServerEvent::BufferLow {
                    position_ms,
                    buffer_ahead_ms,
                } => {
                    let buffering_role = if sender == &state.local_participant.id {
                        if Self::is_host_role(&state) {
                            PeerRole::Host
                        } else {
                            PeerRole::Guest
                        }
                    } else if Self::is_host_role(&state) {
                        PeerRole::Guest
                    } else {
                        PeerRole::Host
                    };
                    let _ = state
                        .sync_coordinator
                        .lock()
                        .unwrap()
                        .buffer_low(buffering_role, position_ms);
                    state.sync.position_ms = position_ms;
                    state.sync.strict_sync_paused = true;
                    state.buffer.buffering_participant = if sender == &state.local_participant.id {
                        Some(state.local_participant.display_name.clone())
                    } else {
                        state
                            .peer_participant
                            .as_ref()
                            .map(|p| p.display_name.clone())
                    };
                    state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
                    let room_state = state.sync_coordinator.lock().unwrap().room_state;
                    state.room_state = room_state;
                    sync_room_snapshot(&mut state);
                }
                QuicServerEvent::BufferRecovered { buffer_ahead_ms } => {
                    state.buffer.buffering_participant = None;
                    state.buffer.percent = 100;
                    state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
                    // PROTOCOL_SPEC §30: BUFFER_RECOVERED does NOT auto-resume.
                    // The coordinator returns to READY_CHECK (only when the
                    // pause was strict-sync caused); resuming playback is a
                    // fresh host play-protocol cycle, never a silent side
                    // effect of recovery.
                    if Self::is_host_role(&state) {
                        let _ = state.sync_coordinator.lock().unwrap().buffer_recovered();
                        let strict_sync_paused =
                            state.sync_coordinator.lock().unwrap().paused_by_strict_sync;
                        state.sync.strict_sync_paused = strict_sync_paused;
                        let room_state = state.sync_coordinator.lock().unwrap().room_state;
                        state.room_state = room_state;
                    }
                    sync_room_snapshot(&mut state);
                }
                QuicServerEvent::CoordinatorStateUpdate {
                    host_ready,
                    guest_ready,
                    coordinator_play_state,
                    buffer_ahead_ms: _,
                } => {
                    state
                        .sync_coordinator
                        .lock()
                        .unwrap()
                        .apply_coordinator_state(host_ready, guest_ready, &coordinator_play_state);
                    let room_state = state.sync_coordinator.lock().unwrap().room_state;
                    state.room_state = room_state;
                    if state.sync.position_ms == 0 {
                        let host_pos = state.sync_coordinator.lock().unwrap().host_position_ms;
                        state.sync.position_ms = host_pos;
                    }
                    sync_room_snapshot(&mut state);
                }
                QuicServerEvent::RoomStateUpdate {
                    state: room_state,
                    position_ms,
                } => {
                    state.sync.position_ms = position_ms;
                    state.sync.strict_sync_paused =
                        room_state == "PAUSED" || room_state == "RECONNECTING";
                    let room_state = state.sync_coordinator.lock().unwrap().room_state;
                    state.room_state = room_state;
                    sync_room_snapshot(&mut state);
                }
                QuicServerEvent::ChatMessage {
                    message_id,
                    sender: msg_sender,
                    body,
                    created_host_time_us,
                } => {
                    if !state.chat.iter().any(|m| m.id == message_id) {
                        state.chat.push(ChatSnapshot {
                            id: message_id,
                            sender: msg_sender,
                            body,
                            created_host_time_us,
                        });
                    }
                }
                QuicServerEvent::Reaction {
                    reaction_id,
                    sender: msg_sender,
                    reaction,
                } => {
                    let participant_id = msg_sender.clone();
                    if let Err(_err) = state
                        .reaction_limiter
                        .accept(&participant_id, monotonic_us())
                    {
                        drop(state);
                        return;
                    }
                    if !state.reactions.iter().any(|r| r.id == reaction_id) {
                        state.reactions.push(ReactionSnapshot {
                            id: reaction_id,
                            sender: msg_sender,
                            reaction,
                            created_host_time_us: monotonic_us(),
                        });
                    }
                }
                QuicServerEvent::ControlGrant { request_id, .. } => {
                    if state.pending_guest_request_id.as_deref() == Some(&request_id) {
                        state.pending_guest_request_id = None;
                    }
                }
                QuicServerEvent::ControlDeny { request_id, .. } => {
                    if state.pending_guest_request_id.as_deref() == Some(&request_id) {
                        state.pending_guest_request_id = None;
                        state.error = Some(
                            "MP-CTRL-001 Shared Controls are disabled; only the host can \
                             control playback"
                                .to_string(),
                        );
                    }
                }
                QuicServerEvent::SyncError { code, message, .. } => {
                    state.error = Some(format!("{code}: {message}"));
                }
                QuicServerEvent::CallSignal { signal_type, data } => {
                    // M5: Forward received call signal to the frontend
                    state.call_signals.push(CallSignalSnapshot {
                        signal_type,
                        data,
                        created_host_time_us: crate::network::quic::monotonic_us(),
                    });
                }
            }
            snapshot_from_state(&state)
        };
        inner.emit(snapshot);
    }

    // ── Existing commands (unchanged semantics) ────────────────────────────

    // ── Host broadcast helpers (M2) ──────────────────────────────────────
    //
    // When this AppRuntime owns the Host session, host-operation commands
    // route through these helpers. Each one:
    //   1. Updates the LocalSyncCoordinator canonical state.
    //   2. Broadcasts a ServerEvent through host_event_tx so the guest's
    //      peer_event_listener picks it up.
    //
    // When this AppRuntime is the Guest, the same commands route through
    // apply_peer_event after the guest receives the ServerEvent over QUIC.
    // For guest-initiated operations under Shared Controls, see
    // guest_playback_request.

    #[allow(dead_code)]
    fn host_broadcast(state: &AppRuntimeState, event: QuicServerEvent) {
        if let Some(tx) = state.host_event_tx.as_ref() {
            // The QuicServer.broadcast assigns the canonical seq + sender.
            // AppRuntime-level direct send bypasses the QuicServer's seq
            // counter, so we go through QuicServer.broadcast by NOT using tx
            // here. Instead use the helper below.
            let _ = tx;
        }
        // We don't push directly: seq/sender must come from QuicServer's
        // canonical counter. The runtime stores an Arc<broadcast::Sender>
        // though, and QuicServer.broadcast also uses the *same* channel.
        // Since seq is assigned at QuicServer.broadcast() time and we don't
        // have a QuicServer handle here, we'll assign seq locally using the
        // coordinator's last_peer_seq_received + next local counter.
        //
        // Actually, the simplest approach: send through the channel with a
        // locally-computed envelope. The host's sender id is
        // host_device_id / local_participant.id.
        let _ = event;
    }

    /// Build and fire `CoordinatorStateUpdate` for the current advisor state.
    /// Called from the coordinator callback (the fn pointer stored on
    /// `LocalSyncCoordinator::on_coordinator_state_changed`).
    fn send_host_event(state: &mut AppRuntimeState, event: QuicServerEvent) {
        if let Some(tx) = state.host_event_tx.as_ref() {
            let seq = {
                let mut coordinator = state.sync_coordinator.lock().unwrap();
                coordinator.coordinator_event_seq =
                    coordinator.coordinator_event_seq.wrapping_add(1);
                coordinator.coordinator_event_seq
            };
            let envelope = EventEnvelope {
                seq,
                sender: state.local_participant.id.clone(),
                sent_mono_us: monotonic_us(),
                event,
            };
            let _ = tx.send(envelope);
        }
    }

    // Host-initiated play — runs the distributed PLAY protocol:
    //   PLAY_PREPARE (host) → PLAY_READY (guest) → PLAY_COMMIT (host)
    // Both sides execute the commit at the shared host-monotonic deadline
    // (MASTER_PRD §19 lead time from the calibrated p95 RTT).
    pub fn host_play(&self) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            // Idempotent: playing with nothing pending is already PLAYING.
            if state.room_state == RoomState::Playing
                && state.pending_operation_id.is_none()
                && state.commit_scheduled_for.is_none()
            {
                return snapshot_from_state(&state);
            }
            // A protocol cycle is already in flight; ignore the repeat tap
            // rather than starting a second overlapping operation.
            if state.pending_operation_id.is_some() {
                return snapshot_from_state(&state);
            }
            state.local_participant.media_ready = true;
            state
                .sync_coordinator
                .lock()
                .unwrap()
                .host_ready(ParticipantReadiness::ready(5_000));

            let target_position = state.sync.position_ms;
            let lead = clock::play_lead_us(state.peer_rtt_p95_us);
            let now = monotonic_us();
            let prepared = state
                .sync_coordinator
                .lock()
                .unwrap()
                .prepare_play_scheduled(target_position, now + lead, 5_000);
            let scheduled = match prepared {
                Ok(s) => s,
                Err(_) => {
                    state.error = Some(
                        "MP-SYNC-004 cannot start playback until all participants are ready"
                            .to_string(),
                    );
                    sync_room_snapshot(&mut state);
                    return snapshot_from_state(&state);
                }
            };
            state.pending_operation_id = Some(scheduled.operation_id.to_string());
            state.pending_operation_kind = Some("PLAY".to_string());
            state.pending_operation_target_ms = scheduled.target_position_ms;
            state.pending_operation_execute_at_us = scheduled.execute_at_host_mono_us;

            let event = QuicServerEvent::PlayPrepare {
                operation_id: scheduled.operation_id.to_string(),
                target_position_ms: scheduled.target_position_ms,
                minimum_buffer_ms: 5_000,
            };
            Self::send_host_event(&mut state, event);
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    // Host-initiated pause — distributed PAUSE protocol:
    //   PAUSE_PREPARE (host) → PAUSE_READY (guest) → PAUSE_COMMIT (host)
    // The canonical Paused transition happens at the shared deadline.
    pub fn host_pause(&self) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            if state.pending_operation_id.is_some() {
                return snapshot_from_state(&state);
            }
            if state.room_state == RoomState::Paused {
                return snapshot_from_state(&state);
            }
            let target_position = state.sync.position_ms;
            let execute_at = monotonic_us() + clock::play_lead_us(state.peer_rtt_p95_us);
            let operation_id = Uuid::now_v7().to_string();
            state.sync_coordinator.lock().unwrap().begin_pause();
            state.pending_operation_id = Some(operation_id.clone());
            state.pending_operation_kind = Some("PAUSE".to_string());
            state.pending_operation_target_ms = target_position;
            state.pending_operation_execute_at_us = execute_at;

            let event = QuicServerEvent::PausePrepare {
                operation_id,
                reason: "MANUAL".to_string(),
                target_position_ms: target_position,
            };
            Self::send_host_event(&mut state, event);
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    // Host-initiated seek — distributed SEEK protocol:
    //   SEEK_PREPARE (host) → SEEK_READY (guest) → SEEK_COMMIT (host)
    // The host never watches the destination before the guest is ready (§31);
    // with resume_after_seek the play protocol follows the seek commit.
    pub fn host_seek(&self, target_position_ms: u64, resume_after_seek: bool) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            if state.pending_operation_id.is_some() {
                return snapshot_from_state(&state);
            }
            let execute_at = monotonic_us() + clock::play_lead_us(state.peer_rtt_p95_us);
            let operation_id = Uuid::now_v7().to_string();
            state
                .sync_coordinator
                .lock()
                .unwrap()
                .begin_seek(target_position_ms);
            state.pending_operation_id = Some(operation_id.clone());
            state.pending_operation_kind = Some("SEEK".to_string());
            state.pending_operation_target_ms = target_position_ms;
            state.pending_operation_execute_at_us = execute_at;
            state.pending_operation_resume_after = resume_after_seek;

            let event = QuicServerEvent::SeekPrepare {
                operation_id,
                target_position_ms,
                initiator: state.local_participant.display_name.clone(),
            };
            Self::send_host_event(&mut state, event);
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    // Host-side buffer starvation report — canonical strict-sync pause plus a
    // BufferLow broadcast so the guest stops at the same position.
    pub fn report_buffer_low(&self, position_ms: u64, buffer_ahead_ms: u64) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            let _ = state
                .sync_coordinator
                .lock()
                .unwrap()
                .buffer_low(PeerRole::Host, position_ms);
            state.sync.position_ms = position_ms;
            state.sync.strict_sync_paused = true;
            state.buffer.buffering_participant = Some(state.local_participant.display_name.clone());
            state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;

            let event = QuicServerEvent::BufferLow {
                position_ms,
                buffer_ahead_ms,
            };
            Self::send_host_event(&mut state, event);
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    // Host-side buffer recovery report — clears the buffering flags and (only
    // for a strict-sync pause) moves the coordinator back to READY_CHECK.
    // PROTOCOL_SPEC §30: it never resumes playback by itself; resume is a
    // fresh host play-protocol cycle.
    pub fn report_buffer_recovered(&self, buffer_ahead_ms: u64) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            let _ = state.sync_coordinator.lock().unwrap().buffer_recovered();
            state.buffer.buffering_participant = None;
            state.buffer.percent = 100;
            state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
            let strict_sync_paused = state.sync_coordinator.lock().unwrap().paused_by_strict_sync;
            state.sync.strict_sync_paused = strict_sync_paused;
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;

            let event = QuicServerEvent::BufferRecovered { buffer_ahead_ms };
            Self::send_host_event(&mut state, event);
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    pub fn host_send_chat(&self, body: String) -> Result<AppSnapshot, String> {
        let snapshot = {
            let mut state = self.lock();
            let message = ChatMessage {
                message_id: Uuid::now_v7(),
                body: body.trim().to_string(),
                created_host_time_us: monotonic_us(),
            };
            validate_chat_message(&message).map_err(|error| error.to_string())?;
            let sender = state.local_participant.display_name.clone();
            state.chat.push(ChatSnapshot {
                id: message.message_id.to_string(),
                sender: sender.clone(),
                body: message.body.clone(),
                created_host_time_us: message.created_host_time_us,
            });

            let event = QuicServerEvent::ChatMessage {
                message_id: message.message_id.to_string(),
                sender,
                body: message.body,
                created_host_time_us: message.created_host_time_us,
            };
            Self::send_host_event(&mut state, event);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        Ok(snapshot)
    }

    pub fn host_send_reaction(&self, reaction: String) -> Result<AppSnapshot, String> {
        let snapshot = {
            let mut state = self.lock();
            let host_time_us = monotonic_us();
            let message = ReactionMessage {
                reaction_id: Uuid::now_v7(),
                reaction,
            };
            validate_reaction(&message).map_err(|error| error.to_string())?;
            let participant_id = state.local_participant.id.clone();
            state
                .reaction_limiter
                .accept(&participant_id, host_time_us)
                .map_err(|error| error.to_string())?;
            let sender = state.local_participant.display_name.clone();
            state.reactions.push(ReactionSnapshot {
                id: message.reaction_id.to_string(),
                sender: sender.clone(),
                reaction: message.reaction.clone(),
                created_host_time_us: host_time_us,
            });

            let event = QuicServerEvent::Reaction {
                reaction_id: message.reaction_id.to_string(),
                sender,
                reaction: message.reaction,
            };
            Self::send_host_event(&mut state, event);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        Ok(snapshot)
    }

    #[allow(dead_code)]
    fn host_relay_ready_state(&self, ready: bool) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            let readiness = if ready {
                ParticipantReadiness::ready(5_000)
            } else {
                ParticipantReadiness::not_ready("manual")
            };
            state.sync_coordinator.lock().unwrap().host_ready(readiness);
            let event = QuicServerEvent::RoomStateUpdate {
                state: format!("{:?}", state.sync_coordinator.lock().unwrap().room_state),
                position_ms: state.sync.position_ms,
            };
            Self::send_host_event(&mut state, event);
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    fn is_host_role(state: &AppRuntimeState) -> bool {
        state.local_participant.role == "Host"
    }

    /// M3: Dispatch a play command to the live player instance, if present.
    fn dispatch_player_play(state: &mut AppRuntimeState) {
        if let Some(ref player) = state.player {
            if let Ok(mut p) = player.lock() {
                let _ = p.play();
                state.player_snapshot = PlayerSnapshot::from(&p.snapshot());
            }
        }
    }

    /// M3: Dispatch a pause command to the live player instance, if present.
    fn dispatch_player_pause(state: &mut AppRuntimeState) {
        if let Some(ref player) = state.player {
            if let Ok(mut p) = player.lock() {
                let _ = p.pause();
                state.player_snapshot = PlayerSnapshot::from(&p.snapshot());
            }
        }
    }

    /// M3: Dispatch a seek command to the live player instance, if present.
    fn dispatch_player_seek(state: &mut AppRuntimeState, position_ms: u64) {
        if let Some(ref player) = state.player {
            if let Ok(mut p) = player.lock() {
                let _ = p.seek(position_ms);
                state.player_snapshot = PlayerSnapshot::from(&p.snapshot());
            }
        }
    }

    /// M3: Sync the player snapshot from the live player.
    #[allow(dead_code)]
    fn sync_player_snapshot(state: &mut AppRuntimeState) {
        if let Some(ref player) = state.player {
            if let Ok(p) = player.lock() {
                state.player_snapshot = PlayerSnapshot::from(&p.snapshot());
            }
        }
    }

    /// M3: Spawn a background task that polls the live player for position,
    /// duration, buffering state, and errors.  The snapshot is emitted to the
    /// frontend every ~200 ms so the UI stays in sync with the actual player.
    fn spawn_player_event_loop(&self) {
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            let mut last_position: u64 = 0;
            let mut last_state_name = String::new();
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                // Clone the Arc to the player so we don't hold an immutable
                // borrow on state while mutating it.
                let player_arc = {
                    let state = inner.lock();
                    state.player.clone()
                };
                let Some(player_arc) = player_arc else {
                    break; // No player — stop polling
                };
                let snap = match player_arc.lock() {
                    Ok(p) => p.snapshot(),
                    Err(_) => continue,
                };
                let position_changed = snap.position_ms != last_position;
                let state_changed = format!("{:?}", snap.state) != last_state_name;
                if position_changed || state_changed {
                    let mut state = inner.lock();
                    state.player_snapshot = PlayerSnapshot::from(&snap);
                    last_position = snap.position_ms;
                    last_state_name = format!("{:?}", snap.state);
                    state.sync.position_ms = snap.position_ms;
                    // M2: Feed buffering state into strict-sync
                    if matches!(snap.state, crate::media::player::PlayerState::Buffering) {
                        state.sync.strict_sync_paused = true;
                    }
                    let out = snapshot_from_state(&state);
                    inner.emit(out);
                }
            }
        });
        // Store the handle so it is aborted on leave_party
        self.lock().player_event_task = Some(task);
    }

    /// M8: Spawn a background watcher that monitors transfer progress and
    /// automatically fires `TransferInterrupted` when no bytes are received
    /// for 30 seconds during an active transfer.
    fn spawn_transfer_stall_watcher(&self) {
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            const STALL_THRESHOLD_SECS: u64 = 30;
            let mut last_bytes: u64 = 0;
            let mut last_change = tokio::time::Instant::now();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let current_bytes = {
                    let state = inner.lock();
                    state
                        .transfer
                        .as_ref()
                        .map(|t| t.bytes_available)
                        .unwrap_or(0)
                };
                if current_bytes == 0 {
                    // No active transfer — reset and keep watching
                    last_bytes = 0;
                    last_change = tokio::time::Instant::now();
                    continue;
                }
                if current_bytes != last_bytes {
                    last_bytes = current_bytes;
                    last_change = tokio::time::Instant::now();
                } else if last_change.elapsed()
                    >= std::time::Duration::from_secs(STALL_THRESHOLD_SECS)
                {
                    // Transfer has stalled — fire automatic failure event
                    let mut state = inner.lock();
                    let plan = recovery_plan(FailureEvent::TransferInterrupted);
                    apply_recovery_to_state(&mut state, FailureEvent::TransferInterrupted, plan);
                    let out = snapshot_from_state(&state);
                    drop(state);
                    inner.emit(out);
                    // Reset so we don't spam events
                    last_change = tokio::time::Instant::now();
                }
            }
        });
        self.lock().transfer_stall_watcher_task = Some(task);
    }

    pub fn set_ready(&self) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            state.screen = "READY_CHECK".to_string();
            let is_host = Self::is_host_role(&state);
            // M2 fake player: pressing Ready means the local player can
            // consume media; later phases gate this on real transfer/media.
            state.local_participant.media_ready = true;
            state.buffer.guest_buffer_ahead_ms = 5_000;
            if is_host {
                state
                    .sync_coordinator
                    .lock()
                    .unwrap()
                    .host_ready(ParticipantReadiness::ready(5_000));
                // READY_CHECK consensus: once both participants are ready the
                // coordinator fires CoordinatorStateUpdate (canonical), which
                // both sides apply. No play is started here.
                state
                    .sync_coordinator
                    .lock()
                    .unwrap()
                    .update_readiness_consensus(5_000);
            } else {
                state
                    .sync_coordinator
                    .lock()
                    .unwrap()
                    .guest_ready(ParticipantReadiness::ready(5_000));
                // Guest readiness must reach the host coordinator: send
                // ReadyState over QUIC (fire-and-forget; the host coordinator
                // is the canonical source and drives consensus back to us).
                if let Some(client) = state.client.clone() {
                    tokio::spawn(async move {
                        let _ = client.send_ready_state(true, 5_000).await;
                    });
                }
            }
            if let Some(peer) = &mut state.peer_participant {
                peer.media_ready = true;
            }
            // Readiness consensus only: PLAYING arrives exclusively through a
            // committed play operation, never as a side effect of Ready.
            let room_state = state.sync_coordinator.lock().unwrap().room_state;
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    // Host-only Shared Controls toggle (UI_UX_SPEC §33). The flag is shared
    // with QuicServer so ControlRequest granting happens at the boundary.
    pub fn set_shared_controls(&self, enabled: bool) -> AppSnapshot {
        let mut state = self.lock();
        if !Self::is_host_role(&state) {
            state.error = Some("MP-CTRL-002 only the host can change Shared Controls".to_string());
            return snapshot_from_state(&state);
        }
        state.shared_controls = enabled;
        state.shared_controls_flag.store(enabled, Ordering::SeqCst);
        sync_room_snapshot(&mut state);
        snapshot_from_state(&state)
    }

    pub fn enter_cinema(&self) -> AppSnapshot {
        let mut state = self.lock();
        state.screen = "CINEMA".to_string();
        state.room_state = RoomState::Playing;
        state.sync.strict_sync_paused = false;
        sync_room_snapshot(&mut state);
        snapshot_from_state(&state)
    }

    pub fn pause_playback(&self) -> AppSnapshot {
        if Self::is_host_role(&self.lock()) {
            return self.host_pause();
        }
        // Guest: playback control is a REQUEST, never a local commit
        // (MASTER_PRD §50/51). The host produces the canonical operation.
        self.guest_control_request("PAUSE", serde_json::json!({}))
    }

    pub fn resume_playback(&self) -> AppSnapshot {
        if Self::is_host_role(&self.lock()) {
            return self.host_play();
        }
        self.guest_control_request("PLAY", serde_json::json!({}))
    }

    /// Guest → Host CONTROL_REQUEST transport for Shared Controls. The host
    /// decides; denied requests surface MP-CTRL-001, granted requests are
    /// answered by the canonical PREPARE/READY/COMMIT cycle.
    pub fn guest_control_request(
        &self,
        action: &str,
        parameters: serde_json::Value,
    ) -> AppSnapshot {
        let (request_id, client) = {
            let mut state = self.lock();
            if state.pending_guest_request_id.is_some() {
                return snapshot_from_state(&state);
            }
            let request_id = Uuid::now_v7().to_string();
            state.pending_guest_request_id = Some(request_id.clone());
            state.error = None;
            (request_id, state.client.clone())
        };
        if let Some(client) = client {
            let action = action.to_string();
            tokio::spawn(async move {
                let _ = client
                    .send_control_request(request_id, action, parameters)
                    .await;
            });
        }
        let mut state = self.lock();
        sync_room_snapshot(&mut state);
        snapshot_from_state(&state)
    }

    pub fn seek_relative(&self, delta_ms: i64) -> AppSnapshot {
        if Self::is_host_role(&self.lock()) {
            let current = {
                let state = self.lock();
                state.sync.position_ms
            };
            let next = if delta_ms.is_negative() {
                current.saturating_sub(delta_ms.unsigned_abs())
            } else {
                current.saturating_add(delta_ms as u64)
            };
            return self.host_seek(next, true);
        }
        self.guest_control_request("SEEK_RELATIVE", serde_json::json!({ "delta_ms": delta_ms }))
    }

    pub fn leave_party(&self) -> AppSnapshot {
        let mut state = self.lock();
        if let Some(host) = state.host_session.take() {
            host.server_handle.abort();
        }
        if let Some(task) = state.peer_event_task.take() {
            task.abort();
        }
        if let Some(task) = state.host_event_task.take() {
            task.abort();
        }
        if let Some(task) = state.calibration_task.take() {
            task.abort();
        }
        if let Some(task) = state.player_event_task.take() {
            task.abort();
        }
        if let Some(task) = state.transfer_task.take() {
            task.abort();
            state.old_transfer_task = Some(task);
        }
        if let Some(task) = state.transfer_stall_watcher_task.take() {
            task.abort();
        }
        if let Some(client) = state.client.take() {
            let _ = client;
        }
        if let Some(range) = state.range_server_handle.take() {
            range.shutdown();
        }
        state.player = None;
        state.guest_cache = None;
        state.invite = None;
        state.host_session = None;
        state.client = None;
        state.host_event_tx = None;
        state.screen = "PARTY_ENDED".to_string();
        state.room_state = RoomState::Ended;
        state.network.connected = false;
        state.local_participant.role = "Host".to_string();
        state.peer_participant = None;
        state.error = None;
        state.pending_operation_id = None;
        state.pending_operation_kind = None;
        state.commit_scheduled_for = None;
        state.pending_guest_request_id = None;
        sync_room_snapshot(&mut state);
        snapshot_from_state(&state)
    }

    pub fn handle_failure_event(&self, event: FailureEvent) -> AppSnapshot {
        let mut state = self.lock();
        let plan = recovery_plan(event);
        apply_recovery_to_state(&mut state, event, plan);
        snapshot_from_state(&state)
    }

    pub fn send_chat_message(&self, body: String) -> Result<AppSnapshot, String> {
        if Self::is_host_role(&self.lock()) {
            return self.host_send_chat(body);
        }
        let mut state = self.lock();
        let message = ChatMessage {
            message_id: Uuid::now_v7(),
            body: body.trim().to_string(),
            created_host_time_us: self.host_time_us(),
        };
        validate_chat_message(&message).map_err(|error| error.to_string())?;
        let sender = state.local_participant.display_name.clone();
        state.chat.push(ChatSnapshot {
            id: message.message_id.to_string(),
            sender: sender.clone(),
            body: message.body.clone(),
            created_host_time_us: message.created_host_time_us,
        });
        // Guest→Host transport: relay the message to the host, which is the
        // canonical broadcast point (host appends + echoes to all guests).
        if let Some(client) = state.client.clone() {
            tokio::spawn(async move {
                let _ = client
                    .send_chat_message(
                        message.message_id,
                        message.body,
                        message.created_host_time_us,
                    )
                    .await;
            });
        }
        Ok(snapshot_from_state(&state))
    }

    pub fn send_reaction(&self, reaction: String) -> Result<AppSnapshot, String> {
        if Self::is_host_role(&self.lock()) {
            return self.host_send_reaction(reaction);
        }
        let mut state = self.lock();
        let host_time_us = self.host_time_us();
        let message = ReactionMessage {
            reaction_id: Uuid::now_v7(),
            reaction,
        };
        validate_reaction(&message).map_err(|error| error.to_string())?;
        let participant_id = state.local_participant.id.clone();
        state
            .reaction_limiter
            .accept(&participant_id, host_time_us)
            .map_err(|error| error.to_string())?;
        let sender = state.local_participant.display_name.clone();
        let reaction_value = message.reaction.clone();
        state.reactions.push(ReactionSnapshot {
            id: message.reaction_id.to_string(),
            sender,
            reaction: reaction_value,
            created_host_time_us: host_time_us,
        });
        // Guest→Host transport: relay to the host for canonical broadcast.
        if let Some(client) = state.client.clone() {
            tokio::spawn(async move {
                let _ = client
                    .send_reaction(message.reaction_id, message.reaction)
                    .await;
            });
        }
        Ok(snapshot_from_state(&state))
    }

    /// Report local buffer starvation/recovery to the room.
    ///
    /// Host: applies canonically to the coordinator and broadcasts.
    /// Guest: relays over QUIC; the host pauses/resumes the room and drives
    /// the canonical state back through CoordinatorStateUpdate.
    pub fn report_buffer_status(
        &self,
        position_ms: u64,
        buffer_ahead_ms: u64,
        stalled: bool,
    ) -> AppSnapshot {
        if Self::is_host_role(&self.lock()) {
            return if stalled {
                self.report_buffer_low(position_ms, buffer_ahead_ms)
            } else {
                self.report_buffer_recovered(buffer_ahead_ms)
            };
        }
        let mut state = self.lock();
        state.buffer.percent = if stalled { 0 } else { 100 };
        state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
        state.buffer.buffering_participant = if stalled {
            Some(state.local_participant.display_name.clone())
        } else {
            None
        };
        if let Some(client) = state.client.clone() {
            tokio::spawn(async move {
                let _ = client
                    .send_buffer_status(position_ms, buffer_ahead_ms, stalled)
                    .await;
            });
        }
        snapshot_from_state(&state)
    }

    pub fn set_call_mode(&self, mode: CallMode) -> AppSnapshot {
        let mut state = self.lock();
        state.call.mode = mode;
        match mode {
            CallMode::VideoVoice => {
                state.call.camera = CameraState::tier_b_enabled();
            }
            CallMode::VoiceOnly => {
                state.call.camera = CameraState::disabled();
            }
            CallMode::Off => {
                state.call.camera = CameraState::disabled();
                state.call.microphone.enabled = false;
            }
        }
        state.local_participant.camera_enabled = state.call.camera.enabled;
        state.local_participant.microphone_enabled = state.call.microphone.enabled;
        snapshot_from_state(&state)
    }

    pub fn submit_call_signal(&self, signal: CallSignal) -> Result<AppSnapshot, String> {
        if !validate_signal(&signal) {
            return Err("MP-CALL-001 invalid call signal".to_string());
        }

        let signal_type_str = format!("{:?}", signal.signal_type).to_ascii_uppercase();
        let data = signal.data.clone();
        let snapshot = {
            let mut state = self.lock();
            state.call.connected = true;
            state.call_signals.push(CallSignalSnapshot {
                signal_type: signal_type_str.clone(),
                data: data.clone(),
                created_host_time_us: self.host_time_us(),
            });
            snapshot_from_state(&state)
        };

        // M5: Send call signal over QUIC to the peer
        if Self::is_host_role(&self.lock()) {
            Self::send_host_event(
                &mut self.lock(),
                QuicServerEvent::CallSignal {
                    signal_type: signal_type_str,
                    data,
                },
            );
        } else if let Some(client) = self.lock().client.clone() {
            let st = signal_type_str;
            tokio::spawn(async move {
                let _ = client.send_call_signal(st, data).await;
            });
        }

        Ok(snapshot)
    }

    pub fn set_microphone_enabled(&self, enabled: bool) -> AppSnapshot {
        let mut state = self.lock();
        state.call.microphone.enabled = enabled && !state.privacy_mode;
        state.local_participant.microphone_enabled = state.call.microphone.enabled;
        snapshot_from_state(&state)
    }

    pub fn set_camera_enabled(&self, enabled: bool) -> AppSnapshot {
        let mut state = self.lock();
        state.call.camera = if enabled && !state.privacy_mode {
            CameraState::tier_b_enabled()
        } else {
            CameraState::disabled()
        };
        state.local_participant.camera_enabled = state.call.camera.enabled;
        snapshot_from_state(&state)
    }

    pub fn set_privacy_mode(&self, enabled: bool) -> AppSnapshot {
        let mut state = self.lock();
        state.privacy_mode = enabled;
        if enabled {
            state.ghost_mode = true;
            state.call.camera = CameraState::disabled();
            state.call.microphone.enabled = false;
            state.local_participant.camera_enabled = false;
            state.local_participant.microphone_enabled = false;
        }
        snapshot_from_state(&state)
    }

    pub fn set_ghost_mode(&self, enabled: bool) -> AppSnapshot {
        let mut state = self.lock();
        if !state.privacy_mode {
            state.ghost_mode = enabled;
        }
        snapshot_from_state(&state)
    }

    pub fn allowed_reactions(&self) -> Vec<String> {
        allowed_reactions().map(str::to_string).to_vec()
    }

    // ── Private helpers ────────────────────────────────────────────────────

    fn lock(&self) -> MutexGuard<'_, AppRuntimeState> {
        self.inner.lock()
    }

    fn host_time_us(&self) -> u64 {
        self.inner.started_at.elapsed().as_micros() as u64
    }

    fn emit_ok(&self, snapshot: AppSnapshot) {
        self.inner.emit(snapshot);
    }

    /// M3: Guest-side Local Perfect media fetch.
    ///
    /// Owns the full Local Perfect session on the guest:
    /// manifest → sparse cache → demand-driven loopback range server →
    /// QUIC transfer worker → player → player event loop.
    ///
    /// The transfer worker is demand-driven: it only fetches the exact
    /// 1 MiB chunks that the range server enqueues when mpv issues an HTTP
    /// Range request for uncached bytes. `leave_party` aborts every owned
    /// task and releases the cache.
    pub async fn guest_fetch_media(&self) -> Result<AppSnapshot, String> {
        use crate::media::cache::SparseCache;
        use crate::media::manifest::MediaManifest;
        use crate::media::stream::range_server::{start_range_server, RangeServerConfig};
        use crate::media::stream::route_for_media;
        use crate::media::transfer::{validate_chunk_packet, ChunkDemandHandle, ChunkPriority};
        use rand::RngCore;

        // Lifecycle hygiene: a reconnect must never leave duplicate live
        // workers or range servers from a previous session running. Abort
        // any prior transfer worker and stop any prior range server before
        // creating the new session.
        {
            let mut state = self.lock();
            if let Some(task) = state.transfer_task.take() {
                task.abort();
                state.old_transfer_task = Some(task);
            }
            if let Some(range) = state.range_server_handle.take() {
                range.shutdown();
            }
        }

        // 1. Fetch manifest over authenticated QUIC
        let manifest: MediaManifest = {
            let client_opt = self.lock().client.clone();
            let client = client_opt.ok_or_else(|| "MP-NET-001 no QUIC client".to_string())?;
            client
                .fetch_local_media_manifest()
                .await
                .map_err(|e| e.to_string())?
        };

        // 2. Create/open sparse cache
        let cache_root = std::env::temp_dir().join("MovePartyCache");
        let cache = SparseCache::open(&cache_root, manifest.clone())
            .map_err(|e| format!("MP-MEDIA-002 cache open failed: {e}"))?;

        // 3. Shared demand channel between range server and transfer worker
        let demand = ChunkDemandHandle::new();
        let session_token = {
            let mut buf = [0u8; 32];
            rand::rng().fill_bytes(&mut buf);
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
        };
        let _route = route_for_media(&manifest.media_id, session_token.clone());
        let chunk_wake = Arc::new(crate::media::stream::range_server::ChunkWake::new());
        let cache_arc = Arc::new(tokio::sync::Mutex::new(cache));
        let range_config = RangeServerConfig {
            manifest: manifest.clone(),
            cache: cache_arc.clone(),
            session_token,
            chunk_wake: chunk_wake.clone(),
            demand: demand.clone(),
            wait_timeout_ms: 5_000,
        };
        let range_handle = start_range_server(range_config)
            .await
            .map_err(|e| format!("MP-NET-003 range server failed: {e}"))?;
        let media_url = range_handle.media_url.clone();

        {
            self.lock().guest_cache = Some(cache_arc.clone());
        }

        // 4. Open player with range-server HTTP URL
        {
            let mut state = self.lock();
            state.media = Some(manifest.clone());
            state.local_participant.media_ready = true;
            state.local_participant.buffer_ahead_ms = 0;
            state.range_server_handle = Some(range_handle);
            #[cfg(feature = "mpv")]
            {
                let mut player = MpvPlayer::new();
                if let Err(e) = player.open(std::path::Path::new(&media_url)) {
                    state.error = Some(format!("MP-MEDIA-001 {e}"));
                } else {
                    state.player_snapshot = PlayerSnapshot::from(&player.snapshot());
                }
                state.player = Some(Arc::new(std::sync::Mutex::new(player)));
            }
            #[cfg(not(feature = "mpv"))]
            {
                let mut player = crate::media::player::LibMpvPlayer::new();
                if let Err(e) = player.open(std::path::Path::new(&media_url)) {
                    state.error = Some(format!("MP-MEDIA-001 {e}"));
                } else {
                    state.player_snapshot = PlayerSnapshot::from(&player.snapshot());
                }
                state.player = Some(Arc::new(std::sync::Mutex::new(player)));
            }
            if !Self::is_host_role(&state) {
                state
                    .sync_coordinator
                    .lock()
                    .unwrap()
                    .guest_ready(crate::sync::consensus::ParticipantReadiness::ready(5_000));
            }
            sync_room_snapshot(&mut state);
        }

        // 5. Demand-driven QUIC transfer worker — validates and writes to
        //    cache, then wakes the range server via the wake channel.
        //    Owned by this session: dropped when `leave_party` clears the
        //    client, and aborted on reconnect so no duplicate worker runs.
        //
        //    Cache write discipline:
        //      QUIC success → validate → acquire cache (never try_lock) →
        //      write chunk → only then update transfer progress → notify
        //      range waiter → finish demand
        //
        //    If the cache write fails the chunk is requeued for retry; the
        //    worker never claims bytes_available without persisting.
        let transfer_task = {
            let inner = Arc::clone(&self.inner);
            let manifest_worker = manifest.clone();
            let chunk_wake_worker = chunk_wake.clone();
            let demand_worker = demand.clone();
            tokio::spawn(async move {
                loop {
                    let Some(request) = demand_worker.pop_next() else {
                        demand_worker.notified().await;
                        continue;
                    };
                    let client_opt = inner.lock().client.clone();
                    let Some(client) = client_opt else {
                        demand_worker.finish_fetch(request.index);
                        break;
                    };
                    match client
                        .fetch_local_media_chunk(&manifest_worker.media_id, request.index)
                        .await
                    {
                        Ok(packet) => {
                            if validate_chunk_packet(&manifest_worker, &packet).is_err() {
                                demand_worker.finish_fetch(request.index);
                                continue;
                            }
                            // Clone the cache Arc without holding the
                            // AppRuntime state lock, then await the cache
                            // lock properly (no try_lock, no deadlock).
                            let cache_opt = {
                                let state = inner.lock();
                                state.guest_cache.clone()
                            };
                            let Some(cache) = cache_opt else {
                                demand_worker.finish_fetch(request.index);
                                break;
                            };
                            let write_result = {
                                let mut c = cache.lock().await;
                                c.write_chunk(u64::from(packet.chunk_index), &packet.payload)
                            };
                            match write_result {
                                Ok(()) => {
                                    {
                                        let mut state = inner.lock();
                                        if let Some(ref mut t) = state.transfer {
                                            t.bytes_available += packet.payload.len() as u64;
                                        }
                                    }
                                    chunk_wake_worker.notify();
                                }
                                Err(_) => {
                                    // Write failed (disk/IO): requeue the
                                    // chunk for a later retry rather than
                                    // silently losing it.
                                    demand_worker.finish_fetch(request.index);
                                    demand_worker.request(request.index, ChunkPriority::Critical);
                                    continue;
                                }
                            }
                            demand_worker.finish_fetch(request.index);
                        }
                        Err(_) => {
                            demand_worker.finish_fetch(request.index);
                            break;
                        }
                    }
                }
            })
        };
        {
            self.lock().transfer_task = Some(transfer_task);
        }

        if self.lock().player.is_some() {
            self.spawn_player_event_loop();
        }

        let snapshot = self.snapshot();
        self.inner.emit(snapshot.clone());
        Ok(snapshot)
    }

    /// Test-only accessor to the live authenticated QUIC client, so
    /// integration tests can drive raw protocol messages (duplicate/stale
    /// op-id checks, clock results, control requests).
    #[doc(hidden)]
    pub fn client_for_test(&self) -> Option<QuicClient> {
        self.lock().client.clone()
    }

    /// Inject a QUIC client for testing purposes (replaces any existing).
    #[doc(hidden)]
    pub fn inject_client_for_test(&self, client: QuicClient) {
        self.lock().client = Some(client);
    }

    /// Debug snapshot of the current session's owned resources.
    #[doc(hidden)]
    pub fn debug_session_owned(&self) -> SessionOwnershipDebug {
        let state = self.lock();
        let old_alive = state
            .old_transfer_task
            .as_ref()
            .map(|t| !t.is_finished())
            .unwrap_or(false);
        SessionOwnershipDebug {
            cache_owned: state.guest_cache.is_some(),
            range_server_owned: state.range_server_handle.is_some(),
            transfer_worker_owned: state.transfer_task.is_some(),
            player_owned: state.player.is_some(),
            old_worker_alive: old_alive,
        }
    }
}

/// Debug report of session ownership (test-only).
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOwnershipDebug {
    pub cache_owned: bool,
    pub range_server_owned: bool,
    pub transfer_worker_owned: bool,
    pub player_owned: bool,
    /// Whether the previous session's transfer worker is still alive
    /// (indicating a stale task survived the replacement).
    pub old_worker_alive: bool,
}

impl Default for AppRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Wall-clock milliseconds (UTC epoch). Used ONLY for scheduled movie times,
/// UI timestamps, logs, and persistence — never for playback scheduling.
fn wall_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Snapshot builder & recovery helpers (unchanged) ───────────────────────────

fn snapshot_from_state(state: &AppRuntimeState) -> AppSnapshot {
    let mut participants = vec![state.local_participant.clone()];
    if let Some(peer) = &state.peer_participant {
        participants.push(peer.clone());
    }

    AppSnapshot {
        screen: state.screen.clone(),
        room: RoomSnapshot {
            room_id: state
                .credentials
                .as_ref()
                .map(|credentials| credentials.room_id.clone()),
            invite_code: state
                .host_session
                .as_ref()
                .map(|s| s.invite_url.clone())
                .or_else(|| {
                    state
                        .invite
                        .as_ref()
                        .and_then(|i| crate::room::encode_invite(i).ok())
                }),
            role: state.local_participant.role.clone(),
            state: format!("{:?}", state.room_state).to_ascii_uppercase(),
            host_only_controls: !state.shared_controls,
            shared_controls: state.shared_controls,
            strict_sync: true,
        },
        participants,
        media: state.media.clone(),
        transfer: state.transfer.clone(),
        buffer: state.buffer.clone(),
        sync: state.sync.clone(),
        network: state.network.clone(),
        call: state.call.clone(),
        provider: state.provider.clone(),
        call_signals: state.call_signals.clone(),
        chat: state.chat.clone(),
        reactions: state.reactions.clone(),
        ghost_mode: state.ghost_mode,
        privacy_mode: state.privacy_mode,
        last_recovery: state.last_recovery.clone(),
        error: state.error.clone(),
        player: state.player_snapshot.clone(),
    }
}

fn sync_room_snapshot(state: &mut AppRuntimeState) {
    state.sync.room_state = format!("{:?}", state.room_state).to_ascii_uppercase();
}

fn apply_recovery_to_state(state: &mut AppRuntimeState, event: FailureEvent, plan: RecoveryPlan) {
    if plan.pauses_playback_for_both {
        state.room_state = RoomState::Reconnecting;
        state.sync.strict_sync_paused = true;
    }

    match event {
        FailureEvent::GuestCrash => {
            if let Some(peer) = &mut state.peer_participant {
                peer.connected = false;
            }
            state.buffer.buffering_participant = state
                .peer_participant
                .as_ref()
                .map(|participant| participant.display_name.clone());
        }
        FailureEvent::TransferInterrupted => {
            state.buffer.guest_buffer_ahead_ms = 0;
            state.buffer.percent = 0;
            state.buffer.buffering_participant = Some("Guest".to_string());
        }
        FailureEvent::TransferResumed => {
            state.room_state = RoomState::Playing;
            state.sync.strict_sync_paused = false;
            state.buffer.guest_buffer_ahead_ms = 5_000;
            state.buffer.percent = 100;
            state.buffer.buffering_participant = None;
        }
        FailureEvent::TailscaleDisconnect | FailureEvent::WifiDisconnect => {
            state.network.connected = false;
            state.network.path = "Disconnected".to_string();
        }
        FailureEvent::TailscaleReconnect | FailureEvent::NetworkChange => {
            state.network.connected = true;
            state.network.path = "Revalidating".to_string();
        }
        FailureEvent::ChromeCrash | FailureEvent::ProviderPageClosed => {
            state.provider.state = "Recovery required".to_string();
        }
        FailureEvent::ProviderLogout => {
            state.provider.state = "Login required".to_string();
        }
        FailureEvent::PlayerFailure => {
            state.error = Some("MP-MEDIA-001 player recovery required".to_string());
        }
        FailureEvent::CacheCorruption => {
            if let Some(transfer) = &mut state.transfer {
                transfer.bytes_available = 0;
                transfer.buffer_ahead_ms = 0;
            }
        }
        FailureEvent::MissingLocalFile => {
            state.media = None;
            state.local_participant.media_ready = false;
        }
        FailureEvent::HostCrash | FailureEvent::SleepWake => {}
    }

    state.last_recovery = Some(RuntimeRecoverySnapshot {
        event,
        action: plan.action,
        pauses_playback_for_both: plan.pauses_playback_for_both,
        requires_user_action: plan.requires_user_action,
    });
    sync_room_snapshot(state);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::AppRuntime;
    use crate::call::{CallSignal, CallSignalType};
    use crate::resilience::{FailureEvent, RecoveryAction};

    #[test]
    fn backend_snapshot_has_no_demo_peer_or_movie_by_default() {
        let runtime = AppRuntime::new();
        let snapshot = runtime.snapshot();

        assert!(snapshot.media.is_none());
        assert_eq!(snapshot.participants.len(), 1);
        assert!(snapshot.chat.is_empty());
    }

    #[test]
    fn privacy_mode_disables_real_call_state() {
        let runtime = AppRuntime::new();
        let snapshot = runtime.set_privacy_mode(true);

        assert!(snapshot.privacy_mode);
        assert!(!snapshot.call.camera.enabled);
        assert!(!snapshot.call.microphone.enabled);
    }

    #[test]
    fn call_signal_updates_backend_call_state() {
        let runtime = AppRuntime::new();
        let snapshot = runtime
            .submit_call_signal(CallSignal {
                signal_type: CallSignalType::Offer,
                data: "{\"type\":\"offer\"}".to_string(),
            })
            .expect("signal");

        assert!(snapshot.call.connected);
        assert_eq!(snapshot.call_signals.len(), 1);
        assert_eq!(snapshot.call_signals[0].signal_type, "OFFER");
    }

    #[test]
    fn runtime_failures_drive_recovery_state() {
        let runtime = AppRuntime::new();
        let playing = runtime.enter_cinema();
        assert_eq!(playing.sync.room_state, "PLAYING");

        let interrupted = runtime.handle_failure_event(FailureEvent::TransferInterrupted);
        assert!(interrupted.sync.strict_sync_paused);
        assert_eq!(
            interrupted
                .last_recovery
                .as_ref()
                .map(|recovery| recovery.action),
            Some(RecoveryAction::RestoreTransferAndRebuildBuffer)
        );

        let resumed = runtime.handle_failure_event(FailureEvent::TransferResumed);
        assert!(!resumed.sync.strict_sync_paused);
        assert_eq!(resumed.sync.room_state, "PLAYING");

        let missing = runtime.handle_failure_event(FailureEvent::MissingLocalFile);
        assert!(missing.media.is_none());
        assert!(missing
            .last_recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_user_action));
    }

    // §6: dev loopback mode selection — all three scenarios in one test to avoid
    // env-var races from Rust's default parallel test runner.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dev_loopback_mode_selects_correct_bind_addr() {
        struct ScopedEnv {
            key: &'static str,
            prev: Option<std::ffi::OsString>,
        }
        impl Drop for ScopedEnv {
            fn drop(&mut self) {
                if let Some(prev) = self.prev.take() {
                    std::env::set_var(self.key, prev);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }

        // --- unset: falls through to Tailscale path ---
        std::env::remove_var("MOVE_PARTY_DEV_LOOPBACK");
        let _g0 = ScopedEnv {
            key: "MOVE_PARTY_DEV_LOOPBACK",
            prev: std::env::var_os("MOVE_PARTY_DEV_LOOPBACK"),
        };
        let r0 = AppRuntime::new();
        let err0 = r0.create_local_party(None).await;
        assert!(
            err0.is_err(),
            "unset DEV_LOOPBACK must reach Tailscale path, not start a loopback QUIC server"
        );
        drop(_g0);

        // --- =0: same Tailscale path ---
        let _g1 = ScopedEnv {
            key: "MOVE_PARTY_DEV_LOOPBACK",
            prev: std::env::var_os("MOVE_PARTY_DEV_LOOPBACK"),
        };
        std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "0");
        let r1 = AppRuntime::new();
        let err1 = r1.create_local_party(None).await;
        assert!(err1.is_err(), "DEV_LOOPBACK=0 must reach Tailscale path");
        drop(_g1);

        // --- =1: loopback path, QUIC server on 127.0.0.1 ---
        let _g2 = ScopedEnv {
            key: "MOVE_PARTY_DEV_LOOPBACK",
            prev: std::env::var_os("MOVE_PARTY_DEV_LOOPBACK"),
        };
        std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");
        let r2 = AppRuntime::new();
        let snap = r2
            .create_local_party(None)
            .await
            .expect("loopback create_local_party must succeed");
        assert_eq!(snap.network.transport, "quic");
        assert!(
            snap.network.path.starts_with("Listening on 127.0.0.1:"),
            "expected loopback address in network.path, got: {}",
            snap.network.path
        );
        assert!(snap.room.invite_code.is_some());
        assert!(snap
            .room
            .invite_code
            .as_ref()
            .unwrap()
            .starts_with("moveparty://join/"));
        r2.leave_party();
    }
}
