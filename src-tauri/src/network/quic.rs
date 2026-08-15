use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
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
use rustls::{
    pki_types::{CertificateDer, PrivatePkcs8KeyDer},
    RootCertStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::identity::{verify_auth_request_signature, DeviceIdentity};
use crate::protocol::SequenceTracker;

pub const QUIC_TRANSPORT_NAME: &str = "quic";
pub const MOVE_PARTY_ALPN: &[&[u8]] = &[b"moveparty-v1"];
const MAX_REQUEST_BYTES: usize = 256 * 1024;
const MAX_AUTH_NONCES: usize = 4_096;

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
}

pub struct QuicServer {
    endpoint: Endpoint,
    certificate: CertificateDer<'static>,
    credentials: RoomCredentials,
    replay_guard: AuthReplayGuard,
}

impl QuicServer {
    pub fn bind(bind_addr: SocketAddr, credentials: RoomCredentials) -> Result<Self, QuicError> {
        validate_quic_bind_addr(bind_addr)?;
        let (server_config, certificate) = configure_server()?;
        let endpoint = Endpoint::server(server_config, bind_addr)?;

        Ok(Self {
            endpoint,
            certificate,
            credentials,
            replay_guard: AuthReplayGuard::default(),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, QuicError> {
        Ok(self.endpoint.local_addr()?)
    }

    pub fn certificate_fingerprint(&self) -> String {
        certificate_fingerprint(&self.certificate)
    }

    pub async fn run(self) -> Result<(), QuicError> {
        while let Some(incoming) = self.endpoint.accept().await {
            let credentials = self.credentials.clone();
            let replay_guard = self.replay_guard.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        let _ = handle_connection(connection, credentials, replay_guard).await;
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

#[derive(Clone)]
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
        server_certificate: CertificateDer<'static>,
        credentials: RoomCredentials,
        identity: DeviceIdentity,
        display_name: impl Into<String>,
    ) -> Result<Self, QuicError> {
        let endpoint = make_client_endpoint(server_certificate)?;
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
            ServerResponse::AuthAccept(_) => Ok(client),
            ServerResponse::AuthReject { code } => Err(QuicError::Auth(code)),
            _ => Err(QuicError::UnexpectedResponse),
        }
    }

    pub async fn heartbeat(&self) -> Result<(), QuicError> {
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
            ServerResponse::HeartbeatAck { .. } => Ok(()),
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
    let octets = ipv4.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

pub fn certificate_fingerprint(certificate: &CertificateDer<'static>) -> String {
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
    crypto.alpn_protocols = MOVE_PARTY_ALPN.iter().map(|value| value.to_vec()).collect();

    let crypto =
        QuicServerConfig::try_from(crypto).map_err(|error| QuicError::Tls(error.to_string()))?;
    Ok((ServerConfig::with_crypto(Arc::new(crypto)), cert_der))
}

fn make_client_endpoint(
    server_certificate: CertificateDer<'static>,
) -> Result<Endpoint, QuicError> {
    ensure_crypto_provider();
    let mut roots = RootCertStore::empty();
    roots
        .add(server_certificate)
        .map_err(|error| QuicError::Tls(error.to_string()))?;

    let mut crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = MOVE_PARTY_ALPN.iter().map(|value| value.to_vec()).collect();

    let crypto =
        QuicClientConfig::try_from(crypto).map_err(|error| QuicError::Tls(error.to_string()))?;
    let client_config = ClientConfig::new(Arc::new(crypto));
    let mut endpoint = Endpoint::client(loopback_bind_addr())?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

fn ensure_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

async fn handle_connection(
    connection: Connection,
    credentials: RoomCredentials,
    replay_guard: AuthReplayGuard,
) -> Result<(), QuicError> {
    let session = Arc::new(Mutex::new(ConnectionSession::default()));
    loop {
        let stream = connection.accept_bi().await;
        let stream = match stream {
            Ok(stream) => stream,
            Err(quinn::ConnectionError::ApplicationClosed { .. }) => return Ok(()),
            Err(error) => return Err(QuicError::Connection(error)),
        };

        let credentials = credentials.clone();
        let replay_guard = replay_guard.clone();
        let session = session.clone();
        tokio::spawn(async move {
            let _ = handle_request(stream, credentials, replay_guard, session).await;
        });
    }
}

#[derive(Debug, Default)]
struct ConnectionSession {
    authenticated_device_id: Option<String>,
    sequence_tracker: SequenceTracker,
}

async fn handle_request(
    (mut send, mut recv): (quinn::SendStream, quinn::RecvStream),
    credentials: RoomCredentials,
    replay_guard: AuthReplayGuard,
    session: Arc<Mutex<ConnectionSession>>,
) -> Result<(), QuicError> {
    let request = read_request(&mut recv).await?;
    let response = match request {
        ClientRequest::HelloAuth { hello, auth } => {
            let device_id = hello.device_id.clone();
            let response = validate_handshake(&credentials, &replay_guard, *hello, *auth);
            if matches!(response, ServerResponse::AuthAccept(_)) {
                let mut session = lock_session(&session);
                session.authenticated_device_id = Some(device_id);
            }
            response
        }
        ClientRequest::Heartbeat {
            seq,
            sender,
            sent_mono_us: _,
            ..
        } => match validate_authenticated_sequence(&session, &sender, seq) {
            Ok(()) => ServerResponse::HeartbeatAck {
                room_state: "LOBBY".to_string(),
            },
            Err(code) => ServerResponse::AuthReject { code },
        },
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
    })
}

fn is_uuid_v7(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|uuid| uuid.get_version_num() == 7)
}

fn is_base64url_128bit(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == 16)
}

fn is_base64url_256bit(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == 32)
}

async fn write_request(
    send: &mut quinn::SendStream,
    request: &ClientRequest,
) -> Result<(), QuicError> {
    write_json(send, request).await
}

async fn write_response(
    send: &mut quinn::SendStream,
    response: &ServerResponse,
) -> Result<(), QuicError> {
    write_json(send, response).await
}

async fn write_json<T: Serialize>(
    send: &mut quinn::SendStream,
    value: &T,
) -> Result<(), QuicError> {
    let json = serde_json::to_vec(value)?;
    let len = u32::try_from(json.len()).map_err(|_| QuicError::UnexpectedResponse)?;
    send.write_all(&len.to_be_bytes()).await?;
    send.write_all(&json).await?;
    Ok(())
}

async fn read_request(recv: &mut quinn::RecvStream) -> Result<ClientRequest, QuicError> {
    let bytes = read_json_bytes(recv).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn read_response(recv: &mut quinn::RecvStream) -> Result<ServerResponse, QuicError> {
    let bytes = read_json_bytes(recv).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn read_json_bytes(recv: &mut quinn::RecvStream) -> Result<Vec<u8>, QuicError> {
    let mut len = [0_u8; 4];
    recv.read_exact(&mut len)
        .await
        .map_err(|error| QuicError::ReadExact(error.to_string()))?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_REQUEST_BYTES {
        return Err(QuicError::UnexpectedResponse);
    }
    let mut bytes = vec![0; len];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|error| QuicError::ReadExact(error.to_string()))?;
    Ok(bytes)
}

fn monotonic_us() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_micros() as u64
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
        loopback_bind_addr, monotonic_us, tailscale_bind_addr, validate_quic_bind_addr,
        AuthRequest, ClientRequest, HelloPayload, QuicClient, QuicError, QuicServer,
        RoomCredentials, ServerResponse,
    };
    use crate::identity::DeviceIdentity;

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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());

        let client = QuicClient::connect(
            addr,
            certificate,
            credentials,
            test_identity(),
            "Test Guest",
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());

        let mut wrong = credentials;
        wrong.join_secret = "wrong".to_string();
        let result =
            QuicClient::connect(addr, certificate, wrong, test_identity(), "Test Guest").await;

        assert!(result.is_err());
        server_task.abort();
    }

    #[tokio::test]
    async fn rejects_malformed_room_id() {
        let credentials = RoomCredentials::new_for_tests();
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let mut hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let mut hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let mut hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = DeviceIdentity::from_seed_for_tests("not-a-uuid", [9; 32]);
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let identity = test_identity();
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let hello = HelloPayload::local("Test Guest", &identity);
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let endpoint = super::make_client_endpoint(certificate).expect("endpoint");
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());
        let client = QuicClient::connect(
            addr,
            certificate,
            credentials,
            test_identity(),
            "Test Guest",
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
        let server = QuicServer::bind(loopback_bind_addr(), credentials.clone()).expect("server");
        let addr = server.local_addr().expect("addr");
        let certificate = server.certificate.clone();
        let server_task = tokio::spawn(server.run());

        let client = QuicClient::connect(
            addr,
            certificate,
            credentials,
            test_identity(),
            "Test Guest",
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
}
