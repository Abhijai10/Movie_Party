//! Native messaging host: manifest generation and request authorisation.
//!
//! The host process is treated as untrusted plumbing. It is spawned by the
//! browser, it forwards opaque frames, and it holds no secret of its own — the
//! shared pairing secret stays in Movie Party's OS-protected storage. Every
//! decision that actually protects the user lives in [`authorize`], which is
//! pure and tested; the stdin/stdout loop in the binary only moves bytes.

use super::{
    origin_is_allowed, verify, Capability, CompanionErrorCode, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};

/// Chrome/Edge register the host under this name in their NativeMessagingHosts
/// directory. Changing it means reinstalling both the manifest and the
/// extension, so it is part of the product's public surface.
pub const HOST_NAME: &str = "desktop.movieparty.companion";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompanionRequest {
    pub kind: String,
    /// A command proves it holds the session key. The handshake that establishes
    /// that key has none yet, so it carries a code instead and omits this.
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub code: Option<String>,
    pub sequence: u64,
    #[serde(default)]
    pub version: Option<u8>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub tab_id: Option<u64>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub position_ms: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub paused: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionResponse {
    pub kind: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
}

impl CompanionResponse {
    pub fn accepted(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            ok: true,
            error: None,
            guidance: None,
            capability: None,
        }
    }

    pub fn refused(kind: &str, code: CompanionErrorCode, capability: Option<Capability>) -> Self {
        Self {
            kind: kind.to_string(),
            ok: false,
            error: Some(format!("{code:?}")),
            guidance: Some(code.guidance().to_string()),
            capability: capability.map(|value| value.as_str().to_string()),
        }
    }
}

/// Which request kinds exist and what each one needs. Unknown kinds are refused,
/// never forwarded: an extension that grows a new capability has to be updated
/// on both sides deliberately.
pub fn capability_for(kind: &str) -> Option<Capability> {
    match kind {
        "handshake" => None,
        "identifyTab" => Some(Capability::IdentifyTab),
        "reportContent" => Some(Capability::ReportContent),
        "reportPlayback" => Some(Capability::ReportPlayback),
        "requestNavigation" => Some(Capability::RequestNavigation),
        "controlPlayback" => Some(Capability::ControlPlayback),
        _ => None,
    }
}

/// Cheap, fatal checks shared by every message. Cheapest first.
fn check_envelope(
    request: &CompanionRequest,
    secret: &[u8; 32],
    last_sequence: Option<u64>,
    raw_len: usize,
) -> Result<(), CompanionErrorCode> {
    if raw_len > MAX_FRAME_BYTES {
        return Err(CompanionErrorCode::FrameTooLarge);
    }
    if request
        .version
        .is_some_and(|version| version != PROTOCOL_VERSION)
    {
        return Err(CompanionErrorCode::VersionMismatch);
    }
    // Replay guard: a service worker can be suspended and revived, so sequences
    // only ever move forward within one paired session.
    if let Some(previous) = last_sequence {
        if request.sequence <= previous {
            return Err(CompanionErrorCode::MalformedFrame);
        }
    }
    if !verify(
        secret,
        &format!("{}:{}", request.sequence, request.kind),
        &request.token,
    ) {
        return Err(CompanionErrorCode::NotPaired);
    }
    Ok(())
}

/// The first message of a session authenticates with the pairing code the user
/// read off Movie Party's screen. There is no shared key to sign with yet —
/// establishing one is the whole point of this exchange — so a MAC here would be
/// circular. Chrome has already pinned the channel to one extension id; the code
/// proves a person, not a script, started it.
pub fn authorize_handshake(
    request: &CompanionRequest,
    expected_code: &str,
    raw_len: usize,
) -> Result<u8, CompanionErrorCode> {
    if request.kind != "handshake" {
        return Err(CompanionErrorCode::MalformedFrame);
    }
    if raw_len > MAX_FRAME_BYTES {
        return Err(CompanionErrorCode::FrameTooLarge);
    }
    if request
        .version
        .is_some_and(|version| version != PROTOCOL_VERSION)
    {
        return Err(CompanionErrorCode::VersionMismatch);
    }
    let provided = request
        .code
        .as_deref()
        .ok_or(CompanionErrorCode::NotPaired)?;
    if !super::equal_constant_time(provided, expected_code) {
        return Err(CompanionErrorCode::PairingFailed);
    }
    super::negotiate(request.version.unwrap_or(PROTOCOL_VERSION))
}

/// A content or control command. Every gate is explicit: known kind, valid
/// token, granted capability, and an on-allowlist destination when the command
/// claims one.
pub fn authorize_command(
    request: &CompanionRequest,
    secret: &[u8; 32],
    last_sequence: Option<u64>,
    granted: &[Capability],
    raw_len: usize,
) -> Result<Capability, CompanionErrorCode> {
    let capability = capability_for(&request.kind).ok_or(CompanionErrorCode::MalformedFrame)?;
    check_envelope(request, secret, last_sequence, raw_len)?;
    if !granted.contains(&capability) {
        return Err(CompanionErrorCode::CapabilityDisabled);
    }
    // Only commands that carry a destination are checked against it, so a
    // playback report is not refused for an absence it never claimed.
    if matches!(
        capability,
        Capability::IdentifyTab | Capability::ReportContent | Capability::RequestNavigation
    ) {
        let url = request
            .url
            .as_deref()
            .ok_or(CompanionErrorCode::MalformedFrame)?;
        if !origin_is_allowed(url) {
            return Err(CompanionErrorCode::OriginNotAllowed);
        }
    }
    Ok(capability)
}

/// Recompute the token a paired extension must present. Exposed so the app can
/// answer a challenge it issued without storing anything in the host.
pub fn token_for(secret: &[u8; 32], sequence: u64, kind: &str) -> String {
    super::sign(secret, &format!("{sequence}:{kind}"))
}

/// The manifest Chrome/Edge read. `allowed_origins` is the only thing that
/// decides which extension may launch this host, so it is built from a validated
/// id and never from a wildcard.
pub fn host_manifest(
    executable_path: &str,
    extension_id: &str,
) -> Result<String, CompanionErrorCode> {
    let origin = super::allowed_origin(extension_id)?;
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "Movie Party companion bridge for a provider tab you already have open",
        "path": executable_path,
        "type": "stdio",
        "allowed_origins": [origin],
    });
    serde_json::to_string_pretty(&manifest).map_err(|_| CompanionErrorCode::MalformedFrame)
}

/// The application identifier from `tauri.conf.json`. Movie Party's local data
/// directory is named for it, so the host must look in exactly that place or it
/// will never find the listener. A test keeps the two equal.
pub const APP_IDENTIFIER: &str = "app.movieparty.desktop";

/// Where Movie Party publishes its loopback companion endpoint while the user has
/// the companion switched on. The host never guesses an address: a missing file
/// means "Movie Party is not listening", which is reported, not retried silently.
pub fn endpoint_path() -> Option<std::path::PathBuf> {
    if let Some(explicit) = std::env::var_os("MOVIE_PARTY_COMPANION_ENDPOINT") {
        return Some(std::path::PathBuf::from(explicit));
    }
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|home| {
        std::path::PathBuf::from(home)
            .join("Library/Application Support")
            .join(APP_IDENTIFIER)
    });
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .map(|appdata| std::path::PathBuf::from(appdata).join(APP_IDENTIFIER));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base: Option<std::path::PathBuf> = None;
    base.map(|dir| dir.join("companion-endpoint.json"))
}

/// The endpoint document Movie Party writes: a loopback port and nothing else.
/// No secret travels in it, because the extension carries the proof itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub port: u16,
    pub protocol: u8,
}

pub fn parse_endpoint(text: &str) -> Result<Endpoint, CompanionErrorCode> {
    let endpoint: Endpoint =
        serde_json::from_str(text).map_err(|_| CompanionErrorCode::MalformedFrame)?;
    if endpoint.protocol != PROTOCOL_VERSION || endpoint.port == 0 {
        return Err(CompanionErrorCode::VersionMismatch);
    }
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::generate_pairing;

    fn new_pairing() -> crate::companion::Pairing {
        generate_pairing()
    }

    fn request(kind: &str, secret: &[u8; 32], sequence: u64) -> CompanionRequest {
        CompanionRequest {
            kind: kind.into(),
            token: token_for(secret, sequence, kind),
            code: None,
            sequence,
            version: Some(PROTOCOL_VERSION),
            url: Some("https://www.netflix.com/title/80181170".into()),
            tab_id: Some(7),
            action: None,
            position_ms: None,
            duration_ms: None,
            title: None,
            paused: None,
        }
    }

    const ALL: [Capability; 5] = [
        Capability::IdentifyTab,
        Capability::ReportContent,
        Capability::ReportPlayback,
        Capability::RequestNavigation,
        Capability::ControlPlayback,
    ];

    #[test]
    fn an_unpaired_or_tampered_token_is_refused_before_anything_else() {
        let pairing = new_pairing();
        let other = new_pairing();
        let message = request("reportContent", &pairing.secret, 1);
        assert_eq!(
            authorize_command(&message, &other.secret, None, &ALL, 512),
            Err(CompanionErrorCode::NotPaired)
        );
        let mut forged = message.clone();
        forged.token = String::new();
        assert_eq!(
            authorize_command(&forged, &pairing.secret, None, &ALL, 512),
            Err(CompanionErrorCode::NotPaired)
        );
        // A token minted for one operation cannot be spent on another.
        let mut swapped = message.clone();
        swapped.kind = "controlPlayback".into();
        assert_eq!(
            authorize_command(&swapped, &pairing.secret, None, &ALL, 512),
            Err(CompanionErrorCode::NotPaired)
        );
    }

    #[test]
    fn replayed_and_regressing_sequences_are_rejected() {
        let pairing = new_pairing();
        let secret = &pairing.secret;
        assert_eq!(
            authorize_command(
                &request("identifyTab", secret, 5),
                secret,
                Some(5),
                &ALL,
                256
            ),
            Err(CompanionErrorCode::MalformedFrame)
        );
        assert_eq!(
            authorize_command(
                &request("identifyTab", secret, 4),
                secret,
                Some(9),
                &ALL,
                256
            ),
            Err(CompanionErrorCode::MalformedFrame)
        );
        assert!(authorize_command(
            &request("identifyTab", secret, 6),
            secret,
            Some(5),
            &ALL,
            256
        )
        .is_ok());
    }

    #[test]
    fn unknown_and_oversized_frames_are_discarded() {
        let pairing = new_pairing();
        let secret = &pairing.secret;
        let mut ghost = request("readCookies", secret, 1);
        ghost.kind = "readCookies".into();
        ghost.token = token_for(secret, 1, "readCookies");
        assert_eq!(
            authorize_command(&ghost, secret, None, &ALL, 256),
            Err(CompanionErrorCode::MalformedFrame)
        );
        assert_eq!(
            authorize_command(
                &request("identifyTab", secret, 1),
                secret,
                None,
                &ALL,
                MAX_FRAME_BYTES + 1
            ),
            Err(CompanionErrorCode::FrameTooLarge)
        );
    }

    #[test]
    fn a_version_disagreement_fails_closed() {
        let pairing = new_pairing();
        let secret = &pairing.secret;
        let mut mismatch = request("identifyTab", secret, 1);
        mismatch.version = Some(PROTOCOL_VERSION + 1);
        assert_eq!(
            authorize_command(&mismatch, secret, None, &ALL, 256),
            Err(CompanionErrorCode::VersionMismatch)
        );
        // A command may omit the version once the handshake pinned it; that is
        // not read as "assume the newest one".
        let mut silent = request("identifyTab", secret, 1);
        silent.version = None;
        assert!(authorize_command(&silent, secret, None, &ALL, 256).is_ok());
    }

    #[test]
    fn only_a_real_handshake_opens_a_session() {
        let pairing = new_pairing();
        let secret = &pairing.secret;
        let code = pairing.code.clone();
        let hello = |code: Option<&str>, sequence: u64| CompanionRequest {
            kind: "handshake".into(),
            token: String::new(),
            code: code.map(str::to_string),
            sequence,
            version: Some(PROTOCOL_VERSION),
            url: None,
            tab_id: None,
            action: None,
            position_ms: None,
            duration_ms: None,
            title: None,
            paused: None,
        };
        assert_eq!(
            authorize_handshake(&hello(Some(&code), 1), &code, 256).unwrap(),
            PROTOCOL_VERSION
        );
        // A code that differs by one character must still be refused outright.
        let wrong = format!(
            "{}{}",
            if code.starts_with('A') { 'B' } else { 'A' },
            &code[1..]
        );
        assert_ne!(wrong, code);
        assert_eq!(
            authorize_handshake(&hello(Some(&wrong), 1), &code, 256),
            Err(CompanionErrorCode::PairingFailed)
        );
        assert_eq!(
            authorize_handshake(&hello(None, 1), &code, 256),
            Err(CompanionErrorCode::NotPaired)
        );
        assert_eq!(
            authorize_handshake(&hello(Some(&code), 1), &code, MAX_FRAME_BYTES + 1),
            Err(CompanionErrorCode::FrameTooLarge)
        );
        let mut future = hello(Some(&code), 1);
        future.version = Some(PROTOCOL_VERSION + 2);
        assert_eq!(
            authorize_handshake(&future, &code, 256),
            Err(CompanionErrorCode::VersionMismatch)
        );
        // A handshake is not a capability, so it cannot be replayed as a command,
        // and a command cannot be passed off as the handshake that has no key yet.
        assert_eq!(
            authorize_command(&hello(Some(&code), 2), secret, None, &ALL, 256),
            Err(CompanionErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn off_origins_are_refused_even_with_a_valid_token() {
        let pairing = new_pairing();
        let secret = &pairing.secret;
        for url in [
            "https://mail.google.com/inbox",
            "https://netflix.com.evil.test/title/1",
            "http://www.netflix.com/title/1",
            "https://www.netflix.com@evil.test/title/1",
            "file:///etc/passwd",
            "javascript:fetch('https://evil.test')",
        ] {
            let mut message = request("reportContent", secret, 1);
            message.url = Some(url.into());
            assert_eq!(
                authorize_command(&message, secret, None, &ALL, 256),
                Err(CompanionErrorCode::OriginNotAllowed),
                "accepted {url}"
            );
        }
        let mut absent = request("reportContent", secret, 1);
        absent.url = None;
        assert_eq!(
            authorize_command(&absent, secret, None, &ALL, 256),
            Err(CompanionErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn a_capability_the_user_never_granted_stays_switched_off() {
        let pairing = new_pairing();
        let secret = &pairing.secret;
        assert_eq!(
            authorize_command(
                &request("controlPlayback", secret, 1),
                secret,
                None,
                &[Capability::IdentifyTab, Capability::ReportContent],
                256
            ),
            Err(CompanionErrorCode::CapabilityDisabled)
        );
        assert_eq!(
            authorize_command(
                &request("identifyTab", secret, 1),
                secret,
                None,
                &[Capability::IdentifyTab],
                256
            )
            .ok(),
            Some(Capability::IdentifyTab)
        );
    }

    #[test]
    fn playback_reports_need_no_url_because_they_claim_no_destination() {
        let pairing = new_pairing();
        let secret = &pairing.secret;
        let mut message = request("reportPlayback", secret, 1);
        message.url = None;
        assert_eq!(
            authorize_command(&message, secret, None, &ALL, 256).ok(),
            Some(Capability::ReportPlayback)
        );
    }

    #[test]
    fn the_manifest_trusts_one_validated_extension_and_no_wildcard() {
        let id = "p".repeat(32);
        let text = host_manifest("/usr/local/bin/movie-party-companion-host", &id).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["name"], HOST_NAME.to_string());
        assert_eq!(value["type"], "stdio".to_string());
        assert_eq!(value["allowed_origins"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["allowed_origins"][0],
            format!("chrome-extension://{id}/").to_string()
        );
        assert!(!text.contains('*'));
        assert!(!text.contains("<all_urls>"));
        for bad in ["a".repeat(31), "a".repeat(33), "a".repeat(31) + "z"] {
            assert!(host_manifest("/tmp/host", &bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn the_endpoint_is_loopback_only_and_version_pinned() {
        assert_eq!(
            parse_endpoint(r#"{"port":54321,"protocol":1}"#)
                .unwrap()
                .port,
            54321
        );
        for bad in [
            r#"{"port":0,"protocol":1}"#,
            r#"{"port":54321,"protocol":2}"#,
            r#"{"port":"54321","protocol":1}"#,
            r#"{"port":54321,"protocol":1,"secret":"x"}"#,
            "not json",
        ] {
            assert!(parse_endpoint(bad).is_err(), "accepted {bad}");
        }
        // An explicit override always wins, so the host is testable without
        // touching a real application-support directory.
        std::env::set_var("MOVIE_PARTY_COMPANION_ENDPOINT", "/tmp/mp-endpoint.json");
        assert_eq!(
            endpoint_path().unwrap(),
            std::path::PathBuf::from("/tmp/mp-endpoint.json")
        );
        std::env::remove_var("MOVIE_PARTY_COMPANION_ENDPOINT");
    }

    #[test]
    fn the_host_looks_where_the_application_actually_stores_its_data() {
        // A wrong identifier would silently mean "no endpoint file", which the
        // user would experience as the companion simply never working.
        let config =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
                .expect("tauri.conf.json");
        let value: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(value["identifier"].as_str().unwrap(), APP_IDENTIFIER);
    }

    #[test]
    fn a_refusal_names_the_recovery_and_the_capability_it_cost() {
        let refusal = CompanionResponse::refused(
            "controlPlayback",
            CompanionErrorCode::CapabilityDisabled,
            Some(Capability::ControlPlayback),
        );
        assert!(!refusal.ok);
        assert_eq!(refusal.error.as_deref(), Some("CapabilityDisabled"));
        assert_eq!(refusal.capability.as_deref(), Some("control_playback"));
        assert!(refusal.guidance.unwrap().contains("switched off"));

        let accepted = CompanionResponse::accepted("identifyTab");
        assert!(accepted.ok);
        // A refusal never carries provider data, and an acceptance never carries
        // an empty error the UI would have to guess about.
        assert_eq!(
            serde_json::to_string(&accepted).unwrap(),
            r#"{"kind":"identifyTab","ok":true}"#
        );
    }

    #[test]
    fn the_request_shape_refuses_extra_fields_and_ignores_nothing() {
        let good = r#"{"kind":"identifyTab","token":"t","sequence":1,"version":1,"url":"https://www.netflix.com/title/1","tabId":2}"#;
        let parsed: CompanionRequest = serde_json::from_str(good).unwrap();
        assert_eq!(parsed.tab_id, Some(2));
        // A cookie or history field smuggled into a known message is a parse
        // failure, not a field nobody looked at.
        for extra in [
            r#"{"kind":"identifyTab","token":"t","sequence":1,"cookies":[]}"#,
            r#"{"kind":"identifyTab","token":"t","sequence":1,"historyUrl":"x"}"#,
            r#"{"kind":"identifyTab","token":"t","sequence":1,"password":"x"}"#,
        ] {
            assert!(
                serde_json::from_str::<CompanionRequest>(extra).is_err(),
                "parsed {extra}"
            );
        }
    }
}
