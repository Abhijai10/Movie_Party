//! Real Windows native-surface end-to-end validation using the production
//! MpvPlayer with bundled libmpv, the SW render API, and a real child HWND.
//!
//! Exercises the exact production path:
//!   MpvPlayer::open → attach_native_surface → play →
//!   render_next_frame → display_frame → StretchDIBits into the child HWND
//!
//! Windows-only.
//!
//! Prerequisites are enforced by `common::require_*`, which fails loudly rather
//! than letting the test report success without executing. The fixture is the
//! same committed file the macOS tests use (no more `C:\Windows\Temp` copy
//! that nothing in the repository created); the runtime is
//! `mpv_runtime/libmpv.dll`, which this repository has no way to build or stage
//! — see BATCH5_REPORT.md, "Windows real tests cannot execute".
//!
//! If that runtime is absent the test FAILS. CI therefore does not run it and
//! reports the skip explicitly instead.

#![cfg(windows)]

mod common;

use std::time::{Duration, Instant};

use movie_party_lib::media::player::mpv_backend::MpvPlayer;
use movie_party_lib::media::player::native_surface::display_frame;
use movie_party_lib::media::player::LocalPlayer;

// ── Minimal Win32 bridge to create a real child window ───────────────────────

#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: *mut core::ffi::c_void,
        menu: *mut core::ffi::c_void,
        instance: *mut core::ffi::c_void,
        parameter: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn DestroyWindow(hwnd: *mut core::ffi::c_void) -> i32;
    fn GetDesktopWindow() -> *mut core::ffi::c_void;
}

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;

struct RealChildWindow {
    hwnd: *mut core::ffi::c_void,
}

impl RealChildWindow {
    fn new(parent: *mut core::ffi::c_void) -> Self {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                640,
                360,
                parent,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(!hwnd.is_null(), "child video window creation failed");
            RealChildWindow { hwnd }
        }
    }
}

impl Drop for RealChildWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}

#[test]
fn bundled_libmpv_renders_onto_real_child_hwnd_through_production_player() {
    let (bundled, test_video) = common::require_runtime_and_fixture();

    std::env::set_var(
        "MOVIE_PARTY_LIBMPV_PATH",
        bundled.to_str().expect("bundled path"),
    );

    // ── 1. Real child HWND ────────────────────────────────────────────────
    let window = RealChildWindow::new(unsafe { GetDesktopWindow() });
    let surface = window.hwnd as usize;
    assert_ne!(surface, 0, "child HWND must be a valid surface handle");
    eprintln!("CHECKPOINT: child HWND created ({surface:#x})");

    // ── 2. Production MpvPlayer ───────────────────────────────────────────
    let mut player = MpvPlayer::new();
    player.open(&test_video).expect("open");
    eprintln!("CHECKPOINT: player opened media");

    // ── 3. Attach native surface → loads bundled libmpv, creates SW ctx ───
    player
        .attach_native_surface(surface)
        .expect("attach_native_surface");
    eprintln!("CHECKPOINT: native surface attached (libmpv loaded + render ctx created)");

    // ── 4. Start playback ─────────────────────────────────────────────────
    player.play().expect("play");
    eprintln!("CHECKPOINT: playback started");

    // ── 5. Render frames and present them into the child HWND ────────────
    let render_start = Instant::now();
    let render_duration = Duration::from_secs(3);
    let mut rendered = 0u32;

    while Instant::now() - render_start < render_duration {
        if let Some((surface, data, w, h, stride)) = player.render_next_frame() {
            display_frame(surface, w, h, stride, data);
            rendered += 1;
        }
        std::thread::sleep(Duration::from_millis(30));
    }

    let elapsed = render_start.elapsed().as_secs_f64();
    assert!(
        rendered > 0,
        "must produce at least one frame in {render_duration:?}"
    );
    eprintln!(
        "RENDER PASS: {rendered} frames in {elapsed:.1}s ({:.0} fps)",
        rendered as f64 / elapsed,
    );

    // ── 6. Pause stops position advancement ──────────────────────────────
    player.pause().expect("pause");
    let pa = player.snapshot().position_ms;
    std::thread::sleep(Duration::from_millis(500));
    let pb = player.snapshot().position_ms;
    assert!(
        pb >= pa && pb - pa < 200,
        "position must not advance while paused (a={pa} b={pb})"
    );
    eprintln!("PASS: pause holds position ({pa}ms → {pb}ms)");

    // ── 7. Seek to a specific position ────────────────────────────────────
    let target = player.snapshot().duration_ms.map_or(0, |d| d / 2);
    if target > 10 {
        player.seek(target).expect("seek");
        std::thread::sleep(Duration::from_millis(500));
        let after = player.snapshot().position_ms;
        assert!(
            after >= target.saturating_sub(1500),
            "seek must move to ~{target} (got {after})"
        );
        eprintln!("PASS: seek moved to ~{after}ms (target {target}ms)");
    }

    // ── 8. Resume plays forward ───────────────────────────────────────────
    player.play().expect("resume");
    let ra = player.snapshot().position_ms;
    std::thread::sleep(Duration::from_millis(500));
    let rb = player.snapshot().position_ms;
    assert!(rb > ra, "resume must advance position (a={ra} b={rb})");
    eprintln!("PASS: resume advances position ({ra}ms → {rb}ms)");

    // ── 9. Cleanup ────────────────────────────────────────────────────────
    player.close();
    drop(window);
    eprintln!("PASS: production MpvPlayer presents onto a real child HWND end-to-end");
}
