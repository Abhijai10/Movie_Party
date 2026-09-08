pub mod diagnostic;
pub mod macos;
pub mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMedium {
    Video,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAvailability {
    Available,
    ProtectedOrBlackFrame,
    NoFrames,
    ExternalVerificationPending,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSampleStats {
    pub frame_count: u32,
    pub mean_luma: f32,
    pub luma_variance: f32,
    pub changed_frame_ratio: f32,
    pub provider_reports_playing: bool,
}

pub fn classify_video_capture(stats: FrameSampleStats) -> CaptureAvailability {
    if stats.frame_count == 0 {
        return CaptureAvailability::NoFrames;
    }

    let looks_black = stats.mean_luma <= 3.0 && stats.luma_variance <= 1.0;
    let not_changing_while_playing =
        stats.provider_reports_playing && stats.changed_frame_ratio < 0.02;

    if looks_black || not_changing_while_playing {
        CaptureAvailability::ProtectedOrBlackFrame
    } else {
        CaptureAvailability::Available
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureFailureAction {
    RetryDiagnosticOnce,
    OfferProviderSyncMode,
}

pub fn failure_action(attempt_count: u8) -> CaptureFailureAction {
    if attempt_count == 0 {
        CaptureFailureAction::RetryDiagnosticOnce
    } else {
        CaptureFailureAction::OfferProviderSyncMode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_normal_changing_video_as_available() {
        assert_eq!(
            classify_video_capture(FrameSampleStats {
                frame_count: 900,
                mean_luma: 78.0,
                luma_variance: 320.0,
                changed_frame_ratio: 0.74,
                provider_reports_playing: true,
            }),
            CaptureAvailability::Available
        );
    }

    #[test]
    fn detects_black_or_static_protected_capture() {
        assert_eq!(
            classify_video_capture(FrameSampleStats {
                frame_count: 900,
                mean_luma: 0.4,
                luma_variance: 0.1,
                changed_frame_ratio: 0.0,
                provider_reports_playing: true,
            }),
            CaptureAvailability::ProtectedOrBlackFrame
        );
        assert_eq!(
            classify_video_capture(FrameSampleStats {
                frame_count: 900,
                mean_luma: 50.0,
                luma_variance: 200.0,
                changed_frame_ratio: 0.0,
                provider_reports_playing: true,
            }),
            CaptureAvailability::ProtectedOrBlackFrame
        );
    }

    #[test]
    fn capture_failure_policy_retries_once_then_falls_back() {
        assert_eq!(failure_action(0), CaptureFailureAction::RetryDiagnosticOnce);
        assert_eq!(
            failure_action(1),
            CaptureFailureAction::OfferProviderSyncMode
        );
        assert_eq!(
            failure_action(2),
            CaptureFailureAction::OfferProviderSyncMode
        );
    }
}
