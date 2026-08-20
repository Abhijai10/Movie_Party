// ─── M3 Closure tests ──────────────────────────────────────────────────────────
// Demand-driven range fetch, binary QUIC bulk transfer over real loopback
// QUIC, lifecycle ownership/cleanup, and strict-sync authority proofs.
// ──────────────────────────────────────────────────────────────────────────────

use std::sync::{Arc, OnceLock};

use tokio::sync::Mutex;

use move_party_lib::app_runtime::AppRuntime;
use move_party_lib::identity::DeviceIdentity;
use move_party_lib::media::cache::SparseCache;
use move_party_lib::media::stream::range_server::{start_range_server, RangeServerConfig};
use move_party_lib::media::transfer::{validate_chunk_packet, ChunkDemandHandle, ChunkPriority};
use move_party_lib::network::quic::QuicClient;
use move_party_lib::room::{
    invite_socket_addr, invite_to_credentials, parse_invite, MovePartyInvite,
};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

fn temp_media(size: usize) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("m3c_{}.mkv", uuid::Uuid::now_v7()));
    let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &data).expect("write fixture");
    path
}

fn invite_details(invite_code: &str) -> (MovePartyInvite, std::net::SocketAddr) {
    let invite = parse_invite(invite_code).expect("parse invite");
    let addr = invite_socket_addr(&invite).expect("addr");
    (invite, addr)
}

fn http_get_range(addr: std::net::SocketAddr, path: &str, range: &str) -> (String, Vec<u8>) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .expect("rt");
    let req = format!(
        "GET {path} HTTP/1.1\r\nRange: {range}\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes()).expect("write");
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).expect("read");

    // Split headers from body at the first CRLFCRLF. The body is then sliced
    // by the declared Content-Length so binary payloads containing CRLFCRLF
    // sequences do not confuse the header boundary.
    let header_end = find_http_header_end(&raw).expect("header terminator");
    let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let content_length = head
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            (k.trim().eq_ignore_ascii_case("content-length"))
                .then(|| v.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let body = raw[header_end..]
        .iter()
        .copied()
        .take(content_length)
        .collect::<Vec<u8>>();
    (head, body)
}

fn find_http_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

// ──────────────────────────────────────────────────────────────────────────────
// M3.1 — an uncached HTTP range request must cause an actual QUIC chunk fetch
// through the demand channel, and the requested bytes must then be served.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uncached_http_range_triggers_real_quic_fetch_and_serves() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");
    let path = temp_media(2_500_000);
    let source = std::fs::read(&path).expect("read source");

    let host = AppRuntime::new();
    let host_snap = host
        .create_local_party(Some(path.to_str().unwrap().to_string()))
        .await
        .expect("host");
    let invite_code = host_snap.room.invite_code.clone().expect("invite");
    let (invite, addr) = invite_details(&invite_code);
    let credentials = invite_to_credentials(&invite);
    let guest_identity = DeviceIdentity::new_ephemeral();
    let (client, _auth) = QuicClient::connect(
        addr,
        invite.server_certificate_fingerprint.clone(),
        credentials,
        guest_identity,
        "M3 Guest".to_string(),
    )
    .await
    .expect("connect");

    let manifest = client.fetch_local_media_manifest().await.expect("manifest");
    assert_eq!(
        manifest.media_id,
        host_snap.media.as_ref().unwrap().media_id
    );

    let cache_root = std::env::temp_dir().join(format!("m3c_cache_{}", uuid::Uuid::now_v7()));
    let cache = SparseCache::open(&cache_root, manifest.clone()).expect("cache");
    let cache_arc = Arc::new(Mutex::new(cache));
    let demand = ChunkDemandHandle::new();
    let (wake_tx, wake_rx) = tokio::sync::watch::channel(0u64);
    let _wake_tx = Arc::new(wake_tx);

    let token = "m3c-token";
    let range = start_range_server(RangeServerConfig {
        manifest: manifest.clone(),
        cache: cache_arc.clone(),
        session_token: token.to_string(),
        chunk_wake_rx: wake_rx,
        demand: demand.clone(),
        wait_timeout_ms: 5_000,
    })
    .await
    .expect("range server");

    // Spawn the production-shaped demand-driven transfer worker.
    let worker_demand = demand.clone();
    let worker_client = client.clone();
    let worker_cache = cache_arc.clone();
    let worker_manifest = manifest.clone();
    let worker = tokio::spawn(async move {
        loop {
            let Some(request) = worker_demand.pop_next() else {
                worker_demand.notified().await;
                continue;
            };
            match worker_client
                .fetch_local_media_chunk(&worker_manifest.media_id, request.index)
                .await
            {
                Ok(packet) => {
                    if validate_chunk_packet(&worker_manifest, &packet).is_ok() {
                        let mut c = worker_cache.lock().await;
                        let _ = c.write_chunk(u64::from(packet.chunk_index), &packet.payload);
                    }
                    worker_demand.finish_fetch(request.index);
                }
                Err(_) => {
                    worker_demand.finish_fetch(request.index);
                    break;
                }
            }
        }
    });

    // Wait for the server to accept connections, then request a range that
    // covers the first full chunk — entirely uncached at this point.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let first_chunk_len = manifest.chunk_size.min(manifest.file_size);
    let (head, body) = http_get_range(
        range.addr,
        &format!("/media/{}?token={}", manifest.media_id, token),
        &format!("bytes=0-{}", first_chunk_len - 1),
    );
    assert!(
        head.contains("206 Partial Content"),
        "expected partial content: {head}"
    );
    assert_eq!(body.len() as u64, first_chunk_len, "served full chunk");
    let expected = &source[..first_chunk_len as usize];
    assert_eq!(body, expected, "served bytes must match the source file");

    worker.abort();
    range.shutdown();
    client.wait_idle().await;
    host.leave_party();
    let _ = std::fs::remove_dir_all(&cache_root);
    let _ = std::fs::remove_file(&path);
    std::env::remove_var("MOVE_PARTY_DEV_LOOPBACK");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distant_seek_repioritizes_demand_before_background() {
    let _guard = env_lock().lock().await;
    let demand = ChunkDemandHandle::new();

    // Background prefetch enqueues the whole early window first.
    for index in 0..10 {
        demand.request(index, ChunkPriority::Background);
    }
    // A seek to a distant region arrives afterwards.
    demand.request(240, ChunkPriority::Critical);

    let first = demand.pop_next().expect("first");
    assert_eq!(
        first.index, 240,
        "critical distant chunk must outrank background"
    );
    assert_eq!(first.priority, ChunkPriority::Critical);
    let second = demand.pop_next().expect("second");
    assert_eq!(second.index, 0, "background then proceeds in order");
}

#[test]
fn duplicate_range_demands_deduplicate() {
    let demand = ChunkDemandHandle::new();
    demand.request(12, ChunkPriority::Critical);
    demand.request(12, ChunkPriority::Critical);
    demand.request(12, ChunkPriority::Background);
    let first = demand.pop_next().expect("pop");
    assert_eq!(first.index, 12);
    assert!(
        demand.is_empty(),
        "duplicate demands must collapse to one fetch"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// M3.3 — Local Perfect session ownership and lifecycle cleanup.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn guest_fetch_media_owns_session_and_leave_releases_everything() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");
    let path = temp_media(1_200_000);

    let host = AppRuntime::new();
    let host_snap = host
        .create_local_party(Some(path.to_str().unwrap().to_string()))
        .await
        .expect("host");
    let invite_code = host_snap.room.invite_code.clone().expect("invite");
    let (invite, addr) = invite_details(&invite_code);
    let credentials = invite_to_credentials(&invite);
    let guest_identity = DeviceIdentity::new_ephemeral();
    let (client, _auth) = QuicClient::connect(
        addr,
        invite.server_certificate_fingerprint.clone(),
        credentials,
        guest_identity,
        "M3 Lifecycle Guest".to_string(),
    )
    .await
    .expect("connect");

    let guest = AppRuntime::new();
    // Install the live client so guest_fetch_media has a transport.
    guest.inject_client_for_test(client);
    let fetch = guest.guest_fetch_media().await.expect("guest fetch");
    assert!(
        fetch.media.is_some(),
        "manifest must be owned by the session"
    );

    // The session owns: manifest, cache, range server, transfer worker, player.
    let owned = guest.debug_session_owned();
    assert!(owned.cache_owned, "guest must own the sparse cache");
    assert!(owned.range_server_owned, "guest must own the range server");
    assert!(
        owned.transfer_worker_owned,
        "guest must own the transfer worker"
    );
    assert!(owned.player_owned, "guest must own the player");

    // Leave: range server stops, cache released, worker aborted, player closed.
    guest.leave_party();
    let after = guest.debug_session_owned();
    assert!(!after.cache_owned);
    assert!(!after.range_server_owned);
    assert!(!after.transfer_worker_owned);
    assert!(!after.player_owned);

    // After leave there is no client, so no stale worker can write anything.
    assert!(
        guest.client_for_test().is_none(),
        "no client must remain after leave"
    );
    host.leave_party();
    let _ = std::fs::remove_file(&path);
    std::env::remove_var("MOVE_PARTY_DEV_LOOPBACK");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnect_reuses_single_session_worker() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");
    let path = temp_media(1_100_000);

    let host = AppRuntime::new();
    let host_snap = host
        .create_local_party(Some(path.to_str().unwrap().to_string()))
        .await
        .expect("host");
    let invite_code = host_snap.room.invite_code.clone().expect("invite");
    let (invite, addr) = invite_details(&invite_code);
    let credentials = invite_to_credentials(&invite);

    let guest = AppRuntime::new();
    async fn connect_guest(
        guest: AppRuntime,
        addr: std::net::SocketAddr,
        fingerprint: String,
        credentials: move_party_lib::network::quic::RoomCredentials,
    ) {
        let identity = DeviceIdentity::new_ephemeral();
        let (client, _auth) = QuicClient::connect(
            addr,
            fingerprint,
            credentials,
            identity,
            "Reconnect Guest".to_string(),
        )
        .await
        .expect("connect");
        guest.inject_client_for_test(client);
        guest.guest_fetch_media().await.expect("fetch");
    }

    connect_guest(
        guest.clone(),
        addr,
        invite.server_certificate_fingerprint.clone(),
        credentials.clone(),
    )
    .await;
    assert!(guest.debug_session_owned().transfer_worker_owned);

    // Replacement: a fresh join replaces the running session. guest_fetch_media
    // must abort the old worker before installing a new one.
    connect_guest(
        guest.clone(),
        addr,
        invite.server_certificate_fingerprint.clone(),
        credentials.clone(),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let owned = guest.debug_session_owned();
    assert!(
        owned.transfer_worker_owned,
        "new session worker must be live"
    );
    assert!(!owned.old_worker_alive, "old session worker must be dead");
    assert!(owned.cache_owned);
    assert!(owned.range_server_owned);
    assert!(guest.client_for_test().is_some());

    guest.leave_party();
    host.leave_party();
    let _ = std::fs::remove_file(&path);
    std::env::remove_var("MOVE_PARTY_DEV_LOOPBACK");
}

// ──────────────────────────────────────────────────────────────────────────────
// M3.4 — Strict sync remains the only authority: guest starvation pauses BOTH
// sides; refill requires a fresh readiness consensus to resume; no independent
// Local Perfect auto-resume.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn guest_starvation_pauses_host_and_guest_with_consensus_resume() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");
    let path = temp_media(1_000_000);

    let host = AppRuntime::new();
    let host_snap = host
        .create_local_party(Some(path.to_str().unwrap().to_string()))
        .await
        .expect("host");
    let invite_code = host_snap.room.invite_code.clone().expect("invite");
    let (_invite, _addr) = invite_details(&invite_code);

    let guest = AppRuntime::new();
    guest
        .join_party(invite_code.clone())
        .await
        .expect("guest join");

    // Let the auto media fetch complete so the guest owns a live session.
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if guest.snapshot().media.is_some() {
            break;
        }
    }
    assert!(guest.snapshot().media.is_some(), "guest must have media");

    // Both sides start playback through the coordinator.
    host.enter_cinema();
    host.resume_playback();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let host_snap = host.snapshot();
    assert_eq!(host_snap.sync.room_state, "PLAYING");

    // Guest cannot provide required playback bytes → starvation event.
    guest.report_buffer_status(1_200, 0, true);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    let host_stalled = host.snapshot();
    let guest_stalled = guest.snapshot();
    assert!(
        host_stalled.sync.strict_sync_paused,
        "host must pause under strict sync"
    );
    assert_eq!(host_stalled.sync.room_state, "BUFFERING");
    assert!(
        guest_stalled.sync.strict_sync_paused,
        "guest must pause under strict sync"
    );

    // Refill: buffer recovered. Recovery must NOT auto-resume playback.
    guest.report_buffer_status(1_200, 8_000, false);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let host_recovered = host.snapshot();
    assert!(
        host_recovered.sync.room_state != "PLAYING",
        "recovery must not auto-resume (got {})",
        host_recovered.sync.room_state
    );

    // Fresh readiness consensus drives the synchronized resume.
    host.set_ready();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    guest.set_ready();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    host.resume_playback();
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    let host_final = host.snapshot();
    assert_eq!(
        host_final.sync.room_state, "PLAYING",
        "synchronized resume requires readiness consensus (state={}, err={:?})",
        host_final.sync.room_state, host_final.player.error_message
    );
    assert!(!host_final.sync.strict_sync_paused);

    guest.leave_party();
    host.leave_party();
    let _ = std::fs::remove_file(&path);
    std::env::remove_var("MOVE_PARTY_DEV_LOOPBACK");
}
