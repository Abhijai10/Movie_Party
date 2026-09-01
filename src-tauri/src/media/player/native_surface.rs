//! Platform-native video host management for the Cinema webview.
//!
//! The host is positioned behind the transparent Cinema webview. React stays
//! above it for controls and overlays while libmpv renders decoded frames
//! through its software render API into an RGBA buffer, which this module
//! pushes onto the host's CALayer (`display_frame`).

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
    pub fn attach(&self, app: &AppHandle, bounds: NativeVideoBounds) -> Result<usize, PlayerError> {
        let bounds = bounds.validate()?;
        let webview = app.get_webview_window("main").ok_or_else(|| {
            PlayerError::NativeSurfaceUnavailable {
                reason: "main webview is unavailable".to_string(),
            }
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
        let scale_factor =
            webview
                .scale_factor()
                .map_err(|error| PlayerError::NativeSurfaceUnavailable {
                    reason: error.to_string(),
                })?;

        webview
            .with_webview(move |webview| {
                let mut guard = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *result
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
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

        let mut guard = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(
            &mut *guard,
            Err(PlayerError::NativeSurfaceUnavailable {
                reason: "native surface result was consumed".to_string(),
            }),
        )
    }

    pub fn detach(&self, app: &AppHandle) -> Result<(), PlayerError> {
        let webview = app.get_webview_window("main").ok_or_else(|| {
            PlayerError::NativeSurfaceUnavailable {
                reason: "main webview is unavailable".to_string(),
            }
        })?;
        let state = self.inner.clone();
        webview
            .with_webview(move |webview| {
                let mut guard = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    layer: *mut std::ffi::c_void,
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
                    msg_void_id_isize_id(
                        parent,
                        "addSubview:positioned:relativeTo:",
                        view,
                        -1,
                        webview_view,
                    );
                    msg_void_bool(webview_view, "setDrawsBackground:", false);
                    let layer = msg_id(view, "layer");
                    if layer.is_null() {
                        return Err(PlayerError::NativeSurfaceUnavailable {
                            reason: "native video view has no layer".to_string(),
                        });
                    }
                    msg_void(layer, "retain");
                    *current = Some(Self { view, layer });
                    current
                        .as_mut()
                        .ok_or_else(|| PlayerError::NativeSurfaceUnavailable {
                            reason: "native video view was not retained".to_string(),
                        })?
                }
            };
            msg_void_rect(surface.view, "setFrame:", native_frame);
            Ok(surface.layer as usize)
        }
    }

    fn detach(self, webview: tauri::webview::PlatformWebview) {
        unsafe {
            msg_void(self.layer, "release");
            msg_void(self.view, "removeFromSuperview");
            msg_void_bool(webview.inner(), "setDrawsBackground:", true);
            msg_void(self.view, "release");
        }
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint {
    x: f64,
    y: f64,
}
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsSize {
    width: f64,
    height: f64,
}
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsRect {
    origin: NsPoint,
    size: NsSize,
}
#[cfg(target_os = "macos")]
#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
    fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
    fn objc_msgSend();
}

// ── RGBA frame display into the native surface ──────────────────────────────
//
// Movie Party renders libmpv frames into an RGBA buffer (software renderer)
// and pushes them onto the native NSView's backing layer. CALayer is
// documented thread-safe for property updates, so the render thread may set
// the layer contents directly.

#[cfg(target_os = "macos")]
struct DisplayFrame {
    surface: usize,
    width: usize,
    height: usize,
    stride: usize,
    data: Vec<u8>,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut std::ffi::c_void;
    fn CGDataProviderCreateWithCopyOfData(data: *const u8, size: isize) -> *mut std::ffi::c_void;
    fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        color_space: *mut std::ffi::c_void,
        bitmap_info: u32,
        provider: *mut std::ffi::c_void,
        decode: *const f64,
        should_interpolate: bool,
        intent: i32,
    ) -> *mut std::ffi::c_void;
    fn CGImageRelease(image: *mut std::ffi::c_void);
    fn CGDataProviderRelease(provider: *mut std::ffi::c_void);
    fn CGColorSpaceRelease(color_space: *mut std::ffi::c_void);
}

#[cfg(target_os = "macos")]
fn display_frame_on_current_thread(frame: &DisplayFrame) {
    unsafe {
        // `surface` is the pre-captured, retained CALayer pointer established
        // during attach on the main thread. CALayer property updates are
        // documented thread-safe, so the render thread may set `contents`
        // directly without touching the owning NSView.
        let layer = frame.surface as *mut std::ffi::c_void;
        if layer.is_null() {
            return;
        }
        if frame.width == 0 || frame.height == 0 || frame.stride < frame.width * 4 {
            return;
        }
        let data_len = frame.stride * frame.height;
        if frame.data.len() < data_len {
            return;
        }
        let color_space = CGColorSpaceCreateDeviceRGB();
        let provider = CGDataProviderCreateWithCopyOfData(frame.data.as_ptr(), data_len as isize);
        // bgr0 from mpv is byte order B,G,R,0 per pixel. CoreGraphics little-
        // endian RGBA (kCGImageAlphaNoneSkipFirst | kCGBitmapByteOrder32Little)
        // reads byte order B,G,R,A where A is ignored for a none-skip alpha.
        const KCG_IMAGE_ALPHA_NONE_SKIP_FIRST: u32 = 2;
        const KCG_BITMAP_BYTE_ORDER_32_LITTLE: u32 = 0x0000_2000;
        let image = CGImageCreate(
            frame.width,
            frame.height,
            8,
            32,
            frame.stride,
            color_space,
            KCG_IMAGE_ALPHA_NONE_SKIP_FIRST | KCG_BITMAP_BYTE_ORDER_32_LITTLE,
            provider,
            std::ptr::null(),
            false,
            0, // kCGRenderingIntentDefault
        );
        if !image.is_null() {
            msg_void_id(layer, "setContents:", image);
            CGImageRelease(image);
        }
        CGDataProviderRelease(provider);
        CGColorSpaceRelease(color_space);
    }
}

/// Push a rendered RGBA frame to the native surface. The layer update happens
/// synchronously on the calling (render) thread; CALayer property access is
/// thread-safe on macOS.
#[cfg(target_os = "macos")]
pub fn display_frame(surface: usize, width: usize, height: usize, stride: usize, data: Vec<u8>) {
    let frame = DisplayFrame {
        surface,
        width,
        height,
        stride,
        data,
    };
    display_frame_on_current_thread(&frame);
}

/// Push a rendered RGBA frame to the native surface. No-op when software
/// frame display is unsupported on this platform.
#[cfg(not(target_os = "macos"))]
pub fn display_frame(
    _surface: usize,
    _width: usize,
    _height: usize,
    _stride: usize,
    _data: Vec<u8>,
) {
}
#[cfg(target_os = "macos")]
unsafe fn selector(name: &str) -> *mut std::ffi::c_void {
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    sel_registerName(bytes.as_ptr().cast())
}
#[cfg(target_os = "macos")]
unsafe fn class(name: &'static str) -> Result<*mut std::ffi::c_void, PlayerError> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    let value = objc_getClass(bytes.as_ptr().cast());
    if value.is_null() {
        Err(PlayerError::NativeSurfaceUnavailable {
            reason: format!("missing Objective-C class {name}"),
        })
    } else {
        Ok(value)
    }
}
#[cfg(target_os = "macos")]
unsafe fn msg_id(target: *mut std::ffi::c_void, name: &'static str) -> *mut std::ffi::c_void {
    let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name))
}
#[cfg(target_os = "macos")]
unsafe fn msg_rect(target: *mut std::ffi::c_void, name: &'static str) -> NsRect {
    let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> NsRect =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name))
}
#[cfg(target_os = "macos")]
unsafe fn msg_id_rect(
    target: *mut std::ffi::c_void,
    name: &'static str,
    frame: NsRect,
) -> *mut std::ffi::c_void {
    let f: extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        NsRect,
    ) -> *mut std::ffi::c_void = std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name), frame)
}
#[cfg(target_os = "macos")]
unsafe fn msg_void(target: *mut std::ffi::c_void, name: &'static str) {
    let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name));
}
#[cfg(target_os = "macos")]
unsafe fn msg_void_bool(target: *mut std::ffi::c_void, name: &'static str, value: bool) {
    let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, bool) =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name), value);
}
#[cfg(target_os = "macos")]
unsafe fn msg_void_rect(target: *mut std::ffi::c_void, name: &'static str, value: NsRect) {
    let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, NsRect) =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name), value);
}
unsafe fn msg_void_id(
    target: *mut std::ffi::c_void,
    name: &'static str,
    value: *mut std::ffi::c_void,
) {
    let f: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void) =
        std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name), value);
}

#[cfg(target_os = "macos")]
unsafe fn msg_void_id_isize_id(
    target: *mut std::ffi::c_void,
    name: &'static str,
    view: *mut std::ffi::c_void,
    position: isize,
    relative: *mut std::ffi::c_void,
) {
    let f: extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        isize,
        *mut std::ffi::c_void,
    ) = std::mem::transmute(objc_msgSend as *const ());
    f(target, selector(name), view, position, relative);
}

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
                    current
                        .as_mut()
                        .ok_or_else(|| PlayerError::NativeSurfaceUnavailable {
                            reason: "Windows video host was not retained".to_string(),
                        })?
                }
            };
            SetWindowPos(
                surface.hwnd,
                HWND_BOTTOM,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
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
    fn attach_or_resize(
        _current: &mut Option<Self>,
        _webview: tauri::webview::PlatformWebview,
        _bounds: NativeVideoBounds,
        _parent_handle: usize,
        _scale_factor: f64,
    ) -> Result<usize, PlayerError> {
        Err(PlayerError::NativeSurfaceUnavailable {
            reason: "native video embedding is unsupported on this platform".to_string(),
        })
    }
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
