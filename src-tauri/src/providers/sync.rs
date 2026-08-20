use super::{
    chrome::CdpCommand,
    generic::GenericProviderAdapter,
    hotstar::{self, HotstarAdapter},
    netflix::{self, NetflixAdapter},
    prime::{self, PrimeAdapter},
    youtube::{self, YoutubeAdapter},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    YouTube,
    Netflix,
    Prime,
    JioHotstar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSupportLevel {
    Supported,
    Experimental,
    SyncOnly,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    ExternalVerificationPending,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityRecord {
    pub provider: ProviderId,
    pub platform: Platform,
    pub sync_status: Option<ProviderSupportLevel>,
    pub verification: VerificationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderSyncAction {
    Play,
    Pause,
    Seek { seconds: f64 },
    SetPlaybackRate { rate: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRecoveryAction {
    Continue,
    StrictGlobalPause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRuntimeError {
    LoginRequired,
    MediaNotDetected,
    PlayerCommandRejected,
    ProviderPageClosed,
    Unknown,
}

pub fn provider_id_from_str(provider_id: &str) -> Option<ProviderId> {
    match provider_id {
        youtube::PROVIDER_ID => Some(ProviderId::YouTube),
        netflix::PROVIDER_ID => Some(ProviderId::Netflix),
        prime::PROVIDER_ID => Some(ProviderId::Prime),
        hotstar::PROVIDER_ID => Some(ProviderId::JioHotstar),
        _ => None,
    }
}

pub fn provider_accepts_url(provider: ProviderId, url: &str) -> bool {
    let host = match host_from_url(url) {
        Some(host) => host.to_ascii_lowercase(),
        None => return false,
    };

    match provider {
        ProviderId::YouTube => youtube::is_youtube_url(url),
        ProviderId::Netflix => host == "netflix.com" || host.ends_with(".netflix.com"),
        ProviderId::Prime => {
            host == "primevideo.com"
                || host.ends_with(".primevideo.com")
                || host == "amazon.com"
                || host.ends_with(".amazon.com")
        }
        ProviderId::JioHotstar => {
            host == "hotstar.com"
                || host.ends_with(".hotstar.com")
                || host == "jiocinema.com"
                || host.ends_with(".jiocinema.com")
        }
    }
}

fn host_from_url(url: &str) -> Option<&str> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))?;
    let end = without_scheme
        .find(['/', '?', '#'])
        .unwrap_or(without_scheme.len());
    let host = &without_scheme[..end];
    (!host.is_empty()).then_some(host)
}

pub fn unverified_compatibility_matrix() -> Vec<CompatibilityRecord> {
    [
        ProviderId::YouTube,
        ProviderId::Netflix,
        ProviderId::Prime,
        ProviderId::JioHotstar,
    ]
    .into_iter()
    .flat_map(|provider| {
        [Platform::Windows, Platform::MacOs]
            .into_iter()
            .map(move |platform| CompatibilityRecord {
                provider,
                platform,
                sync_status: None,
                verification: VerificationStatus::ExternalVerificationPending,
            })
    })
    .collect()
}

pub fn command_for_action(
    provider: ProviderId,
    command_id: u64,
    action: ProviderSyncAction,
) -> CdpCommand {
    match provider {
        ProviderId::YouTube => youtube_command(command_id, action),
        ProviderId::Netflix | ProviderId::Prime | ProviderId::JioHotstar => {
            generic_command(command_id, action)
        }
    }
}

pub fn login_required_command(provider: ProviderId, command_id: u64) -> CdpCommand {
    match provider {
        ProviderId::YouTube => GenericProviderAdapter.detect_page(command_id),
        ProviderId::Netflix => NetflixAdapter::default().login_required(command_id),
        ProviderId::Prime => PrimeAdapter::default().login_required(command_id),
        ProviderId::JioHotstar => HotstarAdapter::default().login_required(command_id),
    }
}

pub fn detect_media_command(provider: ProviderId, command_id: u64) -> CdpCommand {
    match provider {
        ProviderId::YouTube => YoutubeAdapter::default().detect_player(command_id),
        ProviderId::Netflix => NetflixAdapter::default().detect_media(command_id),
        ProviderId::Prime => PrimeAdapter::default().detect_media(command_id),
        ProviderId::JioHotstar => HotstarAdapter::default().detect_media(command_id),
    }
}

pub fn media_identity_command(provider: ProviderId, command_id: u64) -> CdpCommand {
    match provider {
        ProviderId::YouTube => GenericProviderAdapter.identify_media(command_id),
        ProviderId::Netflix => NetflixAdapter::default().media_identity(command_id),
        ProviderId::Prime => PrimeAdapter::default().media_identity(command_id),
        ProviderId::JioHotstar => HotstarAdapter::default().media_identity(command_id),
    }
}

pub fn position_command(provider: ProviderId, command_id: u64) -> CdpCommand {
    match provider {
        ProviderId::YouTube => YoutubeAdapter::default().get_position(command_id),
        ProviderId::Netflix | ProviderId::Prime | ProviderId::JioHotstar => {
            GenericProviderAdapter.get_position(command_id)
        }
    }
}

pub fn buffer_command(provider: ProviderId, command_id: u64) -> CdpCommand {
    match provider {
        ProviderId::YouTube => YoutubeAdapter::default().get_buffer_state(command_id),
        ProviderId::Netflix | ProviderId::Prime | ProviderId::JioHotstar => {
            GenericProviderAdapter.get_buffer_state(command_id)
        }
    }
}

pub fn map_provider_error(
    login_required: bool,
    media_detected: bool,
    target_open: bool,
) -> Option<ProviderRuntimeError> {
    if !target_open {
        return Some(ProviderRuntimeError::ProviderPageClosed);
    }
    if login_required {
        return Some(ProviderRuntimeError::LoginRequired);
    }
    if !media_detected {
        return Some(ProviderRuntimeError::MediaNotDetected);
    }
    None
}

pub fn recovery_action(
    buffer_ahead_seconds: Option<f64>,
    peer_connected: bool,
) -> ProviderRecoveryAction {
    if !peer_connected {
        return ProviderRecoveryAction::StrictGlobalPause;
    }

    match buffer_ahead_seconds {
        Some(seconds) if seconds >= 3.0 => ProviderRecoveryAction::Continue,
        _ => ProviderRecoveryAction::StrictGlobalPause,
    }
}

fn youtube_command(command_id: u64, action: ProviderSyncAction) -> CdpCommand {
    let adapter = YoutubeAdapter::default();
    let generic = GenericProviderAdapter;

    match action {
        ProviderSyncAction::Play => adapter.play(command_id),
        ProviderSyncAction::Pause => adapter.pause(command_id),
        ProviderSyncAction::Seek { seconds } => adapter.seek(command_id, seconds),
        ProviderSyncAction::SetPlaybackRate { rate } => generic.set_playback_rate(command_id, rate),
    }
}

fn generic_command(command_id: u64, action: ProviderSyncAction) -> CdpCommand {
    let adapter = GenericProviderAdapter;

    match action {
        ProviderSyncAction::Play => adapter.play(command_id),
        ProviderSyncAction::Pause => adapter.pause(command_id),
        ProviderSyncAction::Seek { seconds } => adapter.seek(command_id, seconds),
        ProviderSyncAction::SetPlaybackRate { rate } => adapter.set_playback_rate(command_id, rate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(command: &CdpCommand) -> &str {
        command.params["expression"]
            .as_str()
            .expect("expression string")
    }

    #[test]
    fn compatibility_matrix_does_not_claim_untested_support() {
        let matrix = unverified_compatibility_matrix();

        assert_eq!(matrix.len(), 8);
        assert!(matrix.iter().all(|record| record.sync_status.is_none()));
        assert!(matrix
            .iter()
            .all(|record| record.verification == VerificationStatus::ExternalVerificationPending));
    }

    #[test]
    fn provider_ids_map_locked_provider_modules() {
        assert_eq!(provider_id_from_str("youtube"), Some(ProviderId::YouTube));
        assert_eq!(provider_id_from_str("netflix"), Some(ProviderId::Netflix));
        assert_eq!(provider_id_from_str("prime"), Some(ProviderId::Prime));
        assert_eq!(
            provider_id_from_str("jiohotstar"),
            Some(ProviderId::JioHotstar)
        );
        assert_eq!(provider_id_from_str("unknown"), None);
    }

    #[test]
    fn provider_launch_rejects_mismatched_urls() {
        assert!(provider_accepts_url(
            ProviderId::YouTube,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(provider_accepts_url(
            ProviderId::Netflix,
            "https://www.netflix.com/watch/1"
        ));
        assert!(!provider_accepts_url(
            ProviderId::Netflix,
            "https://example.com/watch/1"
        ));
        assert!(!provider_accepts_url(
            ProviderId::YouTube,
            "https://www.youtube.com/watch?v=too-short"
        ));
    }

    #[test]
    fn sync_actions_build_host_committed_media_commands() {
        let play = command_for_action(ProviderId::YouTube, 1, ProviderSyncAction::Play);
        let seek = command_for_action(
            ProviderId::Netflix,
            2,
            ProviderSyncAction::Seek { seconds: 120.0 },
        );

        assert!(expression(&play).contains(".play()"));
        assert!(expression(&seek).contains("currentTime = 120"));
    }

    #[test]
    fn provider_runtime_commands_cover_detection_identity_position_and_buffer() {
        for provider in [
            ProviderId::YouTube,
            ProviderId::Netflix,
            ProviderId::Prime,
            ProviderId::JioHotstar,
        ] {
            assert_eq!(
                login_required_command(provider, 1).method,
                "Runtime.evaluate"
            );
            assert_eq!(detect_media_command(provider, 2).method, "Runtime.evaluate");
            assert_eq!(
                media_identity_command(provider, 3).method,
                "Runtime.evaluate"
            );
            assert_eq!(position_command(provider, 4).method, "Runtime.evaluate");
            assert_eq!(buffer_command(provider, 5).method, "Runtime.evaluate");
        }
    }

    #[test]
    fn maps_provider_runtime_errors_without_claiming_support() {
        assert_eq!(
            map_provider_error(false, true, false),
            Some(ProviderRuntimeError::ProviderPageClosed)
        );
        assert_eq!(
            map_provider_error(true, false, true),
            Some(ProviderRuntimeError::LoginRequired)
        );
        assert_eq!(
            map_provider_error(false, false, true),
            Some(ProviderRuntimeError::MediaNotDetected)
        );
        assert_eq!(map_provider_error(false, true, true), None);
    }

    #[test]
    fn buffer_or_disconnect_requires_strict_global_pause() {
        assert_eq!(
            recovery_action(Some(5.0), true),
            ProviderRecoveryAction::Continue
        );
        assert_eq!(
            recovery_action(Some(2.9), true),
            ProviderRecoveryAction::StrictGlobalPause
        );
        assert_eq!(
            recovery_action(None, true),
            ProviderRecoveryAction::StrictGlobalPause
        );
        assert_eq!(
            recovery_action(Some(10.0), false),
            ProviderRecoveryAction::StrictGlobalPause
        );
    }
}
