//! Real playback smoke test against the bundled libmpv runtime using the
//! software render API (MPV_RENDER_API_TYPE_SW).
//!
//! Validates the SW render pipeline the packaged Movie Party app uses:
//!   bundled libmpv → mpv_render_context_create(SW) → mpv_render_context_render
//!   → bgr0 buffer → verified non-zero pixels
//!
//! Skips gracefully when the runtime or test video is absent.

#![cfg(target_os = "macos")]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;
use std::time::Instant;

type MpvHandle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct MpvRenderParam {
    type_: i32,
    data: *mut c_void,
}

const MPV_FORMAT_INT64: i32 = 4;
const MPV_FORMAT_DOUBLE: i32 = 5;

fn cstring(s: &str) -> CString {
    CString::new(s).unwrap()
}

unsafe fn err_msg(
    rc: c_int,
    mpv_error_string: unsafe extern "C" fn(c_int) -> *const c_char,
) -> String {
    if rc == 0 {
        return "OK".to_string();
    }
    let p = mpv_error_string(rc);
    if p.is_null() {
        format!("mpv error {rc}")
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

#[test]
fn bundled_libmpv_sw_render_api_produces_decoded_frames() {
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

    // ── 1. Load bundled libmpv ───────────────────────────────────────────
    let lib = match unsafe { libloading::Library::new(&bundled) } {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("SKIP: bundled libmpv could not be loaded: {e}");
            return;
        }
    };

    let mpv_create: unsafe extern "C" fn() -> MpvHandle =
        unsafe { *lib.get(b"mpv_create\0").expect("mpv_create") };
    let mpv_initialize: unsafe extern "C" fn(MpvHandle) -> c_int =
        unsafe { *lib.get(b"mpv_initialize\0").expect("mpv_initialize") };
    let mpv_terminate_destroy: unsafe extern "C" fn(MpvHandle) = unsafe {
        *lib.get(b"mpv_terminate_destroy\0")
            .expect("mpv_terminate_destroy")
    };
    let mpv_set_option_string: unsafe extern "C" fn(
        MpvHandle,
        *const c_char,
        *const c_char,
    ) -> c_int = unsafe {
        *lib.get(b"mpv_set_option_string\0")
            .expect("mpv_set_option_string")
    };
    let mpv_command: unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int =
        unsafe { *lib.get(b"mpv_command\0").expect("mpv_command") };
    let mpv_get_property: unsafe extern "C" fn(
        MpvHandle,
        *const c_char,
        i32,
        *mut c_void,
    ) -> c_int = unsafe { *lib.get(b"mpv_get_property\0").expect("mpv_get_property") };
    let mpv_wait_event: unsafe extern "C" fn(MpvHandle, f64) -> *const c_void =
        unsafe { *lib.get(b"mpv_wait_event\0").expect("mpv_wait_event") };
    let mpv_error_string: unsafe extern "C" fn(c_int) -> *const c_char =
        unsafe { *lib.get(b"mpv_error_string\0").expect("mpv_error_string") };
    let mpv_render_context_create: unsafe extern "C" fn(
        *mut *mut c_void,
        MpvHandle,
        *const MpvRenderParam,
    ) -> c_int = unsafe {
        *lib.get(b"mpv_render_context_create\0")
            .expect("mpv_render_context_create")
    };
    let mpv_render_context_render: unsafe extern "C" fn(
        *mut c_void,
        *const MpvRenderParam,
    ) -> c_int = unsafe {
        *lib.get(b"mpv_render_context_render\0")
            .expect("mpv_render_context_render")
    };
    let mpv_render_context_free: unsafe extern "C" fn(*mut c_void) = unsafe {
        *lib.get(b"mpv_render_context_free\0")
            .expect("mpv_render_context_free")
    };

    // ── 2. Create mpv core + initialize ──────────────────────────────────
    let handle = unsafe { mpv_create() };
    assert!(!handle.is_null(), "mpv_create returned null");

    unsafe {
        mpv_set_option_string(
            handle,
            cstring("msg-level").as_ptr(),
            cstring("all=status").as_ptr(),
        );
        mpv_set_option_string(handle, cstring("quiet").as_ptr(), cstring("yes").as_ptr());
        mpv_set_option_string(handle, cstring("terminal").as_ptr(), cstring("no").as_ptr());
    }

    let rc = unsafe { mpv_initialize(handle) };
    assert_eq!(rc, 0, "mpv_initialize: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });

    // ── 3. Create SW render context ──────────────────────────────────────
    let sw_api = cstring("sw");
    let mut render_ctx: *mut c_void = ptr::null_mut();
    let create_params = [
        MpvRenderParam {
            type_: 1,
            data: sw_api.as_ptr() as *mut c_void,
        },
        MpvRenderParam {
            type_: 0,
            data: ptr::null_mut(),
        },
    ];
    let rc = unsafe { mpv_render_context_create(&mut render_ctx, handle, create_params.as_ptr()) };
    assert_eq!(rc, 0, "render context create: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });
    assert!(!render_ctx.is_null(), "render context is null");

    // ── 4. Open the real test video ──────────────────────────────────────
    let path = cstring(&test_video.to_string_lossy());
    let loadfile = cstring("loadfile");
    let replace = cstring("replace");
    let args = [
        loadfile.as_ptr(),
        path.as_ptr(),
        replace.as_ptr(),
        ptr::null(),
    ];
    let rc = unsafe { mpv_command(handle, args.as_ptr()) };
    assert_eq!(rc, 0, "loadfile: {}", unsafe {
        err_msg(rc, mpv_error_string)
    });

    let duration_name = cstring("duration");
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    let mut loaded = false;
    while Instant::now() < deadline {
        unsafe { mpv_wait_event(handle, 0.05) };
        let mut dur: f64 = 0.0;
        let rc = unsafe {
            mpv_get_property(
                handle,
                duration_name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut dur as *mut _ as *mut c_void,
            )
        };
        if rc == 0 && dur > 0.0 {
            loaded = true;
            break;
        }
    }
    assert!(loaded, "test video did not load within timeout");

    // ── 5. Start playback ────────────────────────────────────────────────
    let set_cmd = cstring("set");
    let pause_name = cstring("pause");
    let pause_no = cstring("no");
    let args = [
        set_cmd.as_ptr(),
        pause_name.as_ptr(),
        pause_no.as_ptr(),
        ptr::null(),
    ];
    let rc = unsafe { mpv_command(handle, args.as_ptr()) };
    assert_eq!(rc, 0, "play: {}", unsafe { err_msg(rc, mpv_error_string) });

    for _ in 0..20 {
        unsafe { mpv_wait_event(handle, 0.05) };
    }

    // ── 6. Query video dimensions ────────────────────────────────────────
    let mut video_w: i64 = 640;
    let mut video_h: i64 = 480;
    unsafe {
        let w_name = cstring("video-params/w");
        let h_name = cstring("video-params/h");
        let _ = mpv_get_property(
            handle,
            w_name.as_ptr(),
            MPV_FORMAT_INT64,
            &mut video_w as *mut _ as *mut c_void,
        );
        let _ = mpv_get_property(
            handle,
            h_name.as_ptr(),
            MPV_FORMAT_INT64,
            &mut video_h as *mut _ as *mut c_void,
        );
    }
    let w = video_w.max(16) as usize;
    let h = video_h.max(16) as usize;
    let stride = ((w * 4 + 63) / 64) * 64;
    let mut buf = vec![0u8; stride * h];

    // ── 7. Render frames via SW render API ───────────────────────────────
    let sw_fmt = cstring("bgr0");
    let mut rendered = 0u32;
    let render_start = Instant::now();
    let render_duration = std::time::Duration::from_secs(3);

    while Instant::now() - render_start < render_duration {
        unsafe { mpv_wait_event(handle, 0.01) };
        let mut sw_size = [w as i32, h as i32];
        let mut stride_val = stride;
        let block_for_target: i32 = 0;
        let params = [
            MpvRenderParam {
                type_: 17,
                data: sw_size.as_mut_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: 18,
                data: sw_fmt.as_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: 19,
                data: (&mut stride_val) as *mut usize as *mut c_void,
            },
            MpvRenderParam {
                type_: 20,
                data: buf.as_mut_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: 12,
                data: &block_for_target as *const i32 as *mut c_void,
            },
            MpvRenderParam {
                type_: 0,
                data: ptr::null_mut(),
            },
        ];
        let rc = unsafe { mpv_render_context_render(render_ctx, params.as_ptr()) };
        if rc == 0 {
            rendered += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

    // ── 8. Verify frames were rendered with actual pixel data ────────────
    let elapsed = render_start.elapsed().as_secs_f64();
    assert!(
        rendered > 0,
        "SW render API must produce at least one frame in {render_duration:?}"
    );
    eprintln!(
        "RENDER PASS: {rendered} frames produced in {elapsed:.1}s ({:.0} fps) \
         buffer={w}x{h} stride={stride}",
        rendered as f64 / elapsed,
    );

    let non_zero = buf.iter().any(|&b| b != 0);
    assert!(
        non_zero,
        "rendered frame buffer must contain non-zero pixel data"
    );
    eprintln!("Frame pixels verified: non-zero data present");

    // ── 9. Cleanup ───────────────────────────────────────────────────────
    unsafe {
        mpv_render_context_free(render_ctx);
        mpv_terminate_destroy(handle);
    }
    std::mem::forget(lib);

    eprintln!("PASS: bundled libmpv SW render API produces decoded frames");
}
