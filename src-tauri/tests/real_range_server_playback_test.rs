//! Real libmpv playing a film **through the loopback range server**.
//!
//! This is the path a guest actually uses for Local Perfect: mpv opens
//! `http://127.0.0.1:<port>/media/<id>?token=…`, the range server answers from
//! the sparse cache, and the transfer worker fills it on demand. The response is
//! bounded (`max_response_bytes`) so a film is never allocated whole, which means
//! mpv must handle a 206 that covers less than it asked for and continue from
//! the returned `Content-Range`.
//!
//! Nothing covered that interaction before: the unit tests use a raw HTTP
//! client, and the other real-* tests open a local file path. If libavformat
//! mishandled a short 206, guest playback would stall and no test would say so.
//!
//! macOS-only, like the other `real_*` tests: it loads the bundled
//! `mpv_runtime/libmpv.dylib` directly. Video and audio use mpv's `null`
//! drivers, so it runs headless.

#![cfg(target_os = "macos")]

mod common;

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use movie_party_lib::media::cache::SparseCache;
use movie_party_lib::media::manifest::build_manifest;
use movie_party_lib::media::stream::range_server::{
    start_range_server, ChunkWake, RangeServerConfig,
};
use movie_party_lib::media::transfer::ChunkDemandHandle;

type MpvHandle = *mut c_void;

const MPV_FORMAT_DOUBLE: c_int = 5;
/// Deliberately far below the fixture's size so playback needs several windows.
const RESPONSE_CAP_BYTES: u64 = 16 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bundled_libmpv_plays_a_film_through_bounded_range_responses() {
    let (lib_path, fixture) = common::require_runtime_and_fixture();
    let manifest = build_manifest(&fixture).expect("manifest");
    assert!(
        manifest.file_size > RESPONSE_CAP_BYTES * 2,
        "the fixture must need more than one bounded response ({} bytes)",
        manifest.file_size
    );

    // A cache that is already complete, so this test is about serving and
    // playback, not about transfer.
    let root = std::env::temp_dir().join(uuid::Uuid::now_v7().to_string());
    std::fs::create_dir_all(&root).expect("temp dir");
    let mut cache = SparseCache::open(&root, manifest.clone()).expect("cache");
    let file = std::fs::read(&fixture).expect("read fixture");
    for index in 0..manifest.chunk_count {
        let start = (index * manifest.chunk_size) as usize;
        let end = (((index + 1) * manifest.chunk_size).min(manifest.file_size)) as usize;
        cache.write_chunk(index, &file[start..end]).expect("chunk");
    }
    let cache = Arc::new(tokio::sync::Mutex::new(cache));

    let server = start_range_server(RangeServerConfig {
        manifest: manifest.clone(),
        cache,
        session_token: "range-playback".to_string(),
        chunk_wake: Arc::new(ChunkWake::new()),
        demand: ChunkDemandHandle::new(),
        wait_timeout_ms: 2_000,
        connection_timeout_ms: 5_000,
        max_response_bytes: RESPONSE_CAP_BYTES,
    })
    .await
    .expect("range server");

    let lib = unsafe { libloading::Library::new(&lib_path) }.expect("libmpv");
    type CreateFn = unsafe extern "C" fn() -> MpvHandle;
    type SetOptionFn = unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int;
    type InitializeFn = unsafe extern "C" fn(MpvHandle) -> c_int;
    type CommandFn = unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int;
    type GetPropFn = unsafe extern "C" fn(MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
    type WaitEventFn = unsafe extern "C" fn(MpvHandle, f64) -> *const c_void;
    type TerminateFn = unsafe extern "C" fn(MpvHandle);

    #[allow(clippy::explicit_auto_deref)]
    let mpv_create: &CreateFn = unsafe { &*lib.get(b"mpv_create\0").expect("mpv_create") };
    #[allow(clippy::explicit_auto_deref)]
    let mpv_set_option_string: &SetOptionFn = unsafe {
        &*lib
            .get(b"mpv_set_option_string\0")
            .expect("mpv_set_option_string")
    };
    #[allow(clippy::explicit_auto_deref)]
    let mpv_initialize: &InitializeFn =
        unsafe { &*lib.get(b"mpv_initialize\0").expect("mpv_initialize") };
    #[allow(clippy::explicit_auto_deref)]
    let mpv_command: &CommandFn = unsafe { &*lib.get(b"mpv_command\0").expect("mpv_command") };
    #[allow(clippy::explicit_auto_deref)]
    let mpv_get_property: &GetPropFn =
        unsafe { &*lib.get(b"mpv_get_property\0").expect("mpv_get_property") };
    #[allow(clippy::explicit_auto_deref)]
    let mpv_wait_event: &WaitEventFn =
        unsafe { &*lib.get(b"mpv_wait_event\0").expect("mpv_wait_event") };
    #[allow(clippy::explicit_auto_deref)]
    let mpv_terminate_destroy: &TerminateFn = unsafe {
        &*lib
            .get(b"mpv_terminate_destroy\0")
            .expect("mpv_terminate_destroy")
    };

    let handle = unsafe { mpv_create() };
    assert!(!handle.is_null(), "mpv_create returned null");
    for (name, value) in [
        ("vo", "null"),
        ("ao", "null"),
        ("terminal", "yes"),
        ("msg-level", "all=v"),
    ] {
        let c_name = CString::new(name).unwrap();
        let c_value = CString::new(value).unwrap();
        let rc = unsafe { mpv_set_option_string(handle, c_name.as_ptr(), c_value.as_ptr()) };
        assert_eq!(rc, 0, "setting {name}={value} failed");
    }
    assert_eq!(unsafe { mpv_initialize(handle) }, 0, "mpv_initialize");
    // `terminal=yes` + `msg-level=all=v` make a failed load explain itself in
    // the captured output (this is how "Protocol not found. Make sure FFmpeg is
    // compiled with networking support" was found). libtest only shows it when
    // the test fails, so a passing run stays quiet.

    let property = |name: &str| -> Option<f64> {
        let c_name = CString::new(name).unwrap();
        let mut value: f64 = 0.0;
        let rc = unsafe {
            mpv_get_property(
                handle,
                c_name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut value as *mut _ as *mut c_void,
            )
        };
        (rc == 0).then_some(value)
    };

    let url = CString::new(server.media_url.as_str()).unwrap();
    let loadfile = CString::new("loadfile").unwrap();
    let replace = CString::new("replace").unwrap();
    let args = [
        loadfile.as_ptr(),
        url.as_ptr(),
        replace.as_ptr(),
        ptr::null(),
    ];
    assert_eq!(
        unsafe { mpv_command(handle, args.as_ptr()) },
        0,
        "loadfile {} failed",
        server.media_url
    );

    // Probing the container needs the moov atom; if the short 206 responses
    // confused libavformat, this is where it would show up.
    let probe_deadline = Instant::now() + Duration::from_secs(10);
    let duration = loop {
        unsafe { mpv_wait_event(handle, 0.05) };
        if let Some(duration) = property("duration").filter(|value| *value > 1.0) {
            break duration;
        }
        assert!(
            Instant::now() < probe_deadline,
            "mpv never learned the duration over the bounded range server \
             (media_url={})",
            server.media_url
        );
    };

    let set = CString::new("set").unwrap();
    let pause = CString::new("pause").unwrap();
    let no = CString::new("no").unwrap();
    let args = [set.as_ptr(), pause.as_ptr(), no.as_ptr(), ptr::null()];
    assert_eq!(unsafe { mpv_command(handle, args.as_ptr()) }, 0, "play");

    // Give the player a moment to pull and decode a few windows.
    let start = Instant::now();
    let mut advanced = 0.0_f64;
    while start.elapsed() < Duration::from_secs(6) {
        unsafe { mpv_wait_event(handle, 0.1) };
        advanced = property("time-pos").unwrap_or(0.0);
        if advanced > 0.3 {
            break;
        }
    }
    assert!(
        advanced > 0.3,
        "playback did not advance over the range server (time-pos={advanced}, \
         duration={duration}, media_url={})",
        server.media_url
    );

    // A seek far from the current window exercises the on-demand path again.
    let seek = CString::new("seek").unwrap();
    let target = CString::new(format!("{:.3}", duration / 2.0)).unwrap();
    let absolute = CString::new("absolute").unwrap();
    let args = [
        seek.as_ptr(),
        target.as_ptr(),
        absolute.as_ptr(),
        ptr::null(),
    ];
    assert_eq!(unsafe { mpv_command(handle, args.as_ptr()) }, 0, "seek");

    let seek_deadline = Instant::now() + Duration::from_secs(6);
    loop {
        unsafe { mpv_wait_event(handle, 0.1) };
        let position = property("time-pos").unwrap_or(0.0);
        if (position - duration / 2.0).abs() < 2.0 {
            break;
        }
        assert!(
            Instant::now() < seek_deadline,
            "seek to {:.2}s never took effect (time-pos={position})",
            duration / 2.0
        );
    }

    unsafe { mpv_terminate_destroy(handle) };
    std::mem::forget(lib);
    server.shutdown();
    let _ = std::fs::remove_dir_all(&root);

    eprintln!(
        "PASS: libmpv played and seeked through bounded range responses \
         (cap={RESPONSE_CAP_BYTES} bytes, file={} bytes)",
        manifest.file_size
    );
}
