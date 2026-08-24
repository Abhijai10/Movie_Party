pub const CAPTURE_API: &str = "ScreenCaptureKit";
pub const AUDIO_CAPTURE_API: &str = "ScreenCaptureKit Application Audio";
pub const ENCODE_API: &str = "VideoToolbox H264";
pub const DIAGNOSTIC_SAMPLE_SECONDS: u16 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacosCapturePermission {
    Unknown,
    Granted,
    Denied,
    ExternalVerificationPending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacosCaptureDiagnosticPlan {
    pub video_api: &'static str,
    pub audio_api: &'static str,
    pub encode_api: &'static str,
    pub sample_seconds: u16,
    pub capture_application: bool,
    pub capture_application_audio: bool,
    pub permission: MacosCapturePermission,
}

pub fn diagnostic_plan(permission: MacosCapturePermission) -> MacosCaptureDiagnosticPlan {
    MacosCaptureDiagnosticPlan {
        video_api: CAPTURE_API,
        audio_api: AUDIO_CAPTURE_API,
        encode_api: ENCODE_API,
        sample_seconds: DIAGNOSTIC_SAMPLE_SECONDS,
        capture_application: true,
        capture_application_audio: true,
        permission,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_diagnostic_plan_uses_screencapturekit_and_videotoolbox() {
        let plan = diagnostic_plan(MacosCapturePermission::ExternalVerificationPending);

        assert_eq!(plan.video_api, "ScreenCaptureKit");
        assert_eq!(plan.audio_api, "ScreenCaptureKit Application Audio");
        assert_eq!(plan.encode_api, "VideoToolbox H264");
        assert_eq!(plan.sample_seconds, 30);
        assert!(plan.capture_application);
        assert!(plan.capture_application_audio);
        assert_eq!(
            plan.permission,
            MacosCapturePermission::ExternalVerificationPending
        );
    }
}
