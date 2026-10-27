//! Movie Party native messaging host.
//!
//! Installed by the user, never silently: the browser starts this process when
//! the paired extension sends a message, and the process lives only as long as
//! that port is open. Its whole job is to move length-prefixed JSON between
//! stdin/stdout and the loopback listener Movie Party publishes while the
//! companion is switched on.
//!
//! Deliberate limits:
//! - It holds no secret of its own and cannot authorise anything: every frame
//!   it relays is verified by Movie Party against the paired session key. It
//!   does *see* the handshake reply, which necessarily carries that key to the
//!   extension — so the honest claim is "a copied binary cannot pair without
//!   the on-screen code", not "it can never learn the key".
//! - It refuses unknown message kinds and oversized frames before contacting the
//!   app, so a compromised extension page cannot use it as a generic proxy.
//! - It writes nothing to stdout except protocol frames. Diagnostics go to
//!   stderr; a stray byte there would corrupt the channel.
//! - It never launches a browser, reads a user file, or attaches a debugger.
//!   There is no code path here that could do any of those.

use movie_party_lib::companion::host::{self, CompanionRequest, CompanionResponse};
use movie_party_lib::companion::{
    decode_frame, CompanionErrorCode, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use serde_json::Value;
use std::io::{self, Read, Write};
use std::time::Duration;

fn main() {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout();
    while let Some(payload) = read_frame(&mut stdin) {
        let text = handle(&payload);
        let mut out = Vec::with_capacity(4 + text.len());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(&text);
        if stdout.write_all(&out).is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

/// One inbound frame, or `None` when the browser closed the port.
fn read_frame<R: Read>(reader: &mut R) -> Option<Vec<u8>> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header).ok()?;
    let len = u32::from_le_bytes(header) as usize;
    // Chrome permits 1 MB; Movie Party speaks far less, and a larger frame is
    // either a bug or an attempt to make the bridge buffer somebody's page.
    if len == 0 || len > MAX_FRAME_BYTES {
        return Some(Vec::new());
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}

fn handle(payload: &[u8]) -> Vec<u8> {
    if payload.is_empty() {
        return refuse("", CompanionErrorCode::FrameTooLarge);
    }
    let Ok(value) = decode_frame(payload) else {
        return refuse("", CompanionErrorCode::MalformedFrame);
    };
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Unknown kinds never reach the app, so the bridge is not a general proxy.
    // The handshake is the exception: it carries no capability because
    // establishing one is its whole purpose.
    if kind != "handshake" && host::capability_for(&kind).is_none() {
        return refuse(&kind, CompanionErrorCode::MalformedFrame);
    }
    if serde_json::from_value::<CompanionRequest>(value).is_err() {
        return refuse(&kind, CompanionErrorCode::MalformedFrame);
    }
    let Some(path) = host::endpoint_path() else {
        return refuse(&kind, CompanionErrorCode::ExtensionMissing);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return refuse(&kind, CompanionErrorCode::ExtensionMissing);
    };
    let Ok(endpoint) = host::parse_endpoint(&text) else {
        return refuse(&kind, CompanionErrorCode::VersionMismatch);
    };
    relay(payload, &kind, endpoint.port)
}

/// Forward the frame the browser sent and return the app's frame unchanged.
/// Re-serialising here would couple the bridge to a schema it has no business
/// owning and would silently drop any field a newer app adds.
fn relay(payload: &[u8], kind: &str, port: u16) -> Vec<u8> {
    // Loopback, and the port only ever comes from Movie Party's own endpoint
    // file, so the bridge cannot be pointed at another machine or another app.
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2))
    else {
        return refuse(kind, CompanionErrorCode::ExtensionMissing);
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if stream.write_all(&out).is_err() {
        return refuse(kind, CompanionErrorCode::BrowserClosed);
    }
    match read_frame(&mut stream) {
        // An absent, empty or malformed reply is replaced, never forwarded raw.
        Some(body) if is_json_object(&body) => body,
        Some(_) | None => refuse(kind, CompanionErrorCode::MalformedFrame),
    }
}

fn is_json_object(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body).is_ok_and(|value| value.is_object())
}

fn refuse(kind: &str, error: CompanionErrorCode) -> Vec<u8> {
    serde_json::to_vec(&CompanionResponse::refused(kind, error, None))
        .unwrap_or_else(|_| br#"{"kind":"","ok":false}"#.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oversized_or_empty_frame_never_reaches_the_app() {
        assert_eq!(
            String::from_utf8(handle(&[])).unwrap(),
            String::from_utf8(refuse("", CompanionErrorCode::FrameTooLarge)).unwrap()
        );
        let mut reader = &[0u8; 0][..];
        assert_eq!(read_frame(&mut reader), None);
        // A declared length beyond the protocol cap is refused without reading
        // the body, so the bridge cannot be made to buffer an arbitrary blob.
        let mut header = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes().to_vec();
        header.extend_from_slice(b"x");
        let mut reader = &header[..];
        assert_eq!(read_frame(&mut reader), Some(Vec::new()));
    }

    #[test]
    fn a_handshake_gets_past_the_kind_gate_because_it_carries_no_capability() {
        std::env::set_var(
            "MOVIE_PARTY_COMPANION_ENDPOINT",
            "/tmp/movie-party-absent-endpoint.json",
        );
        let frame = serde_json::json!({
            "kind": "handshake", "sequence": 1, "version": PROTOCOL_VERSION, "code": "ABCD2345",
        });
        let text = String::from_utf8(handle(&serde_json::to_vec(&frame).unwrap())).unwrap();
        assert!(
            text.contains("ExtensionMissing"),
            "the bridge refused its own pairing message: {text}"
        );
        std::env::remove_var("MOVIE_PARTY_COMPANION_ENDPOINT");
    }

    #[test]
    fn only_known_message_kinds_are_forwarded() {
        for kind in ["readCookies", "readHistory", "getAllUrls", "", "execScript"] {
            let frame = serde_json::json!({ "kind": kind, "token": "t", "sequence": 1 });
            let text = String::from_utf8(handle(&serde_json::to_vec(&frame).unwrap())).unwrap();
            assert!(
                text.contains("MalformedFrame"),
                "{kind} was forwarded: {text}"
            );
        }
    }

    #[test]
    fn a_request_carrying_extra_fields_is_rejected_at_the_bridge() {
        let frame = serde_json::json!({
            "kind": "identifyTab",
            "token": "t",
            "sequence": 1,
            "cookies": [{ "name": "session", "value": "secret" }],
        });
        let text = String::from_utf8(handle(&serde_json::to_vec(&frame).unwrap())).unwrap();
        assert!(text.contains("MalformedFrame"), "accepted: {text}");
        assert!(!text.contains("secret"), "echoed the smuggled value");
    }

    #[test]
    fn with_no_app_listening_the_bridge_reports_instead_of_guessing() {
        let frame = serde_json::json!({
            "kind": "identifyTab",
            "token": "t",
            "sequence": 1,
            "url": "https://www.netflix.com/title/80181170",
        });
        // No endpoint file at this path: the app is not running the companion.
        std::env::set_var(
            "MOVIE_PARTY_COMPANION_ENDPOINT",
            "/tmp/movie-party-absent-endpoint.json",
        );
        let text = String::from_utf8(handle(&serde_json::to_vec(&frame).unwrap())).unwrap();
        assert!(text.contains("ExtensionMissing"), "said: {text}");
        std::env::remove_var("MOVIE_PARTY_COMPANION_ENDPOINT");
    }

    #[test]
    fn a_refusal_never_carries_provider_data() {
        let text = String::from_utf8(refuse(
            "reportContent",
            CompanionErrorCode::OriginNotAllowed,
        ))
        .unwrap();
        assert!(text.contains("OriginNotAllowed"));
        assert!(text.contains("supported provider"));
        assert!(!text.contains("netflix.com"), "leaked a URL: {text}");
    }
}
