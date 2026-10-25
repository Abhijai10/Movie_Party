//! Opt-in existing-browser companion.
//!
//! The companion lets a user point Movie Party at a provider tab that is already
//! open and already signed in, instead of launching managed Chrome. It is the
//! only path in this application that touches a browser the user uses for
//! ordinary browsing, so the rules here are deliberately narrower than the
//! managed-Chrome path:
//!
//! - Chrome/Edge talk to Movie Party through a native messaging host, never
//!   through CDP. Attaching a debugger to the user's default profile would hand
//!   Movie Party every other site they have open, so it is refused by design.
//! - The extension proves it is paired before any content command is accepted.
//!   The pairing secret lives in OS-protected storage, alongside the device key.
//! - Every operation names the capability it needs, so a capability the browser,
//!   the provider or policy does not allow is switched off and explained rather
//!   than silently degraded.
//! - Nothing here reads cookies, credentials, auth headers or history. The
//!   protocol has no method that could ask for them, which the security tests
//!   assert against the extension manifest as well.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod host;

/// One protocol revision. A host and an extension that disagree must fail closed.
pub const PROTOCOL_VERSION: u8 = 1;
/// A native message is JSON bounded by Chrome at 1 MB; Movie Party is far
/// tighter because every legitimate message is small.
pub const MAX_FRAME_BYTES: usize = 32 * 1024;
/// A pairing code is short and human-typed, so attempts are capped and the
/// secret is rotated rather than allowed to brute-force.
pub const MAX_PAIRING_ATTEMPTS: u8 = 5;
/// OS-protected storage label for the companion shared secret.
pub const SECRET_LABEL: &str = "movie-party-companion-secret";

/// The providers a companion may report on. This deliberately delegates to
/// `providers::content::provider_page`, the same authority the managed-Chrome
/// path already uses, so the companion can never accept a host the rest of the
/// application rejects — or refuse one it has always supported.
pub const COMPANION_PROVIDERS: [&str; 4] = ["youtube", "netflix", "prime", "jiohotstar"];

pub fn origin_is_allowed(url: &str) -> bool {
    COMPANION_PROVIDERS
        .iter()
        .any(|provider| crate::providers::content::provider_page(provider, url))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompanionErrorCode {
    ExtensionMissing,
    PermissionDenied,
    BrowserClosed,
    TabChanged,
    VersionMismatch,
    NotPaired,
    PairingFailed,
    OriginNotAllowed,
    FrameTooLarge,
    MalformedFrame,
    CapabilityDisabled,
    HostBusy,
}

impl CompanionErrorCode {
    /// Actionable copy: name the problem and the recovery, never a stack trace.
    pub fn guidance(self) -> &'static str {
        match self {
            Self::ExtensionMissing => {
                "Movie Party could not reach the companion host. Install it from Settings > Companion, then reload the extension."
            }
            Self::PermissionDenied => {
                "The companion was not allowed to read this tab. Choose the tab again and accept the browser's prompt; nothing else changes."
            }
            Self::BrowserClosed => {
                "Your browser closed while the companion was connected. Reopen it and pick the tab again."
            }
            Self::TabChanged => {
                "The companion tab navigated or closed. Re-select the tab before continuing."
            }
            Self::VersionMismatch => {
                "The extension and Movie Party speak different companion protocols. Update both, then reconnect."
            }
            Self::NotPaired => {
                "Pair the extension with Movie Party first. The pairing code is shown in Settings > Companion."
            }
            Self::PairingFailed => {
                "That pairing code was rejected. Generate a new one in Settings > Companion and try again."
            }
            Self::OriginNotAllowed => {
                "The companion only reports provider pages. This tab is not a supported provider site, so nothing was sent."
            }
            Self::FrameTooLarge => "The companion message was larger than the protocol allows.",
            Self::MalformedFrame => "The companion message was not well-formed, so it was discarded.",
            Self::CapabilityDisabled => {
                "This browser or provider does not permit that operation, so it is switched off."
            }
            Self::HostBusy => {
                "Another browser window already holds the companion link. Close it and try again."
            }
        }
    }
}

/// The single operation the protocol exposes to Movie Party.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    IdentifyTab,
    ReportContent,
    ReportPlayback,
    RequestNavigation,
    ControlPlayback,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdentifyTab => "identify_tab",
            Self::ReportContent => "report_content",
            Self::ReportPlayback => "report_playback",
            Self::RequestNavigation => "request_navigation",
            Self::ControlPlayback => "control_playback",
        }
    }

    /// A capability the browser, the provider or the user has not granted is
    /// disabled with a reason; it never silently pretends to work.
    pub fn reason_when_disabled(self) -> &'static str {
        match self {
            Self::IdentifyTab => "The extension needs the tab you choose to be a provider page.",
            Self::ReportContent => "Content identity comes from the tab URL, which the extension only reads when you select it.",
            Self::ReportPlayback => "Playback state requires a visible media element on the provider page.",
            Self::RequestNavigation => "Navigation always needs your explicit click; the companion never moves a tab on its own.",
            Self::ControlPlayback => "Play, pause and seek only work where the adapter has verified them for this provider.",
        }
    }
}

/// Envelope shared by both directions. `kind` selects the meaning, and a frame
/// without one is refused rather than guessed at.
pub fn encode_frame(value: &serde_json::Value) -> Result<Vec<u8>, CompanionErrorCode> {
    let text = serde_json::to_vec(value).map_err(|_| CompanionErrorCode::MalformedFrame)?;
    if text.len() > MAX_FRAME_BYTES {
        return Err(CompanionErrorCode::FrameTooLarge);
    }
    let mut out = Vec::with_capacity(4 + text.len());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(&text);
    Ok(out)
}

pub fn decode_frame(bytes: &[u8]) -> Result<serde_json::Value, CompanionErrorCode> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CompanionErrorCode::FrameTooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CompanionErrorCode::MalformedFrame)?;
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| CompanionErrorCode::MalformedFrame)?;
    if !value.is_object() || value.get("kind").and_then(|v| v.as_str()).is_none() {
        return Err(CompanionErrorCode::MalformedFrame);
    }
    Ok(value)
}

/// Chrome and Edge extension IDs are 32 characters from `a..=p`.
pub fn is_valid_extension_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() && (b'a'..=b'p').contains(&byte))
}

/// The exact origin a native messaging host manifest may trust. Anything looser
/// — a wildcard, a missing scheme, a path — would let another extension pair.
pub fn allowed_origin(extension_id: &str) -> Result<String, CompanionErrorCode> {
    if !is_valid_extension_id(extension_id) {
        return Err(CompanionErrorCode::MalformedFrame);
    }
    Ok(format!("chrome-extension://{extension_id}/"))
}

/// A pairing code is what the human types; the secret is what it derives from.
/// The code never leaves the app and the browser never sees the raw secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    pub code: String,
    pub secret: [u8; 32],
}

/// Ambiguous glyphs are excluded: the code is read off a screen and typed.
const CODE_ALPHABET: [u8; 32] = *b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn generate_pairing() -> Pairing {
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    let mut raw = [0u8; 8];
    rand::rng().fill_bytes(&mut raw);
    let code = raw
        .iter()
        .map(|byte| CODE_ALPHABET[(*byte as usize) % CODE_ALPHABET.len()] as char)
        .collect::<String>();
    Pairing { code, secret }
}

/// RFC 2104 HMAC-SHA256. `sha2` is already a dependency and WebCrypto computes
/// the same MAC inside the extension, so pairing needs no new crate on either
/// side and both ends are checkable against the published test vectors.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        block[..digest.len()].copy_from_slice(&digest);
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner_key = block;
    let mut outer_key = block;
    for byte in &mut inner_key {
        *byte ^= 0x36;
    }
    for byte in &mut outer_key {
        *byte ^= 0x5c;
    }
    let inner = Sha256::digest([inner_key.as_slice(), message].concat());
    Sha256::digest([outer_key.as_slice(), inner.as_slice()].concat()).into()
}

pub fn sign(secret: &[u8; 32], challenge: &str) -> String {
    URL_SAFE_NO_PAD.encode(hmac_sha256(secret, challenge.as_bytes()))
}

/// Compare without an early exit. Length is public, content is not.
pub fn equal_constant_time(actual: &str, expected: &str) -> bool {
    let (a, b) = (actual.as_bytes(), expected.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn verify(secret: &[u8; 32], challenge: &str, candidate: &str) -> bool {
    equal_constant_time(candidate, &sign(secret, challenge))
}

/// The app and the extension must agree on one revision before content moves.
pub fn negotiate(version: u8) -> Result<u8, CompanionErrorCode> {
    if version == PROTOCOL_VERSION {
        Ok(PROTOCOL_VERSION)
    } else {
        Err(CompanionErrorCode::VersionMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_reject_oversize_malformed_and_typeless() {
        let message = serde_json::json!({"kind": "handshake", "version": 1});
        let encoded = encode_frame(&message).unwrap();
        assert_eq!(
            decode_frame(&encoded[4..]).unwrap()["kind"],
            "handshake".to_string()
        );
        assert_eq!(
            u32::from_le_bytes(encoded[..4].try_into().unwrap()) as usize,
            encoded.len() - 4
        );
        assert_eq!(
            encode_frame(&serde_json::json!({"kind": "x", "blob": "a".repeat(MAX_FRAME_BYTES)}))
                .unwrap_err(),
            CompanionErrorCode::FrameTooLarge
        );
        for bad in [
            b"{not json".as_slice(),
            br#"{"noKind":1}"#.as_slice(),
            br#""a bare string""#.as_slice(),
            b"\xff\xfe invalid utf8".as_slice(),
        ] {
            assert_eq!(
                decode_frame(bad).unwrap_err(),
                CompanionErrorCode::MalformedFrame
            );
        }
    }

    #[test]
    fn the_mac_matches_the_published_hmac_sha256_test_vectors() {
        // RFC 4231 cases 1, 2, 3 and 6. Cross-checking against published vectors
        // is the only way to know the extension's WebCrypto and this code agree
        // before a browser has ever been pointed at Movie Party.
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let cases: Vec<(Vec<u8>, Vec<u8>, &str)> = vec![
            (
                vec![0x0bu8; 20],
                b"Hi There".to_vec(),
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                b"Jefe".to_vec(),
                b"what do ya want for nothing?".to_vec(),
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                vec![0xaau8; 20],
                vec![0xddu8; 50],
                "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            ),
            (
                // Case 6: a key longer than the hash block size is hashed first.
                vec![0xaau8; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First".to_vec(),
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            ),
        ];
        for (key, message, expected) in &cases {
            assert_eq!(
                &hex(&hmac_sha256(key, message)),
                expected,
                "HMAC mismatch for a {}-byte key",
                key.len()
            );
        }
    }

    #[test]
    fn only_a_real_extension_id_becomes_a_trusted_origin() {
        let id = "a".repeat(32);
        assert_eq!(
            allowed_origin(&id).unwrap(),
            format!("chrome-extension://{id}/")
        );
        for bad in [
            "a".repeat(31),
            "a".repeat(33),
            "a".repeat(31) + "q",
            "a".repeat(31) + "A",
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/".to_string(),
            "a".repeat(31) + "*",
        ] {
            assert!(allowed_origin(&bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn content_reports_are_limited_to_supported_provider_hosts() {
        for allowed in [
            "https://www.netflix.com/title/80181170?ep=70218421",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.primevideo.com/detail/B08C7JV476",
            "https://www.hotstar.com/in/movies/a-film/1234567890/watch",
            // Bare and mobile hosts already accepted by the managed-Chrome path.
            "https://youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ",
        ] {
            assert!(origin_is_allowed(allowed), "refused {allowed}");
        }
        for refused in [
            "https://mail.google.com/inbox",
            "https://bank.example/accounts",
            "http://www.netflix.com/title/1",
            "https://www.netflix.com.evil.test/title/1",
            "https://user:[email protected]/title/1",
            "https://www.netflix.com:8443/title/1",
            "file:///home/user/notes",
            "not a url",
        ] {
            assert!(!origin_is_allowed(refused), "accepted {refused}");
        }
        // The companion must not accept a host the rest of the app rejects, so
        // the two lists cannot drift apart silently.
        for provider in COMPANION_PROVIDERS {
            assert!(
                origin_is_allowed(crate::providers::sync::provider_home_url(
                    crate::providers::sync::provider_id_from_str(provider).unwrap()
                )),
                "provider {provider} home is not companion-allowed"
            );
        }
    }

    #[test]
    fn pairing_verifies_the_holder_and_rejects_every_near_miss() {
        let pairing = generate_pairing();
        assert_eq!(pairing.code.len(), 8, "{:?}", pairing.code);
        assert!(pairing
            .code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
        let challenge = "movie-party-companion-v1:nonce-123";
        let mac = sign(&pairing.secret, challenge);
        assert!(verify(&pairing.secret, challenge, &mac));
        assert!(!verify(&pairing.secret, "other nonce", &mac));
        assert!(!verify(&generate_pairing().secret, challenge, &mac));
        for tampered in [
            mac[..mac.len() - 1].to_string() + "A",
            "A".to_string() + &mac[1..],
            String::new(),
            mac.repeat(2),
        ] {
            assert!(
                !verify(&pairing.secret, challenge, &tampered),
                "accepted {tampered}"
            );
        }
        assert_ne!(generate_pairing().secret, pairing.secret);
    }

    #[test]
    fn version_negotiation_fails_closed() {
        assert_eq!(negotiate(PROTOCOL_VERSION).unwrap(), PROTOCOL_VERSION);
        for wrong in [PROTOCOL_VERSION + 1, 0, 255] {
            assert_eq!(
                negotiate(wrong).unwrap_err(),
                CompanionErrorCode::VersionMismatch
            );
        }
    }

    #[test]
    fn every_capability_and_error_carries_user_facing_words() {
        let capabilities = [
            Capability::IdentifyTab,
            Capability::ReportContent,
            Capability::ReportPlayback,
            Capability::RequestNavigation,
            Capability::ControlPlayback,
        ];
        let mut names: Vec<&str> = capabilities.iter().map(|c| c.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), capabilities.len());
        assert!(capabilities
            .iter()
            .all(|c| !c.reason_when_disabled().is_empty()));

        let codes = [
            CompanionErrorCode::ExtensionMissing,
            CompanionErrorCode::PermissionDenied,
            CompanionErrorCode::BrowserClosed,
            CompanionErrorCode::TabChanged,
            CompanionErrorCode::VersionMismatch,
            CompanionErrorCode::NotPaired,
            CompanionErrorCode::PairingFailed,
            CompanionErrorCode::OriginNotAllowed,
            CompanionErrorCode::FrameTooLarge,
            CompanionErrorCode::MalformedFrame,
            CompanionErrorCode::CapabilityDisabled,
            CompanionErrorCode::HostBusy,
        ];
        let guidance: Vec<&str> = codes.iter().map(|code| code.guidance()).collect();
        let mut distinct = guidance.clone();
        distinct.sort_unstable();
        distinct.dedup();
        // Each failure has to say something different, or the user cannot act
        // on it; a generic "companion error" would be worse than silence.
        assert_eq!(distinct.len(), guidance.len());
        assert!(guidance
            .iter()
            .all(|text| text.split_whitespace().count() >= 4 && text.len() < 400));
    }
}
