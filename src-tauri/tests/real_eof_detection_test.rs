//! End-of-media detection through the **production** player (AUD-03).
//!
//! `real_playback_smoke_test.rs` drives libmpv through raw FFI, so it cannot
//! see the app's own code. This test drives `MpvPlayer` — the exact type the
//! runtime holds — so it covers the three things AUD-03 actually changed:
//!
//!   1. `player_init_options()` applies `keep-open=yes` (without it the file is
//!      unloaded at EOF and `eof-reached` becomes *unreadable*, so the check
//!      below could never fire — verified by mutation, see BATCH8 report);
//!   2. `snapshot()` maps mpv's `eof-reached` onto `PlayerState::Completed`;
//!   3. the position is pinned to the duration at EOF instead of stalling at
//!      whatever the last poll happened to see.
//!
//! macOS-only, and it needs the gitignored `mpv_runtime/libmpv.dylib` — so CI
//! runs zero tests here and reports that explicitly, exactly like the other
//! `real_*` targets. Prerequisites fail loudly rather than skipping silently.
//!
//! Real decoding requires a native surface: `MpvPlayer` loads libmpv only in
//! `attach_native_surface`, and until then `play()` takes a simulation path
//! that reports `Playing` without decoding anything. So this test builds a
//! real `CALayer`, as `real_native_surface_e2e.rs` does.

#![cfg(target_os = "macos")]

mod common;

use std::os::raw::c_void;
use std::time::{Duration, Instant};

use movie_party_lib::media::player::mpv_backend::MpvPlayer;
use movie_party_lib::media::player::{LocalPlayer, PlayerState};

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
}

impl Drop for RealLayer {
    fn drop(&mut self) {
        unsafe {
            msg_void(self.layer, "release");
        }
    }
}

unsafe impl Send for RealLayer {}

/// The committed fixture is ~3 s. Anything beyond this means the transition
/// never happened rather than that the machine was slow.
const EOF_TIMEOUT: Duration = Duration::from_secs(25);

/// A production player genuinely decoding the fixture.
///
/// Returns the layer too: the player holds a raw pointer to it, so it must
/// outlive the player. Tuple fields drop in order, so `player` goes first.
fn playing_production_player() -> (MpvPlayer, RealLayer) {
    let (bundled, fixture) = common::require_runtime_and_fixture();
    std::env::set_var(
        "MOVIE_PARTY_LIBMPV_PATH",
        bundled.to_str().expect("bundled path"),
    );

    let layer = RealLayer::new();
    let mut player = MpvPlayer::new();
    player.open(&fixture).expect("open fixture");
    player
        .attach_native_surface(layer.layer as usize)
        .expect("attach native surface (this is what loads libmpv)");
    player.play().expect("play");
    (player, layer)
}

/// Poll until `predicate` holds, or the deadline passes. Returns the last
/// snapshot either way so a failure can report what was actually observed.
fn wait_for(
    player: &MpvPlayer,
    timeout: Duration,
    predicate: impl Fn(&movie_party_lib::media::player::PlayerSnapshot) -> bool,
) -> movie_party_lib::media::player::PlayerSnapshot {
    let deadline = Instant::now() + timeout;
    let mut last = player.snapshot();
    while Instant::now() < deadline {
        last = player.snapshot();
        if predicate(&last) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    last
}

/// AUD-03: reaching the end of the movie must be *observable*.
///
/// Before the fix there was no `Completed` variant and no EOF check, so the
/// room stayed `Playing` forever with a playhead that had silently stopped.
#[test]
fn production_player_reports_completed_when_the_movie_ends() {
    let (player, _layer) = playing_production_player();

    let last = wait_for(&player, EOF_TIMEOUT, |snap| {
        snap.state == PlayerState::Completed
    });

    assert_eq!(
        last.state,
        PlayerState::Completed,
        "the production player must report Completed at end of media; it reported \
         {:?} at position {:?}/{:?} ms. If this is `Playing` forever, either \
         `keep-open=yes` is missing from player_init_options() (mpv then unloads \
         the file at EOF and `eof-reached` becomes unreadable) or the \
         `eof-reached` mapping in snapshot() is gone.",
        last.state,
        last.position_ms,
        last.duration_ms,
    );

    let duration = last
        .duration_ms
        .expect("duration must still be readable at EOF — that is what keep-open buys");
    assert!(
        duration >= 2_900,
        "the fixture is ~3 s; decoded duration was {duration} ms"
    );

    // Pinned to the end, not stalled wherever the last poll landed. mpv's own
    // last `time-pos` is ~2967 ms, so this asserts the pinning, not mpv.
    assert_eq!(
        last.position_ms, duration,
        "at EOF the position must be pinned to the duration"
    );
}

/// Negative control: `Completed` must mean *the film ended*, not "the player
/// is running".
///
/// Every sample taken well before the end must be non-terminal. If `snapshot()`
/// ever returned `Completed` unconditionally — or if `eof-reached` read as set
/// from the start — this fails while the positive test above still passes.
#[test]
fn production_player_is_not_completed_mid_playback() {
    let (player, _layer) = playing_production_player();

    let mut saw_progress = false;
    let mut samples = 0u32;
    let deadline = Instant::now() + EOF_TIMEOUT;

    while Instant::now() < deadline {
        let snap = player.snapshot();
        samples += 1;
        if let Some(duration) = snap.duration_ms {
            if snap.position_ms + 500 < duration {
                saw_progress = true;
                assert_ne!(
                    snap.state,
                    PlayerState::Completed,
                    "the movie cannot be Completed at {} ms of {} ms",
                    snap.position_ms,
                    duration
                );
            }
        }
        if snap.state == PlayerState::Completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        saw_progress,
        "control is vacuous: the player never reached a position >500 ms before \
         the end, so `Completed` was never tested against a running movie \
         ({samples} samples)"
    );
}

/// AUD-03 recovery: seeking back from the end must clear `Completed`.
///
/// `Completed` is derived from mpv's `eof-reached` on every `snapshot()` rather
/// than latched, so this is what makes "seek back and watch it again" work.
/// It also proves the state is a live reading and not a stuck flag.
#[test]
fn production_player_leaves_completed_after_seeking_back() {
    let (mut player, _layer) = playing_production_player();

    let ended = wait_for(&player, EOF_TIMEOUT, |snap| {
        snap.state == PlayerState::Completed
    });
    assert_eq!(
        ended.state,
        PlayerState::Completed,
        "precondition: the movie must reach Completed first"
    );

    player.seek(500).expect("seek back to 500 ms");

    // The predicate must require BOTH conditions. Waiting only for the state to
    // clear is a race: mpv clears `eof-reached` before `time-pos` has moved, so
    // the wait can return while the position is still pinned at the duration —
    // which is exactly how this test first failed.
    let after = wait_for(&player, Duration::from_secs(10), |snap| {
        snap.state != PlayerState::Completed && snap.position_ms < 1_500
    });

    assert_ne!(
        after.state,
        PlayerState::Completed,
        "after seeking back to 500 ms the player must not still report Completed \
         (position was {:?} ms)",
        after.position_ms
    );
    assert!(
        after.position_ms < 1_500,
        "the seek should have moved the playhead near 500 ms; it reported {} ms",
        after.position_ms
    );
}
