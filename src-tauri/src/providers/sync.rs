use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderReadiness {
    NotStarted,
    Launching,
    LoginRequired,
    Ready,
    Navigating,
    PlaybackReady,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapability {
    pub id: String,
    pub display_name: String,
    pub support_level: String,
    pub title_resolution: String,
    pub sync_available: bool,
    pub shared_available: bool,
    pub shared_reason: String,
    pub verification: String,
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

pub fn provider_capabilities() -> Vec<ProviderCapability> {
    [
        ("youtube", "YouTube", "SUPPORTED", "DIRECT_URL"),
        ("netflix", "Netflix", "SUPPORTED", "PROVIDER_SEARCH"),
        ("prime", "Prime Video", "SUPPORTED", "PROVIDER_SEARCH"),
        ("jiohotstar", "JioHotstar", "SUPPORTED", "PROVIDER_SEARCH"),
    ]
    .into_iter()
    .map(|(id, display_name, support_level, title_resolution)| ProviderCapability {
        id: id.to_string(),
        display_name: display_name.to_string(),
        support_level: support_level.to_string(),
        title_resolution: title_resolution.to_string(),
        sync_available: true,
        shared_available: false,
        shared_reason: "Provider Shared is experimental and unavailable until capture is verified on this device."
            .to_string(),
        verification: "EXTERNAL_VERIFICATION_PENDING".to_string(),
    })
    .collect()
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

/// Outcome of validating a requested provider mode on the stable path.
///
/// F56/F57: this is the security boundary that keeps Provider Shared out of the
/// stable app, and it is deliberately TWO checks. The experimental mode must
/// clear the per-device diagnostic gate, and every mode must then be exactly
/// `PROVIDER_SYNC` to proceed — so even a device whose diagnostic verified
/// capture still cannot enter the experimental path from a stable build.
pub enum ProviderModeGate {
    /// `PROVIDER_SYNC` — the supported provider path.
    Stable,
    /// `PROVIDER_SHARED` without a verified diagnostic on this device.
    SharedUnverified,
    /// Anything else, including `PROVIDER_SHARED` that cleared the diagnostic.
    Unsupported,
}

/// Decide whether a requested provider mode may proceed (F56/F57).
pub fn provider_mode_gate(mode: &str, shared_verified_on_device: bool) -> ProviderModeGate {
    if mode == "PROVIDER_SHARED" && !shared_verified_on_device {
        return ProviderModeGate::SharedUnverified;
    }
    if mode != "PROVIDER_SYNC" {
        return ProviderModeGate::Unsupported;
    }
    ProviderModeGate::Stable
}

/// True when a pasted generic link is one Movie Party should open.
///
/// Generic links are handed to a real managed Chrome instance, so a link that
/// resolves to the user's own machine or LAN would turn that browser into a
/// way to probe local services. The host must therefore be a public
/// destination (F46). The scheme is left as-is: plaintext `http` to a public
/// host is an intended use (see the `media.example` case below), so the
/// restriction stays on the destination rather than narrowing the feature.
pub fn generic_link_accepts_url(url: &str) -> bool {
    match host_from_url(url) {
        Some(host) => is_public_destination(host),
        None => false,
    }
}

/// The host portion of an authority, without any port or IPv6 brackets.
fn bare_host(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        // Bracketed IPv6 literal, optionally followed by `:port`.
        return rest.split(']').next().unwrap_or("");
    }
    // `host:port`. A bare (unbracketed) IPv6 literal cannot carry a port, so
    // splitting on the first ':' is safe here.
    host.split(':').next().unwrap_or(host)
}

/// True when `host` names a destination outside the local machine and LAN.
///
/// Rejects loopback, unspecified, private, link-local, CGNAT and reserved
/// addresses, `localhost`, mDNS `.local` names, and the numeric shorthand
/// forms (`127.1`, `2130706433`) that denote loopback without parsing as an
/// `Ipv4Addr`.
fn is_public_destination(host: &str) -> bool {
    let bare = bare_host(host).trim_end_matches('.');
    if bare.is_empty() {
        return false;
    }

    // A literal address must be a genuinely public one.
    if let Ok(address) = bare.parse::<std::net::IpAddr>() {
        return is_public_address(address);
    }

    let lowered = bare.to_ascii_lowercase();
    if lowered == "localhost" || lowered.ends_with(".localhost") || lowered.ends_with(".local") {
        return false;
    }
    // A public destination is a dotted name, and must not be all digits and
    // dots (which is how `127.1` slips past the address parser).
    if !lowered.contains('.') {
        return false;
    }
    !lowered
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.')
}

fn is_public_address(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 100.64.0.0/10 — RFC 6598 shared address space, which is also the
            // range Tailscale hands out; never a provider destination.
            let carrier_grade_nat = octets[0] == 100 && (64..128).contains(&octets[1]);
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || carrier_grade_nat
                || octets[0] >= 240)
        }
        std::net::IpAddr::V6(v6) => {
            // `::ffff:a.b.c.d` must be judged as the IPv4 address it carries.
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_address(std::net::IpAddr::V4(mapped));
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local())
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

/// Official landing page opened in the managed provider browser before any
/// Movie Party navigation. This is where the user authenticates on the
/// provider's own page; Movie Party never collects provider credentials.
pub fn provider_home_url(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::YouTube => "https://www.youtube.com/",
        ProviderId::Netflix => "https://www.netflix.com/browse",
        ProviderId::Prime => "https://www.primevideo.com/",
        ProviderId::JioHotstar => "https://www.hotstar.com/in",
    }
}

/// Provider search/navigation URL for a user-entered title. Returns `None`
/// for an empty title so Movie Party never navigates on a blank query. The
/// provider's own search page remains authoritative for catalogue results;
/// Movie Party does not scrape or resolve provider catalogues itself.
pub fn provider_search_url(provider: ProviderId, title: &str) -> Option<String> {
    let query = urlencode_query(title.trim());
    if query.is_empty() {
        return None;
    }
    match provider {
        ProviderId::YouTube => Some(format!(
            "https://www.youtube.com/results?search_query={query}"
        )),
        ProviderId::Netflix => Some(format!("https://www.netflix.com/search?q={query}")),
        ProviderId::Prime => Some(format!(
            "https://www.primevideo.com/search/ref=atv_nb_sr?phrase={query}"
        )),
        ProviderId::JioHotstar => Some(format!("https://www.hotstar.com/in/search?q={query}")),
    }
}

/// Maps the honest CDP detection signals to the provider readiness state.
///
/// A real media element on the provider page means playback is ready; a login
/// page means the user must authenticate on the provider's own page; otherwise
/// the authenticated session/browser is simply ready for navigation. `READY`
/// is never derived from Chrome launching alone — it always reflects the page
/// the managed browser actually reached.
pub fn readiness_from_detection(login_required: bool, media_detected: bool) -> ProviderReadiness {
    if media_detected {
        ProviderReadiness::PlaybackReady
    } else if login_required {
        ProviderReadiness::LoginRequired
    } else {
        ProviderReadiness::Ready
    }
}

/// Human-readable description of a readiness state for the provider snapshot.
pub fn readiness_description(readiness: ProviderReadiness) -> &'static str {
    match readiness {
        ProviderReadiness::NotStarted => "Not started",
        ProviderReadiness::Launching => "Launching provider browser",
        ProviderReadiness::LoginRequired => "Sign in to the provider on its own page",
        ProviderReadiness::Ready => "Provider authenticated and ready",
        ProviderReadiness::Navigating => "Opening the title in the provider",
        ProviderReadiness::PlaybackReady => "Playback ready",
        ProviderReadiness::Unavailable => "Provider unavailable",
        ProviderReadiness::Error => "Provider error",
    }
}

/// Percent-encodes a free-form title for use as a URL query value.
fn urlencode_query(input: &str) -> String {
    let mut out = String::new();
    for byte in input.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
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

/// map a CANONICAL coordinator commit to the provider
/// adapter action it implies. This is the bridge between the sync
/// engine's commit kinds ("PLAY"/"PAUSE"/"SEEK", position in ms) and the
/// provider CDP commands (actions with positions in seconds). None
/// means the commit does not drive the provider (unknown kind).
///
/// The coordinator remains the single authority (§15): providers
/// never see guest-originated actions directly — only canonical commits
/// produced by the host.
pub fn provider_sync_action_for_commit(
    commit_kind: &str,
    target_position_ms: u64,
) -> Option<ProviderSyncAction> {
    match commit_kind {
        "PLAY" => Some(ProviderSyncAction::Play),
        "PAUSE" => Some(ProviderSyncAction::Pause),
        "SEEK" => Some(ProviderSyncAction::Seek {
            seconds: target_position_ms as f64 / 1_000.0,
        }),
        _ => None,
    }
}

/// map a CDP/adapter execution failure to the honest runtime
/// error (§29 — no silent fallback; the user must see the
/// provider-mode failure, never a fake local playback). Mirrors
/// `map_provider_error`'s taxonomy for the command-execution path.
pub fn command_error_to_runtime_error(
    error: &crate::providers::chrome::ManagedChromeError,
) -> ProviderRuntimeError {
    match error {
        // The browser died or was closed: the page cannot accept commands.
        crate::providers::chrome::ManagedChromeError::Process(_) => {
            ProviderRuntimeError::ProviderPageClosed
        }
        // CDP-level failures on a live browser: the command was rejected
        // (page state does not accept it, player missing mid-navigation).
        crate::providers::chrome::ManagedChromeError::CdpCommand(_) => {
            ProviderRuntimeError::PlayerCommandRejected
        }
        crate::providers::chrome::ManagedChromeError::MalformedCdpResponse
        | crate::providers::chrome::ManagedChromeError::CdpTimeout
        | crate::providers::chrome::ManagedChromeError::Io(_) => ProviderRuntimeError::Unknown,
        // Launch/config errors cannot occur on an already-running session
        // but map honestly rather than silently succeeding.
        crate::providers::chrome::ManagedChromeError::ChromeUnavailable
        | crate::providers::chrome::ManagedChromeError::NonLocalCdpBind
        | crate::providers::chrome::ManagedChromeError::InvalidProviderProfile
        | crate::providers::chrome::ManagedChromeError::InvalidCdpPort => {
            ProviderRuntimeError::Unknown
        }
    }
}

/// the user-facing error code + description for a provider
/// runtime failure (stable MP-PROVIDER codes, §23). Shared-mode
/// fallback is never implied — Provider Sync failures surface as
/// Provider Sync failures.
pub fn provider_runtime_error_response(error: ProviderRuntimeError) -> (&'static str, String) {
    match error {
        ProviderRuntimeError::LoginRequired => (
            "MP-PROVIDER-004 sign in to the provider before starting playback",
            "Login required".to_string(),
        ),
        ProviderRuntimeError::MediaNotDetected => (
            "MP-PROVIDER-003 no playable media is open in the provider",
            "Media not detected".to_string(),
        ),
        ProviderRuntimeError::PlayerCommandRejected => (
            "MP-PROVIDER-003 the provider rejected the playback command",
            "Command rejected".to_string(),
        ),
        ProviderRuntimeError::ProviderPageClosed => (
            "MP-PROVIDER-003 the provider browser is closed",
            "Provider browser closed".to_string(),
        ),
        ProviderRuntimeError::Unknown => (
            "MP-PROVIDER-003 provider command failed",
            "Provider command failed".to_string(),
        ),
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
    fn capabilities_expose_sync_without_claiming_shared_readiness() {
        let capabilities = provider_capabilities();

        assert_eq!(capabilities.len(), 4);
        assert!(capabilities
            .iter()
            .all(|capability| capability.sync_available));
        assert!(capabilities
            .iter()
            .all(|capability| !capability.shared_available));
        assert!(capabilities
            .iter()
            .all(|capability| capability.verification == "EXTERNAL_VERIFICATION_PENDING"));
        assert!(capabilities
            .iter()
            .all(|capability| capability.support_level == "SUPPORTED"));
        assert_eq!(
            capabilities
                .iter()
                .find(|capability| capability.id == "youtube")
                .map(|capability| capability.title_resolution.as_str()),
            Some("DIRECT_URL")
        );
        assert!(capabilities
            .iter()
            .filter(|capability| capability.id != "youtube")
            .all(|capability| capability.title_resolution == "PROVIDER_SEARCH"));
    }

    #[test]
    fn provider_home_urls_are_https_and_never_empty() {
        for provider in [
            ProviderId::YouTube,
            ProviderId::Netflix,
            ProviderId::Prime,
            ProviderId::JioHotstar,
        ] {
            let url = provider_home_url(provider);
            assert!(
                url.starts_with("https://"),
                "home URL for {provider:?} must be https"
            );
            assert!(!url.is_empty());
        }
    }

    #[test]
    fn provider_search_url_returns_none_for_empty_title() {
        for provider in [
            ProviderId::YouTube,
            ProviderId::Netflix,
            ProviderId::Prime,
            ProviderId::JioHotstar,
        ] {
            assert_eq!(provider_search_url(provider, ""), None);
            assert_eq!(provider_search_url(provider, "   "), None);
        }
    }

    #[test]
    fn provider_search_url_encodes_non_empty_title() {
        for provider in [
            ProviderId::YouTube,
            ProviderId::Netflix,
            ProviderId::Prime,
            ProviderId::JioHotstar,
        ] {
            let url = provider_search_url(provider, "Inception 2010").unwrap();
            assert!(url.starts_with("https://"));
            assert!(url.contains("Inception"));
            assert!(url.contains('+') || url.contains("%20"));
        }
    }

    // ── canonical-commit dispatch + error mapping ─────

    #[test]
    fn canonical_commits_map_to_provider_actions_with_second_positions() {
        assert_eq!(
            provider_sync_action_for_commit("PLAY", 0),
            Some(ProviderSyncAction::Play)
        );
        assert_eq!(
            provider_sync_action_for_commit("PAUSE", 42_000),
            Some(ProviderSyncAction::Pause)
        );
        assert_eq!(
            provider_sync_action_for_commit("SEEK", 90_000),
            Some(ProviderSyncAction::Seek { seconds: 90.0 })
        );
        // Unknown commit kinds drive nothing (never a guessed action).
        assert_eq!(provider_sync_action_for_commit("REWIND", 1_000), None);
        assert_eq!(provider_sync_action_for_commit("", 0), None);
    }

    #[test]
    fn command_errors_map_to_honest_runtime_errors() {
        use crate::providers::chrome::ManagedChromeError;
        // A dead browser is a closed page, never a silent success.
        assert_eq!(
            command_error_to_runtime_error(&ManagedChromeError::Process("exited".into())),
            ProviderRuntimeError::ProviderPageClosed
        );
        // A rejected CDP command is a rejected player command.
        assert_eq!(
            command_error_to_runtime_error(&ManagedChromeError::CdpCommand("no player".into())),
            ProviderRuntimeError::PlayerCommandRejected
        );
        // Transport-level failures stay Unknown (no false specificity).
        assert_eq!(
            command_error_to_runtime_error(&ManagedChromeError::CdpTimeout),
            ProviderRuntimeError::Unknown
        );
        assert_eq!(
            command_error_to_runtime_error(&ManagedChromeError::MalformedCdpResponse),
            ProviderRuntimeError::Unknown
        );
    }

    #[test]
    fn runtime_error_responses_use_stable_provider_codes() {
        let (code, _) = provider_runtime_error_response(ProviderRuntimeError::LoginRequired);
        assert!(
            code.starts_with("MP-PROVIDER-004"),
            "login gate code: {code}"
        );
        for error in [
            ProviderRuntimeError::MediaNotDetected,
            ProviderRuntimeError::PlayerCommandRejected,
            ProviderRuntimeError::ProviderPageClosed,
            ProviderRuntimeError::Unknown,
        ] {
            let (code, _) = provider_runtime_error_response(error);
            assert!(code.starts_with("MP-PROVIDER-003"), "code: {code}");
        }
    }

    #[test]
    fn every_provider_dispatches_through_the_same_adapter_interface() {
        // The coordinator's dispatch is provider-agnostic: the same action
        // produces a CDP command for every provider (§8 — no
        // provider-specific logic outside adapters).
        for provider in [
            ProviderId::YouTube,
            ProviderId::Netflix,
            ProviderId::Prime,
            ProviderId::JioHotstar,
        ] {
            let play = command_for_action(provider, 1, ProviderSyncAction::Play);
            let pause = command_for_action(provider, 2, ProviderSyncAction::Pause);
            let seek = command_for_action(provider, 3, ProviderSyncAction::Seek { seconds: 12.5 });
            assert!(play.method.starts_with("Runtime."), "{provider:?}");
            assert!(pause.method.starts_with("Runtime."), "{provider:?}");
            assert!(seek.method.starts_with("Runtime."), "{provider:?}");
        }
    }

    #[test]
    fn readiness_from_detection_is_truthful_and_distinct() {
        assert_eq!(
            readiness_from_detection(false, false),
            ProviderReadiness::Ready
        );
        assert_eq!(
            readiness_from_detection(true, false),
            ProviderReadiness::LoginRequired
        );
        assert_eq!(
            readiness_from_detection(false, true),
            ProviderReadiness::PlaybackReady
        );
        assert_eq!(
            readiness_from_detection(true, true),
            ProviderReadiness::PlaybackReady
        );
    }

    #[test]
    fn readiness_description_maps_every_variant() {
        for variant in [
            ProviderReadiness::NotStarted,
            ProviderReadiness::Launching,
            ProviderReadiness::LoginRequired,
            ProviderReadiness::Ready,
            ProviderReadiness::Navigating,
            ProviderReadiness::PlaybackReady,
            ProviderReadiness::Unavailable,
            ProviderReadiness::Error,
        ] {
            let desc = readiness_description(variant);
            assert!(
                !desc.is_empty(),
                "description for {variant:?} must not be empty"
            );
        }
    }

    #[test]
    fn generic_links_require_an_http_url_with_a_host() {
        assert!(generic_link_accepts_url("https://example.com/movie.mp4"));
        assert!(generic_link_accepts_url("http://media.example/movie.m3u8"));
        assert!(!generic_link_accepts_url("movieparty://join/room"));
        assert!(!generic_link_accepts_url("not-a-url"));
    }

    // ── F46: generic links must not point at the local machine or LAN ────

    #[test]
    fn f46_generic_link_accepts_public_destinations() {
        for url in [
            "https://example.com/movie.mp4",
            "https://media.example/watch/1",
            "http://media.example/movie.m3u8",
            "https://www.netflix.com/watch/1",
            "https://a.b.c.example.co.uk/x",
        ] {
            assert!(generic_link_accepts_url(url), "{url} must be accepted");
        }
    }

    #[test]
    fn f46_generic_link_refuses_loopback_destinations() {
        for url in [
            "http://localhost/admin",
            "https://localhost:8443/",
            "http://LOCALHOST/",
            "http://sub.localhost/x",
            "http://127.0.0.1:8080/admin",
            "https://127.0.0.1/",
            "http://127.1/",
            "http://2130706433/",
            "http://[::1]/",
            "http://[::1]:9000/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            assert!(
                !generic_link_accepts_url(url),
                "{url} resolves to the local machine and must be refused"
            );
        }
    }

    #[test]
    fn f46_generic_link_refuses_private_and_local_networks() {
        for url in [
            "http://10.0.0.5/admin",
            "http://192.168.1.1/",
            "http://172.16.4.4/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.7:8080/",
            "http://0.0.0.0/",
            "http://printer.local/",
            "http://[fe80::1]/",
            "http://[fd00::1]/",
        ] {
            assert!(
                !generic_link_accepts_url(url),
                "{url} is a private/local destination and must be refused"
            );
        }
    }

    #[test]
    fn f46_generic_link_refuses_malformed_and_unsupported_input() {
        for url in [
            "",
            "   ",
            "not-a-url",
            "example.com/watch",
            "movieparty://join/room",
            "file:///Users/me/movie.mp4",
            "javascript:alert(1)",
            "https://",
            "http:///path",
            "https:///",
        ] {
            assert!(
                !generic_link_accepts_url(url),
                "{url:?} must not be accepted"
            );
        }
    }

    #[test]
    fn f46_supported_provider_urls_are_unaffected() {
        // The hardening must not disturb the allow-listed provider path.
        assert!(provider_accepts_url(
            ProviderId::Netflix,
            "https://www.netflix.com/watch/1"
        ));
        assert!(provider_accepts_url(
            ProviderId::YouTube,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
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

    // ── F56/F57: the Provider Shared boundary ────────────────────────────

    #[test]
    fn f56_stable_provider_sync_mode_is_allowed() {
        assert!(matches!(
            provider_mode_gate("PROVIDER_SYNC", false),
            ProviderModeGate::Stable
        ));
        // A verified diagnostic must not change that — the stable path is the
        // stable path either way.
        assert!(matches!(
            provider_mode_gate("PROVIDER_SYNC", true),
            ProviderModeGate::Stable
        ));
    }

    #[test]
    fn f56_shared_mode_is_refused_without_a_verified_diagnostic() {
        assert!(matches!(
            provider_mode_gate("PROVIDER_SHARED", false),
            ProviderModeGate::SharedUnverified
        ));
    }

    #[test]
    fn f57_shared_mode_stays_refused_even_on_a_verified_device() {
        // The second half of the double gate: the experimental path is
        // unreachable from a stable build no matter what the diagnostic says.
        assert!(matches!(
            provider_mode_gate("PROVIDER_SHARED", true),
            ProviderModeGate::Unsupported
        ));
    }

    #[test]
    fn f56_unknown_modes_are_refused() {
        for mode in [
            "",
            "OFF",
            "SHARED",
            "provider_shared",
            "PROVIDER_SYNC ",
            "GENERIC_LINK",
        ] {
            assert!(
                matches!(
                    provider_mode_gate(mode, false),
                    ProviderModeGate::Unsupported
                ),
                "{mode:?} must not open a provider path"
            );
        }
    }

    #[test]
    fn f57_no_mode_other_than_sync_ever_reaches_the_stable_path() {
        // Exhaustive over the modes the app can name, in both diagnostic
        // states: only PROVIDER_SYNC proceeds.
        for mode in ["PROVIDER_SYNC", "PROVIDER_SHARED", "", "OFF", "junk"] {
            for verified in [false, true] {
                let gate = provider_mode_gate(mode, verified);
                assert_eq!(
                    matches!(gate, ProviderModeGate::Stable),
                    mode == "PROVIDER_SYNC",
                    "{mode:?} verified={verified} must not be Stable"
                );
            }
        }
    }
}
