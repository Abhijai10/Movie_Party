pub const CAPTURE_API: &str = "Windows.Graphics.Capture";
pub const AUDIO_CAPTURE_API: &str = "WASAPI Application Loopback";
pub const DIAGNOSTIC_SAMPLE_SECONDS: u16 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCaptureDiagnosticPlan {
    pub video_api: &'static str,
    pub audio_api: &'static str,
    pub sample_seconds: u16,
    pub capture_selected_window: bool,
    pub capture_provider_audio: bool,
}

pub fn diagnostic_plan() -> WindowsCaptureDiagnosticPlan {
    WindowsCaptureDiagnosticPlan {
        video_api: CAPTURE_API,
        audio_api: AUDIO_CAPTURE_API,
        sample_seconds: DIAGNOSTIC_SAMPLE_SECONDS,
        capture_selected_window: true,
        capture_provider_audio: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_diagnostic_plan_matches_phase_spike_scope() {
        let plan = diagnostic_plan();

        assert_eq!(plan.video_api, "Windows.Graphics.Capture");
        assert_eq!(plan.audio_api, "WASAPI Application Loopback");
        assert_eq!(plan.sample_seconds, 30);
        assert!(plan.capture_selected_window);
        assert!(plan.capture_provider_audio);
    }
}
