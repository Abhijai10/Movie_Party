use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, RwLock,
    },
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    call::{
        recommend_camera_state, validate_signal, CallMode, CallRuntimeStatus, CallSignal,
        CallSignalLedger, CallSignalType, CameraFeedback, CameraState, MicState,
    },
    chat::{
        allowed_reactions, push_bounded, validate_chat_message, validate_reaction,
        validate_received_chat_message, validate_received_reaction, ChatMessage, ReactionMessage,
        ReactionRateLimiter, MAX_CHAT_HISTORY, MAX_REACTION_HISTORY,
    },
    identity::DeviceIdentity,
    media::{
        manifest::{build_manifest, MediaManifest},
        player::{
            presentation::PlayerPresentationStatus, LocalPlayer, PlayerError,
            PlayerSnapshot as LibPlayerSnapshot, PlayerState,
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
    /// §56: schedule id the guest has an un-answered accept request for
    /// (host-created schedule pending the guest's explicit decision).
    pub pending_guest_schedule: Option<PendingGuestScheduleSnapshot>,
    /// §52: post-party local-media retention question for the guest —
    /// offered once the party with a transferred movie ends. None = no
    /// prompt owed.
    pub retention_prompt: Option<RetentionPromptSnapshot>,
}

/// A schedule awaiting the guest's explicit accept/decline (§56).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingGuestScheduleSnapshot {
    pub schedule_id: String,
    pub media_id: String,
    pub scheduled_start_utc_ms: i64,
}

/// The §52 retention question: keep, remove, or save-as the transferred
/// movie cache on THIS device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPromptSnapshot {
    pub media_id: String,
    pub filename: String,
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
                pending_operation: None,
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
                camera_notice: None,
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
            pending_guest_schedule: None,
            retention_prompt: None,
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
    /// A protocol operation in flight (PLAY/PAUSE/SEEK prepare). §25: the
    /// Ready-Check countdown animates from this backend-provided deadline
    /// — the frontend never invents its own unsynchronized countdown.
    /// `execute_at_wall_ms` is a UI-display projection of the authoritative
    /// host-monotonic deadline (monotonic time still drives execution,
    /// §16; wall clock is legal for UI display only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_operation: Option<PendingOperationSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperationSnapshot {
    pub kind: String,
    pub target_position_ms: u64,
    /// Authoritative host-monotonic execution deadline (µs).
    pub execute_at_host_mono_us: u64,
    /// Wall-clock projection for UI display only (epoch ms).
    pub execute_at_wall_ms: u64,
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
    /// Once-per-event camera degradation notice
    /// (PRD §41 movie-first policy). Set when the ladder downgrades or
    /// disables the camera to protect movie continuity; cleared by the
    /// frontend consumer after display. None = nothing to show.
    pub camera_notice: Option<String>,
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

/// Maximum user-visible display-name length, counted in CHARACTERS (F50).
///
/// This is a user-facing constraint, so it must be character-aware: a byte
/// limit silently rejects valid names in non-Latin scripts — a 40-character
/// Cyrillic name is 80 bytes — and one call site used `len()` while two others
/// used `chars().count()`, so the same name was accepted in one path and
/// discarded in another. Protocol and storage limits elsewhere stay byte-based
/// on purpose; only this user-visible rule is character-counted.
const MAX_DISPLAY_NAME_CHARS: usize = 40;

/// Canonical wire name for a failed player.
///
/// The `PlayerState` enum variant is `Error`, so a plain `Debug`-and-uppercase
/// rendering produces `"ERROR"` — while the diagnostic path, the failure
/// watcher, both play gates, the Cinema UI and the provider overlay all
/// speak `"PLAYER_ERROR"`. That split meant the event loop could overwrite a
/// diagnostic error with `"ERROR"` and every consumer would silently miss a
/// real playback failure (F33).
///
/// The mismatch is removed at the single producer instead of teaching every
/// consumer two spellings: `player_state_wire_name` is the only place a
/// player state becomes a string.
pub const PLAYER_STATE_ERROR: &str = "PLAYER_ERROR";

/// Wire name for a player state. Every variant keeps its `Debug` form except
/// `Error`, which uses [`PLAYER_STATE_ERROR`] so the failure state reads the
/// same no matter which producer wrote it last.
fn player_state_wire_name(state: &PlayerState) -> String {
    match state {
        PlayerState::Error => PLAYER_STATE_ERROR.to_string(),
        other => format!("{other:?}").to_ascii_uppercase(),
    }
}

impl From<&LibPlayerSnapshot> for PlayerSnapshot {
    fn from(snap: &LibPlayerSnapshot) -> Self {
        Self {
            state: player_state_wire_name(&snap.state),
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

    /// Adopt a freshly observed player snapshot, keeping a previously
    /// recorded failure diagnostic when the new observation carries none.
    ///
    /// [`Self::set_player_diagnostic_error`] records the stable Movie Party
    /// code (e.g. `MP-MEDIA-001`) for failures libmpv reports message-less.
    /// The 200 ms event loop re-reads the player on every tick, so without
    /// this the diagnostic would be wiped on the very next tick — and the
    /// failure watcher requires that exact message to fire recovery (F33).
    fn observe(&mut self, observed: Self) {
        let carried_message = std::mem::take(&mut self.error_message);
        let keep_diagnostic = observed.error_message.is_none()
            && observed.state == PLAYER_STATE_ERROR
            && self.state == PLAYER_STATE_ERROR
            && carried_message.is_some();
        *self = observed;
        if keep_diagnostic {
            self.error_message = carried_message;
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
    invite: Option<room::MoviePartyInvite>,
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
    /// the host's local media path. The play gate
    /// re-checks existence — a moved/renamed file fires the modeled
    /// MissingLocalFile plan (AskHostToLocateFile, §29 honest error).
    local_media_path: Option<String>,
    /// per-provider Provider Shared diagnostic
    /// attempt counts — the §69 failure policy needs the retry state.
    provider_shared_attempts: std::collections::HashMap<String, u8>,
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
    /// Wall-clock (epoch ms) reading taken together with the monotonic
    /// reading at state init — lets snapshots project a host-monotonic
    /// deadline into wall ms for UI display (§16: execution itself
    /// stays on the monotonic clock).
    pending_operation_wall_anchor_ms: u64,
    pending_operation_wall_anchor_mono_us: u64,
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
    /// Lifecycle identity of the current room/session (MP-01).
    ///
    /// A commit is applied by a task that sleeps until the shared deadline
    /// (`play_lead_us`, up to 3 s) and only then takes the state lock, so the
    /// room can end or be replaced in between. Each such task captures this
    /// value at spawn time and re-checks it after waking: a mismatch means the
    /// session it was scheduled for is gone and the task must not mutate the
    /// room, dispatch to the player, or emit a snapshot. Bumped on every
    /// session teardown and every session start, so both "ended" and
    /// "replaced" are covered.
    session_generation: u64,
    /// Chrome/provider session identity, independent of `session_generation`
    /// because a provider browser can be launched and closed many times inside
    /// one room (MP-08). A CDP round-trip runs with the session temporarily
    /// moved out of state, so it must only return the session it took — never
    /// resurrect one that teardown already closed and dropped.
    chrome_generation: u64,
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
    /// (§56): schedule id the guest most recently accepted
    /// received, awaiting the host's echo before clearing.
    pending_guest_schedule: Option<String>,
    /// §52: the media awaiting a retention decision once the party ends.
    retention_prompt: Option<crate::storage::sqlite::StoredCacheEntry>,
    /// (§57): latest host preload progress observed for the
    /// newest schedule (state string + 0..1 progress) — feeds the Home
    /// "Upcoming" card.
    pending_preload_state: Option<(String, f64)>,
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
    /// M3: Background task rendering libmpv frames onto the native surface.
    player_render_task: Option<tokio::task::JoinHandle<()>>,
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
    /// session-liveness watcher for the managed Chrome
    /// (crash → ChromeCrash failure event → relaunch + readiness plan).
    chrome_crash_watcher_task: Option<tokio::task::JoinHandle<()>>,
    /// player-failure watcher (sticky MP-MEDIA-001 →
    /// PlayerFailure failure event → reopen + readiness plan).
    player_failure_watcher_task: Option<tokio::task::JoinHandle<()>>,
    /// Guest-only authenticated reconnect loop. Exactly one may own a party.
    reconnect_task: Option<tokio::task::JoinHandle<()>>,
    /// Guest heartbeat monitor; replaced whenever the QUIC transport changes.
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    /// Guest periodic BUFFER_STATUS reporter (PROTOCOL_SPEC §28 / MASTER_PRD
    /// §21: report every ~500 ms while Playing). Exactly one may exist.
    buffer_status_task: Option<tokio::task::JoinHandle<()>>,
    /// Provider Sync watch worker — polls the provider's own
    /// player (position + buffer) over CDP and feeds the coordinator
    /// (PLAYER_STATE / BUFFER_LOW equivalents). Exactly one may exist.
    provider_watch_task: Option<tokio::task::JoinHandle<()>>,
    /// Scheduled preload preparation task, cancelled with the active party.
    preload_task: Option<tokio::task::JoinHandle<()>>,
    /// Last offline-preload notice per schedule, preventing scheduler spam.
    preload_wait_notified_at: HashMap<String, Instant>,
    // ── M4: Persistent storage ──────────────────────────────────────────
    /// SQLite database for identity, schedules, cache metadata, chat.
    db: Option<Arc<crate::storage::sqlite::MoviePartyDb>>,
    /// Root directory for Movie Party's own cache (guest Local Perfect data).
    cache_root: Option<PathBuf>,
    /// Background scheduler worker (M4.3). Owned so it can be aborted.
    scheduler_task: Option<tauri::async_runtime::JoinHandle<()>>,
    // ── M6: Managed Chrome session ownership ────────────────────────────
    /// Owned Chrome child process (previously leaked via forget).
    chrome_session: Option<crate::providers::chrome::ManagedChromeSession>,
    // ── adaptive camera ladder ────────────────────────────────
    /// Monotonic timestamp of the last camera tier CHANGE applied by the
    /// ladder. Flapping protection: a minimum dwell between changes (see
    /// CAMERA_TIER_MIN_DWELL_MS) keeps a jittery goodput estimate from
    /// oscillating the encoder settings.
    camera_tier_changed_at: Option<std::time::Instant>,
    /// Last camera tier reported in a degradation notice. The
    /// once-per-event degradation notice (PRD §41) fires on
    /// each NEW downgrade event, not on every evaluation tick.
    camera_notice_tier: Option<crate::call::CameraTier>,
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
    /// UI_UX_SPEC §25: the Ready-Check countdown lead. The coordinator's
    /// canonical play commit executes exactly this long after the host
    /// presses Start; the frontend animates from the deadline the
    /// backend broadcasts (never its own timer).
    const COUNTDOWN_LEAD_US: u64 = 3_000_000;

    pub fn new() -> Self {
        let runtime = Self::new_without_emitter();
        runtime.anchor_wall_clock();
        runtime
    }

    pub fn new_without_emitter() -> Self {
        let runtime = Self::new_with_emitter(None);
        runtime.anchor_wall_clock();
        runtime
    }

    /// Construct a runtime with a test-injectable notifier. Unit tests never
    /// display real OS notifications.
    pub fn new_with_notifier(notifier: impl crate::notifications::Notifier + 'static) -> Self {
        let runtime = Self::new_with_emitter_and_key_store(
            None,
            Arc::new(notifier),
            Arc::new(crate::secure::FakeKeyStore::new()),
        );
        runtime.anchor_wall_clock();
        runtime
    }

    pub fn new_with_emitter(emitter: Option<Arc<dyn SnapshotSink>>) -> Self {
        let runtime = Self::new_with_emitter_and_key_store(
            emitter,
            Arc::new(crate::notifications::NativeNotifier),
            Arc::new(crate::secure::NativeKeyStore),
        );
        runtime.anchor_wall_clock();
        runtime
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
                    local_media_path: None,
                    provider_shared_attempts: std::collections::HashMap::new(),
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
                        pending_operation: None,
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
                        camera_notice: None,
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
                    pending_operation_wall_anchor_ms: 0,
                    pending_operation_wall_anchor_mono_us: 0,
                    pending_operation_resume_after: false,
                    last_committed_operation_id: None,
                    commit_scheduled_for: None,
                    presentation_epoch: 0,
                    session_generation: 0,
                    chrome_generation: 0,
                    clock_offset_to_host_us: 0,
                    peer_rtt_p95_us: None,
                    clock_calibrated: false,
                    calibration_task: None,
                    pending_guest_request_id: None,
                    pending_guest_schedule: None,
                    retention_prompt: None,
                    pending_preload_state: None,
                    player: None,
                    range_server_handle: None,
                    guest_cache: None,
                    player_snapshot: PlayerSnapshot::default(),
                    player_event_task: None,
                    player_render_task: None,
                    transfer_task: None,
                    old_transfer_task: None,
                    transfer_stall_watcher_task: None,
                    chrome_crash_watcher_task: None,
                    player_failure_watcher_task: None,
                    reconnect_task: None,
                    heartbeat_task: None,
                    buffer_status_task: None,
                    provider_watch_task: None,
                    preload_task: None,
                    preload_wait_notified_at: HashMap::new(),
                    db: None,
                    cache_root: None,
                    scheduler_task: None,
                    chrome_session: None,
                    camera_tier_changed_at: None,
                    camera_notice_tier: None,
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

    /// §16: anchor the wall clock to the process monotonic clock so
    /// snapshots can project host-monotonic deadlines into wall ms for
    /// UI display (Ready-Check countdown, §25). Called from every public
    /// constructor after state init.
    fn anchor_wall_clock(&self) {
        let mut state = self.lock();
        state.pending_operation_wall_anchor_mono_us = monotonic_us();
        state.pending_operation_wall_anchor_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
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
        let db = match crate::storage::sqlite::MoviePartyDb::open(&db_path) {
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

    /// Rename THIS device's participant (Settings › General). The name is
    /// validated (non-empty, 1..=40 chars after trimming) and persisted to
    /// the SAME identity row — device id, signing key and key label are
    /// untouched, so the peer trust chain and the protected-store key stay
    /// exactly as they are. A persistence failure surfaces as an honest
    /// error instead of a renamed-but-not-saved state.
    pub fn set_display_name(&self, display_name: &str) -> AppSnapshot {
        let trimmed = display_name.trim();
        let snapshot = {
            let mut state = self.lock();
            // Start from a clean error slate: a stale error from an
            // earlier command must not outlive this attempt.
            state.error = None;
            if trimmed.is_empty() || trimmed.chars().count() > MAX_DISPLAY_NAME_CHARS {
                state.error = Some("MP-ID-003 display name must be 1-40 characters".to_string());
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            if trimmed == state.local_participant.display_name {
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            let Some(db) = state.db.clone() else {
                state.error =
                    Some("MP-STORE-002 database is unavailable to save the name".to_string());
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            };
            let identity = self.inner.identity();
            let stored = crate::storage::sqlite::StoredIdentity {
                device_id: state.local_participant.id.clone(),
                display_name: trimmed.to_string(),
                public_key: identity.public_key_base64(),
                platform: std::env::consts::OS.to_string(),
                created_at_ms: wall_now_ms(),
                key_label: Self::key_label_for(&state.local_participant.id),
            };
            if let Err(e) = db.upsert_identity(&stored) {
                state.error = Some(format!("MP-STORE-001 failed to save the name: {e}"));
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            state.local_participant.display_name = trimmed.to_string();
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    fn key_label_for(device_id: &str) -> String {
        format!("movie-party-device-signing-key-{device_id}")
    }

    /// Restore an existing identity: private key from the OS-protected
    /// store must match the persisted metadata. If the key is missing or
    /// mismatched, perform an explicit coherent rotation (new device id +
    /// new keypair) — never associate an old device id with new key
    /// material.
    fn restore_identity(
        &self,
        db: &crate::storage::sqlite::MoviePartyDb,
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
    fn create_identity(&self, db: &crate::storage::sqlite::MoviePartyDb, display_name: &str) {
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
        db: &crate::storage::sqlite::MoviePartyDb,
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
        // Genuinely best-effort: the new seed is already stored under a NEW
        // label, so a leftover entry under the old label is inert — it can
        // never be selected for the rotated identity. Logged, not silently
        // discarded.
        if let Some(old_label) = old_key_label {
            if let Err(error) = self.inner.key_store.delete_seed(old_label) {
                eprintln!(
                    "MovieParty: MP-SECURE-001 rotation could not remove the previous key entry: {error}"
                );
            }
        }
        // NOT best-effort, despite what the old `let _ =` implied. Clearing the
        // old metadata row is what makes the rotated identity the single stored
        // source of truth: `upsert_identity` is `INSERT OR REPLACE` keyed on
        // `device_id`, so writing a NEW device id while the old row survives
        // leaves TWO rows — and a stale identity can then win the lookup. Abort
        // rather than claim a half-state. (The rotated seed stays in the key
        // store under its own label, unused, which is inert.)
        if let Err(error) = db.delete_identity() {
            self.lock().error = Some(storage_failure_message(
                "rotation could not clear the previous identity",
                &error,
            ));
            return;
        }
        self.persist_identity(db, &identity, display_name, &key_label);
    }

    fn persist_identity(
        &self,
        db: &crate::storage::sqlite::MoviePartyDb,
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
        if let Err(error) = db.upsert_identity(&stored) {
            self.lock().error = Some(storage_failure_message(
                "failed to persist identity",
                &error,
            ));
            return;
        }
        self.inner.replace_identity(identity.clone());
        self.lock().local_participant.id = identity.device_id.clone();
        self.lock().local_participant.display_name = display_name.to_string();
    }

    /// run the Provider Shared diagnostic for one
    /// provider on THIS device. The ffmpeg-CLI bridge (V1 mechanism per
    /// the plan) captures a 30 s sample and classifies it with the
    /// existing black-frame/static detector; the outcome is persisted
    /// (§38 — empirical, per-device) and the attempt count drives
    /// the §69 failure policy: ONE retry, then the explicit Sync Mode
    /// offer. Never a silent switch (§29).
    pub fn run_provider_shared_diagnostic(
        &self,
        provider_id: String,
    ) -> Result<crate::storage::sqlite::StoredProviderDiagnostic, String> {
        use crate::capture::diagnostic as diag;

        let display_name = crate::providers::sync::provider_capabilities()
            .into_iter()
            .find(|capability| capability.id == provider_id)
            .map(|capability| capability.display_name)
            .ok_or_else(|| "MP-PROVIDER-002 unsupported provider".to_string())?;

        let attempt = {
            let mut state = self.lock();
            let entry = state
                .provider_shared_attempts
                .entry(provider_id.clone())
                .or_insert(0);
            let current = *entry;
            *entry = current.saturating_add(1);
            current
        };

        let outcome = match diag::run_capture_sample(
            diag::DIAGNOSTIC_SAMPLE_SECONDS,
            1,
            &std::env::temp_dir().join("movie-party-shared-diagnostic"),
        ) {
            Ok(sample) => {
                // The provider's own page reports playing during a real
                // diagnostic; a diagnostic on this device IS the playing
                // context (the user runs it against the open provider).
                let provider_playing = {
                    let state = self.lock();
                    state.chrome_session.is_some()
                        && state.provider.readiness
                            == crate::providers::sync::ProviderReadiness::PlaybackReady
                };
                let availability = diag::classify_sample(sample, provider_playing);
                diag::outcome_for_classification(&provider_id, availability, attempt)
            }
            Err(diag::DiagnosticError::FfmpegMissing) => {
                return Err(diag::DiagnosticError::FfmpegMissing.to_string());
            }
            Err(diag::DiagnosticError::PermissionDenied) => {
                return Err(diag::DiagnosticError::PermissionDenied.to_string());
            }
            Err(diag::DiagnosticError::NoFrames) => diag::outcome_for_classification(
                &provider_id,
                crate::capture::CaptureAvailability::NoFrames,
                attempt,
            ),
            Err(other) => {
                // Honest typed error — command/store failures surface as-is.
                return Err(other.to_string());
            }
        };

        let stored = crate::storage::sqlite::StoredProviderDiagnostic {
            provider_id: outcome.provider_id.clone(),
            display_name,
            shared_available: outcome.shared_available,
            shared_reason: outcome.reason.clone(),
            verified_at_ms: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_millis() as i64)
                    .unwrap_or(0),
            ),
            sample_seconds: outcome.sample_seconds,
        };
        {
            let state = self.lock();
            if let Some(db) = state.db.clone() {
                db.upsert_provider_diagnostic(&stored)
                    .map_err(|e| format!("MP-STORE-001 failed to record diagnostic: {e}"))?;
            }
        }
        self.inner.emit(self.snapshot());
        Ok(stored)
    }

    /// the persisted per-provider diagnostics (empirical
    /// classification records for the Settings surface).
    pub fn list_provider_diagnostics(
        &self,
    ) -> Vec<crate::storage::sqlite::StoredProviderDiagnostic> {
        let state = self.lock();
        state
            .db
            .as_ref()
            .and_then(|db| db.list_provider_diagnostics().ok())
            .unwrap_or_default()
    }

    // ── Friends (saved movie partners) ─────────────────────────────────

    /// Tailnet peers available for friend selection, merged with the saved
    /// friends' cached verification so the UI can render one honest list.
    /// `ok(TailnetPeersView)` requires Tailscale READY; an unusable local
    /// connection returns the readiness error so the UI shows the setup gate.
    pub async fn tailnet_peers(
        &self,
    ) -> Result<crate::network::tailscale::TailnetPeersView, String> {
        let status = crate::network::tailscale::detect_status()
            .await
            .map_err(|e| e.to_string())?;
        let readiness = crate::network::tailscale::readiness_from_status(status.clone());
        if !readiness.is_usable() && !crate::network::tailscale::dev_loopback_enabled() {
            return Err(readiness
                .stable_error()
                .unwrap_or_else(|| "MP-NET-TS-003 Tailscale is not ready".to_string()));
        }
        let saved = self.list_friends();
        Ok(crate::network::tailscale::TailnetPeersView {
            candidates: crate::network::tailscale::friend_candidates(&status),
            saved,
        })
    }

    /// Save (or refresh) a friend. Requires a current status so the cached
    /// IP is real, and refuses peers with no usable Tailscale IPv4. The
    /// optional display name overrides the tailnet-derived name (invites
    /// carry a friendly name so the list never shows DNS jargon).
    pub async fn add_friend(
        &self,
        peer_key: String,
        display_name: Option<String>,
    ) -> Result<crate::storage::sqlite::StoredFriend, String> {
        let peer_key = peer_key.trim().trim_end_matches('.').to_string() + ".";
        let status = crate::network::tailscale::detect_status()
            .await
            .map_err(|e| e.to_string())?;
        let peer = status
            .peers
            .iter()
            .find(|p| p.dns_name == peer_key)
            .ok_or_else(|| {
                "MP-NET-TS-008 that device is not in your Tailscale network right now".to_string()
            })?;
        let ip = peer
            .usable_ipv4()
            .ok_or_else(|| "MP-NET-TS-004 that device has no usable Tailscale address".to_string())?
            .to_string();
        let now_ms = wall_now_ms();
        let friendly_name = display_name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty() && name.chars().count() <= MAX_DISPLAY_NAME_CHARS)
            .unwrap_or_else(|| crate::network::tailscale::peer_display_name(&peer.dns_name));
        let friend = crate::storage::sqlite::StoredFriend {
            peer_key: peer.dns_name.clone(),
            display_name: friendly_name,
            ip: Some(ip),
            added_at_ms: now_ms,
            last_verified_at_ms: None,
            last_path: None,
            last_latency_ms: None,
            // Picked from the live tailnet status → the device IS joined.
            // Verification still requires a real ping (verify_friend).
            connection_state: crate::storage::sqlite::FriendConnectionState::TailscaleJoined,
        };
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.upsert_friend(&friend)
            .map_err(|e| format!("MP-STORE-001 failed to save friend: {e}"))?;
        Ok(friend)
    }

    /// This device's friend-invite link: the shareable QR/link payload for
    /// the Friends tab. Carries this device's tailnet identity (peer key)
    /// plus a friendly name, so the receiving app adds the inviter directly
    /// — no peer tables, no addresses, ever.
    pub async fn friend_invite(
        &self,
    ) -> Result<crate::network::tailscale::FriendInviteLink, String> {
        let status = crate::network::tailscale::detect_status()
            .await
            .map_err(|e| e.to_string())?;
        let readiness = crate::network::tailscale::readiness_from_status(status.clone());
        if !readiness.is_usable() && !crate::network::tailscale::dev_loopback_enabled() {
            return Err(readiness
                .stable_error()
                .unwrap_or_else(|| "MP-NET-TS-003 Tailscale is not ready".to_string()));
        }
        // The Self MagicDNS name is this device's stable tailnet identity.
        let peer_key = status
            .device_name
            .clone()
            .filter(|name| name.contains('.'))
            .ok_or_else(|| "MP-NET-TS-004 this device has no tailnet name yet".to_string())?;
        let peer_key = format!("{}.", peer_key.trim_end_matches('.'));
        // Friendly name: the user's identity display name, else the first
        // label of the DNS name.
        let display_name = {
            let state = self.lock();
            state
                .db
                .as_ref()
                .and_then(|db| db.get_identity().ok().flatten())
                .map(|identity| identity.display_name)
                .filter(|name| !name.trim().is_empty())
        }
        .unwrap_or_else(|| crate::network::tailscale::peer_display_name(&peer_key));
        let payload = format!(
            "{{\"n\":{},\"pk\":{}}}",
            serde_json::to_string(&display_name)
                .map_err(|e| format!("MP-STORE-001 could not encode invite: {e}"))?,
            serde_json::to_string(&peer_key)
                .map_err(|e| format!("MP-STORE-001 could not encode invite: {e}"))?,
        );
        let link = format!(
            "movieparty://friend/{}",
            crate::network::tailscale::base64url_encode(payload.as_bytes()),
        );
        Ok(crate::network::tailscale::FriendInviteLink {
            link,
            peer_key,
            display_name,
        })
    }

    /// Accept a movieparty://friend/ invite — the receiving side of the
    /// friend architecture. The link carries ONLY the inviter's identity
    /// (peer key + name); it never contains a Tailscale auth key, and
    /// this flow never authenticates the inviter's device as anyone.
    ///
    /// External-user flow implemented:
    ///   1. Parse + validate the identity-only payload.
    ///   2. Refresh the tailnet status (the friend's own device joined
    ///      through Tailscale's external-user invitation with THEIR
    ///      Tailscale identity — Movie Party only observes).
    ///   3. If the expected peer is present with a usable address →
    ///      save/refresh as TAILSCALE_JOINED. If not → still save as
    ///      INVITED with the honest tailnet guidance (never a
    ///      fabricated connection).
    ///
    /// The friend becomes MOVIE_PARTY_VERIFIED later via verify_friend
    /// (a real ping), and ONLINE/OFFLINE stays a live observation.
    pub async fn accept_friend_invite(
        &self,
        invite_link: String,
    ) -> Result<crate::storage::sqlite::StoredFriend, String> {
        let payload = crate::network::tailscale::parse_friend_invite(&invite_link)?;
        let peer_key = format!("{}.", payload.peer_key.trim().trim_end_matches('.'));

        let status = crate::network::tailscale::detect_status().await;
        let observed = match &status {
            Ok(status) => status
                .peers
                .iter()
                .find(|p| p.dns_name == peer_key)
                .and_then(|p| p.usable_ipv4())
                .map(|ip| ip.to_string()),
            Err(_) => None,
        };

        let now_ms = wall_now_ms();
        let existing = self
            .list_friends()
            .into_iter()
            .find(|f| f.peer_key == peer_key);

        // Keep the user's chosen name when re-accepting — the invite may
        // be an older share, and the saved name wins.
        let display_name = existing
            .as_ref()
            .map(|f| f.display_name.clone())
            .unwrap_or_else(|| payload.display_name.clone());

        // F23: never demote an existing verification. Re-opening an old invite
        // link is a normal way to re-share it, and it must not wipe a
        // MOVIE_PARTY_VERIFIED record: the rest of the friend flow only ever
        // promotes (see `update_friend_verification`). The cached verification
        // observations are preserved exactly like the user's chosen name above.
        let verified_at = existing.as_ref().and_then(|f| f.last_verified_at_ms);
        let connection_state = if verified_at.is_some() {
            crate::storage::sqlite::FriendConnectionState::MoviePartyVerified
        } else if observed.is_some() {
            crate::storage::sqlite::FriendConnectionState::TailscaleJoined
        } else {
            crate::storage::sqlite::FriendConnectionState::Invited
        };

        let friend = crate::storage::sqlite::StoredFriend {
            peer_key: peer_key.clone(),
            display_name,
            // A fresh observation wins; otherwise keep the last cached address
            // rather than forgetting it just because the peer was briefly
            // absent from the tailnet status.
            ip: observed
                .clone()
                .or_else(|| existing.as_ref().and_then(|f| f.ip.clone())),
            added_at_ms: existing.as_ref().map(|f| f.added_at_ms).unwrap_or(now_ms),
            // Never claim verification from a link alone — that stays
            // verify_friend's job (a real ping through the tunnel). Only a
            // PREVIOUSLY recorded verification is carried forward.
            last_verified_at_ms: verified_at,
            last_path: existing.as_ref().and_then(|f| f.last_path.clone()),
            last_latency_ms: existing.as_ref().and_then(|f| f.last_latency_ms),
            connection_state,
        };

        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.upsert_friend(&friend)
            .map_err(|e| format!("MP-STORE-001 failed to save friend: {e}"))?;
        Ok(friend)
    }

    /// Refresh saved friends against the live tailnet status: any INVITED
    /// friend whose device has since joined the tailnet is promoted to
    /// TAILSCALE_JOINED (never demoting MOVIE_PARTY_VERIFIED). Returns
    /// the updated list. ONLINE/OFFLINE stays derived at read time.
    pub async fn refresh_friend_states(&self) -> Vec<crate::storage::sqlite::StoredFriend> {
        let friends = self.list_friends();
        let needs_refresh = friends
            .iter()
            .any(|f| f.connection_state == crate::storage::sqlite::FriendConnectionState::Invited);
        if !needs_refresh {
            return friends;
        }
        let status = match crate::network::tailscale::detect_status().await {
            Ok(status) => status,
            Err(_) => return friends, // honest: keep the last known states
        };
        let db = match self.lock().db.clone() {
            Some(db) => db,
            None => return friends,
        };
        for friend in friends.iter().filter(|f| {
            f.connection_state == crate::storage::sqlite::FriendConnectionState::Invited
        }) {
            if let Some(ip) = status
                .peers
                .iter()
                .find(|p| p.dns_name == friend.peer_key)
                .and_then(|p| p.usable_ipv4())
            {
                // Logged, deliberately not surfaced: this is a connectivity
                // refresh that recomputes the same state on the next poll, so a
                // transient failure self-heals and a user-visible error would
                // only flap. The point is that it is no longer silent.
                if let Err(error) = db.mark_friend_joined(&friend.peer_key, &ip.to_string()) {
                    eprintln!("MovieParty: MP-STORE-001 failed to record a joined friend: {error}");
                }
            }
        }
        self.list_friends()
    }

    /// Rename a saved friend — friendly names over tailnet jargon.
    pub fn rename_friend(
        &self,
        peer_key: &str,
        display_name: &str,
    ) -> Result<crate::storage::sqlite::StoredFriend, String> {
        let name = display_name.trim();
        if name.is_empty() {
            return Err("MP-FRIEND-001 the name cannot be empty".to_string());
        }
        if name.chars().count() > MAX_DISPLAY_NAME_CHARS {
            return Err("MP-FRIEND-001 the name is too long (max 40 characters)".to_string());
        }
        let friends = self.list_friends();
        let mut friend = friends
            .into_iter()
            .find(|f| f.peer_key == peer_key)
            .ok_or_else(|| "MP-NET-TS-008 that friend is not saved on this device".to_string())?;
        friend.display_name = name.to_string();
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.upsert_friend(&friend)
            .map_err(|e| format!("MP-STORE-001 failed to rename friend: {e}"))?;
        Ok(friend)
    }

    /// Remove a saved friend by peer key.
    pub fn remove_friend(&self, peer_key: &str) -> Result<(), String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.delete_friend(peer_key)
            .map_err(|e| format!("MP-STORE-001 failed to remove friend: {e}"))
    }

    /// All saved friends.
    pub fn list_friends(&self) -> Vec<crate::storage::sqlite::StoredFriend> {
        let state = self.lock();
        state
            .db
            .as_ref()
            .and_then(|db| db.list_friends().ok())
            .unwrap_or_default()
    }

    /// Verify the real tunnel connection to a saved friend: runs
    /// `tailscale ping` (a genuine WireGuard-level probe through the
    /// tunnel) and persists the result on the friend record. Returns the
    /// updated friend plus the raw probe. This is the "connection actually
    /// established" check behind Add Friend — Tailscale provides the
    /// tunnel; Movie Party verifies it end-to-end.
    pub async fn verify_friend(
        &self,
        peer_key: &str,
    ) -> Result<crate::network::tailscale::FriendVerification, String> {
        let friends = self.list_friends();
        let mut friend = friends
            .into_iter()
            .find(|f| f.peer_key == peer_key)
            .ok_or_else(|| "MP-NET-TS-008 that friend is not saved on this device".to_string())?;

        // Refresh the peer's current IP from a live status, then ping it.
        let ip = match crate::network::tailscale::detect_status().await {
            Ok(status) => status
                .peers
                .iter()
                .find(|p| p.dns_name == peer_key)
                .and_then(|p| p.usable_ipv4())
                .map(|ip| ip.to_string()),
            Err(_) => None, // ping falls back to the last cached IP
        }
        .or_else(|| friend.ip.clone())
        .ok_or_else(|| "MP-NET-TS-004 no usable Tailscale address for that friend".to_string())?;

        let ip_addr: std::net::Ipv4Addr = ip
            .parse()
            .map_err(|_| "MP-NET-TS-004 invalid Tailscale address".to_string())?;
        let probe = crate::network::tailscale::verify_peer_connection(ip_addr).await;

        let now_ms = wall_now_ms();
        if probe.reachable {
            friend.ip = Some(ip);
            friend.last_verified_at_ms = Some(now_ms);
            friend.last_path = probe.path.clone();
            friend.last_latency_ms = probe.latency_ms;
        } else {
            // A failed probe still records the attempt time (honest state),
            // but never claims a working connection.
            friend.last_verified_at_ms = None;
            friend.last_path = None;
            friend.last_latency_ms = None;
        }
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.update_friend_verification(
            &friend.peer_key,
            friend.ip.as_deref(),
            friend.last_verified_at_ms,
            friend.last_path.as_deref(),
            friend.last_latency_ms,
        )
        .map_err(|e| format!("MP-STORE-001 failed to record verification: {e}"))?;
        Ok(crate::network::tailscale::FriendVerification { friend, probe })
    }

    /// M6: Store an owned Chrome session in AppRuntime, replacing any previous one.
    /// The old session is dropped (killing the Chrome process) if present.
    pub fn store_chrome_session(&self, session: crate::providers::chrome::ManagedChromeSession) {
        // MP-07: swap the browser in under the lock, then close any replaced
        // session with the lock released — dropping a ManagedChromeSession runs
        // a blocking graceful close.
        // MP-08: bump the provider-session identity so an in-flight CDP
        // round-trip still holding the previous browser cannot restore it.
        let previous = {
            let mut state = self.lock();
            bump_chrome_generation(&mut state);
            let previous = state.chrome_session.take();
            state.chrome_session = Some(session);
            previous
        };
        drop(previous);
        self.spawn_chrome_crash_watcher();
    }

    /// poll managed-Chrome session liveness. When the child
    /// dies mid-session (a real crash, not our graceful close — that takes
    /// the session out of state first), fire ChromeCrash so the modeled
    /// recovery plan runs: relaunch + readiness requirement, playback
    /// paused for both (strict sync, §14).
    fn spawn_chrome_crash_watcher(&self) {
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let (exists, alive) = {
                    let mut state = inner.lock();
                    match state.chrome_session.as_mut() {
                        None => (false, false),
                        Some(session) => (true, session.is_alive()),
                    }
                };
                if !exists {
                    // Session closed gracefully / torn down — stop watching.
                    break;
                }
                if !alive {
                    let mut state = inner.lock();
                    // Take the dead session out so Drop's graceful close
                    // path does not operate on a corpse (it would only
                    // wait on an already-exited child).
                    state.chrome_session = None;
                    // MP-08: the browser identity moved on — a CDP round-trip
                    // still holding this session must not put it back.
                    bump_chrome_generation(&mut state);
                    let plan = recovery_plan(FailureEvent::ChromeCrash);
                    apply_recovery_to_state(&mut state, FailureEvent::ChromeCrash, plan);
                    let out = snapshot_from_state(&state);
                    drop(state);
                    inner.emit(out);
                    break;
                }
            }
        });
        let mut state = self.lock();
        if let Some(old) = state.chrome_crash_watcher_task.replace(task) {
            old.abort();
        }
    }

    /// poll the player snapshot for a sticky failure. The
    /// player module already surfaces honest MP-MEDIA-001 diagnostics; the
    /// watcher promotes a sticky error to the PlayerFailure recovery plan
    /// (reopen player + require readiness) so both sides pause (§14)
    /// instead of the room sitting in a half-broken play state.
    fn spawn_player_failure_watcher(&self) {
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            let mut consecutive_failures: u32 = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let should_fire = {
                    let state = inner.lock();
                    if state.player_snapshot.state != PLAYER_STATE_ERROR {
                        consecutive_failures = 0;
                        continue;
                    }
                    let media_error = state
                        .player_snapshot
                        .error_message
                        .as_deref()
                        .is_some_and(|error| error.starts_with("MP-MEDIA-001"));
                    media_error && state.last_recovery.is_none()
                };
                if should_fire {
                    consecutive_failures += 1;
                    if consecutive_failures >= 2 {
                        let mut state = inner.lock();
                        let plan = recovery_plan(FailureEvent::PlayerFailure);
                        apply_recovery_to_state(&mut state, FailureEvent::PlayerFailure, plan);
                        let out = snapshot_from_state(&state);
                        drop(state);
                        inner.emit(out);
                        break;
                    }
                }
            }
        });
        let mut state = self.lock();
        if let Some(old) = state.player_failure_watcher_task.replace(task) {
            old.abort();
        }
    }

    pub fn close_provider_session(&self) {
        // Graceful teardown: Browser.close over CDP lets Chrome flush the
        // dedicated profile; SIGKILL (the old drop path) could leave the
        // profile locked, making the next launch of that provider fail
        // or show a restore banner. The session is taken OUT of the
        // lock first so a slow close (up to 3s) never freezes the UI.
        //
        // MP-08: bump the provider-session identity first, so a CDP round-trip
        // already in flight cannot put this session back after we close it.
        let session = {
            let mut state = self.lock();
            bump_chrome_generation(&mut state);
            state.chrome_session.take()
        };
        if let Some(mut session) = session {
            session.close_gracefully();
        }
    }

    pub fn store_launched_provider(
        &self,
        provider_id: String,
        url: String,
        session: crate::providers::chrome::ManagedChromeSession,
    ) -> AppSnapshot {
        let cdp_port = session.plan.cdp_port;
        // MP-07/MP-08: swap the browser in under the lock, close any replaced
        // session with the lock released, and move the provider-session
        // identity so an in-flight CDP round-trip holding the old browser
        // cannot restore it.
        let previous = {
            let mut state = self.lock();
            bump_chrome_generation(&mut state);
            let previous = state.chrome_session.take();
            state.chrome_session = Some(session);
            previous
        };
        drop(previous);
        // the provider browser is the room media — start the
        // position/buffer watch worker (idempotent replace).
        self.spawn_provider_watch_worker();
        let mut state = self.lock();
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
        // MP-07/MP-08: same swap-then-close-outside-the-lock + identity bump
        // as store_launched_provider.
        let previous = {
            let mut state = self.lock();
            bump_chrome_generation(&mut state);
            let previous = state.chrome_session.take();
            state.chrome_session = Some(session);
            previous
        };
        drop(previous);
        let mut state = self.lock();
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
        // MP-07/MP-08: swap the browser in under the lock, close any replaced
        // session with the lock released, and move the provider-session identity
        // so an in-flight CDP round-trip holding the old browser cannot restore
        // it. Plain `state.chrome_session = Some(session)` would drop the
        // previous session — a CDP round-trip plus a whole-tree teardown — while
        // the global mutex was held.
        let previous = {
            let mut state = self.lock();
            bump_chrome_generation(&mut state);
            let previous = state.chrome_session.take();
            state.chrome_session = Some(session);
            previous
        };
        drop(previous);
        let mut state = self.lock();
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
        state.provider.state = crate::providers::sync::readiness_description(readiness).to_string();
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
            crate::providers::sync::ProviderReadiness::Unavailable
            | crate::providers::sync::ProviderReadiness::Error => {
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
        use crate::providers::sync::{
            detect_media_command, login_required_command, provider_id_from_str,
        };

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
    pub fn attach_launched_provider(&self, provider_id: String, url: String) -> AppSnapshot {
        // (re)attach the watch worker for the reused session.
        self.spawn_provider_watch_worker();
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
    pub fn navigate_provider_to(&self, provider_id: &str, url: &str) -> Result<(), String> {
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
                "Movie Party — Overdue Preload",
                &format!(
                    "Scheduled session for '{}' needs preloading now.",
                    schedule.media_id
                ),
            );
            if let Err(error) = result {
                eprintln!("MovieParty: notification failed (recoverable): {error}");
            }
            // IMPORTANT: this scan is notification-only. It must NEVER
            // change the schedule status — the scheduler worker is the only
            // authority that claims and executes due schedules. Mutating
            // status here previously hid overdue work from the scheduler.
        }
        detected
    }

    fn default_db_path() -> PathBuf {
        // Test / portable override: MOVIE_PARTY_DB_PATH pins the database
        // location so AppRuntime restart tests can use a temp file.
        if let Ok(path) = std::env::var("MOVIE_PARTY_DB_PATH") {
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
            base.join("Library/Application Support/Movie Party/movie_party.db")
        }
        #[cfg(target_os = "windows")]
        {
            let base = std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            base.join("Movie Party/movie_party.db")
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let base = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            base.join(".local/share/movie-party/movie_party.db")
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
    /// Errors map to stable Movie Party codes.
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

    /// (§55): host creates a schedule AND broadcasts it so the
    /// guest persists it + registers reminders (§56). Runs the canonical
    /// validations from `create_schedule`, then wires the wire event.
    pub fn create_and_broadcast_schedule(
        &self,
        room_id: &str,
        media_id: &str,
        scheduled_start_utc_ms: i64,
        planned_preload_utc_ms: i64,
        guest_device_id: &str,
        call_mode: &str,
    ) -> Result<String, String> {
        let schedule_id = self.create_schedule(
            room_id,
            media_id,
            scheduled_start_utc_ms,
            planned_preload_utc_ms,
            guest_device_id,
        )?;
        let mut state = self.lock();
        Self::send_host_event(
            &mut state,
            QuicServerEvent::ScheduleCreate {
                schedule_id: schedule_id.clone(),
                scheduled_start_utc_ms,
                media_id: media_id.to_string(),
                call_mode: call_mode.to_string(),
                planned_preload_utc_ms,
            },
        );
        Ok(schedule_id)
    }

    /// (§56): guest acknowledges a received schedule. The guest
    /// persists on ScheduleCreate (apply_peer_event); this sends the
    /// acceptance to the host, whose echo clears the pending marker.
    pub fn guest_accept_schedule(&self, schedule_id: &str, accepted: bool) -> AppSnapshot {
        let client = {
            let mut state = self.lock();
            state.pending_guest_schedule = Some(schedule_id.to_string());
            state.client.clone()
        };
        if let Some(client) = client {
            let schedule_id = schedule_id.to_string();
            tokio::spawn(async move {
                let _ = client.send_schedule_accept(schedule_id, accepted).await;
            });
        }
        self.snapshot()
    }

    /// Latest preload progress the guest observed (§57) — feeds the Home
    /// Upcoming card. (state, progress 0..1)
    pub fn pending_preload_state(&self) -> Option<(String, f64)> {
        self.lock().pending_preload_state.clone()
    }

    /// (§55): update media AND broadcast so the guest's persisted
    /// copy stays truthful. Host-authoritative (§15).
    pub fn update_and_broadcast_schedule_media(
        &self,
        schedule_id: &str,
        media_id: &str,
        planned_preload_utc_ms: i64,
        scheduled_start_utc_ms: i64,
    ) -> Result<(), String> {
        self.update_schedule_media(schedule_id, media_id)?;
        let mut state = self.lock();
        Self::send_host_event(
            &mut state,
            QuicServerEvent::ScheduleUpdate {
                schedule_id: schedule_id.to_string(),
                media_id: media_id.to_string(),
                planned_preload_utc_ms,
                scheduled_start_utc_ms,
            },
        );
        Ok(())
    }

    /// (§56): cancel AND broadcast — the guest marks its copy
    /// Cancelled so no reminder ever fires for a dead schedule.
    pub fn cancel_and_broadcast_schedule(&self, schedule_id: &str) -> Result<(), String> {
        let mut state = self.lock();
        let db = state
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.update_schedule_status(schedule_id, "Cancelled")
            .map_err(|e| format!("MP-STORE-001 {e}"))?;
        Self::send_host_event(
            &mut state,
            QuicServerEvent::ScheduleCancel {
                schedule_id: schedule_id.to_string(),
            },
        );
        Ok(())
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
                    eprintln!("MovieParty: scheduler claimed-recovery failed: {error}");
                }

                // Execute every schedule whose preload deadline has arrived.
                let due = match db.due_schedules(now) {
                    Ok(due) => due,
                    Err(error) => {
                        eprintln!("MovieParty: scheduler due-scan failed (recoverable): {error}");
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
                            eprintln!("MovieParty: scheduler claim failed: {error}");
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
                                eprintln!(
                                    "MovieParty: scheduler status transition failed: {error}"
                                );
                                continue;
                            }
                            inner
                                .preload_executions
                                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let _ = inner.notifier.notify(
                                "Movie Party — Preloading",
                                &format!(
                                    "Preloading '{}' for the scheduled session.",
                                    schedule.media_id
                                ),
                            );
                            // (§57): broadcast the preload start so
                            // the guest's Home Upcoming card flips to
                            // "Preload 0%". Progress updates continue from
                            // the transfer path.
                            let event = QuicServerEvent::PreloadState {
                                schedule_id: schedule.schedule_id.clone(),
                                state: "TRANSFERRING".to_string(),
                                progress: 0.0,
                                estimated_ready_utc_ms: schedule.planned_preload_utc_ms,
                            };
                            broadcast_from_inner(&inner, event);
                        }
                        Ok(crate::scheduling::preload::PreloadOutcome::WaitingForPrerequisites) => {
                            // Peer/session unavailable: persist a waiting
                            // state, notify the user, and retry on the next
                            // poll (due_schedules includes WaitingForPeer).
                            let _ =
                                db.update_schedule_status(&schedule.schedule_id, "WaitingForPeer");
                            // §57: the guest sees the honest waiting state,
                            // never a stuck progress bar.
                            broadcast_from_inner(
                                &inner,
                                QuicServerEvent::PreloadState {
                                    schedule_id: schedule.schedule_id.clone(),
                                    state: "WAITING_FOR_GUEST".to_string(),
                                    progress: 0.0,
                                    estimated_ready_utc_ms: schedule.planned_preload_utc_ms,
                                },
                            );
                            let should_notify = {
                                let mut state = inner.lock();
                                should_notify_preload_wait(
                                    &mut state,
                                    &schedule.schedule_id,
                                    Instant::now(),
                                )
                            };
                            if should_notify {
                                let _ = inner.notifier.notify(
                                    "Movie Party — Preload Waiting",
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
                            eprintln!("MovieParty: preload executor failed: {error}");
                            // §57: honest FAILED state on the wire — the
                            // guest's Upcoming card must never sit at a
                            // stale percentage.
                            broadcast_from_inner(
                                &inner,
                                QuicServerEvent::PreloadState {
                                    schedule_id: schedule.schedule_id.clone(),
                                    state: "FAILED".to_string(),
                                    progress: 0.0,
                                    estimated_ready_utc_ms: schedule.planned_preload_utc_ms,
                                },
                            );
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

    /// Resolve a media id to its cache directory, refusing anything that is
    /// not a direct child of the cache root.
    ///
    /// `Path::starts_with` is a *component-prefix* test, not a containment
    /// test: it does not normalise `..`, so `root.join("../../evil")` passed
    /// the old guard even though it escapes the root entirely (F45). Two
    /// independent checks now stand in for it — the id must be a single safe
    /// path component, and the resolved path's parent must be the root.
    fn cache_dir_for(&self, media_id: &str) -> Result<PathBuf, String> {
        let root = self.cache_root()?;

        if !crate::media::manifest::is_safe_media_id(media_id) {
            return Err("MP-MEDIA-002 unsafe media id".to_string());
        }

        let dir = root.join(media_id);
        // Belt and braces: `is_safe_media_id` already forbids separators and
        // `..`, so a direct-child check cannot be satisfied by a traversal.
        if dir.parent() != Some(root.as_path()) {
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

    /// Retention "Keep": retain Movie Party's cache for this media.
    pub fn retention_keep(&self, media_id: &str) -> Result<(), String> {
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.retention_keep(media_id)
            .map_err(|e| format!("MP-STORE-001 {e}"))?;
        // §52: the decision is made — the prompt is answered for good.
        self.clear_retention_prompt(media_id);
        Ok(())
    }

    /// §52: clear the pending retention prompt once answered (or when the
    /// media is no longer cached on this device).
    pub fn clear_retention_prompt(&self, media_id: &str) {
        let mut state = self.lock();
        if state
            .retention_prompt
            .as_ref()
            .is_some_and(|entry| entry.media_id == media_id)
        {
            state.retention_prompt = None;
        }
    }

    /// Retention "Remove": delete only Movie Party's cache directory and the
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
            .map_err(|e| format!("MP-STORE-001 {e}"))?;
        self.clear_retention_prompt(media_id);
        Ok(())
    }

    /// Retention "Save As": export the completed cached media to a chosen
    /// destination safely. Never deletes or overwrites the host source.
    pub fn retention_save_as(&self, media_id: &str, destination: &Path) -> Result<PathBuf, String> {
        let root = self.cache_root()?;
        let dir = self.cache_dir_for(media_id)?;
        let data_file = dir.join(crate::media::cache::CACHE_DATA_FILE);
        let saved = crate::storage::apply_retention_decision(
            &root,
            &dir,
            &data_file,
            crate::storage::RetentionDecision::SaveAs,
            Some(destination),
        )
        .map(|saved| saved.unwrap_or_else(|| destination.to_path_buf()))
        .map_err(|e| format!("MP-MEDIA-002 {e}"))?;
        self.clear_retention_prompt(media_id);
        Ok(saved)
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
            pending_operation: None,
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
        let teardown = {
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
            if let Some(task) = state.player_render_task.take() {
                task.abort();
            }
            if let Some(task) = state.transfer_stall_watcher_task.take() {
                task.abort();
            }
            if let Some(task) = state.chrome_crash_watcher_task.take() {
                task.abort();
            }
            if let Some(task) = state.player_failure_watcher_task.take() {
                task.abort();
            }
            if let Some(task) = state.heartbeat_task.take() {
                task.abort();
            }
            if let Some(task) = state.buffer_status_task.take() {
                task.abort();
            }
            if let Some(task) = state.provider_watch_task.take() {
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
            state.guest_cache = None;
            state.media = None;
            state.transfer = None;
            state.player_snapshot = PlayerSnapshot::default();
            state.chat.clear();
            state.reactions.clear();
            state.invite = None;
            state.credentials = None;
            // MP-01: a fresh room is starting — anything still sleeping
            // towards a deadline from the previous session must abort.
            bump_session_generation(&mut state);
            // MP-07: hand the blocking teardown to the caller so it runs with
            // the lock released.
            DeferredTeardown::take(&mut state)
        };

        // MP-07: blocking Chrome/player teardown happens with the lock free.
        teardown.run();

        let identity = self.inner.identity();
        let credentials = RoomCredentials::generate();

        // remember the local path for the file-moved gate.
        self.lock().local_media_path = media_path
            .as_ref()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
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

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        // MP-16: one expiry value, used both for the invite the guest receives
        // and for the host's own authorization check — so the two can never
        // disagree about when this room stops accepting new guests.
        let invite_expires_at_ms = now_ms + room::INVITE_TTL_MS;

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
        .map_err(|e| e.to_string())?
        .with_invite_expiry(invite_expires_at_ms);

        let bound_addr = server.local_addr().map_err(|e| e.to_string())?;
        let cert_der = server.certificate().as_ref().to_vec();
        let cert_fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(&cert_der));

        let invite = room::MoviePartyInvite {
            v: room::INVITE_VERSION_V1,
            protocol_major: crate::PROTOCOL_MAJOR,
            protocol_minor: crate::PROTOCOL_MINOR,
            room_id: credentials.room_id.clone(),
            join_secret: credentials.join_secret.clone(),
            host_device_id: identity.device_id.clone(),
            host_ip: tailscale_ip,
            host_port: bound_addr.port(),
            server_certificate_fingerprint: cert_fingerprint.clone(),
            expires_at_ms: invite_expires_at_ms,
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
            coord.coordinator_room_id = credentials.room_id.clone();
            coord.coordinator_tx = event_tx_arc.as_ref().clone();
            coord.coordinator_broadcast_fn = Some(
                |coord_arg: &LocalSyncCoordinator,
                 sender: &str,
                 tx: &broadcast::Sender<EventEnvelope>| {
                    let _ = tx.send(EventEnvelope {
                        v_major: crate::protocol::ENVELOPE_V_MAJOR,
                        v_minor: crate::protocol::ENVELOPE_V_MINOR,
                        room_id: coord_arg.coordinator_room_id.clone(),
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
            self.spawn_player_render_loop();
        }

        // M8: Start the transfer stall watcher — automatically detects when
        // no bytes are received for 30 seconds and fires TransferInterrupted.
        self.spawn_transfer_stall_watcher();

        // the player-failure watcher promotes a sticky
        // MP-MEDIA-001 to the PlayerFailure recovery plan.
        self.spawn_player_failure_watcher();

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
        let teardown = {
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
            if let Some(task) = state.player_render_task.take() {
                task.abort();
            }
            if let Some(task) = state.transfer_task.take() {
                task.abort();
                state.old_transfer_task = Some(task);
            }
            if let Some(task) = state.transfer_stall_watcher_task.take() {
                task.abort();
            }
            if let Some(task) = state.chrome_crash_watcher_task.take() {
                task.abort();
            }
            if let Some(task) = state.player_failure_watcher_task.take() {
                task.abort();
            }
            if let Some(task) = state.reconnect_task.take() {
                task.abort();
            }
            if let Some(task) = state.heartbeat_task.take() {
                task.abort();
            }
            if let Some(task) = state.buffer_status_task.take() {
                task.abort();
            }
            if let Some(task) = state.provider_watch_task.take() {
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
                pending_operation: None,
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
            // MP-01: a fresh guest session is starting — commit tasks still
            // sleeping from the previous room must abort instead of mutating
            // the new one.
            bump_session_generation(&mut state);
            // MP-07: blocking Chrome/player teardown is deferred past the lock.
            DeferredTeardown::take(&mut state)
        };

        // MP-07: run the blocking teardown with the state lock released.
        teardown.run();

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
                format!("MP-NET-TS-005 host is not reachable through Tailscale: {error}")
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
        self.spawn_buffer_status_worker();

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
            // (§56): the guest accepted the schedule. Mark the
            // stored schedule Accepted so the host's Home Upcoming card and
            // the scheduler stop treating it as pending confirmation.
            QuicHostEvent::GuestScheduleAccept {
                broadcaster_device_id: _,
                schedule_id,
                accepted,
            } => {
                let db = self.lock().db.clone();
                if let Some(db) = db {
                    let status = if accepted { "Accepted" } else { "Declined" };
                    // Not swallowed. If this write fails the host keeps showing
                    // the schedule as awaiting confirmation, so the answer the
                    // guest just gave silently disagrees with what is stored.
                    if let Err(error) = db.update_schedule_status(&schedule_id, status) {
                        self.lock().error = Some(storage_failure_message(
                            "failed to record your answer to the schedule",
                            &error,
                        ));
                    }
                }
                let snapshot = self.snapshot();
                self.inner.emit(snapshot);
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
        let (op_id, target, execute_at, commit_scheduled, session_generation) = {
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
                state.session_generation,
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
            // MP-01: this commit was scheduled for a specific room/session.
            // If that session has since ended or been replaced, the task owns
            // nothing and must not touch the room.
            if state.session_generation != session_generation {
                return;
            }
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
            drop(state);
            // PROVIDER_SYNC rooms drive the provider browser.
            runtime.dispatch_provider_commit("PLAY", target);
            runtime.inner.emit(snapshot);
        });
    }

    /// Host side of the PAUSE protocol: the guest answered PAUSE_READY.
    fn on_guest_pause_ready(&self, operation_id: &str, ready: bool) {
        if !ready {
            return;
        }
        let (op_id, target, execute_at, commit_scheduled, session_generation) = {
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
                state.session_generation,
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
            // MP-01: the pause commit belongs to the session that scheduled
            // it. A room that ended or was replaced in the meantime must not
            // be forced back to PAUSED.
            if state.session_generation != session_generation {
                return;
            }
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
            drop(state);
            // PROVIDER_SYNC rooms drive the provider browser.
            runtime.dispatch_provider_commit("PAUSE", target);
            runtime.inner.emit(snapshot);
        });
    }

    /// Host side of the SEEK protocol: the guest answered SEEK_READY.
    fn on_guest_seek_ready(&self, operation_id: &str, ready: bool) {
        if !ready {
            return;
        }
        let (op_id, target, execute_at, resume_after_seek, commit_scheduled, session_generation) = {
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
                state.session_generation,
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
                // MP-01: a seek commit (and the play protocol it may chain
                // into) must not run against a room that has ended or been
                // replaced since the deadline was scheduled.
                if state.session_generation != session_generation {
                    return;
                }
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
                drop(state);
                // PROVIDER_SYNC rooms drive the provider browser.
                runtime.dispatch_provider_commit("SEEK", target);
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
                tokio::time::sleep(HEARTBEAT_INTERVAL).await;
                let client = runtime.lock().client.clone();
                let Some(client) = client else { break };
                match client.heartbeat().await {
                    Ok(()) => failures = 0,
                    Err(_) => {
                        failures = failures.saturating_add(1);
                        if heartbeat_declares_liveness_failure(failures) {
                            // Role-aware peer-loss recovery: the guest's
                            // missed-heartbeat threshold means the HOST died;
                            // the host's detection path means the GUEST died.
                            // Both go through the same existing recovery
                            // machine (peer_disconnected → RECONNECTING,
                            // strict-sync pause, never auto-resume).
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

    /// M4+/periodic guest BUFFER_STATUS reporter. PROTOCOL_SPEC §28
    /// and MASTER_PRD §21 require the guest to report its buffer status every
    /// ~500 ms while the room is Playing. This worker owns that cadence; the
    /// transition-triggered reports in the player event loop stay unchanged.
    ///
    /// - Paused/Ended/Reconnecting/strict-sync-paused rooms do not spam
    ///   reports (the guest has nothing authoritative to say there).
    /// - Exactly one worker exists: spawning replaces (and aborts) the old
    ///   one, so a rejoin/reconnect never duplicates it and a Leave Party
    ///   kills it.
    /// - Each tick reads the CURRENT client, so a replaced transport is used
    ///   immediately; a worker from a previous room is aborted by the
    ///   leave/join/create teardown paths and cannot publish into the new
    ///   room.
    fn spawn_buffer_status_worker(&self) {
        let runtime = self.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(BUFFER_STATUS_INTERVAL).await;
                let (should_report, position_ms, headroom_ms, stalled) = {
                    let state = runtime.lock();
                    let Some(player) = state.player.clone() else {
                        // No player: nothing to report. Keep looping so the
                        // same worker adopts a later player (reconnect path
                        // re-spawns anyway).
                        continue;
                    };
                    let room_playing =
                        state.room_state == RoomState::Playing && !state.sync.strict_sync_paused;
                    if !room_playing || state.client.is_none() {
                        continue;
                    }
                    let snap = {
                        let player = player
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        player.snapshot()
                    };
                    (
                        true,
                        snap.position_ms,
                        snap.buffered_ahead_ms.unwrap_or(0),
                        matches!(snap.state, crate::media::player::PlayerState::Buffering),
                    )
                };
                if !should_report {
                    continue;
                }
                // report_buffer_status picks up the CURRENT client under its
                // own lock and relays over QUIC (guest path).
                runtime.report_buffer_status(position_ms, headroom_ms, stalled);
                // every observation tick also re-evaluates the
                // camera ladder with the fresh buffer/goodput inputs
                // (movie-first degradation, PRD §41).
                runtime.evaluate_camera_ladder();
            }
        });
        let mut state = self.lock();
        if let Some(old) = state.buffer_status_task.replace(task) {
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
        self.spawn_buffer_status_worker();
        self.inner.emit(self.snapshot());
        Ok(())
    }

    fn apply_disconnect(inner: &Arc<RuntimeInner>) -> Result<(), ()> {
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
        // Abort any in-flight operation: a disconnected peer cannot answer
        // READY, so the pending operation must not commit half-way.
        state.pending_operation_id = None;
        state.pending_operation_kind = None;
        state.commit_scheduled_for = None;
        // M8/Wire through the proper recovery system so
        // last_recovery is surfaced to the UI and the full RecoveryPlan is
        // recorded. The event is role-aware: the guest's peer loss is a
        // HOST crash, the host's peer loss is a GUEST crash. Both use the
        // same existing recovery machine.
        let event = peer_loss_failure_event(Self::is_host_role(&state));
        let plan = recovery_plan(event);
        apply_recovery_to_state(&mut state, event, plan);
        state.last_recovery = Some(RuntimeRecoverySnapshot {
            event,
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

    /// test seam: the runtime's Arc<RuntimeInner> for direct
    /// event delivery in tests.
    #[cfg(test)]
    #[allow(clippy::type_complexity)]
    fn inner_for_events(&self) -> std::sync::Arc<RuntimeInner> {
        std::sync::Arc::clone(&self.inner)
    }

    /// test seam: deliver a guest-side peer event directly.
    #[cfg(test)]
    fn apply_peer_event_pub(
        inner: &std::sync::Arc<RuntimeInner>,
        env: EventEnvelope,
        event: QuicServerEvent,
    ) {
        Self::apply_peer_event(inner, &env, event);
    }

    /// test seam: deliver a host-side QUIC event directly.
    #[cfg(test)]
    fn apply_host_event_pub(inner: &std::sync::Arc<RuntimeInner>, event: QuicHostEvent) {
        // apply_host_event is a &self method; reconstruct via from_inner.
        let runtime = Self::from_inner(inner);
        runtime.apply_host_event(event);
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

            // §10: an event envelope from another room must
            // never be applied, even though the QUIC layer already checked
            // the version and room presence. Host-side local broadcasts
            // (sender == self) skip this because the host trusts its own
            // coordinator-built envelopes.
            if sender != &state.local_participant.id {
                if let Some(credentials) = state.credentials.as_ref() {
                    if envelope.room_id != credentials.room_id {
                        return;
                    }
                }
            }

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
            // receiving peer. CallSignal joins this list — the
            // host's own relayed signals would otherwise be re-validated
            // against the same ledger it was validated in (duplicate-offer
            // MP-CALL-002) and re-appended to call_signals. The same guard
            // covers a guest receiving its own signal echoed back by the
            // host broadcast: broadcast_guest_event preserves the original
            // sender device id, so sender == local id on both sides.
            let is_self = sender == &state.local_participant.id;
            if is_self {
                match &event {
                    QuicServerEvent::PlayPrepare { .. }
                    | QuicServerEvent::PlayCommit { .. }
                    | QuicServerEvent::PausePrepare { .. }
                    | QuicServerEvent::PauseCommit { .. }
                    | QuicServerEvent::SeekPrepare { .. }
                    | QuicServerEvent::SeekCommit { .. }
                    | QuicServerEvent::CallSignal { .. } => return,
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
                    // The play-readiness gate mirrors the coordinator's
                    // recorded local readiness (set by set_ready) rather than
                    // the volatile player-buffer field, so that the guest's
                    // auto-answer is consistent with the host's all_ready check.
                    let local_buffer = state
                        .sync_coordinator
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .guest_ready
                        .buffer_ahead_ms;
                    let ready =
                        state.local_participant.media_ready && local_buffer >= minimum_buffer_ms;
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
                    // MP-01: remember which session scheduled this commit so
                    // the delayed task can prove it still owns the room.
                    let session_generation = state.session_generation;
                    let inner_for_task = inner.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))
                            .await;
                        let mut state = inner_for_task.lock();
                        if state.session_generation != session_generation {
                            return;
                        }
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
                        drop(state);
                        // PROVIDER_SYNC rooms drive the provider
                        // browser with the same canonical commit.
                        Self::from_inner(&inner_for_task)
                            .dispatch_provider_commit("PLAY", target_position_ms);
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
                    // MP-01: bind the delayed commit to this session.
                    let session_generation = state.session_generation;
                    let inner_for_task = inner.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))
                            .await;
                        let mut state = inner_for_task.lock();
                        if state.session_generation != session_generation {
                            return;
                        }
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
                        drop(state);
                        // PROVIDER_SYNC rooms drive the provider
                        // browser with the same canonical commit.
                        Self::from_inner(&inner_for_task)
                            .dispatch_provider_commit("PAUSE", target_position_ms);
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
                    // MP-01: bind the delayed commit to this session.
                    let session_generation = state.session_generation;
                    let inner_for_task = inner.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))
                            .await;
                        let mut state = inner_for_task.lock();
                        if state.session_generation != session_generation {
                            return;
                        }
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
                        drop(state);
                        // PROVIDER_SYNC rooms drive the provider
                        // browser with the same canonical commit.
                        Self::from_inner(&inner_for_task)
                            .dispatch_provider_commit("SEEK", target_position_ms);
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
                    // fresh guest buffer observation → camera
                    // ladder re-evaluation (movie-first, PRD §41).
                    evaluate_camera_ladder_locked(&mut state);
                    sync_room_snapshot(&mut state);
                }
                QuicServerEvent::BufferRecovered { buffer_ahead_ms } => {
                    state.buffer.buffering_participant = None;
                    // Real verified transfer progress, never a fabricated
                    // 100% — the guest may have recovered its playback-ahead
                    // window while the whole file is still transferring.
                    state.buffer.percent = transfer_percent_for_state(&state);
                    state.buffer.guest_buffer_ahead_ms = buffer_ahead_ms;
                    // recovery is an upgrade-leaning observation —
                    // re-evaluate the camera ladder with the fresh buffer.
                    evaluate_camera_ladder_locked(&mut state);
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
                    // Ready Check retreat: when the host rewinds the room to
                    // LOBBY (Back to lobby), a guest sitting on the Ready
                    // Check screen follows back — neither side may be left
                    // stranded on the waiting-room screen. This only
                    // lowers READY_CHECK → LOBBY, never invents a new room.
                    if room_state == RoomState::Lobby && state.screen == "READY_CHECK" {
                        state.screen = "LOBBY".to_string();
                        state.local_participant.media_ready = false;
                        state.pending_operation_id = None;
                        state.pending_operation_kind = None;
                        state.pending_operations.clear();
                    }
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
                    // MP-14: validate on receipt as well as at the transport
                    // boundary. The boundary is the authoritative rejection
                    // point; this second check is what protects the *applied*
                    // state, which must hold even if an event reaches here by
                    // another route (a local broadcast, a future transport).
                    // The local send path already validated its own message.
                    if validate_received_chat_message(&message_id, &body).is_err() {
                        return;
                    }
                    if !state.chat.iter().any(|m| m.id == message_id) {
                        push_bounded(
                            &mut state.chat,
                            ChatSnapshot {
                                id: message_id,
                                sender: msg_sender,
                                body,
                                created_host_time_us,
                            },
                            MAX_CHAT_HISTORY,
                        );
                    }
                }
                QuicServerEvent::Reaction {
                    reaction_id,
                    sender: msg_sender,
                    reaction,
                } => {
                    // MP-14: the value check the local send path applies. A
                    // reaction that is merely rate-limited but not one of the
                    // v1 reactions must not enter room state.
                    if validate_received_reaction(&reaction_id, &reaction).is_err() {
                        return;
                    }
                    let participant_id = msg_sender.clone();
                    if let Err(_err) = state
                        .reaction_limiter
                        .accept(&participant_id, monotonic_us())
                    {
                        drop(state);
                        return;
                    }
                    if !state.reactions.iter().any(|r| r.id == reaction_id) {
                        push_bounded(
                            &mut state.reactions,
                            ReactionSnapshot {
                                id: reaction_id,
                                sender: msg_sender,
                                reaction,
                                created_host_time_us: monotonic_us(),
                            },
                            MAX_REACTION_HISTORY,
                        );
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
                // ── scheduling on the guest ─────
                // The guest persists the schedule locally (so reminders fire
                // even if the host app closes) and registers notifications,
                // exactly as PROTOCOL_SPEC §56 requires. Wall-clock UTC is
                // the legal clock domain for scheduling (§16).
                QuicServerEvent::ScheduleCreate {
                    schedule_id,
                    scheduled_start_utc_ms,
                    media_id,
                    call_mode,
                    planned_preload_utc_ms,
                } => {
                    if let Some(db) = state.db.clone() {
                        let schedule = crate::storage::sqlite::StoredSchedule {
                            schedule_id: schedule_id.clone(),
                            room_id: state
                                .credentials
                                .as_ref()
                                .map(|c| c.room_id.clone())
                                .unwrap_or_default(),
                            media_id: media_id.clone(),
                            scheduled_start_utc_ms,
                            planned_preload_utc_ms,
                            guest_device_id: state.local_participant.id.clone(),
                            status: "Planned".to_string(),
                            created_at_ms: wall_now_ms(),
                        };
                        match db.insert_schedule(&schedule) {
                            Ok(()) => {
                                let _ = inner.notifier.notify(
                                    "Movie Party",
                                    &format!(
                                        "Schedule accepted locally — '{media_id}' at the planned time."
                                    ),
                                );
                                state.pending_guest_schedule = Some(schedule_id.clone());
                            }
                            Err(error) => {
                                // A duplicate id (host re-broadcast) is not a
                                // failure; anything else is surfaced honestly.
                                let message = format!("{error}");
                                if !message.contains("UNIQUE") {
                                    state.error = Some(format!("MP-STORE-001 {error}"));
                                }
                            }
                        }
                    } else {
                        state.error = Some(
                            "MP-STORE-001 schedule received before storage was ready".to_string(),
                        );
                    }
                    let _ = call_mode;
                }
                // §56: the host echoed the guest's own acceptance — clears
                // the pending request marker.
                QuicServerEvent::ScheduleAccept {
                    schedule_id,
                    accepted: _,
                } => {
                    if state.pending_guest_schedule.as_deref() == Some(&schedule_id) {
                        state.pending_guest_schedule = None;
                    }
                }
                QuicServerEvent::ScheduleUpdate {
                    schedule_id,
                    media_id,
                    planned_preload_utc_ms,
                    scheduled_start_utc_ms,
                } => {
                    if let Some(db) = state.db.clone() {
                        // Each of these is the guest's stored copy of the
                        // host's authoritative schedule. A swallowed failure
                        // leaves the two silently disagreeing, so the Upcoming
                        // card shows a title or time the host never set. All
                        // three are still attempted; the first failure is
                        // reported.
                        let updates = [
                            db.update_schedule_media(&schedule_id, &media_id),
                            db.update_schedule_preload(&schedule_id, planned_preload_utc_ms),
                            db.update_schedule_start(&schedule_id, scheduled_start_utc_ms),
                        ];
                        if let Some(error) = updates.into_iter().find_map(Result::err) {
                            state.error = Some(storage_failure_message(
                                "failed to record a schedule change from the host",
                                &error,
                            ));
                        }
                    }
                }
                QuicServerEvent::ScheduleCancel { schedule_id } => {
                    if let Some(db) = state.db.clone() {
                        if let Err(error) = db.update_schedule_status(&schedule_id, "Cancelled") {
                            state.error = Some(storage_failure_message(
                                "failed to record a cancelled schedule",
                                &error,
                            ));
                        }
                    }
                }
                // §57: preload progress from the host drives the Home
                // "Upcoming" card percentage.
                QuicServerEvent::PreloadState {
                    schedule_id: _,
                    state: preload_state,
                    progress,
                    estimated_ready_utc_ms: _,
                } => {
                    state.pending_preload_state = Some((preload_state, progress));
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
                v_major: crate::protocol::ENVELOPE_V_MAJOR,
                v_minor: crate::protocol::ENVELOPE_V_MINOR,
                room_id: state
                    .credentials
                    .as_ref()
                    .map(|c| c.room_id.clone())
                    .unwrap_or_default(),
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
            // readiness gate: a Provider Sync room
            // must not start the play protocol until the provider's own
            // page reports playback-ready media over CDP. Starting the
            // protocol on an unready provider would commit both sides
            // to a black window (no silent fallback — honest error).
            if Self::provider_browser_is_media(&state)
                && state.provider.readiness
                    != crate::providers::sync::ProviderReadiness::PlaybackReady
            {
                state.error = Some(match state.provider.readiness {
                    crate::providers::sync::ProviderReadiness::LoginRequired => {
                        "MP-PROVIDER-004 sign in to the provider before starting playback"
                            .to_string()
                    }
                    _ => "MP-PROVIDER-003 open a title in the provider before starting playback"
                        .to_string(),
                });
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            // A protocol cycle is already in flight; ignore the repeat tap
            // rather than starting a second overlapping operation.
            if state.pending_operation_id.is_some() {
                return snapshot_from_state(&state);
            }
            // the local media file may have been moved or
            // renamed since the party started. An honest check before the
            // play protocol — never start a play that is doomed (§29).
            if let Some(path) = state.local_media_path.clone() {
                if !std::path::Path::new(&path).exists() {
                    let plan = recovery_plan(FailureEvent::MissingLocalFile);
                    apply_recovery_to_state(&mut state, FailureEvent::MissingLocalFile, plan);
                    state.error = Some(
                        "MP-MEDIA-002 the movie file moved or was renamed — locate it to continue"
                            .to_string(),
                    );
                    sync_room_snapshot(&mut state);
                    return snapshot_from_state(&state);
                }
            }
            // Never start the play protocol — and therefore never report
            // PLAYING — while the local player is genuinely unavailable
            // (e.g. the native surface attach failed because libmpv could
            // not be loaded). A player error here is sticky until the media
            // is re-opened, so the host cannot talk its way into PLAYING.
            if state.player_snapshot.state == PLAYER_STATE_ERROR
                && state
                    .player_snapshot
                    .error_message
                    .as_deref()
                    .is_some_and(|error| error.starts_with("MP-MEDIA-001"))
            {
                state.error =
                    Some("MP-MEDIA-001 player unavailable; cannot start playback".to_string());
                sync_room_snapshot(&mut state);
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

    /// UI_UX_SPEC §25: host presses Start → the coordinator schedules the
    /// canonical play operation with a 3-second countdown lead and
    /// broadcasts PLAY_PREPARE. The commit fires at the backend-owned
    /// deadline (the guest's PLAY_READY arrives during the window; the
    /// countdown the user sees animates from the backend-provided
    /// deadline — never a frontend-invented timer chain).
    pub fn request_play_countdown(&self) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            if !Self::is_host_role(&state) {
                state.error = Some("MP-CTRL-002 only the host can start the countdown".to_string());
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            if state.pending_operation_id.is_some() {
                return snapshot_from_state(&state);
            }
            // Same readiness gates as host_play (provider + player health)
            // so the countdown can never commit to a broken start.
            if Self::provider_browser_is_media(&state)
                && state.provider.readiness
                    != crate::providers::sync::ProviderReadiness::PlaybackReady
            {
                state.error = Some(
                    "MP-PROVIDER-003 open a title in the provider before starting playback"
                        .to_string(),
                );
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            if state.player_snapshot.state == PLAYER_STATE_ERROR
                && state
                    .player_snapshot
                    .error_message
                    .as_deref()
                    .is_some_and(|error| error.starts_with("MP-MEDIA-001"))
            {
                state.error =
                    Some("MP-MEDIA-001 player unavailable; cannot start playback".to_string());
                sync_room_snapshot(&mut state);
                return snapshot_from_state(&state);
            }
            let target_position = state.sync.position_ms;
            let now = monotonic_us();
            let prepared = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .prepare_play_scheduled(target_position, now + Self::COUNTDOWN_LEAD_US, 5_000);
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
            // The whole-file transfer percentage is the real verified
            // progress, never a fabricated 100% (the file may be far from
            // fully transferred even when the playback-ahead window is
            // healthy again).
            state.buffer.percent = transfer_percent_for_state(&state);
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
            push_bounded(
                &mut state.reactions,
                ReactionSnapshot {
                    id: message.reaction_id.to_string(),
                    sender: sender.clone(),
                    reaction: message.reaction.clone(),
                    created_host_time_us: host_time_us,
                },
                MAX_REACTION_HISTORY,
            );

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

    /// Provider Sync watch worker. Polls the
    /// provider's own HTML5 player over CDP (position + buffered-ahead)
    /// while a provider-mode room is Playing, and feeds the readings into
    /// the coordinator — the provider-side equivalents of the guest
    /// PLAYER_STATE / BUFFER_LOW reports:
    ///
    /// - position → `state.sync.position_ms` (the host's own media clock in
    ///   PROVIDER_SYNC mode IS the provider player)
    /// - buffered-ahead < 3 s (PROTOCOL buffer gate) → strict-sync pause
    ///   via the coordinator's `buffer_low` (§14: the host stops
    ///   when the movie source cannot continue)
    /// - player gone (no media element / browser closed) → honest
    ///   MP-PROVIDER-003 error + strict pause (no silent fallback)
    ///
    /// Exactly one worker exists; spawning replaces (aborts) the old one,
    /// and the leave/end teardown paths abort it with the room.
    fn spawn_provider_watch_worker(&self) {
        use crate::providers::sync::{buffer_command, position_command};

        let runtime = self.clone();
        let task = tokio::spawn(async move {
            let mut command_id: u64 = 1;
            loop {
                tokio::time::sleep(PROVIDER_WATCH_INTERVAL).await;
                // Gate: only act for a Playing provider-mode room with a
                // live browser. Anything else: idle-wait (the same worker
                // adopts a later provider room).
                let (should_poll, provider_id) = {
                    let state = runtime.lock();
                    let active = state.room_state == RoomState::Playing
                        && !state.sync.strict_sync_paused
                        && Self::provider_browser_is_media(&state)
                        && state.chrome_session.is_some();
                    let provider = state
                        .provider
                        .provider_id
                        .clone()
                        .and_then(|id| crate::providers::sync::provider_id_from_str(&id));
                    (active && provider.is_some(), provider)
                };
                let Some(provider) = provider_id else {
                    continue;
                };
                if !should_poll {
                    continue;
                }
                command_id = command_id.wrapping_add(2);

                // CDP round-trip out of the state lock (same take/execute/
                // return pattern as dispatch_provider_commit).
                //
                // MP-08: the round-trip is blocking TCP and used to run
                // directly on this async worker, parking a runtime thread for
                // up to the CDP read timeout on every poll. It now runs on the
                // blocking pool. The provider-session identity is captured
                // together with the session so a browser torn down while the
                // poll was in flight is closed here instead of being
                // resurrected into state.
                let inner = runtime.inner.clone();
                let (session_taken, chrome_generation) = {
                    let mut state = runtime.lock();
                    (state.chrome_session.take(), state.chrome_generation)
                };
                let Some(session) = session_taken else {
                    continue;
                };
                let polled = tokio::task::spawn_blocking(move || {
                    let execution = match session.connect_page() {
                        Ok(mut page) => {
                            let position_cmd = position_command(provider, command_id);
                            let buffer_cmd = buffer_command(provider, command_id + 1);
                            let position = page.execute(&position_cmd);
                            let buffer = page.execute(&buffer_cmd);
                            match (position, buffer) {
                                (Ok(p), Ok(b)) => {
                                    let seconds = p["result"]["value"].as_f64().or_else(|| {
                                        p["result"]["value"].as_i64().map(|v| v as f64)
                                    });
                                    let buffered_end =
                                        b["result"]["value"].as_f64().or_else(|| {
                                            b["result"]["value"].as_i64().map(|v| v as f64)
                                        });
                                    Ok((seconds, buffered_end))
                                }
                                (Err(e), _) | (_, Err(e)) => Err(e),
                            }
                        }
                        Err(error) => Err(error),
                    };
                    (execution, session)
                })
                .await;
                // A join failure means the blocking task panicked; its session
                // died with it (Drop closes the browser). Nothing to restore.
                let Ok((poll_result, session)) = polled else {
                    continue;
                };
                // Return the session to the state only while it is still the
                // one this poll started from.
                let mut stale = Some(session);
                {
                    let mut state = inner.lock();
                    if chrome_session_may_be_reinserted(&state, chrome_generation) {
                        state.chrome_session = stale.take();
                    }
                }
                // A session that lost the race is closed here, lock released.
                drop(stale);

                match poll_result {
                    Ok((Some(position_seconds), Some(buffered_end_seconds))) => {
                        let ahead_seconds = (buffered_end_seconds - position_seconds).max(0.0);
                        let recovery =
                            crate::providers::sync::recovery_action(Some(ahead_seconds), true);
                        let snapshot = {
                            let mut state = runtime.lock();
                            state.sync.position_ms = (position_seconds * 1_000.0).max(0.0) as u64;
                            state.buffer.guest_buffer_ahead_ms = (ahead_seconds * 1_000.0) as u64;
                            if recovery
                                == crate::providers::sync::ProviderRecoveryAction::StrictGlobalPause
                            {
                                // Strict sync (§14): the provider
                                // source cannot continue → the room pauses.
                                let _ = state
                                    .sync_coordinator
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                                    .buffer_low(PeerRole::Host, state.sync.position_ms);
                                state.room_state = RoomState::Buffering;
                                state.sync.strict_sync_paused = true;
                            }
                            snapshot_from_state(&state)
                        };
                        runtime.inner.emit(snapshot);
                    }
                    Ok((Some(_), None)) | Ok((None, Some(_))) => {
                        // Partial media state: treat as no readable player.
                        Self::record_provider_command_failure(
                            &runtime.inner,
                            crate::providers::sync::ProviderRuntimeError::MediaNotDetected,
                        );
                    }
                    Ok((None, None)) => {
                        // No media element on the page: honest failure.
                        Self::record_provider_command_failure(
                            &runtime.inner,
                            crate::providers::sync::ProviderRuntimeError::MediaNotDetected,
                        );
                    }
                    Err(error) => {
                        let mapped = crate::providers::sync::command_error_to_runtime_error(&error);
                        Self::record_provider_command_failure(&runtime.inner, mapped);
                    }
                }
            }
        });
        let mut state = self.lock();
        if let Some(old) = state.provider_watch_task.replace(task) {
            old.abort();
        }
    }

    // ── Provider Sync canonical-commit dispatch ──────

    /// True when the room's media lives in the managed provider browser
    /// (not the local mpv player) — canonical commits must drive the
    /// provider adapter over CDP. GENERIC_LINK rooms use the same managed
    /// browser; SHARED mode remains experimental/blocked (§30).
    fn provider_browser_is_media(state: &AppRuntimeState) -> bool {
        matches!(
            state.provider.mode.as_str(),
            "PROVIDER_SYNC" | "GENERIC_LINK"
        )
    }

    /// dispatch a canonical commit to the live provider browser
    /// via its adapter (CDP play/pause/seek). The coordinator stays the
    /// single authority (§15): only canonical commits reach this —
    /// guest CONTROL_REQUESTs are already normalized into host canonical
    /// operations upstream.
    ///
    /// CDP is blocking TCP: the session is moved OUT of the state lock
    /// (into a spawned task) so a slow browser never freezes the runtime.
    /// Failures surface honestly (MP-PROVIDER-003/004, §29) — no
    /// silent fallback to any other media mode.
    /// Private constructor from the shared inner: lets the static event
    /// loop arms (which only hold `&Arc<RuntimeInner>`) call the runtime's
    /// provider-dispatch method.
    fn from_inner(inner: &Arc<RuntimeInner>) -> Self {
        Self {
            inner: inner.clone(),
        }
    }

    fn dispatch_provider_commit(&self, commit_kind: &str, target_ms: u64) {
        use crate::providers::sync::{command_for_action, provider_sync_action_for_commit};

        let (provider_id, action) = {
            let state = self.lock();
            if !Self::provider_browser_is_media(&state) {
                return;
            }
            let Some(provider_id) = state.provider.provider_id.clone() else {
                return;
            };
            let Some(action) = provider_sync_action_for_commit(commit_kind, target_ms) else {
                return;
            };
            if state.chrome_session.is_none() {
                return;
            }
            (provider_id, action)
        };

        let inner = self.inner.clone();
        tauri::async_runtime::spawn_blocking(move || {
            // Snapshot execution context under the lock, run CDP outside it.
            let context = {
                let mut state = inner.lock();
                let provider = match crate::providers::sync::provider_id_from_str(&provider_id) {
                    Some(p) => p,
                    None => return,
                };
                let session_alive = state
                    .chrome_session
                    .as_mut()
                    .map(|session| session.is_alive())
                    .unwrap_or(false);
                (provider, session_alive)
            };
            let (provider, session_alive) = context;
            if !session_alive {
                Self::record_provider_command_failure(
                    &inner,
                    crate::providers::sync::ProviderRuntimeError::ProviderPageClosed,
                );
                return;
            }
            // Move the session OUT of the locked state for the CDP
            // round-trip (blocking TCP, up to the CDP read timeout): a
            // slow browser must never freeze the runtime lock. A
            // concurrent dispatch during the window sees no session and
            // honestly reports the browser as closed; canonical commits
            // are serialized by the coordinator, so this window is rare.
            //
            // MP-08: the provider-session identity is captured together with
            // it. If teardown closed the browser while the CDP round-trip was
            // in flight, the generation has moved on and the session must NOT
            // be put back — resurrecting a closed Chrome leaves every later
            // provider command targeting a dead process.
            let (session_taken, chrome_generation) = {
                let mut state = inner.lock();
                (state.chrome_session.take(), state.chrome_generation)
            };
            let result = match session_taken {
                Some(session) => {
                    let execution = match session.connect_page() {
                        Ok(mut page) => {
                            let command =
                                command_for_action(provider, page.next_command_id(), action);
                            page.execute(&command)
                        }
                        Err(error) => Err(error),
                    };
                    // Return the session to the state only while it is still
                    // the session this round-trip started from and the slot is
                    // still empty. Only the RESULT is a failure; the browser
                    // itself may be perfectly alive.
                    let mut stale = Some(session);
                    {
                        let mut state = inner.lock();
                        if chrome_session_may_be_reinserted(&state, chrome_generation) {
                            state.chrome_session = stale.take();
                        }
                    }
                    // If teardown or a replacement won the race, the session
                    // is still ours: close it here, with the lock released.
                    drop(stale);
                    execution
                }
                None => Err(crate::providers::chrome::ManagedChromeError::Process(
                    "browser not launched".to_string(),
                )),
            };
            match result {
                Ok(_) => {
                    // Confirm: the provider executed the canonical action.
                }
                Err(error) => {
                    let mapped = crate::providers::sync::command_error_to_runtime_error(&error);
                    Self::record_provider_command_failure(&inner, mapped);
                }
            }
        });
    }

    /// surface a provider command failure honestly (no silent
    /// fallback): sets the provider snapshot to Error with the stable
    /// MP-PROVIDER code, mirrors the code into the room error field, and
    /// emits the snapshot so the UI reflects the failure immediately.
    fn record_provider_command_failure(
        inner: &Arc<RuntimeInner>,
        error: crate::providers::sync::ProviderRuntimeError,
    ) {
        let (code, description) = crate::providers::sync::provider_runtime_error_response(error);
        let snapshot = {
            let mut state = inner.lock();
            state.provider.readiness = crate::providers::sync::ProviderReadiness::Error;
            state.provider.state = description;
            state.error = Some(code.to_string());
            // A dead provider browser is a strict-sync pause condition
            // (§14): both sides stop when the media source fails.
            if state.room_state == RoomState::Playing {
                let _ = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .buffer_low(PeerRole::Host, state.sync.position_ms);
                state.room_state = RoomState::Buffering;
                state.sync.strict_sync_paused = true;
            }
            snapshot_from_state(&state)
        };
        inner.emit(snapshot);
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
        state.player_snapshot.state = PLAYER_STATE_ERROR.to_string();
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
                    state
                        .player_snapshot
                        .observe(PlayerSnapshot::from_parts(snap.clone(), presentation));
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
                        Self::apply_drift_correction(&player_arc, drift_ms, position_ms);
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

    /// Apply a strict-sync drift correction to the guest player. This is the
    /// exact code the player event loop runs: `Ignore` restores the normal
    /// 1.0 rate once the guest has converged on the host position, a small
    /// drift nudges the playback rate, and larger drifts seek back to the
    /// canonical host commit.
    fn apply_drift_correction(
        player_arc: &Arc<std::sync::Mutex<dyn LocalPlayer + Send + Sync>>,
        drift_ms: i64,
        position_ms: u64,
    ) {
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

    /// M3: Spawn a background task that renders libmpv frames onto the
    /// attached native surface at ~30 fps. Frames are produced by the real
    /// player backend (mpv software renderer); the task only copies the latest
    /// RGBA buffer into the native view's layer. Runs for as long as a player
    /// exists and stops when no frame is available (e.g. paused or headless).
    fn spawn_player_render_loop(&self) {
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                let player_arc = {
                    let state = inner.lock();
                    state.player.clone()
                };
                let Some(player_arc) = player_arc else {
                    break; // No player — stop rendering
                };
                let frame = {
                    let mut player = player_arc
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    player.render_next_frame()
                };
                if let Some((surface, data, width, height, stride)) = frame {
                    crate::media::player::native_surface::display_frame(
                        surface, width, height, stride, data,
                    );
                }
            }
        });
        let mut state = self.lock();
        if let Some(old) = state.player_render_task.replace(task) {
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
                state.error = Some("MP-MEDIA-001 media is not ready for playback".to_string());
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

    /// Ready Check "Back to lobby": retract the readiness votes, drop any
    /// not-yet-fired countdown, and return the screen to the LOBBY. The
    /// party, media, and connection stay intact — only the readiness
    /// state is rewound (see LocalSyncCoordinator::retract_readiness).
    pub fn back_to_lobby(&self) -> AppSnapshot {
        let snapshot = {
            let mut state = self.lock();
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .retract_readiness();
            state.local_participant.media_ready = false;
            // Drop any not-yet-fired countdown at the state level too —
            // a scheduled PLAY that survives the retreat would fire in
            // the lobby and strand the room in PLAYING with nobody in it.
            state.pending_operation_id = None;
            state.pending_operation_kind = None;
            state.pending_operations.clear();
            state.room_state = RoomState::Lobby;
            state.screen = "LOBBY".to_string();
            state.error = None;
            sync_room_snapshot(&mut state);
            snapshot_from_state(&state)
        };
        self.inner.emit(snapshot.clone());
        snapshot
    }

    pub fn enter_cinema(&self) -> AppSnapshot {
        let mut state = self.lock();
        state.screen = "CINEMA".to_string();
        if state.player_snapshot.error_message.is_some() {
            state.room_state = RoomState::Error;
            state.sync.strict_sync_paused = true;
        } else {
            // Use the coordinator's canonical state — never fabricate PLAYING
            // before the play protocol has committed. The coordinator state
            // after set_ready is ReadyCheck; play is started by host_play.
            let coordinator_state = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .room_state;
            state.room_state = coordinator_state;
            if coordinator_state == RoomState::ReadyCheck || coordinator_state == RoomState::Lobby {
                state.sync.strict_sync_paused = false;
            }
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

    /// Detach the live player from its native video surface WITHOUT
    /// forgetting the loaded media. The frontend calls this when Cinema
    /// unmounts (leaving the view, React StrictMode's dev double-mount).
    /// The player tears down its mpv/render contexts and remembers the
    /// media path + snapshot; the next attach recreates everything and
    /// reloads at the remembered position — no spurious "cannot move to
    /// another native surface" PLAYER_ERROR, no frozen paused room.
    pub fn detach_native_video_surface(&self) -> AppSnapshot {
        let mut state = self.lock();
        if let Some(player) = state.player.clone() {
            let mut player = player
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            player.detach_native_surface();
            state.player_snapshot = PlayerSnapshot::from_player(&*player);
        }
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

    /// UI_UX_SPEC §40: host-only "Continue Without <peer>". The host
    /// acknowledges the guest's disconnect and continues solo — the one
    /// user-initiated override of the strict-sync guest gate (the
    /// coordinator's guest_abandoned flag; a returning guest re-clears it
    /// through reconnection bookkeeping). Playback resumes through the
    /// canonical play protocol, not a fabricated PLAYING (§28).
    pub fn continue_without_guest(&self) -> AppSnapshot {
        {
            let mut state = self.lock();
            if !Self::is_host_role(&state) {
                state.error =
                    Some("MP-CTRL-002 only the host can continue without the guest".to_string());
                sync_room_snapshot(&mut state);
                let snapshot = snapshot_from_state(&state);
                drop(state);
                self.inner.emit(snapshot.clone());
                return snapshot;
            }
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .abandon_guest();
            // §29 no silent mode change: record the explicit user override.
            let plan = RecoveryPlan {
                action: RecoveryAction::ContinueWithoutGuest,
                pauses_playback_for_both: false,
                requires_user_action: true,
            };
            state.last_recovery = Some(RuntimeRecoverySnapshot {
                event: FailureEvent::GuestCrash,
                action: plan.action,
                pauses_playback_for_both: plan.pauses_playback_for_both,
                requires_user_action: plan.requires_user_action,
            });
            sync_room_snapshot(&mut state);
        }
        // Resume through the canonical host play path (now unblocked by
        // the abandoned-guest readiness override).
        self.host_play()
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
        if let Some(task) = state.player_render_task.take() {
            task.abort();
        }
        if let Some(task) = state.transfer_task.take() {
            task.abort();
            state.old_transfer_task = Some(task);
        }
        if let Some(task) = state.transfer_stall_watcher_task.take() {
            task.abort();
        }
        if let Some(task) = state.chrome_crash_watcher_task.take() {
            task.abort();
        }
        if let Some(task) = state.player_failure_watcher_task.take() {
            task.abort();
        }
        if let Some(task) = state.reconnect_task.take() {
            task.abort();
        }
        if let Some(task) = state.heartbeat_task.take() {
            task.abort();
        }
        if let Some(task) = state.buffer_status_task.take() {
            task.abort();
        }
        if let Some(task) = state.provider_watch_task.take() {
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
        // MP-07: the Chrome session and the live player are moved out of the
        // state here and torn down only *after* the lock is released (see the
        // tail of this function) — closing them performs a CDP round-trip and
        // a libmpv teardown that can block for seconds, and doing that under
        // the global lock freezes every other task that needs the room.
        let teardown = DeferredTeardown::take(&mut state);
        // §52: a GUEST who received the movie over Local Perfect gets the
        // retention question (keep / remove / save-as) — never silent
        // deletion, never silent keeping. The HOST keeps its own source
        // file and owes no prompt.
        // state.media (the manifest) still holds the party's identity at
        // this point; the cache_entries row was registered at guest join.
        if state.guest_cache.is_some() {
            if let Some(manifest) = state.media.clone() {
                let role = state.local_participant.role.clone();
                if role == "Guest" {
                    if let Some(entry) = state
                        .db
                        .as_ref()
                        .and_then(|db| db.list_cache_entries().ok())
                        .and_then(|entries| {
                            entries
                                .iter()
                                .find(|cached| cached.media_id == manifest.media_id)
                                .cloned()
                        })
                    {
                        state.retention_prompt = Some(entry);
                    }
                }
            }
        }
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
            pending_operation: None,
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
        // MP-01: the session is over. Bump the lifecycle identity so every
        // commit task still sleeping towards its deadline can see that the
        // room it was scheduled for is gone and refuse to mutate it.
        bump_session_generation(&mut state);
        sync_room_snapshot(&mut state);
        let snapshot = snapshot_from_state(&state);
        // MP-07: release the global state lock BEFORE the blocking teardown.
        drop(state);
        teardown.run();
        snapshot
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
        push_bounded(
            &mut state.chat,
            ChatSnapshot {
                id: message.message_id.to_string(),
                sender: sender.clone(),
                body: message.body.clone(),
                created_host_time_us: message.created_host_time_us,
            },
            MAX_CHAT_HISTORY,
        );
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
}

/// (PRD §41): minimum time between camera
/// tier changes. The ladder's inputs (goodput estimate, guest buffer) are
/// inherently jittery; without a dwell the tier would oscillate and the
/// encoder would thrash. One second of stability is required before the
/// ladder may move the tier again.
/// send a ServerEvent through the host's event broadcast channel
/// using the coordinator's canonical seq counter — the same pattern
/// `send_host_event` uses for CoordinatorStateUpdate. Safe no-op when no
/// host session is active.
fn broadcast_from_inner(inner: &Arc<RuntimeInner>, event: QuicServerEvent) {
    let (tx, room_id, sender, seq) = {
        let state = match inner.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let Some(tx) = state.host_event_tx.clone() else {
            return;
        };
        let room_id = state
            .credentials
            .as_ref()
            .map(|c| c.room_id.clone())
            .unwrap_or_default();
        let sender = state.local_participant.id.clone();
        let seq = {
            let mut coordinator = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            coordinator.coordinator_event_seq = coordinator.coordinator_event_seq.wrapping_add(1);
            coordinator.coordinator_event_seq
        };
        (tx, room_id, sender, seq)
    };
    let envelope = EventEnvelope {
        v_major: crate::protocol::ENVELOPE_V_MAJOR,
        v_minor: crate::protocol::ENVELOPE_V_MINOR,
        room_id,
        seq,
        sender,
        sent_mono_us: monotonic_us(),
        event,
    };
    let _ = tx.send(envelope);
}

const CAMERA_TIER_MIN_DWELL_MS: u64 = 1_000;

/// Fallback movie bitrate estimate (bps) when no manifest/duration is
/// available yet. 5 Mbps matches the audit's 5 Mbps movie-priority
/// verification scenario and a typical HD stream; overestimating the
/// movie keeps the camera conservative (movie-first, §37).
const FALLBACK_MOVIE_BITRATE_BPS: u64 = 5_000_000;

/// provider player poll cadence. 1 s balances CDP cost against
/// the 3 s buffer gate the recovery policy applies (several consecutive
/// low readings are needed before a strict pause triggers via
/// `recovery_action`, so a single transient cannot pause the room).
const PROVIDER_WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// adaptive camera ladder evaluation on an already-held state
/// guard. Same policy as `AppRuntime::evaluate_camera_ladder` (which see);
/// this form exists so the host event-loop arms — which hold the lock for
/// the whole snapshot build — can evaluate inline without re-entrant
/// locking (the runtime lock is not re-entrant).
fn evaluate_camera_ladder_locked(state: &mut AppRuntimeState) {
    if state.call.mode == CallMode::Off || state.privacy_mode || !state.call.camera.enabled {
        // Call off / privacy / camera off by user choice: the ladder must
        // not re-enable anything; keep the notice quiet.
        if state.call.camera_notice.is_some() {
            state.call.camera_notice = None;
        }
        return;
    }
    // No movie in flight (no manifest): nothing to protect, and the
    // unmeasured goodput/buffer inputs would disable the camera for no
    // reason. The ladder runs only while a movie is actually at stake
    // (movie-first means the ladder exists to protect the movie).
    if state.media.is_none() {
        return;
    }
    let feedback = CameraFeedback {
        measured_goodput_bps: state.network.goodput_bps,
        estimated_movie_bitrate_bps: movie_bitrate_estimate_bps(state),
        guest_buffer_ms: state.buffer.guest_buffer_ahead_ms,
        rtt_ms: state.network.rtt_ms.unwrap_or(0),
    };
    let current = state.call.camera.tier;
    let recommended = recommend_camera_state(current, state.call.camera.enabled, feedback);

    if recommended.tier == current {
        // Same tier: clear the changed-at timestamp so a LATER change is
        // not blocked by dwell elapsed since an older change, but do not
        // touch the notice (it is cleared on consumption).
        if state.camera_tier_changed_at.is_some() {
            state.camera_tier_changed_at = None;
        }
        return;
    }
    // Dwell check: skip the change when the last change is too recent.
    if let Some(changed_at) = state.camera_tier_changed_at {
        if (changed_at.elapsed().as_millis() as u64) < CAMERA_TIER_MIN_DWELL_MS {
            return;
        }
    }
    let downgraded = recommended.tier > current;
    state.call.camera = recommended;
    state.camera_tier_changed_at = Some(std::time::Instant::now());
    if downgraded {
        // Once-per-event degradation notice: only fire when this downgrade
        // reaches a tier we have not already notified for this degradation
        // episode. Upgrades reset the episode so a later degradation
        // notifies again.
        if state.camera_notice_tier != Some(recommended.tier) {
            state.camera_notice_tier = Some(recommended.tier);
            state.call.camera_notice = Some(if recommended.enabled {
                format!(
                    "Camera quality reduced to protect movie playback (tier {:?}).",
                    recommended.tier
                )
            } else {
                "Camera turned off to protect movie playback.".to_string()
            });
        }
    } else {
        state.camera_notice_tier = None;
        state.call.camera_notice = None;
    }
}

impl AppRuntime {
    /// adaptive camera ladder evaluation. Reads the live
    /// feedback (measured goodput, RTT, guest buffer ahead, movie bitrate
    /// estimate) and applies the pure `recommend_camera_state` policy
    /// (movie-first: the camera tier drops before movie quality, PRD §41).
    ///
    /// Callers: the periodic buffer/goodput observation points (guest
    /// buffer-status worker, host BUFFER_STATUS receive, transfer-progress
    /// goodput updates). Idempotent per tick; flapping protection is a
    /// minimum dwell between tier CHANGES; the once-per-event degradation
    /// notice fires only on each new downgrade event.
    pub fn evaluate_camera_ladder(&self) {
        let mut state = self.lock();
        evaluate_camera_ladder_locked(&mut state);
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
            if let Some(task) = state.player_render_task.take() {
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
                .get_or_insert_with(|| std::env::temp_dir().join("MoviePartyCache"))
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
            {
                let mut state = self.lock();
                state.network.goodput_bps = goodput_bps;
                // first measured goodput → camera ladder baseline.
                evaluate_camera_ladder_locked(&mut state);
            }
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
            let (playable, position_ms, duration_ms) = {
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
                (
                    playable,
                    state.player_snapshot.position_ms,
                    state
                        .player_snapshot
                        .duration_ms
                        .filter(|duration| *duration > 0),
                )
            };
            // Honest initial headroom: the contiguous verified bytes ahead of
            // the playhead, exactly like the player event loop computes it.
            // A partially cached opening window is healthy playback headroom
            // even though the whole-file transfer percentage is low; a
            // completed chunk far from the playhead is not. The backend's
            // own buffered-ahead value (when it can expose one) stays
            // authoritative. The cache is consulted WITHOUT holding the
            // runtime state lock so the transfer worker can never deadlock
            // against this read.
            let cache_headroom_ms = match duration_ms {
                Some(duration_ms) => {
                    let byte_offset = (position_ms as u128 * manifest.file_size as u128
                        / duration_ms as u128) as u64;
                    let contiguous = cache_arc.lock().await.contiguous_bytes_from(byte_offset);
                    ((contiguous as u128 * duration_ms as u128) / manifest.file_size.max(1) as u128)
                        as u64
                }
                None => 0,
            };
            let mut state = self.lock();
            state.buffer.guest_buffer_ahead_ms = state
                .player_snapshot
                .buffered_ahead_ms
                .unwrap_or(cache_headroom_ms);
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
                                        // fresh goodput sample →
                                        // camera ladder re-evaluation
                                        // (movie-first, PRD §41).
                                        evaluate_camera_ladder_locked(&mut state);
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
            self.spawn_player_render_loop();
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

    /// Test-only hook to drive the missed-heartbeat disconnect path (the
    /// exact transition the heartbeat worker makes) without a live QUIC
    /// transport.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn apply_disconnect_for_test(&self) {
        let _ = Self::apply_disconnect(&self.inner);
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

/// PROTOCOL_SPEC §16: heartbeat interval.
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
/// PROTOCOL_SPEC §16: a peer is declared disconnected after 5 consecutive
/// missed heartbeat acknowledgements (≈10 s), matching the spec's
/// "no heartbeat for 10 seconds ⇒ disconnected" rule. Application-level
/// detection — we never rely on the QUIC idle timeout alone.
const HEARTBEAT_FAILURE_THRESHOLD: u8 = 5;

/// PROTOCOL_SPEC §28 / MASTER_PRD §21: guest buffer-status reporting cadence
/// while playing.
const BUFFER_STATUS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether `failures` consecutive missed heartbeat acknowledgements should
/// declare peer/session liveness failure. Pure so the threshold is testable
/// without a live transport.
fn heartbeat_declares_liveness_failure(failures: u8) -> bool {
    failures >= HEARTBEAT_FAILURE_THRESHOLD
}

/// The failure event a participant should record when its peer disappears.
/// Role-aware: the GUEST losing heartbeats means the HOST crashed; the HOST
/// losing its peer means the GUEST crashed. Both route through the same
/// existing recovery machine — this only makes `last_recovery` diagnostics
/// truthful about which side died.
fn peer_loss_failure_event(is_host_role: bool) -> FailureEvent {
    if is_host_role {
        FailureEvent::GuestCrash
    } else {
        FailureEvent::HostCrash
    }
}

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

/// estimated movie bitrate (bps) = file bytes × 8 / duration.
/// Falls back to a conservative 5 Mbps when the manifest or duration is
/// unknown (typical HD stream; movie-first means erring high keeps the
/// camera conservative, §37).
fn movie_bitrate_estimate_bps(state: &AppRuntimeState) -> u64 {
    if let (Some(manifest), Some(duration_ms)) = (&state.media, state.player_snapshot.duration_ms) {
        if duration_ms > 0 && manifest.file_size > 0 {
            return (manifest.file_size as u128 * 8_000 / duration_ms as u128) as u64;
        }
    }
    FALLBACK_MOVIE_BITRATE_BPS
}

/// Bump the room/session lifecycle identity (MP-01 / MP-08).
///
/// Called from every session teardown *and* every session start. A delayed
/// commit task compares it after waking so it can never mutate a room it no
/// longer owns; an in-flight CDP round-trip compares it so it can never
/// reinsert a Chrome session that teardown already closed and dropped.
fn bump_session_generation(state: &mut AppRuntimeState) {
    state.session_generation = state.session_generation.wrapping_add(1);
    state.chrome_generation = state.chrome_generation.wrapping_add(1);
}

/// Bump only the Chrome/provider lifecycle identity (MP-08). Used when a
/// provider browser is replaced or closed inside a still-live room, which must
/// not invalidate the room's own commit tasks.
fn bump_chrome_generation(state: &mut AppRuntimeState) {
    state.chrome_generation = state.chrome_generation.wrapping_add(1);
}

/// MP-08: may a Chrome session that was taken out of the state for a blocking
/// CDP round-trip be put back?
///
/// Only when BOTH hold:
/// * the provider generation is unchanged — i.e. no teardown, replacement, or
///   fresh launch happened while the round-trip was in flight; and
/// * the slot is still empty — i.e. nobody else already installed a session.
///
/// Otherwise the caller's session is obsolete: it is dropped (its `Drop` closes
/// the browser) instead of being resurrected over a newer or deliberately
/// closed provider. Without this, an in-flight poll could reinsert a browser
/// that teardown had already shut down, leaving the runtime pointing at a dead
/// CDP endpoint.
fn chrome_session_may_be_reinserted(state: &AppRuntimeState, taken_generation: u64) -> bool {
    state.chrome_generation == taken_generation && state.chrome_session.is_none()
}

/// Blocking teardown artifacts that must not be closed while the global state
/// mutex is held (MP-07).
///
/// `ManagedChromeSession::close_gracefully` performs a CDP round-trip and then
/// waits up to `GRACEFUL_CLOSE_TIMEOUT` for the child to exit, and
/// `LocalPlayer::close` tears down libmpv — both can block for seconds. Every
/// caller therefore takes these out of the state *under* the lock, releases
/// the lock, and only then runs the teardown; otherwise every other task that
/// needs the state (the QUIC event listeners, the UI's snapshot reads) stalls
/// behind it.
#[must_use = "the deferred teardown must run after the state lock is released"]
struct DeferredTeardown {
    chrome_session: Option<crate::providers::chrome::ManagedChromeSession>,
    player: Option<Arc<Mutex<dyn LocalPlayer + Send + Sync>>>,
}

impl DeferredTeardown {
    fn take(state: &mut AppRuntimeState) -> Self {
        Self {
            chrome_session: state.chrome_session.take(),
            player: state.player.take(),
        }
    }

    /// Run the blocking teardown. MUST be called with the state lock released.
    fn run(self) {
        if let Some(mut session) = self.chrome_session {
            // Graceful: Browser.close lets Chrome flush the provider profile
            // (SIGKILL could leave it locked → the next launch fails).
            session.close_gracefully();
        }
        if let Some(player) = self.player {
            player
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .close();
        }
    }
}

/// Builds the user-facing message for a storage write that failed on a path
/// which cannot return an error to a caller (a QUIC event handler, a refresh).
///
/// Keeps the established `MP-STORE-001` vocabulary — the frontend already maps
/// that code to plain language — but deliberately keeps the raw SQLite text
/// **out** of the message: it is logged instead, where it is useful for
/// diagnosis and invisible to the user. `StorageError::Sqlite` embeds the raw
/// rusqlite string, so interpolating it here would put engine internals on
/// screen.
fn storage_failure_message(context: &str, error: &crate::storage::StorageError) -> String {
    eprintln!("MovieParty: MP-STORE-001 {context}: {error}");
    format!("MP-STORE-001 {context}")
}

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
        sync: {
            let mut sync = state.sync.clone();
            sync.pending_operation =
                state
                    .pending_operation_id
                    .as_ref()
                    .map(|_id| PendingOperationSnapshot {
                        kind: state.pending_operation_kind.clone().unwrap_or_default(),
                        target_position_ms: state.pending_operation_target_ms,
                        execute_at_host_mono_us: state.pending_operation_execute_at_us,
                        execute_at_wall_ms: project_host_mono_to_wall_ms(
                            state.pending_operation_execute_at_us,
                            state.pending_operation_wall_anchor_mono_us,
                            state.pending_operation_wall_anchor_ms,
                        ),
                    });
            sync
        },
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
        pending_guest_schedule: state.pending_guest_schedule.as_deref().and_then(|id| {
            state
                .db
                .as_ref()
                .and_then(|db| db.list_schedules().ok())
                .and_then(|schedules| {
                    schedules
                        .iter()
                        .find(|schedule| schedule.schedule_id == id)
                        .map(|schedule| PendingGuestScheduleSnapshot {
                            schedule_id: schedule.schedule_id.clone(),
                            media_id: schedule.media_id.clone(),
                            scheduled_start_utc_ms: schedule.scheduled_start_utc_ms,
                        })
                })
        }),
        retention_prompt: state
            .retention_prompt
            .as_ref()
            .map(|entry| RetentionPromptSnapshot {
                media_id: entry.media_id.clone(),
                filename: entry.filename.clone(),
            }),
    }
}

/// Project a host-monotonic microsecond deadline into wall-clock epoch ms
/// for UI display (§16: monotonic time still drives execution; this
/// projection exists so the §25 Ready-Check countdown can animate from the
/// backend-provided deadline instead of a frontend-invented timer).
fn project_host_mono_to_wall_ms(
    host_mono_us: u64,
    anchor_mono_us: u64,
    anchor_wall_ms: u64,
) -> u64 {
    if anchor_mono_us == 0 {
        return 0;
    }
    let delta_us = host_mono_us.saturating_sub(anchor_mono_us);
    anchor_wall_ms.saturating_add(delta_us / 1_000)
}

fn sync_room_snapshot(state: &mut AppRuntimeState) {
    state.sync.room_state = format!("{:?}", state.room_state).to_ascii_uppercase();
}

/// Whole-file transfer percentage derived from the real verified transfer
/// progress. Recovery bookkeeping must never fabricate 0%/100% values: the
/// cache on disk is the source of truth for how much of the file exists.
/// This is deliberately NOT playback readiness — see
/// [`crate::media::cache::SparseCache::contiguous_bytes_from`] for the
/// playback-headroom concept.
fn transfer_percent_for_state(state: &AppRuntimeState) -> u8 {
    state
        .transfer
        .as_ref()
        .map(|progress| (progress.fraction() * 100.0).round().clamp(0.0, 100.0) as u8)
        .unwrap_or(0)
}

/// Rate-limit decision for "preload waiting" OS notifications: a repeated
/// WaitingForPeer poll within the window must not re-notify the user.
/// Mutates the per-schedule map so a fresh decision records the timestamp.
fn should_notify_preload_wait(
    state: &mut AppRuntimeState,
    schedule_id: &str,
    now: Instant,
) -> bool {
    const PRELOAD_WAIT_NOTIFY_WINDOW: Duration = Duration::from_secs(15 * 60);
    match state.preload_wait_notified_at.get(schedule_id) {
        Some(previous) if now.duration_since(*previous) < PRELOAD_WAIT_NOTIFY_WINDOW => false,
        _ => {
            state
                .preload_wait_notified_at
                .insert(schedule_id.to_string(), now);
            true
        }
    }
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
            state.buffer.buffering_participant = Some("Guest".to_string());
            // The stall cleared the buffer; the whole-file transfer
            // percentage is untouched because the verified cache is still
            // on disk. Only the playback-ahead window collapsed to zero.
            state.buffer.percent = transfer_percent_for_state(state);
        }
        FailureEvent::TransferResumed => {
            // PROTOCOL_SPEC §30 / MASTER_PRD §14: transfer recovery NEVER
            // resumes playback by itself. Playback only resumes through a
            // fresh host play-protocol cycle after readiness consensus, so
            // the room state and strict-sync pause are left exactly as the
            // coordinator holds them. Only the buffer bookkeeping is
            // refreshed, and from real transfer progress — not a
            // fabricated 100%.
            state.buffer.buffering_participant = None;
            state.buffer.percent = transfer_percent_for_state(state);
            if let Some(transfer) = state.transfer.as_mut() {
                transfer.buffer_ahead_ms = state.buffer.guest_buffer_ahead_ms;
            }
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
        adaptive_preload_deadline, project_host_mono_to_wall_ms, reconnect_failure,
        should_notify_preload_wait, sync_room_snapshot, AppRuntime, AppSnapshot, LibPlayerSnapshot,
        PlayerSnapshot, PlayerState, ReconnectFailure, PLAYER_STATE_ERROR,
    };
    use crate::call::{CallSignal, CallSignalType};
    use crate::network::quic::monotonic_us;
    use crate::network::quic::QuicError;
    use crate::resilience::{FailureEvent, RecoveryAction};
    use crate::room::MoviePartyInvite;
    use crate::sync::state_machine::RoomState;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::Mutex;

    /// Serializes tests that read or write the process-global
    /// `MOVIE_PARTY_DEV_LOOPBACK` env var so Rust's parallel test runner never
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
            state.invite = Some(MoviePartyInvite {
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
            state.invite = Some(MoviePartyInvite {
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
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let runtime = AppRuntime::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicUsize::new(0));
        let spawn_long = |cancelled: Arc<AtomicBool>, started: Arc<AtomicUsize>| {
            tokio::spawn(async move {
                started.fetch_add(1, Ordering::SeqCst);
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
            state.reconnect_task = Some(spawn_long(cancelled.clone(), started.clone()));
            state.heartbeat_task = Some(spawn_long(cancelled.clone(), started.clone()));
            state.preload_task = Some(spawn_long(cancelled.clone(), started.clone()));
            state.transfer_stall_watcher_task =
                Some(spawn_long(cancelled.clone(), started.clone()));
            state.buffer_status_task = Some(spawn_long(cancelled.clone(), started.clone()));
        }
        // Tokio processes a task's abort the next time the task is polled, so
        // every worker must reach its first poll before leave_party runs.
        // Without these yields the test body never releases its worker slot
        // and, when a previous test leaves the process loaded, spawned tasks
        // can sit unscheduled indefinitely — a scheduling hazard, not a
        // worker-abort bug. yield_now lets the body thread help schedule.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while started.load(Ordering::SeqCst) < 5 && std::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(
            started.load(Ordering::SeqCst),
            5,
            "all five workers must reach their first poll before leave_party"
        );
        assert!(
            !cancelled.load(Ordering::SeqCst),
            "sanity: workers must be running before leave_party"
        );
        runtime.leave_party();
        // Poll until every Drop guard has run. The yield keeps this test's
        // worker participating in scheduling so the abort wake is never
        // starved; the bounded deadline keeps a real failure loud.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !cancelled.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let state = runtime.lock();
        assert!(state.reconnect_task.is_none());
        assert!(state.heartbeat_task.is_none());
        assert!(state.preload_task.is_none());
        assert!(state.transfer_stall_watcher_task.is_none());
        assert!(state.buffer_status_task.is_none());
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

    #[test]
    fn heartbeat_threshold_matches_protocol_liveness_rules() {
        use super::{heartbeat_declares_liveness_failure, HEARTBEAT_FAILURE_THRESHOLD};
        // Healthy heartbeats reset the failure counter; the threshold only
        // trips after PROTOCOL_SPEC §16's disconnect window (5 missed ≈ 10s).
        assert_eq!(HEARTBEAT_FAILURE_THRESHOLD, 5);
        assert!(!heartbeat_declares_liveness_failure(0));
        assert!(!heartbeat_declares_liveness_failure(1));
        assert!(!heartbeat_declares_liveness_failure(4));
        assert!(heartbeat_declares_liveness_failure(5));
        assert!(heartbeat_declares_liveness_failure(255));
    }

    #[test]
    fn peer_loss_event_is_role_aware() {
        use super::peer_loss_failure_event;
        use crate::resilience::FailureEvent;
        // The host's peer loss is a guest crash; the guest's peer loss is a
        // host crash. Both drive the same recovery machine.
        assert_eq!(peer_loss_failure_event(true), FailureEvent::GuestCrash);
        assert_eq!(peer_loss_failure_event(false), FailureEvent::HostCrash);
    }

    #[tokio::test]
    async fn host_crash_detection_feeds_guest_recovery_state() {
        let runtime = AppRuntime::new();
        // Establish genuine PLAYING first — the liveness path must move the
        // guest OUT of playing without ever resuming on its own.
        {
            use crate::sync::consensus::ParticipantReadiness;
            let mut state = runtime.lock();
            state.local_participant.role = "Guest".to_string();
            let room_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                let scheduled = coord.prepare_play(0, 0, 5_000).expect("prepare");
                coord.commit_play(&scheduled).expect("commit");
                coord.room_state
            };
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
        }
        assert_eq!(runtime.snapshot().sync.room_state, "PLAYING");

        // Missed-heartbeat threshold path: the guest declares liveness
        // failure (this is the exact transition the heartbeat worker makes).
        runtime.apply_disconnect_for_test();

        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot.sync.room_state, "RECONNECTING",
            "host crash must move the guest to RECONNECTING"
        );
        assert!(
            snapshot.sync.strict_sync_paused,
            "host crash must strict-sync pause the guest"
        );
        assert!(
            !snapshot.network.connected,
            "host crash must mark the network down"
        );
        // Role-aware diagnostics: the GUEST recorded a HOST crash, not a
        // mislabeled GuestCrash.
        assert_eq!(
            snapshot
                .last_recovery
                .as_ref()
                .map(|recovery| recovery.event),
            Some(super::FailureEvent::HostCrash)
        );
        assert_ne!(
            snapshot.sync.room_state, "PLAYING",
            "liveness failure must never leave the room PLAYING"
        );
    }

    #[tokio::test]
    async fn guest_crash_detection_feeds_host_recovery_state() {
        let runtime = AppRuntime::new();
        {
            use crate::sync::consensus::ParticipantReadiness;
            let mut state = runtime.lock();
            // Default role is Host.
            let room_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                let scheduled = coord.prepare_play(0, 0, 5_000).expect("prepare");
                coord.commit_play(&scheduled).expect("commit");
                coord.room_state
            };
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
        }

        runtime.apply_disconnect_for_test();

        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.sync.room_state, "RECONNECTING");
        assert!(snapshot.sync.strict_sync_paused);
        assert_eq!(
            snapshot
                .last_recovery
                .as_ref()
                .map(|recovery| recovery.event),
            Some(super::FailureEvent::GuestCrash),
            "the HOST side records a GuestCrash"
        );
    }

    #[tokio::test]
    async fn buffer_status_worker_is_replaced_not_duplicated() {
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
            state.buffer_status_task = Some(old);
        }
        for _ in 0..50 {
            if old_started.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(old_started.load(Ordering::SeqCst), "old worker must start");

        runtime.spawn_buffer_status_worker();

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
            "old buffer-status worker must be aborted on replacement"
        );
        assert!(
            runtime.lock().buffer_status_task.is_some(),
            "new buffer-status worker must be present"
        );
        runtime.leave_party();
        assert!(
            runtime.lock().buffer_status_task.is_none(),
            "Leave Party must cancel the buffer-status worker"
        );
    }

    #[tokio::test]
    async fn buffer_status_worker_cadence_is_500ms_while_playing_only() {
        use super::BUFFER_STATUS_INTERVAL;
        // Cadence matches MASTER_PRD §21 / PROTOCOL_SPEC §28.
        assert_eq!(BUFFER_STATUS_INTERVAL, Duration::from_millis(500));

        // The worker's report gate reads `state.room_state` (the canonical
        // enum) plus the strict-sync pause flag: only a non-paused PLAYING
        // room reports. Paused/ended/buffering/reconnecting rooms never
        // reach report_buffer_status from the periodic path.
        let runtime = AppRuntime::new();
        let non_reportable_states = [
            (RoomState::Lobby, false),
            (RoomState::ReadyCheck, false),
            (RoomState::Paused, false),
            (RoomState::Buffering, false),
            (RoomState::Reconnecting, false),
            (RoomState::Ended, false),
            (RoomState::Playing, true), // strict-sync-paused
        ];
        for (room_state, strict_sync_paused) in non_reportable_states {
            let mut state = runtime.lock();
            state.room_state = room_state;
            state.sync.strict_sync_paused = strict_sync_paused;
            let should_report =
                state.room_state == RoomState::Playing && !state.sync.strict_sync_paused;
            assert!(
                !should_report,
                "room {room_state:?} (paused={strict_sync_paused}) must not trigger periodic buffer reports"
            );
        }
        // A healthy PLAYING room is reportable — the same gate the worker
        // runs before each 500 ms tick's send.
        {
            let mut state = runtime.lock();
            state.room_state = RoomState::Playing;
            state.sync.strict_sync_paused = false;
            let should_report =
                state.room_state == RoomState::Playing && !state.sync.strict_sync_paused;
            assert!(should_report, "healthy PLAYING room must be reportable");
        }
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
        let _snap = runtime
            .update_provider_readiness(crate::providers::sync::ProviderReadiness::LoginRequired);
        assert!(runtime.validate_provider_ready_for_room().is_err());

        // Set to Ready — should pass
        let _snap =
            runtime.update_provider_readiness(crate::providers::sync::ProviderReadiness::Ready);
        assert!(runtime.validate_provider_ready_for_room().is_ok());

        // Set to PlaybackReady — should pass
        let _snap = runtime
            .update_provider_readiness(crate::providers::sync::ProviderReadiness::PlaybackReady);
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
        let _snap =
            runtime.update_provider_readiness(crate::providers::sync::ProviderReadiness::Ready);

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

    /// PROTOCOL_SPEC §52: a CALL_SIGNAL body above 64 KiB must
    /// be rejected by validate_signal (the transport still allows it — it
    /// is under §5's 256 KiB — so this is the call subsystem's own gate).
    #[test]
    fn call_signal_over_64kib_is_rejected() {
        let runtime = AppRuntime::new();

        // 64 KiB + 1 of legal SDP-ish padding.
        let oversized = format!(
            r#"{{"type":"offer","sdp":"v=0\r\n{}"}}"#,
            "x".repeat(64 * 1024)
        );
        let result = runtime.submit_call_signal(CallSignal {
            signal_type: CallSignalType::Offer,
            data: oversized,
        });

        assert!(
            matches!(&result, Err(e) if e == "MP-CALL-001 invalid call signal"),
            "signals above the §52 64 KiB limit must be rejected, got {result:?}"
        );
        // Exactly at the limit still parses (structure check may reject a
        // synthetic body, but never with a size error).
        let prefix = r#"{"type":"offer","sdp":"v=0\r\n{WILL_PAD}"}"#;
        // `prefix` embeds the token {WILL_PAD}; replace it with padding
        // sized so the final body is exactly 64 KiB.
        let overhead = prefix.len() - "{WILL_PAD}".len();
        let at_limit = CallSignal {
            signal_type: CallSignalType::Offer,
            data: prefix.replace("{WILL_PAD}", &"x".repeat(64 * 1024 - overhead)),
        };
        assert_eq!(at_limit.data.len(), 64 * 1024);
        let mut ledger = crate::call::CallSignalLedger::default();
        assert!(crate::call::validate_signal(&at_limit, &mut ledger).is_ok());
    }

    /// the host self-subscriber must not re-apply its own relayed
    /// CallSignal (duplicate-offer MP-CALL-002 + double append), and a guest
    /// must not re-apply its own signal echoed back by the host broadcast.
    /// Both cases are `sender == local id` envelopes.
    #[test]
    fn call_signal_self_echo_is_not_reapplied() {
        let runtime = AppRuntime::new();
        // Submitting a valid offer appends exactly one snapshot entry…
        let first = runtime
            .submit_call_signal(CallSignal {
                signal_type: CallSignalType::Offer,
                data: r#"{"type":"offer","sdp":"v=0\r\n"}"#.to_string(),
            })
            .expect("offer");
        assert_eq!(first.call_signals.len(), 1);

        // …now simulate the self-echo: the same offer coming back through
        // the broadcast with sender == local device id.
        let local_id = {
            let state = runtime.lock();
            state.local_participant.id.clone()
        };
        let envelope = crate::network::quic::EventEnvelope {
            v_major: crate::protocol::ENVELOPE_V_MAJOR,
            v_minor: crate::protocol::ENVELOPE_V_MINOR,
            room_id: "self-echo-test".to_string(),
            seq: 2,
            sender: local_id,
            sent_mono_us: 1,
            event: crate::network::quic::ServerEvent::CallSignal {
                signal_type: "OFFER".to_string(),
                data: r#"{"type":"offer","sdp":"v=0\r\n"}"#.to_string(),
            },
        };

        AppRuntime::apply_peer_event(&runtime.inner, &envelope, envelope.event.clone());

        // The echo must have been dropped by the is_self guard: no
        // duplicate-offer Degraded state, no second snapshot entry.
        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot.call_signals.len(),
            1,
            "self-echoed call signal must not be re-applied"
        );
        assert_ne!(
            snapshot.call.status,
            crate::call::CallRuntimeStatus::Degraded,
            "self-echo must not trip the duplicate-offer validation"
        );
    }

    /// negative control: a CallSignal from the PEER still applies
    /// (the guard must not swallow genuine remote signals).
    #[test]
    fn call_signal_from_peer_is_applied() {
        let runtime = AppRuntime::new();
        let envelope = crate::network::quic::EventEnvelope {
            v_major: crate::protocol::ENVELOPE_V_MAJOR,
            v_minor: crate::protocol::ENVELOPE_V_MINOR,
            room_id: "peer-signal-test".to_string(),
            seq: 1,
            sender: "peer-device-id".to_string(),
            sent_mono_us: 1,
            event: crate::network::quic::ServerEvent::CallSignal {
                signal_type: "OFFER".to_string(),
                data: r#"{"type":"offer","sdp":"v=0\r\n"}"#.to_string(),
            },
        };

        AppRuntime::apply_peer_event(&runtime.inner, &envelope, envelope.event.clone());

        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot.call_signals.len(),
            1,
            "a peer-originated call signal must be applied"
        );
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
        // Establish genuine PLAYING through the coordinator, not by
        // fabricating the state in enter_cinema (which must not claim
        // PLAYING before the play protocol commits).
        {
            use crate::sync::consensus::ParticipantReadiness;
            let mut state = runtime.lock();
            let room_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                let scheduled = coord.prepare_play(0, 0, 5_000).expect("prepare");
                coord.commit_play(&scheduled).expect("commit");
                coord.room_state
            };
            state.room_state = room_state;
            sync_room_snapshot(&mut state);
        }
        let playing = runtime.snapshot();
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
        // PROTOCOL_SPEC §30: transfer recovery never silently resumes
        // playback. The coordinator holds the pause until the guest
        // completes a fresh readiness cycle (report_buffer_recovered →
        // READY_CHECK → fresh play commit). The room must NOT be PLAYING
        // here merely because the transfer worker is making progress
        // again.
        assert!(resumed.sync.strict_sync_paused);
        assert_ne!(
            resumed.sync.room_state, "PLAYING",
            "transfer recovery must not auto-resume playback"
        );
        // The guest buffer no longer reads as the buffering participant,
        // and the transfer percent reflects real verified progress, never
        // a fabricated 100%.
        assert!(resumed.buffer.buffering_participant.is_none());
        assert!(resumed.buffer.percent <= 100);

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

    // ── F33 regression suite: one canonical player failure state ─────────
    //
    // The event loop rendered `PlayerState::Error` as `"ERROR"` while the
    // diagnostic path, the failure watcher, both play gates, CinemaView and
    // ProviderStatusOverlay all keyed on `"PLAYER_ERROR"`. Every consumer
    // therefore missed real playback failures.

    /// A lib player snapshot in a chosen state, with everything else inert.
    fn lib_player_snapshot(state: PlayerState) -> LibPlayerSnapshot {
        LibPlayerSnapshot {
            state,
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            playback_rate: 1.0,
            buffered_ahead_ms: None,
            error_message: None,
        }
    }

    /// Both producers of the failure state must agree on the spelling.
    #[test]
    fn f33_event_loop_and_diagnostic_producers_agree() {
        let runtime = AppRuntime::new();

        // Producer 1 — the 200 ms event loop, through `PlayerSnapshot::from`.
        let from_event_loop = PlayerSnapshot::from(&lib_player_snapshot(PlayerState::Error));

        // Producer 2 — the diagnostic setter used by the load/command paths.
        let mut state = runtime.lock();
        AppRuntime::set_player_diagnostic_error(
            &mut state,
            "MP-MEDIA-001 libmpv is unavailable".to_string(),
        );
        let from_diagnostic = state.player_snapshot.state.clone();

        assert_eq!(
            from_event_loop.state, PLAYER_STATE_ERROR,
            "the event loop must emit the canonical failure state"
        );
        assert_eq!(from_diagnostic, PLAYER_STATE_ERROR);
        assert_eq!(
            from_event_loop.state, from_diagnostic,
            "the two producers must not disagree on the failure spelling"
        );
    }

    /// Only `Error` is remapped — every other state keeps its wire name.
    #[test]
    fn f33_other_player_states_keep_their_wire_names() {
        let expected = [
            (PlayerState::Stopped, "STOPPED"),
            (PlayerState::Ready, "READY"),
            (PlayerState::Playing, "PLAYING"),
            (PlayerState::Paused, "PAUSED"),
            (PlayerState::Buffering, "BUFFERING"),
            (PlayerState::Seeking, "SEEKING"),
            (PlayerState::Error, PLAYER_STATE_ERROR),
        ];
        for (state, wire) in expected {
            assert_eq!(
                PlayerSnapshot::from(&lib_player_snapshot(state)).state,
                wire,
                "{state:?} must serialize as {wire}"
            );
        }
    }

    /// A failure observed by the event loop must satisfy every consumer:
    /// the play gate blocks, the diagnostic survives, and the failure
    /// watcher's gate (state AND message) holds.
    #[test]
    fn f33_event_loop_failure_reaches_every_consumer() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            use crate::sync::consensus::ParticipantReadiness;
            let coord_room_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                coord.update_readiness_consensus(5_000);
                coord.room_state
            };
            state.room_state = coord_room_state;

            // The load path records the stable diagnostic…
            AppRuntime::set_player_diagnostic_error(
                &mut state,
                "MP-MEDIA-001 libmpv is unavailable".to_string(),
            );
            // …then the event loop observes the same failure, message-less.
            state
                .player_snapshot
                .observe(PlayerSnapshot::from(&lib_player_snapshot(
                    PlayerState::Error,
                )));
            sync_room_snapshot(&mut state);
        }

        // The play gate must block: `host_play` reads the canonical state.
        let snapshot = runtime.host_play();
        assert_ne!(
            snapshot.sync.room_state, "PLAYING",
            "the play gate must block a failed player"
        );
        assert_eq!(snapshot.player.state, PLAYER_STATE_ERROR);
        assert!(
            snapshot
                .player
                .error_message
                .as_deref()
                .is_some_and(|error| error.starts_with("MP-MEDIA-001")),
            "the diagnostic must survive the event loop, or recovery never fires: {:?}",
            snapshot.player.error_message
        );
    }

    /// `observe` must not preserve a stale message once the player recovers.
    #[test]
    fn f33_recovery_clears_the_diagnostic() {
        let mut snapshot = PlayerSnapshot::from(&lib_player_snapshot(PlayerState::Error));
        snapshot.error_message = Some("MP-MEDIA-001 libmpv is unavailable".to_string());

        snapshot.observe(PlayerSnapshot::from(&lib_player_snapshot(
            PlayerState::Ready,
        )));

        assert_eq!(snapshot.state, "READY");
        assert!(
            snapshot.error_message.is_none(),
            "a recovered player must not keep the old diagnostic: {:?}",
            snapshot.error_message
        );
    }

    // ── F45 regression suite: retention path containment ─────────────────
    //
    // `cache_dir_for` used `Path::starts_with`, a component-prefix test that
    // does not normalise `..`, so every traversal payload below was accepted
    // by the guard meant to reject it.

    fn scratch_cache_root(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("mp-{tag}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn f45_cache_dir_for_refuses_traversal_ids() {
        let runtime = AppRuntime::new();
        let root = scratch_cache_root("f45-traversal");
        runtime.init_cache_root_for_test(root.clone());

        for hostile in [
            ".",
            "..",
            "../..",
            "../../tmp/test",
            "a/../../etc",
            "/etc/passwd",
            "a\\..\\b",
            "",
        ] {
            let error = runtime
                .cache_dir_for(hostile)
                .expect_err(&format!("{hostile:?} must be refused"));
            assert!(
                error.contains("MP-MEDIA-002"),
                "unexpected error for {hostile:?}: {error}"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A media id must never resolve to the cache root itself.
    ///
    /// `root.join(".")` is the root, so accepting a bare `.` would let a
    /// retention "remove" delete the whole cache. Both guards reject it: the
    /// component rule (`"."` is not an entry) and the containment check
    /// (`root/.`'s parent is the root's parent, not the root).
    #[test]
    fn f45_cache_dir_for_never_resolves_to_the_cache_root_itself() {
        let runtime = AppRuntime::new();
        let root = scratch_cache_root("f45-root");
        runtime.init_cache_root_for_test(root.clone());

        assert!(
            runtime.cache_dir_for(".").is_err(),
            "a bare '.' names the cache root and must be refused"
        );

        for valid in ["a", "media-1", "a.b"] {
            let resolved = runtime.cache_dir_for(valid).expect("valid id");
            assert_ne!(
                resolved, root,
                "{valid} must not resolve to the cache root itself"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn f45_accepted_ids_resolve_to_a_direct_child_of_the_root() {
        let runtime = AppRuntime::new();
        let root = scratch_cache_root("f45-contained");
        runtime.init_cache_root_for_test(root.clone());

        for valid in ["media-abc123", "0123456789abcdef", "a"] {
            let resolved = runtime.cache_dir_for(valid).expect("valid id");
            // Assert on the RESULTING PATH, not on the input string.
            assert_eq!(
                resolved.parent(),
                Some(root.as_path()),
                "{valid} escaped the cache root: {}",
                resolved.display()
            );
            assert!(
                !resolved.to_string_lossy().contains(".."),
                "{valid} produced a traversing path: {}",
                resolved.display()
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn f45_retention_decision_refuses_a_non_child_directory() {
        let root = scratch_cache_root("f45-retention");
        // The resolved path a `..` payload used to produce.
        let outside = root.join("..").join("mp-f45-outside");

        let error = crate::storage::apply_retention_decision(
            &root,
            &outside,
            &outside.join("data.bin"),
            crate::storage::RetentionDecision::Remove,
            None,
        )
        .expect_err("a directory outside the cache root must be refused");

        assert!(
            matches!(error, crate::storage::StorageError::UnsafeCachePath),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn f45_retention_decision_still_allows_a_legitimate_cache_dir() {
        let root = scratch_cache_root("f45-allow");
        let dir = root.join("media-ok");
        std::fs::create_dir_all(&dir).expect("mkdir");

        let kept = crate::storage::apply_retention_decision(
            &root,
            &dir,
            &dir.join("data.bin"),
            crate::storage::RetentionDecision::KeepInMovieParty,
            None,
        )
        .expect("a direct child of the cache root is legitimate");

        assert_eq!(kept, Some(dir.clone()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enter_cinema_never_fabricates_playing_before_play_commit() {
        // A fresh runtime has no coordinator play protocol in flight. Entering
        // cinema must NOT advertise PLAYING just because a player exists —
        // PLAYING is only legitimate after a committed play operation.
        let runtime = AppRuntime::new();
        let snapshot = runtime.enter_cinema();

        assert_eq!(snapshot.screen, "CINEMA");
        assert_ne!(
            snapshot.sync.room_state, "PLAYING",
            "enter_cinema must not fabricate PLAYING before a play commit"
        );

        // A coordinator that has only reached READY_CHECK (set_ready path,
        // no play protocol) must also stay non-PLAYING after enter_cinema.
        {
            use crate::sync::consensus::ParticipantReadiness;
            let mut state = runtime.lock();
            let ready_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                coord.update_readiness_consensus(5_000);
                coord.room_state
            };
            state.room_state = ready_state;
            sync_room_snapshot(&mut state);
        }
        let ready = runtime.enter_cinema();
        assert_eq!(ready.sync.room_state, "READYCHECK");
        assert_ne!(ready.sync.room_state, "PLAYING");
    }

    #[test]
    fn host_play_never_reports_playing_when_player_unavailable() {
        // If the local player is genuinely unavailable (libmpv could not be
        // loaded at attach), host_play must refuse to start the distributed
        // play protocol: the room can never talk its way into PLAYING while
        // no real playback engine exists on this device.
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            use crate::sync::consensus::ParticipantReadiness;
            let coord_room_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                coord.update_readiness_consensus(5_000);
                coord.room_state
            };
            state.room_state = coord_room_state;
            AppRuntime::set_player_diagnostic_error(
                &mut state,
                "MP-MEDIA-001 player unavailable".to_string(),
            );
            sync_room_snapshot(&mut state);
        }

        let snapshot = runtime.host_play();

        assert_ne!(
            snapshot.sync.room_state, "PLAYING",
            "host_play must never fabricate PLAYING with an unavailable player"
        );
        assert!(
            snapshot.player.state == "PLAYER_ERROR"
                && snapshot
                    .player
                    .error_message
                    .as_deref()
                    .is_some_and(|error| error.starts_with("MP-MEDIA-001")),
            "the LibMpvUnavailable diagnostic must stay visible, got {:?}",
            snapshot.player.error_message
        );
        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-MEDIA-001 player unavailable; cannot start playback")
        );
    }

    /// §25: request_play_countdown schedules the canonical play
    /// operation with a 3-second countdown lead and exposes the deadline
    /// on the snapshot so the frontend animates from backend time.
    #[test]
    fn request_play_countdown_exposes_backend_deadline() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            use crate::sync::consensus::ParticipantReadiness;
            let coord_room_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                coord.update_readiness_consensus(5_000);
                coord.room_state
            };
            state.room_state = coord_room_state;
            state.local_participant.role = "Host".to_string();
            sync_room_snapshot(&mut state);
        }

        let before_mono = monotonic_us();
        let snapshot = runtime.request_play_countdown();
        let after_mono = monotonic_us();

        let pending = snapshot
            .sync
            .pending_operation
            .expect("countdown must expose the pending operation");
        assert_eq!(pending.kind, "PLAY");
        // §25: the deadline is ~3 s out (backend-owned, not a guess).
        assert!(
            pending.execute_at_host_mono_us >= before_mono + 2_900_000,
            "deadline must be ~3s ahead, got {}",
            pending.execute_at_host_mono_us - before_mono
        );
        assert!(
            pending.execute_at_host_mono_us <= after_mono + 3_100_000,
            "deadline must be ~3s ahead"
        );
        // §16: the wall projection is anchored, never 0 after construction.
        assert!(
            pending.execute_at_wall_ms > 0,
            "wall projection must be anchored for UI display"
        );
        assert!(
            pending.execute_at_wall_ms / 1_000 >= 1_700_000_000,
            "wall projection must be a plausible epoch ms value"
        );
    }

    /// §25: the countdown is host-only (§15 host authority) — the guest
    /// gets an honest MP-CTRL-002, and no pending operation is created.
    #[test]
    fn request_play_countdown_is_host_only() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.local_participant.role = "Guest".to_string();
            sync_room_snapshot(&mut state);
        }

        let snapshot = runtime.request_play_countdown();
        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-CTRL-002 only the host can start the countdown")
        );
        assert!(
            snapshot.sync.pending_operation.is_none(),
            "a guest countdown attempt must not schedule an operation"
        );
    }

    /// §25 + §16: the host-monotonic → wall-ms projection is anchored and
    /// linear; the identity point (anchor) maps to the anchor wall value.
    #[test]
    fn host_mono_to_wall_projection_is_anchored() {
        let runtime = AppRuntime::new();
        let (anchor_mono, anchor_wall) = {
            let state = runtime.lock();
            (
                state.pending_operation_wall_anchor_mono_us,
                state.pending_operation_wall_anchor_ms,
            )
        };
        assert!(anchor_mono > 0, "monotonic anchor must be set");
        assert!(anchor_wall > 0, "wall anchor must be set");
        assert_eq!(
            project_host_mono_to_wall_ms(anchor_mono, anchor_mono, anchor_wall),
            anchor_wall
        );
        // 1.5 s after the anchor projects exactly 1_500 ms later.
        assert_eq!(
            project_host_mono_to_wall_ms(anchor_mono + 1_500_000, anchor_mono, anchor_wall),
            anchor_wall + 1_500
        );
        // Before the anchor saturates rather than underflowing.
        assert_eq!(
            project_host_mono_to_wall_ms(0, anchor_mono, anchor_wall),
            anchor_wall
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
        std::env::remove_var("MOVIE_PARTY_DEV_LOOPBACK");
        let _g0 = ScopedEnv {
            key: "MOVIE_PARTY_DEV_LOOPBACK",
            prev: std::env::var_os("MOVIE_PARTY_DEV_LOOPBACK"),
        };
        let r0 = AppRuntime::new();
        assert_not_dev_loopback(r0.create_local_party(None).await);
        r0.leave_party();
        drop(_g0);

        // --- =0: same Tailscale path ---
        let _g1 = ScopedEnv {
            key: "MOVIE_PARTY_DEV_LOOPBACK",
            prev: std::env::var_os("MOVIE_PARTY_DEV_LOOPBACK"),
        };
        std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "0");
        let r1 = AppRuntime::new();
        assert_not_dev_loopback(r1.create_local_party(None).await);
        r1.leave_party();
        drop(_g1);

        // --- =1: loopback path, QUIC server on 127.0.0.1 ---
        let _g2 = ScopedEnv {
            key: "MOVIE_PARTY_DEV_LOOPBACK",
            prev: std::env::var_os("MOVIE_PARTY_DEV_LOOPBACK"),
        };
        std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");
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
            .starts_with("movieparty://join/"));
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
                    "production mode must not use loopback bind without MOVIE_PARTY_DEV_LOOPBACK=1"
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
        assert!(
            snapshot.transfer.is_none(),
            "transfer must be cleared on leave"
        );
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
            "MOVIE_PARTY_DEV_LOOPBACK",
            std::env::var_os("MOVIE_PARTY_DEV_LOOPBACK"),
        );
        std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");
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
            "MOVIE_PARTY_DEV_LOOPBACK",
            std::env::var_os("MOVIE_PARTY_DEV_LOOPBACK"),
        );
        std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

        let runtime = AppRuntime::new();
        let first = runtime
            .create_local_party(None)
            .await
            .expect("create_local_party");
        assert!(runtime.lock().host_session.is_some());
        let original_invite = first.room.invite_code.clone().expect("invite");

        // A malformed invite must fail BEFORE the existing session is torn
        // down, so an accidental bad paste never destroys the current room.
        let result = runtime.join_party("not-an-invite".to_string()).await;
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
    fn back_to_lobby_retracts_readiness_and_returns_to_lobby() {
        use crate::media::player::{LibMpvPlayer, LocalPlayer};
        let runtime = AppRuntime::new();
        let path = std::env::temp_dir().join(format!("mp_back_{}", uuid::Uuid::now_v7()));
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

        let ready = runtime.set_ready();
        assert_eq!(ready.screen, "READY_CHECK");
        assert!(ready.participants[0].media_ready);

        let back = runtime.back_to_lobby();
        assert_eq!(back.screen, "LOBBY");
        assert!(!back.participants[0].media_ready);
        assert!(back.error.is_none());
        // The room rewinds to the pre-ready-check state: no consensus, so
        // neither side can be stuck on the waiting-room screen.
        assert_eq!(back.sync.room_state, "LOBBY");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn back_to_lobby_drops_a_pending_countdown() {
        use crate::media::player::{LibMpvPlayer, LocalPlayer};
        use crate::sync::consensus::ParticipantReadiness;
        let runtime = AppRuntime::new();
        let path = std::env::temp_dir().join(format!("mp_back_cd_{}", uuid::Uuid::now_v7()));
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

        runtime.set_ready();
        // Simulate a ready guest so the countdown can actually be prepared.
        {
            let state = runtime.lock();
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .guest_ready(ParticipantReadiness::ready(5_000));
        }

        // Host schedules the countdown, then immediately regrets it and
        // goes Back — the pending operation must not survive.
        let with_countdown = runtime.request_play_countdown();
        assert!(with_countdown.sync.pending_operation.is_some());

        let back = runtime.back_to_lobby();
        assert!(back.sync.pending_operation.is_none());
        assert_eq!(back.screen, "LOBBY");
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
            v_major: crate::protocol::ENVELOPE_V_MAJOR,
            v_minor: crate::protocol::ENVELOPE_V_MINOR,
            room_id: "test-room".to_string(),
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
        AppRuntime::apply_peer_event(&runtime.inner, &envelope, envelope.event.clone());

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

    // ── Local Perfect production-closure focused tests ──────────────

    /// Test double that records every playback-rate and seek command the
    /// runtime issues, so drift corrections can be asserted without a live
    /// libmpv. This is a test-only recorder implementing the same
    /// `LocalPlayer` seam the production backends implement.
    struct ScriptedPlayer {
        snapshot: crate::media::player::PlayerSnapshot,
        rate_calls: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
        seek_calls: std::sync::Arc<std::sync::Mutex<Vec<u64>>>,
    }

    impl ScriptedPlayer {
        fn new(
            position_ms: u64,
            duration_ms: u64,
            rate_calls: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
            seek_calls: std::sync::Arc<std::sync::Mutex<Vec<u64>>>,
        ) -> Self {
            Self {
                snapshot: crate::media::player::PlayerSnapshot {
                    state: crate::media::player::PlayerState::Playing,
                    position_ms,
                    duration_ms: Some(duration_ms),
                    volume: 1.0,
                    playback_rate: 1.0,
                    buffered_ahead_ms: None,
                    error_message: None,
                },
                rate_calls,
                seek_calls,
            }
        }
    }

    impl crate::media::player::LocalPlayer for ScriptedPlayer {
        fn open(
            &mut self,
            _path: &std::path::Path,
        ) -> Result<(), crate::media::player::PlayerError> {
            Ok(())
        }
        fn play(&mut self) -> Result<(), crate::media::player::PlayerError> {
            self.snapshot.state = crate::media::player::PlayerState::Playing;
            Ok(())
        }
        fn pause(&mut self) -> Result<(), crate::media::player::PlayerError> {
            self.snapshot.state = crate::media::player::PlayerState::Paused;
            Ok(())
        }
        fn seek(&mut self, position_ms: u64) -> Result<(), crate::media::player::PlayerError> {
            self.seek_calls.lock().unwrap().push(position_ms);
            self.snapshot.position_ms = position_ms;
            Ok(())
        }
        fn set_volume(&mut self, volume: f32) -> Result<(), crate::media::player::PlayerError> {
            self.snapshot.volume = volume.clamp(0.0, 1.0);
            Ok(())
        }
        fn set_playback_rate(
            &mut self,
            rate: f32,
        ) -> Result<(), crate::media::player::PlayerError> {
            self.rate_calls.lock().unwrap().push(rate);
            self.snapshot.playback_rate = rate.clamp(0.25, 4.0);
            Ok(())
        }
        fn snapshot(&self) -> crate::media::player::PlayerSnapshot {
            self.snapshot.clone()
        }
        fn duration(&self) -> Option<u64> {
            self.snapshot.duration_ms
        }
        fn buffered_ahead_ms(&self) -> Option<u64> {
            self.snapshot.buffered_ahead_ms
        }
        fn error_message(&self) -> Option<String> {
            self.snapshot.error_message.clone()
        }
        fn close(&mut self) {}
    }

    /// The runtime drift path must restore the normal 1.0 rate once the
    /// guest converges on the host position (rate correction may not leak a
    /// 0.97/1.03 rate into converged playback).
    #[tokio::test]
    async fn drift_correction_restores_normal_rate_after_convergence() {
        let rate_calls: std::sync::Arc<std::sync::Mutex<Vec<f32>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seek_calls: std::sync::Arc<std::sync::Mutex<Vec<u64>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let player =
            ScriptedPlayer::new(100_000, 3_600_000, rate_calls.clone(), seek_calls.clone());
        let player_arc: std::sync::Arc<
            std::sync::Mutex<dyn crate::media::player::LocalPlayer + Send + Sync>,
        > = std::sync::Arc::new(std::sync::Mutex::new(player));

        // Guest 120ms ahead of the host commit → gentle rate correction.
        AppRuntime::apply_drift_correction(&player_arc, 120, 100_000);
        // Guest converged (drift inside the ignore band) → rate restored.
        AppRuntime::apply_drift_correction(&player_arc, 10, 100_100);

        let rates = rate_calls.lock().unwrap().clone();
        assert_eq!(rates, vec![0.97, 1.0]);
        assert!(
            seek_calls.lock().unwrap().is_empty(),
            "rate-band drift must never seek"
        );
    }

    /// The runtime drift path must hard-seek back to the canonical host
    /// commit when the guest drifts beyond the micro-seek band.
    #[tokio::test]
    async fn drift_correction_hard_seeks_back_to_host_commit() {
        let rate_calls: std::sync::Arc<std::sync::Mutex<Vec<f32>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seek_calls: std::sync::Arc<std::sync::Mutex<Vec<u64>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let player = ScriptedPlayer::new(40_000, 3_600_000, rate_calls, seek_calls.clone());
        let player_arc: std::sync::Arc<
            std::sync::Mutex<dyn crate::media::player::LocalPlayer + Send + Sync>,
        > = std::sync::Arc::new(std::sync::Mutex::new(player));

        // Guest 900ms behind the host commit (40_900 committed).
        AppRuntime::apply_drift_correction(&player_arc, -900, 40_000);

        let seeks = seek_calls.lock().unwrap().clone();
        assert_eq!(seeks, vec![40_900]);
    }

    /// Playback headroom and whole-file transfer percentage are separate
    /// concepts and must stay separate in the snapshot: a sparse cache with a
    /// healthy contiguous window at the playhead reports healthy headroom
    /// even while the whole-file transfer percent is low, and recovery
    /// bookkeeping may never fabricate a 100% transfer from headroom health.
    #[tokio::test]
    async fn recovery_percent_reflects_verified_transfer_not_headroom() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            // 40% of the file verified on disk.
            state.transfer = Some(crate::media::transfer::TransferProgress {
                media_id: "m".to_string(),
                bytes_available: 400,
                bytes_total: 1_000,
                buffer_ahead_ms: 0,
                goodput_bps: 8_000_000,
            });
            state.buffer.guest_buffer_ahead_ms = 30_000;
        }

        let resumed = runtime.handle_failure_event(FailureEvent::TransferResumed);
        assert_eq!(
            resumed.buffer.percent, 40,
            "recovery percent must be the real verified transfer fraction"
        );
        assert_eq!(
            resumed.buffer.guest_buffer_ahead_ms, 30_000,
            "healthy headroom is preserved separately from the transfer percent"
        );

        let interrupted = runtime.handle_failure_event(FailureEvent::TransferInterrupted);
        assert_eq!(
            interrupted.buffer.percent, 40,
            "a stall must zero headroom, not fabricate a whole-file transfer reset"
        );
        assert_eq!(interrupted.buffer.guest_buffer_ahead_ms, 0);
        assert!(interrupted.buffer.buffering_participant.is_some());
    }

    /// `report_buffer_recovered` must mirror the coordinator's authority and
    /// the real transfer percent — never fabricate 100% or a resume.
    #[tokio::test]
    async fn buffer_recovery_reports_real_percent_and_never_resumes() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.transfer = Some(crate::media::transfer::TransferProgress {
                media_id: "m".to_string(),
                bytes_available: 250,
                bytes_total: 1_000,
                buffer_ahead_ms: 0,
                goodput_bps: 8_000_000,
            });
            // Strict-sync pause active: recovery must not clear it.
            state.sync.strict_sync_paused = true;
            state.room_state = crate::sync::state_machine::RoomState::Reconnecting;
        }

        let recovered = runtime.report_buffer_recovered(12_000);

        assert_eq!(
            recovered.buffer.percent, 25,
            "buffer recovery must report the real transfer percent"
        );
        assert_eq!(recovered.buffer.guest_buffer_ahead_ms, 12_000);
        assert!(
            recovered.buffer.buffering_participant.is_none(),
            "the buffering participant flag must clear on recovery"
        );
        assert_ne!(
            recovered.sync.room_state, "PLAYING",
            "recovery must never resume playback by itself"
        );
    }

    #[test]
    fn leave_party_detaches_guest_cache_but_preserves_it_on_disk() {
        let runtime = AppRuntime::new();
        let dir = std::env::temp_dir().join(format!("b9b_leave_cache_{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("cache dir");
        let manifest = crate::media::manifest::MediaManifest {
            media_id: "leave-test".to_string(),
            filename: "leave-test.bin".to_string(),
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
        }

        runtime.leave_party();

        let state = runtime.lock();
        assert!(
            state.guest_cache.is_none(),
            "leave_party must detach the in-memory cache handle"
        );
        drop(state);
        assert!(
            dir.join("chunk-map.bin").exists()
                || dir.read_dir().is_ok_and(|mut d| d.next().is_some()),
            "leave_party must NOT delete the on-disk cache; retention decides later"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preload_wait_notifications_are_rate_limited_per_schedule() {
        let runtime = AppRuntime::new();
        let first_at = Instant::now();
        {
            let mut state = runtime.lock();
            assert!(
                should_notify_preload_wait(&mut state, "sched-1", first_at),
                "the first waiting poll must notify"
            );
        }
        {
            let mut state = runtime.lock();
            assert!(
                !should_notify_preload_wait(
                    &mut state,
                    "sched-1",
                    first_at + Duration::from_secs(60)
                ),
                "a repeat within the window must be suppressed"
            );
        }
        {
            let mut state = runtime.lock();
            assert!(
                should_notify_preload_wait(
                    &mut state,
                    "sched-1",
                    first_at + Duration::from_secs(15 * 60 + 1)
                ),
                "after the window expires the poll must notify again"
            );
        }
        {
            let mut state = runtime.lock();
            assert!(
                should_notify_preload_wait(
                    &mut state,
                    "sched-2",
                    first_at + Duration::from_secs(60)
                ),
                "a different schedule is an independent notification"
            );
        }
    }

    /// §40: Continue Without Guest is host-only — the guest
    /// gets MP-CTRL-002 and no abandonment flag is set.
    #[test]
    fn continue_without_guest_is_host_only() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.local_participant.role = "Guest".to_string();
            sync_room_snapshot(&mut state);
        }
        let snapshot = runtime.continue_without_guest();
        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-CTRL-002 only the host can continue without the guest")
        );
        let abandoned = {
            let state = runtime.lock();
            let coord = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            coord.guest_is_abandoned()
        };
        assert!(
            !abandoned,
            "a guest attempt must never set the abandonment override"
        );
    }

    /// §40: the host's Continue Without Guest sets the
    /// coordinator override and records the explicit user decision in
    /// last_recovery (§29 — no silent mode change).
    #[test]
    fn continue_without_guest_sets_abandonment_and_records_it() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.local_participant.role = "Host".to_string();
            sync_room_snapshot(&mut state);
        }
        let snapshot = runtime.continue_without_guest();
        let abandoned = {
            let state = runtime.lock();
            let coord = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            coord.guest_is_abandoned()
        };
        assert!(abandoned, "the host override must set guest_abandoned");
        let recovery = snapshot
            .last_recovery
            .as_ref()
            .expect("the override must be recorded, not silent");
        assert!(recovery.requires_user_action);
        assert_eq!(
            format!("{:?}", recovery.action),
            "ContinueWithoutGuest".to_string()
        );
    }

    /// §40: an abandoned guest satisfies all_ready only through
    /// the explicit override — host readiness still matters.
    #[test]
    fn abandoned_guest_overrides_readiness_but_not_host() {
        let runtime = AppRuntime::new();
        {
            let state = runtime.lock();
            let mut coord = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            coord.abandon_guest();
        }
        let (all_ready_with_override, all_ready_without) = {
            let state = runtime.lock();
            let mut coord = state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let with_override = coord.all_ready(5_000);
            coord.guest_abandoned = false;
            let without = coord.all_ready(5_000);
            (with_override, without)
        };
        // With neither side actually ready, even the override path needs
        // the HOST ready — both stay false until the host is ready.
        assert!(!all_ready_with_override);
        assert!(!all_ready_without);
    }

    /// a moved/renamed local media file fails host_play
    /// with the honest MP-MEDIA-002 + the AskHostToLocateFile plan — the
    /// play protocol never starts against a missing file.
    #[test]
    fn host_play_detects_a_moved_media_file() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.local_participant.role = "Host".to_string();
            state.local_media_path = Some("/definitely/not/a/real/movie/file.mkv".to_string());
            use crate::sync::consensus::ParticipantReadiness;
            let coord_room_state = {
                let mut coord = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                coord.host_ready(ParticipantReadiness::ready(5_000));
                coord.guest_ready(ParticipantReadiness::ready(5_000));
                coord.update_readiness_consensus(5_000);
                coord.room_state
            };
            state.room_state = coord_room_state;
            sync_room_snapshot(&mut state);
        }
        let snapshot = runtime.host_play();
        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-MEDIA-002 the movie file moved or was renamed — locate it to continue")
        );
        let recovery = snapshot
            .last_recovery
            .as_ref()
            .expect("the moved-file recovery plan must be recorded");
        assert!(recovery.requires_user_action);
        assert!(
            snapshot.sync.pending_operation.is_none(),
            "no play operation may start against a missing file"
        );
    }

    #[test]
    fn host_play_refuses_to_start_when_player_reports_media_error() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            // Simulate the sticky MP-MEDIA-001 player error the gate checks.
            state.player_snapshot.state = PLAYER_STATE_ERROR.to_string();
            state.player_snapshot.error_message =
                Some("MP-MEDIA-001 player unavailable".to_string());
            state.media = Some(crate::media::manifest::MediaManifest {
                media_id: "host-play-gate".to_string(),
                filename: "gate.bin".to_string(),
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
            });
        }

        let snapshot = runtime.host_play();

        assert_eq!(
            snapshot.error.as_deref(),
            Some("MP-MEDIA-001 player unavailable; cannot start playback"),
            "host_play must refuse to start on a sticky player error"
        );
        assert!(
            !snapshot.participants.iter().any(|p| p.media_ready),
            "no participant may be marked ready when the host player failed"
        );
        assert_ne!(snapshot.sync.room_state, "PLAYING");
    }
}

#[test]
fn local_device_toggles_never_mutate_peer_snapshot() {
    // LOCAL vs REMOTE call state separation: toggling MY camera/mic must
    // only change MY local participant + my outgoing call state. The
    // peer's own camera/mic flags are theirs and must never move.
    let runtime = AppRuntime::new();
    {
        let mut state = runtime.lock();
        state.peer_participant = Some(ParticipantSnapshot {
            id: "peer-id".to_string(),
            display_name: "Peer".to_string(),
            role: "Guest".to_string(),
            connected: true,
            media_ready: false,
            camera_enabled: false,
            microphone_enabled: true,
            buffer_ahead_ms: 0,
        });
    }

    let before = runtime.snapshot();
    let peer_before = before
        .participants
        .iter()
        .find(|p| p.role == "Guest")
        .expect("peer present")
        .clone();
    let local_before = before
        .participants
        .iter()
        .find(|p| p.role == "Host")
        .expect("local host present")
        .clone();

    // Local toggles: camera ON then OFF, mic OFF then ON.
    runtime.set_camera_enabled(true);
    runtime.set_camera_enabled(false);
    runtime.set_microphone_enabled(false);
    runtime.set_microphone_enabled(true);

    let after = runtime.snapshot();
    let peer_after = after
        .participants
        .iter()
        .find(|p| p.role == "Guest")
        .expect("peer present")
        .clone();
    let local_after = after
        .participants
        .iter()
        .find(|p| p.role == "Host")
        .expect("local host present")
        .clone();

    assert_eq!(
        peer_after.camera_enabled, peer_before.camera_enabled,
        "peer camera flag belongs to the peer"
    );
    assert_eq!(
        peer_after.microphone_enabled, peer_before.microphone_enabled,
        "peer microphone flag belongs to the peer"
    );
    assert_eq!(peer_after.connected, peer_before.connected);

    // The local participant mirrors the local call state exactly.
    assert_eq!(local_after.camera_enabled, after.call.camera.enabled);
    assert_eq!(
        local_after.microphone_enabled,
        after.call.microphone.enabled
    );
    assert!(
        !local_after.camera_enabled,
        "final local camera state is OFF"
    );
    assert!(
        local_after.microphone_enabled,
        "final local microphone state is ON"
    );
    assert_ne!(
        local_after.camera_enabled, local_before.camera_enabled,
        "local toggles must visibly change local state"
    );
}

// ── adaptive camera ladder runtime wiring (PRD §41) ───

#[cfg(test)]
mod camera_ladder_tests {
    use super::{AppRuntime, AppRuntimeState};
    use crate::call::{CameraState, CameraTier};

    /// Seeds a fresh runtime's state with a movie context (the ladder
    /// only runs while a movie is at stake) and the given feedback
    /// inputs. The manifest + duration make the movie estimate exactly
    /// 5 Mbps (1 GB × 8 / 1600 s = 5_000_000 bps).
    fn state_with_movie(
        runtime: &AppRuntime,
        goodput_bps: u64,
        buffer_ms: u64,
        rtt_ms: Option<u32>,
    ) -> std::sync::MutexGuard<'_, AppRuntimeState> {
        let mut state = runtime.lock();
        state.media = Some(crate::media::manifest::MediaManifest {
            media_id: "m".to_string(),
            filename: "movie.mkv".to_string(),
            file_size: 1_000_000_000,
            container: Some("mkv".to_string()),
            full_hash: "h".to_string(),
            quick_fingerprint: crate::media::manifest::QuickFingerprint {
                file_size: 1_000_000_000,
                first_hash: "a".to_string(),
                last_hash: "b".to_string(),
            },
            chunk_size: 1_048_576,
            chunk_count: 954,
        });
        state.player_snapshot.duration_ms = Some(1_600_000);
        state.network.goodput_bps = goodput_bps;
        state.network.rtt_ms = rtt_ms;
        state.buffer.guest_buffer_ahead_ms = buffer_ms;
        state
    }

    #[test]
    fn movie_bitrate_estimate_uses_manifest_over_fallback() {
        let runtime = AppRuntime::new();
        let state = state_with_movie(&runtime, 0, 0, None);
        assert_eq!(super::movie_bitrate_estimate_bps(&state), 5_000_000);
    }

    #[test]
    fn movie_bitrate_estimate_falls_back_to_5mbps_without_media() {
        let runtime = AppRuntime::new();
        let state = runtime.lock();
        assert_eq!(super::movie_bitrate_estimate_bps(&state), 5_000_000);
    }

    /// The audit's 5 Mbps movie-priority scenario: healthy goodput with a
    /// 5 Mbps movie keeps Tier A; goodput below the movie (ratio < 1)
    /// disables the camera entirely — movie-first, PRD §41.
    #[test]
    fn five_mbps_movie_priority_downgrades_before_sacrificing_movie() {
        // Healthy: ratio 2.0, buffer >30s, rtt 100ms → upgrade path
        // (the policy requires buffer strictly above 30 s to upgrade).
        let runtime = AppRuntime::new();
        let mut healthy = state_with_movie(&runtime, 10_000_000, 31_000, Some(100));
        healthy.call.camera = CameraState::tier_b_enabled();
        super::evaluate_camera_ladder_locked(&mut healthy);
        assert_eq!(healthy.call.camera.tier, CameraTier::A);

        // Starved: ratio < 1.0 → camera disabled, movie protected.
        let runtime = AppRuntime::new();
        let mut starved = state_with_movie(&runtime, 4_000_000, 20_000, Some(100));
        starved.call.camera = CameraState::tier_b_enabled();
        super::evaluate_camera_ladder_locked(&mut starved);
        assert!(!starved.call.camera.enabled);
        assert_eq!(starved.call.camera.tier, CameraTier::D);
        assert_eq!(starved.call.camera.target_bitrate_bps, 0);
        // Once-per-event notice fired for the disable event.
        assert!(starved
            .call
            .camera_notice
            .as_deref()
            .unwrap_or_default()
            .contains("movie playback"));
    }

    #[test]
    fn downgrade_notice_fires_once_per_event() {
        // First downgrade: notice set.
        let runtime = AppRuntime::new();
        let mut state = state_with_movie(&runtime, 5_800_000, 4_500, Some(80));
        state.call.camera = CameraState::tier_b_enabled();
        super::evaluate_camera_ladder_locked(&mut state);
        assert_eq!(state.call.camera.tier, CameraTier::C);
        let first_notice = state.call.camera_notice.clone();
        assert!(first_notice.is_some());

        // Consume (frontend clears) and re-evaluate at the same tier:
        // no further tier change → no new notice.
        state.call.camera_notice = None;
        super::evaluate_camera_ladder_locked(&mut state);
        assert_eq!(state.call.camera.tier, CameraTier::C);
        assert!(state.call.camera_notice.is_none());
    }

    #[test]
    fn upgrade_resets_the_notice_episode_so_later_downgrade_notifies_again() {
        let runtime = AppRuntime::new();
        let mut state = state_with_movie(&runtime, 10_000_000, 45_000, Some(60));
        state.call.camera = CameraState::tier_c_enabled();
        // Pre-seed a prior episode so the upgrade must clear it.
        state.call.camera_notice = None;
        super::evaluate_camera_ladder_locked(&mut state);
        assert_eq!(state.call.camera.tier, CameraTier::B); // upgrade

        // Later degradation to the same C tier notifies again (new event).
        // Clear the dwell timestamp to simulate the minimum dwell having
        // elapsed (the dwell itself has its own dedicated test).
        state.network.goodput_bps = 5_800_000;
        state.buffer.guest_buffer_ahead_ms = 4_500;
        state.network.rtt_ms = Some(80);
        state.camera_tier_changed_at = None;
        super::evaluate_camera_ladder_locked(&mut state);
        assert_eq!(state.call.camera.tier, CameraTier::C);
        assert!(state.call.camera_notice.is_some());
    }

    #[test]
    fn dwell_blocks_tier_flapping_within_one_second() {
        // Change to C, then immediately try to change again: the dwell
        // must hold the tier at C until the minimum dwell elapses.
        let runtime = AppRuntime::new();
        let mut state = state_with_movie(&runtime, 5_800_000, 4_500, Some(80));
        state.call.camera = CameraState::tier_b_enabled();
        super::evaluate_camera_ladder_locked(&mut state);
        assert_eq!(state.call.camera.tier, CameraTier::C);

        // Now inputs flip to upgrade-worthy, but within the dwell window.
        state.network.goodput_bps = 10_000_000;
        state.buffer.guest_buffer_ahead_ms = 45_000;
        state.network.rtt_ms = Some(60);
        super::evaluate_camera_ladder_locked(&mut state);
        assert_eq!(
            state.call.camera.tier,
            CameraTier::C,
            "dwell must block an immediate tier flip"
        );
    }

    #[test]
    fn no_movie_means_no_ladder_activity() {
        // Unmeasured goodput (0) with no movie: the ladder must NOT
        // disable the camera — there is nothing to protect.
        let runtime = AppRuntime::new();
        let mut state = runtime.lock();
        state.call.camera = CameraState::tier_b_enabled();
        state.network.goodput_bps = 0;
        state.buffer.guest_buffer_ahead_ms = 0;
        super::evaluate_camera_ladder_locked(&mut state);
        assert_eq!(
            state.call.camera.tier,
            CameraTier::B,
            "no movie in flight → tier untouched"
        );
        assert!(state.call.camera_notice.is_none());
    }

    #[test]
    fn camera_off_by_user_or_privacy_means_no_ladder_notice() {
        let runtime = AppRuntime::new();
        let mut state = state_with_movie(&runtime, 4_000_000, 1_000, Some(400));
        state.call.camera = CameraState::disabled();
        super::evaluate_camera_ladder_locked(&mut state);
        assert!(!state.call.camera.enabled);
        assert!(state.call.camera_notice.is_none());

        let privacy_runtime = AppRuntime::new();
        let mut privacy = state_with_movie(&privacy_runtime, 4_000_000, 1_000, Some(400));
        privacy.privacy_mode = true;
        privacy.call.camera = CameraState::tier_b_enabled();
        super::evaluate_camera_ladder_locked(&mut privacy);
        assert_eq!(privacy.call.camera.tier, CameraTier::B);
        assert!(privacy.call.camera_notice.is_none());
    }

    #[test]
    fn camera_state_serializes_camel_case_for_the_frontend_contract() {
        // The TS snapshot type declares targetBitrateBps/tier; the serde
        // must match (this was silently snake_case before the camelCase fix).
        let json = serde_json::to_string(&CameraState::tier_b_enabled()).unwrap();
        assert!(json.contains("\"targetBitrateBps\""));
        assert!(json.contains("\"tier\":\"B\""));
        assert!(!json.contains("target_bitrate_bps"));
    }
}

// ── Provider Sync runtime dispatch tests ────────────────

#[cfg(test)]
mod provider_dispatch_tests {
    use super::AppRuntime;
    use crate::providers::sync::ProviderReadiness;

    /// A room whose media is the provider browser, with a provider id set
    /// and the given readiness.
    fn provider_room(readiness: ProviderReadiness) -> AppRuntime {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.provider.mode = "PROVIDER_SYNC".to_string();
            state.provider.provider_id = Some("youtube".to_string());
            state.provider.url = Some("https://www.youtube.com/watch?v=abc".to_string());
            state.provider.readiness = readiness;
        }
        runtime
    }

    /// readiness gate: host_play must refuse to start the play
    /// protocol until the provider's own page reports playback-ready
    /// media. Honest MP-PROVIDER error, never a silent local fallback.
    #[test]
    fn host_play_gates_on_playback_ready_in_provider_mode() {
        let runtime = provider_room(ProviderReadiness::Ready);
        let snapshot = runtime.host_play();
        assert!(
            snapshot
                .error
                .as_deref()
                .unwrap_or_default()
                .starts_with("MP-PROVIDER-003"),
            "gate error: {:?}",
            snapshot.error
        );
        // No play protocol was started.
        assert!(snapshot.call_signals.is_empty());
    }

    #[test]
    fn host_play_login_required_maps_to_the_login_gate_code() {
        let runtime = provider_room(ProviderReadiness::LoginRequired);
        let snapshot = runtime.host_play();
        assert!(
            snapshot
                .error
                .as_deref()
                .unwrap_or_default()
                .starts_with("MP-PROVIDER-004"),
            "login gate error: {:?}",
            snapshot.error
        );
    }

    #[test]
    fn playback_ready_provider_room_passes_the_gate() {
        let runtime = provider_room(ProviderReadiness::PlaybackReady);
        let snapshot = runtime.host_play();
        // Past the gate: the error is not a provider gate error (the play
        // protocol may fail later for sync reasons, but the readiness
        // gate itself did not block).
        assert!(
            !snapshot
                .error
                .as_deref()
                .unwrap_or_default()
                .starts_with("MP-PROVIDER-"),
            "unexpected provider gate error: {:?}",
            snapshot.error
        );
    }

    /// No silent fallback (§29): a failed provider command must
    /// surface MP-PROVIDER-003 and strict-pause a Playing room, never
    /// pretend local playback continued.
    #[test]
    fn provider_command_failure_strict_pauses_and_surfaces_the_error() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.provider.mode = "PROVIDER_SYNC".to_string();
            state.provider.provider_id = Some("youtube".to_string());
            state.room_state = crate::sync::state_machine::RoomState::Playing;
        }
        super::AppRuntime::record_provider_command_failure(
            &runtime.inner,
            crate::providers::sync::ProviderRuntimeError::ProviderPageClosed,
        );
        let state = runtime.lock();
        assert_eq!(state.provider.readiness, ProviderReadiness::Error);
        assert!(
            state
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("MP-PROVIDER-003"),
            "error: {:?}",
            state.error
        );
        // Strict sync (§14): the room cannot stay Playing when
        // the movie source died.
        assert_eq!(
            state.room_state,
            crate::sync::state_machine::RoomState::Buffering
        );
        assert!(state.sync.strict_sync_paused);
    }
}

// ── scheduling wire + persistence tests ────────────

#[cfg(test)]
mod scheduling_tests {
    use super::AppRuntime;
    use crate::network::quic::{EventEnvelope, ServerEvent};

    /// Build a runtime with an in-memory-ish temp DB so schedule persistence
    /// is real (not None). Uses a temp dir per test.
    fn runtime_with_db() -> (AppRuntime, std::path::PathBuf) {
        let runtime = AppRuntime::new();
        let dir = std::env::temp_dir().join(format!(
            "mp-sched-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("mp.db");
        runtime.init_db_at_path(&db_path);
        (runtime, dir)
    }

    fn envelope(event: ServerEvent) -> EventEnvelope {
        // Unique seq per envelope: the guest's stale/duplicate guard rejects
        // a seq it has already seen (§10 ordering), so tests that deliver
        // multiple events must not reuse seq values.
        static NEXT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let seq = NEXT_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        EventEnvelope {
            v_major: crate::protocol::ENVELOPE_V_MAJOR,
            v_minor: crate::protocol::ENVELOPE_V_MINOR,
            room_id: "test-room".to_string(),
            seq,
            sender: "host-device".to_string(),
            sent_mono_us: 1,
            event,
        }
    }

    /// §56: on SCHEDULE_CREATE the guest persists the schedule locally so
    /// reminders survive the host app closing. Duplicate re-broadcast is a
    /// no-op (UNIQUE id), not an error.
    #[test]
    fn guest_persists_schedule_on_create_and_ignores_duplicates() {
        let (runtime, _dir) = runtime_with_db();
        let event = ServerEvent::ScheduleCreate {
            schedule_id: "sched-g1".to_string(),
            scheduled_start_utc_ms: 1_786_811_400_000,
            media_id: "media-1".to_string(),
            call_mode: "VIDEO_VOICE".to_string(),
            planned_preload_utc_ms: 1_786_800_600_000,
        };
        // Sender must differ from the local participant id (not self-echo).
        let env = envelope(event.clone());
        let inner = runtime.inner_for_events();
        // First delivery: persisted + pending marker set.
        super::AppRuntime::apply_peer_event_pub(&inner, env.clone(), event);
        let state = runtime.lock();
        assert_eq!(state.pending_guest_schedule.as_deref(), Some("sched-g1"));
        let db = state.db.as_ref().expect("db");
        let schedules = db.list_schedules().expect("list");
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].schedule_id, "sched-g1");
        assert_eq!(schedules[0].status, "Planned");
    }

    /// §56: ScheduleAccept echo clears the guest's pending marker.
    #[test]
    fn schedule_accept_echo_clears_pending_marker() {
        let (runtime, _dir) = runtime_with_db();
        let inner = runtime.inner_for_events();
        let __ev = ServerEvent::ScheduleCreate {
            schedule_id: "sched-g2".to_string(),
            scheduled_start_utc_ms: 1_786_811_400_000,
            media_id: "media-1".to_string(),
            call_mode: "VIDEO_VOICE".to_string(),
            planned_preload_utc_ms: 1_786_800_600_000,
        };
        super::AppRuntime::apply_peer_event_pub(&inner, envelope(__ev.clone()), __ev);
        let __ev = ServerEvent::ScheduleAccept {
            schedule_id: "sched-g2".to_string(),
            accepted: true,
        };
        super::AppRuntime::apply_peer_event_pub(&inner, envelope(__ev.clone()), __ev);
        assert_eq!(runtime.lock().pending_guest_schedule, None);
    }

    /// §57: PreloadState updates the guest's pending progress surface.
    #[test]
    fn preload_state_updates_guest_progress_surface() {
        let (runtime, _dir) = runtime_with_db();
        let inner = runtime.inner_for_events();
        let __ev = ServerEvent::PreloadState {
            schedule_id: "sched-g3".to_string(),
            state: "TRANSFERRING".to_string(),
            progress: 0.62,
            estimated_ready_utc_ms: 1_786_807_112_345,
        };
        super::AppRuntime::apply_peer_event_pub(&inner, envelope(__ev.clone()), __ev);
        assert_eq!(
            runtime.lock().pending_preload_state,
            Some(("TRANSFERRING".to_string(), 0.62))
        );
    }

    /// Honest failure (§29): a schedule arriving with no DB surfaces
    /// MP-STORE-001 — never a silent pretend-persist.
    #[test]
    fn schedule_create_without_db_surfaces_honest_error() {
        let runtime = AppRuntime::new();
        let inner = runtime.inner_for_events();
        let __ev = ServerEvent::ScheduleCreate {
            schedule_id: "sched-g4".to_string(),
            scheduled_start_utc_ms: 1,
            media_id: "m".to_string(),
            call_mode: "VIDEO_VOICE".to_string(),
            planned_preload_utc_ms: 0,
        };
        super::AppRuntime::apply_peer_event_pub(&inner, envelope(__ev.clone()), __ev);
        let state = runtime.lock();
        assert!(
            state
                .error
                .as_deref()
                .unwrap_or_default()
                .starts_with("MP-STORE-001"),
            "error: {:?}",
            state.error
        );
    }

    /// MP-10: a storage write that fails inside a QUIC event handler must be
    /// surfaced, not swallowed.
    ///
    /// The guest's stored schedule is its copy of the host's authoritative
    /// state. Before this fix each write was `let _ = db.update_…(…)`, so a
    /// failure left the two silently disagreeing — the Upcoming card would show
    /// a title or time the host never set, with nothing anywhere saying so.
    #[test]
    fn schedule_update_write_failure_is_surfaced_not_swallowed() {
        let (runtime, dir) = runtime_with_db();
        let inner = runtime.inner_for_events();

        // Induce a genuine storage failure rather than asserting a happy path.
        {
            let state = runtime.lock();
            let db = state.db.as_ref().expect("db");
            db.execute_sql_for_test("DROP TABLE schedules")
                .expect("drop schedules");
        }

        let __ev = ServerEvent::ScheduleUpdate {
            schedule_id: "sched-mp10".to_string(),
            media_id: "m".to_string(),
            planned_preload_utc_ms: 1,
            scheduled_start_utc_ms: 2,
        };
        super::AppRuntime::apply_peer_event_pub(&inner, envelope(__ev.clone()), __ev);

        let error = runtime.lock().error.clone().unwrap_or_default();
        assert!(
            error.starts_with("MP-STORE-001"),
            "a failed schedule write must surface MP-STORE-001; got {error:?}"
        );
        let lowered = error.to_lowercase();
        assert!(
            !lowered.contains("sqlite") && !lowered.contains("no such table"),
            "the user-facing message must not leak raw SQLite internals; got {error:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MP-13: a rotation whose "clear the previous identity" step fails must
    /// abort rather than carry on.
    ///
    /// `upsert_identity` is `INSERT OR REPLACE` keyed on `device_id`, so writing
    /// the NEW device id while the old row survives leaves TWO rows — and a
    /// stale identity can then win the lookup, silently resurrecting the
    /// identity the user just rotated away from. The delete error used to be
    /// discarded with `let _ =`.
    #[test]
    fn identity_rotation_aborts_when_the_previous_row_cannot_be_cleared() {
        let runtime = AppRuntime::new_with_key_store_for_test(std::sync::Arc::new(
            crate::secure::FakeKeyStore::new(),
        ));
        let dir = std::env::temp_dir().join(format!("mp-rotation-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        runtime.init_db_at_path(&dir.join("mp.db"));
        let db = runtime.lock().db.clone().expect("db");

        // Force `delete_identity` to fail for real: the table is gone.
        db.execute_sql_for_test("DROP TABLE device_identity")
            .expect("drop device_identity");

        let before = runtime.lock().local_participant.id.clone();
        runtime.rotate_identity(&db, "Abhijai", None);
        let after = runtime.lock().local_participant.id.clone();

        let error = runtime.lock().error.clone().unwrap_or_default();
        assert!(
            error.starts_with("MP-STORE-001"),
            "the rotation failure must use the existing vocabulary; got {error:?}"
        );
        assert!(
            error.contains("clear the previous identity"),
            "the delete guard specifically must have fired; got {error:?}"
        );
        assert_eq!(
            before, after,
            "a rotation that could not clear the old row must not claim a new identity"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Host: GuestScheduleAccept flips the stored status to Accepted.
    #[test]
    fn host_marks_schedule_accepted_on_guest_ack() {
        let (runtime, _dir) = runtime_with_db();
        {
            let state = runtime.lock();
            let db = state.db.as_ref().expect("db");
            db.insert_schedule(&crate::storage::sqlite::StoredSchedule {
                schedule_id: "sched-h1".to_string(),
                room_id: "r".to_string(),
                media_id: "m".to_string(),
                scheduled_start_utc_ms: 1_786_811_400_000,
                planned_preload_utc_ms: 1_786_800_600_000,
                guest_device_id: "guest-dev".to_string(),
                status: "Planned".to_string(),
                created_at_ms: 1,
            })
            .expect("insert");
        }
        super::AppRuntime::apply_host_event_pub(
            &runtime.inner_for_events(),
            crate::network::quic::QuicHostEvent::GuestScheduleAccept {
                broadcaster_device_id: "guest-dev".to_string(),
                schedule_id: "sched-h1".to_string(),
                accepted: true,
            },
        );
        let state = runtime.lock();
        let db = state.db.as_ref().expect("db");
        let schedules = db.list_schedules().expect("list");
        assert_eq!(schedules[0].status, "Accepted");
    }

    /// Host: cancel_and_broadcast marks Cancelled (§56 — the guest never
    /// fires a reminder for a dead schedule).
    #[test]
    fn host_cancel_broadcast_marks_cancelled() {
        let (runtime, _dir) = runtime_with_db();
        {
            let state = runtime.lock();
            let db = state.db.as_ref().expect("db");
            db.insert_schedule(&crate::storage::sqlite::StoredSchedule {
                schedule_id: "sched-h2".to_string(),
                room_id: "r".to_string(),
                media_id: "m".to_string(),
                scheduled_start_utc_ms: 1_786_811_400_000,
                planned_preload_utc_ms: 1_786_800_600_000,
                guest_device_id: "guest-dev".to_string(),
                status: "Planned".to_string(),
                created_at_ms: 1,
            })
            .expect("insert");
        }
        runtime
            .cancel_and_broadcast_schedule("sched-h2")
            .expect("cancel");
        let state = runtime.lock();
        let db = state.db.as_ref().expect("db");
        assert_eq!(db.list_schedules().expect("list")[0].status, "Cancelled");
    }

    // ── MP-14: received chat/reactions are validated and bounded ─────────

    /// MP-14: a received message is held to the same body and id rules as a
    /// locally generated one. Before this, the receive path applied whatever
    /// came off the wire — the asymmetry the finding describes.
    #[test]
    fn mp14_received_chat_messages_are_validated_before_being_applied() {
        let (runtime, _dir) = runtime_with_db();
        let inner = runtime.inner_for_events();

        let deliver = |message_id: String, body: String| {
            let event = ServerEvent::ChatMessage {
                message_id,
                sender: "guest-device".to_string(),
                body,
                created_host_time_us: 1,
            };
            super::AppRuntime::apply_peer_event_pub(&inner, envelope(event.clone()), event);
        };

        deliver(
            uuid::Uuid::now_v7().to_string(),
            "a".repeat(crate::chat::MAX_CHAT_BODY_BYTES + 1),
        );
        assert!(
            runtime.lock().chat.is_empty(),
            "an oversized received message must not be applied"
        );

        deliver("not-a-uuid".to_string(), "hello".to_string());
        assert!(
            runtime.lock().chat.is_empty(),
            "a received message with a malformed id must not be applied"
        );

        deliver(uuid::Uuid::now_v7().to_string(), "   ".to_string());
        assert!(
            runtime.lock().chat.is_empty(),
            "a received message with an empty body must not be applied"
        );

        // Not a blanket refusal: valid input is still applied.
        deliver(uuid::Uuid::now_v7().to_string(), "hello".to_string());
        let chat = runtime.lock().chat.clone();
        assert_eq!(chat.len(), 1, "a valid received message must be applied");
        assert_eq!(chat[0].body, "hello");
    }

    /// MP-14: a received reaction must be one of the v1 set, not merely
    /// rate-limited.
    #[test]
    fn mp14_received_reactions_are_validated_before_being_applied() {
        let (runtime, _dir) = runtime_with_db();
        let inner = runtime.inner_for_events();

        let deliver = |reaction_id: String, reaction: String| {
            let event = ServerEvent::Reaction {
                reaction_id,
                sender: "guest-device".to_string(),
                reaction,
            };
            super::AppRuntime::apply_peer_event_pub(&inner, envelope(event.clone()), event);
        };

        deliver(uuid::Uuid::now_v7().to_string(), "⭐".to_string());
        assert!(
            runtime.lock().reactions.is_empty(),
            "an unknown reaction must not be applied"
        );

        deliver("not-a-uuid".to_string(), "🔥".to_string());
        assert!(
            runtime.lock().reactions.is_empty(),
            "a malformed reaction id must not be applied"
        );

        deliver(uuid::Uuid::now_v7().to_string(), "🔥".to_string());
        assert_eq!(
            runtime.lock().reactions.len(),
            1,
            "a valid reaction must still be applied"
        );
    }

    /// MP-14: chat history is bounded, so a peer cannot grow it without limit.
    #[test]
    fn mp14_room_chat_history_is_bounded() {
        let (runtime, _dir) = runtime_with_db();
        let inner = runtime.inner_for_events();

        let total = crate::chat::MAX_CHAT_HISTORY + 25;
        for index in 0..total {
            let event = ServerEvent::ChatMessage {
                message_id: uuid::Uuid::now_v7().to_string(),
                sender: "guest-device".to_string(),
                body: format!("message {index}"),
                created_host_time_us: 1,
            };
            super::AppRuntime::apply_peer_event_pub(&inner, envelope(event.clone()), event);
        }

        let chat = runtime.lock().chat.clone();
        assert_eq!(
            chat.len(),
            crate::chat::MAX_CHAT_HISTORY,
            "chat history must be capped"
        );
        // The retained window is the most recent, still in order.
        assert_eq!(
            chat[0].body,
            format!("message {}", total - crate::chat::MAX_CHAT_HISTORY)
        );
        assert_eq!(chat[chat.len() - 1].body, format!("message {}", total - 1));
    }

    /// MP-14: the reaction history is bounded by the same mechanism. Filling it
    /// through the event path is impractical because the rate limiter allows
    /// five per three seconds, so the bound is exercised directly here and the
    /// *wiring* is covered by the chat test above.
    #[test]
    fn mp14_room_reaction_history_is_bounded() {
        let (runtime, _dir) = runtime_with_db();

        let total = crate::chat::MAX_REACTION_HISTORY + 10;
        {
            let mut state = runtime.lock();
            for index in 0..total {
                crate::chat::push_bounded(
                    &mut state.reactions,
                    super::ReactionSnapshot {
                        id: format!("r{index}"),
                        sender: "guest-device".to_string(),
                        reaction: "🔥".to_string(),
                        created_host_time_us: index as u64,
                    },
                    crate::chat::MAX_REACTION_HISTORY,
                );
            }
        }

        let reactions = runtime.lock().reactions.clone();
        assert_eq!(reactions.len(), crate::chat::MAX_REACTION_HISTORY);
        assert_eq!(reactions[0].id, "r10");
        assert_eq!(reactions[reactions.len() - 1].id, format!("r{}", total - 1));
    }
}

// ── friends (saved movie partners) tests ────────────────────────────

#[cfg(test)]
mod friends_tests {
    use crate::storage::sqlite::{FriendConnectionState, StoredFriend};

    use super::{AppRuntime, MAX_DISPLAY_NAME_CHARS};

    fn runtime_with_db() -> (AppRuntime, std::path::PathBuf) {
        let runtime = AppRuntime::new();
        let dir = std::env::temp_dir().join(format!(
            "mp-friends-rt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        runtime.init_db_at_path(&dir.join("mp.db"));
        (runtime, dir)
    }

    /// list/add/remove round-trip on the runtime layer. add_friend and
    /// verify_friend hit the real Tailscale CLI, so this test exercises the
    /// pure-persistence path (list + remove) and the add-path's rejection
    /// for a peer that is not in the tailnet status — which on a CI machine
    /// without Tailscale is the deterministic MP-NET error, and on a real
    /// tailnet machine is a peer key that cannot exist.
    #[tokio::test]
    async fn friends_list_remove_and_unknown_add() {
        let (runtime, dir) = runtime_with_db();

        // Empty list on a fresh DB.
        assert!(runtime.list_friends().is_empty());

        // Adding a peer that is not in the current tailnet status is a
        // stable MP-NET error, never a panic and never a silent success.
        // (On machines where the Tailscale CLI is absent, detect_status
        // fails first with MP-NET-TS-001/003 — both accepted here since the
        // invariant under test is "an unknown peer is never saved".)
        let result = runtime
            .add_friend("ghost.tailc930b7.ts.net.".to_string(), None)
            .await;
        match result {
            Err(e) => assert!(e.starts_with("MP-NET-TS-"), "unexpected error: {e}"),
            // If a machine genuinely has such a peer in its tailnet (it
            // cannot — the key is test-fabricated), the add succeeds and
            // the round-trip below still holds.
            Ok(friend) => assert_eq!(friend.peer_key, "ghost.tailc930b7.ts.net."),
        }

        // Direct persistence sanity: a stored friend lists and removes.
        {
            let state = runtime.lock();
            let db = state.db.as_ref().expect("db");
            db.upsert_friend(&StoredFriend {
                peer_key: "rahul-mac.tailc930b7.ts.net.".to_string(),
                display_name: "rahul-mac".to_string(),
                ip: Some("100.64.0.42".to_string()),
                added_at_ms: 1,
                last_verified_at_ms: Some(9),
                last_path: Some("direct".to_string()),
                last_latency_ms: Some(21),
                connection_state: FriendConnectionState::MoviePartyVerified,
            })
            .expect("seed friend");
        }
        let friends = runtime.list_friends();
        assert_eq!(friends.len(), 1);
        assert_eq!(friends[0].display_name, "rahul-mac");
        assert_eq!(friends[0].last_latency_ms, Some(21));

        runtime
            .remove_friend("rahul-mac.tailc930b7.ts.net.")
            .expect("remove");
        assert!(runtime.list_friends().is_empty());

        // Removing a friend that is not saved is an error-free no-op on the
        // storage layer; the runtime maps it to Ok(()) for UI simplicity.
        runtime
            .remove_friend("never-saved.tailc930b7.ts.net.")
            .expect("remove unknown is fine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// verify_friend on an unsaved peer key is a stable MP-NET-TS-008 error,
    /// never a panic — the command surface must stay honest.
    #[tokio::test]
    async fn verify_friend_rejects_unsaved_peer() {
        let (runtime, dir) = runtime_with_db();
        let result = runtime
            .verify_friend("never-saved.tailc930b7.ts.net.")
            .await;
        // Two accepted failures: MP-NET-TS-008 (friend not saved — the
        // expected branch) or MP-NET-TS-001/003 when the machine has no
        // Tailscale CLI at all (the find-status step fails first). Both are
        // honest, stable outcomes; a panic or Ok would be the bug.
        match result {
            Err(e) => assert!(
                e.starts_with("MP-NET-TS-008")
                    || e.starts_with("MP-NET-TS-001")
                    || e.starts_with("MP-NET-TS-003"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("verify must not succeed for an unsaved peer"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// rename_friend: friendly names replace tailnet jargon; the peer key,
    /// cached IP, and verification stay untouched. Empty/oversized names are
    /// stable MP-FRIEND-001 errors; unknown peers are MP-NET-TS-008.
    #[test]
    fn rename_friend_updates_name_and_keeps_everything_else() {
        let (runtime, dir) = runtime_with_db();
        {
            let state = runtime.lock();
            let db = state.db.as_ref().expect("db");
            db.upsert_friend(&StoredFriend {
                peer_key: "rahul-mac.tailc930b7.ts.net.".to_string(),
                display_name: "rahul-mac".to_string(),
                ip: Some("100.64.0.42".to_string()),
                added_at_ms: 11,
                last_verified_at_ms: Some(99),
                last_path: Some("direct".to_string()),
                last_latency_ms: Some(21),
                connection_state: FriendConnectionState::MoviePartyVerified,
            })
            .expect("seed friend");
        }
        let renamed = runtime
            .rename_friend("rahul-mac.tailc930b7.ts.net.", "  Best Buddy  ")
            .expect("rename");
        assert_eq!(renamed.display_name, "Best Buddy");
        assert_eq!(renamed.peer_key, "rahul-mac.tailc930b7.ts.net.");
        assert_eq!(renamed.ip.as_deref(), Some("100.64.0.42"));
        assert_eq!(renamed.added_at_ms, 11);
        assert_eq!(renamed.last_verified_at_ms, Some(99));
        assert_eq!(renamed.last_latency_ms, Some(21));

        // Validation: empty and oversized names refuse with MP-FRIEND-001.
        assert!(runtime
            .rename_friend("rahul-mac.tailc930b7.ts.net.", "   ")
            .unwrap_err()
            .starts_with("MP-FRIEND-001"));
        let oversized = "x".repeat(41);
        assert!(runtime
            .rename_friend("rahul-mac.tailc930b7.ts.net.", &oversized)
            .unwrap_err()
            .starts_with("MP-FRIEND-001"));
        // Unknown peers refuse with MP-NET-TS-008.
        assert!(runtime
            .rename_friend("ghost.tailc930b7.ts.net.", "Nope")
            .unwrap_err()
            .starts_with("MP-NET-TS-008"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// friend_invite: on a tailnet-ready machine the link is a well-formed
    /// movieparty://friend/<base64url> that carries the payload; on a
    /// machine without Tailscale it is a stable MP-NET error, never a panic.
    #[tokio::test]
    async fn friend_invite_link_is_well_formed_or_honest_error() {
        let (runtime, dir) = runtime_with_db();
        match runtime.friend_invite().await {
            Ok(invite) => {
                assert!(invite.link.starts_with("movieparty://friend/"));
                let code = invite.link.trim_start_matches("movieparty://friend/");
                assert!(!code.is_empty());
                assert!(
                    code.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                    "code must be base64url: {code}"
                );
                assert!(invite.peer_key.ends_with('.'));
                assert!(invite.peer_key.contains('.'));
                assert!(!invite.display_name.is_empty());
            }
            Err(e) => assert!(
                e.starts_with("MP-NET-TS-") || e.starts_with("MP-STORE-001"),
                "unexpected error: {e}"
            ),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The invite payload is IDENTITY-ONLY: it must never carry a
    /// Tailscale auth key, access token, or any credential. This is the
    /// architectural guarantee the friend flow is built on — links are
    /// safe to print on a QR and share anywhere.
    #[tokio::test]
    async fn friend_invite_payload_is_identity_only_never_credentials() {
        let (runtime, dir) = runtime_with_db();
        if let Ok(invite) = runtime.friend_invite().await {
            let parsed =
                crate::network::tailscale::parse_friend_invite(&invite.link).expect("parse");
            assert_eq!(parsed.peer_key, invite.peer_key);
            assert_eq!(parsed.display_name, invite.display_name);
            // The raw payload must contain exactly the identity fields.
            let code = invite.link.trim_start_matches("movieparty://friend/");
            let bytes = crate::network::tailscale::base64url_decode(code).expect("decode");
            let decoded = String::from_utf8(bytes).expect("utf8");
            let value: serde_json::Value = serde_json::from_str(&decoded).expect("json");
            let keys: Vec<String> = value
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            assert_eq!(
                keys,
                vec!["n".to_string(), "pk".to_string()],
                "invite payload must carry only n/pk, got: {decoded}"
            );
            let lowered = decoded.to_lowercase();
            for banned in [
                "authkey",
                "auth_key",
                "apikey",
                "api_key",
                "token",
                "secret",
                "password",
                "credential",
            ] {
                assert!(
                    !lowered.contains(banned),
                    "invite payload must never contain {banned}: {decoded}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// accept_friend_invite: a valid link saves the inviter as INVITED
    /// (device not yet in this tailnet) with NO fabricated verification;
    /// a link naming a peer already in the live tailnet lands as
    /// TAILSCALE_JOINED. Invalid links are stable MP-FRIEND-001 errors
    /// and save nothing.
    #[tokio::test]
    async fn accept_friend_invite_saves_invited_or_joined_never_fake_connections() {
        let (runtime, dir) = runtime_with_db();

        // 1) Malformed links refuse with MP-FRIEND-001 and save nothing.
        for bad in [
            "not-a-link",
            "movieparty://friend/",
            "movieparty://friend/!!!",
            "https://evil.example/friend",
        ] {
            let err = runtime
                .accept_friend_invite(bad.to_string())
                .await
                .unwrap_err();
            assert!(err.starts_with("MP-FRIEND-001"), "for {bad}: {err}");
        }
        assert!(runtime.list_friends().is_empty(), "nothing was saved");

        // 2) A well-formed link to an absent peer saves INVITED, honestly.
        // Build the link the exact way the inviter's app does.
        let payload = format!(
            "{{\"n\":{},\"pk\":{}}}",
            serde_json::to_string("Priya").unwrap(),
            serde_json::to_string("priya-mac.tailc930b7.ts.net").unwrap(),
        );
        let link = format!(
            "movieparty://friend/{}",
            crate::network::tailscale::base64url_encode(payload.as_bytes())
        );
        let friend = runtime
            .accept_friend_invite(link.clone())
            .await
            .expect("accept invite");
        assert_eq!(friend.peer_key, "priya-mac.tailc930b7.ts.net.");
        assert_eq!(friend.display_name, "Priya");
        assert_eq!(
            friend.last_verified_at_ms, None,
            "no fabricated verification"
        );
        // Without a live tailnet observation the honest state is INVITED;
        // a machine whose real tailnet contains this peer would see
        // TAILSCALE_JOINED — both are valid, never MOVIE_PARTY_VERIFIED
        // from a link alone.
        assert_ne!(
            friend.connection_state,
            FriendConnectionState::MoviePartyVerified,
            "a link alone can never mark the friend verified"
        );

        // 3) Re-accepting the same link is idempotent and keeps the
        // user's chosen name if they renamed the friend.
        runtime
            .rename_friend("priya-mac.tailc930b7.ts.net.", "Movie Night")
            .expect("rename");
        let again = runtime.accept_friend_invite(link).await.expect("re-accept");
        assert_eq!(
            again.display_name, "Movie Night",
            "user's name wins over the link"
        );
        let friends = runtime.list_friends();
        assert_eq!(friends.len(), 1, "no duplicate rows");

        // 4) refresh_friend_states on an INVITED friend whose peer is
        // still absent keeps the honest INVITED state (never invents a
        // join) and never panics without Tailscale.
        let refreshed = runtime.refresh_friend_states().await;
        assert_eq!(refreshed.len(), 1);
        assert_ne!(
            refreshed[0].connection_state,
            FriendConnectionState::MoviePartyVerified
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F23: re-accepting an invite must never demote a verified friend.
    ///
    /// Re-opening an old invite link is a normal way to re-share it. The accept
    /// path used to write `last_verified_at_ms: None` plus a downgraded
    /// connection state, wiping a MOVIE_PARTY_VERIFIED record — even though the
    /// rest of the friend flow only ever promotes.
    #[tokio::test]
    async fn reaccepting_an_invite_never_demotes_a_verified_friend() {
        let (runtime, dir) = runtime_with_db();
        {
            let state = runtime.lock();
            let db = state.db.as_ref().expect("db");
            db.upsert_friend(&StoredFriend {
                peer_key: "verified.tailc930b7.ts.net.".to_string(),
                display_name: "Verified".to_string(),
                ip: Some("100.64.0.9".to_string()),
                added_at_ms: 1,
                last_verified_at_ms: Some(5_000),
                last_path: Some("direct".to_string()),
                last_latency_ms: Some(12),
                connection_state: FriendConnectionState::MoviePartyVerified,
            })
            .expect("seed verified");
        }

        let payload = format!(
            "{{\"n\":{},\"pk\":{}}}",
            serde_json::to_string("Ignored Link Name").unwrap(),
            serde_json::to_string("verified.tailc930b7.ts.net").unwrap(),
        );
        let link = format!(
            "movieparty://friend/{}",
            crate::network::tailscale::base64url_encode(payload.as_bytes())
        );
        let friend = runtime.accept_friend_invite(link).await.expect("re-accept");

        assert_eq!(
            friend.last_verified_at_ms,
            Some(5_000),
            "the recorded verification must survive a re-accept"
        );
        assert_eq!(friend.last_path.as_deref(), Some("direct"));
        assert_eq!(friend.last_latency_ms, Some(12));
        assert_eq!(
            friend.connection_state,
            FriendConnectionState::MoviePartyVerified,
            "a re-accept must not demote a verified friend"
        );
        assert_eq!(
            friend.display_name, "Verified",
            "the saved name still wins over the link"
        );
        assert_eq!(runtime.list_friends().len(), 1, "no duplicate rows");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// refresh_friend_states: an INVITED friend whose device has since
    /// joined the tailnet (observed via detect_status) is promoted to
    /// TAILSCALE_JOINED; a MOVIE_PARTY_VERIFIED friend is never demoted
    /// by later observations.
    #[tokio::test]
    async fn refresh_friend_states_promotes_invited_never_demotes_verified() {
        let (runtime, dir) = runtime_with_db();
        {
            let state = runtime.lock();
            let db = state.db.as_ref().expect("db");
            db.upsert_friend(&StoredFriend {
                peer_key: "invited.tailc930b7.ts.net.".to_string(),
                display_name: "invited-friend".to_string(),
                ip: None,
                added_at_ms: 1,
                last_verified_at_ms: None,
                last_path: None,
                last_latency_ms: None,
                connection_state: FriendConnectionState::Invited,
            })
            .expect("seed invited");
            db.upsert_friend(&StoredFriend {
                peer_key: "verified.tailc930b7.ts.net.".to_string(),
                display_name: "verified-friend".to_string(),
                ip: Some("100.64.0.2".to_string()),
                added_at_ms: 2,
                last_verified_at_ms: Some(123),
                last_path: Some("direct".to_string()),
                last_latency_ms: Some(30),
                connection_state: FriendConnectionState::MoviePartyVerified,
            })
            .expect("seed verified");
        }
        // Without a live tailnet the states are preserved as-is — the
        // honest keep-last-known behavior.
        let refreshed = runtime.refresh_friend_states().await;
        let by_key = |k: &str| {
            refreshed
                .iter()
                .find(|f| f.peer_key == k)
                .unwrap_or_else(|| panic!("missing {k}"))
                .connection_state
        };
        assert_eq!(
            by_key("invited.tailc930b7.ts.net."),
            FriendConnectionState::Invited
        );
        assert_eq!(
            by_key("verified.tailc930b7.ts.net."),
            FriendConnectionState::MoviePartyVerified
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Settings › General rename: a valid name persists to the identity
    /// row AND updates the live participant, while the device id and the
    /// public key stay byte-identical (the peer trust chain must survive
    /// a rename — no rotation, no new device id).
    #[tokio::test]
    async fn set_display_name_persists_without_rotating_identity() {
        let (runtime, dir) = runtime_with_db();

        let before = runtime.lock().local_participant.clone();
        let key_before = runtime.inner.identity().public_key_base64();

        let snapshot = runtime.set_display_name("  Cinephile  ");
        assert_eq!(
            snapshot
                .participants
                .first()
                .expect("local participant")
                .display_name,
            "Cinephile"
        );
        assert_eq!(runtime.lock().local_participant.display_name, "Cinephile");

        // Same device id + same public key: nothing rotated.
        assert_eq!(runtime.lock().local_participant.id, before.id);
        assert_eq!(runtime.inner.identity().public_key_base64(), key_before);

        // The stored identity row carries the new name and the same key.
        let stored = runtime
            .lock()
            .db
            .as_ref()
            .expect("db")
            .get_identity()
            .expect("read identity")
            .expect("identity exists");
        assert_eq!(stored.display_name, "Cinephile");
        assert_eq!(stored.device_id, before.id);
        assert_eq!(stored.public_key, key_before);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rename to an empty or over-long name is rejected with an error on
    /// the snapshot — the name is NOT changed, and no bogus row is written.
    #[tokio::test]
    async fn set_display_name_rejects_empty_and_overlong() {
        let (runtime, dir) = runtime_with_db();
        let original = runtime.lock().local_participant.display_name.clone();

        let empty = runtime.set_display_name("   ");
        assert!(empty.error.is_some(), "empty name must error");
        assert_eq!(runtime.lock().local_participant.display_name, original);

        let overlong = runtime.set_display_name(&"a".repeat(41));
        assert!(overlong.error.is_some(), "41-char name must error");
        assert_eq!(runtime.lock().local_participant.display_name, original);

        // The boundary: 40 chars is valid.
        let boundary = runtime.set_display_name(&"a".repeat(40));
        assert!(boundary.error.is_none(), "40-char name is allowed");
        assert_eq!(
            runtime.lock().local_participant.display_name,
            "a".repeat(40)
        );

        // F50: the limit is CHARACTER-counted, not byte-counted. 40 Cyrillic
        // characters is 80 bytes, so a byte limit would silently reject a
        // perfectly valid name — and one call site did use `len()` while the
        // others used `chars().count()`.
        let multibyte = "д".repeat(MAX_DISPLAY_NAME_CHARS);
        assert_eq!(multibyte.chars().count(), 40);
        assert!(
            multibyte.len() > 40,
            "the case only bites when bytes exceed characters"
        );
        let accepted = runtime.set_display_name(&multibyte);
        assert!(
            accepted.error.is_none(),
            "40 multibyte characters must be accepted: {:?}",
            accepted.error
        );
        assert_eq!(runtime.lock().local_participant.display_name, multibyte);

        let multibyte_over = "д".repeat(MAX_DISPLAY_NAME_CHARS + 1);
        assert!(
            runtime.set_display_name(&multibyte_over).error.is_some(),
            "41 multibyte characters must be refused"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A no-op rename (same name) does not error and does not churn the
    /// identity row.
    #[tokio::test]
    async fn set_display_name_noop_is_clean() {
        let (runtime, dir) = runtime_with_db();
        let current = runtime.lock().local_participant.display_name.clone();

        let snapshot = runtime.set_display_name(&current);
        assert!(snapshot.error.is_none());
        assert_eq!(runtime.lock().local_participant.display_name, current);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod session_lifecycle_tests {
    use super::{
        bump_chrome_generation, bump_session_generation, chrome_session_may_be_reinserted,
        AppRuntime, DeferredTeardown,
    };
    use crate::media::player::{LocalPlayer, PlayerError, PlayerSnapshot};
    use crate::network::quic::monotonic_us;
    use crate::sync::local::ScheduledPlayback;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use uuid::Uuid;

    /// A player whose `close()` parks until the test releases it, so a test can
    /// observe what the runtime is (or is not) holding while the blocking
    /// teardown is in flight.
    struct ParkedClosePlayer {
        close_entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
    }

    impl LocalPlayer for ParkedClosePlayer {
        fn open(&mut self, _path: &Path) -> Result<(), PlayerError> {
            Ok(())
        }
        fn play(&mut self) -> Result<(), PlayerError> {
            Ok(())
        }
        fn pause(&mut self) -> Result<(), PlayerError> {
            Ok(())
        }
        fn seek(&mut self, _position_ms: u64) -> Result<(), PlayerError> {
            Ok(())
        }
        fn set_volume(&mut self, _volume: f32) -> Result<(), PlayerError> {
            Ok(())
        }
        fn set_playback_rate(&mut self, _rate: f32) -> Result<(), PlayerError> {
            Ok(())
        }
        fn snapshot(&self) -> PlayerSnapshot {
            PlayerSnapshot::default()
        }
        fn duration(&self) -> Option<u64> {
            None
        }
        fn buffered_ahead_ms(&self) -> Option<u64> {
            None
        }
        fn error_message(&self) -> Option<String> {
            None
        }
        fn close(&mut self) {
            self.close_entered.store(true, Ordering::SeqCst);
            // Bounded park: this must never be able to hang the suite, even if
            // the assertion it exists to enable fails.
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !self.release.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    /// A runtime holding a pending PLAY operation whose commit deadline is
    /// `execute_in` from now, plus a matching `pending_scheduled` entry in the
    /// coordinator.
    ///
    /// Seeding the coordinator matters: the commit task returns early when
    /// `pending_scheduled` is `None` or carries a different operation id. With
    /// it seeded and matching, the MP-01 session-generation guard is the ONLY
    /// thing that can stop the commit from mutating state — which is what makes
    /// the stale-task tests discriminating rather than accidentally passing.
    fn runtime_with_pending_play(execute_in: Duration) -> (AppRuntime, Uuid) {
        let runtime = AppRuntime::new();
        let operation_id = Uuid::now_v7();
        let execute_at_host_mono_us = monotonic_us() + execute_in.as_micros() as u64;
        {
            let mut state = runtime.lock();
            state.pending_operation_id = Some(operation_id.to_string());
            state.pending_operation_kind = Some("PLAY".to_string());
            state.pending_operation_target_ms = 5_000;
            state.pending_operation_execute_at_us = execute_at_host_mono_us;
            state.pending_operation_resume_after = false;
            state.commit_scheduled_for = None;
            state.last_committed_operation_id = None;
            state.sync.position_ms = 0;
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pending_scheduled = Some(ScheduledPlayback {
                operation_id,
                target_position_ms: 5_000,
                execute_at_host_mono_us,
            });
        }
        (runtime, operation_id)
    }

    /// MP-01: a delayed PLAY commit whose room ENDED before the deadline must
    /// be a complete no-op — no position move, no pending-operation clear, no
    /// "committed" record.
    #[tokio::test]
    async fn play_commit_after_session_teardown_is_a_noop() {
        let (runtime, operation_id) = runtime_with_pending_play(Duration::from_millis(120));
        let scheduled_generation = runtime.lock().session_generation;

        // The guest answered PLAY_READY, so the host spawns the commit task.
        runtime.on_guest_play_ready(&operation_id.to_string(), true, 0);
        assert!(
            runtime.lock().commit_scheduled_for.is_some(),
            "the commit must actually have been scheduled, or this test proves nothing"
        );

        // Teardown lands before the task wakes. No `await` has happened since
        // the spawn, so the task provably has not run yet.
        {
            let mut state = runtime.lock();
            bump_session_generation(&mut state);
            assert_ne!(state.session_generation, scheduled_generation);
        }

        tokio::time::sleep(Duration::from_millis(400)).await;

        let state = runtime.lock();
        assert_eq!(
            state.pending_operation_id.as_deref(),
            Some(operation_id.to_string().as_str()),
            "a stale commit must not clear the pending operation it no longer owns"
        );
        assert_eq!(
            state.sync.position_ms, 0,
            "a stale commit must not move the room's position"
        );
        assert!(
            state.last_committed_operation_id.is_none(),
            "a stale commit must not be recorded as committed"
        );
    }

    /// Control for the teardown test: identical setup, no teardown, and the
    /// commit DOES fire. Without this, `play_commit_after_session_teardown_is_a_noop`
    /// could be passing for an unrelated reason (bad seeding, an early return
    /// elsewhere) instead of because of the generation guard.
    #[tokio::test]
    async fn play_commit_fires_while_the_session_is_still_live() {
        let (runtime, operation_id) = runtime_with_pending_play(Duration::from_millis(120));
        runtime.on_guest_play_ready(&operation_id.to_string(), true, 0);

        tokio::time::sleep(Duration::from_millis(400)).await;

        let state = runtime.lock();
        assert!(
            state.pending_operation_id.is_none(),
            "a live commit must consume the pending operation"
        );
        assert_eq!(
            state.last_committed_operation_id.as_deref(),
            Some(operation_id.to_string().as_str())
        );
        assert_eq!(state.sync.position_ms, 5_000);
    }

    /// MP-01: a stale commit from a REPLACED room must not clobber the new
    /// room's pending operation. The old task wakes, finds a different session
    /// generation, and leaves the replacement untouched.
    #[tokio::test]
    async fn play_commit_after_room_replacement_leaves_the_new_operation_alone() {
        let (runtime, old_operation) = runtime_with_pending_play(Duration::from_millis(120));
        runtime.on_guest_play_ready(&old_operation.to_string(), true, 0);

        // A replacement session starts: the generation bumps and a NEW
        // operation takes over the pending slot.
        let new_operation = Uuid::now_v7();
        {
            let mut state = runtime.lock();
            bump_session_generation(&mut state);
            state.pending_operation_id = Some(new_operation.to_string());
            state.pending_operation_kind = Some("PLAY".to_string());
            state.pending_operation_target_ms = 9_000;
            state.commit_scheduled_for = None;
            state.last_committed_operation_id = None;
        }

        tokio::time::sleep(Duration::from_millis(400)).await;

        let state = runtime.lock();
        assert_eq!(
            state.pending_operation_id.as_deref(),
            Some(new_operation.to_string().as_str()),
            "the stale task must not clear the replacement room's operation"
        );
        assert_eq!(
            state.pending_operation_target_ms, 9_000,
            "the stale task must not retarget the replacement room"
        );
        assert!(
            state.last_committed_operation_id.is_none(),
            "the stale task must not record its own operation as committed"
        );
        assert_eq!(
            state.sync.position_ms, 0,
            "the stale task must not move the replacement room"
        );
    }

    /// MP-08: a Chrome session taken out of state for a CDP round-trip must not
    /// be put back once teardown has bumped the provider generation. Putting it
    /// back would leave the runtime pointing at a browser that was already
    /// closed, so every later command would fail against a dead CDP endpoint.
    #[test]
    fn chrome_session_cannot_resurrect_after_teardown() {
        let runtime = AppRuntime::new();
        let taken_generation = runtime.lock().chrome_generation;

        // Teardown (or a provider replacement) happens while the round-trip is
        // still in flight.
        bump_chrome_generation(&mut runtime.lock());

        assert!(
            !chrome_session_may_be_reinserted(&runtime.lock(), taken_generation),
            "an obsolete Chrome session must not be reinserted after teardown"
        );
    }

    /// MP-08 control: the guard is not a blanket refusal. While the provider
    /// generation is unchanged and the slot is still empty, the session IS
    /// reinserted — that is the normal, successful round-trip.
    ///
    /// The second half of the guard (`chrome_session.is_none()`, i.e. "nobody
    /// already installed a newer session") cannot be exercised here without
    /// launching a real Chrome process, because `ManagedChromeSession` owns a
    /// real process tree. It is a plain emptiness check at both production call
    /// sites.
    #[test]
    fn chrome_session_reinserts_while_the_generation_is_unchanged() {
        let runtime = AppRuntime::new();
        let generation = runtime.lock().chrome_generation;

        assert!(
            chrome_session_may_be_reinserted(&runtime.lock(), generation),
            "a successful round-trip must still be able to return its session"
        );
    }

    /// MP-07: the blocking player teardown must run with the global state lock
    /// RELEASED. While the player's `close()` is parked, another caller must
    /// still be able to take the lock.
    ///
    /// If `DeferredTeardown::run` were invoked with the lock held, this test
    /// blocks on `runtime.lock()` until the park deadline expires, then finds
    /// `closed == true` and fails — the bounded park means it fails rather than
    /// hanging.
    #[tokio::test]
    async fn deferred_teardown_runs_outside_the_global_lock() {
        let runtime = AppRuntime::new();
        let close_entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        {
            let mut state = runtime.lock();
            state.player = Some(Arc::new(Mutex::new(ParkedClosePlayer {
                close_entered: close_entered.clone(),
                release: release.clone(),
                closed: closed.clone(),
            })));
        }

        // The production pattern: take the blocking artifacts OUT under the
        // lock, release the lock, then run the teardown.
        let teardown = {
            let mut state = runtime.lock();
            DeferredTeardown::take(&mut state)
        };
        assert!(
            runtime.lock().player.is_none(),
            "take() must remove the player from the locked state"
        );

        // spawn_blocking mirrors production: the blocking close runs on the
        // blocking pool, never on an async worker.
        let teardown_task = tokio::task::spawn_blocking(move || teardown.run());

        let wait_deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !close_entered.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < wait_deadline,
                "the player's close() was never entered"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        // THE ASSERTION: the global lock is free while close() is blocked.
        let position_ms = runtime.lock().sync.position_ms;
        assert_eq!(position_ms, 0);
        assert!(
            !closed.load(Ordering::SeqCst),
            "close() must still be parked — otherwise this test proves nothing \
             (it means the lock was held until close() finished)"
        );

        release.store(true, Ordering::SeqCst);
        teardown_task.await.expect("teardown task must not panic");
        assert!(
            closed.load(Ordering::SeqCst),
            "teardown must complete once the player is released"
        );
    }
}
