//! Audio track handling against real libmpv (AUD-06).
//!
//! The audit's finding was blunt: **nothing in this repository had ever produced
//! a sound.** The fixture had no audio track at all, so every "playback" test
//! was silent by construction and audio was enabled only by mpv's default —
//! invisible to review and to every gate.
//!
//! Two things now cover it:
//!   * `player_requests_audio_output_explicitly` (unit) pins the production
//!     option decision — `audio`/`aid`/`ao` are set explicitly, not by omission.
//!   * this test proves libmpv actually *finds and decodes* an audio track, and
//!     that the video-only fixture yields none.
//!
//! The negative control is the video-only fixture: if mpv reported an audio
//! track for both files, the positive assertion would be about mpv rather than
//! about the media, and would prove nothing.
//!
//! Raw mpv client API rather than `MpvPlayer`, deliberately: `PlayerSnapshot`
//! carries no track information, so the production type cannot answer the
//! question. What is verified here is the media and the runtime; the app's
//! *choice* to request audio is the unit test above.
//!
//! macOS-only; needs the gitignored runtime, so CI runs zero tests here and
//! reports that explicitly. Prerequisites fail loudly rather than skipping.

#![cfg(target_os = "macos")]

mod common;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::time::{Duration, Instant};

type MpvHandle = *mut c_void;

const MPV_FORMAT_INT64: i32 = 4;
const MPV_FORMAT_DOUBLE: i32 = 5;

/// The audio options production sets, replicated here so the runtime is
/// configured the same way the app configures it. `ao=null` because this runs
/// headless — no sound reaches a speaker either way, and this test does not
/// claim it does.
const AUDIO_OPTIONS: [(&str, &str); 6] = [
    ("vo", "null"),
    ("ao", "null"),
    ("audio", "auto"),
    ("aid", "auto"),
    ("keep-open", "yes"),
    ("terminal", "no"),
];

struct Mpv {
    handle: MpvHandle,
    get_property: unsafe extern "C" fn(MpvHandle, *const c_char, i32, *mut c_void) -> c_int,
    command: unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int,
    wait_event: unsafe extern "C" fn(MpvHandle, f64) -> *const c_void,
    terminate_destroy: unsafe extern "C" fn(MpvHandle),
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
}

impl Mpv {
    fn err(&self, code: c_int) -> String {
        let p = unsafe { (self.error_string)(code) };
        if p.is_null() {
            format!("mpv error {code}")
        } else {
            unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
        }
    }

    fn get_i64(&self, name: &str) -> Option<i64> {
        let n = CString::new(name).ok()?;
        let mut out: i64 = 0;
        let rc = unsafe {
            (self.get_property)(
                self.handle,
                n.as_ptr(),
                MPV_FORMAT_INT64,
                &mut out as *mut _ as *mut c_void,
            )
        };
        (rc == 0).then_some(out)
    }

    fn get_f64(&self, name: &str) -> Option<f64> {
        let n = CString::new(name).ok()?;
        let mut out: f64 = 0.0;
        let rc = unsafe {
            (self.get_property)(
                self.handle,
                n.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut out as *mut _ as *mut c_void,
            )
        };
        (rc == 0).then_some(out)
    }

    fn cmd(&self, args: &[&str]) -> c_int {
        let owned: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(ptr::null());
        unsafe { (self.command)(self.handle, ptrs.as_ptr()) }
    }

    fn pump(&self, seconds: f64) {
        let deadline = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < deadline {
            unsafe { (self.wait_event)(self.handle, 0.05) };
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { (self.terminate_destroy)(self.handle) };
    }
}

fn open(lib: &libloading::Library) -> Mpv {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let s = unsafe { lib.get(concat!($name, "\0").as_bytes()) }.expect($name);
            let f: $ty = *s;
            f
        }};
    }

    let create = sym!("mpv_create", unsafe extern "C" fn() -> MpvHandle);
    let initialize = sym!("mpv_initialize", unsafe extern "C" fn(MpvHandle) -> c_int);
    let set_option = sym!(
        "mpv_set_option_string",
        unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int
    );

    let handle = unsafe { create() };
    assert!(!handle.is_null(), "mpv_create returned null");

    for (key, value) in AUDIO_OPTIONS {
        let ck = CString::new(key).unwrap();
        let cv = CString::new(value).unwrap();
        let rc = unsafe { set_option(handle, ck.as_ptr(), cv.as_ptr()) };
        assert_eq!(rc, 0, "setting {key}={value} failed");
    }

    let rc = unsafe { initialize(handle) };
    assert_eq!(rc, 0, "mpv_initialize failed");

    Mpv {
        handle,
        get_property: sym!(
            "mpv_get_property",
            unsafe extern "C" fn(MpvHandle, *const c_char, i32, *mut c_void) -> c_int
        ),
        command: sym!(
            "mpv_command",
            unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int
        ),
        wait_event: sym!(
            "mpv_wait_event",
            unsafe extern "C" fn(MpvHandle, f64) -> *const c_void
        ),
        terminate_destroy: sym!("mpv_terminate_destroy", unsafe extern "C" fn(MpvHandle)),
        error_string: sym!(
            "mpv_error_string",
            unsafe extern "C" fn(c_int) -> *const c_char
        ),
    }
}

/// Load `path`, play it, and wait until the file is probed (duration readable).
fn play(mpv: &Mpv, path: &std::path::Path) {
    let p = path.to_string_lossy().to_string();
    let rc = mpv.cmd(&["loadfile", &p, "replace"]);
    assert_eq!(rc, 0, "loadfile failed: {}", mpv.err(rc));
    mpv.pump(1.0);
    let rc = mpv.cmd(&["set", "pause", "no"]);
    assert_eq!(rc, 0, "play failed: {}", mpv.err(rc));
    // Long enough for the demuxer to resolve track metadata and open the AO.
    mpv.pump(1.5);
}

#[test]
fn audio_bearing_fixture_yields_an_audio_track() {
    let (bundled, video_only, with_audio) = common::require_runtime_fixture_and_audio();
    let lib = unsafe { libloading::Library::new(&bundled) }.expect("load bundled libmpv");

    // ── Positive: the audio fixture must produce a real audio track ──────────
    let mpv = open(&lib);
    play(&mpv, &with_audio);

    let tracks = mpv.get_i64("track-list/count");
    let aid = mpv.get_i64("aid");
    let audio_channels = mpv.get_i64("audio-params/channel-count");
    let out_channels = mpv.get_i64("audio-out-params/channel-count");
    eprintln!(
        "AUDIO FIXTURE: track-list/count={tracks:?} aid={aid:?} \
         audio-params/channel-count={audio_channels:?} \
         audio-out-params/channel-count={out_channels:?}"
    );

    assert!(
        tracks.unwrap_or(0) >= 2,
        "the audio fixture must present at least a video and an audio track; \
         mpv reported track-list/count={tracks:?}"
    );
    assert!(
        aid.unwrap_or(0) >= 1,
        "mpv must select an audio track for the audio fixture; aid={aid:?}. \
         If this is 0 or unreadable, `aid=auto` is not taking effect or the \
         file's audio track is not decodable."
    );
    assert!(
        audio_channels.unwrap_or(0) >= 1,
        "the selected audio track must resolve real channel parameters; \
         audio-params/channel-count={audio_channels:?}"
    );

    drop(mpv);

    // ── Negative control: the video-only fixture must NOT ───────────────────
    // Without this, "mpv reported an audio track" could be a property of mpv
    // rather than of the media, and the assertion above would prove nothing.
    let control = open(&lib);
    play(&control, &video_only);

    let control_tracks = control.get_i64("track-list/count");
    let control_aid = control.get_i64("aid");
    let control_channels = control.get_i64("audio-params/channel-count");
    eprintln!(
        "VIDEO-ONLY FIXTURE: track-list/count={control_tracks:?} aid={control_aid:?} \
         audio-params/channel-count={control_channels:?}"
    );

    assert!(
        control_channels.unwrap_or(0) < 1,
        "control: the video-only fixture must have NO decodable audio track, but \
         audio-params/channel-count={control_channels:?}. The positive assertion \
         above is therefore not about the media."
    );
    assert!(
        control_aid.unwrap_or(0) < 1,
        "control: the video-only fixture must not select an audio track; \
         aid={control_aid:?}"
    );
    assert!(
        control_tracks.unwrap_or(0) < tracks.unwrap_or(0),
        "control: the audio fixture must have MORE tracks than the video-only one \
         ({tracks:?} vs {control_tracks:?})"
    );

    let duration = control.get_f64("duration");
    assert!(
        duration.unwrap_or(0.0) > 1.0,
        "control: the video-only fixture must still decode as a real movie; \
         duration={duration:?}"
    );
}
