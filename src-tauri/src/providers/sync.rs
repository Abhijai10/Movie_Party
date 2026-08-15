use super::{
    chrome::CdpCommand,
    generic::GenericProviderAdapter,
    hotstar, netflix, prime,
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

pub fn provider_id_from_str(provider_id: &str) -> Option<ProviderId> {
    match provider_id {
        youtube::PROVIDER_ID => Some(ProviderId::YouTube),
        netflix::PROVIDER_ID => Some(ProviderId::Netflix),
        prime::PROVIDER_ID => Some(ProviderId::Prime),
        hotstar::PROVIDER_ID => Some(ProviderId::JioHotstar),
        _ => None,
    }
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
