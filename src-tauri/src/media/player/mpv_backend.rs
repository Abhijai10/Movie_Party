//! Real libmpv backend using dynamic loading (dlopen/dlsym).
//!
//! This module is only compiled when the `mpv` feature is enabled.
//! At runtime, the libmpv shared library must be present on the system.
//!
//! The FFI bindings wrap the mpv client API:
//! https://mpv.io/manual/stable/#command-interface

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

use super::{is_streaming_media_source, LocalPlayer, PlayerError, PlayerSnapshot, PlayerState};

// ── mpv FFI type aliases ─────────────────────────────────────────────────────

type MpvHandle = *mut c_void;
type MpvEvent = *const c_void;

// ── mpv function signatures loaded via libloading ─────────────────────────────

struct MpvFns {
    mpv_create: unsafe extern "C" fn() -> MpvHandle,
    mpv_initialize: unsafe extern "C" fn(MpvHandle) -> c_int,
    #[allow(dead_code)]
    mpv_destroy: unsafe extern "C" fn(MpvHandle),
    mpv_terminate_destroy: unsafe extern "C" fn(MpvHandle),
    mpv_command: unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int,
    mpv_get_property: unsafe extern "C" fn(MpvHandle, *const c_char, u64, *mut c_void) -> c_int,
    mpv_set_property: unsafe extern "C" fn(MpvHandle, *const c_char, u64, *const c_void) -> c_int,
    #[allow(dead_code)]
    mpv_set_option_string: unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
    mpv_wait_event: unsafe extern "C" fn(MpvHandle, f64) -> MpvEvent,
    mpv_error_string: unsafe extern "C" fn(c_int) -> *const c_char,
}

// mpv format constants
const MPV_FORMAT_DOUBLE: u64 = 5;
const MPV_FORMAT_INT64: u64 = 8;
const MPV_FORMAT_STRING: u64 = 1;

// mpv error codes we care about
const MPV_ERROR_SUCCESS: c_int = 0;

// ── FFI helper functions ─────────────────────────────────────────────────────

extern "C" {
    #[allow(dead_code)]
    fn free(ptr: *mut c_void);
}

/// SAFETY: `ptr` must have been returned by mpv (e.g. mpv_get_property with
/// MPV_FORMAT_STRING). After this call the pointer is invalid.
#[allow(dead_code)]
unsafe fn libc_free(ptr: *mut c_void) {
    free(ptr);
}

/// SAFETY: mpv handle must be valid and `name` must be a valid mpv property.
/// Returns `None` on any mpv error.
unsafe fn mpv_get_double(mpv: MpvHandle, fns: &MpvFns, name: &str) -> Option<f64> {
    let c_name = CString::new(name).ok()?;
    let mut out: f64 = 0.0;
    let result = (fns.mpv_get_property)(
        mpv,
        c_name.as_ptr(),
        MPV_FORMAT_DOUBLE,
        &mut out as *mut _ as *mut c_void,
    );
    if result == MPV_ERROR_SUCCESS {
        Some(out)
    } else {
        None
    }
}

/// SAFETY: mpv handle must be valid and `name` must be a valid mpv property.
/// Returns `None` on any mpv error.
unsafe fn mpv_get_int64(mpv: MpvHandle, fns: &MpvFns, name: &str) -> Option<i64> {
    let c_name = CString::new(name).ok()?;
    let mut out: i64 = 0;
    let result = (fns.mpv_get_property)(
        mpv,
        c_name.as_ptr(),
        MPV_FORMAT_INT64,
        &mut out as *mut _ as *mut c_void,
    );
    if result == MPV_ERROR_SUCCESS {
        Some(out)
    } else {
        None
    }
}

/// SAFETY: mpv handle must be valid. Option name and value must be valid C strings.
#[allow(dead_code)]
unsafe fn mpv_set_option(
    mpv: MpvHandle,
    fns: &MpvFns,
    name: &str,
    value: &str,
) -> Result<(), PlayerError> {
    let c_name = CString::new(name).map_err(|_| PlayerError::PlaybackError {
        message: "invalid option name".to_string(),
    })?;
    let c_value = CString::new(value).map_err(|_| PlayerError::PlaybackError {
        message: "invalid option value".to_string(),
    })?;
    let result = (fns.mpv_set_option_string)(mpv, c_name.as_ptr(), c_value.as_ptr());
    if result == MPV_ERROR_SUCCESS {
        Ok(())
    } else {
        let error_str = (fns.mpv_error_string)(result);
        let msg = if error_str.is_null() {
            "unknown mpv error".to_string()
        } else {
            CStr::from_ptr(error_str).to_string_lossy().into_owned()
        };
        Err(PlayerError::PlaybackError { message: msg })
    }
}

/// Convert an mpv return code into the stable player error contract.
///
/// Runtime dependency note: this backend is compiled only with the `mpv`
/// feature, but the libmpv shared library must still be present at runtime.
/// Missing libraries map to `MP-MEDIA-001`; command/property failures map to
/// `MP-MEDIA-006` instead of being silently ignored.
unsafe fn mpv_result(result: c_int, fns: &MpvFns, fallback: &str) -> Result<(), PlayerError> {
    if result == MPV_ERROR_SUCCESS {
        return Ok(());
    }

    let error_str = (fns.mpv_error_string)(result);
    let message = if error_str.is_null() {
        fallback.to_string()
    } else {
        CStr::from_ptr(error_str).to_string_lossy().into_owned()
    };
    Err(PlayerError::PlaybackError { message })
}

// ── MpvPlayer ────────────────────────────────────────────────────────────────

/// Real libmpv player backend using dynamic loading.
///
/// Loads the mpv shared library at creation time. If the library cannot be
/// loaded, the player remains in a disabled state and all commands return
/// `PlayerError::LibMpvUnavailable`.
pub struct MpvPlayer {
    handle: Option<MpvHandle>,
    fns: Option<MpvFns>,
    loaded_path: Option<std::path::PathBuf>,
    snapshot: PlayerSnapshot,
}

impl MpvPlayer {
    /// Create a new MpvPlayer by attempting to dynamically load libmpv.
    pub fn new() -> Self {
        match Self::load_library() {
            Ok((handle, fns)) => Self {
                handle: Some(handle),
                fns: Some(fns),
                loaded_path: None,
                snapshot: PlayerSnapshot::default(),
            },
            Err(_) => Self {
                handle: None,
                fns: None,
                loaded_path: None,
                snapshot: PlayerSnapshot::default(),
            },
        }
    }

    /// Attempt to load the mpv shared library and create an mpv context.
    fn load_library() -> Result<(MpvHandle, MpvFns), PlayerError> {
        let lib = Self::open_mpv_library()?;
        let fns = MpvFns {
            mpv_create: unsafe {
                *lib.get::<unsafe extern "C" fn() -> MpvHandle>(b"mpv_create\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_create symbol: {e}"),
                    })?
            },
            mpv_initialize: unsafe {
                *lib.get::<unsafe extern "C" fn(MpvHandle) -> c_int>(b"mpv_initialize\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_initialize symbol: {e}"),
                    })?
            },
            mpv_destroy: unsafe {
                *lib.get::<unsafe extern "C" fn(MpvHandle)>(b"mpv_destroy\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_destroy symbol: {e}"),
                    })?
            },
            mpv_terminate_destroy: unsafe {
                *lib.get::<unsafe extern "C" fn(MpvHandle)>(b"mpv_terminate_destroy\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_terminate_destroy symbol: {e}"),
                    })?
            },
            mpv_command: unsafe {
                *lib.get::<unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int>(
                    b"mpv_command\0",
                )
                .map_err(|e| PlayerError::InitFailed {
                    reason: format!("mpv_command symbol: {e}"),
                })?
            },
            mpv_get_property: unsafe {
                *lib.get::<unsafe extern "C" fn(
                    MpvHandle,
                    *const c_char,
                    u64,
                    *mut c_void,
                ) -> c_int>(b"mpv_get_property\0")
                .map_err(|e| PlayerError::InitFailed {
                    reason: format!("mpv_get_property symbol: {e}"),
                })?
            },
            mpv_set_property: unsafe {
                *lib.get::<unsafe extern "C" fn(
                    MpvHandle,
                    *const c_char,
                    u64,
                    *const c_void,
                ) -> c_int>(b"mpv_set_property\0")
                .map_err(|e| PlayerError::InitFailed {
                    reason: format!("mpv_set_property symbol: {e}"),
                })?
            },
            mpv_set_option_string: unsafe {
                *lib.get::<unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int>(
                    b"mpv_set_option_string\0",
                )
                .map_err(|e| PlayerError::InitFailed {
                    reason: format!("mpv_set_option_string symbol: {e}"),
                })?
            },
            mpv_wait_event: unsafe {
                *lib.get::<unsafe extern "C" fn(MpvHandle, f64) -> MpvEvent>(b"mpv_wait_event\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_wait_event symbol: {e}"),
                    })?
            },
            mpv_error_string: unsafe {
                *lib.get::<unsafe extern "C" fn(c_int) -> *const c_char>(b"mpv_error_string\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_error_string symbol: {e}"),
                    })?
            },
        };

        // Create mpv instance
        let mpv = unsafe { (fns.mpv_create)() };
        if mpv.is_null() {
            return Err(PlayerError::InitFailed {
                reason: "mpv_create returned null".to_string(),
            });
        }

        // Prevent libmpv from creating its own user-facing window. Visible
        // presentation must be provided by the native render host; without
        // that host the runtime reports presentation as unavailable.
        unsafe {
            mpv_set_option(mpv, &fns, "vo", "libmpv")?;
        }

        // Initialize the mpv context
        let result = unsafe { (fns.mpv_initialize)(mpv) };
        if result != MPV_ERROR_SUCCESS {
            let error_str = unsafe { (fns.mpv_error_string)(result) };
            let msg = if error_str.is_null() {
                "mpv_initialize failed".to_string()
            } else {
                unsafe { CStr::from_ptr(error_str).to_string_lossy().into_owned() }
            };
            unsafe { (fns.mpv_terminate_destroy)(mpv) };
            return Err(PlayerError::InitFailed { reason: msg });
        }

        // Note: lib must stay loaded for the lifetime of the mpv handle.
        // We use ManuallyDrop to keep it alive.
        let _lib = std::mem::ManuallyDrop::new(lib);

        Ok((mpv, fns))
    }

    fn open_mpv_library() -> Result<libloading::Library, PlayerError> {
        let candidates = Self::mpv_library_candidates();
        for candidate in &candidates {
            if let Ok(lib) = unsafe { libloading::Library::new(candidate) } {
                return Ok(lib);
            }
        }
        Err(PlayerError::LibMpvUnavailable)
    }

    fn mpv_library_candidates() -> Vec<String> {
        let mut candidates = Vec::new();

        #[cfg(target_os = "macos")]
        {
            candidates.push("libmpv.dylib".to_string());
            candidates.push("/opt/homebrew/lib/libmpv.dylib".to_string());
            candidates.push("/usr/local/lib/libmpv.dylib".to_string());
            candidates.push("/Applications/mpv.app/Contents/MacOS/libmpv.dylib".to_string());
        }

        #[cfg(target_os = "windows")]
        {
            candidates.push("mpv-2.dll".to_string());
            candidates.push(r"C:\Program Files\mpv\mpv-2.dll".to_string());
            candidates.push(r"C:\Program Files (x86)\mpv\mpv-2.dll".to_string());
        }

        #[cfg(target_os = "linux")]
        {
            candidates.push("libmpv.so".to_string());
            candidates.push("/usr/lib/libmpv.so".to_string());
            candidates.push("/usr/lib/x86_64-linux-gnu/libmpv.so".to_string());
        }

        candidates
    }

    fn ensure_ready(&self) -> Result<(MpvHandle, &MpvFns), PlayerError> {
        match (self.handle, &self.fns) {
            (Some(h), Some(fns)) => {
                if self.loaded_path.is_none() {
                    return Err(PlayerError::NotReady);
                }
                Ok((h, fns))
            }
            _ => Err(PlayerError::LibMpvUnavailable),
        }
    }
}

impl Default for MpvPlayer {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: MpvPlayer wraps a raw mpv_handle (C void pointer) obtained from
// mpv_create(). The mpv client API is thread-safe — mpv_command,
// mpv_set_property, and mpv_get_property may be called from any thread as
// long as concurrent access to the same handle is serialized. In this
// implementation all mpv calls go through the MpvPlayer methods, and
// AppRuntime always wraps MpvPlayer in `Arc<Mutex<MpvPlayer>>`, ensuring
// exclusive access. The libmpv library is kept alive via ManuallyDrop in
// load_library(). Drop calls mpv_terminate_destroy which is safe to call
// from any thread that last held the handle.
unsafe impl Send for MpvPlayer {}
unsafe impl Sync for MpvPlayer {}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        if let (Some(handle), Some(fns)) = (self.handle.take(), self.fns.take()) {
            unsafe { (fns.mpv_terminate_destroy)(handle) };
        }
    }
}

impl LocalPlayer for MpvPlayer {
    fn open(&mut self, path: &Path) -> Result<(), PlayerError> {
        let (handle, fns) = match (self.handle, &self.fns) {
            (Some(h), Some(fns)) => (h, fns),
            _ => return Err(PlayerError::LibMpvUnavailable),
        };

        if !is_streaming_media_source(path) && !path.exists() {
            return Err(PlayerError::MissingMedia {
                path: path.display().to_string(),
            });
        }

        let path_str = path.to_str().ok_or_else(|| PlayerError::LoadFailed {
            reason: "path contains invalid UTF-8".to_string(),
        })?;
        let c_path = CString::new(path_str).map_err(|_| PlayerError::LoadFailed {
            reason: "path contains null byte".to_string(),
        })?;

        // Execute: loadfile <path> replace
        let loadfile_cmd = CString::new("loadfile").unwrap();
        let replace_arg = CString::new("replace").unwrap();
        let args = [
            loadfile_cmd.as_ptr(),
            c_path.as_ptr(),
            replace_arg.as_ptr(),
            ptr::null(),
        ];
        let result = unsafe { (fns.mpv_command)(handle, args.as_ptr()) };

        if result != MPV_ERROR_SUCCESS {
            let error_str = unsafe { (fns.mpv_error_string)(result) };
            let msg = if error_str.is_null() {
                "loadfile failed".to_string()
            } else {
                unsafe { CStr::from_ptr(error_str).to_string_lossy().into_owned() }
            };
            return Err(PlayerError::LoadFailed { reason: msg });
        }

        self.loaded_path = Some(path.to_path_buf());
        self.snapshot.state = PlayerState::Ready;
        self.snapshot.error_message = None;

        // Wait briefly for the file to be loaded and metadata to become available
        unsafe { (fns.mpv_wait_event)(handle, 0.1) };

        // Query duration
        if let Some(dur) = unsafe { mpv_get_double(handle, fns, "duration") } {
            self.snapshot.duration_ms = Some((dur * 1000.0) as u64);
        }

        Ok(())
    }

    fn play(&mut self) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let pause_false = CString::new("false").unwrap();
        let pause_name = CString::new("pause").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    pause_name.as_ptr(),
                    MPV_FORMAT_STRING,
                    pause_false.as_ptr() as *const c_void,
                ),
                fns,
                "failed to start playback",
            )?;
            // Wait for play event
            (fns.mpv_wait_event)(handle, 0.05);
        }
        self.snapshot.state = PlayerState::Playing;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let pause_true = CString::new("true").unwrap();
        let pause_name = CString::new("pause").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    pause_name.as_ptr(),
                    MPV_FORMAT_STRING,
                    pause_true.as_ptr() as *const c_void,
                ),
                fns,
                "failed to pause playback",
            )?;
            (fns.mpv_wait_event)(handle, 0.05);
        }
        self.snapshot.state = PlayerState::Paused;
        Ok(())
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let seek_cmd = CString::new("seek").unwrap();
        let position_seconds = position_ms as f64 / 1_000.0;
        let pos_str = CString::new(format!("{position_seconds:.3}")).unwrap();
        let absolute = CString::new("absolute").unwrap();
        let args = [
            seek_cmd.as_ptr(),
            pos_str.as_ptr(),
            absolute.as_ptr(),
            ptr::null(),
        ];
        unsafe {
            mpv_result(
                (fns.mpv_command)(handle, args.as_ptr()),
                fns,
                "failed to seek playback",
            )?;
            (fns.mpv_wait_event)(handle, 0.05);
        }
        self.snapshot.position_ms = position_ms;
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let vol = volume.clamp(0.0, 1.0) * 100.0;
        let vol_str = CString::new(format!("{vol:.0}")).unwrap();
        let vol_name = CString::new("volume").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    vol_name.as_ptr(),
                    MPV_FORMAT_STRING,
                    vol_str.as_ptr() as *const c_void,
                ),
                fns,
                "failed to set volume",
            )?;
        }
        self.snapshot.volume = volume;
        Ok(())
    }

    fn set_playback_rate(&mut self, rate: f32) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let rate = rate.clamp(0.25, 4.0);
        let rate_str = CString::new(format!("{rate}")).unwrap();
        let speed_name = CString::new("speed").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    speed_name.as_ptr(),
                    MPV_FORMAT_STRING,
                    rate_str.as_ptr() as *const c_void,
                ),
                fns,
                "failed to set playback rate",
            )?;
        }
        self.snapshot.playback_rate = rate;
        Ok(())
    }

    fn snapshot(&self) -> PlayerSnapshot {
        let mut snap = self.snapshot.clone();

        // Update live position from mpv
        if let (Some(handle), Some(fns)) = (self.handle, &self.fns) {
            unsafe {
                if let Some(pos) = mpv_get_double(handle, fns, "time-pos") {
                    snap.position_ms = (pos * 1000.0) as u64;
                }
                if let Some(dur) = mpv_get_double(handle, fns, "duration") {
                    snap.duration_ms = Some((dur * 1000.0) as u64);
                }
                // Update state from mpv pause property
                if let Some(pause) = mpv_get_int64(handle, fns, "pause") {
                    if pause != 0 {
                        snap.state = PlayerState::Paused;
                    } else if snap.state == PlayerState::Paused || snap.state == PlayerState::Ready
                    {
                        snap.state = PlayerState::Playing;
                    }
                }
            }
        }

        snap
    }

    fn duration(&self) -> Option<u64> {
        if let (Some(handle), Some(fns)) = (self.handle, &self.fns) {
            unsafe {
                if let Some(dur) = mpv_get_double(handle, fns, "duration") {
                    return Some((dur * 1000.0) as u64);
                }
            }
        }
        self.snapshot.duration_ms
    }

    fn buffered_ahead_ms(&self) -> Option<u64> {
        // mpv doesn't directly expose "buffered ahead" for local files.
        // For local perfect mode, the entire file is effectively available.
        // For partial cache playback, the cache layer reports this separately.
        None
    }

    fn error_message(&self) -> Option<String> {
        self.snapshot.error_message.clone()
    }

    fn close(&mut self) {
        if let (Some(handle), Some(fns)) = (self.handle, self.fns.take()) {
            // Stop playback
            let stop_cmd = CString::new("stop").unwrap();
            let null_term = ptr::null();
            let args = [stop_cmd.as_ptr(), null_term];
            unsafe {
                (fns.mpv_command)(handle, args.as_ptr());
            }
        }
        self.loaded_path = None;
        self.snapshot = PlayerSnapshot::default();
    }
}

#[cfg(test)]
mod tests {
    use super::MpvPlayer;

    #[test]
    fn mpv_player_creation_does_not_panic() {
        let _player = MpvPlayer::new();
        // Creation should never panic even if mpv is unavailable
    }
}
