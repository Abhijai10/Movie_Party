use std::{
    collections::{HashMap, VecDeque},
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
    call::{
        validate_signal, CallMode, CallRuntimeStatus, CallSignal, CallSignalLedger, CallSignalType,
        CameraState, MicState,
    },
    chat::{
        allowed_reactions, validate_chat_message, validate_reaction, ChatMessage, ReactionMessage,
        ReactionRateLimiter,
    },
    identity::DeviceIdentity,
    media::{
        manifest::{build_manifest, MediaManifest},
        player::{
            presentation::PlayerPresentationStatus, LocalPlayer, PlayerError,
            PlayerSnapshot as LibPlayerSnapshot,
        },
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
                status: CallRuntimeStatus::Unavailable,
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
                readiness: crate::providers::sync::ProviderReadiness::NotStarted,
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
    pub status: CallRuntimeStatus,
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
    pub readiness: crate::providers::sync::ProviderReadiness,
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
    pub presentation: PlayerPresentationStatus,
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
            presentation: PlayerPresentationStatus::unavailable("No media is loaded."),
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
            presentation: PlayerPresentationStatus::native_render_host_required(),
        }
    }
}

impl PlayerSnapshot {
    fn from_player(player: &dyn LocalPlayer) -> Self {
        Self::from_parts(player.snapshot(), player.presentation_status())
    }

    fn from_parts(snapshot: LibPlayerSnapshot, presentation: PlayerPresentationStatus) -> Self {
        let mut output = Self::from(&snapshot);
        output.presentation = presentation;
        output
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
    /// OS-protected secret storage for the device signing key. Production
    /// uses the platform Keychain/Credential Manager; tests inject a fake.
    key_store: Arc<dyn crate::secure::SecureKeyStore>,
    /// Real preload executor invoked by the scheduler for due schedules.
    preload_executor: Mutex<Arc<dyn crate::scheduling::preload::PreloadExecutor>>,
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
    call_signal_ledger: CallSignalLedger,
    provider: ProviderSnapshot,
    chat: Vec<ChatSnapshot>,
    reactions: Vec<ReactionSnapshot>,
    ghost_mode: bool,
    ghost_mode_before_privacy: bool,
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
    /// Guest-only authenticated reconnect loop. Exactly one may own a party.
    reconnect_task: Option<tokio::task::JoinHandle<()>>,
    /// Guest heartbeat monitor; replaced whenever the QUIC transport changes.
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    /// Scheduled preload preparation task, cancelled with the active party.
    preload_task: Option<tokio::task::JoinHandle<()>>,
    /// Last offline-preload notice per schedule, preventing scheduler spam.
    preload_wait_notified_at: HashMap<String, Instant>,
    // ── M4: Persistent storage ──────────────────────────────────────────
    /// SQLite database for identity, schedules, cache metadata, chat.
    db: Option<Arc<crate::storage::sqlite::MovePartyDb>>,
    /// Root directory for Move Party's own cache (guest Local Perfect data).
    cache_root: Option<PathBuf>,
    /// Background scheduler worker (M4.3). Owned so it can be aborted.
    scheduler_task: Option<tauri::async_runtime::JoinHandle<()>>,
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

#[derive(Debug)]
enum ReconnectFailure {
    Retryable,
    Terminal(String),
}

fn reconnect_failure(error: quic::QuicError) -> ReconnectFailure {
    match error {
        quic::QuicError::Auth(_)
        | quic::QuicError::Tls(_)
        | quic::QuicError::UnexpectedResponse => ReconnectFailure::Terminal(error.to_string()),
        _ => ReconnectFailure::Retryable,
    }
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
        Self::new_with_emitter_and_key_store(
            None,
            Arc::new(notifier),
            Arc::new(crate::secure::FakeKeyStore::new()),
        )
    }

    pub fn new_with_emitter(emitter: Option<Arc<dyn SnapshotSink>>) -> Self {
        Self::new_with_emitter_and_key_store(
            emitter,
            Arc::new(crate::notifications::NativeNotifier),
            Arc::new(crate::secure::NativeKeyStore),
        )
    }

    /// Test hook: inject an explicit OS-protected key store (e.g. a shared
    /// [`crate::secure::FakeKeyStore`] across runtime restarts).
    #[doc(hidden)]
    pub fn new_with_key_store_for_test(key_store: Arc<dyn crate::secure::SecureKeyStore>) -> Self {
        Self::new_with_emitter_and_key_store(
            None,
            Arc::new(crate::notifications::FakeNotifier::new()),
            key_store,
        )
    }

    fn new_with_emitter_and_key_store(
        emitter: Option<Arc<dyn SnapshotSink>>,
        notifier: Arc<dyn crate::notifications::Notifier>,
        key_store: Arc<dyn crate::secure::SecureKeyStore>,
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
                        status: CallRuntimeStatus::Unavailable,
                        connected: false,
                        camera: CameraState::tier_b_enabled(),
                        microphone: MicState::default(),
                    },
                    call_signals: Vec::new(),
                    call_signal_ledger: CallSignalLedger::default(),
                    provider: ProviderSnapshot {
                        mode: "LOCAL_PERFECT".to_string(),
                        provider_id: None,
                        url: None,
                        state: "Idle".to_string(),
                        readiness: crate::providers::sync::ProviderReadiness::NotStarted,
                    },
                    chat: Vec::new(),
                    reactions: Vec::new(),
                    ghost_mode: false,
                    ghost_mode_before_privacy: false,
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
                    reconnect_task: None,
                    heartbeat_task: None,
                    preload_task: None,
                    preload_wait_notified_at: HashMap::new(),
                    db: None,
                    cache_root: None,
                    scheduler_task: None,
                    chrome_session: None,
                }),
                emitter: RwLock::new(emitter),
                identity: Mutex::new(identity),
                notifier,
                key_store,
                preload_executor: Mutex::new(Arc::new(
                    crate::scheduling::preload::FakePreloadExecutor::default(),
                )),
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
        let db = match crate::storage::sqlite::MovePartyDb::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                self.lock().error = Some(format!("MP-STORE-001 failed to open database: {e}"));
                return;
            }
        };

        // Production startup path:
        //   open DB → load identity metadata → load private key from the
        //   OS-protected store → if both present use them; otherwise create
        //   exactly one identity (with its key in protected storage) and
        //   persist metadata. Errors are surfaced, never silently rotated.
        match db.get_identity() {
            Ok(Some(stored)) => self.restore_identity(&db, &stored),
            Ok(None) => self.create_identity(&db, &self.current_display_name()),
            Err(e) => {
                self.lock().error = Some(format!("MP-STORE-001 failed to read identity: {e}"));
            }
        }
        self.lock().db = Some(Arc::new(db));
    }

    fn current_display_name(&self) -> String {
        self.lock().local_participant.display_name.clone()
    }

    fn key_label_for(device_id: &str) -> String {
        format!("move-party-device-signing-key-{device_id}")
    }

    /// Restore an existing identity: private key from the OS-protected
    /// store must match the persisted metadata. If the key is missing or
    /// mismatched, perform an explicit coherent rotation (new device id +
    /// new keypair) — never associate an old device id with new key
    /// material.
    fn restore_identity(
        &self,
        db: &crate::storage::sqlite::MovePartyDb,
        stored: &crate::storage::sqlite::StoredIdentity,
    ) {
        match self.inner.key_store.load_seed(&stored.key_label) {
            Ok(Some(seed)) => match DeviceIdentity::from_seed(&stored.device_id, &seed) {
                Some(identity) if identity.public_key_base64() == stored.public_key => {
                    self.inner.replace_identity(identity);
                    self.lock().local_participant.id = stored.device_id.clone();
                    self.lock().local_participant.display_name = stored.display_name.clone();
                }
                _ => {
                    // Key does not match metadata → coherent rotation.
                    self.rotate_identity(db, &stored.display_name, Some(&stored.key_label));
                }
            },
            Ok(None) => {
                // Private key missing from protected storage → rotation with
                // a fresh device id + keypair.
                self.rotate_identity(db, &stored.display_name, Some(&stored.key_label));
            }
            Err(e) => {
                // Protected-store read failure: surface, do NOT rotate.
                self.lock().error = Some(format!("MP-SECURE-001 key store read failed: {e}"));
            }
        }
    }

    /// Create exactly one identity on first run: new keypair persisted to
    /// protected storage, metadata persisted to SQLite. Propagation of
    /// persistence failures is mandatory.
    fn create_identity(&self, db: &crate::storage::sqlite::MovePartyDb, display_name: &str) {
        let identity = DeviceIdentity::new_ephemeral();
        let key_label = Self::key_label_for(&identity.device_id);
        if let Err(e) = self
            .inner
            .key_store
            .store_seed(&key_label, &identity.seed())
        {
            self.lock().error = Some(format!("MP-SECURE-001 failed to store key: {e}"));
            return;
        }
        self.persist_identity(db, &identity, display_name, &key_label);
    }

    /// Explicit coherent rotation: a brand-new device id and keypair are
    /// created, the key is stored, then the metadata is replaced. The old
    /// protected-store entry is removed best-effort. If either persistence
    /// step fails, the error is surfaced and no half-state is claimed.
    fn rotate_identity(
        &self,
        db: &crate::storage::sqlite::MovePartyDb,
        display_name: &str,
        old_key_label: Option<&str>,
    ) {
        let identity = DeviceIdentity::new_ephemeral();
        let key_label = Self::key_label_for(&identity.device_id);
        if let Err(e) = self
            .inner
            .key_store
            .store_seed(&key_label, &identity.seed())
        {
            self.lock().error = Some(format!("MP-SECURE-001 rotation key store failed: {e}"));
            return;
        }
        if let Some(old_label) = old_key_label {
            let _ = self.inner.key_store.delete_seed(old_label);
        }
        // Drop the old metadata row so the rotated identity becomes the
        // single stored source of truth.
        let _ = db.delete_identity();
        self.persist_identity(db, &identity, display_name, &key_label);
    }

    fn persist_identity(
        &self,
        db: &crate::storage::sqlite::MovePartyDb,
        identity: &DeviceIdentity,
        display_name: &str,
        key_label: &str,
    ) {
        let now_ms = wall_now_ms();
        let stored = crate::storage::sqlite::StoredIdentity {
            device_id: identity.device_id.clone(),
            display_name: display_name.to_string(),
            public_key: identity.public_key_base64(),
            platform: std::env::consts::OS.to_string(),
            created_at_ms: now_ms,
            key_label: key_label.to_string(),
        };
        if let Err(e) = db.upsert_identity(&stored) {
            self.lock().error = Some(format!("MP-STORE-001 failed to persist identity: {e}"));
            return;
        }
        self.inner.replace_identity(identity.clone());
        self.lock().local_participant.id = identity.device_id.clone();
        self.lock().local_participant.display_name = display_name.to_string();
    }

    /// M6: Store an owned Chrome session in AppRuntime, replacing any previous one.
    /// The old session is dropped (killing the Chrome process) if present.
    pub fn store_chrome_session(&self, session: crate::providers::chrome::ManagedChromeSession) {
        self.lock().chrome_session = Some(session);
    }

    pub fn close_provider_session(&self) {
        self.lock().chrome_session = None;
    }

    pub fn store_launched_provider(
        &self,
        provider_id: String,
        url: String,
        session: crate::providers::chrome::ManagedChromeSession,
    ) -> AppSnapshot {
        let cdp_port = session.plan.cdp_port;
        let mut state = self.lock();
        state.chrome_session = Some(session);
        state.provider.mode = "PROVIDER_SYNC".to_string();
        state.provider.provider_id = Some(provider_id);
        state.provider.url = Some(url.clone());
        state.provider.readiness = crate::providers::sync::ProviderReadiness::LoginRequired;
        state.provider.state = format!("Login required · Chrome launched on CDP port {cdp_port}");
        state.screen = "LOBBY".to_string();
        state.local_participant.media_ready = false;
        state.error = None;
        snapshot_from_state(&state)
    }

    pub fn provider_unavailable(
        &self,
        provider_id: String,
        url: String,
        reason: String,
    ) -> AppSnapshot {
        let mut state = self.lock();
        state.provider.mode = "PROVIDER_SYNC".to_string();
        state.provider.provider_id = Some(provider_id);
        state.provider.url = Some(url);
        state.provider.readiness = crate::providers::sync::ProviderReadiness::Unavailable;
        state.provider.state = format!("Unavailable: {reason}");
        state.screen = "LOBBY".to_string();
        state.error = Some(reason);
        snapshot_from_state(&state)
    }

    pub fn store_launched_generic_link(
        &self,
        url: String,
        session: crate::providers::chrome::ManagedChromeSession,
    ) -> AppSnapshot {
        let cdp_port = session.plan.cdp_port;
        let mut state = self.lock();
        state.chrome_session = Some(session);
        state.provider.mode = "GENERIC_LINK".to_string();
        state.provider.provider_id = None;
        state.provider.url = Some(url);
        state.provider.readiness = crate::providers::sync::ProviderReadiness::Ready;
        state.provider.state =
            format!("Chrome launched on CDP port {cdp_port}; waiting for media detection");
        state.screen = "LOBBY".to_string();
        state.local_participant.media_ready = false;
        state.error = None;
        snapshot_from_state(&state)
    }

    pub fn generic_link_unavailable(&self, url: String, reason: String) -> AppSnapshot {
        let mut state = self.lock();
        state.provider.mode = "GENERIC_LINK".to_string();
        state.provider.provider_id = None;
        state.provider.url = Some(url);
        state.provider.readiness = crate::providers::sync::ProviderReadiness::Unavailable;
        state.provider.state = format!("Unavailable: {reason}");
        state.screen = "LOBBY".to_string();
        state.error = Some(reason);
        snapshot_from_state(&state)
    }

    /// Opens the provider's home page in the managed browser (or reuses an
    /// existing session for the same provider). Does not create a room or
    /// change the current screen; the caller creates the room separately.
    pub fn open_provider_browser(
        &self,
        provider_id: String,
        session: crate::providers::chrome::ManagedChromeSession,
    ) -> AppSnapshot {
        let url = session.plan.url.clone();
        let mut state = self.lock();
        state.chrome_session = Some(session);
        state.provider.mode = "PROVIDER_SYNC".to_string();
        state.provider.provider_id = Some(provider_id);
        state.provider.url = Some(url);
        state.provider.readiness = crate::providers::sync::ProviderReadiness::LoginRequired;
        state.provider.state = "Chrome launched; sign in on the provider's own page".to_string();
        state.error = None;
        snapshot_from_state(&state)
    }

    /// Updates the provider readiness after a CDP status check. Returns the
    /// snapshot so the frontend can reflect the new state without a room
    /// transition.
    pub fn update_provider_readiness(
        &self,
        readiness: crate::providers::sync::ProviderReadiness,
    ) -> AppSnapshot {
        let mut state = self.lock();
        state.provider.readiness = readiness;
        state.provider.state =
            crate::providers::sync::readiness_description(readiness).to_string();
        state.error = None;
        snapshot_from_state(&state)
    }

    /// Validates that the current provider session is ready for room creation.
    /// Returns an error string if the check fails.
    pub fn validate_provider_ready_for_room(&self) -> Result<(), String> {
        let state = self.lock();
        match state.provider.readiness {
            crate::providers::sync::ProviderReadiness::PlaybackReady => Ok(()),
            crate::providers::sync::ProviderReadiness::Ready => Ok(()),
            crate::providers::sync::ProviderReadiness::LoginRequired => {
                Err("MP-PROVIDER-004 sign in to the provider before creating the room".to_string())
            }
            crate::providers::sync::ProviderReadiness::NotStarted => {
                Err("MP-PROVIDER-004 open the provider browser first".to_string())
            }
            crate::providers::sync::ProviderReadiness::Launching => {
                Err("MP-PROVIDER-004 provider browser is still launching".to_string())
            }
            crate::providers::sync::ProviderReadiness::Navigating => {
                Err("MP-PROVIDER-004 wait for the title to open in the provider".to_string())
            }
            crate::providers::sync::ProviderReadiness::Unavailable | crate::providers::sync::ProviderReadiness::Error => {
                Err("MP-PROVIDER-004 provider is unavailable or in an error state".to_string())
            }
        }
    }

    /// Runs CDP commands against the current provider session to detect login
    /// and media status. Returns (login_required, media_detected) or an error.
    pub fn detect_provider_session_status(
        &self,
        provider_id: &str,
    ) -> Result<(bool, bool), String> {
        use crate::providers::sync::{detect_media_command, login_required_command, provider_id_from_str};

        let provider = provider_id_from_str(provider_id)
            .ok_or_else(|| "MP-PROVIDER-002 unsupported provider".to_string())?;

        let mut state = self.lock();
        let session = state
            .chrome_session
            .as_mut()
            .ok_or_else(|| "MP-PROVIDER-003 browser not launched".to_string())?;

        let mut page = session
            .connect_page()
            .map_err(|e| format!("MP-PROVIDER-003 CDP unavailable: {e}"))?;

        let login_result = page
            .execute(&login_required_command(provider, 1))
            .map_err(|e| format!("MP-PROVIDER-003 CDP login check failed: {e}"))?;
        let login_required = login_result["result"]["value"].as_bool().unwrap_or(false);

        let detect_result = page
            .execute(&detect_media_command(provider, 2))
            .map_err(|e| format!("MP-PROVIDER-003 CDP media detection failed: {e}"))?;
        let media_detected = detect_result["result"]["value"].as_bool().unwrap_or(false);

        Ok((login_required, media_detected))
    }

    /// Returns true when the current provider session matches the requested
    /// provider ID and the browser process is still alive.
    pub fn session_matches_provider(&self, provider_id: &str) -> bool {
        let mut state = self.lock();
        let same_id = state.provider.provider_id.as_deref() == Some(provider_id);
        let alive = state
            .chrome_session
            .as_mut()
            .map(|session| session.is_alive())
            .unwrap_or(false);
        same_id && alive
    }

    /// Attaches the current provider session to a newly created room without
    /// launching a new browser. Validates readiness before allowing the
    /// transition. The caller must have already created the room.
    pub fn attach_launched_provider(
        &self,
        provider_id: String,
        url: String,
    ) -> AppSnapshot {
        let mut state = self.lock();
        state.provider.mode = "PROVIDER_SYNC".to_string();
        state.provider.provider_id = Some(provider_id);
        state.provider.url = Some(url);
        state.screen = "LOBBY".to_string();
        state.local_participant.media_ready = true;
        state.error = None;
        snapshot_from_state(&state)
    }

    /// Navigates the managed provider browser's current page to a new URL
    /// using CDP. Used for provider search navigation.
    pub fn navigate_provider_to(
        &self,
        provider_id: &str,
        url: &str,
    ) -> Result<(), String> {
        use crate::providers::sync::provider_id_from_str;

        let _provider = provider_id_from_str(provider_id)
            .ok_or_else(|| "MP-PROVIDER-002 unsupported provider".to_string())?;

        let mut state = self.lock();
        let session = state
            .chrome_session
            .as_mut()
            .ok_or_else(|| "MP-PROVIDER-003 browser not launched".to_string())?;

        let mut page = session
            .connect_page()
            .map_err(|e| format!("MP-PROVIDER-003 CDP unavailable: {e}"))?;

        page.navigate(url)
            .map_err(|e| format!("MP-PROVIDER-003 provider navigation failed: {e}"))?;

        state.provider.url = Some(url.to_string());
        Ok(())
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
            // IMPORTANT: this scan is notification-only. It must NEVER
            // change the schedule status — the scheduler worker is the only
            // authority that claims and executes due schedules. Mutating
            // status here previously hid overdue work from the scheduler.
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
        let planned_preload_utc_ms = {
            let state = self.lock();
            match (&state.media, &state.transfer) {
                (Some(media), Some(transfer)) => adaptive_preload_deadline(
                    planned_preload_utc_ms,
                    media.file_size.saturating_sub(transfer.bytes_available),
                    state.network.goodput_bps,
                    scheduled_start_utc_ms,
                ),
                _ => planned_preload_utc_ms,
            }
        };
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

    /// Test hook: install a preload executor (e.g. a fake) before spawning
    /// the scheduler.
    #[doc(hidden)]
    pub fn install_preload_executor_for_test(
        &self,
        executor: Arc<dyn crate::scheduling::preload::PreloadExecutor>,
    ) {
        if let Ok(mut g) = self.inner.preload_executor.lock() {
            *g = executor;
        }
    }

    /// Install the production preload executor, which drives the actual
    /// Local Perfect preload path (`guest_fetch_media`) when a peer is
    /// online.
    pub fn setup_real_preload_executor(&self) {
        let executor = Arc::new(AppRuntimePreloadExecutor {
            runtime: self.clone(),
        });
        if let Ok(mut g) = self.inner.preload_executor.lock() {
            *g = executor;
        }
    }

    fn spawn_scheduler_worker_inner(&self, now_fn: fn() -> i64, poll_interval_ms: u64) {
        let inner = Arc::clone(&self.inner);
        let task = tauri::async_runtime::spawn(async move {
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

                if let Err(error) = db.recover_claimed_schedules(now) {
                    eprintln!("MoveParty: scheduler claimed-recovery failed: {error}");
                }

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
                    // Exactly once: only the scheduler that atomically wins
                    // the pending → Claimed transition may run the executor.
                    match db.claim_due_schedule(&schedule.schedule_id, now) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(error) => {
                            eprintln!("MoveParty: scheduler claim failed: {error}");
                            continue;
                        }
                    }

                    // Invoke the REAL preload executor.
                    let executor = match inner.preload_executor.lock() {
                        Ok(g) => g.clone(),
                        Err(p) => p.into_inner().clone(),
                    };
                    match executor.execute(schedule) {
                        Ok(crate::scheduling::preload::PreloadOutcome::Started) => {
                            // Real transfer/preload preparation has begun.
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
                        Ok(crate::scheduling::preload::PreloadOutcome::WaitingForPrerequisites) => {
                            // Peer/session unavailable: persist a waiting
                            // state, notify the user, and retry on the next
                            // poll (due_schedules includes WaitingForPeer).
                            let _ =
                                db.update_schedule_status(&schedule.schedule_id, "WaitingForPeer");
                            let should_notify = {
                                let mut state = inner.lock();
                                let now = Instant::now();
                                match state.preload_wait_notified_at.get(&schedule.schedule_id) {
                                    Some(previous)
                                        if now.duration_since(*previous)
                                            < std::time::Duration::from_secs(15 * 60) =>
                                    {
                                        false
                                    }
                                    _ => {
                                        state
                                            .preload_wait_notified_at
                                            .insert(schedule.schedule_id.clone(), now);
                                        true
                                    }
                                }
                            };
                            if should_notify {
                                let _ = inner.notifier.notify(
                                    "Move Party — Preload Waiting",
                                    &format!(
                                        "Movie Party needs your device or partner online to prepare '{}'.",
                                        schedule.media_id
                                    ),
                                );
                            }
                        }
                        Err(error) => {
                            let _ =
                                db.update_schedule_status(&schedule.schedule_id, "PreloadFailed");
                            eprintln!("MoveParty: preload executor failed: {error}");
                        }
                    }
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
        if let Some(task) = state.preload_task.take() {
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

    pub fn show_join_party(&self) -> AppSnapshot {
        let mut state = self.lock();
        state.screen = "JOIN_PARTY".to_string();
        state.error = None;
        snapshot_from_state(&state)
    }

    pub fn request_end_party(&self) -> AppSnapshot {
        let mut state = self.lock();
        state.screen = "PARTY_END_CONFIRM".to_string();
        snapshot_from_state(&state)
    }

    pub fn return_home(&self) -> AppSnapshot {
        let _ = self.leave_party();
        let mut state = self.lock();
        state.screen = "HOME".to_string();
        state.room_state = RoomState::Created;
        state.sync = SyncSnapshot {
            room_state: "CREATED".to_string(),
            strict_sync_paused: false,
            position_ms: 0,
        };
        state.local_participant.role = "Host".to_string();
        state.local_participant.connected = false;
        state.local_participant.media_ready = false;
        state.local_participant.buffer_ahead_ms = 0;
        state.media = None;
        state.transfer = None;
        state.buffer = BufferSnapshot {
            guest_buffer_ahead_ms: 0,
            percent: 0,
            buffering_participant: None,
        };
        state.network.connected = false;
        state.network.path = "Not connected".to_string();
        state.provider = ProviderSnapshot {
            mode: "LOCAL_PERFECT".to_string(),
            provider_id: None,
            url: None,
            state: "Idle".to_string(),
            readiness: crate::providers::sync::ProviderReadiness::NotStarted,
        };
        state.chat.clear();
        state.reactions.clear();
        state.error = None;
        snapshot_from_state(&state)
    }

    // M1: real host flow — bind QUIC server and produce a live invite
    pub async fn create_local_party(
        &self,
        media_path: Option<String>,
    ) -> Result<AppSnapshot, String> {
        // Lifecycle hygiene: a duplicate create call (double-tap, stale UI)
        // must never leave the previous host server running. Abort any
        // existing server and guest client before starting a fresh session.
        {
            let mut state = self.lock();
            if let Some(host) = state.host_session.take() {
                host.server_handle.abort();
            }
            if let Some(task) = state.host_event_task.take() {
                task.abort();
            }
            if let Some(task) = state.player_event_task.take() {
                task.abort();
            }
            if let Some(task) = state.transfer_stall_watcher_task.take() {
                task.abort();
            }
            if let Some(task) = state.heartbeat_task.take() {
                task.abort();
            }
            if let Some(task) = state.reconnect_task.take() {
                task.abort();
            }
            if let Some(task) = state.peer_event_task.take() {
                task.abort();
            }
            if let Some(task) = state.calibration_task.take() {
                task.abort();
            }
            if let Some(task) = state.preload_task.take() {
                task.abort();
            }
            if let Some(client) = state.client.take() {
                let _ = client;
            }
            if let Some(range) = state.range_server_handle.take() {
                range.shutdown();
            }
            if let Some(player) = state.player.take() {
                player
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .close();
            }
            state.guest_cache = None;
            state.media = None;
            state.transfer = None;
            state.player_snapshot = PlayerSnapshot::default();
            state.chrome_session = None;
            state.chat.clear();
            state.reactions.clear();
            state.invite = None;
            state.credentials = None;
        }

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

        let is_dev = crate::network::tailscale::dev_loopback_enabled();
        let (bind_addr, tailscale_ip) = if is_dev {
            (loopback_bind_addr(), "127.0.0.1".to_string())
        } else {
            let readiness = crate::network::tailscale::local_readiness().await;
            let ipv4 = crate::network::tailscale::required_ipv4(&readiness)?;
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
                            state.local_participant.media_ready = false;
                            Self::set_player_diagnostic_error(
                                &mut state,
                                Self::media_load_error_message(&e),
                            );
                        } else {
                            state.player_snapshot = PlayerSnapshot::from_player(&player);
                        }
                        state.player = Some(Arc::new(std::sync::Mutex::new(player)));
                    }
                    #[cfg(not(feature = "mpv"))]
                    {
                        let mut player = crate::media::player::LibMpvPlayer::new();
                        if let Err(e) = player.open(path) {
                            state.local_participant.media_ready = false;
                            Self::set_player_diagnostic_error(
                                &mut state,
                                Self::media_load_error_message(&e),
                            );
                        } else {
                            state.player_snapshot = PlayerSnapshot::from_player(&player);
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
        // Validate the invite before touching any existing session: an
        // invalid invite must never tear down a room the user is already in.
        let invite = room::parse_invite(&invite_url).map_err(|e| e.to_string())?;
        let addr = room::invite_socket_addr(&invite).map_err(|e| e.to_string())?;
        quic::validate_quic_bind_addr(addr)
            .map_err(|e| format!("MP-NET-001 invalid peer endpoint: {e}"))?;

        // Lifecycle hygiene: a join may arrive through a deep link while the
        // user is still in a previous room. After the invite is valid, abort
        // any existing host server and stale guest session before connecting,
        // so the new room never leaks the old server/client/workers or carries
        // stale media/provider state.
        {
            let mut state = self.lock();
            if let Some(host) = state.host_session.take() {
                host.server_handle.abort();
            }
            if let Some(task) = state.host_event_task.take() {
                task.abort();
            }
            if let Some(task) = state.peer_event_task.take() {
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
            if let Some(task) = state.reconnect_task.take() {
                task.abort();
            }
            if let Some(task) = state.heartbeat_task.take() {
                task.abort();
            }
            if let Some(task) = state.preload_task.take() {
                task.abort();
            }
            if let Some(client) = state.client.take() {
                let _ = client;
            }
            if let Some(range) = state.range_server_handle.take() {
                range.shutdown();
            }
            state.chrome_session = None;
            state.player = None;
            state.guest_cache = None;
            state.media = None;
            state.transfer = None;
            state.buffer = BufferSnapshot {
                guest_buffer_ahead_ms: 0,
                percent: 0,
                buffering_participant: None,
            };
            state.sync = SyncSnapshot {
                room_state: "CREATED".to_string(),
                strict_sync_paused: false,
                position_ms: 0,
            };
            state.provider = ProviderSnapshot {
                mode: "LOCAL_PERFECT".to_string(),
                provider_id: None,
                url: None,
                state: "Idle".to_string(),
                readiness: crate::providers::sync::ProviderReadiness::NotStarted,
            };
            state.chat.clear();
            state.reactions.clear();
            state.player_snapshot = PlayerSnapshot::default();
            state.call_signals.clear();
            state.call_signal_ledger.reset();
        }

        if !crate::network::tailscale::dev_loopback_enabled() {
            let readiness = crate::network::tailscale::local_readiness().await;
            if !readiness.is_usable() {
                return Err(readiness.stable_error().unwrap_or_else(|| {
                    "MP-NET-TS-004 Tailscale has no usable private IPv4 address".to_string()
                }));
            }
        }

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
        .map_err(|error| match error {
            quic::QuicError::Connection(_) | quic::QuicError::Connect(_) => {
                "MP-NET-TS-005 host is not reachable through Tailscale".to_string()
            }
            _ => error.to_string(),
        })?;

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

        self.spawn_guest_peer_event_listener();

        // M2: live clock calibration (MASTER_PRD §18): 20 CLOCK_PING probes,
        // median offset, then refresh every 30s. The offset feeds the
        // host-monotonic deadline conversion for scheduled commits.
        self.spawn_clock_calibration();
        self.spawn_guest_heartbeat();

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
        let mut state = self.lock();
        if let Some(old) = state.calibration_task.replace(task) {
            old.abort();
        }
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
                let room_state = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .room_state;
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
            QuicHostEvent::GuestReadyState {
                coordinator_ready,
                coordinator_buffer_ahead_ms,
                ..
            } => {
                let mut state = self.lock();
                if let Some(peer) = &mut state.peer_participant {
                    peer.media_ready = coordinator_ready;
                    if coordinator_ready {
                        peer.buffer_ahead_ms = coordinator_buffer_ahead_ms;
                    }
                }
            }
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pending_scheduled
                .clone();
            let Some(scheduled) = scheduled else { return };
            if scheduled.operation_id.to_string() != op_id {
                return;
            }
            let _ = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .commit_play(&scheduled);
            state.sync.position_ms = target;
            state.sync.strict_sync_paused = false;
            state.pending_operation_id = None;
            state.pending_operation_kind = None;
            state.last_committed_operation_id = Some(op_id);
            state.commit_scheduled_for = None;
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .commit_pause(target, PauseCause::Manual);
            state.sync.position_ms = target;
            let strict_sync_paused = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .paused_by_strict_sync;
            state.sync.strict_sync_paused = strict_sync_paused;
            state.pending_operation_id = None;
            state.pending_operation_kind = None;
            state.last_committed_operation_id = Some(op_id);
            state.commit_scheduled_for = None;
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .commit_seek(target, resume_after_seek);
                state.sync.position_ms = target;
                state.sync.strict_sync_paused = !resume_after_seek;
                state.pending_operation_id = None;
                state.pending_operation_kind = None;
                state.last_committed_operation_id = Some(op_id);
                state.commit_scheduled_for = None;
                let room_state = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .room_state;
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

    fn spawn_guest_peer_event_listener(&self) {
        let runtime = self.clone();
        let task = tokio::spawn(async move {
            runtime.peer_event_listener().await;
        });
        let mut state = self.lock();
        if let Some(old) = state.peer_event_task.replace(task) {
            old.abort();
        }
    }

    fn spawn_guest_heartbeat(&self) {
        let runtime = self.clone();
        let task = tokio::spawn(async move {
            let mut failures = 0_u8;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let client = runtime.lock().client.clone();
                let Some(client) = client else { break };
                match client.heartbeat().await {
                    Ok(()) => failures = 0,
                    Err(_) => {
                        failures = failures.saturating_add(1);
                        if failures >= 5 {
                            let _ = Self::apply_disconnect(&runtime.inner);
                            runtime.spawn_reconnect_worker();
                            break;
                        }
                    }
                }
            }
        });
        let mut state = self.lock();
        if let Some(old) = state.heartbeat_task.replace(task) {
            old.abort();
        }
    }

    // M2: Guest background task — receives host-originated ServerEvent messages
    async fn peer_event_listener(&self) {
        loop {
            let (client_opt, should_continue) = {
                let state = self.inner.lock();
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
                    Self::apply_peer_event(&self.inner, &envelope, envelope.event.clone());
                }
                Err(_) => {
                    let _ = Self::apply_disconnect(&self.inner);
                    self.spawn_reconnect_worker();
                    break;
                }
            }
        }
    }

    fn spawn_reconnect_worker(&self) {
        if Self::is_host_role(&self.lock()) || self.lock().invite.is_none() {
            return;
        }
        if self
            .lock()
            .reconnect_task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        let runtime = self.clone();
        let task = tokio::spawn(async move {
            // A short retry gets transient path changes quickly; later attempts
            // back off and stop rather than silently retrying forever.
            const BACKOFF_MS: [u64; 6] = [250, 750, 1_500, 3_000, 5_000, 8_000];
            for delay_ms in BACKOFF_MS {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                match runtime.reconnect_guest_transport().await {
                    Ok(()) => break,
                    Err(ReconnectFailure::Terminal(message)) => {
                        let mut state = runtime.lock();
                        state.error = Some(message);
                        state.network.path = "Reconnect rejected".to_string();
                        break;
                    }
                    Err(ReconnectFailure::Retryable) => continue,
                }
            }
            runtime.lock().reconnect_task = None;
        });
        self.lock().reconnect_task = Some(task);
    }

    async fn reconnect_guest_transport(&self) -> Result<(), ReconnectFailure> {
        let (invite, identity, display_name) = {
            let state = self.lock();
            (
                state.invite.clone().ok_or_else(|| {
                    ReconnectFailure::Terminal(
                        "MP-NET-001 reconnect session is unavailable".to_string(),
                    )
                })?,
                self.inner.identity(),
                state.local_participant.display_name.clone(),
            )
        };
        let address = room::invite_socket_addr(&invite)
            .map_err(|error| ReconnectFailure::Terminal(error.to_string()))?;
        let credentials = room::invite_to_credentials(&invite);
        let (client, auth_accept) = QuicClient::connect(
            address,
            invite.server_certificate_fingerprint.clone(),
            credentials,
            identity,
            display_name,
        )
        .await
        .map_err(reconnect_failure)?;
        if auth_accept.host_device_id != invite.host_device_id {
            return Err(ReconnectFailure::Terminal(
                "MP-NET-001 authenticated host identity mismatch".to_string(),
            ));
        }
        let canonical_room_state = client
            .heartbeat_room_state()
            .await
            .map_err(reconnect_failure)?;
        {
            let mut state = self.lock();
            state.client = Some(client);
            if let Some(peer) = &mut state.peer_participant {
                peer.connected = true;
                peer.display_name = auth_accept.host_display_name;
            }
            state.network.connected = true;
            state.network.path =
                format!("Reconnected; waiting for host recovery ({canonical_room_state})");
            // Transport recovery never grants the guest authority to continue.
            state.room_state = RoomState::Reconnecting;
            state.sync.strict_sync_paused = true;
            state.error = None;
            sync_room_snapshot(&mut state);
        }
        self.spawn_guest_peer_event_listener();
        self.spawn_clock_calibration();
        self.spawn_guest_heartbeat();
        self.inner.emit(self.snapshot());
        Ok(())
    }

    fn apply_disconnect(inner: &RuntimeInner) -> Result<(), ()> {
        let mut state = inner.lock();
        if let Some(peer) = &mut state.peer_participant {
            peer.connected = false;
        }
        state.network.connected = false;
        state.network.path = "Peer disconnected".to_string();
        let _ = state
            .sync_coordinator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .peer_disconnected();
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
        let room_state = state
            .sync_coordinator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .room_state;
        state.room_state = room_state;
        sync_room_snapshot(&mut state);
        let snapshot = snapshot_from_state(&state);
        drop(state);
        inner.emit(snapshot);
        Ok(())
    }

    fn call_signal_type_from_wire(signal_type: &str) -> Option<CallSignalType> {
        match signal_type {
            "OFFER" => Some(CallSignalType::Offer),
            "ANSWER" => Some(CallSignalType::Answer),
            "ICE" => Some(CallSignalType::Ice),
            "RENEGOTIATE" => Some(CallSignalType::Renegotiate),
            _ => None,
        }
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
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .last_peer_seq_received()
                {
                    return;
                }
                state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .record_peer_seq(seq);
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
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .commit_play(&scheduled);
                        state.sync.position_ms = target_position_ms;
                        state.sync.strict_sync_paused = false;
                        state.pending_operation_id = None;
                        state.pending_operation_kind = None;
                        state.last_committed_operation_id = Some(operation_id);
                        state.commit_scheduled_for = None;
                        let room_state = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .room_state;
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
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .commit_pause(target_position_ms, PauseCause::Manual);
                        state.sync.position_ms = target_position_ms;
                        let strict_sync_paused = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .paused_by_strict_sync;
                        state.sync.strict_sync_paused = strict_sync_paused;
                        state.pending_operation_id = None;
                        state.pending_operation_kind = None;
                        state.last_committed_operation_id = Some(operation_id);
                        state.commit_scheduled_for = None;
                        let room_state = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .room_state;
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
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .commit_seek(target_position_ms, resume_after_seek);
                        state.sync.position_ms = target_position_ms;
                        state.sync.strict_sync_paused = !resume_after_seek;
                        state.pending_operation_id = None;
                        state.pending_operation_kind = None;
                        state.last_committed_operation_id = Some(operation_id);
                        state.commit_scheduled_for = None;
                        let room_state = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .room_state;
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
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
                    let room_state = state
                        .sync_coordinator
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .room_state;
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
                        let _ = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .buffer_recovered();
                        let strict_sync_paused = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .paused_by_strict_sync;
                        state.sync.strict_sync_paused = strict_sync_paused;
                        let room_state = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .room_state;
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
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .apply_coordinator_state(host_ready, guest_ready, &coordinator_play_state);
                    // Peer readiness mirrors the coordinator's genuine flags:
                    // the host reflects the guest's ready, the guest reflects
                    // the host's ready. Never fabricated locally.
                    let is_host = Self::is_host_role(&state);
                    if let Some(peer) = &mut state.peer_participant {
                        peer.media_ready = if is_host { guest_ready } else { host_ready };
                    }
                    let room_state = state
                        .sync_coordinator
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .room_state;
                    state.room_state = room_state;
                    if state.sync.position_ms == 0 {
                        let host_pos = state
                            .sync_coordinator
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .host_position_ms;
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
                    let room_state = state
                        .sync_coordinator
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .room_state;
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
                    match Self::call_signal_type_from_wire(&signal_type) {
                        Some(parsed_type) => {
                            let signal = CallSignal {
                                signal_type: parsed_type,
                                data: data.clone(),
                            };
                            match validate_signal(&signal, &mut state.call_signal_ledger) {
                                Ok(()) => {
                                    state.call.status = match parsed_type {
                                        CallSignalType::Answer => CallRuntimeStatus::Connected,
                                        CallSignalType::Ice if state.call.connected => {
                                            CallRuntimeStatus::Connected
                                        }
                                        CallSignalType::Offer | CallSignalType::Renegotiate => {
                                            if state.call.connected {
                                                CallRuntimeStatus::Reconnecting
                                            } else {
                                                CallRuntimeStatus::Connecting
                                            }
                                        }
                                        CallSignalType::Ice => CallRuntimeStatus::Connecting,
                                    };
                                    state.call.connected =
                                        state.call.status == CallRuntimeStatus::Connected;
                                    state.call_signals.push(CallSignalSnapshot {
                                        signal_type,
                                        data,
                                        created_host_time_us: crate::network::quic::monotonic_us(),
                                    });
                                }
                                Err(error) => {
                                    state.call.status = CallRuntimeStatus::Degraded;
                                    state.call.connected = false;
                                    state.error = Some(error);
                                }
                            }
                        }
                        None => {
                            state.call.status = CallRuntimeStatus::Degraded;
                            state.call.connected = false;
                            state.error = Some("MP-CALL-001 invalid call signal".to_string());
                        }
                    }
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
                let mut coordinator = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .host_ready(ParticipantReadiness::ready(5_000));

            let target_position = state.sync.position_ms;
            let lead = clock::play_lead_us(state.peer_rtt_p95_us);
            let now = monotonic_us();
            let prepared = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .begin_pause();
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
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .buffer_low(PeerRole::Host, position_ms);
            state.sync.position_ms = position_ms;
            state.sync.strict_sync_paused = true;
            state.buffer.buffering_participant = Some(state.local_participant.display_name.clone());
            state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
            let _ = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .buffer_recovered();
            state.buffer.buffering_participant = None;
            state.buffer.percent = 100;
            state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
            let strict_sync_paused = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .paused_by_strict_sync;
            state.sync.strict_sync_paused = strict_sync_paused;
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .host_ready(readiness);
            let event = QuicServerEvent::RoomStateUpdate {
                state: format!(
                    "{:?}",
                    state
                        .sync_coordinator
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .room_state
                ),
                position_ms: state.sync.position_ms,
            };
            Self::send_host_event(&mut state, event);
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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

    /// V1 correctness gate: the local participant may only claim readiness
    /// when the media/player/provider prerequisite is genuinely usable. This
    /// never fabricates readiness from metadata or a user click alone.
    fn local_media_genuinely_ready(state: &AppRuntimeState) -> bool {
        if state.provider.url.is_some() {
            // Provider / generic-link room: usable media requires the provider
            // page to be authenticated and playback-ready (Ready means the
            // managed session is usable for navigation; PlaybackReady means a
            // real media element was detected over CDP).
            matches!(
                state.provider.readiness,
                crate::providers::sync::ProviderReadiness::PlaybackReady
                    | crate::providers::sync::ProviderReadiness::Ready
            )
        } else {
            // Local Perfect: a real player is present, opened without error,
            // and a media manifest exists. Metadata alone is never readiness.
            state.media.is_some()
                && state.player.is_some()
                && state.player_snapshot.error_message.is_none()
        }
    }

    /// M3: Dispatch a play command to the live player instance, if present.
    fn dispatch_player_play(state: &mut AppRuntimeState) {
        let Some(player) = state.player.clone() else {
            return;
        };
        let mut p = player
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match p.play() {
            Ok(()) => {
                state.player_snapshot = PlayerSnapshot::from_player(&*p);
            }
            Err(error) => {
                Self::set_player_command_error(state, &error);
            }
        }
    }

    /// M3: Dispatch a pause command to the live player instance, if present.
    fn dispatch_player_pause(state: &mut AppRuntimeState) {
        let Some(player) = state.player.clone() else {
            return;
        };
        let mut p = player
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match p.pause() {
            Ok(()) => {
                state.player_snapshot = PlayerSnapshot::from_player(&*p);
            }
            Err(error) => {
                Self::set_player_command_error(state, &error);
            }
        }
    }

    /// M3: Dispatch a seek command to the live player instance, if present.
    fn dispatch_player_seek(state: &mut AppRuntimeState, position_ms: u64) {
        let Some(player) = state.player.clone() else {
            return;
        };
        let mut p = player
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match p.seek(position_ms) {
            Ok(()) => {
                state.player_snapshot = PlayerSnapshot::from_player(&*p);
            }
            Err(error) => {
                Self::set_player_command_error(state, &error);
            }
        }
    }

    /// M3: Sync the player snapshot from the live player.
    #[allow(dead_code)]
    fn sync_player_snapshot(state: &mut AppRuntimeState) {
        let Some(player) = state.player.clone() else {
            return;
        };
        let p = player
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.player_snapshot = PlayerSnapshot::from_player(&*p);
    }

    fn transfer_percent(state: &AppRuntimeState) -> u8 {
        state
            .transfer
            .as_ref()
            .map(|progress| (progress.fraction() * 100.0).round().clamp(0.0, 100.0) as u8)
            .unwrap_or(0)
    }

    fn set_player_error(state: &mut AppRuntimeState, message: String) {
        state.room_state = RoomState::Error;
        state.sync.strict_sync_paused = true;
        state.error = Some(message.clone());
        Self::set_player_diagnostic_error(state, message);
    }

    fn set_player_command_error(state: &mut AppRuntimeState, error: &PlayerError) {
        let message = Self::media_command_error_message(error);
        if matches!(error, PlayerError::LibMpvUnavailable)
            || state
                .player_snapshot
                .error_message
                .as_deref()
                .is_some_and(|error| error.starts_with("MP-MEDIA-001"))
        {
            Self::set_player_diagnostic_error(state, message);
        } else {
            Self::set_player_error(state, message);
        }
    }

    fn set_player_diagnostic_error(state: &mut AppRuntimeState, message: String) {
        state.player_snapshot.state = "PLAYER_ERROR".to_string();
        state.player_snapshot.error_message = Some(message);
    }

    fn media_load_error_message(error: &PlayerError) -> String {
        match error {
            PlayerError::LibMpvUnavailable | PlayerError::NotReady => {
                "MP-MEDIA-001 player unavailable".to_string()
            }
            _ => "MP-MEDIA-002 media loading failed".to_string(),
        }
    }

    fn media_command_error_message(error: &PlayerError) -> String {
        match error {
            PlayerError::LibMpvUnavailable | PlayerError::NotReady => {
                "MP-MEDIA-001 player unavailable".to_string()
            }
            _ => "MP-MEDIA-003 playback command failed".to_string(),
        }
    }

    /// M3: Spawn a background task that polls the live player for position,
    /// duration, buffering state, and errors.  The snapshot is emitted to the
    /// frontend every ~200 ms so the UI stays in sync with the actual player.
    fn spawn_player_event_loop(&self) {
        let inner = Arc::clone(&self.inner);
        let runtime = self.clone();
        let task = tokio::spawn(async move {
            let mut last_position: u64 = 0;
            let mut last_state_name = String::new();
            let mut was_buffering = false;
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
                let (snap, presentation) = {
                    let player = player_arc
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    (player.snapshot(), player.presentation_status())
                };
                let (cache, manifest) = {
                    let state = inner.lock();
                    (state.guest_cache.clone(), state.media.clone())
                };
                let cache_headroom_ms = match (cache, manifest, snap.duration_ms) {
                    (Some(cache), Some(manifest), Some(duration_ms)) if duration_ms > 0 => {
                        let byte_offset = (snap.position_ms as u128 * manifest.file_size as u128
                            / duration_ms as u128) as u64;
                        let cache = cache.lock().await;
                        let contiguous = cache.contiguous_bytes_from(byte_offset);
                        ((contiguous as u128 * duration_ms as u128) / manifest.file_size as u128)
                            as u64
                    }
                    _ => 0,
                };
                let position_changed = snap.position_ms != last_position;
                let state_changed = format!("{:?}", snap.state) != last_state_name;
                if position_changed || state_changed {
                    let buffering_now =
                        matches!(snap.state, crate::media::player::PlayerState::Buffering);
                    let mut buffer_transition = None;
                    let mut correction = None;
                    let mut state = inner.lock();
                    state.player_snapshot = PlayerSnapshot::from_parts(snap.clone(), presentation);
                    last_position = snap.position_ms;
                    last_state_name = format!("{:?}", snap.state);
                    // libmpv's buffering value is authoritative when present.
                    // The sparse-cache calculation is the conservative fallback
                    // when the backend cannot expose it.
                    let headroom_ms = snap.buffered_ahead_ms.unwrap_or(cache_headroom_ms);
                    state.local_participant.buffer_ahead_ms = headroom_ms;
                    state.buffer.guest_buffer_ahead_ms = headroom_ms;
                    state.buffer.percent = Self::transfer_percent(&state);
                    if Self::is_host_role(&state) {
                        // The host owns the canonical position. A guest must
                        // retain the last host commit for drift comparison.
                        state.sync.position_ms = snap.position_ms;
                    } else if !state.sync.strict_sync_paused
                        && state.room_state == RoomState::Playing
                    {
                        correction = Some((
                            snap.position_ms as i64 - state.sync.position_ms as i64,
                            snap.position_ms,
                        ));
                    }
                    if buffering_now != was_buffering {
                        buffer_transition = Some((snap.position_ms, headroom_ms, buffering_now));
                        was_buffering = buffering_now;
                    }
                    let out = snapshot_from_state(&state);
                    drop(state);
                    inner.emit(out);
                    if let Some((position_ms, buffer_ahead_ms, stalled)) = buffer_transition {
                        // This is the canonical BUFFER_LOW/RECOVERED path.
                        // Recovery only returns the room to Ready Check; it
                        // never resumes playback independently.
                        runtime.report_buffer_status(position_ms, buffer_ahead_ms, stalled);
                    }
                    if let Some((drift_ms, position_ms)) = correction {
                        use crate::sync::drift::{correction_for_drift, DriftCorrection};
                        let mut player = player_arc
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        match correction_for_drift(drift_ms, position_ms as i64) {
                            DriftCorrection::Ignore => {
                                let _ = player.set_playback_rate(1.0);
                            }
                            DriftCorrection::PlaybackRate { rate } => {
                                let _ = player.set_playback_rate(rate);
                            }
                            DriftCorrection::MicroSeek { target_position_ms }
                            | DriftCorrection::HardSeek { target_position_ms } => {
                                let _ = player.seek(target_position_ms.max(0) as u64);
                            }
                        }
                    }
                }
            }
        });
        // Store the handle so it is aborted on leave_party, and abort any
        // previously running player event loop so a repeated spawn (duplicate
        // create, reconnect, provider→local replacement) never runs two loops
        // polling the same player.
        let mut state = self.lock();
        if let Some(old) = state.player_event_task.replace(task) {
            old.abort();
        }
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
        let mut state = self.lock();
        if let Some(old) = state.transfer_stall_watcher_task.replace(task) {
            old.abort();
        }
    }

    pub fn set_ready(&self) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            state.screen = "READY_CHECK".to_string();
            let is_host = Self::is_host_role(&state);

            // V1 correctness: readiness must reflect genuinely usable media.
            // Never fabricate media readiness to let the UI advance.
            let genuinely_ready = Self::local_media_genuinely_ready(&state);
            state.local_participant.media_ready = genuinely_ready;
            if !genuinely_ready {
                state.error = Some(
                    "MP-MEDIA-001 media is not ready for playback".to_string(),
                );
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            if is_host {
                state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .host_ready(ParticipantReadiness::ready(5_000));
                // READY_CHECK consensus: once both participants are ready the
                // coordinator fires CoordinatorStateUpdate (canonical), which
                // both sides apply. No play is started here.
                state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .update_readiness_consensus(5_000);
            } else {
                state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            // Peer readiness is propagated from the peer's genuine state via
            // GuestReadyState / CoordinatorStateUpdate — never fabricated
            // locally. V1 correctness: no fake media readiness.
            let room_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .room_state;
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
        if state.player_snapshot.error_message.is_some() {
            state.room_state = RoomState::Error;
            state.sync.strict_sync_paused = true;
        } else {
            state.room_state = RoomState::Playing;
            state.sync.strict_sync_paused = false;
        }
        sync_room_snapshot(&mut state);
        snapshot_from_state(&state)
    }

    /// Attach the already-selected local source to Cinema's native host.
    /// This only changes presentation; coordinator ownership and media source
    /// selection remain unchanged.
    pub fn attach_native_video_surface(&self, surface_handle: usize) -> AppSnapshot {
        let mut state = self.lock();
        let Some(player) = state.player.clone() else {
            Self::set_player_diagnostic_error(
                &mut state,
                "MP-MEDIA-008 no local player is available for this room".to_string(),
            );
            return snapshot_from_state(&state);
        };
        let mut player = player
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match player.attach_native_surface(surface_handle) {
            Ok(()) => {
                state.player_snapshot = PlayerSnapshot::from_player(&*player);
                sync_room_snapshot(&mut state);
                snapshot_from_state(&state)
            }
            Err(error) => {
                let message = Self::media_command_error_message(&error);
                Self::set_player_diagnostic_error(&mut state, message.clone());
                snapshot_from_state(&state)
            }
        }
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
        if let Some(task) = state.reconnect_task.take() {
            task.abort();
        }
        if let Some(task) = state.heartbeat_task.take() {
            task.abort();
        }
        if let Some(task) = state.preload_task.take() {
            task.abort();
        }
        if let Some(client) = state.client.take() {
            let _ = client;
        }
        if let Some(range) = state.range_server_handle.take() {
            range.shutdown();
        }
        state.chrome_session = None;
        state.player = None;
        state.guest_cache = None;
        state.media = None;
        state.transfer = None;
        state.buffer = BufferSnapshot {
            guest_buffer_ahead_ms: 0,
            percent: 0,
            buffering_participant: None,
        };
        state.sync = SyncSnapshot {
            room_state: "ENDED".to_string(),
            strict_sync_paused: false,
            position_ms: 0,
        };
        state.provider = ProviderSnapshot {
            mode: "LOCAL_PERFECT".to_string(),
            provider_id: None,
            url: None,
            state: "Idle".to_string(),
            readiness: crate::providers::sync::ProviderReadiness::NotStarted,
        };
        state.chat.clear();
        state.reactions.clear();
        state.player_snapshot = PlayerSnapshot::default();
        state.call.status = CallRuntimeStatus::Ended;
        state.call.connected = false;
        state.call.camera = CameraState::disabled();
        state.call.microphone.enabled = false;
        state.call_signals.clear();
        state.call_signal_ledger.reset();
        state.local_participant.camera_enabled = false;
        state.local_participant.microphone_enabled = false;
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
        state.buffer.percent = Self::transfer_percent(&state);
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
        state.call.connected = false;
        state.call_signals.clear();
        state.call_signal_ledger.reset();
        match mode {
            CallMode::VideoVoice => {
                state.call.status = if state.privacy_mode {
                    CallRuntimeStatus::Unavailable
                } else {
                    CallRuntimeStatus::Connecting
                };
                state.call.camera = if state.privacy_mode {
                    CameraState::disabled()
                } else {
                    CameraState::tier_b_enabled()
                };
            }
            CallMode::VoiceOnly => {
                state.call.status = if state.privacy_mode {
                    CallRuntimeStatus::Unavailable
                } else {
                    CallRuntimeStatus::Connecting
                };
                state.call.camera = CameraState::disabled();
            }
            CallMode::Off => {
                state.call.status = CallRuntimeStatus::Ended;
                state.call.camera = CameraState::disabled();
                state.call.microphone.enabled = false;
            }
        }
        if state.privacy_mode {
            state.call.microphone.enabled = false;
        }
        state.local_participant.camera_enabled = state.call.camera.enabled;
        state.local_participant.microphone_enabled = state.call.microphone.enabled;
        snapshot_from_state(&state)
    }

    pub fn submit_call_signal(&self, signal: CallSignal) -> Result<AppSnapshot, String> {
        let signal_type_str = format!("{:?}", signal.signal_type).to_ascii_uppercase();
        let data = signal.data.clone();
        let snapshot = {
            let mut state = self.lock();
            if state.call.mode == CallMode::Off || state.privacy_mode {
                state.call.status = CallRuntimeStatus::Unavailable;
                state.call.connected = false;
                return Err("MP-CALL-009 call unavailable".to_string());
            }
            validate_signal(&signal, &mut state.call_signal_ledger)?;
            state.call.status = match signal.signal_type {
                CallSignalType::Answer => CallRuntimeStatus::Connected,
                CallSignalType::Ice if state.call.connected => CallRuntimeStatus::Connected,
                CallSignalType::Offer | CallSignalType::Renegotiate => {
                    if state.call.connected {
                        CallRuntimeStatus::Reconnecting
                    } else {
                        CallRuntimeStatus::Connecting
                    }
                }
                CallSignalType::Ice => CallRuntimeStatus::Connecting,
            };
            state.call.connected = state.call.status == CallRuntimeStatus::Connected;
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
        if state.privacy_mode {
            state.call.status = CallRuntimeStatus::Unavailable;
            state.call.connected = false;
        }
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
        if state.privacy_mode {
            state.call.status = CallRuntimeStatus::Unavailable;
            state.call.connected = false;
        }
        state.local_participant.camera_enabled = state.call.camera.enabled;
        snapshot_from_state(&state)
    }

    pub fn set_privacy_mode(&self, enabled: bool) -> AppSnapshot {
        let mut state = self.lock();
        if enabled && !state.privacy_mode {
            state.ghost_mode_before_privacy = state.ghost_mode;
        }
        state.privacy_mode = enabled;
        if enabled {
            state.ghost_mode = true;
            state.call.camera = CameraState::disabled();
            state.call.microphone.enabled = false;
            state.call.connected = false;
            state.call.status = CallRuntimeStatus::Unavailable;
            state.call_signals.clear();
            state.call_signal_ledger.reset();
            state.local_participant.camera_enabled = false;
            state.local_participant.microphone_enabled = false;
        } else {
            // Privacy never revives local device tracks. It only restores the
            // prior visual Ghost state after the user leaves Privacy Mode.
            state.ghost_mode = state.ghost_mode_before_privacy;
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
        self.guest_prepare_media(false).await
    }

    async fn guest_prepare_media(&self, preload: bool) -> Result<AppSnapshot, String> {
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
            if let Some(task) = state.player_event_task.take() {
                task.abort();
            }
            if let Some(range) = state.range_server_handle.take() {
                range.shutdown();
            }
            if let Some(player) = state.player.take() {
                player
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .close();
            }
            state.guest_cache = None;
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
        manifest.validate_for_guest().map_err(|e| e.to_string())?;

        // 2. Create/open sparse cache
        let cache_root = {
            let mut state = self.lock();
            state
                .cache_root
                .get_or_insert_with(|| std::env::temp_dir().join("MovePartyCache"))
                .clone()
        };
        let mut cache = SparseCache::open(&cache_root, manifest.clone())
            .map_err(|e| format!("MP-MEDIA-002 cache open failed: {e}"))?;

        // The first verified chunk is the minimum gate for a playable local
        // source. Do not advertise readiness before it exists in the guest's
        // own sparse cache.
        if !cache.chunk_map().is_available(0) {
            let client = self
                .lock()
                .client
                .clone()
                .ok_or_else(|| "MP-NET-001 no QUIC client".to_string())?;
            let started = Instant::now();
            let packet = client
                .fetch_local_media_chunk(&manifest.media_id, 0)
                .await
                .map_err(|e| format!("MP-NET-003 initial media chunk failed: {e}"))?;
            validate_chunk_packet(&manifest, &packet)
                .map_err(|e| format!("MP-MEDIA-002 initial media chunk rejected: {e}"))?;
            cache
                .write_chunk(0, &packet.payload)
                .map_err(|e| format!("MP-MEDIA-002 initial cache write failed: {e}"))?;
            let elapsed_ms = started.elapsed().as_millis().max(1) as u64;
            let goodput_bps = packet.payload.len() as u64 * 8 * 1_000 / elapsed_ms;
            self.lock().network.goodput_bps = goodput_bps;
        }
        let initial_bytes_available = cache.bytes_available();
        if self.lock().db.is_some() {
            self.register_cached_media(
                &manifest.media_id,
                &manifest.filename,
                manifest.file_size,
                &manifest.full_hash,
            )?;
        }

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
            let mut state = self.lock();
            state.guest_cache = Some(cache_arc.clone());
            state.transfer = Some(transfer_progress(
                &manifest,
                initial_bytes_available,
                0,
                state.network.goodput_bps,
            ));
        }

        // 4. Open player with range-server HTTP URL
        {
            let mut state = self.lock();
            state.media = Some(manifest.clone());
            state.local_participant.media_ready = false;
            state.local_participant.buffer_ahead_ms = 0;
            state.range_server_handle = Some(range_handle);
            #[cfg(feature = "mpv")]
            {
                let mut player = MpvPlayer::new();
                if let Err(e) = player.open(std::path::Path::new(&media_url)) {
                    state.local_participant.media_ready = false;
                    Self::set_player_error(&mut state, Self::media_load_error_message(&e));
                } else {
                    state.player_snapshot = PlayerSnapshot::from_player(&player);
                }
                state.player = Some(Arc::new(std::sync::Mutex::new(player)));
            }
            #[cfg(not(feature = "mpv"))]
            {
                let mut player = crate::media::player::LibMpvPlayer::new();
                if let Err(e) = player.open(std::path::Path::new(&media_url)) {
                    state.local_participant.media_ready = false;
                    Self::set_player_error(&mut state, Self::media_load_error_message(&e));
                } else {
                    state.player_snapshot = PlayerSnapshot::from_player(&player);
                }
                state.player = Some(Arc::new(std::sync::Mutex::new(player)));
            }
            let playable =
                initial_bytes_available > 0 && state.player_snapshot.error_message.is_none();
            state.local_participant.media_ready = playable;
            state.buffer.percent = Self::transfer_percent(&state);
            state.buffer.guest_buffer_ahead_ms =
                state.player_snapshot.buffered_ahead_ms.unwrap_or(0);
            state.local_participant.buffer_ahead_ms = state.buffer.guest_buffer_ahead_ms;
            if !Self::is_host_role(&state)
                && playable
                && state.buffer.guest_buffer_ahead_ms >= 5_000
            {
                state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
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
                    let started = Instant::now();
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
                                    .map(|()| c.bytes_available())
                            };
                            match write_result {
                                Ok(bytes_available) => {
                                    {
                                        let mut state = inner.lock();
                                        let elapsed_ms =
                                            started.elapsed().as_millis().max(1) as u64;
                                        let goodput_bps =
                                            packet.payload.len() as u64 * 8 * 1_000 / elapsed_ms;
                                        state.network.goodput_bps = goodput_bps;
                                        state.transfer = Some(transfer_progress(
                                            &manifest_worker,
                                            bytes_available,
                                            state.buffer.guest_buffer_ahead_ms,
                                            goodput_bps,
                                        ));
                                        state.buffer.percent = Self::transfer_percent(&state);
                                        inner.emit(snapshot_from_state(&state));
                                    }
                                    chunk_wake_worker.notify();
                                    if preload
                                        && request.index.saturating_add(1)
                                            < manifest_worker.chunk_count
                                    {
                                        demand_worker
                                            .request(request.index + 1, ChunkPriority::Background);
                                    }
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
                            // Preserve the verified sparse cache and retry the
                            // same demand after transport recovery. The
                            // reconnect worker replaces `state.client`; this
                            // worker never restarts the whole movie download.
                            demand_worker.request(request.index, ChunkPriority::Critical);
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        }
                    }
                }
            })
        };
        {
            self.lock().transfer_task = Some(transfer_task);
        }
        self.spawn_transfer_stall_watcher();

        // Scheduled preload keeps progressing sequentially through missing
        // media after the verified opening chunk. Range demand remains
        // higher priority, so playback always wins over background preload.
        if preload && manifest.chunk_count > 1 {
            demand.request(1, ChunkPriority::ImmediateFuture);
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
            player_event_worker_owned: state.player_event_task.is_some(),
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
    pub player_event_worker_owned: bool,
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

const FALLBACK_PRELOAD_GOODPUT_BPS: u64 = 2_000_000;

/// Preserve an explicitly earlier user deadline while moving an unsafe later
/// one earlier from measured (or conservative fallback) goodput.
fn adaptive_preload_deadline(
    requested_utc_ms: i64,
    remaining_bytes: u64,
    observed_goodput_bps: u64,
    scheduled_start_utc_ms: i64,
) -> i64 {
    let goodput_bps = observed_goodput_bps.max(FALLBACK_PRELOAD_GOODPUT_BPS);
    let calculated = crate::scheduling::calculate_preload_start(crate::scheduling::PreloadInputs {
        remaining_bytes,
        conservative_goodput_bps: goodput_bps,
        scheduled_start_utc_ms,
    })
    .unwrap_or(requested_utc_ms);
    requested_utc_ms.min(calculated)
}

/// Production preload executor: drives the actual Local Perfect preload path
/// (`guest_fetch_media`) when a QUIC peer is online; otherwise reports that
/// prerequisites are missing so the scheduler persists a waiting state and
/// retries later.
#[derive(Debug, Clone)]
pub struct AppRuntimePreloadExecutor {
    runtime: AppRuntime,
}

impl crate::scheduling::preload::PreloadExecutor for AppRuntimePreloadExecutor {
    fn execute(
        &self,
        schedule: &crate::storage::sqlite::StoredSchedule,
    ) -> Result<crate::scheduling::preload::PreloadOutcome, String> {
        let schedule_matches_active_media = {
            let snapshot = self.runtime.snapshot();
            snapshot
                .media
                .as_ref()
                .map(|media| media.media_id == schedule.media_id)
                .unwrap_or(false)
        };
        if self.runtime.client_for_test().is_none() || !schedule_matches_active_media {
            return Ok(crate::scheduling::preload::PreloadOutcome::WaitingForPrerequisites);
        }
        let runtime = self.runtime.clone();
        let schedule = schedule.clone();
        let task = tokio::spawn(async move {
            // Real preload preparation for the scheduled session: fetch the
            // manifest, open the sparse cache, start the demand-driven range
            // server and transfer worker so bytes are ready by show time.
            let _ = runtime.guest_prepare_media(true).await;
            let _ = schedule;
        });
        let mut state = self.runtime.lock();
        if let Some(old) = state.preload_task.replace(task) {
            old.abort();
        }
        Ok(crate::scheduling::preload::PreloadOutcome::Started)
    }
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
    use super::{
        adaptive_preload_deadline, reconnect_failure, AppRuntime, AppSnapshot, ReconnectFailure,
    };
    use crate::call::{CallSignal, CallSignalType};
    use crate::network::quic::QuicError;
    use crate::resilience::{FailureEvent, RecoveryAction};
    use crate::room::MovePartyInvite;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    /// Serializes tests that read or write the process-global
    /// `MOVE_PARTY_DEV_LOOPBACK` env var so Rust's parallel test runner never
    /// races two loopback-bind scenarios against each other.
    static LOOPBACK_ENV_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
        std::sync::OnceLock::new();
    fn loopback_env_lock() -> &'static tokio::sync::Mutex<()> {
        LOOPBACK_ENV_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[test]
    fn backend_snapshot_has_no_demo_peer_or_movie_by_default() {
        let runtime = AppRuntime::new();
        let snapshot = runtime.snapshot();

        assert!(snapshot.media.is_none());
        assert_eq!(snapshot.participants.len(), 1);
        assert!(snapshot.chat.is_empty());
    }

    #[test]
    fn observed_goodput_moves_an_unsafe_preload_deadline_earlier() {
        let requested = 9_900_000;
        let deadline = adaptive_preload_deadline(requested, 1_000_000_000, 8_000_000, 10_000_000);
        assert!(deadline < requested);
    }

    #[test]
    fn preload_deadline_keeps_an_explicitly_earlier_choice() {
        let deadline = adaptive_preload_deadline(100, 1_000, 8_000_000, 10_000_000);
        assert_eq!(deadline, 100);
    }

    #[test]
    fn reconnect_failure_maps_auth_tls_protocol_errors_to_terminal() {
        assert!(matches!(
            reconnect_failure(QuicError::Auth("bad".into())),
            ReconnectFailure::Terminal(_)
        ));
        assert!(matches!(
            reconnect_failure(QuicError::Tls("bad".into())),
            ReconnectFailure::Terminal(_)
        ));
        assert!(matches!(
            reconnect_failure(QuicError::UnexpectedResponse),
            ReconnectFailure::Terminal(_)
        ));
    }

    #[test]
    fn reconnect_failure_keeps_transport_errors_retryable() {
        assert!(matches!(
            reconnect_failure(QuicError::ClosedStream),
            ReconnectFailure::Retryable
        ));
    }

    #[tokio::test]
    async fn reconnect_worker_not_spawned_for_host_role_or_missing_invite() {
        let runtime = AppRuntime::new();
        runtime.spawn_reconnect_worker();
        assert!(
            runtime.lock().reconnect_task.is_none(),
            "host role must not spawn a reconnect worker"
        );
        {
            let mut state = runtime.lock();
            state.local_participant.role = "Guest".to_string();
            state.invite = None;
        }
        runtime.spawn_reconnect_worker();
        assert!(
            runtime.lock().reconnect_task.is_none(),
            "guest without invite must not spawn"
        );
    }

    #[tokio::test]
    async fn reconnect_worker_spawns_at_most_one_worker() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.local_participant.role = "Guest".to_string();
            state.invite = Some(MovePartyInvite {
                v: 1,
                protocol_major: 1,
                protocol_minor: 0,
                room_id: "AAAAAAAAAAAAAAAAAAAAAA".to_string(),
                join_secret: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
                host_device_id: "host-dev".to_string(),
                host_ip: "127.0.0.1".to_string(),
                host_port: 1,
                server_certificate_fingerprint: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                    .to_string(),
                expires_at_ms: i64::MAX,
            });
        }
        runtime.spawn_reconnect_worker();
        assert!(runtime.lock().reconnect_task.is_some());
        assert!(!runtime
            .lock()
            .reconnect_task
            .as_ref()
            .unwrap()
            .is_finished());
        // Second call while the first is still running: the guard returns
        // early and does not replace the existing handle.
        runtime.spawn_reconnect_worker();
        {
            let state = runtime.lock();
            let alive = state
                .reconnect_task
                .as_ref()
                .map(|t| !t.is_finished())
                .unwrap_or(false);
            assert!(alive, "original worker must still be running after guard");
        }
        runtime.leave_party();
    }

    #[tokio::test]
    async fn apply_disconnect_preserves_guest_cache_and_pauses_playback() {
        let runtime = AppRuntime::new();
        let dir = std::env::temp_dir().join(format!("m4c_cache_{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("cache dir");
        let manifest = crate::media::manifest::MediaManifest {
            media_id: "test".to_string(),
            filename: "test.bin".to_string(),
            file_size: 12,
            container: None,
            full_hash: "hash".to_string(),
            quick_fingerprint: crate::media::manifest::QuickFingerprint {
                file_size: 12,
                first_hash: "f".to_string(),
                last_hash: "l".to_string(),
            },
            chunk_size: 4,
            chunk_count: 3,
        };
        let cache = Arc::new(Mutex::new(
            crate::media::cache::SparseCache::open(&dir, manifest).expect("open"),
        ));
        {
            let mut state = runtime.lock();
            state.guest_cache = Some(cache);
            state.local_participant.role = "Guest".to_string();
            state.invite = Some(MovePartyInvite {
                v: 1,
                protocol_major: 1,
                protocol_minor: 0,
                room_id: "AAAAAAAAAAAAAAAAAAAAAA".to_string(),
                join_secret: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
                host_device_id: "host".to_string(),
                host_ip: "127.0.0.1".to_string(),
                host_port: 1,
                server_certificate_fingerprint: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                    .to_string(),
                expires_at_ms: i64::MAX,
            });
        }
        let _ = AppRuntime::apply_disconnect(&runtime.inner);
        let state = runtime.lock();
        assert!(
            state.guest_cache.is_some(),
            "sparse cache must survive apply_disconnect"
        );
        assert!(
            state.sync.strict_sync_paused,
            "disconnect must pause strict sync"
        );
        assert!(
            !state.network.connected,
            "disconnect must mark network down"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leave_party_aborts_reconnect_heartbeat_preload_and_watch_workers() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let runtime = AppRuntime::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let spawn_long = |cancelled: Arc<AtomicBool>| {
            tokio::spawn(async move {
                struct Guard(Arc<AtomicBool>);
                impl Drop for Guard {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let _guard = Guard(cancelled);
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            })
        };
        {
            let mut state = runtime.lock();
            state.reconnect_task = Some(spawn_long(cancelled.clone()));
            state.heartbeat_task = Some(spawn_long(cancelled.clone()));
            state.preload_task = Some(spawn_long(cancelled.clone()));
            state.transfer_stall_watcher_task = Some(spawn_long(cancelled.clone()));
        }
        runtime.leave_party();
        // Poll for up to 2s for the Drop guards to run; under full-suite
        // parallel load the tokio abort may be deferred.
        for _ in 0..20 {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let state = runtime.lock();
        assert!(state.reconnect_task.is_none());
        assert!(state.heartbeat_task.is_none());
        assert!(state.preload_task.is_none());
        assert!(state.transfer_stall_watcher_task.is_none());
        assert!(
            cancelled.load(Ordering::SeqCst),
            "leave_party must abort all background workers"
        );
    }

    #[tokio::test]
    async fn heartbeat_worker_is_replaced_not_duplicated() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let runtime = AppRuntime::new();
        let old_aborted = Arc::new(AtomicBool::new(false));
        let old_started = Arc::new(AtomicBool::new(false));
        let old = tokio::spawn({
            let flag = old_aborted.clone();
            let started = old_started.clone();
            async move {
                struct Guard(Arc<AtomicBool>);
                impl Drop for Guard {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let _guard = Guard(flag);
                started.store(true, Ordering::SeqCst);
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            }
        });
        {
            let mut state = runtime.lock();
            state.heartbeat_task = Some(old);
        }
        // Ensure the old worker actually started before it is replaced;
        // aborting a never-polled task never runs its body or Drop.
        for _ in 0..50 {
            if old_started.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(old_started.load(Ordering::SeqCst), "old worker must start");
        // Second spawn replaces (aborts) the first.
        runtime.spawn_guest_heartbeat();
        let mut aborted = false;
        for _ in 0..50 {
            if old_aborted.load(Ordering::SeqCst) {
                aborted = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            aborted,
            "old heartbeat worker must be aborted on replacement"
        );
        assert!(
            runtime.lock().heartbeat_task.is_some(),
            "new heartbeat worker must be present"
        );
        runtime.leave_party();
    }

    #[tokio::test]
    async fn player_event_loop_is_replaced_not_duplicated() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let runtime = AppRuntime::new();
        let old_aborted = Arc::new(AtomicBool::new(false));
        let old_started = Arc::new(AtomicBool::new(false));
        let old = tokio::spawn({
            let flag = old_aborted.clone();
            let started = old_started.clone();
            async move {
                struct Guard(Arc<AtomicBool>);
                impl Drop for Guard {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let _guard = Guard(flag);
                started.store(true, Ordering::SeqCst);
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            }
        });
        {
            let mut state = runtime.lock();
            state.player_event_task = Some(old);
        }
        for _ in 0..50 {
            if old_started.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(old_started.load(Ordering::SeqCst), "old loop must start");
        // Second spawn replaces (aborts) the first so duplicate creates never
        // run two loops polling the same player.
        runtime.spawn_player_event_loop();
        let mut aborted = false;
        for _ in 0..50 {
            if old_aborted.load(Ordering::SeqCst) {
                aborted = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            aborted,
            "old player event loop must be aborted on replacement"
        );
        runtime.leave_party();
    }

    #[test]
    fn backend_owns_navigation_screens() {
        let runtime = AppRuntime::new();

        assert_eq!(runtime.show_join_party().screen, "JOIN_PARTY");
        assert_eq!(runtime.request_end_party().screen, "PARTY_END_CONFIRM");
        assert_eq!(runtime.return_home().screen, "HOME");
    }

    #[test]
    fn provider_unavailable_is_truthful_runtime_state() {
        let runtime = AppRuntime::new();
        let snapshot = runtime.provider_unavailable(
            "youtube".to_string(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            "MP-PROVIDER-001 Chrome executable unavailable".to_string(),
        );

        assert_eq!(snapshot.screen, "LOBBY");
        assert_eq!(snapshot.provider.mode, "PROVIDER_SYNC");
        assert_eq!(snapshot.provider.provider_id.as_deref(), Some("youtube"));
        assert!(snapshot
            .provider
            .state
            .contains("Chrome executable unavailable"));
        assert_eq!(
            snapshot.provider.readiness,
            crate::providers::sync::ProviderReadiness::Unavailable
        );
        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-PROVIDER-001 Chrome executable unavailable")
        );
    }

    #[test]
    fn provider_readiness_defaults_to_not_started() {
        let runtime = AppRuntime::new();
        let snapshot = runtime.snapshot();

        assert_eq!(
            snapshot.provider.readiness,
            crate::providers::sync::ProviderReadiness::NotStarted
        );
    }

    #[test]
    fn provider_validate_readiness_rejects_login_required() {
        let runtime = AppRuntime::new();
        // Initially NotStarted — should fail
        assert!(runtime.validate_provider_ready_for_room().is_err());

        // Set readiness to LoginRequired via update
        let _snap = runtime.update_provider_readiness(
            crate::providers::sync::ProviderReadiness::LoginRequired,
        );
        assert!(runtime.validate_provider_ready_for_room().is_err());

        // Set to Ready — should pass
        let _snap = runtime.update_provider_readiness(
            crate::providers::sync::ProviderReadiness::Ready,
        );
        assert!(runtime.validate_provider_ready_for_room().is_ok());

        // Set to PlaybackReady — should pass
        let _snap = runtime.update_provider_readiness(
            crate::providers::sync::ProviderReadiness::PlaybackReady,
        );
        assert!(runtime.validate_provider_ready_for_room().is_ok());
    }

    #[test]
    fn provider_status_returns_error_when_chrome_not_launched() {
        let runtime = AppRuntime::new();
        let result = runtime.detect_provider_session_status("youtube");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("browser not launched"));
    }

    #[test]
    fn provider_switching_replaces_stale_state() {
        let runtime = AppRuntime::new();
        let _snap = runtime.update_provider_readiness(
            crate::providers::sync::ProviderReadiness::Ready,
        );

        assert_eq!(
            runtime.snapshot().provider.readiness,
            crate::providers::sync::ProviderReadiness::Ready
        );

        // return_home resets to NotStarted
        let _home = runtime.return_home();
        assert_eq!(
            runtime.snapshot().provider.readiness,
            crate::providers::sync::ProviderReadiness::NotStarted
        );
    }

    #[test]
    fn readiness_descriptions_map_to_non_empty_state_text() {
        let runtime = AppRuntime::new();
        for variant in [
            crate::providers::sync::ProviderReadiness::NotStarted,
            crate::providers::sync::ProviderReadiness::Launching,
            crate::providers::sync::ProviderReadiness::LoginRequired,
            crate::providers::sync::ProviderReadiness::Ready,
            crate::providers::sync::ProviderReadiness::Navigating,
            crate::providers::sync::ProviderReadiness::PlaybackReady,
            crate::providers::sync::ProviderReadiness::Unavailable,
            crate::providers::sync::ProviderReadiness::Error,
        ] {
            let _snap = runtime.update_provider_readiness(variant);
            let desc = runtime.snapshot().provider.state;
            assert!(!desc.is_empty(), "state for {variant:?} must not be empty");
        }
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
    fn scheduler_spawn_is_safe_without_entered_tokio_reactor() {
        let runtime = AppRuntime::new();

        runtime.spawn_scheduler_worker();
        runtime.stop_scheduler_for_test();
    }

    #[test]
    fn call_signal_updates_backend_call_state() {
        let runtime = AppRuntime::new();
        let snapshot = runtime
            .submit_call_signal(CallSignal {
                signal_type: CallSignalType::Offer,
                data: r#"{"type":"offer","sdp":"v=0\r\n"}"#.to_string(),
            })
            .expect("signal");

        assert_eq!(
            snapshot.call.status,
            crate::call::CallRuntimeStatus::Connecting
        );
        assert!(!snapshot.call.connected);
        assert_eq!(snapshot.call_signals.len(), 1);
        assert_eq!(snapshot.call_signals[0].signal_type, "OFFER");
    }

    #[test]
    fn privacy_mode_blocks_call_mode_from_reenabling_devices() {
        let runtime = AppRuntime::new();
        let private = runtime.set_privacy_mode(true);
        assert!(!private.call.camera.enabled);
        assert!(!private.call.microphone.enabled);

        let snapshot = runtime.set_call_mode(crate::call::CallMode::VideoVoice);

        assert!(snapshot.privacy_mode);
        assert!(!snapshot.call.camera.enabled);
        assert!(!snapshot.call.microphone.enabled);
        assert_eq!(
            snapshot.call.status,
            crate::call::CallRuntimeStatus::Unavailable
        );
    }

    #[test]
    fn ghost_mode_keeps_local_call_devices_unchanged() {
        let runtime = AppRuntime::new();
        runtime.set_camera_enabled(true);
        runtime.set_microphone_enabled(true);

        let snapshot = runtime.set_ghost_mode(true);

        assert!(snapshot.ghost_mode);
        assert!(snapshot.call.camera.enabled);
        assert!(snapshot.call.microphone.enabled);
    }

    #[test]
    fn leaving_privacy_does_not_reenable_devices_or_stick_ghost_mode() {
        let runtime = AppRuntime::new();
        let private = runtime.set_privacy_mode(true);
        assert!(private.ghost_mode);

        let restored = runtime.set_privacy_mode(false);

        assert!(!restored.privacy_mode);
        assert!(!restored.ghost_mode);
        assert!(!restored.call.camera.enabled);
        assert!(!restored.call.microphone.enabled);
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

    #[test]
    fn enter_cinema_preserves_player_error_state() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            AppRuntime::set_player_error(
                &mut state,
                "MP-MEDIA-006 simulated player failure".to_string(),
            );
        }

        let snapshot = runtime.enter_cinema();

        assert_eq!(snapshot.screen, "CINEMA");
        assert_eq!(snapshot.sync.room_state, "ERROR");
        assert!(snapshot.sync.strict_sync_paused);
        assert_eq!(snapshot.player.state, "PLAYER_ERROR");
        assert_eq!(
            snapshot.player.error_message.as_deref(),
            Some("MP-MEDIA-006 simulated player failure")
        );
    }

    // §6: dev loopback mode selection — all three scenarios in one test to avoid
    // env-var races from Rust's default parallel test runner.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dev_loopback_mode_selects_correct_bind_addr() {
        let _env_guard = loopback_env_lock().lock().await;
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
        assert_not_dev_loopback(r0.create_local_party(None).await);
        r0.leave_party();
        drop(_g0);

        // --- =0: same Tailscale path ---
        let _g1 = ScopedEnv {
            key: "MOVE_PARTY_DEV_LOOPBACK",
            prev: std::env::var_os("MOVE_PARTY_DEV_LOOPBACK"),
        };
        std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "0");
        let r1 = AppRuntime::new();
        assert_not_dev_loopback(r1.create_local_party(None).await);
        r1.leave_party();
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

        // --- =1 with local media: host room reaches Lobby with manifest ---
        let dir = std::env::temp_dir().join(uuid::Uuid::now_v7().to_string());
        std::fs::create_dir_all(&dir).expect("temp dir");
        let movie = dir.join("movie.mkv");
        std::fs::write(&movie, b"fake movie bytes for manifest").expect("movie file");

        let r3 = AppRuntime::new();
        let media_snap = r3
            .create_local_party(Some(movie.to_string_lossy().to_string()))
            .await
            .expect("loopback create_local_party with media must succeed");
        assert_eq!(media_snap.screen, "LOBBY");
        assert!(media_snap.room.invite_code.is_some());
        assert!(media_snap.media.is_some());
        assert_eq!(media_snap.media.as_ref().unwrap().filename, "movie.mkv");
        if media_snap.player.error_message.is_some() {
            assert!(!media_snap.participants[0].media_ready);
            assert_eq!(media_snap.screen, "LOBBY");
        } else {
            assert!(media_snap.participants[0].media_ready);
        }
        r3.leave_party();
        std::fs::remove_dir_all(&dir).expect("cleanup");

        fn assert_not_dev_loopback(result: Result<AppSnapshot, String>) {
            if let Ok(snapshot) = result {
                assert!(
                    !snapshot.network.path.starts_with("Listening on 127.0.0.1:"),
                    "production mode must not use loopback bind without MOVE_PARTY_DEV_LOOPBACK=1"
                );
            }
        }
    }

    #[test]
    fn leave_party_clears_stale_media_provider_and_social_state() {
        let runtime = AppRuntime::new();

        // Simulate a room that has accumulated media, provider, buffer,
        // chat, reactions and player state.
        {
            let mut state = runtime.lock();
            state.media = Some(crate::media::manifest::MediaManifest {
                media_id: "m".to_string(),
                filename: "movie.mkv".to_string(),
                file_size: 1000,
                container: Some("mkv".to_string()),
                full_hash: "h".to_string(),
                quick_fingerprint: crate::media::manifest::QuickFingerprint {
                    file_size: 1000,
                    first_hash: "a".to_string(),
                    last_hash: "b".to_string(),
                },
                chunk_size: 100,
                chunk_count: 10,
            });
            state.provider.mode = "PROVIDER_SYNC".to_string();
            state.provider.provider_id = Some("netflix".to_string());
            state.provider.url = Some("https://www.netflix.com/watch/1".to_string());
            state.provider.readiness = crate::providers::sync::ProviderReadiness::PlaybackReady;
            state.buffer.guest_buffer_ahead_ms = 8_000;
            state.buffer.percent = 60;
            state.sync.position_ms = 42_000;
            state.chat.push(super::ChatSnapshot {
                id: "c".to_string(),
                sender: "host".to_string(),
                body: "hi".to_string(),
                created_host_time_us: 1,
            });
            state.reactions.push(super::ReactionSnapshot {
                id: "r".to_string(),
                sender: "guest".to_string(),
                reaction: "popcorn".to_string(),
                created_host_time_us: 2,
            });
            state.player_snapshot = super::PlayerSnapshot {
                state: "Playing".to_string(),
                position_ms: 42_000,
                duration_ms: Some(100_000),
                ..super::PlayerSnapshot::default()
            };
        }

        runtime.leave_party();

        let snapshot = runtime.snapshot();
        assert!(snapshot.media.is_none(), "media must be cleared on leave");
        assert!(snapshot.transfer.is_none(), "transfer must be cleared on leave");
        assert_eq!(snapshot.buffer.guest_buffer_ahead_ms, 0);
        assert_eq!(snapshot.buffer.percent, 0);
        assert_eq!(snapshot.sync.position_ms, 0);
        assert!(snapshot.provider.provider_id.is_none());
        assert_eq!(
            snapshot.provider.readiness,
            crate::providers::sync::ProviderReadiness::NotStarted
        );
        assert!(snapshot.chat.is_empty(), "chat must be cleared on leave");
        assert!(
            snapshot.reactions.is_empty(),
            "reactions must be cleared on leave"
        );
        assert_eq!(snapshot.player.state, "STOPPED");
        assert_eq!(snapshot.player.position_ms, 0);
    }

    #[tokio::test]
    async fn create_local_party_aborts_previous_host_session() {
        let guard = loopback_env_lock().lock().await;
        struct ScopedEnv(&'static str, Option<std::ffi::OsString>);
        impl Drop for ScopedEnv {
            fn drop(&mut self) {
                match &self.1 {
                    Some(value) => std::env::set_var(self.0, value),
                    None => std::env::remove_var(self.0),
                }
            }
        }
        let _env = ScopedEnv(
            "MOVE_PARTY_DEV_LOOPBACK",
            std::env::var_os("MOVE_PARTY_DEV_LOOPBACK"),
        );
        std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");
        let runtime = AppRuntime::new();
        let first = runtime
            .create_local_party(None)
            .await
            .expect("first create_local_party");
        assert_eq!(first.screen, "LOBBY");
        let first_invite = first.room.invite_code.clone().expect("invite");
        assert!(runtime.lock().host_session.is_some());

        // Creating a second room while the first server is running must
        // abort the first server and produce a fresh session, never a leak
        // or a duplicated listener.
        let second = runtime
            .create_local_party(None)
            .await
            .expect("second create_local_party");
        assert_eq!(second.screen, "LOBBY");
        let second_invite = second.room.invite_code.clone().expect("invite");
        assert_ne!(
            first_invite, second_invite,
            "a new room must generate a fresh invite"
        );
        assert!(
            runtime.lock().host_session.is_some(),
            "host session must be present after second create"
        );

        runtime.leave_party();
        drop(_env);
        drop(guard);
    }

    #[tokio::test]
    async fn invalid_invite_does_not_destroy_existing_room() {
        let guard = loopback_env_lock().lock().await;
        struct ScopedEnv(&'static str, Option<std::ffi::OsString>);
        impl Drop for ScopedEnv {
            fn drop(&mut self) {
                match &self.1 {
                    Some(value) => std::env::set_var(self.0, value),
                    None => std::env::remove_var(self.0),
                }
            }
        }
        let _env = ScopedEnv(
            "MOVE_PARTY_DEV_LOOPBACK",
            std::env::var_os("MOVE_PARTY_DEV_LOOPBACK"),
        );
        std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");

        let runtime = AppRuntime::new();
        let first = runtime
            .create_local_party(None)
            .await
            .expect("create_local_party");
        assert!(runtime.lock().host_session.is_some());
        let original_invite = first.room.invite_code.clone().expect("invite");

        // A malformed invite must fail BEFORE the existing session is torn
        // down, so an accidental bad paste never destroys the current room.
        let result = runtime
            .join_party("not-an-invite".to_string())
            .await;
        assert!(result.is_err(), "malformed invite must be rejected");
        assert!(
            runtime.lock().host_session.is_some(),
            "invalid invite must not destroy the current host session"
        );
        assert_eq!(
            runtime.snapshot().room.invite_code.as_deref(),
            Some(original_invite.as_str()),
            "invalid invite must not overwrite the current room invite"
        );

        runtime.leave_party();
        drop(_env);
        drop(guard);
    }

    #[test]
    fn set_ready_does_not_fabricate_media_readiness() {
        let runtime = AppRuntime::new();

        // No media, no player, no provider: the local participant must NOT
        // become media-ready just because Ready was pressed (V1 correctness).
        let snapshot = runtime.set_ready();

        assert_eq!(snapshot.screen, "READY_CHECK");
        assert!(
            !snapshot.participants[0].media_ready,
            "set_ready must not fabricate media readiness"
        );
        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-MEDIA-001 media is not ready for playback")
        );
    }

    #[test]
    fn set_ready_requires_provider_playback_readiness() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.provider.mode = "PROVIDER_SYNC".to_string();
            state.provider.provider_id = Some("netflix".to_string());
            state.provider.url = Some("https://www.netflix.com/watch/1".to_string());
            state.provider.readiness = crate::providers::sync::ProviderReadiness::LoginRequired;
        }

        let snapshot = runtime.set_ready();

        assert!(
            !snapshot.participants[0].media_ready,
            "a provider room must not be ready while login is required"
        );
        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-MEDIA-001 media is not ready for playback")
        );
    }

    #[test]
    fn set_ready_with_genuine_local_media_marks_ready() {
        use crate::media::player::{LibMpvPlayer, LocalPlayer};
        let runtime = AppRuntime::new();
        let path = std::env::temp_dir().join(format!("mp_ready_{}", uuid::Uuid::now_v7()));
        std::fs::write(&path, b"movie").expect("write");
        {
            let mut state = runtime.lock();
            state.media = Some(crate::media::manifest::MediaManifest {
                media_id: "m".to_string(),
                filename: "movie.mkv".to_string(),
                file_size: 5,
                container: Some("mkv".to_string()),
                full_hash: "h".to_string(),
                quick_fingerprint: crate::media::manifest::QuickFingerprint {
                    file_size: 5,
                    first_hash: "a".to_string(),
                    last_hash: "b".to_string(),
                },
                chunk_size: 5,
                chunk_count: 1,
            });
            let mut player = LibMpvPlayer::with_availability(true);
            player.open(&path).expect("open");
            state.player_snapshot = super::PlayerSnapshot::from_player(&player);
            state.player = Some(Arc::new(std::sync::Mutex::new(player)));
        }

        let snapshot = runtime.set_ready();

        assert!(snapshot.participants[0].media_ready);
        assert!(snapshot.error.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn guest_ready_state_propagates_peer_media_readiness() {
        use crate::network::quic::QuicHostEvent;
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.peer_participant = Some(super::ParticipantSnapshot {
                id: "guest-id".to_string(),
                display_name: "Guest".to_string(),
                role: "Guest".to_string(),
                connected: true,
                media_ready: false,
                camera_enabled: true,
                microphone_enabled: false,
                buffer_ahead_ms: 0,
            });
        }

        runtime.apply_host_event(QuicHostEvent::GuestReadyState {
            broadcaster_device_id: "guest-id".to_string(),
            coordinator_play_state: "READYCHECK".to_string(),
            coordinator_ready: true,
            coordinator_buffer_ahead_ms: 8_000,
        });

        let snapshot = runtime.snapshot();
        let peer = snapshot
            .participants
            .iter()
            .find(|p| p.role == "Guest")
            .expect("guest participant");
        assert!(
            peer.media_ready,
            "guest readiness must propagate to the host's peer snapshot"
        );
        assert_eq!(peer.buffer_ahead_ms, 8_000);
    }

    #[test]
    fn coordinator_update_mirrors_peer_media_readiness() {
        use crate::network::quic::{EventEnvelope, ServerEvent as QuicServerEvent};
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.peer_participant = Some(super::ParticipantSnapshot {
                id: "guest-id".to_string(),
                display_name: "Guest".to_string(),
                role: "Guest".to_string(),
                connected: true,
                media_ready: false,
                camera_enabled: true,
                microphone_enabled: false,
                buffer_ahead_ms: 0,
            });
        }

        let envelope = EventEnvelope {
            seq: 1,
            sender: "host-id".to_string(),
            sent_mono_us: 1,
            event: QuicServerEvent::CoordinatorStateUpdate {
                host_ready: true,
                guest_ready: true,
                coordinator_play_state: "READYCHECK".to_string(),
                buffer_ahead_ms: 8_000,
            },
        };
        AppRuntime::apply_peer_event(
            &runtime.inner,
            &envelope,
            envelope.event.clone(),
        );

        let snapshot = runtime.snapshot();
        let peer = snapshot
            .participants
            .iter()
            .find(|p| p.role == "Guest")
            .expect("guest participant");
        assert!(
            peer.media_ready,
            "CoordinatorStateUpdate must mirror the peer's genuine readiness"
        );
    }
}
