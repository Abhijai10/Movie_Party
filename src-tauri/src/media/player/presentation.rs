#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlayerPresentationMode {
    EmbeddedNative,
    ExternalNativeWindow,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerPresentationStatus {
    pub mode: PlayerPresentationMode,
    pub platform: String,
    pub bridge: String,
    pub resize_managed: bool,
    pub ipc_managed: bool,
    pub lifecycle_managed: bool,
    pub message: String,
}

impl PlayerPresentationStatus {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            mode: PlayerPresentationMode::Unavailable,
            platform: std::env::consts::OS.to_string(),
            bridge: "none".to_string(),
            resize_managed: false,
            ipc_managed: false,
            lifecycle_managed: false,
            message: message.into(),
        }
    }

    pub fn external_native_window() -> Self {
        Self {
            mode: PlayerPresentationMode::ExternalNativeWindow,
            platform: std::env::consts::OS.to_string(),
            bridge: "libmpv default video output".to_string(),
            resize_managed: false,
            ipc_managed: true,
            lifecycle_managed: true,
            message: "Playback is controlled by Move Party, but video is not embedded in the app window yet."
                .to_string(),
        }
    }

    pub fn native_render_host_required() -> Self {
        Self {
            mode: PlayerPresentationMode::Unavailable,
            platform: std::env::consts::OS.to_string(),
            bridge: embedded_native_bridge().to_string(),
            resize_managed: false,
            ipc_managed: true,
            lifecycle_managed: true,
            message: "Waiting for the Cinema native video surface before starting local playback."
                .to_string(),
        }
    }

    pub fn embedded_native(bridge: impl Into<String>) -> Self {
        Self {
            mode: PlayerPresentationMode::EmbeddedNative,
            platform: std::env::consts::OS.to_string(),
            bridge: bridge.into(),
            resize_managed: true,
            ipc_managed: true,
            lifecycle_managed: true,
            message: "Playback is hosted inside the Move Party cinema surface.".to_string(),
        }
    }
}

pub fn embedded_native_bridge() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "NSView child host via libmpv wid"
    }
    #[cfg(target_os = "windows")]
    {
        "HWND child host via libmpv wid"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "unsupported platform"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_window_status_is_honest_about_embedding_gap() {
        let status = PlayerPresentationStatus::external_native_window();

        assert_eq!(status.mode, PlayerPresentationMode::ExternalNativeWindow);
        assert!(!status.resize_managed);
        assert!(status.ipc_managed);
        assert!(status.lifecycle_managed);
    }

    #[test]
    fn embedded_status_covers_resize_ipc_and_lifecycle() {
        let status = PlayerPresentationStatus::embedded_native(embedded_native_bridge());

        assert_eq!(status.mode, PlayerPresentationMode::EmbeddedNative);
        assert!(status.resize_managed);
        assert!(status.ipc_managed);
        assert!(status.lifecycle_managed);
    }

    #[test]
    fn render_host_required_does_not_claim_external_or_embedded_video() {
        let status = PlayerPresentationStatus::native_render_host_required();

        assert_eq!(status.mode, PlayerPresentationMode::Unavailable);
        assert!(status.ipc_managed);
        assert!(status.lifecycle_managed);
        assert!(!status.resize_managed);
    }
}
