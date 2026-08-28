//! Platform-native video host management for the Cinema webview.
//!
//! The host is positioned behind the transparent Cinema webview. React stays
//! above it for controls and overlays while libmpv receives the host's native
//! handle through its `wid` option.

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use super::PlayerError;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl NativeVideoBounds {
    fn validate(self) -> Result<Self, PlayerError> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width < 1.0
            || self.height < 1.0
        {
            return Err(PlayerError::NativeSurfaceUnavailable {
                reason: "invalid Cinema surface bounds".to_string(),
            });
        }
        Ok(self)
    }
}

#[derive(Default)]
pub struct NativeVideoSurfaceState {
    inner: Arc<Mutex<Option<PlatformSurface>>>,
}

impl NativeVideoSurfaceState {
    pub fn attach(
        &self,
        app: &AppHandle,
        bounds: NativeVideoBounds,
    ) -> Result<usize, PlayerError> {
        let bounds = bounds.validate()?;
        let webview = app
            .get_webview_window("main")
            .ok_or_else(|| PlayerError::NativeSurfaceUnavailable {
                reason: "main webview is unavailable".to_string(),
            })?;
        let output = std::sync::Arc::new(Mutex::new(Err(PlayerError::NativeSurfaceUnavailable {
            reason: "native surface creation did not run".to_string(),
        })));
        let result = output.clone();
        let state = self.inner.clone();
        #[cfg(windows)]
        let parent_handle = webview
            .hwnd()
            .map_err(|error| PlayerError::NativeSurfaceUnavailable {
                reason: error.to_string(),
            })?
            .0 as usize;
        #[cfg(not(windows))]
        let parent_handle = 0_usize;
        let scale_factor = webview.scale_factor().map_err(|error| {
            PlayerError::NativeSurfaceUnavailable {
                reason: error.to_string(),
            }
        })?;

        webview
            .with_webview(move |webview| {
                let mut guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                *result.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    PlatformSurface::attach_or_resize(
                        &mut guard,
                        webview,
                        bounds,
                        parent_handle,
                        scale_factor,
                    );
            })
            .map_err(|error| PlayerError::NativeSurfaceUnavailable {
                reason: error.to_string(),
            })?;

        let mut guard = output.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(
            &mut *guard,
            Err(PlayerError::NativeSurfaceUnavailable {
                reason: "native surface result was consumed".to_string(),
            }),
        )
    }

    pub fn detach(&self, app: &AppHandle) -> Result<(), PlayerError> {
        let webview = app
            .get_webview_window("main")
            .ok_or_else(|| PlayerError::NativeSurfaceUnavailable {
                reason: "main webview is unavailable".to_string(),
            })?;
        let state = self.inner.clone();
        webview
            .with_webview(move |webview| {
                let mut guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(surface) = guard.take() {
                    surface.detach(webview);
                }
            })
            .map_err(|error| PlayerError::NativeSurfaceUnavailable {
                reason: error.to_string(),
            })
    }
}

#[cfg(target_os = "macos")]
struct PlatformSurface {
    view: *mut std::ffi::c_void,
}

#[cfg(target_os = "macos")]
unsafe impl Send for PlatformSurface {}

#[cfg(target_os = "macos")]
impl PlatformSurface {
    fn attach_or_resize(
        current: &mut Option<Self>,
        webview: tauri::webview::PlatformWebview,
        bounds: NativeVideoBounds,
        _parent_handle: usize,
        _scale_factor: f64,
    ) -> Result<usize, PlayerError> {
        unsafe {
            let webview_view = webview.inner();
            if webview_view.is_null() {
                return Err(PlayerError::NativeSurfaceUnavailable {
                    reason: "WKWebView handle is null".to_string(),
                });
            }
            let parent = msg_id(webview_view, "superview");
            if parent.is_null() {
                return Err(PlayerError::NativeSurfaceUnavailable {
                    reason: "WKWebView has no native parent".to_string(),
                });
            }
            let frame = msg_rect(webview_view, "frame");
            let webview_bounds = msg_rect(webview_view, "bounds");
            let native_frame = NsRect {
                origin: NsPoint {
                    x: frame.origin.x + bounds.x,
                    y: frame.origin.y + webview_bounds.size.height - bounds.y - bounds.height,
                },
                size: NsSize {
                    width: bounds.width,
                    height: bounds.height,
                },
            };

            let surface = match current.as_mut() {
                Some(surface) => surface,
                None => {
                    let class = class("NSView")?;
                    let allocated = msg_id(class, "alloc");
                    let view = msg_id_rect(allocated, "initWithFrame:", native_frame);
                    if view.is_null() {
                        return Err(PlayerError::NativeSurfaceUnavailable {
                            reason: "could not allocate native video view".to_string(),
                        });
                    }
                    msg_void_bool(view, "setWantsLayer:", true);
                    msg_void_id_isize_id(parent, "addSubview:positioned:relativeTo:", view, -1, webview_view);
                    msg_void_bool(webview_view, "setDrawsBackground:", false);
                    *current = Some(Self { view });
                    current.as_mut().ok_or_else(|| PlayerError::NativeSurfaceUnavailable {
                        reason: "native video view was not retained".to_string(),
                    })?
                }
            };
            msg_void_rect(surface.view, "setFrame:", native_frame);
            Ok(surface.view as usize)
        }
    }

    fn detach(self, webview: tauri::webview::PlatformWebview) {
        unsafe {
            msg_void(self.view, "removeFromSuperview");
            msg_void_bool(webview.inner(), "setDrawsBackground:", true);
            msg_void(self.view, "release");
        }
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint { x: f64, y: f64 }
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsSize { width: f64, height: f64 }
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsRect { origin: NsPoint, size: NsSize }
#[cfg(target_os = "macos")]
#[link(name = "objc")]
extern "C" { fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void; fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void; fn objc_msgSend(); }
#[cfg(target_os = "macos")]
unsafe fn selector(name: &str) -> *mut std::ffi::c_void {
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    sel_registerName(bytes.as_ptr().cast())
}
#[cfg(target_os = "macos")]
unsafe fn class(name: &'static str) -> Result<*mut std::ffi::c_void, PlayerError> { let mut bytes = name.as_bytes().to_vec(); bytes.push(0); let value = objc_getClass(bytes.as_ptr().cast()); if value.is_null() { Err(PlayerError::NativeSurfaceUnavailable { reason: format!("missing Objective-C class {name}") }) } else { Ok(value) } }
#[cfg(target_os = "macos")]
unsafe fn msg_id(target: *mut std::ffi::c_void, name: &'static str) -> *mut std::ffi::c_void { let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void = std::mem::transmute(objc_msgSend as *const ()); f(target, selector(name)) }
#[cfg(target_os = "macos")]
unsafe fn msg_rect(target: *mut std::ffi::c_void, name: &'static str) -> NsRect { let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> NsRect = std::mem::transmute(objc_msgSend as *const ()); f(target, selector(name)) }
#[cfg(target_os = "macos")]
unsafe fn msg_id_rect(target: *mut std::ffi::c_void, name: &'static str, frame: NsRect) -> *mut std::ffi::c_void { let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, NsRect) -> *mut std::ffi::c_void = std::mem::transmute(objc_msgSend as *const ()); f(target, selector(name), frame) }
#[cfg(target_os = "macos")]
unsafe fn msg_void(target: *mut std::ffi::c_void, name: &'static str) { let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) = std::mem::transmute(objc_msgSend as *const ()); f(target, selector(name)); }
#[cfg(target_os = "macos")]
unsafe fn msg_void_bool(target: *mut std::ffi::c_void, name: &'static str, value: bool) { let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, bool) = std::mem::transmute(objc_msgSend as *const ()); f(target, selector(name), value); }
#[cfg(target_os = "macos")]
unsafe fn msg_void_rect(target: *mut std::ffi::c_void, name: &'static str, value: NsRect) { let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, NsRect) = std::mem::transmute(objc_msgSend as *const ()); f(target, selector(name), value); }
#[cfg(target_os = "macos")]
unsafe fn msg_void_id_isize_id(target: *mut std::ffi::c_void, name: &'static str, view: *mut std::ffi::c_void, position: isize, relative: *mut std::ffi::c_void) { let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, isize, *mut std::ffi::c_void) = std::mem::transmute(objc_msgSend as *const ()); f(target, selector(name), view, position, relative); }

#[cfg(windows)]
struct PlatformSurface {
    hwnd: *mut std::ffi::c_void,
}
#[cfg(windows)]
unsafe impl Send for PlatformSurface {}
#[cfg(windows)]
impl PlatformSurface {
    fn attach_or_resize(
        current: &mut Option<Self>,
        _webview: tauri::webview::PlatformWebview,
        bounds: NativeVideoBounds,
        parent_handle: usize,
        scale_factor: f64,
    ) -> Result<usize, PlayerError> {
        unsafe {
            let x = (bounds.x * scale_factor).round() as i32;
            let y = (bounds.y * scale_factor).round() as i32;
            let width = (bounds.width * scale_factor).round() as i32;
            let height = (bounds.height * scale_factor).round() as i32;
            let surface = match current.as_mut() {
                Some(surface) => surface,
                None => {
                    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
                    let hwnd = CreateWindowExW(
                        0,
                        class.as_ptr(),
                        std::ptr::null(),
                        WS_CHILD | WS_VISIBLE,
                        x,
                        y,
                        width,
                        height,
                        parent_handle as *mut std::ffi::c_void,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    if hwnd.is_null() {
                        return Err(PlayerError::NativeSurfaceUnavailable {
                            reason: "could not create the Windows video host".to_string(),
                        });
                    }
                    SetWindowPos(hwnd, HWND_BOTTOM, x, y, width, height, SWP_NOACTIVATE);
                    *current = Some(Self { hwnd });
                    current.as_mut().ok_or_else(|| PlayerError::NativeSurfaceUnavailable {
                        reason: "Windows video host was not retained".to_string(),
                    })?
                }
            };
            SetWindowPos(surface.hwnd, HWND_BOTTOM, x, y, width, height, SWP_NOACTIVATE);
            Ok(surface.hwnd as usize)
        }
    }

    fn detach(self, _webview: tauri::webview::PlatformWebview) {
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(windows)]
const WS_CHILD: u32 = 0x4000_0000;
#[cfg(windows)]
const WS_VISIBLE: u32 = 0x1000_0000;
#[cfg(windows)]
const SWP_NOACTIVATE: u32 = 0x0010;
#[cfg(windows)]
const HWND_BOTTOM: *mut std::ffi::c_void = 1 as *mut std::ffi::c_void;
#[cfg(windows)]
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
        parent: *mut std::ffi::c_void,
        menu: *mut std::ffi::c_void,
        instance: *mut std::ffi::c_void,
        parameter: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn SetWindowPos(
        hwnd: *mut std::ffi::c_void,
        insert_after: *mut std::ffi::c_void,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn DestroyWindow(hwnd: *mut std::ffi::c_void) -> i32;
}

#[cfg(not(any(target_os = "macos", windows)))]
struct PlatformSurface;
#[cfg(not(any(target_os = "macos", windows)))]
impl PlatformSurface {
    fn attach_or_resize(_current: &mut Option<Self>, _webview: tauri::webview::PlatformWebview, _bounds: NativeVideoBounds, _parent_handle: usize, _scale_factor: f64) -> Result<usize, PlayerError> { Err(PlayerError::NativeSurfaceUnavailable { reason: "native video embedding is unsupported on this platform".to_string() }) }
    fn detach(self, _webview: tauri::webview::PlatformWebview) {}
}

#[cfg(test)]
mod tests {
    use super::NativeVideoBounds;

    #[test]
    fn accepts_finite_cinema_bounds() {
        assert!(NativeVideoBounds {
            x: 12.0,
            y: 24.0,
            width: 640.0,
            height: 360.0,
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn rejects_empty_or_non_finite_cinema_bounds() {
        assert!(NativeVideoBounds {
            x: f64::NAN,
            y: 0.0,
            width: 640.0,
            height: 360.0,
        }
        .validate()
        .is_err());
        assert!(NativeVideoBounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 360.0,
        }
        .validate()
        .is_err());
    }
}
