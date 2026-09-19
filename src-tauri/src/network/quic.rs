use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use quinn::{
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
    ClientConfig, Connection, Endpoint, ServerConfig,
};
use rand::RngCore;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::identity::{verify_auth_request_signature, DeviceIdentity};
use crate::protocol::{
    SequenceTracker, ENVELOPE_V_MAJOR, ENVELOPE_V_MINOR, MAX_CONTROL_MESSAGE_BYTES,
};
use crate::sync::clock::ClockSample;

pub const QUIC_TRANSPORT_NAME: &str = "quic";
pub const MOVIE_PARTY_ALPN: &[&[u8]] = &[b"movieparty-v1"];
/// PROTOCOL_SPEC §5: any control message (request, response, or event
/// envelope) larger than 256 KiB must be rejected with MP-PROTO-004. This
/// limit is shared by both directions of the framed JSON control channel.
const MAX_CONTROL_FRAME_BYTES: usize = MAX_CONTROL_MESSAGE_BYTES;
/// Hard cap for binary-adjacent reads that are NOT control messages — the
/// chunk-stream fallback path that may carry an oversized JSON error after
/// the MPCK magic check, and similar raw-stream diagnostics. Movie chunks
/// travel on dedicated QUIC streams and are exempt from the control limit
/// (§5), so they only need a DoS-bounding frame cap.
const MAX_RAW_STREAM_BYTES: usize = 2 * 1024 * 1024;
const MAX_AUTH_NONCES: usize = 4_096;

/// Wraps a [`ServerEvent`] with metadata identifying the originating peer and
/// the monotonic time at which the event was scheduled by the sender.
///
/// `EventEnvelope` is the unit that the Guest receives from the Host over
/// server-opened bidirectional QUIC streams. The `event` payload by itself
/// remains the protocol message; the envelope metadata is used by the runtime
/// for sequence enforcement (duplicate/stale rejection) and for converting the
/// host's monotonic schedule time into the guest's monotonic timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// PROTOCOL_SPEC §10 envelope version. Same major required; a newer
    /// minor from the peer is tolerated (§6) because every added field has
    /// so far been ignorable.
    pub v_major: u16,
    pub v_minor: u16,
    /// Room the event belongs to (§10). Empty is only legal pre-auth.
    pub room_id: String,
    pub seq: u64,
    pub sender: String,
    pub sent_mono_us: u64,
    pub event: ServerEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomCredentials {
    pub room_id: String,
    pub join_secret: String,
}

impl RoomCredentials {
    pub fn generate() -> Self {
        let mut room_id = [0_u8; 16];
        let mut join_secret = [0_u8; 32];
        rand::rng().fill_bytes(&mut room_id);
        rand::rng().fill_bytes(&mut join_secret);

        Self {
            room_id: URL_SAFE_NO_PAD.encode(room_id),
            join_secret: URL_SAFE_NO_PAD.encode(join_secret),
        }
    }

    pub fn new_for_tests() -> Self {
        Self {
            room_id: "EjRWeJCrze8BI0VniavN7w".to_string(),
            join_secret: "test-secret".to_string(),
        }
    }

    fn join_secret_hash(&self) -> String {
        hash_secret(&self.join_secret)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloPayload {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub arch: String,
    pub app_version: String,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub public_key: String,
}

impl HelloPayload {
    pub fn local(display_name: impl Into<String>, identity: &DeviceIdentity) -> Self {
        Self {
            device_id: identity.device_id.clone(),
            display_name: display_name.into(),
            platform: current_platform().to_string(),
            arch: std::env::consts::ARCH.to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_major: crate::PROTOCOL_MAJOR,
            protocol_minor: crate::PROTOCOL_MINOR,
            public_key: identity.public_key_base64(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthRequest {
    pub room_id: String,
    pub join_secret_hash: String,
    pub invite_nonce: String,
    pub device_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthAccept {
    pub session_id: String,
    pub room_role: String,
    pub host_display_name: String,
    pub host_device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ClientRequest {
    HelloAuth {
        hello: Box<HelloPayload>,
        auth: Box<AuthRequest>,
    },
    Heartbeat {
        seq: u64,
        sender: String,
        sent_mono_us: u64,
        room_state: String,
        last_seen_peer_seq: u64,
    },
    Ping {
        seq: u64,
        sender: String,
        sent_mono_us: u64,
        probe_id: u64,
        t0_us: u64,
    },
    ThroughputUpload {
        seq: u64,
        sender: String,
        sent_mono_us: u64,
        bytes: u64,
    },
    ReadyState {
        seq: u64,
        sender: String,
        ready: bool,
        buffer_ahead_ms: u64,
    },
    BufferStatus {
        seq: u64,
        sender: String,
        position_ms: u64,
        buffer_ahead_ms: u64,
        stalled: bool,
    },
    PlayReady {
        seq: u64,
        sender: String,
        operation_id: String,
        ready: bool,
        position_ms: u64,
        buffer_ahead_ms: u64,
    },
    PauseReady {
        seq: u64,
        sender: String,
        operation_id: String,
        ready: bool,
    },
    SeekReady {
        seq: u64,
        sender: String,
        operation_id: String,
        ready: bool,
        buffer_ahead_ms: u64,
    },
    ClockResult {
        seq: u64,
        sender: String,
        offset_to_host_us: i64,
        rtt_us: u64,
        sample_count: u64,
        quality: String,
    },
    ChatMessage {
        seq: u64,
        sender: String,
        message_id: String,
        body: String,
        created_host_time_us: u64,
    },
    Reaction {
        seq: u64,
        sender: String,
        reaction_id: String,
        reaction: String,
    },
    ControlRequest {
        seq: u64,
        sender: String,
        request_id: String,
        action: String,
        parameters: serde_json::Value,
    },
    CallSignal {
        seq: u64,
        sender: String,
        signal_type: String,
        data: String,
    },
    /// M3: Guest requests the media manifest for Local Perfect transfer.
    ManifestRequest { seq: u64, sender: String },
    /// M3: Guest requests a specific chunk by index.
    ChunkRequest {
        seq: u64,
        sender: String,
        media_id: String,
        chunk_index: u64,
    },
    /// (§56): guest accepts a host-created schedule. The host
    /// relays the acknowledgement back as a ScheduleAccept broadcast.
    ScheduleAccept {
        seq: u64,
        sender: String,
        schedule_id: String,
        accepted: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ServerResponse {
    AuthAccept(AuthAccept),
    AuthReject {
        code: String,
    },
    HeartbeatAck {
        room_state: String,
    },
    Pong {
        probe_id: u64,
        t0_us: u64,
        host_receive_us: u64,
        host_send_us: u64,
    },
    ThroughputResult {
        bytes: u64,
        elapsed_us: u128,
        goodput_bps: u64,
    },
    ReadyAck {
        ready: bool,
    },
    BufferAck {
        accepted: bool,
    },
    PlayReadyAck {
        operation_id: String,
        accepted: bool,
    },
    PauseReadyAck {
        operation_id: String,
        accepted: bool,
    },
    SeekReadyAck {
        operation_id: String,
        accepted: bool,
    },
    ClockResultAck {
        accepted: bool,
    },
    ChatAccepted {
        message_id: String,
    },
    ReactionAccepted {
        reaction_id: String,
    },
    ControlResponse {
        request_id: String,
        granted: bool,
        reason: Option<String>,
    },
    /// (§56): the host acknowledged the guest's schedule acceptance.
    ScheduleAcceptAck {
        schedule_id: String,
        accepted: bool,
    },
    /// M3: Host sends the media manifest to the guest.
    ManifestResponse {
        manifest: crate::media::manifest::MediaManifest,
    },
    /// M3: Host indicates the requested chunk is not available.
    ChunkUnavailable {
        media_id: String,
        chunk_index: u64,
    },
    /// M3: Media transfer error.
    MediaError {
        code: String,
        message: String,
        operation_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
/// `Eq` is deliberately NOT derived: `PreloadState.progress` is an `f64`
/// (§57), and no code compares whole ServerEvents for identity — ordering
/// is the envelope seq, not the payload.
pub enum ServerEvent {
    PlayPrepare {
        operation_id: String,
        target_position_ms: u64,
        minimum_buffer_ms: u64,
    },
    PlayCommit {
        operation_id: String,
        target_position_ms: u64,
        execute_at_host_mono_us: u64,
        presentation_epoch: u64,
    },
    PausePrepare {
        operation_id: String,
        reason: String,
        target_position_ms: u64,
    },
    PauseCommit {
        operation_id: String,
        target_position_ms: u64,
        execute_at_host_mono_us: u64,
    },
    SeekPrepare {
        operation_id: String,
        target_position_ms: u64,
        initiator: String,
    },
    SeekCommit {
        operation_id: String,
        target_position_ms: u64,
        execute_at_host_mono_us: u64,
        resume_after_seek: bool,
    },
    BufferLow {
        position_ms: u64,
        buffer_ahead_ms: u64,
    },
    BufferRecovered {
        buffer_ahead_ms: u64,
    },
    RoomStateUpdate {
        state: String,
        position_ms: u64,
    },
    CoordinatorStateUpdate {
        host_ready: bool,
        guest_ready: bool,
        coordinator_play_state: String,
        buffer_ahead_ms: u64,
    },
    ChatMessage {
        message_id: String,
        sender: String,
        body: String,
        created_host_time_us: u64,
    },
    Reaction {
        reaction_id: String,
        sender: String,
        reaction: String,
    },
    ControlGrant {
        request_id: String,
        action: String,
    },
    ControlDeny {
        request_id: String,
        reason: String,
    },
    SyncError {
        code: String,
        message: String,
        operation_id: Option<String>,
    },
    CallSignal {
        signal_type: String,
        data: String,
    },
    /// (PROTOCOL_SPEC §55): host broadcasts a created schedule.
    /// Scheduling uses wall-clock UTC (PROTOCOL_SPEC §55; wall clock is legal
    /// for scheduled movie time — playback sync stays monotonic, §16).
    ScheduleCreate {
        schedule_id: String,
        scheduled_start_utc_ms: i64,
        media_id: String,
        call_mode: String,
        planned_preload_utc_ms: i64,
    },
    /// §55: guest acknowledgement of a schedule. The host relays acceptance
    /// to the scheduling host so persistence + reminder registration can be
    /// confirmed on both sides.
    ScheduleAccept {
        schedule_id: String,
        accepted: bool,
    },
    /// Host-side schedule mutation (media/preload moved) broadcast to the
    /// guest so its persisted copy stays truthful.
    ScheduleUpdate {
        schedule_id: String,
        media_id: String,
        planned_preload_utc_ms: i64,
        scheduled_start_utc_ms: i64,
    },
    /// §56: a cancelled schedule never fires notifications on either side.
    ScheduleCancel {
        schedule_id: String,
    },
    /// §57: host preload progress for a schedule the guest accepted.
    PreloadState {
        schedule_id: String,
        state: String,
        progress: f64,
        estimated_ready_utc_ms: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RttResult {
    pub rtt_us: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThroughputResult {
    pub bytes: u64,
    pub elapsed_us: u128,
    pub goodput_bps: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum QuicError {
    #[error("MP-NET-001 QUIC endpoint error: {0}")]
    Endpoint(#[from] std::io::Error),
    #[error("MP-NET-001 QUIC connection failed: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("MP-NET-001 QUIC connect failed: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("MP-NET-001 QUIC stream write failed: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("MP-NET-001 QUIC stream closed before write completed")]
    ClosedStream,
    #[error("MP-NET-001 QUIC stream read failed: {0}")]
    ReadExact(String),
    #[error("MP-NET-001 QUIC stream read failed: {0}")]
    Read(#[from] quinn::ReadToEndError),
    #[error("MP-NET-001 QUIC request serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("MP-NET-001 QUIC TLS configuration failed: {0}")]
    Tls(String),
    #[error("MP-NET-001 QUIC authentication failed: {0}")]
    Auth(String),
    #[error("MP-NET-001 QUIC bind address must be loopback or Tailscale IPv4")]
    InvalidBindAddress,
    #[error("MP-NET-001 QUIC response mismatch")]
    UnexpectedResponse,
    #[error("MP-PROTO-004 control message exceeds 256 KiB limit ({len} bytes)")]
    MessageTooLarge { len: usize },
    #[error("MP-PROTO-001 envelope protocol version mismatch: {detail}")]
    EnvelopeVersion { detail: String },
    #[error("MP-PROTO-003 envelope room mismatch: {detail}")]
    EnvelopeRoom { detail: String },
}

#[derive(Debug, Clone)]
pub enum QuicHostEvent {
    PeerAuthenticated {
        device_id: String,
        display_name: String,
    },
    PeerDisconnected {
        device_id: String,
    },
    GuestReadyState {
        broadcaster_device_id: String,
        coordinator_play_state: String,
        coordinator_ready: bool,
        coordinator_buffer_ahead_ms: u64,
    },
    GuestPlayReady {
        broadcaster_device_id: String,
        operation_id: String,
        ready: bool,
        position_ms: u64,
        buffer_ahead_ms: u64,
    },
    GuestPauseReady {
        broadcaster_device_id: String,
        operation_id: String,
        ready: bool,
    },
    GuestSeekReady {
        broadcaster_device_id: String,
        operation_id: String,
        ready: bool,
        buffer_ahead_ms: u64,
    },
    GuestControlRequest {
        broadcaster_device_id: String,
        request_id: String,
        action: String,
        parameters: serde_json::Value,
    },
    /// (§56): the guest accepted a broadcast schedule. The host
    /// relays acceptance into the room (the scheduling host confirms its
    /// own persistence; the guest registers local reminders).
    GuestScheduleAccept {
        broadcaster_device_id: String,
        schedule_id: String,
        accepted: bool,
    },
    ClockResultReceived {
        broadcaster_device_id: String,
        offset_to_host_us: i64,
        rtt_p95_us: u64,
        sample_count: u64,
        quality: String,
    },
}

pub struct QuicServer {
    endpoint: Endpoint,
    certificate: CertificateDer<'static>,
    credentials: RoomCredentials,
    replay_guard: AuthReplayGuard,
    pub host_display_name: String,
    pub host_device_id: String,
    pub event_callback: Option<std::sync::Arc<dyn Fn(QuicHostEvent) + Send + Sync>>,
    pub local_media: Option<String>,
    event_tx: Option<std::sync::Arc<broadcast::Sender<EventEnvelope>>>,
    next_event_seq: AtomicU64,
    shared_controls: Option<std::sync::Arc<AtomicBool>>,
}

impl QuicServer {
    pub fn bind(
        bind_addr: SocketAddr,
        credentials: RoomCredentials,
        host_display_name: String,
        host_device_id: String,
    ) -> Result<Self, QuicError> {
        validate_quic_bind_addr(bind_addr)?;
        let (server_config, certificate) = configure_server()?;
        let endpoint = Endpoint::server(server_config, bind_addr)?;

        Ok(Self {
            endpoint,
            certificate,
            credentials,
            replay_guard: AuthReplayGuard::default(),
            host_display_name,
            host_device_id,
            event_callback: None,
            local_media: None,
            event_tx: None,
            next_event_seq: AtomicU64::new(1),
            shared_controls: None,
        })
    }

    pub fn bind_with_local_media(
        bind_addr: SocketAddr,
        credentials: RoomCredentials,
        host_display_name: String,
        host_device_id: String,
        media_path: std::path::PathBuf,
    ) -> Result<Self, QuicError> {
        let mut server = Self::bind(bind_addr, credentials, host_display_name, host_device_id)?;
        server.local_media = Some(media_path.to_string_lossy().into_owned());
        Ok(server)
    }

    pub fn with_event_callback(
        mut self,
        callback: std::sync::Arc<dyn Fn(QuicHostEvent) + Send + Sync>,
    ) -> Self {
        self.event_callback = Some(callback);
        self
    }

    pub fn with_event_broadcast(
        mut self,
        tx: std::sync::Arc<broadcast::Sender<EventEnvelope>>,
    ) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Share the host's Shared-Controls flag so request routing can grant or
    /// deny guest control requests at the transport boundary.
    pub fn with_shared_controls(mut self, flag: std::sync::Arc<AtomicBool>) -> Self {
        self.shared_controls = Some(flag);
        self
    }

    /// Broadcast a [`ServerEvent`] to all subscribers. The event is wrapped in
    /// an [`EventEnvelope`] carrying a strictly-increasing sequence number
    /// (drawn from `next_event_seq`), the host's device id as `sender`, and the
    /// typed broadcast helpers for the scheduling wire events
    /// (§55–§57). Every one goes through `broadcast` so the envelope carries
    /// the host's canonical seq — a guest can never see a stale/duplicate
    /// ordering (§10).
    pub fn broadcast_schedule_created(
        &self,
        schedule_id: String,
        scheduled_start_utc_ms: i64,
        media_id: String,
        call_mode: String,
        planned_preload_utc_ms: i64,
    ) {
        self.broadcast(ServerEvent::ScheduleCreate {
            schedule_id,
            scheduled_start_utc_ms,
            media_id,
            call_mode,
            planned_preload_utc_ms,
        });
    }

    pub fn broadcast_schedule_updated(
        &self,
        schedule_id: String,
        media_id: String,
        planned_preload_utc_ms: i64,
        scheduled_start_utc_ms: i64,
    ) {
        self.broadcast(ServerEvent::ScheduleUpdate {
            schedule_id,
            media_id,
            planned_preload_utc_ms,
            scheduled_start_utc_ms,
        });
    }

    pub fn broadcast_schedule_cancelled(&self, schedule_id: String) {
        self.broadcast(ServerEvent::ScheduleCancel { schedule_id });
    }

    /// §57: preload progress. `progress` is clamped to 0.0..=1.0 on the wire
    /// so a malformed peer reading can never imply >100 %.
    pub fn broadcast_preload_state(
        &self,
        schedule_id: String,
        state: String,
        progress: f64,
        estimated_ready_utc_ms: i64,
    ) {
        self.broadcast(ServerEvent::PreloadState {
            schedule_id,
            state,
            progress: progress.clamp(0.0, 1.0),
            estimated_ready_utc_ms,
        });
    }

    /// current monotonic timestamp.
    ///
    /// Sequence numbers are assigned *here*, on the QuicServer, because there
    /// is a single canonical source per host: this guarantees that the
    /// dispatcher task and any AppRuntime-level callers share one ordering.
    pub fn broadcast(&self, event: ServerEvent) {
        if let Some(ref tx) = self.event_tx {
            let seq = self.next_event_seq.fetch_add(1, Ordering::SeqCst);
            let envelope = EventEnvelope {
                v_major: ENVELOPE_V_MAJOR,
                v_minor: ENVELOPE_V_MINOR,
                room_id: self.credentials.room_id.clone(),
                seq,
                sender: self.host_device_id.clone(),
                sent_mono_us: monotonic_us(),
                event,
            };
            let _ = tx.send(envelope);
        }
    }

    /// The host's canonical room id, for envelopes built outside
    /// [`QuicServer::broadcast`] (e.g. the coordinator callback in
    /// AppRuntime, which only holds the broadcast channel).
    pub fn room_id(&self) -> &str {
        &self.credentials.room_id
    }

    pub fn certificate(&self) -> &rustls::pki_types::CertificateDer<'static> {
        &self.certificate
    }

    pub fn local_addr(&self) -> Result<SocketAddr, QuicError> {
        Ok(self.endpoint.local_addr()?)
    }

    pub fn certificate_fingerprint(&self) -> String {
        certificate_fingerprint(&self.certificate)
    }

    pub async fn run(
        self,
        coordinator_handle: Option<Arc<Mutex<crate::sync::local::LocalSyncCoordinator>>>,
    ) -> Result<(), QuicError> {
        while let Some(incoming) = self.endpoint.accept().await {
            let credentials = self.credentials.clone();
            let replay_guard = self.replay_guard.clone();
            let host_display_name = self.host_display_name.clone();
            let host_device_id = self.host_device_id.clone();
            let event_callback = self.event_callback.clone();
            let local_media = self.local_media.clone();
            let event_tx = self.event_tx.clone();
            let coordinator = coordinator_handle.clone();
            let shared_controls = self.shared_controls.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        let _ = handle_connection(
                            connection,
                            credentials,
                            replay_guard,
                            host_display_name,
                            host_device_id,
                            event_callback,
                            local_media,
                            event_tx,
                            coordinator,
                            shared_controls,
                        )
                        .await;
                    }
                    Err(_error) => {}
                }
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
struct AuthReplayGuard {
    accepted_nonces: Arc<Mutex<HashSet<String>>>,
}

impl AuthReplayGuard {
    fn accept_once(&self, device_id: &str, invite_nonce: &str) -> bool {
        let mut accepted_nonces = match self.accepted_nonces.lock() {
            Ok(accepted_nonces) => accepted_nonces,
            Err(poisoned) => poisoned.into_inner(),
        };

        if accepted_nonces.len() >= MAX_AUTH_NONCES {
            accepted_nonces.clear();
        }

        accepted_nonces.insert(format!("{device_id}:{invite_nonce}"))
    }
}

#[derive(Clone, Debug)]
pub struct QuicClient {
    endpoint: Endpoint,
    connection: Connection,
    credentials: RoomCredentials,
    identity: DeviceIdentity,
    next_seq: Arc<AtomicU64>,
}

impl QuicClient {
    pub async fn connect(
        server_addr: SocketAddr,
        server_certificate_fingerprint: String,
        credentials: RoomCredentials,
        identity: crate::identity::DeviceIdentity,
        display_name: String,
    ) -> Result<(Self, AuthAccept), QuicError> {
        let endpoint = make_client_endpoint(server_certificate_fingerprint)?;
        let connection = endpoint.connect(server_addr, "localhost")?.await?;
        let hello = HelloPayload::local(display_name, &identity);
        let client = Self {
            endpoint,
            connection,
            credentials,
            identity,
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(client.auth_request()),
            })
            .await?;

        match response {
            ServerResponse::AuthAccept(accept) => Ok((client, accept)),
            ServerResponse::AuthReject { code } => Err(QuicError::Auth(code)),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    pub async fn heartbeat(&self) -> Result<(), QuicError> {
        self.heartbeat_room_state().await.map(|_| ())
    }

    /// Return the host's current canonical room state. This is used after an
    /// authenticated transport replacement; it does not authorize a guest to
    /// resume playback on its own.
    pub async fn heartbeat_room_state(&self) -> Result<String, QuicError> {
        let response = self
            .send_request(ClientRequest::Heartbeat {
                seq: self.next_seq(),
                sender: self.identity.device_id.clone(),
                sent_mono_us: monotonic_us(),
                room_state: "LOBBY".to_string(),
                last_seen_peer_seq: 0,
            })
            .await?;

        match response {
            ServerResponse::HeartbeatAck { room_state } => Ok(room_state),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    pub async fn rtt_probe(&self) -> Result<RttResult, QuicError> {
        let start = Instant::now();
        let response = self
            .send_request(ClientRequest::Ping {
                seq: self.next_seq(),
                sender: self.identity.device_id.clone(),
                sent_mono_us: monotonic_us(),
                probe_id: 1,
                t0_us: 0,
            })
            .await?;

        match response {
            ServerResponse::Pong { .. } => Ok(RttResult {
                rtt_us: start.elapsed().as_micros(),
            }),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    /// One CLOCK_PING probe. Returns the full Pong so the guest can compute a
    /// proper `ClockSample` (t0 guest / host receive / host send / t3 guest).
    pub async fn clock_probe(&self, probe_id: u64, t0_us: u64) -> Result<ClockSample, QuicError> {
        let response = self
            .send_request(ClientRequest::Ping {
                seq: self.next_seq(),
                sender: self.identity.device_id.clone(),
                sent_mono_us: monotonic_us(),
                probe_id,
                t0_us,
            })
            .await?;

        match response {
            ServerResponse::Pong {
                host_receive_us,
                host_send_us,
                ..
            } => Ok(ClockSample {
                t0_guest_us: t0_us as i64,
                host_receive_us: host_receive_us as i64,
                host_send_us: host_send_us as i64,
                t3_guest_us: monotonic_us() as i64,
            }),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    pub async fn send_play_ready(
        &self,
        operation_id: String,
        ready: bool,
        position_ms: u64,
        buffer_ahead_ms: u64,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::PlayReady {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            operation_id,
            ready,
            position_ms,
            buffer_ahead_ms,
        })
        .await
    }

    pub async fn send_pause_ready(
        &self,
        operation_id: String,
        ready: bool,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::PauseReady {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            operation_id,
            ready,
        })
        .await
    }

    pub async fn send_seek_ready(
        &self,
        operation_id: String,
        ready: bool,
        buffer_ahead_ms: u64,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::SeekReady {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            operation_id,
            ready,
            buffer_ahead_ms,
        })
        .await
    }

    pub async fn send_clock_result(
        &self,
        offset_to_host_us: i64,
        rtt_p95_us: u64,
        sample_count: u64,
        quality: &str,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::ClockResult {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            offset_to_host_us,
            rtt_us: rtt_p95_us,
            sample_count,
            quality: quality.to_string(),
        })
        .await
    }

    pub async fn send_control_request(
        &self,
        request_id: String,
        action: String,
        parameters: serde_json::Value,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::ControlRequest {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            request_id,
            action,
            parameters,
        })
        .await
    }

    /// (§56): guest acknowledges a schedule. Fire-and-forget like
    /// the other guest control sends — the authoritative confirmation arrives
    /// as the host's ScheduleAccept *broadcast*.
    pub async fn send_schedule_accept(
        &self,
        schedule_id: String,
        accepted: bool,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::ScheduleAccept {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            schedule_id,
            accepted,
        })
        .await
    }

    pub async fn upload_synthetic_payload(
        &self,
        bytes: u64,
        chunk_size: usize,
    ) -> Result<ThroughputResult, QuicError> {
        let (mut send, mut recv) = self.connection.open_bi().await?;
        let request = ClientRequest::ThroughputUpload {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            sent_mono_us: monotonic_us(),
            bytes,
        };
        write_request(&mut send, &request).await?;

        let chunk = vec![0xA5; chunk_size.max(1)];
        let mut remaining = bytes;
        while remaining > 0 {
            let len = remaining.min(chunk.len() as u64) as usize;
            send.write_all(&chunk[..len]).await?;
            remaining -= len as u64;
        }
        send.finish().map_err(|_| QuicError::ClosedStream)?;

        let response = read_response(&mut recv).await?;
        match response {
            ServerResponse::ThroughputResult {
                bytes,
                elapsed_us,
                goodput_bps,
            } => Ok(ThroughputResult {
                bytes,
                elapsed_us,
                goodput_bps,
            }),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    pub async fn send_control(&self, request: ClientRequest) -> Result<ServerResponse, QuicError> {
        self.send_request(request).await
    }

    pub async fn send_ready_state(
        &self,
        ready: bool,
        buffer_ahead_ms: u64,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::ReadyState {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            ready,
            buffer_ahead_ms,
        })
        .await
    }

    pub async fn send_chat_message(
        &self,
        message_id: Uuid,
        body: String,
        created_host_time_us: u64,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::ChatMessage {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            message_id: message_id.to_string(),
            body,
            created_host_time_us,
        })
        .await
    }

    pub async fn send_reaction(
        &self,
        reaction_id: Uuid,
        reaction: String,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::Reaction {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            reaction_id: reaction_id.to_string(),
            reaction,
        })
        .await
    }

    pub async fn send_call_signal(
        &self,
        signal_type: String,
        data: String,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::CallSignal {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            signal_type,
            data,
        })
        .await
    }

    /// Report local buffer starvation/recovery to the host.
    ///
    /// The host treats this as canonical strict-sync input: `stalled: true`
    /// pauses the room at the reported position, `stalled: false` resumes it.
    pub async fn send_buffer_status(
        &self,
        position_ms: u64,
        buffer_ahead_ms: u64,
        stalled: bool,
    ) -> Result<ServerResponse, QuicError> {
        self.send_control(ClientRequest::BufferStatus {
            seq: self.next_seq(),
            sender: self.identity.device_id.clone(),
            position_ms,
            buffer_ahead_ms,
            stalled,
        })
        .await
    }

    /// M3: Fetch the media manifest from the host for Local Perfect transfer.
    pub async fn fetch_local_media_manifest(
        &self,
    ) -> Result<crate::media::manifest::MediaManifest, QuicError> {
        let response = self
            .send_control(ClientRequest::ManifestRequest {
                seq: self.next_seq(),
                sender: self.identity.device_id.clone(),
            })
            .await?;
        match response {
            ServerResponse::ManifestResponse { manifest } => Ok(manifest),
            ServerResponse::MediaError { .. } => Err(QuicError::UnexpectedResponse),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    /// M3: Fetch a single chunk from the host by index.
    ///
    /// The chunk is transported on a dedicated QUIC stream using the binary
    /// chunk-stream format (`MPCK` magic, PROTOCOL_SPEC §45). Control JSON
    /// never carries media payloads.
    pub async fn fetch_local_media_chunk(
        &self,
        media_id: &str,
        chunk_index: u64,
    ) -> Result<crate::media::transfer::ChunkPacket, QuicError> {
        let (mut send, mut recv) = self.connection.open_bi().await?;
        write_request(
            &mut send,
            &ClientRequest::ChunkRequest {
                seq: self.next_seq(),
                sender: self.identity.device_id.clone(),
                media_id: media_id.to_string(),
                chunk_index,
            },
        )
        .await?;
        send.finish().map_err(|_| QuicError::ClosedStream)?;

        // Read the leading 4 bytes: a binary chunk starts with the MPCK
        // magic; otherwise the host wrote a JSON error response.
        let mut magic = [0u8; 4];
        recv.read_exact(&mut magic)
            .await
            .map_err(|error| QuicError::ReadExact(error.to_string()))?;
        let mut rest = recv
            .read_to_end(MAX_RAW_STREAM_BYTES)
            .await
            .map_err(QuicError::Read)?;
        let mut payload = Vec::with_capacity(4 + rest.len());
        payload.extend_from_slice(&magic);
        payload.append(&mut rest);
        if &magic == crate::media::transfer::CHUNK_MAGIC {
            return crate::media::transfer::decode_chunk_stream(&payload)
                .map_err(|_| QuicError::UnexpectedResponse);
        }
        let response: ServerResponse =
            serde_json::from_slice(&payload).map_err(|_| QuicError::UnexpectedResponse)?;
        match response {
            ServerResponse::ChunkUnavailable { .. }
            | ServerResponse::MediaError { .. }
            | ServerResponse::AuthReject { .. } => Err(QuicError::UnexpectedResponse),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    /// Receive the next event opened by the host.
    ///
    /// The Host opens one bidirectional stream per [`EventEnvelope`] (see
    /// `handle_connection`'s dispatcher task) and writes a single envelope
    /// into the stream before half-closing the send side. The Guest side
    /// therefore calls [`Connection::accept_bi`] to receive the next
    /// server-opened stream, reads a single envelope from it, then discards
    /// the unused `send` half.
    ///
    /// Using `accept_bi` for receiving host-originated events is critical:
    /// the alternative (Guest opens a stream and reads from it) would
    /// race against the host's accept loop for client→host requests.
    pub async fn listen_for_server_event(&self) -> Result<EventEnvelope, QuicError> {
        let (_send, mut recv) = self.connection.accept_bi().await?;
        let envelope = read_event_envelope(&mut recv).await?;
        // Drain any trailing EOF and discard the unused send half. Errors here
        // are tolerated — the envelope was already received.
        let _ = recv.read_to_end(0).await;
        Ok(envelope)
    }

    pub async fn wait_idle(&self) {
        self.connection.close(0u32.into(), b"done");
        self.endpoint.wait_idle().await;
    }

    fn auth_request(&self) -> AuthRequest {
        let invite_nonce = Uuid::now_v7().to_string();
        let join_secret_hash = self.credentials.join_secret_hash();
        AuthRequest {
            room_id: self.credentials.room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: invite_nonce.clone(),
            device_signature: self.identity.sign_auth_request(
                &self.credentials.room_id,
                &join_secret_hash,
                &invite_nonce,
            ),
        }
    }

    async fn send_request(&self, request: ClientRequest) -> Result<ServerResponse, QuicError> {
        let (mut send, mut recv) = self.connection.open_bi().await?;
        write_request(&mut send, &request).await?;
        send.finish().map_err(|_| QuicError::ClosedStream)?;
        read_response(&mut recv).await
    }

    fn next_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::SeqCst)
    }
}

pub fn loopback_bind_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

/// Local bind address for a QUIC client endpoint.
///
/// The client must NOT pin its source socket to the loopback address: macOS
/// refuses to send UDP from a `127.0.0.1`-bound socket to a destination that
/// is routed through a non-loopback interface, and a real guest's destination
/// is the host's Tailscale CGNAT IPv4. Binding to the unspecified address lets
/// the OS choose the correct source interface per destination (Tailscale's
/// utun for remote CGNAT hosts, loopback for dev-loopback hosts). This is what
/// makes a genuine two-device Tailscale join work.
pub fn client_bind_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
}

pub fn tailscale_bind_addr(local_ipv4: Ipv4Addr) -> Result<SocketAddr, QuicError> {
    let addr = SocketAddr::new(
        IpAddr::V4(local_ipv4),
        crate::network::tailscale::DEFAULT_TAILSCALE_PORT,
    );
    validate_quic_bind_addr(addr)?;
    Ok(addr)
}

pub fn validate_quic_bind_addr(bind_addr: SocketAddr) -> Result<(), QuicError> {
    match bind_addr.ip() {
        IpAddr::V4(ipv4) if ipv4.is_loopback() || is_tailscale_ipv4(ipv4) => Ok(()),
        _ => Err(QuicError::InvalidBindAddress),
    }
}

fn is_tailscale_ipv4(ipv4: Ipv4Addr) -> bool {
    crate::network::tailscale::is_usable_tailscale_ipv4(ipv4)
}

pub fn certificate_fingerprint(certificate: &rustls::pki_types::CertificateDer<'_>) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(certificate.as_ref()))
}

fn hash_secret(secret: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes()))
}

fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "unsupported",
    }
}

fn configure_server() -> Result<(ServerConfig, CertificateDer<'static>), QuicError> {
    ensure_crypto_provider();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .map_err(|error| QuicError::Tls(error.to_string()))?;
    let cert_der = CertificateDer::from(cert.cert);
    let priv_key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());

    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], priv_key.into())
        .map_err(|error| QuicError::Tls(error.to_string()))?;
    crypto.alpn_protocols = MOVIE_PARTY_ALPN
        .iter()
        .map(|value| value.to_vec())
        .collect();

    let crypto =
        QuicServerConfig::try_from(crypto).map_err(|error| QuicError::Tls(error.to_string()))?;
    Ok((ServerConfig::with_crypto(Arc::new(crypto)), cert_der))
}

/// Verifies the peer's certificate by pinned SHA-256 fingerprint **and**
/// proves possession of that certificate's private key.
///
/// Fingerprint pinning alone is *not* authentication. The certificate DER is
/// public: it is generated per host session, sent in the clear in every
/// handshake, and its fingerprint travels in the invite. Anyone holding a
/// copy could present it. TLS 1.3 proves key possession through the
/// `CertificateVerify` signature, so that signature must be checked against
/// the certificate's own public key — which is what these callbacks now do
/// (F44). Without it the pin restricts *which* certificate is acceptable but
/// authenticates nothing.
struct FingerprintVerifier {
    fingerprint: String,
    /// Signature algorithms of the crypto provider the endpoint was built
    /// with. Must come from the same provider, or a legitimately negotiated
    /// scheme could be rejected as unsupported.
    signature_algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl std::fmt::Debug for FingerprintVerifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `WebPkiSupportedAlgorithms` is not `Debug`; the trait requires it,
        // so render only the pin.
        formatter
            .debug_struct("FingerprintVerifier")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl rustls::client::danger::ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let actual = certificate_fingerprint(end_entity);
        if actual == self.fingerprint {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("fingerprint mismatch".into()))
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.signature_algorithms)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.signature_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.signature_algorithms.supported_schemes()
    }
}

fn make_client_endpoint(server_certificate_fingerprint: String) -> Result<Endpoint, QuicError> {
    ensure_crypto_provider();
    let verifier = std::sync::Arc::new(FingerprintVerifier {
        fingerprint: server_certificate_fingerprint,
        signature_algorithms: installed_signature_algorithms(),
    });
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    crypto.alpn_protocols = MOVIE_PARTY_ALPN
        .iter()
        .map(|value| value.to_vec())
        .collect();

    let crypto =
        QuicClientConfig::try_from(crypto).map_err(|error| QuicError::Tls(error.to_string()))?;
    let client_config = ClientConfig::new(Arc::new(crypto));
    let mut endpoint = Endpoint::client(client_bind_addr())?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

fn ensure_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// Signature algorithms of the process-wide crypto provider actually in
/// use. Read back from the installed default so the client verifier and the
/// endpoint are guaranteed to agree on schemes, even if another provider
/// was installed before Movie Party had a chance to.
fn installed_signature_algorithms() -> rustls::crypto::WebPkiSupportedAlgorithms {
    rustls::crypto::CryptoProvider::get_default()
        .map(|provider| provider.signature_verification_algorithms)
        .unwrap_or_else(|| {
            rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms
        })
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    connection: Connection,
    credentials: RoomCredentials,
    replay_guard: AuthReplayGuard,
    host_display_name: String,
    host_device_id: String,
    event_callback: Option<std::sync::Arc<dyn Fn(QuicHostEvent) + Send + Sync>>,
    local_media: Option<String>,
    event_tx: Option<std::sync::Arc<broadcast::Sender<EventEnvelope>>>,
    coordinator: std::option::Option<
        std::sync::Arc<std::sync::Mutex<crate::sync::local::LocalSyncCoordinator>>,
    >,
    shared_controls: Option<std::sync::Arc<AtomicBool>>,
) -> Result<(), QuicError> {
    let session = Arc::new(Mutex::new(ConnectionSession::default()));

    if let Some(ref tx) = event_tx {
        let connection_for_events = connection.clone();
        let tx_for_events = tx.clone();
        tokio::spawn(async move {
            let mut rx = tx_for_events.subscribe();
            loop {
                let Ok(envelope) = rx.recv().await else { break };
                let Ok((mut send, _)) = connection_for_events.open_bi().await else {
                    break;
                };
                if write_event_envelope(&mut send, &envelope).await.is_err() {
                    break;
                }
                if send.finish().is_err() {
                    break;
                }
            }
        });
    }

    loop {
        let stream = connection.accept_bi().await;
        let stream = match stream {
            Ok(stream) => stream,
            Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                fire_peer_disconnected(&session, &event_callback);
                return Ok(());
            }
            Err(error) => {
                fire_peer_disconnected(&session, &event_callback);
                return Err(QuicError::Connection(error));
            }
        };

        let credentials = credentials.clone();
        let replay_guard = replay_guard.clone();
        let session = session.clone();
        let host_display_name = host_display_name.clone();
        let host_device_id = host_device_id.clone();
        let event_callback = event_callback.clone();
        let local_media = local_media.clone();
        let coordinator = coordinator.clone();
        let event_tx = event_tx.clone();
        let shared_controls = shared_controls.clone();
        tokio::spawn(async move {
            let _ = handle_request(
                stream,
                credentials,
                replay_guard,
                host_display_name,
                host_device_id,
                event_callback,
                local_media,
                session,
                coordinator,
                event_tx,
                shared_controls,
            )
            .await;
        });
    }
}

#[derive(Debug, Default)]
struct ConnectionSession {
    authenticated_device_id: Option<String>,
    authenticated_display_name: Option<String>,
    sequence_tracker: SequenceTracker,
    guest_clock_offset_to_host_us: i64,
    guest_clock_rtt_p95_us: u64,
    guest_clock_sample_count: u64,
    guest_clock_quality: String,
}

fn fire_peer_disconnected(
    session: &Arc<Mutex<ConnectionSession>>,
    event_callback: &Option<std::sync::Arc<dyn Fn(QuicHostEvent) + Send + Sync>>,
) {
    let session = lock_session(session);
    if let Some(device_id) = session.authenticated_device_id.clone() {
        if let Some(ref cb) = event_callback {
            cb(QuicHostEvent::PeerDisconnected { device_id });
        }
    }
}

fn guest_display_name(session: &Arc<Mutex<ConnectionSession>>) -> String {
    lock_session(session)
        .authenticated_display_name
        .clone()
        .unwrap_or_else(|| "Guest".to_string())
}

fn broadcast_guest_event(
    event_tx: &Option<std::sync::Arc<broadcast::Sender<EventEnvelope>>>,
    seq: u64,
    sender_device_id: &str,
    room_id: &str,
    event: ServerEvent,
) {
    if let Some(tx) = event_tx {
        let _ = tx.send(EventEnvelope {
            v_major: ENVELOPE_V_MAJOR,
            v_minor: ENVELOPE_V_MINOR,
            room_id: room_id.to_string(),
            seq,
            sender: sender_device_id.to_string(),
            sent_mono_us: monotonic_us(),
            event,
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_request(
    (mut send, mut recv): (quinn::SendStream, quinn::RecvStream),
    credentials: RoomCredentials,
    replay_guard: AuthReplayGuard,
    host_display_name: String,
    host_device_id: String,
    event_callback: Option<std::sync::Arc<dyn Fn(QuicHostEvent) + Send + Sync>>,
    _local_media: Option<String>,
    session: Arc<Mutex<ConnectionSession>>,
    coordinator: std::option::Option<
        std::sync::Arc<std::sync::Mutex<crate::sync::local::LocalSyncCoordinator>>,
    >,
    event_tx: Option<std::sync::Arc<broadcast::Sender<EventEnvelope>>>,
    shared_controls: Option<std::sync::Arc<AtomicBool>>,
) -> Result<(), QuicError> {
    let request = read_request(&mut recv).await?;
    let response = match request {
        ClientRequest::HelloAuth { hello, auth } => {
            let authenticated_device_id = hello.device_id.clone();
            let guest_display_name = hello.display_name.clone();
            let response = validate_handshake(
                &credentials,
                &replay_guard,
                *hello,
                *auth,
                host_display_name,
                host_device_id,
            );
            if matches!(response, ServerResponse::AuthAccept(_)) {
                let mut session = lock_session(&session);
                session.authenticated_device_id = Some(authenticated_device_id.clone());
                session.authenticated_display_name = Some(guest_display_name.clone());

                if let Some(ref cb) = event_callback {
                    cb(QuicHostEvent::PeerAuthenticated {
                        device_id: authenticated_device_id,
                        display_name: guest_display_name,
                    });
                }
            }
            response
        }
        ClientRequest::Heartbeat { seq, sender, .. } => {
            match validate_authenticated_sequence(&session, &sender, seq) {
                Ok(()) => ServerResponse::HeartbeatAck {
                    room_state: coordinator
                        .as_ref()
                        .and_then(|coordinator| {
                            coordinator
                                .lock()
                                .ok()
                                .map(|c| format!("{:?}", c.room_state).to_ascii_uppercase())
                        })
                        .unwrap_or_else(|| "LOBBY".to_string()),
                },
                Err(code) => ServerResponse::AuthReject { code },
            }
        }
        ClientRequest::Ping {
            seq,
            sender,
            sent_mono_us: _,
            probe_id,
            t0_us,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                let receive = monotonic_us();
                ServerResponse::Pong {
                    probe_id,
                    t0_us,
                    host_receive_us: receive,
                    host_send_us: monotonic_us(),
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::ThroughputUpload {
            seq,
            sender,
            sent_mono_us: _,
            bytes,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                let start = Instant::now();
                let mut remaining = bytes;
                while remaining > 0 {
                    match recv.read_chunk(64 * 1024, true).await {
                        Ok(Some(chunk)) => {
                            remaining = remaining.saturating_sub(chunk.bytes.len() as u64);
                        }
                        Ok(None) => break,
                        Err(error) => return Err(QuicError::Read(error.into())),
                    }
                }
                let elapsed = start.elapsed();
                ServerResponse::ThroughputResult {
                    bytes: bytes.saturating_sub(remaining),
                    elapsed_us: elapsed.as_micros(),
                    goodput_bps: goodput_bps(bytes.saturating_sub(remaining), elapsed),
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::ReadyState {
            seq,
            sender,
            ready,
            buffer_ahead_ms,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                // Readiness consensus ONLY: pressing Ready must never by-pass
                // the distributed play protocol (PLAY_PREPARE → PLAY_READY →
                // PLAY_COMMIT). The coordinator records guest readiness and —
                // once BOTH participants are ready — enters READY_CHECK so
                // both sides can see the consensus. PLAYING only ever arrives
                // via a committed play operation.
                if let Some(ref coord) = coordinator {
                    if let Ok(mut guard) = coord.lock() {
                        if ready {
                            guard.guest_ready(crate::sync::consensus::ParticipantReadiness::ready(
                                5_000,
                            ));
                        } else {
                            guard.guest_ready(
                                crate::sync::consensus::ParticipantReadiness::not_ready("guest"),
                            );
                        }
                        guard.update_readiness_consensus(5_000);
                    }
                }
                if let Some(ref cb) = event_callback {
                    cb(QuicHostEvent::GuestReadyState {
                        broadcaster_device_id: sender,
                        coordinator_play_state: coordinator
                            .as_ref()
                            .and_then(|c| c.lock().ok())
                            .map(|c| format!("{:?}", c.room_state))
                            .unwrap_or_default(),
                        coordinator_ready: ready,
                        coordinator_buffer_ahead_ms: buffer_ahead_ms,
                    });
                }
                ServerResponse::ReadyAck { ready }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::PlayReady {
            seq,
            sender,
            operation_id,
            ready,
            position_ms,
            buffer_ahead_ms,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                if let Some(ref cb) = event_callback {
                    cb(QuicHostEvent::GuestPlayReady {
                        broadcaster_device_id: sender,
                        operation_id: operation_id.clone(),
                        ready,
                        position_ms,
                        buffer_ahead_ms,
                    });
                }
                ServerResponse::PlayReadyAck {
                    operation_id,
                    accepted: true,
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::PauseReady {
            seq,
            sender,
            operation_id,
            ready,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                if let Some(ref cb) = event_callback {
                    cb(QuicHostEvent::GuestPauseReady {
                        broadcaster_device_id: sender,
                        operation_id: operation_id.clone(),
                        ready,
                    });
                }
                ServerResponse::PauseReadyAck {
                    operation_id,
                    accepted: true,
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::SeekReady {
            seq,
            sender,
            operation_id,
            ready,
            buffer_ahead_ms,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                if let Some(ref cb) = event_callback {
                    cb(QuicHostEvent::GuestSeekReady {
                        broadcaster_device_id: sender,
                        operation_id: operation_id.clone(),
                        ready,
                        buffer_ahead_ms,
                    });
                }
                ServerResponse::SeekReadyAck {
                    operation_id,
                    accepted: true,
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::ClockResult {
            seq,
            sender,
            offset_to_host_us,
            rtt_us,
            sample_count,
            quality,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                {
                    let mut session = lock_session(&session);
                    session.guest_clock_offset_to_host_us = offset_to_host_us;
                    session.guest_clock_rtt_p95_us = rtt_us;
                    session.guest_clock_sample_count = sample_count;
                    session.guest_clock_quality = quality.clone();
                }
                if let Some(ref cb) = event_callback {
                    cb(QuicHostEvent::ClockResultReceived {
                        broadcaster_device_id: sender,
                        offset_to_host_us,
                        rtt_p95_us: rtt_us,
                        sample_count,
                        quality,
                    });
                }
                ServerResponse::ClockResultAck { accepted: true }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::BufferStatus {
            seq,
            sender,
            position_ms,
            buffer_ahead_ms,
            stalled,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                if stalled {
                    broadcast_guest_event(
                        &event_tx,
                        seq,
                        &sender,
                        &credentials.room_id,
                        ServerEvent::BufferLow {
                            position_ms,
                            buffer_ahead_ms,
                        },
                    );
                } else {
                    broadcast_guest_event(
                        &event_tx,
                        seq,
                        &sender,
                        &credentials.room_id,
                        ServerEvent::BufferRecovered { buffer_ahead_ms },
                    );
                }
                ServerResponse::BufferAck { accepted: true }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::ChatMessage {
            seq,
            sender,
            message_id,
            body,
            created_host_time_us,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                broadcast_guest_event(
                    &event_tx,
                    seq,
                    &sender,
                    &credentials.room_id,
                    ServerEvent::ChatMessage {
                        message_id: message_id.clone(),
                        sender: guest_display_name(&session),
                        body: body.clone(),
                        created_host_time_us,
                    },
                );
                ServerResponse::ChatAccepted { message_id }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::Reaction {
            seq,
            sender,
            reaction_id,
            reaction,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                broadcast_guest_event(
                    &event_tx,
                    seq,
                    &sender,
                    &credentials.room_id,
                    ServerEvent::Reaction {
                        reaction_id: reaction_id.clone(),
                        sender: guest_display_name(&session),
                        reaction: reaction.clone(),
                    },
                );
                ServerResponse::ReactionAccepted { reaction_id }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::ControlRequest {
            seq,
            sender,
            request_id,
            action,
            parameters,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                let granted = shared_controls
                    .as_ref()
                    .map(|flag| flag.load(Ordering::SeqCst))
                    .unwrap_or(false);
                if granted {
                    // GRANT means "host will consider it": the host AppRuntime
                    // performs the normal authoritative PREPARE/READY/COMMIT
                    // operation. The guest never commits anything itself.
                    if let Some(ref cb) = event_callback {
                        cb(QuicHostEvent::GuestControlRequest {
                            broadcaster_device_id: sender,
                            request_id: request_id.clone(),
                            action,
                            parameters,
                        });
                    }
                    ServerResponse::ControlResponse {
                        request_id,
                        granted: true,
                        reason: None,
                    }
                } else {
                    // The denial must ALSO reach the guest as an event: the
                    // in-band response alone is invisible to the guest
                    // AppRuntime (its control requests are fire-and-forget).
                    // The seq comes from the coordinator's monotonic counter
                    // so the guest's stale/duplicate guard stays consistent.
                    let seq = coordinator
                        .as_ref()
                        .and_then(|c| c.lock().ok())
                        .map(|mut guard| {
                            guard.coordinator_event_seq =
                                guard.coordinator_event_seq.wrapping_add(1);
                            guard.coordinator_event_seq
                        })
                        .unwrap_or(1);
                    if let Some(ref tx) = event_tx {
                        let envelope = EventEnvelope {
                            v_major: ENVELOPE_V_MAJOR,
                            v_minor: ENVELOPE_V_MINOR,
                            room_id: credentials.room_id.clone(),
                            seq,
                            sender: host_device_id.clone(),
                            sent_mono_us: monotonic_us(),
                            event: ServerEvent::ControlDeny {
                                request_id: request_id.clone(),
                                reason: "host_only_controls".to_string(),
                            },
                        };
                        let _ = tx.send(envelope);
                    }
                    ServerResponse::ControlResponse {
                        request_id,
                        granted: false,
                        reason: Some("host_only_controls".to_string()),
                    }
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        ClientRequest::CallSignal {
            seq,
            sender,
            signal_type,
            data,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                broadcast_guest_event(
                    &event_tx,
                    seq,
                    &sender,
                    &credentials.room_id,
                    ServerEvent::CallSignal { signal_type, data },
                );
                ServerResponse::ReadyAck { ready: true }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        // (§56): the guest's schedule acknowledgement is relayed
        // to the host AppRuntime and echoed to both rooms as a broadcast so
        // each side can persist + register reminders exactly once.
        ClientRequest::ScheduleAccept {
            seq,
            sender,
            schedule_id,
            accepted,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                broadcast_guest_event(
                    &event_tx,
                    seq,
                    &sender,
                    &credentials.room_id,
                    ServerEvent::ScheduleAccept {
                        schedule_id: schedule_id.clone(),
                        accepted,
                    },
                );
                if let Some(ref cb) = event_callback {
                    cb(QuicHostEvent::GuestScheduleAccept {
                        broadcaster_device_id: sender,
                        schedule_id: schedule_id.clone(),
                        accepted,
                    });
                }
                ServerResponse::ScheduleAcceptAck {
                    schedule_id,
                    accepted,
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
        // M3: Guest requests the media manifest for Local Perfect transfer.
        ClientRequest::ManifestRequest { seq, sender } => {
            match validate_authenticated_sequence(&session, &sender, seq) {
                Ok(()) => {
                    if let Some(ref media_path) = _local_media {
                        let path = std::path::Path::new(media_path);
                        match crate::media::manifest::build_manifest(path) {
                            Ok(manifest) => ServerResponse::ManifestResponse { manifest },
                            Err(e) => ServerResponse::MediaError {
                                code: "MP-MEDIA-001".to_string(),
                                message: format!("manifest build failed: {e}"),
                                operation_id: None,
                            },
                        }
                    } else {
                        ServerResponse::MediaError {
                            code: "MP-MEDIA-002".to_string(),
                            message: "no local media available".to_string(),
                            operation_id: None,
                        }
                    }
                }
                Err(code) => ServerResponse::AuthReject { code },
            }
        }
        // M3: Guest requests a specific chunk. The chunk is read from the
        // local file and transported on this dedicated stream using the
        // binary chunk-stream format (PROTOCOL_SPEC §45). Control JSON never
        // carries media payloads — the raw chunk bytes travel on the stream
        // after the small authenticated JSON request.
        ClientRequest::ChunkRequest {
            seq,
            sender,
            media_id,
            chunk_index,
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => {
                if let Some(ref media_path) = _local_media {
                    let path = std::path::Path::new(media_path);
                    match read_chunk_from_file(path, chunk_index) {
                        Ok((hash, payload)) => {
                            let packet = crate::media::transfer::ChunkPacket {
                                media_id: media_id.clone(),
                                chunk_index: u32::try_from(chunk_index).unwrap_or(u32::MAX),
                                hash,
                                payload,
                            };
                            match crate::media::transfer::encode_chunk_stream(&packet) {
                                Ok(bytes) => {
                                    send.write_all(&bytes).await.map_err(QuicError::Write)?;
                                    send.finish().map_err(|_| QuicError::ClosedStream)?;
                                    return Ok(());
                                }
                                Err(_) => ServerResponse::MediaError {
                                    code: "MP-MEDIA-002".to_string(),
                                    message: "chunk stream encode failed".to_string(),
                                    operation_id: None,
                                },
                            }
                        }
                        Err(e) => ServerResponse::MediaError {
                            code: "MP-MEDIA-002".to_string(),
                            message: format!("chunk read failed: {e}"),
                            operation_id: None,
                        },
                    }
                } else {
                    ServerResponse::ChunkUnavailable {
                        media_id,
                        chunk_index,
                    }
                }
            }
            Err(code) => ServerResponse::AuthReject { code },
        },
    };

    write_response(&mut send, &response).await?;
    send.finish().map_err(|_| QuicError::ClosedStream)?;
    Ok(())
}

fn validate_authenticated_sequence(
    session: &Arc<Mutex<ConnectionSession>>,
    sender: &str,
    seq: u64,
) -> Result<(), String> {
    let mut session = lock_session(session);
    match session.authenticated_device_id.as_deref() {
        Some(device_id) if device_id == sender => {}
        Some(_) => return Err("SENDER_MISMATCH".to_string()),
        None => return Err("AUTH_REQUIRED".to_string()),
    }

    if !session.sequence_tracker.accept(seq) {
        return Err("INVALID_SEQUENCE".to_string());
    }

    Ok(())
}

fn lock_session(
    session: &Arc<Mutex<ConnectionSession>>,
) -> std::sync::MutexGuard<'_, ConnectionSession> {
    match session.lock() {
        Ok(session) => session,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn validate_handshake(
    credentials: &RoomCredentials,
    replay_guard: &AuthReplayGuard,
    hello: HelloPayload,
    auth: AuthRequest,
    host_display_name: String,
    host_device_id: String,
) -> ServerResponse {
    if hello.protocol_major != crate::PROTOCOL_MAJOR {
        return ServerResponse::AuthReject {
            code: "PROTOCOL_MISMATCH".to_string(),
        };
    }

    if hello.protocol_minor > crate::PROTOCOL_MINOR {
        return ServerResponse::AuthReject {
            code: "UNSUPPORTED_MINOR_VERSION".to_string(),
        };
    }

    if !is_uuid_v7(&hello.device_id) {
        return ServerResponse::AuthReject {
            code: "INVALID_DEVICE_ID".to_string(),
        };
    }

    if !matches!(hello.platform.as_str(), "windows" | "macos") {
        return ServerResponse::AuthReject {
            code: "UNSUPPORTED_PLATFORM".to_string(),
        };
    }

    if !is_base64url_128bit(&auth.room_id) {
        return ServerResponse::AuthReject {
            code: "INVALID_ROOM_ID".to_string(),
        };
    }

    if !is_base64url_256bit(&auth.join_secret_hash) {
        return ServerResponse::AuthReject {
            code: "INVALID_SECRET_HASH".to_string(),
        };
    }

    if !is_base64url_256bit(&hello.public_key) {
        return ServerResponse::AuthReject {
            code: "INVALID_PUBLIC_KEY".to_string(),
        };
    }

    if auth.room_id != credentials.room_id
        || auth.join_secret_hash != credentials.join_secret_hash()
    {
        return ServerResponse::AuthReject {
            code: "INVALID_SECRET".to_string(),
        };
    }

    if verify_auth_request_signature(
        &hello.public_key,
        &auth.device_signature,
        &auth.room_id,
        &auth.join_secret_hash,
        &auth.invite_nonce,
        &hello.device_id,
    )
    .is_err()
    {
        return ServerResponse::AuthReject {
            code: "INVALID_SIGNATURE".to_string(),
        };
    }

    if !replay_guard.accept_once(&hello.device_id, &auth.invite_nonce) {
        return ServerResponse::AuthReject {
            code: "REPLAYED_AUTH".to_string(),
        };
    }

    ServerResponse::AuthAccept(AuthAccept {
        session_id: Uuid::now_v7().to_string(),
        room_role: "guest".to_string(),
        host_display_name,
        host_device_id,
    })
}

fn is_uuid_v7(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|uuid| uuid.get_version_num() == 7)
}

pub fn is_base64url_128bit(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == 16)
}

pub fn is_base64url_256bit(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == 32)
}

async fn write_request(
    send: &mut (impl tokio::io::AsyncWrite + Unpin),
    request: &ClientRequest,
) -> Result<(), QuicError> {
    write_json(send, request).await
}

async fn write_response(
    send: &mut (impl tokio::io::AsyncWrite + Unpin),
    response: &ServerResponse,
) -> Result<(), QuicError> {
    write_json(send, response).await
}

async fn write_json<T: Serialize>(
    send: &mut (impl tokio::io::AsyncWrite + Unpin),
    value: &T,
) -> Result<(), QuicError> {
    use tokio::io::AsyncWriteExt;
    let json = serde_json::to_vec(value)?;
    let len = u32::try_from(json.len()).map_err(|_| QuicError::UnexpectedResponse)?;
    // §5: refuse to serialize a control frame larger than 256 KiB. The
    // local side built it, so this is a programmer error surfaced as
    // MP-PROTO-004 rather than silently sending an oversized frame the
    // peer must reject.
    if json.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(QuicError::MessageTooLarge { len: json.len() });
    }
    send.write_all(&len.to_be_bytes()).await?;
    send.write_all(&json).await?;
    Ok(())
}

async fn read_request(
    recv: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<ClientRequest, QuicError> {
    let bytes = read_json_bytes(recv).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn read_response(
    recv: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<ServerResponse, QuicError> {
    let bytes = read_json_bytes(recv).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn read_json_bytes(
    recv: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<Vec<u8>, QuicError> {
    use tokio::io::AsyncReadExt;
    let mut len = [0_u8; 4];
    recv.read_exact(&mut len)
        .await
        .map_err(|error| QuicError::ReadExact(error.to_string()))?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_CONTROL_FRAME_BYTES {
        return Err(QuicError::MessageTooLarge { len });
    }
    let mut bytes = vec![0; len];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|error| QuicError::ReadExact(error.to_string()))?;
    Ok(bytes)
}

async fn write_event_envelope(
    send: &mut (impl tokio::io::AsyncWrite + Unpin),
    envelope: &EventEnvelope,
) -> Result<(), QuicError> {
    write_json(send, envelope).await
}

/// Read and validate a host event envelope (§10): the transport framing
/// limit is enforced by [`read_json_bytes`]; this adds the envelope-level
/// checks — protocol version (§6) and room binding (§10) — so malformed
/// peer input is rejected before it can be applied to runtime state
/// (§67: malformed input must never crash or corrupt the app).
async fn read_event_envelope(
    recv: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<EventEnvelope, QuicError> {
    let bytes = read_json_bytes(recv).await?;
    let envelope: EventEnvelope = serde_json::from_slice(&bytes)?;
    validate_event_envelope(&envelope)?;
    Ok(envelope)
}

/// Envelope validation shared by the QUIC receive path and tests.
/// The room_id must be present: event envelopes only travel after
/// authentication, and §10 allows an empty room_id only during the
/// initial HELLO — which is a ClientRequest, never an EventEnvelope.
fn validate_event_envelope(envelope: &EventEnvelope) -> Result<(), QuicError> {
    if envelope.v_major != ENVELOPE_V_MAJOR {
        return Err(QuicError::EnvelopeVersion {
            detail: format!("expected {ENVELOPE_V_MAJOR}, got {}", envelope.v_major),
        });
    }
    if envelope.v_minor > ENVELOPE_V_MINOR {
        return Err(QuicError::EnvelopeVersion {
            detail: format!("expected <= {ENVELOPE_V_MINOR}, got {}", envelope.v_minor),
        });
    }
    if envelope.room_id.trim().is_empty() {
        return Err(QuicError::EnvelopeRoom {
            detail: "room_id missing (only legal before authentication)".to_string(),
        });
    }
    Ok(())
}

static MONO_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Monotonic microseconds since the first call in this process.
/// Used for protocol timestamps and scheduling; never wall-clock.
pub fn monotonic_us() -> u64 {
    MONO_START.get_or_init(std::time::Instant::now);
    mono_start().elapsed().as_micros() as u64
}

fn mono_start() -> std::time::Instant {
    *MONO_START.get_or_init(std::time::Instant::now)
}

/// Convert a HOST-monotonic microsecond deadline into this process's
/// [`std::time::Instant`], applying the guest↔host clock offset.
///
/// Hosts call with `offset_to_host_us = 0`. Guests pass their calibrated
/// `offset_to_host_us` (host time = guest time + offset).
pub fn instant_for_host_mono(host_mono_us: u64, offset_to_host_us: i64) -> std::time::Instant {
    let guest_mono_us = (host_mono_us as i64).saturating_sub(offset_to_host_us);
    mono_start() + std::time::Duration::from_micros(guest_mono_us.max(0) as u64)
}

/// M3: Read a single chunk from the host's local media file.
/// Returns (blake3_hash, raw_bytes).
fn read_chunk_from_file(
    path: &std::path::Path,
    chunk_index: u64,
) -> Result<(String, Vec<u8>), std::io::Error> {
    use std::io::{Read, Seek, SeekFrom};

    const CHUNK_SIZE: u64 = 1_048_576; // 1 MiB

    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    let offset = chunk_index * CHUNK_SIZE;
    if offset >= file_size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "chunk index out of range",
        ));
    }
    let len = CHUNK_SIZE.min(file_size - offset) as usize;
    let mut buf = vec![0u8; len];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut buf)?;
    let hash =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(blake3::hash(&buf).as_bytes());
    Ok((hash, buf))
}

fn goodput_bps(bytes: u64, elapsed: Duration) -> u64 {
    let elapsed_us = elapsed.as_micros().max(1) as u64;
    bytes.saturating_mul(8).saturating_mul(1_000_000) / elapsed_us
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{atomic::AtomicU64, Arc},
    };

    use super::{
        certificate_fingerprint, ensure_crypto_provider, installed_signature_algorithms,
        loopback_bind_addr, monotonic_us, tailscale_bind_addr, validate_quic_bind_addr,
        AuthRequest, ClientRequest, EventEnvelope, FingerprintVerifier, HelloPayload, QuicClient,
        QuicError, QuicServer, RoomCredentials, ServerEvent, ServerResponse,
    };
    use crate::identity::DeviceIdentity;
    use crate::protocol::MAX_CONTROL_MESSAGE_BYTES;
    use rustls::pki_types::CertificateDer;
    use tokio::io::AsyncWriteExt;

    const TEST_DEVICE_ID: &str = "0198c3d0-7c55-7f82-9af2-36c9946b2974";

    fn test_identity() -> DeviceIdentity {
        DeviceIdentity::from_seed_for_tests(TEST_DEVICE_ID, [9; 32])
    }

    #[test]
    fn generated_room_credentials_match_protocol_shapes() {
        let first = RoomCredentials::generate();
        let second = RoomCredentials::generate();

        assert!(super::is_base64url_128bit(&first.room_id));
        assert!(super::is_base64url_256bit(&first.join_secret));
        assert_ne!(first.room_id, second.room_id);
        assert_ne!(first.join_secret, second.join_secret);
    }

    #[test]
    fn quic_bind_addresses_are_limited_to_loopback_or_tailscale() {
        assert!(validate_quic_bind_addr(loopback_bind_addr()).is_ok());
        assert_eq!(
            tailscale_bind_addr(Ipv4Addr::new(100, 64, 0, 10)).expect("tailscale bind"),
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(100, 64, 0, 10)),
                crate::network::tailscale::DEFAULT_TAILSCALE_PORT,
            )
        );
        assert!(matches!(
            validate_quic_bind_addr(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)),
            Err(QuicError::InvalidBindAddress)
        ));
        assert!(matches!(
            validate_quic_bind_addr(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
                0
            )),
            Err(QuicError::InvalidBindAddress)
        ));
    }

    #[tokio::test]
    async fn authenticates_heartbeat_and_measures_rtt() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));

        let (client, _) = QuicClient::connect(
            addr,
            fingerprint,
            credentials,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await
        .expect("client");

        client.heartbeat().await.expect("heartbeat");
        let rtt = client.rtt_probe().await.expect("rtt");
        assert!(rtt.rtt_us > 0);
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_wrong_join_secret() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));

        let mut wrong = credentials;
        wrong.join_secret = "wrong".to_string();
        let result = QuicClient::connect(
            addr,
            fingerprint,
            wrong,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await;

        assert!(result.is_err());
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_malformed_room_id() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let hello = HelloPayload::local("Test Guest".to_string(), &identity);
        let malformed_room_id = "not-base64url-room-id".to_string();
        let join_secret_hash = credentials.join_secret_hash();
        let invite_nonce = "nonce".to_string();
        let auth = AuthRequest {
            room_id: malformed_room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: invite_nonce.clone(),
            device_signature: identity.sign_auth_request(
                &malformed_room_id,
                &join_secret_hash,
                &invite_nonce,
            ),
        };

        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("auth response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "INVALID_ROOM_ID".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_malformed_join_secret_hash() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let hello = HelloPayload::local("Test Guest".to_string(), &identity);
        let malformed_hash = "short".to_string();
        let invite_nonce = "nonce".to_string();
        let auth = AuthRequest {
            room_id: credentials.room_id.clone(),
            join_secret_hash: malformed_hash.clone(),
            invite_nonce: invite_nonce.clone(),
            device_signature: identity.sign_auth_request(
                &credentials.room_id,
                &malformed_hash,
                &invite_nonce,
            ),
        };

        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("auth response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "INVALID_SECRET_HASH".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_malformed_public_key() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let mut hello = HelloPayload::local("Test Guest".to_string(), &identity);
        hello.public_key = "short".to_string();
        let join_secret_hash = credentials.join_secret_hash();
        let invite_nonce = "nonce".to_string();
        let auth = AuthRequest {
            room_id: credentials.room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: invite_nonce.clone(),
            device_signature: identity.sign_auth_request(
                &credentials.room_id,
                &join_secret_hash,
                &invite_nonce,
            ),
        };

        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("auth response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "INVALID_PUBLIC_KEY".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_tampered_device_signature() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let hello = HelloPayload::local("Test Guest".to_string(), &identity);
        let join_secret_hash = credentials.join_secret_hash();
        let auth = AuthRequest {
            room_id: credentials.room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: "nonce".to_string(),
            device_signature: identity.sign_auth_request(
                &credentials.room_id,
                &join_secret_hash,
                "different-nonce",
            ),
        };

        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("auth response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "INVALID_SIGNATURE".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_unsupported_hello_platform() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let mut hello = HelloPayload::local("Test Guest".to_string(), &identity);
        hello.platform = "linux".to_string();
        let join_secret_hash = credentials.join_secret_hash();
        let invite_nonce = "nonce".to_string();
        let auth = AuthRequest {
            room_id: credentials.room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: invite_nonce.clone(),
            device_signature: identity.sign_auth_request(
                &credentials.room_id,
                &join_secret_hash,
                &invite_nonce,
            ),
        };

        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("auth response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "UNSUPPORTED_PLATFORM".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_future_hello_minor_version() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let mut hello = HelloPayload::local("Test Guest".to_string(), &identity);
        hello.protocol_minor = crate::PROTOCOL_MINOR + 1;
        let join_secret_hash = credentials.join_secret_hash();
        let invite_nonce = "nonce".to_string();
        let auth = AuthRequest {
            room_id: credentials.room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: invite_nonce.clone(),
            device_signature: identity.sign_auth_request(
                &credentials.room_id,
                &join_secret_hash,
                &invite_nonce,
            ),
        };

        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("auth response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "UNSUPPORTED_MINOR_VERSION".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_non_uuid_v7_device_id() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = DeviceIdentity::from_seed_for_tests("not-a-uuid", [9; 32]);
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let hello = HelloPayload::local("Test Guest".to_string(), &identity);
        let join_secret_hash = credentials.join_secret_hash();
        let invite_nonce = "nonce".to_string();
        let auth = AuthRequest {
            room_id: credentials.room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: invite_nonce.clone(),
            device_signature: identity.sign_auth_request(
                &credentials.room_id,
                &join_secret_hash,
                &invite_nonce,
            ),
        };

        let response = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("auth response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "INVALID_DEVICE_ID".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_replayed_auth_nonce() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: credentials.clone(),
            identity: identity.clone(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };
        let hello = HelloPayload::local("Test Guest".to_string(), &identity);
        let join_secret_hash = credentials.join_secret_hash();
        let auth = AuthRequest {
            room_id: credentials.room_id.clone(),
            join_secret_hash: join_secret_hash.clone(),
            invite_nonce: "nonce".to_string(),
            device_signature: identity.sign_auth_request(
                &credentials.room_id,
                &join_secret_hash,
                "nonce",
            ),
        };

        let first = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello.clone()),
                auth: Box::new(auth.clone()),
            })
            .await
            .expect("first auth");
        let second = client
            .send_request(ClientRequest::HelloAuth {
                hello: Box::new(hello),
                auth: Box::new(auth),
            })
            .await
            .expect("replayed auth");

        assert!(matches!(first, ServerResponse::AuthAccept(_)));
        assert_eq!(
            second,
            ServerResponse::AuthReject {
                code: "REPLAYED_AUTH".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_requests_before_authentication() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials,
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let endpoint = super::make_client_endpoint(fingerprint).expect("endpoint");
        let connection = endpoint
            .connect(addr, "localhost")
            .expect("connect")
            .await
            .expect("connected");
        let client = QuicClient {
            endpoint,
            connection,
            credentials: RoomCredentials::new_for_tests(),
            identity: test_identity(),
            next_seq: Arc::new(AtomicU64::new(1)),
        };

        let response = client
            .send_request(ClientRequest::Heartbeat {
                seq: 1,
                sender: TEST_DEVICE_ID.to_string(),
                sent_mono_us: monotonic_us(),
                room_state: "LOBBY".to_string(),
                last_seen_peer_seq: 0,
            })
            .await
            .expect("response");

        assert_eq!(
            response,
            ServerResponse::AuthReject {
                code: "AUTH_REQUIRED".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_duplicate_or_stale_authenticated_sequence() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let (client, _) = QuicClient::connect(
            addr,
            fingerprint,
            credentials,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await
        .expect("client");

        let first = client
            .send_request(ClientRequest::Heartbeat {
                seq: 1,
                sender: TEST_DEVICE_ID.to_string(),
                sent_mono_us: monotonic_us(),
                room_state: "LOBBY".to_string(),
                last_seen_peer_seq: 0,
            })
            .await
            .expect("first");
        let duplicate = client
            .send_request(ClientRequest::Heartbeat {
                seq: 1,
                sender: TEST_DEVICE_ID.to_string(),
                sent_mono_us: monotonic_us(),
                room_state: "LOBBY".to_string(),
                last_seen_peer_seq: 0,
            })
            .await
            .expect("duplicate");

        assert!(matches!(first, ServerResponse::HeartbeatAck { .. }));
        assert_eq!(
            duplicate,
            ServerResponse::AuthReject {
                code: "INVALID_SEQUENCE".to_string()
            }
        );
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn transfers_synthetic_payload() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));

        let (client, _) = QuicClient::connect(
            addr,
            fingerprint,
            credentials,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await
        .expect("client");
        let result = client
            .upload_synthetic_payload(512 * 1024, 16 * 1024)
            .await
            .expect("throughput");

        assert_eq!(result.bytes, 512 * 1024);
        assert!(result.goodput_bps > 0);
        client.wait_idle().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_server_with_wrong_certificate_fingerprint() {
        let wrong_fingerprint = "mSncRHUcatB8mqTKA0jVJPmc0JaWJsm4u17SWO9M-q0".to_string();

        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let real_fingerprint = server.certificate_fingerprint();

        assert_ne!(wrong_fingerprint, real_fingerprint);
        assert!(super::is_base64url_256bit(&wrong_fingerprint));

        let server_task = tokio::spawn(server.run(None));

        let result = QuicClient::connect(
            addr,
            wrong_fingerprint,
            credentials,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await;

        assert!(
            result.is_err(),
            "wrong certificate fingerprint must cause TLS/QUIC connect to fail"
        );
        server_task.abort();
    }

    // ── F44: handshake signature verification ────────────────────────────
    //
    // Fingerprint pinning alone does not authenticate a peer: the certificate
    // DER is public (sent in the clear in every handshake, and its fingerprint
    // travels in the invite). TLS proves key possession through the
    // `CertificateVerify` signature. Both signature callbacks used to return
    // "valid" unconditionally, so a peer holding only the public certificate
    // could complete a handshake. These tests exercise that path for real.

    /// F44-A: a genuine server — correct certificate AND a real
    /// `CertificateVerify` signature — is still accepted end to end.
    #[tokio::test]
    async fn f44_a_valid_certificate_and_signature_are_accepted() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));

        let (client, _) = QuicClient::connect(
            addr,
            fingerprint,
            credentials,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await
        .expect("a valid certificate with a valid handshake signature must be accepted");

        // The authenticated session still works after signature verification.
        client.heartbeat().await.expect("heartbeat");
        client.wait_idle().await;
        server_task.abort();
    }

    /// F44-B: the pin is still enforced — a wrong fingerprint is rejected.
    ///
    /// Asserted directly against the verifier so the rejection is attributed
    /// to the fingerprint check and not to a signature failure.
    #[test]
    fn f44_b_wrong_certificate_fingerprint_is_rejected() {
        use rustls::client::danger::ServerCertVerifier;

        ensure_crypto_provider();
        let genuine = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("genuine certificate");
        let other = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("other certificate");
        let genuine_der = CertificateDer::from(genuine.cert);
        let other_der = CertificateDer::from(other.cert);

        let verifier = FingerprintVerifier {
            fingerprint: certificate_fingerprint(&genuine_der),
            signature_algorithms: installed_signature_algorithms(),
        };

        assert_ne!(
            certificate_fingerprint(&genuine_der),
            certificate_fingerprint(&other_der)
        );

        // The pinned certificate is accepted…
        verifier
            .verify_server_cert(
                &genuine_der,
                &[],
                &rustls::pki_types::ServerName::try_from("localhost").expect("server name"),
                &[],
                rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(
                    1_700_000_000,
                )),
            )
            .expect("the pinned certificate must be accepted");

        // …and any other certificate is rejected.
        let error = verifier
            .verify_server_cert(
                &other_der,
                &[],
                &rustls::pki_types::ServerName::try_from("localhost").expect("server name"),
                &[],
                rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(
                    1_700_000_000,
                )),
            )
            .expect_err("a different certificate must be rejected by the pin");
        assert!(
            error.to_string().contains("fingerprint mismatch"),
            "expected a fingerprint mismatch, got: {error}"
        );
    }

    /// F44-C: a correct certificate carrying a FORGED handshake signature is
    /// rejected.
    ///
    /// Drives `verify_tls13_signature` directly, and deliberately asserts both
    /// halves — the pinned key's own signature verifies, and a signature over
    /// the same transcript from a different key does not — so a blanket
    /// "always reject" implementation cannot pass this test. Under the old
    /// stubbed callback the forgery was accepted.
    ///
    /// Why not end to end: rustls refuses to build a server config whose
    /// private key does not match its certificate
    /// (`InconsistentKeys(KeyMismatch)`), so a real peer cannot emit a
    /// mismatched `CertificateVerify` for us to observe. That refusal is a
    /// useful second line of defence, but it is not the one this codebase
    /// relies on — the client verifier must reject the forgery itself.
    #[test]
    fn f44_c_correct_certificate_but_forged_handshake_signature_is_rejected() {
        use rcgen::SigningKey;
        use rustls::client::danger::ServerCertVerifier;
        use rustls::internal::msgs::codec::{Codec, Reader};

        ensure_crypto_provider();

        // The certificate whose fingerprint the client pins.
        let pinned = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("pinned certificate");
        let cert_der = CertificateDer::from(pinned.cert);
        let verifier = FingerprintVerifier {
            fingerprint: certificate_fingerprint(&cert_der),
            signature_algorithms: installed_signature_algorithms(),
        };

        let transcript = b"movie-party CertificateVerify transcript";
        // rcgen's default key is ECDSA P-256, so this is the scheme the
        // certificate's public key can actually verify.
        let scheme = rustls::SignatureScheme::ECDSA_NISTP256_SHA256;

        let signed = |signature: &[u8]| {
            let mut bytes = Vec::new();
            scheme.encode(&mut bytes);
            bytes.extend_from_slice(&(signature.len() as u16).to_be_bytes());
            bytes.extend_from_slice(signature);
            let mut reader = Reader::init(&bytes);
            rustls::DigitallySignedStruct::read(&mut reader).expect("decode dss")
        };

        // Control: the pinned key's own signature over this transcript verifies.
        let genuine = pinned
            .signing_key
            .sign(transcript)
            .expect("sign with the pinned key");
        verifier
            .verify_tls13_signature(transcript, &cert_der, &signed(&genuine))
            .expect("the pinned key's own signature must verify");

        // The forgery: same transcript, signed by a key we do not trust.
        let impostor = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("impostor key");
        let forged = impostor.signing_key.sign(transcript).expect("sign");
        assert_ne!(
            genuine, forged,
            "the two keys must produce different signatures"
        );

        let error = verifier
            .verify_tls13_signature(transcript, &cert_der, &signed(&forged))
            .expect_err("a signature made by another key must be rejected");
        assert!(
            !error.to_string().is_empty(),
            "the rejection must carry a reason"
        );
    }

    /// F44-D: malformed and wrong-signer signatures are rejected.
    ///
    /// Drives `verify_tls13_signature` directly, so the rejection is
    /// attributed to signature verification and nothing else. Under the old
    /// stubbed callback every case below returned "valid".
    #[test]
    fn f44_d_malformed_signature_is_rejected() {
        use rcgen::SigningKey;
        use rustls::client::danger::ServerCertVerifier;
        use rustls::internal::msgs::codec::{Codec, Reader};

        ensure_crypto_provider();
        let genuine = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("genuine certificate");
        let cert_der = CertificateDer::from(genuine.cert);
        let verifier = FingerprintVerifier {
            fingerprint: certificate_fingerprint(&cert_der),
            signature_algorithms: installed_signature_algorithms(),
        };

        let transcript = b"movie-party CertificateVerify transcript";
        let scheme = rustls::SignatureScheme::ECDSA_NISTP256_SHA256;

        // The public `DigitallySignedStruct` constructor is crate-private, so
        // build one from its wire encoding via the codec rustls exposes for
        // integration tests: scheme (u16) || signature length (u16) || bytes.
        let signed = |signature: &[u8]| {
            let mut bytes = Vec::new();
            scheme.encode(&mut bytes);
            bytes.extend_from_slice(&(signature.len() as u16).to_be_bytes());
            bytes.extend_from_slice(signature);
            let mut reader = Reader::init(&bytes);
            rustls::DigitallySignedStruct::read(&mut reader).expect("decode dss")
        };

        // A well-formed signature produced by a DIFFERENT key.
        let impostor = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("impostor key");
        let forged = impostor.signing_key.sign(transcript).expect("sign");
        assert!(!forged.is_empty());
        assert!(
            verifier
                .verify_tls13_signature(transcript, &cert_der, &signed(&forged))
                .is_err(),
            "a signature made by another key must not verify against this certificate"
        );

        // Truncated / empty signature bytes.
        for malformed in [vec![], vec![0u8; 1], vec![0u8; 8]] {
            assert!(
                verifier
                    .verify_tls13_signature(transcript, &cert_der, &signed(&malformed))
                    .is_err(),
                "a malformed signature ({} bytes) must be rejected",
                malformed.len()
            );
        }

        // Garbage of a plausible length.
        assert!(
            verifier
                .verify_tls13_signature(transcript, &cert_der, &signed(&[0xAB_u8; 64]))
                .is_err(),
            "garbage signature bytes must be rejected"
        );
    }

    // ── PROTOCOL_SPEC §67: malformed-input property tests ─────────

    /// §67 + §5: a control frame declaring an oversized length prefix must
    /// be rejected with MP-PROTO-004 before any allocation or parse, and
    /// must never be parsed as a valid request.
    #[tokio::test]
    async fn rejects_oversized_control_frame_length_prefix() {
        // In-memory duplex stream pair emulating a QUIC stream pair.
        let (mut client_send, mut server_recv) = tokio::io::duplex(64);

        // Advertise a 257 KiB frame: one byte over the §5 limit.
        let oversized = MAX_CONTROL_MESSAGE_BYTES + 1;
        let handle = tokio::spawn(async move {
            client_send
                .write_all(&(oversized as u32).to_be_bytes())
                .await
                .expect("write len");
        });

        let result = super::read_json_bytes(&mut server_recv).await;
        handle.await.expect("writer");
        assert!(
            matches!(result, Err(QuicError::MessageTooLarge { len }) if len == oversized),
            "oversized length prefix must yield MP-PROTO-004, got {result:?}"
        );
    }

    /// §5 boundary: a frame exactly at the limit is accepted by the length
    /// gate (its content may still fail JSON parsing, which is correct).
    #[tokio::test]
    async fn accepts_control_frame_at_exact_limit() {
        let (mut client_send, mut server_recv) = tokio::io::duplex(MAX_CONTROL_MESSAGE_BYTES + 16);

        let payload = vec![b'{'; MAX_CONTROL_MESSAGE_BYTES];
        let handle = tokio::spawn(async move {
            client_send
                .write_all(&(payload.len() as u32).to_be_bytes())
                .await
                .expect("write len");
            client_send.write_all(&payload).await.expect("write body");
        });

        let bytes = super::read_json_bytes(&mut server_recv)
            .await
            .expect("frame");
        handle.await.expect("writer");
        assert_eq!(bytes.len(), MAX_CONTROL_MESSAGE_BYTES);
        // The frame is at the size limit but is not valid JSON → parse error,
        // not a size error, and never a crash (§67).
        let parsed: Result<ClientRequest, _> = serde_json::from_slice(&bytes);
        assert!(parsed.is_err());
    }

    /// §67: unknown message type tags must fail deserialization rather than
    /// being silently executed.
    #[tokio::test]
    async fn rejects_unknown_request_type_tag() {
        let unknown = br#"{"type":"TotallyUnknownType","payload":{"seq":1,"sender":"x"}}"#;
        let parsed: Result<ClientRequest, _> = serde_json::from_slice(unknown);
        assert!(
            parsed.is_err(),
            "unknown type tag must be rejected by the parser"
        );

        // Same property over the wire framing path.
        let (mut client_send, mut server_recv) = tokio::io::duplex(128);
        let frame = unknown.to_vec();
        let handle = tokio::spawn(async move {
            client_send
                .write_all(&(frame.len() as u32).to_be_bytes())
                .await
                .expect("write len");
            client_send.write_all(&frame).await.expect("write body");
        });

        let result = super::read_request(&mut server_recv).await;
        handle.await.expect("writer");
        assert!(result.is_err(), "unknown type must be rejected on the wire");
    }

    /// §67: missing required fields must fail deserialization.
    #[test]
    fn rejects_requests_with_missing_required_fields() {
        // HelloAuth without the `hello` field.
        let missing_hello = br#"{"type":"HelloAuth","payload":{"auth":{}}}"#;
        assert!(serde_json::from_slice::<ClientRequest>(missing_hello).is_err());

        // Heartbeat without `seq`.
        let missing_seq = br#"{"type":"Heartbeat","payload":{"sender":"x"}}"#;
        assert!(serde_json::from_slice::<ClientRequest>(missing_seq).is_err());

        // CallSignal without `signal_type`.
        let missing_type = br#"{"type":"CallSignal","payload":{"seq":1,"sender":"x","data":""}}"#;
        assert!(serde_json::from_slice::<ClientRequest>(missing_type).is_err());

        // EventEnvelope without the new §10 version/room fields.
        let legacy_envelope = br#"{"seq":1,"sender":"x","sent_mono_us":1,"event":{"type":"RoomStateUpdate","payload":{"state":"LOBBY","position_ms":0}}}"#;
        assert!(serde_json::from_slice::<EventEnvelope>(legacy_envelope).is_err());
    }

    /// §67: unsigned-required fields reject negative values.
    #[test]
    fn rejects_negative_values_where_unsigned_required() {
        let negative_seq = br#"{"type":"Heartbeat","payload":{"seq":-1,"sender":"x","room_state":"LOBBY","last_seen_peer_seq":0}}"#;
        assert!(serde_json::from_slice::<ClientRequest>(negative_seq).is_err());
    }

    /// §67 + §64: syntactically valid frames with a zero sequence, a duplicate,
    /// or a sequence behind the reorder window must be rejected by the
    /// authenticated-sequence gate — while a legitimate out-of-order sequence
    /// (the cross-stream case) is accepted.
    #[tokio::test]
    async fn sequence_gate_rejects_zero_duplicates_and_out_of_window_replays() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(None));
        let (client, _) = QuicClient::connect(
            addr,
            fingerprint,
            credentials,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await
        .expect("client");

        let ready = |client: &QuicClient, seq: u64| ClientRequest::ReadyState {
            seq,
            sender: client.identity.device_id.clone(),
            ready: true,
            buffer_ahead_ms: 1_000,
        };

        // Sequence zero is never valid (§64: starts at 1).
        let zero = client
            .send_control(ready(&client, 0))
            .await
            .expect("response for seq=0");
        assert!(
            matches!(zero, ServerResponse::AuthReject { code } if code == "INVALID_SEQUENCE"),
            "seq=0 must be rejected as INVALID_SEQUENCE"
        );

        // A high seq then a *far* lower seq: the lower one is behind the
        // reorder window, so it stays rejected exactly as before.
        let high = 1 + crate::protocol::SEQUENCE_REORDER_WINDOW + 10;
        let accepted = client
            .send_control(ready(&client, high))
            .await
            .expect("response for the high seq");
        assert!(
            matches!(accepted, ServerResponse::ReadyAck { .. }),
            "a fresh high sequence must be accepted"
        );
        let stale = client
            .send_control(ready(&client, 1))
            .await
            .expect("response for the stale seq");
        assert!(
            matches!(stale, ServerResponse::AuthReject { code } if code == "INVALID_SEQUENCE"),
            "a sequence behind the reorder window must be rejected as INVALID_SEQUENCE"
        );

        // A duplicate is still a replay, however close to the mark it sits.
        let duplicate = client
            .send_control(ready(&client, high))
            .await
            .expect("response for the duplicate seq");
        assert!(
            matches!(duplicate, ServerResponse::AuthReject { code } if code == "INVALID_SEQUENCE"),
            "a replayed sequence must be rejected as INVALID_SEQUENCE"
        );

        client.wait_idle().await;
        server_task.abort();
    }

    /// §64 regression for the Windows `test_b` stall.
    ///
    /// The guest's `ReadyState` and `BufferStatus` are independent sends, each
    /// on its own QUIC bidirectional stream, and `ready_both` fires them
    /// concurrently. Whichever stream the host's accept loop reaches first gets
    /// processed first, so the higher sequence regularly arrives first. The
    /// strict watermark used to reject the loser as `INVALID_SEQUENCE`, the
    /// guest ignored the rejection (`let _ =`), nothing retried, and the host
    /// never recorded guest readiness — the room sat outside `READYCHECK`
    /// until the 30 s poll deadline.
    ///
    /// This test drives the *hostile* order deterministically (higher sequence
    /// first) and also exercises genuine concurrency, asserting both that no
    /// message is rejected and that the host coordinator actually records the
    /// guest's readiness.
    #[tokio::test]
    async fn ready_state_and_buffer_status_survive_cross_stream_reordering() {
        use crate::sync::local::LocalSyncCoordinator;

        let credentials = RoomCredentials::new_for_tests();
        let coordinator = Arc::new(std::sync::Mutex::new(LocalSyncCoordinator::new()));
        let server = QuicServer::bind(
            loopback_bind_addr(),
            credentials.clone(),
            "Host".to_string(),
            TEST_DEVICE_ID.to_string(),
        )
        .expect("server");
        let addr = server.local_addr().expect("addr");
        let fingerprint = server.certificate_fingerprint();
        let server_task = tokio::spawn(server.run(Some(coordinator.clone())));
        let (client, _) = QuicClient::connect(
            addr,
            fingerprint,
            credentials,
            test_identity(),
            "Test Guest".to_string(),
        )
        .await
        .expect("client");

        // 1. Genuine concurrency, exactly the production shape: two control
        //    messages written back-to-back on independent bidirectional
        //    streams, in the client's natural sequence range. QUIC promises
        //    nothing about the order *between* streams, so the server may read
        //    them either way round — and neither may be rejected.
        //
        //    This part runs FIRST so the client's own counter still starts at
        //    1: `send_control` with an explicit `seq` does not advance it, so
        //    hand-written sequences below would collide with these.
        let (ready_result, buffer_result) = tokio::join!(
            client.send_ready_state(true, 5_000),
            client.send_buffer_status(0, 8_000, false),
        );
        assert!(
            matches!(ready_result, Ok(ServerResponse::ReadyAck { .. })),
            "concurrent ReadyState must never be rejected, got {ready_result:?}"
        );
        assert!(
            matches!(buffer_result, Ok(ServerResponse::BufferAck { .. })),
            "concurrent BufferStatus must never be rejected, got {buffer_result:?}"
        );

        // The host coordinator must have genuinely recorded guest readiness —
        // that is the state `READYCHECK` consensus depends on. Without it the
        // room sits outside READYCHECK forever, which is the reported Windows
        // CI failure.
        let guest_ready = coordinator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .guest_ready;
        assert!(
            guest_ready.player_ready,
            "host coordinator must record the guest as ready after ReadyState"
        );

        // 2. Hostile but deterministic: the higher sequence is delivered and
        //    processed first, then the lower one arrives. High explicit values
        //    keep this range disjoint from the client's own counter above, so
        //    the pair cannot collide with anything already accepted.
        let sender = client.identity.device_id.clone();
        let later = client
            .send_control(ClientRequest::ReadyState {
                seq: 100,
                sender: sender.clone(),
                ready: true,
                buffer_ahead_ms: 5_000,
            })
            .await
            .expect("ReadyState seq=100 response");
        assert!(
            matches!(later, ServerResponse::ReadyAck { ready: true }),
            "the later sequence must be accepted first"
        );

        let earlier = client
            .send_control(ClientRequest::BufferStatus {
                seq: 99,
                sender: sender.clone(),
                position_ms: 0,
                buffer_ahead_ms: 8_000,
                stalled: false,
            })
            .await
            .expect("BufferStatus seq=99 response");
        assert!(
            matches!(earlier, ServerResponse::BufferAck { accepted: true }),
            "the earlier sequence must not be dropped as stale"
        );

        client.wait_idle().await;
        server_task.abort();
    }

    /// §10: envelopes with a wrong version or a missing room are rejected
    /// by the receive-side validation before reaching runtime state.
    #[test]
    fn rejects_envelopes_with_wrong_version_or_missing_room() {
        let base = EventEnvelope {
            v_major: 1,
            v_minor: 0,
            room_id: "room".to_string(),
            seq: 1,
            sender: "host".to_string(),
            sent_mono_us: 1,
            event: ServerEvent::RoomStateUpdate {
                state: "LOBBY".to_string(),
                position_ms: 0,
            },
        };

        // Wrong major.
        let wrong_major = EventEnvelope {
            v_major: 2,
            ..base.clone()
        };
        assert!(matches!(
            super::validate_event_envelope(&wrong_major),
            Err(QuicError::EnvelopeVersion { .. })
        ));

        // Newer minor.
        let newer_minor = EventEnvelope {
            v_minor: 1,
            ..base.clone()
        };
        assert!(matches!(
            super::validate_event_envelope(&newer_minor),
            Err(QuicError::EnvelopeVersion { .. })
        ));

        // Missing room.
        let no_room = EventEnvelope {
            room_id: String::new(),
            ..base.clone()
        };
        assert!(matches!(
            super::validate_event_envelope(&no_room),
            Err(QuicError::EnvelopeRoom { .. })
        ));

        // The well-formed envelope passes.
        assert!(super::validate_event_envelope(&base).is_ok());
    }

    /// (§55–§57): every scheduling event round-trips through the
    /// JSON + u32-BE wire format with its payload intact. This proves the
    /// serde tag/content contract the frontend relies on.
    #[test]
    fn scheduling_events_round_trip_through_serde() {
        let events = vec![
            (
                ServerEvent::ScheduleCreate {
                    schedule_id: "sched-1".to_string(),
                    scheduled_start_utc_ms: 1_786_811_400_000,
                    media_id: "media-1".to_string(),
                    call_mode: "VIDEO_VOICE".to_string(),
                    planned_preload_utc_ms: 1_786_800_600_000,
                },
                "ScheduleCreate",
            ),
            (
                ServerEvent::ScheduleAccept {
                    schedule_id: "sched-1".to_string(),
                    accepted: true,
                },
                "ScheduleAccept",
            ),
            (
                ServerEvent::ScheduleUpdate {
                    schedule_id: "sched-1".to_string(),
                    media_id: "media-2".to_string(),
                    planned_preload_utc_ms: 1_786_800_600_000,
                    scheduled_start_utc_ms: 1_786_811_400_000,
                },
                "ScheduleUpdate",
            ),
            (
                ServerEvent::ScheduleCancel {
                    schedule_id: "sched-1".to_string(),
                },
                "ScheduleCancel",
            ),
            (
                ServerEvent::PreloadState {
                    schedule_id: "sched-1".to_string(),
                    state: "TRANSFERRING".to_string(),
                    progress: 0.43,
                    estimated_ready_utc_ms: 1_786_807_112_345,
                },
                "PreloadState",
            ),
        ];
        for (event, tag) in events {
            let json = serde_json::to_vec(&event).expect("serialize");
            let value: serde_json::Value = serde_json::from_slice(&json).expect("value");
            assert_eq!(value["type"], tag, "wire tag for {tag}");
            let parsed: ServerEvent = serde_json::from_slice(&json).expect("deserialize");
            assert_eq!(parsed, event, "round trip for {tag}");
        }
    }

    /// §57: the progress value is clamped on the wire so a peer can never
    /// imply >100 % preload.
    #[test]
    fn preload_state_progress_is_bounded_on_the_wire() {
        let event = ServerEvent::PreloadState {
            schedule_id: "s".to_string(),
            state: "TRANSFERRING".to_string(),
            progress: 0.43,
            estimated_ready_utc_ms: 0,
        };
        let json = serde_json::to_vec(&event).expect("serialize");
        let parsed: ServerEvent = serde_json::from_slice(&json).expect("deserialize");
        let ServerEvent::PreloadState { progress, .. } = parsed else {
            panic!("expected PreloadState");
        };
        assert!((0.0..=1.0).contains(&progress));
    }

    /// §10 round trip: an envelope built by the host broadcast path
    /// survives the guest receive path unchanged.
    #[test]
    fn event_envelope_round_trips_through_serde() {
        let envelope = EventEnvelope {
            v_major: 1,
            v_minor: 0,
            room_id: "EjRWeJCrze8BI0VniavN7w".to_string(),
            seq: 7,
            sender: TEST_DEVICE_ID.to_string(),
            sent_mono_us: 12_345,
            event: ServerEvent::CallSignal {
                signal_type: "OFFER".to_string(),
                data: "{\"type\":\"offer\",\"sdp\":\"v=0\\r\\n\"}".to_string(),
            },
        };

        let json = serde_json::to_vec(&envelope).expect("serialize");
        let parsed: EventEnvelope = serde_json::from_slice(&json).expect("deserialize");
        assert_eq!(parsed, envelope);
        assert!(super::validate_event_envelope(&parsed).is_ok());

        // A legacy peer that predates §10 (no version/room fields) cannot
        // be deserialized: the incompatibility surfaces at the parse layer,
        // never mid-application (§68 → major bump documented in ADR-0001).
        let stripped: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&envelope).expect("json")).expect("value");
        let mut legacy = stripped;
        let obj = legacy.as_object_mut().expect("object");
        obj.remove("v_major");
        obj.remove("v_minor");
        obj.remove("room_id");
        let legacy_bytes = serde_json::to_vec(&legacy).expect("legacy bytes");
        assert!(serde_json::from_slice::<EventEnvelope>(&legacy_bytes).is_err());
    }

    /// §52 + §5 property: a call signal larger than 64 KiB is accepted by
    /// the transport (it is under 256 KiB) but must be rejected by the call
    /// subsystem's own payload validation (MP-CALL-001, tightened
    /// further). Here we assert the transport-level boundary so the two
    /// limits stay correctly layered.
    #[tokio::test]
    async fn transport_allows_signal_under_control_limit() {
        let (mut client_send, mut server_recv) = tokio::io::duplex(128 * 1024);

        // 100 KiB signal body — over §52's 64 KiB, under §5's 256 KiB.
        let big = "x".repeat(100 * 1024);
        let request = ClientRequest::CallSignal {
            seq: 1,
            sender: "device".to_string(),
            signal_type: "ICE".to_string(),
            data: big,
        };
        let frame = serde_json::to_vec(&request).expect("json");
        let handle = tokio::spawn(async move {
            client_send
                .write_all(&(frame.len() as u32).to_be_bytes())
                .await
                .expect("write len");
            client_send.write_all(&frame).await.expect("write body");
        });

        let parsed = super::read_request(&mut server_recv).await;
        handle.await.expect("writer");
        // Transport layer: frame accepted (under 256 KiB).
        assert!(parsed.is_ok());
    }
}
