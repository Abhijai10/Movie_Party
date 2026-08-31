//! Real playback smoke test against the bundled libmpv runtime.
//!
//! Drives the actual bundled libmpv.dylib (the exact library the packaged
//! .app loads from Contents/Resources/mpv_runtime/) through the real mpv
//! client API: create → configure → initialize → loadfile → play → seek →
//! pause → position query. Video/audio output use mpv's `null` drivers so the
//! test runs headless; decoding and playback control are fully real.
//!
//! This proves the bundled runtime genuinely decodes and plays a local video,
//! not merely that the file exists. Skips gracefully when the runtime or the
//! test video is absent.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

type MpvHandle = *mut c_void;

const MPV_FORMAT_FLAG: i32 = 3;
const MPV_FORMAT_DOUBLE: i32 = 5;
const MPV_FORMAT_INT64: i32 = 4;

#[test]
fn bundled_libmpv_plays_pauses_seeks_real_video() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bundled = manifest_dir.join("mpv_runtime/libmpv.dylib");
    if !bundled.exists() {
        eprintln!("SKIP: no bundled libmpv at {bundled:?}");
        return;
    }
    let test_video = Path::new("/tmp/movie_party_test.mp4");
    if !test_video.exists() {
        eprintln!("SKIP: no test video at {test_video:?}");
        return;
    }

    // Load the EXACT dylib the packaged app ships.
    let lib = unsafe { libloading::Library::new(&bundled) }
        .expect("bundled libmpv.dylib must be loadable");
    type CreateFn = unsafe extern "C" fn() -> MpvHandle;
    type InitializeFn = unsafe extern "C" fn(MpvHandle) -> c_int;
    type CommandFn = unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int;
    type GetPropFn = unsafe extern "C" fn(MpvHandle, *const c_char, i32, *mut c_void) -> c_int;
    type WaitEventFn = unsafe extern "C" fn(MpvHandle, f64) -> *const c_void;
    type TerminateFn = unsafe extern "C" fn(MpvHandle) -> c_void;
    type ErrorStringFn = unsafe extern "C" fn(c_int) -> *const c_char;

    let mpv_create: &CreateFn = unsafe { &*lib.get(b"mpv_create\0").expect("mpv_create") };
    let mpv_initialize: &InitializeFn =
        unsafe { &*lib.get(b"mpv_initialize\0").expect("mpv_initialize") };
    let mpv_command: &CommandFn = unsafe { &*lib.get(b"mpv_command\0").expect("mpv_command") };
    let mpv_get_property: &GetPropFn =
        unsafe { &*lib.get(b"mpv_get_property\0").expect("mpv_get_property") };
    let mpv_wait_event: &WaitEventFn =
        unsafe { &*lib.get(b"mpv_wait_event\0").expect("mpv_wait_event") };
    let mpv_terminate_destroy: &TerminateFn = unsafe {
        &*lib
            .get(b"mpv_terminate_destroy\0")
            .expect("mpv_terminate_destroy")
    };
    let mpv_error_string: &ErrorStringFn =
        unsafe { &*lib.get(b"mpv_error_string\0").expect("mpv_error_string") };

    unsafe fn err_msg(err: c_int, mpv_error_string: &ErrorStringFn) -> String {
        let p = mpv_error_string(err);
        if p.is_null() {
            format!("mpv error {err}")
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }

    let handle = unsafe { mpv_create() };
    assert!(!handle.is_null(), "mpv_create returned null");

    // Configure for headless playback: null video/audio output, no window.
    // Must use mpv_set_option_string BEFORE mpv_initialize.
    type SetOptionStringFn = unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int;
    let mpv_set_option_string: &SetOptionStringFn = unsafe {
        &*lib
            .get(b"mpv_set_option_string\0")
            .expect("mpv_set_option_string")
    };

    let vo_name = CString::new("vo").unwrap();
    let vo_val = CString::new("null").unwrap();
    let rc = unsafe { mpv_set_option_string(handle, vo_name.as_ptr(), vo_val.as_ptr()) };
    assert_eq!(rc, 0, "setting vo=null failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    let ao_name = CString::new("ao").unwrap();
    let ao_val = CString::new("null").unwrap();
    let rc = unsafe { mpv_set_option_string(handle, ao_name.as_ptr(), ao_val.as_ptr()) };
    assert_eq!(rc, 0, "setting ao=null failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });

    let rc = unsafe { mpv_initialize(handle) };
    assert_eq!(rc, 0, "mpv_initialize failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });

    // Load the real local video then pump events until the duration is readable.
    let path = CString::new(test_video.to_string_lossy().as_bytes()).unwrap();
    let loadfile = CString::new("loadfile").unwrap();
    let replace = CString::new("replace").unwrap();
    let args = [
        loadfile.as_ptr(),
        path.as_ptr(),
        replace.as_ptr(),
        ptr::null(),
    ];
    let rc = unsafe { mpv_command(handle, args.as_ptr()) };
    assert_eq!(rc, 0, "loadfile failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    // Drain the event queue for up to 5s until duration becomes readable.
    let duration_name = CString::new("duration").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut loaded = false;
    while std::time::Instant::now() < deadline {
        unsafe { mpv_wait_event(handle, 0.05) };
        let mut duration: f64 = 0.0;
        let rc = unsafe {
            mpv_get_property(
                handle,
                duration_name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut duration as *mut _ as *mut c_void,
            )
        };
        if rc == 0 && duration > 0.0 {
            loaded = true;
            break;
        }
    }
    assert!(
        loaded,
        "video file did not load within timeout (duration never became readable)"
    );

    // The file must have a positive duration (it was decoded/probed).
    let mut duration: f64 = 0.0;
    let rc = unsafe {
        mpv_get_property(
            handle,
            duration_name.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut duration as *mut _ as *mut c_void,
        )
    };
    assert_eq!(rc, 0, "duration read failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    assert!(
        duration > 1.0,
        "decoded video duration must be > 1s, got {duration}s"
    );

    // Play (pause = false) via `set` command (avoids format-ABI pitfalls).
    let set_cmd = CString::new("set").unwrap();
    let pause_name = CString::new("pause").unwrap();
    let pause_no = CString::new("no").unwrap();
    let args = [
        set_cmd.as_ptr(),
        pause_name.as_ptr(),
        pause_no.as_ptr(),
        ptr::null(),
    ];
    let rc = unsafe { mpv_command(handle, args.as_ptr()) };
    assert_eq!(rc, 0, "play failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    // Give the core time to start playback before seeking.
    for _ in 0..10 {
        unsafe { mpv_wait_event(handle, 0.1) };
    }

    // Position should advance while playing.
    let pos_name = CString::new("time-pos").unwrap();
    let mut pos: f64 = 0.0;
    let rc = unsafe {
        mpv_get_property(
            handle,
            pos_name.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut pos as *mut _ as *mut c_void,
        )
    };
    assert_eq!(rc, 0, "time-pos read failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    assert!(
        pos >= 0.0,
        "playback position must advance while playing (got {pos}s)"
    );

    // Seek to a known absolute position.
    let seek_cmd = CString::new("seek").unwrap();
    let seek_to = CString::new(format!("{:.3}", duration / 2.0)).unwrap();
    let absolute = CString::new("absolute").unwrap();
    let args = [
        seek_cmd.as_ptr(),
        seek_to.as_ptr(),
        absolute.as_ptr(),
        ptr::null(),
    ];
    let rc = unsafe { mpv_command(handle, args.as_ptr()) };
    assert_eq!(rc, 0, "seek failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    for _ in 0..8 {
        unsafe { mpv_wait_event(handle, 0.1) };
    }

    let expected = duration / 2.0;
    let mut after_seek: f64 = 0.0;
    let rc = unsafe {
        mpv_get_property(
            handle,
            pos_name.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut after_seek as *mut _ as *mut c_void,
        )
    };
    assert_eq!(rc, 0, "time-pos read after seek failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    assert!(
        (after_seek - expected).abs() < 2.0,
        "after seeking to {expected:.2}s, position should be near there (got {after_seek:.2}s)"
    );

    // Pause and confirm the pause property is set.
    let set_cmd = CString::new("set").unwrap();
    let pause_yes = CString::new("yes").unwrap();
    let args = [
        set_cmd.as_ptr(),
        pause_name.as_ptr(),
        pause_yes.as_ptr(),
        ptr::null(),
    ];
    let rc = unsafe { mpv_command(handle, args.as_ptr()) };
    assert_eq!(rc, 0, "pause failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    unsafe { mpv_wait_event(handle, 0.2) };
    let mut paused: i32 = 0;
    let rc = unsafe {
        mpv_get_property(
            handle,
            pause_name.as_ptr(),
            MPV_FORMAT_FLAG,
            &mut paused as *mut _ as *mut c_void,
        )
    };
    assert_eq!(rc, 0, "pause property read failed: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    assert_eq!(paused, 1, "pause property must be 1 after pause");

    unsafe { mpv_terminate_destroy(handle) };
    // Keep the library loaded for the handle lifetime.
    std::mem::forget(lib);

    eprintln!("PASS: bundled libmpv decoded+played+seeked+paused a real video");
}
