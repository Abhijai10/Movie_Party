//! Real native-surface end-to-end validation using the production MpvPlayer
//! with bundled libmpv, SW render API, and a real CALayer.
//!
//! Exercises the exact production path:
//!   MpvPlayer::open → attach_native_surface → play →
//!   render_next_frame → display_frame → CALayer contents
//!
//! Skips gracefully when the bundled runtime or test video is absent.
//! macOS-only.

#![cfg(target_os = "macos")]

use std::os::raw::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use movie_party_lib::media::player::mpv_backend::MpvPlayer;
use movie_party_lib::media::player::LocalPlayer;

// ── Minimal Objective-C bridge to create a real CALayer ──────────────────────

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const i8) -> *mut c_void;
    fn sel_registerName(name: *const i8) -> *mut c_void;
    fn objc_msgSend();
}

unsafe fn selector(name: &str) -> *mut c_void {
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    sel_registerName(bytes.as_ptr().cast())
}

unsafe fn msg_id(target: *mut c_void, name: &str) -> *mut c_void {
    let f: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name))
}

unsafe fn msg_void(target: *mut c_void, name: &str) {
    let f: extern "C" fn(*mut c_void, *mut c_void) = std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name));
}

unsafe fn msg_void_id(target: *mut c_void, name: &str, value: *mut c_void) {
    let f: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name), value);
}

struct RealLayer {
    layer: *mut c_void,
}

impl RealLayer {
    fn new() -> Self {
        unsafe {
            let cls = objc_getClass(c"CALayer".as_ptr());
            assert!(!cls.is_null(), "CALayer class must exist");
            let layer = msg_id(cls, "layer");
            assert!(!layer.is_null(), "[CALayer layer] returned null");
            msg_void(layer, "retain");
            RealLayer { layer }
        }
    }

    fn has_contents(&self) -> bool {
        unsafe { !msg_id(self.layer, "contents").is_null() }
    }
}

impl Drop for RealLayer {
    fn drop(&mut self) {
        unsafe {
            msg_void_id(self.layer, "setContents:", std::ptr::null_mut());
            msg_void(self.layer, "release");
        }
    }
}

unsafe impl Send for RealLayer {}

#[test]
fn bundled_libmpv_renders_onto_real_calayer_through_production_player() {
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

    std::env::set_var(
        "MOVIE_PARTY_LIBMPV_PATH",
        bundled.to_str().expect("bundled path"),
    );

    // ── 1. Real CALayer ───────────────────────────────────────────────────
    let layer = RealLayer::new();
    let layer_ptr = layer.layer as usize;
    assert!(
        !layer.has_contents(),
        "fresh layer must start with nil contents"
    );
    eprintln!("CHECKPOINT: CALayer created");

    // ── 2. Production MpvPlayer ───────────────────────────────────────────
    let mut player = MpvPlayer::new();
    player.open(test_video).expect("open");
    eprintln!("CHECKPOINT: player opened media");

    // ── 3. Attach native surface → loads bundled libmpv, creates render ctx ─
    // NOTE: load_current_media inside the player sets vo=null/ao=null which
    // conflicts with the SW render API. We detach and re-attach, or we set
    // the options manually. For this test we call attach_native_surface directly
    // and let it handle the load.
    player
        .attach_native_surface(layer_ptr)
        .expect("attach_native_surface");
    eprintln!("CHECKPOINT: native surface attached (libmpv loaded + render ctx created)");

    // ── 4. Start playback ─────────────────────────────────────────────────
    player.play().expect("play");
    eprintln!("CHECKPOINT: playback started");

    // ── 5. Render frames and push to CALayer ──────────────────────────────
    let render_start = Instant::now();
    let render_duration = Duration::from_secs(3);
    let mut rendered = 0u32;

    while Instant::now() - render_start < render_duration {
        if let Some((surface, data, w, h, stride)) = player.render_next_frame() {
            movie_party_lib::media::player::native_surface::display_frame(
                surface, w, h, stride, data,
            );
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

    // ── 6. Verify CALayer received frames ─────────────────────────────────
    assert!(
        layer.has_contents(),
        "CALayer must have non-nil contents after rendering frames"
    );
    eprintln!("PASS: CALayer contents is non-nil");

    // ── 7. Pause stops position advancement ───────────────────────────────
    player.pause().expect("pause");
    let pa = player.snapshot().position_ms;
    std::thread::sleep(Duration::from_millis(500));
    let pb = player.snapshot().position_ms;
    assert!(
        pb >= pa && pb - pa < 200,
        "position must not advance while paused (a={pa} b={pb})"
    );
    eprintln!("PASS: pause holds position ({pa}ms → {pb}ms)");

    // ── 8. Seek to a specific position ────────────────────────────────────
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

    // ── 9. Resume plays forward ───────────────────────────────────────────
    player.play().expect("resume");
    let ra = player.snapshot().position_ms;
    std::thread::sleep(Duration::from_millis(500));
    let rb = player.snapshot().position_ms;
    assert!(rb > ra, "resume must advance position (a={ra} b={rb})");
    eprintln!("PASS: resume advances position ({ra}ms → {rb}ms)");

    // ── 10. Cleanup ───────────────────────────────────────────────────────
    player.close();
    eprintln!("PASS: production MpvPlayer renders onto real CALayer end-to-end");
}
