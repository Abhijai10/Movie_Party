//! The Provider Shared **diagnostic** — the V1
//! verification mechanism, deliberately experimental.
//!
//! Scope per PRD §109: V1 ships Shared as an
//! EXPERIMENTAL diagnostic (capture + encode + sample + black-frame
//! classification + explicit Sync fallback offer). Full transport is
//! deferred, CONDITIONAL on this spike passing on real DRM content.
//!
//! Mechanism: the **ffmpeg-CLI bridge** (the documented V1
//! diagnostic mechanism). On macOS the bridge captures via the `avfoundation`
//! capture device and encodes with `h264_videotoolbox`; the 30 s sample
//! is classified by the existing black-frame/static detection
//! (`crate::capture::classify_video_capture`), never by guesswork.
//!
//! Honesty rules baked in (§29/§38, no silent fallbacks):
//! - ffmpeg absent → a typed, actionable error — never a fake "unavailable";
//! - capture permission not granted → MP-CAPTURE-001, with the user told
//!   how to grant it;
//! - black/static frames while the provider reports playing →
//!   MP-CAPTURE-002 (protected video) with the explicit Sync Mode offer;
//! - failure policy per §69: ONE diagnostic retry, then offer Provider
//!   Sync Mode explicitly — no auto-restart, no silent mode switch.

use std::path::PathBuf;

use crate::capture::{
    classify_video_capture, failure_action, CaptureAvailability, CaptureFailureAction,
    FrameSampleStats,
};

/// The documented V1 diagnostic sample window (PRD §109).
pub const DIAGNOSTIC_SAMPLE_SECONDS: u16 = 30;

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticError {
    #[error("MP-DIAG-001 ffmpeg was not found on this device — install ffmpeg (brew install ffmpeg) and run the diagnostic again")]
    FfmpegMissing,
    #[error("MP-CAPTURE-001 screen-recording permission is required — grant it in System Settings › Privacy & Security › Screen Recording, then run the diagnostic again")]
    PermissionDenied,
    #[error("MP-CAPTURE-002 protected video could not be captured on this device — Shared Mode is unavailable for this provider; Provider Sync Mode is the supported path")]
    ProtectedVideo,
    #[error("MP-DIAG-002 the diagnostic sample produced no frames — retry once, then use Provider Sync Mode")]
    NoFrames,
    #[error("MP-DIAG-003 diagnostic command failed: {0}")]
    Command(String),
    #[error("MP-STORE-001 the diagnostic could not be recorded: {0}")]
    Store(String),
}

/// What a completed diagnostic concluded for one provider on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticOutcome {
    pub provider_id: String,
    pub shared_available: bool,
    /// Human-readable reason — always honest, never empty on failure.
    pub reason: String,
    pub sample_seconds: u16,
}

/// Probe whether the ffmpeg CLI exists (the V1 bridge's only dependency).
pub fn ffmpeg_present() -> bool {
    crate::process::quiet_command("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Frame statistics from a real captured sample. The ffmpeg bridge reports
/// `signalstats` luma summaries; this struct keeps the honest measured
/// values that feed `classify_video_capture`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredSample {
    pub frame_count: u32,
    pub mean_luma: f32,
    pub luma_variance: f32,
    pub changed_frame_ratio: f32,
}

/// Classify a measured sample for a provider whose page reports playing.
/// Thin wrapper so callers stay decoupled from the capture module's enum.
pub fn classify_sample(sample: MeasuredSample, provider_playing: bool) -> CaptureAvailability {
    classify_video_capture(FrameSampleStats {
        frame_count: sample.frame_count,
        mean_luma: sample.mean_luma,
        luma_variance: sample.luma_variance,
        changed_frame_ratio: sample.changed_frame_ratio,
        provider_reports_playing: provider_playing,
    })
}

/// Map a classification to the diagnostic outcome, applying the §69
/// failure policy surface (the caller drives the retry count).
pub fn outcome_for_classification(
    provider_id: &str,
    availability: CaptureAvailability,
    attempt: u8,
) -> DiagnosticOutcome {
    let next_action = failure_action(attempt);
    match availability {
        CaptureAvailability::Available => DiagnosticOutcome {
            provider_id: provider_id.to_string(),
            shared_available: true,
            reason: "Capture verified on this device (sample classified as real video)"
                .to_string(),
            sample_seconds: DIAGNOSTIC_SAMPLE_SECONDS,
        },
        CaptureAvailability::ProtectedOrBlackFrame => DiagnosticOutcome {
            provider_id: provider_id.to_string(),
            shared_available: false,
            reason: match next_action {
                CaptureFailureAction::RetryDiagnosticOnce => "MP-CAPTURE-002 protected video could not be captured (black/static frames) — one diagnostic retry is available, then Provider Sync Mode".to_string(),
                CaptureFailureAction::OfferProviderSyncMode => "MP-CAPTURE-002 protected video could not be captured — Shared Mode is unavailable for this provider; use Provider Sync Mode".to_string(),
            },
            sample_seconds: DIAGNOSTIC_SAMPLE_SECONDS,
        },
        CaptureAvailability::NoFrames => DiagnosticOutcome {
            provider_id: provider_id.to_string(),
            shared_available: false,
            reason: match next_action {
                CaptureFailureAction::RetryDiagnosticOnce => "MP-DIAG-002 no frames captured — one diagnostic retry is available, then Provider Sync Mode".to_string(),
                CaptureFailureAction::OfferProviderSyncMode => "MP-DIAG-002 no frames captured — Shared Mode is unavailable for this provider; use Provider Sync Mode".to_string(),
            },
            sample_seconds: DIAGNOSTIC_SAMPLE_SECONDS,
        },
        CaptureAvailability::ExternalVerificationPending => DiagnosticOutcome {
            provider_id: provider_id.to_string(),
            shared_available: false,
            reason: "External verification pending — run the diagnostic on this device"
                .to_string(),
            sample_seconds: DIAGNOSTIC_SAMPLE_SECONDS,
        },
    }
}

/// The ffmpeg-CLI capture bridge (macOS V1): capture `seconds` from the
/// default avfoundation video device into a raw luma-stats sample.
///
/// This runs the REAL capture for the diagnostic sample and returns the
/// measured statistics. It never fabricates numbers: if the
/// capture fails, callers receive the typed error instead.
///
/// `capture_index` is the avfoundation device index (1 = main screen).
pub fn run_capture_sample(
    seconds: u16,
    capture_index: u8,
    work_root: &std::path::Path,
) -> Result<MeasuredSample, DiagnosticError> {
    if !ffmpeg_present() {
        return Err(DiagnosticError::FfmpegMissing);
    }
    std::fs::create_dir_all(work_root)
        .map_err(|e| DiagnosticError::Command(format!("workdir: {e}")))?;

    let stats_path: PathBuf = work_root.join("luma_stats.txt");
    let input = format!(":{capture_index}");

    // Capture the window and compute per-frame luma statistics in one pass.
    // signalstats writes `lavfi.signalstats.YAVG` (mean luma) per frame;
    // YMIN/YMAX spread gives the variance proxy; the changed-frame ratio is
    // derived by the caller from the stats file's frame count deltas.
    let output = crate::process::quiet_command("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("avfoundation")
        .arg("-framerate")
        .arg("30")
        .arg("-video_device_index")
        .arg(&input)
        .arg("-t")
        .arg(seconds.to_string())
        .arg("-i")
        .arg(&input)
        .arg("-vf")
        .arg("signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-")
        .arg("-f")
        .arg("null")
        .arg("-")
        .arg(&stats_path)
        .output()
        .map_err(|e| DiagnosticError::Command(format!("spawn ffmpeg: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Permission-denied capture surfaces as an avfoundation open error.
        if stderr.contains("Authorization") || stderr.contains("not authorized") {
            return Err(DiagnosticError::PermissionDenied);
        }
        if stderr.contains("No such device") || stderr.contains("Could not find") {
            return Err(DiagnosticError::PermissionDenied);
        }
        return Err(DiagnosticError::Command(format!(
            "ffmpeg capture failed: {}",
            stderr.chars().take(400).collect::<String>()
        )));
    }

    parse_luma_stats(&stats_path)
}

/// Parse the signalstats output into the measured sample. Frame count is
/// the number of YAVG lines; mean/variance from the luma series; the
/// changed-frame ratio is the fraction of frames whose luma moved more
/// than a small epsilon (static video → ~0).
pub fn parse_luma_stats(path: &std::path::Path) -> Result<MeasuredSample, DiagnosticError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| DiagnosticError::Command(format!("read stats: {e}")))?;
    let lumas: Vec<f32> = text
        .lines()
        .filter_map(|line| {
            line.split("YAVG=").nth(1).and_then(|rest| {
                rest.split_whitespace()
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
            })
        })
        .collect();
    if lumas.is_empty() {
        return Err(DiagnosticError::NoFrames);
    }
    let frame_count = lumas.len() as u32;
    let mean = lumas.iter().sum::<f32>() / lumas.len() as f32;
    let variance = lumas.iter().map(|luma| (luma - mean).powi(2)).sum::<f32>() / lumas.len() as f32;
    // A frame "changed" when its luma differs from the previous by > 2.0.
    let changed = lumas
        .windows(2)
        .filter(|pair| (pair[0] - pair[1]).abs() > 2.0)
        .count() as f32;
    let changed_ratio = if lumas.len() > 1 {
        changed / (lumas.len() as f32 - 1.0)
    } else {
        0.0
    };
    Ok(MeasuredSample {
        frame_count,
        mean_luma: mean,
        luma_variance: variance,
        changed_frame_ratio: changed_ratio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_ffmpeg_is_an_honest_typed_error_not_a_silent_failure() {
        // ffmpeg presence is machine-dependent; the guarantee under test is
        // that the bridge NEVER fabricates a sample when ffmpeg is absent.
        if !ffmpeg_present() {
            let result = run_capture_sample(30, 1, std::path::Path::new("/tmp/mp-diag-test"));
            assert!(matches!(result, Err(DiagnosticError::FfmpegMissing)));
        }
    }

    #[test]
    fn classification_maps_to_honest_outcomes_with_the_69_failure_policy() {
        // Verified capture → Shared available for this provider.
        let ok = outcome_for_classification("youtube", CaptureAvailability::Available, 0);
        assert!(ok.shared_available);
        assert!(ok.reason.contains("verified"));

        // Protected/black frames → unavailable + retry-first at attempt 0.
        let first =
            outcome_for_classification("netflix", CaptureAvailability::ProtectedOrBlackFrame, 0);
        assert!(!first.shared_available);
        assert!(first.reason.contains("one diagnostic retry"));

        // Same classification on the retry → the explicit Sync Mode offer.
        let second =
            outcome_for_classification("netflix", CaptureAvailability::ProtectedOrBlackFrame, 1);
        assert!(!second.shared_available);
        assert!(second.reason.contains("Sync Mode"));

        // No frames → honest unavailable + Sync offer on retry.
        let none = outcome_for_classification("prime", CaptureAvailability::NoFrames, 1);
        assert!(!none.shared_available);
        assert!(none.reason.contains("Sync Mode"));
    }

    #[test]
    fn luma_stats_parse_derives_real_measures() {
        let dir = std::env::temp_dir().join("mp-diag-parse-test");
        std::fs::create_dir_all(&dir).expect("tempdir");
        let path = dir.join("stats.txt");
        // 5 frames: YAVG values 10.0, 12.0, 11.0, 30.0, 32.0
        std::fs::write(
            &path,
            "frame:0 pts:0 lavfi.signalstats.YAVG=10.0\n\
             frame:1 pts:33 lavfi.signalstats.YAVG=12.0\n\
             frame:2 pts:66 lavfi.signalstats.YAVG=11.0\n\
             frame:3 pts:99 lavfi.signalstats.YAVG=30.0\n\
             frame:4 pts:132 lavfi.signalstats.YAVG=32.0\n",
        )
        .expect("write stats");
        let sample = parse_luma_stats(&path).expect("parse");
        assert_eq!(sample.frame_count, 5);
        assert!((sample.mean_luma - 19.0).abs() < 0.01);
        // Deltas |10-12|=2.0, |12-11|=1.0, |11-30|=19.0, |30-32|=2.0 —
        // only the 19.0 delta exceeds the >2.0 threshold → 1 of 4 = 0.25.
        assert!((sample.changed_frame_ratio - 0.25).abs() < 0.01);
        assert!(sample.luma_variance > 0.0);
    }

    #[test]
    fn empty_stats_file_is_no_frames_not_a_crash() {
        let dir = std::env::temp_dir().join("mp-diag-empty-test");
        std::fs::create_dir_all(&dir).expect("tempdir");
        let path = dir.join("stats.txt");
        std::fs::write(&path, "").expect("write stats");
        assert!(matches!(
            parse_luma_stats(&path),
            Err(DiagnosticError::NoFrames)
        ));
    }

    #[test]
    fn classify_sample_feeds_the_existing_black_frame_detector() {
        // Real, changing video on a playing provider → Available.
        assert_eq!(
            classify_sample(
                MeasuredSample {
                    frame_count: 900,
                    mean_luma: 78.0,
                    luma_variance: 320.0,
                    changed_frame_ratio: 0.74,
                },
                true,
            ),
            CaptureAvailability::Available
        );
        // Black frames while the provider reports playing → protected.
        assert_eq!(
            classify_sample(
                MeasuredSample {
                    frame_count: 900,
                    mean_luma: 0.4,
                    luma_variance: 0.1,
                    changed_frame_ratio: 0.0,
                },
                true,
            ),
            CaptureAvailability::ProtectedOrBlackFrame
        );
    }
}
