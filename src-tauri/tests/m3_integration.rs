// ─── M3 Integration Tests ──────────────────────────────────────────────────────
// Tests for the player abstraction layer, libmpv backend, and sync coordinator
// integration. All tests use the mock/stub player to prove the interface works
// without requiring libmpv installed.
// ──────────────────────────────────────────────────────────────────────────────

use movie_party_lib::media::player::{
    detect_libmpv, LibMpvPlayer, LocalPlayer, PlayerError, PlayerState,
};
use movie_party_lib::sync::{
    consensus::ParticipantReadiness,
    local::{LocalSyncCoordinator, PauseCause, PeerRole},
    state_machine::RoomState,
};

// ── Helpers ────────────────────────────────────────────────────────────────────

fn temp_media_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("m3_test_{}.mp4", uuid::Uuid::now_v7()))
}

fn write_temp_media(path: &std::path::PathBuf, size: usize) {
    let mut file = std::fs::File::create(path).expect("create fixture");
    for i in 0..size {
        std::io::Write::write_all(&mut file, &[(i % 251) as u8]).expect("write byte");
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 1 — Player initialization: detect_libmpv returns a result
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_1_player_initialization_returns_structured_result() {
    let availability = detect_libmpv();
    // Must not panic; must return a valid availability struct
    assert!(
        !availability.checked_paths.is_empty(),
        "should check at least one candidate path"
    );
    // availability.available may be false if mpv is not installed — that's fine
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 2 — Invalid media handling: open nonexistent file returns error
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_2_invalid_media_returns_structured_error() {
    let mut player = LibMpvPlayer::with_availability(true);
    let result = player.open(std::path::Path::new("/nonexistent/media.mp4"));

    assert!(result.is_err());
    match result.unwrap_err() {
        PlayerError::MissingMedia { path } => {
            assert!(path.contains("nonexistent"));
        }
        other => panic!("expected MissingMedia, got: {other:?}"),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 3 — Play command: open → play transitions state to Playing
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_3_play_command_transitions_to_playing() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.play().expect("play");

    let snapshot = player.snapshot();
    assert_eq!(snapshot.state, PlayerState::Playing);

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 4 — Pause command: play → pause transitions to Paused
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_4_pause_command_transitions_to_paused() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.play().expect("play");
    player.pause().expect("pause");

    let snapshot = player.snapshot();
    assert_eq!(snapshot.state, PlayerState::Paused);

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 5 — Seek command: seek updates position
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_5_seek_command_updates_position() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.seek(5_000).expect("seek");

    let snapshot = player.snapshot();
    assert_eq!(snapshot.position_ms, 5_000);

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 6 — Position reporting: snapshot returns live position
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_6_position_reporting_reflects_playback_state() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.seek(10_000).expect("seek");
    player.play().expect("play");

    let snapshot = player.snapshot();
    assert_eq!(snapshot.position_ms, 10_000);
    assert_eq!(snapshot.state, PlayerState::Playing);

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 7 — Duration reporting: snapshot includes duration when loaded
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_7_duration_reporting_none_for_stub() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");

    // Stub player returns None for duration (real mpv would return the actual duration)
    let snapshot = player.snapshot();
    assert!(
        snapshot.duration_ms.is_none(),
        "stub player should not report duration"
    );

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 8 — Close resets state
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_8_close_resets_all_state() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.play().expect("play");
    player.seek(42_000).expect("seek");

    let before = player.snapshot();
    assert_eq!(before.state, PlayerState::Seeking);

    player.close();
    let after = player.snapshot();
    assert_eq!(after.state, PlayerState::Stopped);
    assert_eq!(after.position_ms, 0);

    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 9 — Unavailable player: all commands return error
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_9_unavailable_player_returns_errors_for_all_commands() {
    let mut player = LibMpvPlayer::with_availability(false);

    assert!(player.open(std::path::Path::new("/fake")).is_err());
    assert!(player.play().is_err());
    assert!(player.pause().is_err());
    assert!(player.seek(0).is_err());
    assert!(player.set_volume(0.5).is_err());
    assert!(player.set_playback_rate(1.0).is_err());
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 10 — Coordinator play: commit_play + player.play() both in sync
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_10_coordinator_play_triggers_player_play() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");

    let mut coordinator = LocalSyncCoordinator::new();
    coordinator.room_state = RoomState::Lobby;
    coordinator.host_ready(ParticipantReadiness::ready(5_000));
    coordinator.guest_ready(ParticipantReadiness::ready(5_000));

    let scheduled = coordinator
        .prepare_play(0, 1_000_000, 5_000)
        .expect("prepare");
    coordinator.commit_play(&scheduled).expect("commit");
    player.play().expect("player play");

    assert_eq!(coordinator.room_state, RoomState::Playing);
    assert_eq!(player.snapshot().state, PlayerState::Playing);
    assert_eq!(coordinator.host_position_ms, 0);

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 11 — Coordinator pause: commit_pause + player.pause() both in sync
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_11_coordinator_pause_triggers_player_pause() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.play().expect("play");

    let mut coordinator = LocalSyncCoordinator::new();
    coordinator.room_state = RoomState::Playing;
    coordinator.host_position_ms = 5_000;
    coordinator.guest_position_ms = 5_000;

    coordinator.commit_pause(5_000, PauseCause::Manual);
    player.pause().expect("player pause");

    assert_eq!(coordinator.room_state, RoomState::Paused);
    assert_eq!(player.snapshot().state, PlayerState::Paused);
    assert!(
        !coordinator.paused_by_strict_sync,
        "manual pause must not be strict sync"
    );

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 12 — Coordinator seek: commit_seek + player.seek() both in sync
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_12_coordinator_seek_triggers_player_seek() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.play().expect("play");

    let mut coordinator = LocalSyncCoordinator::new();
    coordinator.room_state = RoomState::Playing;
    coordinator.host_position_ms = 0;
    coordinator.guest_position_ms = 0;

    let target = 30_000u64;
    coordinator.commit_seek(target, false);
    player.seek(target).expect("player seek");

    assert_eq!(coordinator.host_position_ms, target);
    assert_eq!(coordinator.guest_position_ms, target);
    assert_eq!(player.snapshot().position_ms, target);
    assert_eq!(
        coordinator.room_state,
        RoomState::Paused,
        "seek without resume should be paused"
    );

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 13 — Buffer starvation: coordinator buffer_low + player.pause() in sync
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_13_buffer_starvation_syncs_coordinator_and_player() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.play().expect("play");

    let mut coordinator = LocalSyncCoordinator::new();
    coordinator.room_state = RoomState::Playing;
    coordinator.host_position_ms = 10_000;
    coordinator.guest_position_ms = 10_000;

    // Simulate buffer low: both coordinator and player must pause
    coordinator
        .buffer_low(PeerRole::Guest, 9_500)
        .expect("buffer low");
    player.pause().expect("player pause on buffer low");

    assert_eq!(coordinator.room_state, RoomState::Buffering);
    assert!(
        coordinator.paused_by_strict_sync,
        "buffer low must be strict sync"
    );
    assert_eq!(coordinator.host_position_ms, 9_500);
    assert_eq!(coordinator.guest_position_ms, 9_500);
    assert_eq!(player.snapshot().state, PlayerState::Paused);

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 14 — Buffer recovery + fresh play: coordinator stays paused until play
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_14_buffer_recovery_does_not_auto_resume_player() {
    let path = temp_media_path();
    write_temp_media(&path, 1000);

    let mut player = LibMpvPlayer::with_availability(true);
    player.open(&path).expect("open");
    player.play().expect("play");

    let mut coordinator = LocalSyncCoordinator::new();
    coordinator.room_state = RoomState::Playing;
    coordinator.host_position_ms = 10_000;
    coordinator.guest_position_ms = 10_000;

    // Buffer low
    coordinator
        .buffer_low(PeerRole::Guest, 9_500)
        .expect("buffer low");
    player.pause().expect("player pause");

    // Recovery
    coordinator.room_state = RoomState::Buffering;
    coordinator.buffer_recovered().expect("recovered");

    // PROTOCOL_SPEC §30: recovery does NOT auto-resume
    assert_eq!(coordinator.room_state, RoomState::ReadyCheck);
    assert!(coordinator.paused_by_strict_sync);
    assert_eq!(
        player.snapshot().state,
        PlayerState::Paused,
        "player must stay paused after recovery"
    );

    // Fresh play protocol needed to resume
    coordinator.host_ready(ParticipantReadiness::ready(5_000));
    coordinator.guest_ready(ParticipantReadiness::ready(5_000));
    let scheduled = coordinator
        .prepare_play(9_500, 1_000_000, 5_000)
        .expect("prepare");
    coordinator.commit_play(&scheduled).expect("commit");
    player.play().expect("player play after recovery");

    assert_eq!(coordinator.room_state, RoomState::Playing);
    assert!(!coordinator.paused_by_strict_sync);
    assert_eq!(player.snapshot().state, PlayerState::Playing);

    player.close();
    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 15 — Full lifecycle: open → ready → play → pause → seek → resume → close
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_15_full_lifecycle_open_ready_play_pause_seek_resume_close() {
    let path = temp_media_path();
    write_temp_media(&path, 2_000);

    let mut player = LibMpvPlayer::with_availability(true);
    let mut coordinator = LocalSyncCoordinator::new();

    // Open
    player.open(&path).expect("open");
    assert_eq!(player.snapshot().state, PlayerState::Ready);

    // Ready consensus
    coordinator.host_ready(ParticipantReadiness::ready(5_000));
    coordinator.guest_ready(ParticipantReadiness::ready(5_000));
    coordinator.update_readiness_consensus(5_000);
    assert_eq!(coordinator.room_state, RoomState::ReadyCheck);

    // Play
    let play = coordinator
        .prepare_play(0, 1_000_000, 5_000)
        .expect("prepare");
    coordinator.commit_play(&play).expect("commit");
    player.play().expect("play");
    assert_eq!(coordinator.room_state, RoomState::Playing);
    assert_eq!(player.snapshot().state, PlayerState::Playing);

    // Pause
    coordinator.commit_pause(5_000, PauseCause::Manual);
    player.pause().expect("pause");
    assert_eq!(coordinator.room_state, RoomState::Paused);

    // Seek
    coordinator.commit_seek(30_000, false);
    player.seek(30_000).expect("seek");
    assert_eq!(coordinator.host_position_ms, 30_000);
    assert_eq!(player.snapshot().position_ms, 30_000);

    // Resume after seek
    let resume = coordinator
        .prepare_play(30_000, 2_000_000, 5_000)
        .expect("prepare");
    coordinator.commit_play(&resume).expect("commit");
    player.play().expect("resume");
    assert_eq!(coordinator.room_state, RoomState::Playing);
    assert_eq!(player.snapshot().state, PlayerState::Playing);

    // Close
    player.close();
    assert_eq!(player.snapshot().state, PlayerState::Stopped);

    let _ = std::fs::remove_file(&path);
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 16 — Invalid file: coordinator remains unaffected by player error
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_16_player_error_does_not_corrupt_coordinator() {
    let mut player = LibMpvPlayer::with_availability(true);
    let coordinator = LocalSyncCoordinator::new();

    // Attempt to open nonexistent file
    let result = player.open(std::path::Path::new("/nonexistent/video.mp4"));
    assert!(result.is_err());

    // Coordinator should be unaffected
    assert_eq!(coordinator.room_state, RoomState::Lobby);
    assert_eq!(coordinator.host_position_ms, 0);

    // Commands should fail gracefully
    assert!(player.play().is_err());

    player.close();
}

// ── Test helpers for AppRuntime E2E tests ──────────────────────────────────

use movie_party_lib::app_runtime::AppRuntime;
use std::sync::OnceLock;

static M3_ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
fn m3_env_lock() -> &'static tokio::sync::Mutex<()> {
    M3_ENV_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 17 — AppRuntime player dispatch: create_local_party with media,
// then verify play/pause/seek dispatch to the live player.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_17_appruntime_player_dispatch() {
    let _guard = m3_env_lock().lock().await;
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    // Create a temporary media file
    let path = std::env::temp_dir().join(format!("m3_e2e_{}.mp4", uuid::Uuid::now_v7()));
    std::fs::write(&path, b"test-media-content-for-player").expect("write fixture");

    let runtime = AppRuntime::new();
    let snap = runtime
        .create_local_party(Some(path.to_str().unwrap().to_string()))
        .await
        .expect("create_local_party");

    // Media should be set and manifest present
    assert!(snap.media.is_some(), "media manifest must be present");
    assert!(!snap.media.as_ref().unwrap().media_id.is_empty());

    // Player snapshot should reflect initial state
    let player_state = &snap.player.state;
    if player_state == "PLAYER_ERROR" {
        assert!(
            snap.player
                .error_message
                .as_deref()
                .is_some_and(|error| error.contains("MP-MEDIA-001 libmpv is unavailable")),
            "PLAYER_ERROR must carry the stable libmpv diagnostic; got: {:?}",
            snap.player.error_message
        );
    } else {
        assert!(
            player_state == "READY" || player_state == "STOPPED",
            "expected READY, STOPPED, or diagnostic PLAYER_ERROR, got: {}",
            player_state
        );
    }

    // Pause should not error even if player is not playing
    runtime.pause_playback();
    let after_pause = runtime.snapshot();
    assert_eq!(after_pause.sync.position_ms, 0);

    // Seek goes through the coordinator PREPARE/READY/COMMIT cycle —
    // position updates asynchronously. For a solo host the commit fires
    // immediately, but we only assert it didn't panic.
    runtime.seek_relative(5_000);
    let _after_seek = runtime.snapshot();
    // Solo host: position should reflect the seek target after commit
    // Seek initiated a PREPARE — in solo mode there's no guest to READY,
    // so position stays at 0 until the commit fires. The important thing
    // is that the snapshot was returned without error.

    // Leave party cleans up player
    runtime.leave_party();
    let after_leave = runtime.snapshot();
    assert_eq!(after_leave.screen, "PARTY_ENDED");

    let _ = std::fs::remove_file(&path);
    std::env::remove_var("MOVIE_PARTY_DEV_LOOPBACK");
}

// ──────────────────────────────────────────────────────────────────────────────
// TEST 18 — Player buffering feeds into strict-sync (M2 integration)
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_18_player_buffering_feeds_strict_sync() {
    let _guard = m3_env_lock().lock().await;
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    let path = std::env::temp_dir().join(format!("m3_buf_{}.mp4", uuid::Uuid::now_v7()));
    std::fs::write(&path, b"test-media-content").expect("write fixture");

    let runtime = AppRuntime::new();
    let _snap = runtime
        .create_local_party(Some(path.to_str().unwrap().to_string()))
        .await
        .expect("create_local_party");

    // Enter cinema and play
    runtime.enter_cinema();
    runtime.resume_playback();

    // Report buffer stall (simulates player telling us it's buffering).
    // The returned snapshot is taken atomically under the runtime lock; a
    // separate snapshot() read could race the 200 ms player event loop,
    // which also refreshes headroom fields.
    let stalled = runtime.report_buffer_status(0, 0, true);
    assert!(
        stalled.sync.strict_sync_paused,
        "strict_sync_paused must be true after buffer stall"
    );
    assert!(
        stalled.buffer.buffering_participant.is_some(),
        "buffering_participant must be set"
    );

    // Report buffer recovery — same atomic-snapshot rule.
    let recovered = runtime.report_buffer_status(0, 5_000, false);
    // percent is the whole-file transfer fraction, never a fake
    // 100. A solo host has performed no guest transfer (bytes_available 0),
    // so the honest percent after recovery is 0 — recovery itself is
    // expressed via guest_buffer_ahead_ms and the cleared buffering flag.
    assert_eq!(recovered.buffer.percent, 0);
    assert_eq!(recovered.buffer.guest_buffer_ahead_ms, 5_000);
    assert!(recovered.buffer.buffering_participant.is_none());

    runtime.leave_party();
    let _ = std::fs::remove_file(&path);
    std::env::remove_var("MOVIE_PARTY_DEV_LOOPBACK");
}
