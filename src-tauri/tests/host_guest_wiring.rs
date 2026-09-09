use base64::Engine;
use movie_party_lib::app_runtime::{AppRuntime, AppSnapshot, SnapshotSink};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
struct RecordingSink {
    snapshots: Arc<Mutex<Vec<AppSnapshot>>>,
}

impl SnapshotSink for RecordingSink {
    fn emit(&self, snapshot: &AppSnapshot) {
        self.snapshots.lock().unwrap().push(snapshot.clone());
    }
}

fn make_tampered_invite(invite_url: &str) -> String {
    let hash_pos = invite_url.find('#').expect("invite must have fragment");
    let prefix = &invite_url[..=hash_pos];
    let descriptor_b64 = &invite_url[hash_pos + 1..];

    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(descriptor_b64)
        .expect("valid base64url descriptor");

    let mut invite: movie_party_lib::room::MoviePartyInvite =
        serde_json::from_slice(&json).expect("valid invite JSON");

    // Replace with a well-formed 32-byte base64url value that is definitively wrong
    invite.server_certificate_fingerprint =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xABu8; 32]);

    let tampered_json = serde_json::to_string(&invite).expect("serialize tampered invite");
    let tampered_descriptor =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(tampered_json.as_bytes());

    format!("{}{}", prefix, tampered_descriptor)
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_host_guest_wiring() {
    // Enable deterministic development loopback for M1 tests
    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");

    let host_sink = Arc::new(RecordingSink::default());
    let host = AppRuntime::new_with_emitter(Some(host_sink.clone()));

    // Create a temporary dummy file to act as the host's local media
    let temp_dir = std::env::temp_dir();
    let media_path = temp_dir.join("test_movie.mp4");
    std::fs::write(&media_path, b"dummy media content").unwrap();

    let host_snapshot = host
        .create_local_party(Some(media_path.to_string_lossy().to_string()))
        .await
        .expect("host should create party");

    let invite_code = host_snapshot.room.invite_code.expect("invite code");
    println!("INVITE CODE: {}", invite_code);
    assert!(invite_code.starts_with("movieparty://join/"));

    let guest_sink = Arc::new(RecordingSink::default());
    let guest = AppRuntime::new_with_emitter(Some(guest_sink.clone()));

    let guest_snapshot = guest
        .join_party(invite_code)
        .await
        .expect("guest should join party");

    assert!(guest_snapshot.network.connected);
    assert_eq!(guest_snapshot.room.state, "LOBBY");

    // Wait for host to observe the authenticated peer (background tokio task)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let host_peer_id: Option<String>;
    loop {
        let snapshots = host_sink.snapshots.lock().unwrap();
        if let Some(s) = snapshots.last() {
            if s.network.connected && s.room.state == "LOBBY" {
                if let Some(peer) = s.participants.iter().find(|p| p.role == "Guest") {
                    host_peer_id = Some(peer.id.clone());
                    break;
                }
            }
        }
        drop(snapshots);
        if tokio::time::Instant::now() >= deadline {
            panic!("host did not observe peer authentication within timeout");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let host_peer_id = host_peer_id.expect("guest participant must be visible to host after auth");
    // "<host>" and "<Guest>" placeholders should never appear in real wire-up
    assert_ne!(host_peer_id, "<host>");
    assert_ne!(host_peer_id, "<Guest>");

    // ID chain: host_peer_id (from host polling) must equal guest's own ID
    let guest_self = guest_snapshot
        .participants
        .iter()
        .find(|p| p.role == "Guest")
        .expect("guest must see self");
    assert_eq!(host_peer_id, guest_self.id); // Host and guest agree on guest device ID

    // Guest must see a non-placeholder Host peer
    let guest_peer = guest_snapshot
        .participants
        .iter()
        .find(|p| p.role == "Host")
        .expect("guest must see host peer");
    assert_ne!(guest_peer.id, "<host>");
    assert_ne!(guest_peer.id, "<Guest>");

    // Host self-view must match what the guest sees
    let host_self = host_snapshot
        .participants
        .iter()
        .find(|p| p.role == "Host")
        .expect("host must see self");
    assert_eq!(guest_peer.id, host_self.id); // Guest and host agree on host device ID
    assert_eq!(guest_peer.display_name, host_self.display_name); // Display names match

    assert_eq!(host_snapshot.room.room_id, guest_snapshot.room.room_id);

    // Cleanup
    let _ = std::fs::remove_file(media_path);
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_rejects_tampered_invite_certificate_fingerprint() {
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

    std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");
    let _env_guard = ScopedEnv {
        key: "MOVIE_PARTY_DEV_LOOPBACK",
        prev: std::env::var_os("MOVIE_PARTY_DEV_LOOPBACK"),
    };

    let host = AppRuntime::new();
    let host_snapshot = host
        .create_local_party(None)
        .await
        .expect("host should create party");
    let invite_code = host_snapshot.room.invite_code.expect("invite code");

    let tampered_invite = make_tampered_invite(&invite_code);
    assert!(tampered_invite.starts_with("movieparty://join/"));
    assert_ne!(
        tampered_invite, invite_code,
        "tampered invite must differ from original"
    );

    let guest = AppRuntime::new();
    let join_result = guest.join_party(tampered_invite).await;

    assert!(
        join_result.is_err(),
        "join_party must reject tampered invite fingerprint"
    );

    // Give any background task a moment to run (it should not produce a connected state)
    tokio::time::sleep(Duration::from_millis(300)).await;

    let guest_snapshot = guest.snapshot();
    assert!(
        !guest_snapshot.network.connected,
        "guest must not show connected after tampered-invite rejection"
    );

    // Host must not gain an authenticated peer from the failed join attempt
    let host_after = host.snapshot();
    assert!(
        !host_after.network.connected || host_after.participants.iter().all(|p| p.role != "Guest"),
        "host must not show a Guest peer from a rejected tampered invite"
    );
}
