use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{telemetry::beta, APP_NAME, PROTOCOL_MAJOR, PROTOCOL_MINOR};

pub const UNKNOWN_PATH: &str = "UNKNOWN";
pub const DIAGNOSTIC_BUNDLE_FILE_EXTENSION: &str = "movie-party-diagnostics.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticExport {
    pub app_name: &'static str,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub app_version: String,
    pub platform: String,
    pub beta_events: Vec<DiagnosticBetaEvent>,
    pub redacted_log_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticBetaEvent {
    pub kind: &'static str,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticExportRequest {
    pub app_version: String,
    pub platform: String,
    pub beta_events: Vec<beta::BetaEvent>,
    pub raw_log_lines: Vec<String>,
    pub destination: PathBuf,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DiagnosticExportError {
    #[error("MP-DIAG-001 diagnostic destination must be a file path")]
    DestinationIsDirectory,
    #[error("MP-DIAG-002 diagnostic destination must use .json")]
    DestinationMustBeJson,
    #[error("MP-DIAG-003 diagnostic bundle could not be serialized: {0}")]
    Serialize(String),
    #[error("MP-DIAG-004 diagnostic bundle could not be written: {0}")]
    Write(String),
}

pub fn build_diagnostic_export(request: &DiagnosticExportRequest) -> DiagnosticExport {
    let bundle = beta::build_diagnostic_bundle(
        request.app_version.clone(),
        request.platform.clone(),
        request.beta_events.clone(),
        &request.raw_log_lines,
    );

    DiagnosticExport {
        app_name: APP_NAME,
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
        app_version: bundle.app_version,
        platform: bundle.platform,
        beta_events: bundle
            .beta_events
            .into_iter()
            .map(|event| DiagnosticBetaEvent {
                kind: beta_event_kind_name(event.kind),
                notes: event.notes,
            })
            .collect(),
        redacted_log_lines: bundle.redacted_log_lines,
    }
}

pub fn export_diagnostic_bundle(
    request: &DiagnosticExportRequest,
) -> Result<PathBuf, DiagnosticExportError> {
    validate_destination(&request.destination)?;
    let export = build_diagnostic_export(request);
    let payload = serde_json::to_vec_pretty(&export)
        .map_err(|error| DiagnosticExportError::Serialize(error.to_string()))?;

    fs::write(&request.destination, payload)
        .map_err(|error| DiagnosticExportError::Write(error.to_string()))?;

    Ok(request.destination.clone())
}

fn validate_destination(path: &Path) -> Result<(), DiagnosticExportError> {
    if path.is_dir() {
        return Err(DiagnosticExportError::DestinationIsDirectory);
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err(DiagnosticExportError::DestinationMustBeJson);
    }

    Ok(())
}

fn beta_event_kind_name(kind: beta::BetaEventKind) -> &'static str {
    match kind {
        beta::BetaEventKind::TrustedFriendInstall => "trusted_friend_install",
        beta::BetaEventKind::BugReport => "bug_report",
        beta::BetaEventKind::DiagnosticBundleExport => "diagnostic_bundle_export",
        beta::BetaEventKind::RealMovieNight => "real_movie_night",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn temp_json_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{nonce}.json"))
    }

    fn request(destination: PathBuf) -> DiagnosticExportRequest {
        DiagnosticExportRequest {
            app_version: "0.1.0".to_string(),
            platform: "macos".to_string(),
            beta_events: vec![beta::BetaEvent {
                kind: beta::BetaEventKind::DiagnosticBundleExport,
                notes: "manual export".to_string(),
            }],
            raw_log_lines: vec![
                "room_id=abc token=secret".to_string(),
                format!("tailscale_path={UNKNOWN_PATH} sync_drift_ms=42"),
            ],
            destination,
        }
    }

    #[test]
    fn diagnostic_export_redacts_secret_values_and_keeps_stable_metadata() {
        let export =
            build_diagnostic_export(&request(PathBuf::from("movie-party-diagnostics.json")));

        assert_eq!(export.app_name, APP_NAME);
        assert_eq!(export.protocol_major, 1);
        assert_eq!(export.protocol_minor, 0);
        assert_eq!(export.redacted_log_lines[0], "room_id=abc [REDACTED]");
        assert_eq!(export.beta_events[0].kind, "diagnostic_bundle_export");
    }

    #[test]
    fn writes_diagnostic_bundle_as_json_file() {
        let destination = temp_json_path("movie-party-diagnostics");
        let written = export_diagnostic_bundle(&request(destination.clone())).expect("write");

        let contents = fs::read_to_string(&written).expect("read");
        assert!(contents.contains("\"app_name\": \"Movie Party\""));
        assert!(contents.contains("[REDACTED]"));
        assert!(!contents.contains("token=secret"));

        fs::remove_file(written).expect("cleanup");
    }

    #[test]
    fn rejects_directory_or_non_json_destination() {
        let directory_error = export_diagnostic_bundle(&request(std::env::temp_dir()))
            .expect_err("directory rejected");
        assert_eq!(
            directory_error,
            DiagnosticExportError::DestinationIsDirectory
        );

        let txt_error = export_diagnostic_bundle(&request(PathBuf::from("diagnostics.txt")))
            .expect_err("extension rejected");
        assert_eq!(txt_error, DiagnosticExportError::DestinationMustBeJson);
    }
}
