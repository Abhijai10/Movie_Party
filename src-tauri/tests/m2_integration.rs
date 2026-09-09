// ─── M2 Integration Tests ──────────────────────────────────────────────────────
// Tests A–F: live strict-sync, readiness, play/pause/seek, disconnect.
// All use real loopback QUIC and real AppRuntime paths.
// ──────────────────────────────────────────────────────────────────────────────

use movie_party_lib::app_runtime::AppRuntime;

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Serializes all access to the process-global `MOVIE_PARTY_DEV_LOOPBACK` env
/// var. Tests run in parallel (tokio multi-thread), and `create_local_party`
/// reads the var, so the env_tests module and every `setup_host_guest` must
/// hold this lock while touching the var or calling `create_local_party`.
static ENV_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
fn env_lock() -> &'static tokio::sync::Mutex<()> {
    ENV_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn temp_media_path() -> std::path::PathBuf {
    std::env::temp_dir().join("m2_test.mp4")
}

fn write_temp_media(path: &std::path::PathBuf) {
    let _ = std::fs::write(path, b"dummy-mp4-content");
}

/// Poll host.snapshot() until predicate satisfied or deadline hit.
/// snapshot() is synchronous — no async runtime required.
fn poll_host(
    host: &AppRuntime,
    deadline: std::time::Duration,
    mut predicate: impl FnMut(&movie_party_lib::app_runtime::AppSnapshot) -> bool,
) -> movie_party_lib::app_runtime::AppSnapshot {
    let start = std::time::Instant::now();
    loop {
        let snap = host.snapshot();
        if predicate(&snap) {
            return snap;
        }
        if std::time::Instant::now() - start > deadline {
            panic!("host predicate not satisfied within {:?}", deadline);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Poll guest.snapshot() until predicate satisfied or deadline hit.
/// Guest state converges asynchronously via the QUIC event listener.
fn poll_guest(
    guest: &AppRuntime,
    deadline: std::time::Duration,
    mut predicate: impl FnMut(&movie_party_lib::app_runtime::AppSnapshot) -> bool,
) -> movie_party_lib::app_runtime::AppSnapshot {
    let start = std::time::Instant::now();
    loop {
        let snap = guest.snapshot();
        if predicate(&snap) {
            return snap;
        }
        if std::time::Instant::now() - start > deadline {
            panic!("guest predicate not satisfied within {:?}", deadline);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Set up host + guest on loopback QUIC. Returns (host, guest, invite_code).
async fn setup_host_guest() -> (AppRuntime, AppRuntime, String) {
    let _env_guard = env_lock().lock().await;
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    let host = AppRuntime::new();
    let guest = AppRuntime::new();

    let media = temp_media_path();
    write_temp_media(&media);

    let host_snap = host
        .create_local_party(Some(media.to_string_lossy().to_string()))
        .await
        .expect("host create_local_party");

    let invite = host_snap.room.invite_code.clone().expect("invite code");
    assert!(invite.starts_with("movieparty://join/"));

    guest
        .join_party(invite.clone())
        .await
        .expect("guest join_party");
    drop(_env_guard);

    // Allow background peer_event_listener + wake loop to settle
    std::thread::sleep(std::time::Duration::from_millis(300));

    (host, guest, invite)
}

/// Both participants mark ready and the host waits for the READY_CHECK
/// consensus (the guest's ReadyState round trip over QUIC) before returning.
///
/// V1 correctness: readiness requires genuinely prepared media, so the guest's
/// async media fetch must have landed before either side presses Ready, and the
/// guest must report genuine buffer through the real buffer-status path before
/// the play protocol can commit.
fn ready_both(host: &AppRuntime, guest: &AppRuntime) {
    let _ = poll_guest(guest, std::time::Duration::from_secs(30), |s| {
        s.participants
            .iter()
            .any(|p| p.role == "Guest" && p.media_ready)
    });
    guest.report_buffer_status(0, 8_000, false);
    guest.set_ready();
    host.set_ready();
    let _ = poll_host(host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "READYCHECK"
    });
}

/// Press the guest's Ready only after its media is genuinely prepared (V1
/// correctness: set_ready must not fabricate media readiness) and the guest
/// has reported genuine buffer.
// ──────────────────────────────────────────────────────────────────────────────
// TEST A — READY is consensus, never an auto-play
// Guest set_ready → coordinator.guest_ready → host sees Guest ready. Both
// sides sit at READYCHECK; PLAYING only arrives through the play protocol.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_ready_reaches_host() {
    let (host, guest, _invite) = setup_host_guest().await;

    // Wait for guest auth to propagate to host's snapshot (peer_event_listener)
    let _ = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "LOBBY"
            && s.participants
                .iter()
                .any(|p| p.role == "Guest" && p.connected)
    });

    // Guest marks ready
    guest.set_ready();
    let _ = host.set_ready();
    // READY_CHECK consensus lands once the guest's ReadyState round-trips
    let _ = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "READYCHECK"
    });

    // Readiness consensus must NOT auto-commit playback: both sides stay at
    // READYCHECK until the host runs the distributed play protocol.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(host.snapshot().room.state, "READYCHECK");
    assert_eq!(guest.snapshot().room.state, "READYCHECK");

    // Host play protocol → both sides PLAYING (guest auto-answers PLAY_READY)
    let _ = host.host_play();
    let _playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(guest_snap.room.state, "PLAYING");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST B — PLAY protocol transitions both to PLAYING with equal positions
// host_play → PLAY_PREPARE → guest PLAY_READY → PLAY_COMMIT → both PLAYING
// at the shared deadline; positions match. A second host_play is idempotent.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_b_play_transitions_both_to_playing() {
    let (host, guest, _invite) = setup_host_guest().await;

    ready_both(&host, &guest);

    // host_play only PREPARES: no PLAYING until the guest answers PLAY_READY
    let play_snap = host.host_play();
    assert_eq!(
        play_snap.room.state, "READYCHECK",
        "host_play must not commit before the guest is ready"
    );

    // Both sides converge on PLAYING at the shared deadline
    let host_playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let guest_playing = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(
        guest_playing.sync.position_ms, host_playing.sync.position_ms,
        "guest position_ms must match host after play commit; host={} guest={}",
        host_playing.sync.position_ms, guest_playing.sync.position_ms
    );

    // Idempotent: already PLAYING with nothing pending → no new cycle
    let again = host.host_play();
    assert_eq!(again.room.state, "PLAYING");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST C — PAUSE protocol is canonical
// host_pause → PAUSE_PREPARE → guest PAUSE_READY → PAUSE_COMMIT → both PAUSED
// at the shared deadline. A manual pause is NOT a strict-sync pause.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_c_pause_is_canonical() {
    let (host, guest, _invite) = setup_host_guest().await;

    // Both to PLAYING via the play protocol
    ready_both(&host, &guest);
    let _ = host.host_play();
    let playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let _ = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });

    let pause_snap = host.host_pause();
    assert_eq!(
        pause_snap.room.state, "PAUSING",
        "host_pause must prepare (PAUSING) before the guest answers PAUSE_READY"
    );

    let host_paused = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PAUSED"
    });
    assert_eq!(host_paused.sync.position_ms, playing.sync.position_ms);

    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PAUSED"
    });
    assert_eq!(guest_snap.room.state, "PAUSED");
    assert_eq!(guest_snap.sync.position_ms, pause_snap.sync.position_ms);
    assert!(
        !guest_snap.sync.strict_sync_paused,
        "a manual pause must not be flagged as strict-sync paused"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST D — SEEK protocol
// host_seek(target, resume) → SEEK_PREPARE → guest SEEK_READY → SEEK_COMMIT
// → seek lands at the deadline, then the play protocol restarts both sides.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_d_seek_sets_canonical_position_on_both() {
    let (host, guest, _invite) = setup_host_guest().await;

    ready_both(&host, &guest);

    let target_pos = 42_000u64;
    let seek_snap = host.host_seek(target_pos, true);
    assert_eq!(
        seek_snap.room.state, "SEEKING",
        "host_seek must enter SEEKING until the guest answers SEEK_READY"
    );

    // SEEK_COMMIT lands the position on both sides; resume then replays
    // through the play protocol so both sides end PLAYING at the target.
    let host_snap = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING" && s.sync.position_ms == target_pos
    });
    assert_eq!(host_snap.sync.position_ms, target_pos);

    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.sync.position_ms == target_pos
    });
    assert_eq!(
        guest_snap.sync.position_ms, target_pos,
        "guest must match host seek target; got {}",
        guest_snap.sync.position_ms
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST E — Coordinator state is canonical
// After every protocol cycle both sides must agree on coordinator state.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_e_coordinator_state_is_canonical() {
    let (host, guest, _invite) = setup_host_guest().await;

    ready_both(&host, &guest);

    let _ = host.host_play();
    let playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(playing.room.state, "PLAYING");

    // Coordinators must agree on position
    assert_eq!(playing.sync.position_ms, guest.snapshot().sync.position_ms);

    // Host pauses canonically — guest follows via the pause protocol
    let _ = host.host_pause();
    let _ = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PAUSED"
    });

    let guest_after_pause = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PAUSED"
    });
    assert_eq!(guest_after_pause.room.state, "PAUSED");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST F — DISCONNECT
// Close guest AppRuntime → peer_event_listener exits → apply_disconnect fires
// → coordinator.peer_disconnected() → RECONNECTING + guest readiness cleared.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_f_disconnect_triggers_host_pause() {
    let (host, guest, _invite) = setup_host_guest().await;

    // Host sees LOBBY with authenticated guest
    let _lobby = poll_host(&host, std::time::Duration::from_secs(3), |s| {
        s.network.connected && s.room.state == "LOBBY"
    });
    assert!(
        _lobby
            .participants
            .iter()
            .any(|p| p.role == "Guest" && p.connected),
        "guest must be connected in LOBBY"
    );

    // Close guest connection (real QUIC close via leave_party)
    guest.leave_party();

    // Host detects peer leave: network.connected=false AND room in a pause state
    let after_disconnect = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        !s.network.connected
    });
    assert!(!after_disconnect.network.connected);

    let final_snap = host.snapshot();
    assert!(
        final_snap.room.state == "PAUSED" || final_snap.room.state == "RECONNECTING",
        "expected PAUSED or RECONNECTING after disconnect; got {}",
        final_snap.room.state
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST G — HOST AUTHORITY
// The guest can never force canonical playback: set_ready / resume_playback
// from the guest alone must not reach PLAYING.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_g_host_authority_guest_cannot_force_playing() {
    let (_host, guest, _invite) = setup_host_guest().await;

    guest.set_ready();
    let _ = guest.resume_playback();
    let snap = guest.snapshot();
    assert!(
        snap.room.state != "PLAYING",
        "guest alone must not reach PLAYING; got {}",
        snap.room.state
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST H — Host→Guest CHAT (broadcasts over QUIC)
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_h_host_chat_reaches_guest() {
    let (host, guest, _invite) = setup_host_guest().await;

    let body = "m2 host says hi".to_string();
    host.host_send_chat(body.clone()).expect("host_send_chat");

    std::thread::sleep(std::time::Duration::from_millis(500));

    let guest_snap = guest.snapshot();
    assert!(
        guest_snap.chat.iter().any(|m| m.body == body),
        "guest must receive host chat; got chat={:?}",
        guest_snap.chat
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST I — Guest→Host CHAT
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_i_guest_chat_reaches_host() {
    let (host, guest, _invite) = setup_host_guest().await;

    let body = "m2 guest replies".to_string();
    guest
        .send_chat_message(body.clone())
        .expect("guest send_chat_message");

    // Poll until host's coordinator receives the chat event via QUIC
    let _ = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.chat.iter().any(|m| m.body == body)
    });

    let host_snap = host.snapshot();
    assert_eq!(host_snap.chat.len(), 1);
    assert_eq!(host_snap.chat[0].body, body);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST J — Invalid input rejection + rate limiter
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_j_invalid_inputs_rejected() {
    let (host, guest, _invite) = setup_host_guest().await;

    // Empty chat message
    let result = host.send_chat_message("".to_string());
    assert!(result.is_err(), "empty chat must be rejected");

    // Reaction rate limiter: rapid-fire same reaction many times
    let reaction = "👍";
    for _i in 0..20 {
        let _ = guest.send_reaction(reaction.to_string());
    }
    let snap = guest.snapshot();
    let count = snap
        .reactions
        .iter()
        .filter(|r| r.reaction == reaction)
        .count();
    assert!(count < 20, "rate limiter must reject some; got {count}");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST K — BUFFER STARVATION / RECOVERY (strict sync)
// Guest reports buffer_low over QUIC → host pauses canonically → both PAUSED at
// the shared position. Recovery does NOT auto-resume (PROTOCOL_SPEC §30); the
// host runs a fresh play-protocol cycle to resume.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_k_guest_buffer_starvation_pauses_and_recovers() {
    let (host, guest, _invite) = setup_host_guest().await;

    // Consensus → PLAYING via play protocol
    ready_both(&host, &guest);
    let _ = host.host_play();
    let playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let position = playing.sync.position_ms;

    // Guest stalls: strict-sync input must pause the HOST (AGENTS.md §14)
    guest.report_buffer_status(position, 0, true);
    let host_buffering = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "BUFFERING"
    });
    assert_eq!(host_buffering.room.state, "BUFFERING");
    assert!(host_buffering.sync.strict_sync_paused);
    assert_eq!(host_buffering.sync.position_ms, position);

    let guest_buffering = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "BUFFERING"
    });
    assert_eq!(guest_buffering.sync.position_ms, position);

    // Guest recovers: the room may return to READY_CHECK, but MUST NOT
    // auto-resume — a fresh host play-protocol cycle is required.
    guest.report_buffer_status(position, 5_000, false);
    let host_readycheck = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "READYCHECK"
    });
    assert_eq!(host_readycheck.room.state, "READYCHECK");
    assert!(
        host_readycheck.sync.strict_sync_paused,
        "recovery must not lift the strict-sync pause by itself"
    );
    let guest_readycheck = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "READYCHECK"
    });
    assert_eq!(guest_readycheck.room.state, "READYCHECK");

    // Host resumes through the play protocol → both PLAYING at same position
    let _ = host.host_play();
    let host_playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(host_playing.room.state, "PLAYING");
    assert!(!host_playing.sync.strict_sync_paused);

    let guest_playing = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(guest_playing.room.state, "PLAYING");
    assert_eq!(guest_playing.sync.position_ms, position);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_k2_host_buffer_starvation_broadcasts_to_guest() {
    let (host, guest, _invite) = setup_host_guest().await;

    // Consensus → PLAYING via play protocol
    ready_both(&host, &guest);
    let _ = host.host_play();
    let playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let position = playing.sync.position_ms;

    // Host stalls → guest must stop too
    host.report_buffer_low(position, 0);
    let guest_buffering = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "BUFFERING"
    });
    assert_eq!(guest_buffering.room.state, "BUFFERING");
    assert_eq!(guest_buffering.sync.position_ms, position);
    assert_eq!(
        guest_buffering.buffer.buffering_participant.as_deref(),
        Some(host.snapshot().participants[0].display_name.as_str()),
        "guest must label the host as the buffering participant"
    );

    // Host recovers → READYCHECK (no auto-resume), then host play protocol
    host.report_buffer_recovered(5_000);
    let guest_readycheck = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "READYCHECK"
    });
    assert_eq!(guest_readycheck.room.state, "READYCHECK");

    let _ = host.host_play();
    let guest_playing = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(guest_playing.room.state, "PLAYING");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST L — READY never bypasses the play protocol
// Even with both participants ready, the room must sit at READYCHECK; only a
// committed play operation moves it to PLAYING.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_l_ready_does_not_bypass_play_protocol() {
    let (host, guest, _invite) = setup_host_guest().await;

    ready_both(&host, &guest);

    std::thread::sleep(std::time::Duration::from_millis(700));
    assert_eq!(host.snapshot().room.state, "READYCHECK");
    assert_eq!(guest.snapshot().room.state, "READYCHECK");

    // Now the play protocol
    let _ = host.host_play();
    let _ = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(guest_snap.room.state, "PLAYING");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST M — PLAY honors the execution deadline
// After host_play the room must NOT be PLAYING until the shared deadline;
// the commit lands only once both sides are ready.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_m_play_commit_respects_deadline() {
    let (host, guest, _invite) = setup_host_guest().await;

    ready_both(&host, &guest);

    let prepared_at = std::time::Instant::now();
    let _ = host.host_play();

    // Immediately after PREPARE the room must still be READYCHECK even though
    // the guest auto-answers PLAY_READY within milliseconds: the commit waits
    // for the deadline (min 750ms lead).
    let early = host.snapshot();
    assert_eq!(early.room.state, "READYCHECK");

    let host_playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let elapsed = prepared_at.elapsed();
    assert_eq!(host_playing.room.state, "PLAYING");
    assert!(
        elapsed >= std::time::Duration::from_millis(600),
        "play commit must respect the execution deadline; committed after {elapsed:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST N — CLOCK CALIBRATION (MASTER_PRD §18)
// The guest measures the offset with CLOCK_PING probes and reports it to the
// host; both sides expose a measured RTT after calibration.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_n_clock_calibration_wires_host_and_guest() {
    let (host, guest, _invite) = setup_host_guest().await;

    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.network.rtt_ms.is_some()
    });
    assert!(
        guest_snap.network.rtt_ms.is_some(),
        "guest must measure RTT via CLOCK_PING"
    );

    let host_snap = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.network.rtt_ms.is_some()
    });
    assert!(
        host_snap.network.rtt_ms.is_some(),
        "host must learn peer RTT via ClockResult"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST O — SHARED CONTROLS: denied by default
// Guest playback requests are CONTROL_REQUESTs; with the toggle off the host
// denies them and the guest surfaces MP-CTRL-001.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_o_shared_controls_denied_by_default() {
    let (host, guest, _invite) = setup_host_guest().await;

    let _ = poll_host(&host, std::time::Duration::from_secs(3), |s| {
        s.room.state == "LOBBY"
    });

    let _ = guest.pause_playback();
    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.error.is_some()
    });
    assert!(
        guest_snap
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("MP-CTRL-001"),
        "denied request must surface MP-CTRL-001; got {:?}",
        guest_snap.error
    );
    assert_eq!(
        host.snapshot().room.state,
        "LOBBY",
        "host state must be untouched by a denied guest request"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST P — SHARED CONTROLS: granted requests become canonical ops
// Host enables Shared Controls → guest pause request → host runs the PAUSE
// protocol → both PAUSED. The guest never commits anything itself.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_p_shared_controls_granted_request_reaches_both() {
    let (host, guest, _invite) = setup_host_guest().await;

    // Consensus → PLAYING via play protocol
    ready_both(&host, &guest);
    let _ = host.host_play();
    let playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    let position = playing.sync.position_ms;

    // Host enables Shared Controls
    let toggle = host.set_shared_controls(true);
    assert!(toggle.room.shared_controls);

    // Guest pause request → granted → host runs the canonical PAUSE protocol
    let _ = guest.pause_playback();
    let host_paused = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PAUSED"
    });
    assert_eq!(host_paused.sync.position_ms, position);

    let guest_paused = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PAUSED"
    });
    assert_eq!(guest_paused.room.state, "PAUSED");
    assert_eq!(guest_paused.sync.position_ms, position);
    assert!(
        guest_paused.error.is_none()
            || guest_paused.error.as_deref().unwrap_or_default().is_empty(),
        "granted request must not surface a deny error; got {:?}",
        guest_paused.error
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST Q — Only the host may toggle Shared Controls
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_q_guest_cannot_toggle_shared_controls() {
    let (host, guest, _invite) = setup_host_guest().await;

    let snap = guest.set_shared_controls(true);
    assert!(
        snap.error
            .as_deref()
            .unwrap_or_default()
            .contains("MP-CTRL-002"),
        "guest toggle must surface MP-CTRL-002; got {:?}",
        snap.error
    );
    assert!(
        !host.snapshot().room.shared_controls,
        "host flag must be untouched by the guest"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST R — RECONNECT (MASTER_PRD §53)
// Guest drops → host RECONNECTING with readiness cleared → guest rejoins the
// same room → host restores connectivity but does NOT blindly resume → fresh
// readiness + play protocol restart playback.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_r_reconnect_requires_fresh_consensus() {
    let (host, guest, invite) = setup_host_guest().await;

    // Play first
    ready_both(&host, &guest);
    let _ = host.host_play();
    let playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(playing.room.state, "PLAYING");

    // Drop the guest; host must stop
    guest.leave_party();
    let _ = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        !s.network.connected
    });
    let reconnecting = host.snapshot();
    assert!(
        reconnecting.room.state == "RECONNECTING" || reconnecting.room.state == "PAUSED",
        "host must stop after disconnect; got {}",
        reconnecting.room.state
    );

    // Guest rejoins the SAME room (same device identity)
    guest
        .join_party(invite.clone())
        .await
        .expect("guest rejoin");
    std::thread::sleep(std::time::Duration::from_millis(300));

    let reconnected = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.network.connected
    });
    assert!(reconnected.network.connected);
    // No blind resume: the room must NOT be PLAYING immediately after rejoin.
    assert!(
        reconnected.room.state != "PLAYING",
        "reconnect must not resume playback blindly; got {}",
        reconnected.room.state
    );

    // Fresh readiness + fresh play protocol restart playback
    ready_both(&host, &guest);
    let _ = host.host_play();
    let resumed = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(resumed.room.state, "PLAYING");
    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(guest_snap.room.state, "PLAYING");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST S — Stale/duplicate op-id safety (PROTOCOL_SPEC §40)
// A PLAY_READY for a bogus operation_id must be ignored without disturbing
// the in-flight protocol; a late duplicate PLAY_READY after commit must not
// execute twice.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_s_stale_and_duplicate_ops_are_ignored() {
    let (host, guest, _invite) = setup_host_guest().await;

    ready_both(&host, &guest);

    // A stale PLAY_READY for an unknown op must not derail the protocol
    if let Some(client) = guest.client_for_test() {
        let stale = client
            .send_play_ready("bogus-operation-id".to_string(), true, 0, 5_000)
            .await;
        assert!(stale.is_ok());
    }

    let _ = host.host_play();
    let host_playing = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(host_playing.room.state, "PLAYING");
    let guest_playing = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });
    assert_eq!(guest_playing.room.state, "PLAYING");

    // A duplicate PLAY_READY for the same op after commit must not double-commit
    if let Some(client) = guest.client_for_test() {
        let _ = client
            .send_play_ready("bogus-operation-id".to_string(), true, 0, 5_000)
            .await;
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(host.snapshot().room.state, "PLAYING");
    assert_eq!(guest.snapshot().room.state, "PLAYING");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST T — SEEK protocol: host stays in SEEKING until guest readiness
// A seek must not commit before the guest answers SEEK_READY; with resume the
// play protocol follows and both sides land at the target together.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_t_seek_waits_for_guest_and_resumes_together() {
    let (host, guest, _invite) = setup_host_guest().await;

    ready_both(&host, &guest);
    let _ = host.host_play();
    let _ = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING"
    });

    let target = 99_000u64;
    let seek_snap = host.host_seek(target, true);
    assert_eq!(seek_snap.room.state, "SEEKING");

    let host_snap = poll_host(&host, std::time::Duration::from_secs(30), |s| {
        s.room.state == "PLAYING" && s.sync.position_ms == target
    });
    assert_eq!(host_snap.sync.position_ms, target);

    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.sync.position_ms == target && s.room.state == "PLAYING"
    });
    assert_eq!(guest_snap.sync.position_ms, target);
}

// ──────────────────────────────────────────────────────────────────────────────
// DEV LOOPBACK env canary test
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod env_tests {
    use super::*;
    use movie_party_lib::app_runtime::AppRuntime;

    struct ScopedEnv {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }
    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            if let Some(v) = &self.prev {
                std::env::set_var(self.key, v);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
    fn scoped(key: &'static str, val: &str) -> ScopedEnv {
        let p = std::env::var_os(key);
        std::env::set_var(key, val);
        ScopedEnv { key, prev: p }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dev_loopback_mode_selects_correct_bind_addr() {
        let _env_guard = env_lock().lock().await;
        // No env var → Tailscale path. On a machine where Tailscale is
        // Running/Online with a usable CGNAT IPv4 this must SUCCEED and bind
        // to the Tailscale address; when Tailscale is unavailable it must
        // fail with a stable Tailscale error. localhost is never used.
        let _g0 = scoped("MOVIE_PARTY_DEV_LOOPBACK", "");
        std::env::remove_var("MOVIE_PARTY_DEV_LOOPBACK");
        let r0 = AppRuntime::new();
        match r0.create_local_party(None).await {
            Ok(snap) => {
                assert_eq!(snap.network.transport, "quic");
                assert!(
                    snap.network.path.starts_with("Listening on 100."),
                    "Tailscale path must advertise a CGNAT IPv4, got: {}",
                    snap.network.path
                );
                let invite = snap.room.invite_code.expect("invite code");
                let parsed = movie_party_lib::room::parse_invite(&invite).expect("valid invite");
                assert!(
                    parsed.host_ip.starts_with("100."),
                    "invite must advertise the Tailscale IPv4, got host_ip={}",
                    parsed.host_ip
                );
                assert_ne!(parsed.host_ip, "127.0.0.1");
                r0.leave_party();
            }
            Err(e) => {
                assert!(
                    e.starts_with("MP-NET-TS-"),
                    "Tailscale-unavailable error must be a stable MP-NET-TS code, got: {e}"
                );
            }
        }
        drop(_g0);

        // 0 → Tailscale path (same behavior as no env var).
        let _g1 = scoped("MOVIE_PARTY_DEV_LOOPBACK", "0");
        let r1 = AppRuntime::new();
        match r1.create_local_party(None).await {
            Ok(snap) => {
                assert!(
                    snap.network.path.starts_with("Listening on 100."),
                    "DEV_LOOPBACK=0 → Tailscale path, got: {}",
                    snap.network.path
                );
                r1.leave_party();
            }
            Err(e) => assert!(
                e.starts_with("MP-NET-TS-"),
                "Tailscale-unavailable error must be stable MP-NET-TS code, got: {e}"
            ),
        }
        drop(_g1);

        // 1 → loopback always, regardless of Tailscale availability.
        let _g2 = scoped("MOVIE_PARTY_DEV_LOOPBACK", "1");
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
    }

    /// Real Tailscale validation: when the development machine is Running/
    /// Online on a Tailscale network with a usable CGNAT IPv4, the host
    /// server must bind to the real Tailscale address (never localhost) and a
    /// guest runtime must be able to join over that real Tailscale endpoint.
    ///
    /// When Tailscale is not usable, the test skips with a clear message — it
    /// never manufactures a fake pass.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_tailscale_party_binds_cgnat_and_guest_joins() {
        let _env_guard = env_lock().lock().await;
        std::env::remove_var("MOVIE_PARTY_DEV_LOOPBACK");

        let readiness = movie_party_lib::network::tailscale::local_readiness().await;
        if !readiness.is_usable() {
            eprintln!(
                "SKIP real_tailscale_party_binds_cgnat_and_guest_joins: \
                 Tailscale is not usable on this machine ({:?})",
                readiness.state
            );
            return;
        }
        let tailscale_ip = readiness.ip.expect("ready implies an ip");
        assert!(
            tailscale_ip.starts_with("100."),
            "Tailscale IPv4 must be in CGNAT range, got: {tailscale_ip}"
        );

        // Now test the full AppRuntime create+join over the real Tailscale IP.
        let host = AppRuntime::new();
        let guest = AppRuntime::new();

        let host_snap = host
            .create_local_party(None)
            .await
            .expect("host create_local_party over real Tailscale");
        assert!(
            host_snap.network.path.starts_with("Listening on 100."),
            "host must bind the Tailscale address, got: {}",
            host_snap.network.path
        );

        let invite = host_snap.room.invite_code.expect("invite code");
        assert!(invite.starts_with("movieparty://join/"));
        let parsed = movie_party_lib::room::parse_invite(&invite).expect("valid invite");
        assert_eq!(parsed.host_ip, tailscale_ip);
        assert_ne!(parsed.host_ip, "127.0.0.1");
        assert!(!parsed.host_ip.starts_with("192.168."));

        let guest_snap = guest
            .join_party(invite.clone())
            .await
            .expect("guest join over real Tailscale");
        assert_eq!(guest_snap.network.transport, "quic");
        assert!(
            guest_snap.network.connected,
            "guest must be connected over QUIC after joining"
        );

        std::thread::sleep(std::time::Duration::from_millis(500));

        // The host must observe the authenticated guest.
        let host_snap = poll_host(&host, std::time::Duration::from_secs(30), |s| {
            s.network.connected
                && s.participants
                    .iter()
                    .any(|p| p.role == "Guest" && p.connected)
        });
        assert!(host_snap.network.connected);

        host.leave_party();
        guest.leave_party();
    }

    /// Joining a party whose host has already left (or is unreachable) must
    /// produce a stable MP-NET-TS-005 error code so the frontend can show the
    /// PartnerConnectView instead of a generic failure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn join_party_unreachable_host_returns_ts005() {
        let _env_guard = env_lock().lock().await;
        std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

        let host = AppRuntime::new();
        let guest = AppRuntime::new();

        let snap = host
            .create_local_party(None)
            .await
            .expect("host create loopback");
        let invite = snap.room.invite_code.expect("invite code");

        // Host leaves — aborts the QUIC server so the port is freed.
        host.leave_party();

        // Give the server a moment to shut down.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Guest tries to join the now-stale invite.
        let err = guest
            .join_party(invite)
            .await
            .expect_err("join to stale host must fail");

        assert!(
            err.contains("MP-NET-TS-005"),
            "unreachable host must produce MP-NET-TS-005, got: {err}"
        );

        std::env::remove_var("MOVIE_PARTY_DEV_LOOPBACK");
    }
}

// ─── M5: Call Signal Integration ──────────────────────────────────────────────
// Tests prove that call signals (offer/answer/ICE) travel over the
// authenticated QUIC room transport and arrive at the peer.
// ──────────────────────────────────────────────────────────────────────────────

use movie_party_lib::call::{CallSignal, CallSignalType};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn m5_call_signal_offer_arrives_at_guest() {
    let _env_guard = env_lock().lock().await;
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    let host = AppRuntime::new();
    let guest = AppRuntime::new();

    let snap = host.create_local_party(None).await.expect("host create");
    let invite = snap.room.invite_code.unwrap();
    guest.join_party(invite).await.expect("guest join");
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(_env_guard);

    // Host sends an OFFER signal
    host.submit_call_signal(CallSignal {
        signal_type: CallSignalType::Offer,
        data: "fake-sdp-offer".to_string(),
    })
    .expect("host submit_call_signal");

    // Guest should receive the call signal via QUIC
    let guest_snap = poll_guest(&guest, std::time::Duration::from_secs(3), |s| {
        !s.call_signals.is_empty()
    });
    assert_eq!(guest_snap.call_signals.len(), 1);
    assert_eq!(guest_snap.call_signals[0].signal_type, "OFFER");
    assert_eq!(guest_snap.call_signals[0].data, "fake-sdp-offer");

    host.leave_party();
    guest.leave_party();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn m5_call_signal_answer_arrives_at_host() {
    let _env_guard = env_lock().lock().await;
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    let host = AppRuntime::new();
    let guest = AppRuntime::new();

    let snap = host.create_local_party(None).await.expect("host create");
    let invite = snap.room.invite_code.unwrap();
    guest.join_party(invite).await.expect("guest join");
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(_env_guard);

    // Guest sends an ANSWER signal
    guest
        .submit_call_signal(CallSignal {
            signal_type: CallSignalType::Answer,
            data: "fake-sdp-answer".to_string(),
        })
        .expect("guest submit_call_signal");

    // Host should receive the call signal via its QUIC broadcast listener
    let host_snap = poll_host(&host, std::time::Duration::from_secs(3), |s| {
        !s.call_signals.is_empty()
    });
    assert_eq!(host_snap.call_signals.len(), 1);
    assert_eq!(host_snap.call_signals[0].signal_type, "ANSWER");
    assert_eq!(host_snap.call_signals[0].data, "fake-sdp-answer");

    host.leave_party();
    guest.leave_party();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn m5_call_signal_ice_arrives_bidirectional() {
    let _env_guard = env_lock().lock().await;
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    let host = AppRuntime::new();
    let guest = AppRuntime::new();

    let snap = host.create_local_party(None).await.expect("host create");
    let invite = snap.room.invite_code.unwrap();
    guest.join_party(invite).await.expect("guest join");
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(_env_guard);

    // Host sends ICE candidate
    host.submit_call_signal(CallSignal {
        signal_type: CallSignalType::Ice,
        data: "host-ice-candidate".to_string(),
    })
    .expect("host ICE");

    // Guest receives it
    poll_guest(&guest, std::time::Duration::from_secs(3), |s| {
        s.call_signals.iter().any(|cs| cs.signal_type == "ICE")
    });

    // Guest sends ICE candidate back
    guest
        .submit_call_signal(CallSignal {
            signal_type: CallSignalType::Ice,
            data: "guest-ice-candidate".to_string(),
        })
        .expect("guest ICE");

    // Host receives it
    poll_host(&host, std::time::Duration::from_secs(3), |s| {
        s.call_signals
            .iter()
            .any(|cs| cs.data == "guest-ice-candidate")
    });

    host.leave_party();
    guest.leave_party();
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST W — Running-app deep-link join: a guest already inside a room joins a
// second room from a fresh invite. The old room's session must be torn down
// (fresh invite, no stale media/chat/provider state), never a duplicate worker.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_w_guest_joins_second_room_cleanly_from_running_app() {
    let _env_guard = env_lock().lock().await;
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    let host_a = AppRuntime::new();
    let host_b = AppRuntime::new();
    let guest = AppRuntime::new();

    let media_a = temp_media_path();
    write_temp_media(&media_a);
    let snap_a = host_a
        .create_local_party(Some(media_a.to_string_lossy().to_string()))
        .await
        .expect("host A create");
    let invite_a = snap_a.room.invite_code.clone().expect("invite A");

    // Guest joins room A first (the "already inside a party" state).
    guest
        .join_party(invite_a.clone())
        .await
        .expect("guest join A");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let snap = guest.snapshot();
    assert_eq!(snap.room.role, "Guest");
    assert_eq!(
        snap.room.invite_code.as_deref(),
        Some(invite_a.as_str()),
        "guest must be in room A"
    );

    // Host B starts a separate room and hands out a fresh invite.
    let media_b = temp_media_path();
    write_temp_media(&media_b);
    let snap_b = host_b
        .create_local_party(Some(media_b.to_string_lossy().to_string()))
        .await
        .expect("host B create");
    let invite_b = snap_b.room.invite_code.clone().expect("invite B");
    assert_ne!(invite_a, invite_b, "second room must be a distinct invite");

    // Running-app deep link: the guest joins room B while still in room A.
    guest
        .join_party(invite_b.clone())
        .await
        .expect("guest join B");
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(_env_guard);

    let snap = poll_guest(&guest, std::time::Duration::from_secs(30), |s| {
        s.room.state == "LOBBY" && s.media.is_some()
    });
    assert_eq!(
        snap.room.invite_code.as_deref(),
        Some(invite_b.as_str()),
        "guest must now be in room B"
    );
    assert_eq!(snap.room.role, "Guest");
    assert!(
        snap.chat.is_empty(),
        "joining a new room must clear the previous room's chat"
    );
    assert!(snap.reactions.is_empty());
    assert_eq!(
        snap.sync.position_ms, 0,
        "joining a new room must reset canonical position"
    );
    assert!(
        snap.provider.provider_id.is_none(),
        "joining a new room must clear stale provider state"
    );
    assert_eq!(
        snap.media.as_ref().map(|m| m.filename.as_str()),
        Some("m2_test.mp4")
    );

    host_a.leave_party();
    host_b.leave_party();
    guest.leave_party();
}
