//! Real libmpv backend using dynamic loading (dlopen/dlsym) with the
//! software renderer (`mpv_render_context` / `MPV_RENDER_API_TYPE_SW`).
//!
//! This module is only compiled when the `mpv` feature is enabled.
//! At runtime, the bundled libmpv shared library must be present in the
//! `.app` bundle at `Contents/Resources/mpv_runtime/`.
//!
//! GPU context embedding (`wid`) is NOT supported by this LGPL build of
//! libmpv (the `gpu`/`gpu-next` VOs have no macOS GPU context).  Instead
//! we use the `mpv_render_context` API with software rendering, which
//! produces an RGBA buffer that the host pushes onto the native NSView's
//! backing layer via `native_surface::display_frame`.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

use super::{
    is_streaming_media_source, presentation, LocalPlayer, PlayerError, PlayerSnapshot, PlayerState,
};

type MpvHandle = *mut c_void;
type MpvEvent = *const c_void;

#[derive(Clone, Copy)]
struct MpvFns {
    mpv_create: unsafe extern "C" fn() -> MpvHandle,
    mpv_initialize: unsafe extern "C" fn(MpvHandle) -> c_int,
    mpv_terminate_destroy: unsafe extern "C" fn(MpvHandle),
    mpv_command: unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int,
    mpv_get_property: unsafe extern "C" fn(MpvHandle, *const c_char, u64, *mut c_void) -> c_int,
    mpv_set_property: unsafe extern "C" fn(MpvHandle, *const c_char, u64, *const c_void) -> c_int,
    mpv_set_option_string: unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
    mpv_wait_event: unsafe extern "C" fn(MpvHandle, f64) -> MpvEvent,
    mpv_error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    mpv_render_context_create:
        unsafe extern "C" fn(*mut *mut c_void, MpvHandle, *const MpvRenderParam) -> c_int,
    mpv_render_context_render: unsafe extern "C" fn(*mut c_void, *const MpvRenderParam) -> c_int,
    mpv_render_context_free: unsafe extern "C" fn(*mut c_void),
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MpvRenderParam {
    type_: i32,
    data: *mut c_void,
}

const MPV_FORMAT_FLAG: u64 = 3;
const MPV_FORMAT_DOUBLE: u64 = 5;
const MPV_FORMAT_INT64: u64 = 4;
const MPV_ERROR_SUCCESS: c_int = 0;

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
        mpv_result(result, fns, "option failed")
    }
}

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

pub struct MpvPlayer {
    handle: Option<MpvHandle>,
    fns: Option<MpvFns>,
    loaded_path: Option<std::path::PathBuf>,
    snapshot: PlayerSnapshot,
    surface_handle: Option<usize>,
    unavailable: bool,
    render_ctx: Option<*mut c_void>,
    render_w: usize,
    render_h: usize,
    render_stride: usize,
    render_buf: Vec<u8>,
}

impl MpvPlayer {
    pub fn new() -> Self {
        Self {
            handle: None,
            fns: None,
            loaded_path: None,
            snapshot: PlayerSnapshot::default(),
            surface_handle: None,
            unavailable: false,
            render_ctx: None,
            render_w: 640,
            render_h: 480,
            render_stride: 0,
            render_buf: Vec::new(),
        }
    }

    fn load_library(
        _surface_handle: usize,
    ) -> Result<(MpvHandle, MpvFns, *mut c_void), PlayerError> {
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
                *lib.get::<unsafe extern "C" fn(MpvHandle, *const c_char, u64, *mut c_void) -> c_int>(b"mpv_get_property\0").map_err(|e| PlayerError::InitFailed { reason: format!("mpv_get_property symbol: {e}") })?
            },
            mpv_set_property: unsafe {
                *lib.get::<unsafe extern "C" fn(MpvHandle, *const c_char, u64, *const c_void) -> c_int>(b"mpv_set_property\0").map_err(|e| PlayerError::InitFailed { reason: format!("mpv_set_property symbol: {e}") })?
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
            mpv_render_context_create: unsafe {
                *lib.get::<unsafe extern "C" fn(
                    *mut *mut c_void,
                    MpvHandle,
                    *const MpvRenderParam,
                ) -> c_int>(b"mpv_render_context_create\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_render_context_create symbol: {e}"),
                    })?
            },
            mpv_render_context_render: unsafe {
                *lib.get::<unsafe extern "C" fn(*mut c_void, *const MpvRenderParam) -> c_int>(
                    b"mpv_render_context_render\0",
                )
                .map_err(|e| PlayerError::InitFailed {
                    reason: format!("mpv_render_context_render symbol: {e}"),
                })?
            },
            mpv_render_context_free: unsafe {
                *lib.get::<unsafe extern "C" fn(*mut c_void)>(b"mpv_render_context_free\0")
                    .map_err(|e| PlayerError::InitFailed {
                        reason: format!("mpv_render_context_free symbol: {e}"),
                    })?
            },
        };
        let mpv = unsafe { (fns.mpv_create)() };
        if mpv.is_null() {
            return Err(PlayerError::InitFailed {
                reason: "mpv_create returned null".to_string(),
            });
        }
        unsafe {
            mpv_set_option(mpv, &fns, "msg-level", "all=status")?;
            mpv_set_option(mpv, &fns, "quiet", "yes")?;
            mpv_set_option(mpv, &fns, "terminal", "no")?;
        }
        let result = unsafe { (fns.mpv_initialize)(mpv) };
        if result != MPV_ERROR_SUCCESS {
            let err_str = unsafe { (fns.mpv_error_string)(result) };
            let msg = if err_str.is_null() {
                "mpv_initialize failed".to_string()
            } else {
                unsafe { CStr::from_ptr(err_str).to_string_lossy().into_owned() }
            };
            unsafe { (fns.mpv_terminate_destroy)(mpv) };
            return Err(PlayerError::InitFailed { reason: msg });
        }
        let sw_api = CString::new("sw").unwrap();
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
        let rc_create = unsafe {
            (fns.mpv_render_context_create)(&mut render_ctx, mpv, create_params.as_ptr())
        };
        if rc_create != 0 {
            let err_str = unsafe { (fns.mpv_error_string)(rc_create) };
            unsafe { (fns.mpv_terminate_destroy)(mpv) };
            return Err(PlayerError::InitFailed {
                reason: if err_str.is_null() {
                    "render context creation failed".to_string()
                } else {
                    unsafe { CStr::from_ptr(err_str).to_string_lossy().into_owned() }
                },
            });
        }
        if render_ctx.is_null() {
            unsafe { (fns.mpv_terminate_destroy)(mpv) };
            return Err(PlayerError::InitFailed {
                reason: "render context is null".to_string(),
            });
        }
        let _lib = std::mem::ManuallyDrop::new(lib);
        Ok((mpv, fns, render_ctx))
    }

    fn open_mpv_library() -> Result<libloading::Library, PlayerError> {
        let candidates = super::candidate_libmpv_paths();
        for candidate in &candidates {
            if let Ok(lib) = unsafe { libloading::Library::new(candidate) } {
                return Ok(lib);
            }
        }
        Err(PlayerError::LibMpvUnavailable)
    }

    fn ensure_ready(&self) -> Result<(MpvHandle, MpvFns), PlayerError> {
        match (self.handle, self.fns) {
            (Some(h), Some(fns)) => Ok((h, fns)),
            _ => Err(PlayerError::LibMpvUnavailable),
        }
    }

    fn load_current_media(&mut self) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let local_path = self.loaded_path.clone().ok_or(PlayerError::NotReady)?;
        let path_str = local_path.to_str().ok_or_else(|| PlayerError::LoadFailed {
            reason: "path contains invalid UTF-8".to_string(),
        })?;
        let c_path = CString::new(path_str).map_err(|_| PlayerError::LoadFailed {
            reason: "path contains null byte".to_string(),
        })?;
        let pause_true: i32 = 1;
        let pause_name = CString::new("pause").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    pause_name.as_ptr(),
                    MPV_FORMAT_FLAG,
                    &pause_true as *const i32 as *const c_void,
                ),
                &fns,
                "failed to prepare media paused",
            )?;
        }
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
            let err_str = unsafe { (fns.mpv_error_string)(result) };
            let reason = if err_str.is_null() {
                "loadfile failed".to_string()
            } else {
                unsafe { CStr::from_ptr(err_str).to_string_lossy().into_owned() }
            };
            return Err(PlayerError::LoadFailed { reason });
        }
        for _ in 0..10 {
            unsafe { (fns.mpv_wait_event)(handle, 0.1) };
        }
        if let Some(duration) = unsafe { mpv_get_double(handle, &fns, "duration") } {
            self.snapshot.duration_ms = Some((duration * 1000.0) as u64);
        }
        if let Some(w) = unsafe { mpv_get_int64(handle, &fns, "video-params/w") } {
            if let Some(h) = unsafe { mpv_get_int64(handle, &fns, "video-params/h") } {
                self.render_w = w as usize;
                self.render_h = h as usize;
            }
        }
        Ok(())
    }

    fn ensure_render_buf(&mut self, handle: MpvHandle, fns: &MpvFns) {
        unsafe {
            let w = mpv_get_int64(handle, fns, "video-params/w").unwrap_or(self.render_w as i64);
            let h = mpv_get_int64(handle, fns, "video-params/h").unwrap_or(self.render_h as i64);
            let w = w.max(16) as usize;
            let h = h.max(16) as usize;
            self.render_w = w;
            self.render_h = h;
            let stride = (w * 4).div_ceil(64) * 64;
            self.render_stride = stride;
            self.render_buf.resize(stride * h, 0);
        }
    }
}

impl Default for MpvPlayer {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for MpvPlayer {}
unsafe impl Sync for MpvPlayer {}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        if let Some(ctx) = self.render_ctx.take() {
            if let Some(fns) = &self.fns {
                unsafe { (fns.mpv_render_context_free)(ctx) };
            }
        }
        if let (Some(handle), Some(fns)) = (self.handle.take(), self.fns.take()) {
            unsafe { (fns.mpv_terminate_destroy)(handle) };
        }
    }
}

impl LocalPlayer for MpvPlayer {
    fn open(&mut self, path: &Path) -> Result<(), PlayerError> {
        if self.unavailable {
            return Err(PlayerError::LibMpvUnavailable);
        }
        if !is_streaming_media_source(path) && !path.exists() {
            return Err(PlayerError::MissingMedia {
                path: path.display().to_string(),
            });
        }
        self.loaded_path = Some(path.to_path_buf());
        self.snapshot.state = PlayerState::Ready;
        self.snapshot.error_message = None;
        if self.handle.is_some() {
            self.load_current_media()?;
        }
        Ok(())
    }

    fn play(&mut self) -> Result<(), PlayerError> {
        if self.unavailable {
            return Err(PlayerError::LibMpvUnavailable);
        }
        if self.handle.is_none() && self.loaded_path.is_some() {
            // Simulation window before the native surface attaches: libmpv
            // has not been loaded yet, so no real decode can start here.
            // Real production playback always attaches the surface first
            // (which loads libmpv and opens the media); the play protocol
            // gates on attach failure through the player error state.
            self.snapshot.state = PlayerState::Playing;
            return Ok(());
        }
        let (handle, fns) = self.ensure_ready()?;
        let pause_false: i32 = 0;
        let pause_name = CString::new("pause").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    pause_name.as_ptr(),
                    MPV_FORMAT_FLAG,
                    &pause_false as *const i32 as *const c_void,
                ),
                &fns,
                "failed to start playback",
            )?;
        }
        self.snapshot.state = PlayerState::Playing;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), PlayerError> {
        if self.unavailable {
            return Err(PlayerError::LibMpvUnavailable);
        }
        if self.handle.is_none() && self.loaded_path.is_some() {
            self.snapshot.state = PlayerState::Paused;
            return Ok(());
        }
        let (handle, fns) = self.ensure_ready()?;
        let pause_true: i32 = 1;
        let pause_name = CString::new("pause").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    pause_name.as_ptr(),
                    MPV_FORMAT_FLAG,
                    &pause_true as *const i32 as *const c_void,
                ),
                &fns,
                "failed to pause playback",
            )?;
        }
        self.snapshot.state = PlayerState::Paused;
        Ok(())
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), PlayerError> {
        if self.unavailable {
            return Err(PlayerError::LibMpvUnavailable);
        }
        if self.handle.is_none() && self.loaded_path.is_some() {
            self.snapshot.position_ms = position_ms;
            return Ok(());
        }
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
                &fns,
                "failed to seek playback",
            )?;
        }
        self.snapshot.position_ms = position_ms;
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let clamped_volume = volume.clamp(0.0, 1.0);
        let vol_value: f64 = (clamped_volume * 100.0) as f64;
        let vol_name = CString::new("volume").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    vol_name.as_ptr(),
                    MPV_FORMAT_DOUBLE,
                    &vol_value as *const f64 as *const c_void,
                ),
                &fns,
                "failed to set volume",
            )?;
        }
        self.snapshot.volume = clamped_volume;
        Ok(())
    }

    fn set_playback_rate(&mut self, rate: f32) -> Result<(), PlayerError> {
        let (handle, fns) = self.ensure_ready()?;
        let rate = rate.clamp(0.25, 4.0);
        let rate_value: f64 = rate as f64;
        let speed_name = CString::new("speed").unwrap();
        unsafe {
            mpv_result(
                (fns.mpv_set_property)(
                    handle,
                    speed_name.as_ptr(),
                    MPV_FORMAT_DOUBLE,
                    &rate_value as *const f64 as *const c_void,
                ),
                &fns,
                "failed to set rate",
            )?;
        }
        self.snapshot.playback_rate = rate;
        Ok(())
    }

    fn snapshot(&self) -> PlayerSnapshot {
        let mut snap = self.snapshot.clone();
        if let (Some(handle), Some(fns)) = (self.handle, &self.fns) {
            unsafe {
                if let Some(pos) = mpv_get_double(handle, fns, "time-pos") {
                    snap.position_ms = (pos * 1000.0) as u64;
                }
                if let Some(dur) = mpv_get_double(handle, fns, "duration") {
                    snap.duration_ms = Some((dur * 1000.0) as u64);
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
        None
    }
    fn error_message(&self) -> Option<String> {
        self.snapshot.error_message.clone()
    }

    fn close(&mut self) {
        if let (Some(handle), Some(fns)) = (self.handle, &self.fns) {
            let stop_cmd = CString::new("stop").unwrap();
            let null_term = ptr::null();
            let args = [stop_cmd.as_ptr(), null_term];
            unsafe {
                (fns.mpv_command)(handle, args.as_ptr());
            }
        }
        if let Some(ctx) = self.render_ctx.take() {
            if let Some(fns) = &self.fns {
                unsafe { (fns.mpv_render_context_free)(ctx) };
            }
        }
        if let (Some(handle), Some(fns)) = (self.handle.take(), self.fns.take()) {
            unsafe { (fns.mpv_terminate_destroy)(handle) };
        }
        self.loaded_path = None;
        self.snapshot = PlayerSnapshot::default();
        self.surface_handle = None;
        self.render_w = 640;
        self.render_h = 480;
    }

    /// Detach from the native surface while REMEMBERING the media (see the
    /// trait docs). The mpv/render contexts are destroyed so the surface's
    /// NSView can be released; `loaded_path` and the snapshot (position,
    /// paused/playing state) survive for a clean re-attach.
    fn detach_native_surface(&mut self) {
        if let (Some(handle), Some(fns)) = (self.handle, &self.fns) {
            let stop_cmd = CString::new("stop").unwrap();
            let null_term = ptr::null();
            let args = [stop_cmd.as_ptr(), null_term];
            unsafe {
                (fns.mpv_command)(handle, args.as_ptr());
            }
        }
        if let Some(ctx) = self.render_ctx.take() {
            if let Some(fns) = &self.fns {
                unsafe { (fns.mpv_render_context_free)(ctx) };
            }
        }
        if let (Some(handle), Some(fns)) = (self.handle.take(), self.fns.take()) {
            unsafe { (fns.mpv_terminate_destroy)(handle) };
        }
        self.surface_handle = None;
        self.render_w = 640;
        self.render_h = 480;
        self.render_stride = 0;
        self.render_buf.clear();
        // loaded_path + snapshot intentionally survive.
    }

    fn attach_native_surface(&mut self, surface_handle: usize) -> Result<(), PlayerError> {
        if surface_handle == 0 {
            return Err(PlayerError::NativeSurfaceUnavailable {
                reason: "native surface handle is null".to_string(),
            });
        }
        if self.surface_handle == Some(surface_handle) && self.handle.is_some() {
            return Ok(());
        }
        if self.handle.is_some() {
            // Move semantics: an attach to a NEW surface while mpv is live
            // (React StrictMode remount racing the unmount detach, or a
            // window rebuild) tears down the old contexts first and reattaches
            // — the media reloads at the remembered position. The previous
            // hard error here surfaced as a sticky "MP-MEDIA-003 playback
            // command failed" the moment Cinema was entered twice.
            self.detach_native_surface();
        }
        let desired = self.snapshot.clone();
        let (handle, fns, render_ctx) = match Self::load_library(surface_handle) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.unavailable = true;
                self.snapshot.state = PlayerState::Error;
                self.snapshot.error_message = Some(error.to_string());
                return Err(error);
            }
        };
        self.handle = Some(handle);
        self.fns = Some(fns);
        self.surface_handle = Some(surface_handle);
        self.render_ctx = Some(render_ctx);
        let empty_stride = (self.render_w * 4).div_ceil(64) * 64;
        self.render_stride = empty_stride;
        self.render_buf.resize(empty_stride * self.render_h, 0);
        if self.loaded_path.is_some() {
            self.load_current_media()?;
            if desired.position_ms > 0 {
                self.seek(desired.position_ms)?;
            }
            if desired.state == PlayerState::Playing {
                self.play()?;
            }
        }
        Ok(())
    }

    fn presentation_status(&self) -> presentation::PlayerPresentationStatus {
        if self.handle.is_some() && self.surface_handle.is_some() {
            presentation::PlayerPresentationStatus::embedded_native(
                presentation::embedded_native_bridge(),
            )
        } else if self.handle.is_some() {
            presentation::PlayerPresentationStatus::native_render_host_required()
        } else {
            presentation::PlayerPresentationStatus::unavailable("libmpv is not initialized.")
        }
    }

    fn render_next_frame(&mut self) -> Option<(usize, Vec<u8>, usize, usize, usize)> {
        let (handle, fns) = match (self.handle, self.fns) {
            (Some(h), Some(f)) => (h, f),
            _ => return None,
        };
        let ctx = self.render_ctx?;
        if self.render_buf.is_empty() {
            self.ensure_render_buf(handle, &fns);
        }
        // Pump events to keep the mpv core alive during playback. A short
        // timeout prevents blocking the player lock for too long.
        unsafe { (fns.mpv_wait_event)(handle, 0.01) };
        let w = self.render_w;
        let h = self.render_h;
        let stride = self.render_stride;
        if stride == 0 || w == 0 || h == 0 {
            return None;
        }
        let sw_fmt = CString::new("bgr0").unwrap();
        let mut sw_size = [w as i32, h as i32];
        let mut stride_val = stride;
        let block_for_target: i32 = 0;
        let params = [
            MpvRenderParam {
                type_: 17,
                data: sw_size.as_mut_ptr() as *mut c_void,
            }, // SW_SIZE
            MpvRenderParam {
                type_: 18,
                data: sw_fmt.as_ptr() as *mut c_void,
            }, // SW_FORMAT
            MpvRenderParam {
                type_: 19,
                data: (&mut stride_val) as *mut usize as *mut c_void,
            }, // SW_STRIDE
            MpvRenderParam {
                type_: 20,
                data: self.render_buf.as_mut_ptr() as *mut c_void,
            }, // SW_POINTER
            MpvRenderParam {
                type_: 12,
                data: &block_for_target as *const i32 as *mut c_void,
            }, // BLOCK_FOR_TARGET_TIME=0
            MpvRenderParam {
                type_: 0,
                data: ptr::null_mut(),
            },
        ];
        let rc = unsafe { (fns.mpv_render_context_render)(ctx, params.as_ptr()) };
        if rc == 0 {
            let surface = self.surface_handle.unwrap_or(0);
            let data = self.render_buf.clone();
            Some((surface, data, w, h, stride))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MpvPlayer, PlayerError, PlayerState};
    use crate::media::player::LocalPlayer;
    use std::path::Path;

    #[test]
    fn mpv_player_creation_does_not_panic() {
        let _player = MpvPlayer::new();
        // Creation should never panic even if mpv is unavailable
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bundled_libmpv_path_resolves_correctly_inside_app_bundle() {
        let exe_dir = Path::new("/Applications/Movie Party.app/Contents/MacOS");
        let bundled = crate::media::player::bundled_libmpv_path(exe_dir);
        // Normalize the `..` component so the comparison reflects what the
        // filesystem resolves to at runtime.
        let normalized: std::path::PathBuf =
            bundled
                .components()
                .fold(std::path::PathBuf::new(), |mut acc, comp| {
                    match comp {
                        std::path::Component::ParentDir => {
                            acc.pop();
                        }
                        _ => acc.push(comp.as_os_str()),
                    }
                    acc
                });
        assert_eq!(
            normalized,
            Path::new("/Applications/Movie Party.app/Contents/Resources/mpv_runtime/libmpv.dylib"),
            "bundled path must resolve to Contents/Resources/mpv_runtime/"
        );
    }

    #[test]
    fn candidate_paths_include_bundled_before_homebrew() {
        let candidates = crate::media::player::candidate_libmpv_paths();
        let bundled_idx = candidates
            .iter()
            .position(|c| c.to_string_lossy().contains("mpv_runtime"));
        let homebrew_idx = candidates
            .iter()
            .position(|c| c.to_string_lossy().contains("/opt/homebrew/"));
        let usrlocal_idx = candidates
            .iter()
            .position(|c| c.to_string_lossy().contains("/usr/local/"));
        if let (Some(b), Some(h)) = (bundled_idx, homebrew_idx) {
            assert!(b < h, "bundled path must be checked before Homebrew path");
        }
        if let (Some(b), Some(u)) = (bundled_idx, usrlocal_idx) {
            assert!(b < u, "bundled path must be checked before /usr/local path");
        }
    }

    #[test]
    fn bundled_runtime_is_loadable_when_present() {
        // This test verifies that the BUNDLED runtime (staged by the build
        // script into src-tauri/mpv_runtime/) can be opened via libloading
        // and resolves the mpv client API symbols. It only runs when the
        // runtime actually exists (e.g. after `scripts/stage-libmpv-macos.sh`).
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let bundled_path = manifest_dir.join("mpv_runtime/libmpv.dylib");
        if !bundled_path.exists() {
            eprintln!("no bundled libmpv at {bundled_path:?} — skipping test");
            return;
        }
        let lib = unsafe { libloading::Library::new(&bundled_path) }
            .expect("bundled libmpv.dylib must be loadable via dlopen");
        let _fn: unsafe extern "C" fn() -> *mut std::ffi::c_void = unsafe {
            *lib.get::<unsafe extern "C" fn() -> *mut std::ffi::c_void>(b"mpv_create\0")
                .expect("mpv_create symbol must resolve from bundled libmpv")
        };
        // If we reach here, the library loaded and the symbol resolved.
        // (mpv_create itself requires a real process — we just verify the
        //  dylib object resolves correctly.)
    }

    #[test]
    fn unavailable_player_cannot_claim_playing() {
        // Simulate a failed native-surface attach: the player is marked
        // unavailable because libmpv could not be loaded. Every playback
        // command must fail loudly instead of silently claiming success.
        let mut player = MpvPlayer::new();
        player.unavailable = true;
        player.snapshot.state = PlayerState::Error;
        player.snapshot.error_message = Some("libmpv could not be loaded".to_string());
        player.loaded_path = Some(std::path::PathBuf::from("/fake/movie.mp4"));

        assert!(matches!(player.play(), Err(PlayerError::LibMpvUnavailable)));
        assert!(matches!(
            player.pause(),
            Err(PlayerError::LibMpvUnavailable)
        ));
        assert!(matches!(
            player.seek(10_000),
            Err(PlayerError::LibMpvUnavailable)
        ));
        assert!(matches!(
            player.open(std::path::Path::new("/fake/movie.mp4")),
            Err(PlayerError::LibMpvUnavailable)
        ));
        assert_eq!(player.snapshot().state, PlayerState::Error);
        assert_eq!(
            player.snapshot().error_message.as_deref(),
            Some("libmpv could not be loaded")
        );
    }
}
