use std::sync::OnceLock;
use tokio::sync::Mutex;

use move_party_lib::app_runtime::AppRuntime;
use move_party_lib::media::cache::CACHE_DATA_FILE;
use move_party_lib::media::transfer::validate_chunk_packet;
use move_party_lib::storage::sqlite::{MovePartyDb, StoredSchedule};
use move_party_lib::storage::{apply_retention_decision, RetentionDecision};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_e2e_host_guest_manifest_transfer() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MOVE_PARTY_DEV_LOOPBACK", "1");
    let path = std::env::temp_dir().join(format!("m3_e2e_{}.mkv", uuid::Uuid::now_v7()));
    let data: Vec<u8> = (0..2_500_000).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &data).expect("write fixture");
    let manifest = move_party_lib::media::manifest::build_manifest(&path).expect("build_manifest");
    assert!(manifest.chunk_count >= 2);

    let host = AppRuntime::new();
    let host_snap = host
        .create_local_party(Some(path.to_str().unwrap().to_string()))
        .await
        .expect("host");
    assert!(host_snap.media.is_some());

    let invite_code = host_snap.room.invite_code.clone().expect("invite");
    let guest = AppRuntime::new();
    guest.join_party(invite_code).await.expect("guest join");

    // Wait for the auto-fetch to complete
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if guest.snapshot().media.is_some() {
            break;
        }
    }

    let gs = guest.snapshot();
    if let Some(ref gm) = gs.media {
        assert_eq!(gm.media_id, manifest.media_id);
        assert_eq!(gm.file_size, manifest.file_size);
    } else {
        let result = guest.guest_fetch_media().await;
        let gs2 = guest.snapshot();
        assert!(
            gs2.media.is_some(),
            "manual fetch must succeed: {:?}",
            result.err()
        );
    }

    host.leave_party();
    guest.leave_party();
    let _ = std::fs::remove_file(&path);
    std::env::remove_var("MOVE_PARTY_DEV_LOOPBACK");
}

#[test]
fn test_corrupt_chunk_rejected() {
    use move_party_lib::media::manifest::{MediaManifest, QuickFingerprint};
    let m = MediaManifest {
        media_id: "ct".into(),
        filename: "m.bin".into(),
        file_size: 8,
        container: None,
        full_hash: "h".into(),
        quick_fingerprint: QuickFingerprint {
            file_size: 8,
            first_hash: "f".into(),
            last_hash: "l".into(),
        },
        chunk_size: 4,
        chunk_count: 2,
    };
    let v = move_party_lib::media::transfer::build_chunk_packet(&m, 0, b"abcd".to_vec()).unwrap();
    assert!(validate_chunk_packet(&m, &v).is_ok());
    let mut c = v.clone();
    c.payload[0] = b'x';
    assert!(validate_chunk_packet(&m, &c).is_err());
    let mut w = v.clone();
    w.media_id = "wrong".into();
    assert!(validate_chunk_packet(&m, &w).is_err());
    let mut b = v.clone();
    b.chunk_index = 99;
    assert!(validate_chunk_packet(&m, &b).is_err());
}

#[test]
fn test_identity_survives_db_reopen() {
    let p = std::env::temp_dir().join(format!("m4_id_{}.db", uuid::Uuid::now_v7()));
    {
        let db = MovePartyDb::open(&p).unwrap();
        db.upsert_identity(&move_party_lib::storage::sqlite::StoredIdentity {
            device_id: "dev-42".into(),
            display_name: "User".into(),
            public_key: "pk".into(),
            platform: "macos".into(),
            created_at_ms: 1700000000000,
        })
        .unwrap();
    }
    {
        let db = MovePartyDb::open(&p).unwrap();
        let i = db.get_identity().unwrap().unwrap();
        assert_eq!(i.device_id, "dev-42");
    }
    let _ = std::fs::remove_file(&p);
}

#[test]
fn test_schedule_crud_persists() {
    let p = std::env::temp_dir().join(format!("m4_sched_{}.db", uuid::Uuid::now_v7()));
    let s = StoredSchedule {
        schedule_id: "s1".into(),
        room_id: "r1".into(),
        media_id: "m1".into(),
        scheduled_start_utc_ms: 1700003600000,
        planned_preload_utc_ms: 1700002800000,
        guest_device_id: "g1".into(),
        status: "Planned".into(),
        created_at_ms: 1700000000000,
    };
    {
        let db = MovePartyDb::open(&p).unwrap();
        db.insert_schedule(&s).unwrap();
        assert_eq!(db.list_schedules().unwrap().len(), 1);
    }
    {
        let db = MovePartyDb::open(&p).unwrap();
        db.update_schedule_status("s1", "Preloading").unwrap();
        assert_eq!(db.list_schedules().unwrap()[0].status, "Preloading");
    }
    {
        let db = MovePartyDb::open(&p).unwrap();
        db.delete_schedule("s1").unwrap();
        assert!(db.list_schedules().unwrap().is_empty());
    }
    let _ = std::fs::remove_file(&p);
}

#[test]
fn test_preload_calculation_matches_prd() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let future = now_ms + 3_600_000;
    let p = MovePartyDb::calculate_preload_start(1_000_000_000, 5_000_000, future);
    assert_eq!(p, future - 1_180_000);
}

#[test]
fn test_zero_goodput_returns_scheduled_time() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let future = now_ms + 3_600_000;
    let p = MovePartyDb::calculate_preload_start(1_000_000_000, 0, future);
    assert_eq!(p, future);
}

#[test]
fn test_overdue_schedule_detected() {
    let p = std::env::temp_dir().join(format!("m4_ov_{}.db", uuid::Uuid::now_v7()));
    let db = MovePartyDb::open(&p).unwrap();
    db.insert_schedule(&StoredSchedule {
        schedule_id: "o1".into(),
        room_id: "r1".into(),
        media_id: "m1".into(),
        scheduled_start_utc_ms: 1700003600000,
        planned_preload_utc_ms: 1_000_000_000_000,
        guest_device_id: "g1".into(),
        status: "Planned".into(),
        created_at_ms: 1700000000000,
    })
    .unwrap();
    assert_eq!(db.overdue_schedules().unwrap().len(), 1);
    db.update_schedule_status("o1", "PreloadDue").unwrap();
    assert!(db.overdue_schedules().unwrap().is_empty());
    let _ = std::fs::remove_file(&p);
}

#[test]
fn test_retention_remove_preserves_source() {
    let root = std::env::temp_dir().join(format!("m4_ret_{}", uuid::Uuid::now_v7()));
    let md = root.join("media-id");
    std::fs::create_dir_all(&md).unwrap();
    std::fs::write(md.join(CACHE_DATA_FILE), b"cached").unwrap();
    std::fs::write(root.join("host.mkv"), b"original").unwrap();
    apply_retention_decision(
        &root,
        &md,
        &md.join(CACHE_DATA_FILE),
        RetentionDecision::Remove,
        None,
    )
    .unwrap();
    assert!(!md.exists());
    assert!(root.join("host.mkv").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_retention_save_as() {
    let root = std::env::temp_dir().join(format!("m4_sa_{}", uuid::Uuid::now_v7()));
    let md = root.join("media-id");
    std::fs::create_dir_all(&md).unwrap();
    std::fs::write(md.join(CACHE_DATA_FILE), b"cached").unwrap();
    std::fs::write(root.join("host.mkv"), b"original").unwrap();
    let dest = root.join("exported.mkv");
    apply_retention_decision(
        &root,
        &md,
        &md.join(CACHE_DATA_FILE),
        RetentionDecision::SaveAs,
        Some(&dest),
    )
    .unwrap();
    assert!(dest.exists());
    assert_eq!(std::fs::read(&dest).unwrap(), b"cached");
    assert!(root.join("host.mkv").exists());
    let _ = std::fs::remove_dir_all(&root);
}
