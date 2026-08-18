#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureEvent {
    ChromeCrash,
    HostCrash,
    GuestCrash,
    TransferInterrupted,
    TransferResumed,
    TailscaleDisconnect,
    TailscaleReconnect,
    NetworkChange,
    WifiDisconnect,
    SleepWake,
    ProviderLogout,
    ProviderPageClosed,
    PlayerFailure,
    CacheCorruption,
    MissingLocalFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryAction {
    RelaunchChromeAndRequireReadiness,
    RestoreHostSessionIfPossible,
    PauseBothAndWaitForGuestReconnect,
    RestoreTransferAndRebuildBuffer,
    ResumeAfterTransferCatchesUp,
    PauseBothAndWaitForTailscale,
    RevalidateNetworkAndRebuildBuffers,
    PauseBothAndWaitForNetwork,
    RecheckDevicesAndClocks,
    RequireProviderLoginInChrome,
    ReopenProviderTargetAndRequireReadiness,
    ReopenPlayerAndRequireReadiness,
    RemoveCorruptCacheAndReRequestChunks,
    AskHostToLocateFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPlan {
    pub action: RecoveryAction,
    pub pauses_playback_for_both: bool,
    pub requires_user_action: bool,
}

pub fn recovery_plan(event: FailureEvent) -> RecoveryPlan {
    match event {
        FailureEvent::ChromeCrash => RecoveryPlan {
            action: RecoveryAction::RelaunchChromeAndRequireReadiness,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::HostCrash => RecoveryPlan {
            action: RecoveryAction::RestoreHostSessionIfPossible,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::GuestCrash => RecoveryPlan {
            action: RecoveryAction::PauseBothAndWaitForGuestReconnect,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::TransferInterrupted => RecoveryPlan {
            action: RecoveryAction::RestoreTransferAndRebuildBuffer,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::TransferResumed => RecoveryPlan {
            action: RecoveryAction::ResumeAfterTransferCatchesUp,
            pauses_playback_for_both: false,
            requires_user_action: false,
        },
        FailureEvent::TailscaleDisconnect => RecoveryPlan {
            action: RecoveryAction::PauseBothAndWaitForTailscale,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::TailscaleReconnect | FailureEvent::NetworkChange => RecoveryPlan {
            action: RecoveryAction::RevalidateNetworkAndRebuildBuffers,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::WifiDisconnect => RecoveryPlan {
            action: RecoveryAction::PauseBothAndWaitForNetwork,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::SleepWake => RecoveryPlan {
            action: RecoveryAction::RecheckDevicesAndClocks,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::ProviderLogout => RecoveryPlan {
            action: RecoveryAction::RequireProviderLoginInChrome,
            pauses_playback_for_both: true,
            requires_user_action: true,
        },
        FailureEvent::ProviderPageClosed => RecoveryPlan {
            action: RecoveryAction::ReopenProviderTargetAndRequireReadiness,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::PlayerFailure => RecoveryPlan {
            action: RecoveryAction::ReopenPlayerAndRequireReadiness,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::CacheCorruption => RecoveryPlan {
            action: RecoveryAction::RemoveCorruptCacheAndReRequestChunks,
            pauses_playback_for_both: true,
            requires_user_action: false,
        },
        FailureEvent::MissingLocalFile => RecoveryPlan {
            action: RecoveryAction::AskHostToLocateFile,
            pauses_playback_for_both: true,
            requires_user_action: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_resilience_event_has_an_explicit_recovery_plan() {
        let events = [
            FailureEvent::ChromeCrash,
            FailureEvent::HostCrash,
            FailureEvent::GuestCrash,
            FailureEvent::TransferInterrupted,
            FailureEvent::TailscaleDisconnect,
            FailureEvent::TailscaleReconnect,
            FailureEvent::NetworkChange,
            FailureEvent::WifiDisconnect,
            FailureEvent::SleepWake,
            FailureEvent::ProviderLogout,
            FailureEvent::ProviderPageClosed,
            FailureEvent::PlayerFailure,
            FailureEvent::CacheCorruption,
            FailureEvent::MissingLocalFile,
        ];

        for event in events {
            let plan = recovery_plan(event);

            assert!(plan.pauses_playback_for_both);
        }
        assert!(!recovery_plan(FailureEvent::TransferResumed).pauses_playback_for_both);
    }

    #[test]
    fn provider_logout_and_missing_file_require_user_action() {
        assert!(recovery_plan(FailureEvent::ProviderLogout).requires_user_action);
        assert!(recovery_plan(FailureEvent::MissingLocalFile).requires_user_action);
        assert!(!recovery_plan(FailureEvent::CacheCorruption).requires_user_action);
    }

    #[test]
    fn reconnect_events_revalidate_network_before_resume() {
        assert_eq!(
            recovery_plan(FailureEvent::TailscaleReconnect).action,
            RecoveryAction::RevalidateNetworkAndRebuildBuffers
        );
        assert_eq!(
            recovery_plan(FailureEvent::NetworkChange).action,
            RecoveryAction::RevalidateNetworkAndRebuildBuffers
        );
    }
}
