// ─── M4 Closure tests ──────────────────────────────────────────────────────────
// AppRuntime identity restart, schedule CRUD, real scheduler worker, notification
// abstraction, and retention runtime paths.
// ──────────────────────────────────────────────────────────────────────────────

use std::sync::OnceLock;

use tokio::sync::Mutex;

use move_party_lib::app_runtime::AppRuntime;
use move_party_lib::storage::sqlite::MovePartyDb;

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

fn temp_db_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("m4c_{tag}_{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("move_party.db")
}

// ──────────────────────────────────────────────────────────────────────────────
// M4.1 — AppRuntime identity restoration across a full runtime restart.
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn appruntime_identity_restart_matches_exact_device_id() {
    let _guard = env_lock().lock();
    let db_path = temp_db_path("identity");

    let device_id_a;
    let public_key_a;
    {
        let runtime_a = AppRuntime::new();
        runtime_a.init_db_at_path(&db_path);
        let identity_a = runtime_a.device_identity_for_test();
        device_id_a = identity_a.device_id.clone();
        public_key_a = identity_a.public_key_base64();
        assert!(
            !device_id_a.is_empty(),
            "identity must be created exactly once"
        );
    }

    // Runtime B uses the same database — the exact same identity must come back.
    let (device_id_b, public_key_b);
    {
        let runtime_b = AppRuntime::new();
        runtime_b.init_db_at_path(&db_path);
        let identity_b = runtime_b.device_identity_for_test();
        device_id_b = identity_b.device_id.clone();
        public_key_b = identity_b.public_key_base64();
    }

    assert_eq!(
        device_id_a, device_id_b,
        "device id must survive an AppRuntime restart"
    );
    assert_eq!(
        public_key_a, public_key_b,
        "signing key must be restored from the persisted seed"
    );

    // Exactly one identity row exists (never created a replacement).
    let db = MovePartyDb::open(&db_path).expect("open");
    let stored = db.get_identity().expect("get").expect("identity");
    assert_eq!(stored.device_id, device_id_b);
    assert!(stored.signing_key_seed.is_some(), "seed must be persisted");

    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

#[test]
fn appruntime_identity_is_created_exactly_once() {
    let _guard = env_lock().lock();
    let db_path = temp_db_path("identity_once");

    let (id1, id2);
    {
        let runtime = AppRuntime::new();
        runtime.init_db_at_path(&db_path);
        id1 = runtime.device_identity_for_test().device_id.clone();
        // Second init on the same runtime must not create a replacement.
        runtime.init_db_at_path(&db_path);
        id2 = runtime.device_identity_for_test().device_id.clone();
    }

    assert_eq!(id1, id2, "init_db must never replace an existing identity");
    let db = MovePartyDb::open(&db_path).expect("open");
    assert_eq!(db.list_schedules().expect("schedules").len(), 0);
    assert!(db.get_identity().expect("get").is_some());

    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

// ──────────────────────────────────────────────────────────────────────────────
// M4.2 — Schedule CRUD through AppRuntime (persist real data, survive restart).
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn appruntime_schedule_crud_persists_and_survives_restart() {
    let _guard = env_lock().lock();
    let db_path = temp_db_path("schedule_crud");

    let created_id;
    {
        let runtime = AppRuntime::new();
        runtime.init_db_at_path(&db_path);
        let now_ms = now_utc_ms();
        let scheduled_ms = now_ms + 3_600_000;
        let preload_ms = now_ms + 1_800_000;

        created_id = runtime
            .create_schedule("room-1", "media-1", scheduled_ms, preload_ms, "guest-1")
            .expect("create");
        assert!(!created_id.is_empty());

        let list = runtime.list_schedules().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].media_id, "media-1");
        assert_eq!(list[0].scheduled_start_utc_ms, scheduled_ms);
        assert_eq!(list[0].guest_device_id, "guest-1");
        assert_eq!(list[0].status, "Planned");
    }

    // Restart: schedule must survive.
    {
        let runtime = AppRuntime::new();
        runtime.init_db_at_path(&db_path);

        let list = runtime.list_schedules().expect("list2");
        assert_eq!(list.len(), 1, "schedule must survive restart");
        assert_eq!(list[0].schedule_id, created_id);

        // Update: media and timing change.
        runtime
            .update_schedule_media(&created_id, "media-2")
            .expect("update");
        let updated = runtime.list_schedules().expect("list3");
        assert_eq!(updated[0].media_id, "media-2");

        // Delete prevents later execution.
        runtime.delete_schedule(&created_id).expect("delete");
        assert!(runtime.list_schedules().expect("list4").is_empty());
    }

    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

#[test]
fn appruntime_schedule_create_validates_dto() {
    let _guard = env_lock().lock();
    let db_path = temp_db_path("schedule_validate");
    let runtime = AppRuntime::new();
    runtime.init_db_at_path(&db_path);

    let now_ms = now_utc_ms();
    // Empty media id is invalid.
    assert!(runtime
        .create_schedule(
            "room-1",
            "",
            now_ms + 3_600_000,
            now_ms + 1_800_000,
            "guest-1"
        )
        .is_err());
    // Preload after scheduled start is invalid.
    assert!(runtime
        .create_schedule(
            "room-1",
            "media-1",
            now_ms + 3_600_000,
            now_ms + 7_200_000,
            "guest-1"
        )
        .is_err());
    assert!(runtime.list_schedules().expect("list").is_empty());

    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

// ──────────────────────────────────────────────────────────────────────────────
// M4.3 — Real scheduler worker: deadline wait, execution, once-only, reschedule,
// delete prevention, overdue recovery after restart.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_future_schedule_does_not_run_early() {
    let _guard = env_lock().lock().await;
    let db_path = temp_db_path("sched_future");
    let runtime = AppRuntime::new();
    runtime.init_db_at_path(&db_path);

    let now_ms = now_utc_ms();
    let scheduled_ms = now_ms + 3_600_000;
    let preload_ms = now_ms + 1_800_000;
    let id = runtime
        .create_schedule(
            "room-1",
            "media-future",
            scheduled_ms,
            preload_ms,
            "guest-1",
        )
        .expect("create");

    runtime.spawn_scheduler_worker_for_test(now_utc_ms(), 120);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let list = runtime.list_schedules().expect("list");
    let s = list.iter().find(|s| s.schedule_id == id).expect("schedule");
    assert_eq!(s.status, "Planned", "future schedule must not run early");

    runtime.stop_scheduler_for_test();
    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_due_schedule_executes_once() {
    let _guard = env_lock().lock().await;
    let db_path = temp_db_path("sched_due");
    let runtime = AppRuntime::new();
    runtime.init_db_at_path(&db_path);

    let now_ms = now_utc_ms();
    let preload_ms = now_ms + 200;
    let scheduled_ms = now_ms + 3_600_000;
    let id = runtime
        .create_schedule("room-1", "media-due", scheduled_ms, preload_ms, "guest-1")
        .expect("create");

    // Preload deadline has passed → executes immediately (exactly once).
    runtime.spawn_scheduler_worker_for_test(now_utc_ms(), 60);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    let list = runtime.list_schedules().expect("list");
    let s = list.iter().find(|s| s.schedule_id == id).expect("schedule");
    assert_eq!(
        s.status, "Transferring",
        "due schedule must transition to Transferring: got {}",
        s.status
    );
    assert_eq!(
        runtime.preload_execution_count(),
        1,
        "due schedule must execute exactly once"
    );

    runtime.stop_scheduler_for_test();
    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_overdue_schedule_executes_after_restart() {
    let _guard = env_lock().lock().await;
    let db_path = temp_db_path("sched_overdue");

    let id;
    {
        let runtime = AppRuntime::new();
        runtime.init_db_at_path(&db_path);
        let now_ms = now_utc_ms();
        id = runtime
            .create_schedule(
                "room-1",
                "media-overdue",
                now_ms + 3_600_000,
                now_ms - 1_000,
                "guest-1",
            )
            .expect("create");
    }

    // Restart: the overdue schedule (still Planned) must be recovered and
    // executed by the new scheduler.
    let runtime = AppRuntime::new();
    runtime.init_db_at_path(&db_path);
    let overdue = runtime.list_schedules().expect("list");
    assert_eq!(overdue[0].status, "Planned", "still Planned before run");

    runtime.spawn_scheduler_worker_for_test(now_utc_ms(), 60);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    let list = runtime.list_schedules().expect("list2");
    let s = list.iter().find(|s| s.schedule_id == id).expect("schedule");
    assert_eq!(s.status, "Transferring", "overdue executes after restart");
    assert_eq!(runtime.preload_execution_count(), 1);

    runtime.stop_scheduler_for_test();
    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_update_changes_deadline_and_delete_prevents_execution() {
    let _guard = env_lock().lock().await;
    let db_path = temp_db_path("sched_update_delete");
    let runtime = AppRuntime::new();
    runtime.init_db_at_path(&db_path);

    let now_ms = now_utc_ms();
    let id = runtime
        .create_schedule(
            "room-1",
            "media-ud",
            now_ms + 3_600_000,
            now_ms + 2_000,
            "guest-1",
        )
        .expect("create");

    // Push the preload deadline far into the future; the schedule must NOT
    // execute at the old deadline.
    runtime
        .update_schedule_preload(&id, now_ms + 3_000_000)
        .expect("update");
    runtime.spawn_scheduler_worker_for_test(now_utc_ms(), 100);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let list = runtime.list_schedules().expect("list");
    let s = list.iter().find(|s| s.schedule_id == id).expect("schedule");
    assert_eq!(
        s.status, "Planned",
        "update must shift the execution deadline"
    );
    assert_eq!(runtime.preload_execution_count(), 0);

    // Delete the schedule — it must never execute.
    runtime.delete_schedule(&id).expect("delete");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(runtime.preload_execution_count(), 0, "deleted must not run");

    runtime.stop_scheduler_for_test();
    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

// ──────────────────────────────────────────────────────────────────────────────
// M4.4 — Notifications: fake notifier records events; failure is recoverable.
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn scheduler_notifications_use_mock_and_recover_on_failure() {
    let _guard = env_lock().lock();
    let db_path = temp_db_path("notify");

    let runtime = AppRuntime::new_with_notifier(move_party_lib::notifications::FakeNotifier::new());
    runtime.init_db_at_path(&db_path);
    let now_ms = now_utc_ms();
    let id = runtime
        .create_schedule(
            "room-1",
            "media-notify",
            now_ms + 3_600_000,
            now_ms - 1,
            "guest-1",
        )
        .expect("create");

    // Overdue preload detection must send a notification through the mock.
    let notified = runtime.check_overdue_schedules_with_notifier();
    assert!(notified, "overdue schedule must be notified");

    // A failing notifier must not crash the scheduler or the runtime.
    let failing_runtime =
        AppRuntime::new_with_notifier(move_party_lib::notifications::FailingNotifier);
    failing_runtime.init_db_at_path(&db_path);
    let _ = failing_runtime.check_overdue_schedules_with_notifier();
    assert_eq!(
        failing_runtime.list_schedules().expect("list")[0].schedule_id,
        id,
        "runtime must stay usable after a notification failure"
    );

    let _ = std::fs::remove_dir_all(db_path.parent().expect("parent"));
}

// ──────────────────────────────────────────────────────────────────────────────
// M4.5 — Retention runtime paths: Keep / Remove / Save As never touch the
// host's original source file.
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn retention_runtime_keep_remove_save_as_preserve_host_source() {
    let _guard = env_lock().lock();
    let db_path = temp_db_path("retention");

    let root = db_path.parent().expect("parent");
    let host_source = root.join("host_original.mkv");
    std::fs::write(&host_source, b"original host file").expect("host source");

    let runtime = AppRuntime::new();
    runtime.init_db_at_path(&db_path);
    runtime.init_cache_root_for_test(root.to_path_buf());

    let media_id = "ret-media";
    let cache_dir = root.join(media_id);
    std::fs::create_dir_all(&cache_dir).expect("cache dir");
    std::fs::write(
        cache_dir.join(move_party_lib::media::cache::CACHE_DATA_FILE),
        b"cached movie bytes",
    )
    .expect("cached data");
    runtime
        .register_cached_media(media_id, "movie.mkv", 18, "hash")
        .expect("register");

    // Keep: cache remains.
    runtime.retention_keep(media_id).expect("keep");
    assert!(cache_dir.exists(), "Keep must retain the cache");
    assert!(
        host_source.exists(),
        "Keep must never touch the host source"
    );

    // Remove: cache deleted, host source untouched.
    runtime.retention_remove(media_id).expect("remove");
    assert!(
        !cache_dir.exists(),
        "Remove must delete only Move Party cache"
    );
    assert!(
        host_source.exists(),
        "Remove must never delete the host source"
    );
    assert!(
        runtime.list_cached_media().expect("list").is_empty(),
        "cache metadata must be removed"
    );

    // Save As: cache copied to destination, host source untouched.
    std::fs::create_dir_all(&cache_dir).expect("cache dir 2");
    std::fs::write(
        cache_dir.join(move_party_lib::media::cache::CACHE_DATA_FILE),
        b"cached movie bytes",
    )
    .expect("cached data 2");
    runtime
        .register_cached_media(media_id, "movie.mkv", 18, "hash")
        .expect("register 2");
    let destination = root.join("exported.mkv");
    runtime
        .retention_save_as(media_id, &destination)
        .expect("save as");
    assert_eq!(
        std::fs::read(&destination).expect("export"),
        b"cached movie bytes"
    );
    assert!(
        host_source.exists(),
        "Save As must never touch the host source"
    );

    let _ = std::fs::remove_dir_all(root);
}

fn now_utc_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64
}
