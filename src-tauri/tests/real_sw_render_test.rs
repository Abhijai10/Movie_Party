//! Real playback validation against the bundled libmpv runtime using the
//! software render API (MPV_RENDER_API_TYPE_SW).
//!
//! Validates the SW render pipeline the packaged Movie Party app uses:
//!   bundled libmpv → mpv_render_context_create(SW) → mpv_render_context_render
//!   → bgr0 buffer → verified non-zero pixels
//!
//! macOS-only: it loads `mpv_runtime/libmpv.dylib` directly.
//!
//! # What this test got wrong before (see BATCH5_REPORT.md)
//!
//! It rendered into one buffer in a fixed 3.0 s loop and then asserted on the
//! *final* buffer contents. That is not the claim it wants to make, and it was
//! wrong for two compounding reasons:
//!
//! 1. **The render window outlived the media.** The fixture is exactly 3.000 s
//!    and playback starts ~1 s before the render loop, so the loop always
//!    finished *past the end of the file*.
//! 2. **mpv renders a black frame when there is no frame to present.** With no
//!    new frame available, `mpv_render_context_render` writes a fully zeroed
//!    buffer (measured: all 307 200 bytes written, all zero) — so the final
//!    buffer was *legitimately* black, and the assertion failed even though the
//!    render pipeline was working correctly.
//!
//! Measured on the old code: 47 renders carried real pixel data, the last 3 were
//! post-end-of-media black, and the assertion sampled one of those 3. Shrinking
//! the window to 1 s made the test pass with no other change — which is what
//! identified it as a test defect rather than a rendering defect.
//!
//! The test now states its requirement directly: *produce at least N renders
//! that carry real pixel data*. It classifies every render (using
//! `mpv_render_context_update`, which the old code never consulted), keeps the
//! best frame as evidence, and never asserts on an arbitrary final buffer.
//!
//! Prerequisites are enforced by `common::require_*`, which fails loudly rather
//! than letting the test report success without executing.

#![cfg(target_os = "macos")]

mod common;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::time::{Duration, Instant};

type MpvHandle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct MpvRenderParam {
    type_: i32,
    data: *mut c_void,
}

const MPV_FORMAT_INT64: i32 = 4;
const MPV_FORMAT_DOUBLE: i32 = 5;

/// `mpv_render_context_update` flag: a new frame is available to render.
const MPV_RENDER_UPDATE_FRAME: u64 = 1;

/// Hard ceiling on the render loop, so a broken pipeline fails instead of hanging.
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);

/// A sentinel that is itself non-zero, so "mpv wrote nothing" and "mpv wrote a
/// black frame" are distinguishable instead of both reading as a zeroed buffer.
const SENTINEL: u8 = 0xAB;

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
    let (bundled, test_video) = common::require_runtime_and_fixture();

    // ── 1. Load bundled libmpv ───────────────────────────────────────────
    let lib = unsafe { libloading::Library::new(&bundled) }
        .expect("bundled libmpv.dylib must be loadable (checked by require_libmpv)");

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
    let mpv_render_context_update: unsafe extern "C" fn(*mut c_void) -> u64 = unsafe {
        *lib.get(b"mpv_render_context_update\0")
            .expect("mpv_render_context_update")
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
    let deadline = Instant::now() + Duration::from_secs(5);
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

    // Guard the fixture's length explicitly. Without this, a too-short fixture
    // produces the confusing "buffer must contain non-zero pixel data" symptom
    // described in the module docs instead of naming the real problem.
    let mut duration: f64 = 0.0;
    unsafe {
        mpv_get_property(
            handle,
            duration_name.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut duration as *mut _ as *mut c_void,
        );
    }
    assert!(
        duration >= common::MIN_FIXTURE_SECONDS,
        "the fixture must be at least {:.1}s long so the render loop stays inside \
         the media; {test_video:?} is {duration:.3}s. Regenerate it with \
         ./scripts/make-test-media-macos.sh",
        common::MIN_FIXTURE_SECONDS
    );
    eprintln!(
        "fixture duration {duration:.3}s (>= {:.1}s required)",
        common::MIN_FIXTURE_SECONDS
    );

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
    let stride = (w * 4).div_ceil(64) * 64;
    let mut buf = vec![0u8; stride * h];
    let buffer_len = buf.len();

    // ── 7. Render via the SW render API, classifying every render ────────
    //
    // The loop's requirement is explicit: collect enough renders that carry
    // real pixel data. It stops as soon as it has them, so it does not depend
    // on the fixture's length and cannot fail merely because playback ended.
    let sw_fmt = cstring("bgr0");
    let mut renders = 0u32;
    let mut frames_with_pixels = 0u32;
    let mut frames_without_new_frame = 0u32;
    let mut frames_written_black = 0u32;
    let mut best_nonzero = 0usize;
    let mut first16 = [0u8; 16];
    let render_start = Instant::now();

    while render_start.elapsed() < RENDER_TIMEOUT
        && frames_with_pixels < common::MIN_FRAMES_WITH_PIXELS
    {
        let frame_available =
            unsafe { mpv_render_context_update(render_ctx) } & MPV_RENDER_UPDATE_FRAME != 0;

        buf.fill(SENTINEL);

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
        assert_eq!(rc, 0, "render: {}", unsafe {
            err_msg(rc, mpv_error_string)
        });
        renders += 1;

        let written = buf.iter().filter(|&&b| b != SENTINEL).count();
        let nonzero = buf.iter().filter(|&&b| b != 0).count();

        if !frame_available {
            // Not an error — mpv reports no new frame between video frames —
            // but recorded, because a run made entirely of these is the
            // signature of the defect described in the module docs.
            frames_without_new_frame += 1;
        }

        if written == 0 {
            // mpv wrote nothing at all: no frame was presented to this buffer.
            continue;
        }

        if nonzero > 0 {
            frames_with_pixels += 1;
            if nonzero > best_nonzero {
                best_nonzero = nonzero;
                first16.copy_from_slice(&buf[..16]);
            }
        } else {
            // mpv wrote a frame, and every byte of it was zero.
            frames_written_black += 1;
        }

        std::thread::sleep(Duration::from_millis(15));
    }

    let elapsed = render_start.elapsed().as_secs_f64();
    eprintln!(
        "SW render: {renders} renders in {elapsed:.2}s — {frames_with_pixels} with pixel data, \
         {frames_written_black} black, {frames_without_new_frame} with no new frame \
         (buffer {w}x{h} stride {stride})"
    );

    // ── 8. The actual claim ──────────────────────────────────────────────
    assert!(
        frames_with_pixels >= common::MIN_FRAMES_WITH_PIXELS,
        "the SW render API must produce at least {} renders carrying real pixel data; \
         got {frames_with_pixels} ({frames_written_black} wrote an all-black frame, \
         {frames_without_new_frame} had no frame to present). A run where every render \
         is black usually means the render window outlived the media — see the module \
         docs.",
        common::MIN_FRAMES_WITH_PIXELS
    );
    assert!(
        best_nonzero > 0,
        "the best rendered frame must contain non-zero pixel data"
    );
    eprintln!(
        "Frame pixels verified: up to {best_nonzero}/{buffer_len} non-zero bytes, \
         first16={first16:?}"
    );

    // ── 9. Cleanup ───────────────────────────────────────────────────────
    unsafe {
        mpv_render_context_free(render_ctx);
        mpv_terminate_destroy(handle);
    }
    std::mem::forget(lib);

    eprintln!("PASS: bundled libmpv SW render API produces decoded frames");
}
