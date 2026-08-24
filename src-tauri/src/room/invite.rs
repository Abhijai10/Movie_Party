use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

use crate::network::quic::{is_base64url_128bit, is_base64url_256bit, RoomCredentials};

pub const INVITE_SCHEME: &str = "moveparty";
pub const INVITE_HOST_PATH: &str = "join";
pub const INVITE_VERSION_V1: u8 = 1;
pub const INVITE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovePartyInvite {
    pub v: u8,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub room_id: String,
    pub join_secret: String,
    pub host_device_id: String,
    pub host_ip: String,
    pub host_port: u16,
    pub server_certificate_fingerprint: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InviteError {
    #[error("MP-ROOM-001 invite must start with moveparty://join/")]
    InvalidScheme,
    #[error("MP-ROOM-001 invite path/room id is missing")]
    InvalidRoomId,
    #[error("MP-ROOM-001 invite descriptor fragment is missing")]
    MissingFragment,
    #[error("MP-ROOM-001 invite descriptor is not valid base64url: {0}")]
    InvalidEncoding(String),
    #[error("MP-ROOM-001 invite descriptor JSON is invalid: {0}")]
    InvalidJson(String),
    #[error("MP-ROOM-001 invite version {0} is not supported")]
    UnsupportedVersion(u8),
    #[error("MP-ROOM-001 invite protocol {0}.{1} is not supported")]
    UnsupportedProtocol(u16, u16),
    #[error("MP-ROOM-001 invite has expired")]
    Expired,
    #[error("MP-ROOM-001 invite room id must be 128-bit base64url")]
    InvalidRoomIdShape,
    #[error("MP-ROOM-001 invite join secret must be 256-bit base64url")]
    InvalidJoinSecret,
    #[error("MP-ROOM-001 invite fingerprint must be 256-bit base64url")]
    InvalidFingerprintShape,
    #[error("MP-ROOM-001 invite host address is invalid: {0}")]
    InvalidHostAddress(String),
    #[error("MP-ROOM-001 invite room id in fragment does not match path")]
    RoomIdMismatch,
}

pub fn encode_invite(invite: &MovePartyInvite) -> Result<String, InviteError> {
    validate_invite_shape(invite)?;
    let json =
        serde_json::to_string(invite).map_err(|err| InviteError::InvalidJson(err.to_string()))?;
    let descriptor = URL_SAFE_NO_PAD.encode(json.as_bytes());
    Ok(format!(
        "{}://{}/{}#{}",
        INVITE_SCHEME, INVITE_HOST_PATH, invite.room_id, descriptor
    ))
}

pub fn parse_invite(url: &str) -> Result<MovePartyInvite, InviteError> {
    let prefix = format!("{}://{}/", INVITE_SCHEME, INVITE_HOST_PATH);
    if !url.starts_with(&prefix) {
        return Err(InviteError::InvalidScheme);
    }

    let rest = &url[prefix.len()..];
    let hash_pos = rest.find('#').ok_or(InviteError::MissingFragment)?;
    let room_id = rest[..hash_pos].to_string();

    if room_id.trim().is_empty() {
        return Err(InviteError::InvalidRoomId);
    }

    let descriptor_b64 = &rest[hash_pos + 1..];
    let json = URL_SAFE_NO_PAD
        .decode(descriptor_b64)
        .map_err(|err| InviteError::InvalidEncoding(err.to_string()))?;

    let invite: MovePartyInvite =
        serde_json::from_slice(&json).map_err(|err| InviteError::InvalidJson(err.to_string()))?;

    if invite.v != INVITE_VERSION_V1 {
        return Err(InviteError::UnsupportedVersion(invite.v));
    }
    if invite.protocol_major != crate::PROTOCOL_MAJOR {
        return Err(InviteError::UnsupportedProtocol(
            invite.protocol_major,
            invite.protocol_minor,
        ));
    }
    if invite.protocol_minor > crate::PROTOCOL_MINOR {
        return Err(InviteError::UnsupportedProtocol(
            invite.protocol_major,
            invite.protocol_minor,
        ));
    }
    if invite.room_id != room_id {
        return Err(InviteError::RoomIdMismatch);
    }
    if invite.expires_at_ms <= 0 {
        return Err(InviteError::Expired); // Invalid expiry counts as expired/rejected
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    if now_ms >= invite.expires_at_ms {
        return Err(InviteError::Expired);
    }

    validate_invite_shape(&invite)?;
    Ok(invite)
}

pub fn invite_to_credentials(invite: &MovePartyInvite) -> RoomCredentials {
    RoomCredentials {
        room_id: invite.room_id.clone(),
        join_secret: invite.join_secret.clone(),
    }
}

pub fn invite_socket_addr(invite: &MovePartyInvite) -> Result<SocketAddr, InviteError> {
    let addr = format!("{}:{}", invite.host_ip, invite.host_port);
    addr.parse::<SocketAddr>()
        .map_err(|_| InviteError::InvalidHostAddress(addr))
}

fn validate_invite_shape(invite: &MovePartyInvite) -> Result<(), InviteError> {
    if !is_base64url_128bit(&invite.room_id) {
        return Err(InviteError::InvalidRoomIdShape);
    }
    if !is_base64url_256bit(&invite.join_secret) {
        return Err(InviteError::InvalidJoinSecret);
    }
    if !is_base64url_256bit(&invite.server_certificate_fingerprint) {
        return Err(InviteError::InvalidFingerprintShape);
    }
    invite
        .host_ip
        .parse::<IpAddr>()
        .map_err(|_| InviteError::InvalidHostAddress(invite.host_ip.clone()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_encode_and_parse_invite() {
        let invite = MovePartyInvite {
            v: INVITE_VERSION_V1,
            protocol_major: crate::PROTOCOL_MAJOR,
            protocol_minor: crate::PROTOCOL_MINOR,
            room_id: "EjRWeJCrze8BI0VniavN7w".to_string(),
            join_secret: "mSncRHUcatB8mqTKA0jVJPmc0JaWJsm4u17SWO9M-q0".to_string(),
            host_device_id: "0198c3d0-7c55-7f82-9af2-36c9946b2974".to_string(),
            host_ip: "127.0.0.1".to_string(),
            host_port: 47_821,
            server_certificate_fingerprint: "3VJR83iDjJSTGaBR8Ywkwm5e1eNRTLnDfdrIsEc2Plw"
                .to_string(),
            expires_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                + 10000,
        };

        let encoded = encode_invite(&invite).expect("encode");
        assert!(encoded.starts_with("moveparty://join/"));

        let round_tripped = parse_invite(&encoded).expect("parse");
        assert_eq!(round_tripped.room_id, invite.room_id);
        assert_eq!(round_tripped.join_secret, invite.join_secret);
        assert_eq!(round_tripped.host_port, invite.host_port);
    }

    #[test]
    fn rejects_invalid_scheme() {
        let err = parse_invite("https://evil.example/").unwrap_err();
        assert!(matches!(err, InviteError::InvalidScheme));
    }

    #[test]
    fn rejects_malformed_room_id() {
        let invite = MovePartyInvite {
            v: INVITE_VERSION_V1,
            protocol_major: 1,
            protocol_minor: 0,
            room_id: "short".to_string(),
            join_secret: "mSncRHUcatB8mqTKA0jVJPmc0JaWJsm4u17SWO9M-q0".to_string(),
            host_device_id: "device".to_string(),
            host_ip: "127.0.0.1".to_string(),
            host_port: 1,
            server_certificate_fingerprint: "3VJR83iDjJSTGaBR8Ywkwm5e1eNRTLnDfdrIsEc2Plw"
                .to_string(),
            expires_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                + 10000,
        };
        let err = encode_invite(&invite).unwrap_err();
        assert!(matches!(err, InviteError::InvalidRoomIdShape));
    }

    #[test]
    fn invite_to_credentials_preserves_secret() {
        let invite = MovePartyInvite {
            v: INVITE_VERSION_V1,
            protocol_major: 1,
            protocol_minor: 0,
            room_id: "EjRWeJCrze8BI0VniavN7w".to_string(),
            join_secret: "secret".to_string(),
            host_device_id: "d".to_string(),
            host_ip: "127.0.0.1".to_string(),
            host_port: 1,
            server_certificate_fingerprint: "c".repeat(43),
            expires_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                + 10000,
        };
        let creds = invite_to_credentials(&invite);
        assert_eq!(creds.room_id, invite.room_id);
        assert_eq!(creds.join_secret, "secret");
    }

    #[test]
    fn test_invite_expiry() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let mut invite = MovePartyInvite {
            v: INVITE_VERSION_V1,
            protocol_major: crate::PROTOCOL_MAJOR,
            protocol_minor: crate::PROTOCOL_MINOR,
            room_id: "EjRWeJCrze8BI0VniavN7w".to_string(),
            join_secret: "mSncRHUcatB8mqTKA0jVJPmc0JaWJsm4u17SWO9M-q0".to_string(),
            host_device_id: "0198c3d0-7c55-7f82-9af2-36c9946b2974".to_string(),
            host_ip: "127.0.0.1".to_string(),
            host_port: 47_821,
            server_certificate_fingerprint: "3VJR83iDjJSTGaBR8Ywkwm5e1eNRTLnDfdrIsEc2Plw"
                .to_string(),
            expires_at_ms: now_ms + 10000, // future
        };

        let encoded_future = encode_invite(&invite).unwrap();
        assert!(parse_invite(&encoded_future).is_ok());

        invite.expires_at_ms = now_ms - 10000; // past
        let encoded_past = encode_invite(&invite).unwrap();
        assert!(matches!(
            parse_invite(&encoded_past).unwrap_err(),
            InviteError::Expired
        ));

        invite.expires_at_ms = 0; // zero
        let encoded_zero = encode_invite(&invite).unwrap();
        assert!(matches!(
            parse_invite(&encoded_zero).unwrap_err(),
            InviteError::Expired
        ));

        invite.expires_at_ms = -5000; // negative
        let encoded_neg = encode_invite(&invite).unwrap();
        assert!(matches!(
            parse_invite(&encoded_neg).unwrap_err(),
            InviteError::Expired
        ));
    }

    #[test]
    fn test_protocol_compatibility() {
        let mut invite = MovePartyInvite {
            v: INVITE_VERSION_V1,
            protocol_major: crate::PROTOCOL_MAJOR,
            protocol_minor: crate::PROTOCOL_MINOR,
            room_id: "EjRWeJCrze8BI0VniavN7w".to_string(),
            join_secret: "mSncRHUcatB8mqTKA0jVJPmc0JaWJsm4u17SWO9M-q0".to_string(),
            host_device_id: "device".to_string(),
            host_ip: "127.0.0.1".to_string(),
            host_port: 47_821,
            server_certificate_fingerprint: "3VJR83iDjJSTGaBR8Ywkwm5e1eNRTLnDfdrIsEc2Plw"
                .to_string(),
            expires_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                + 10000,
        };

        // correct
        let encoded = encode_invite(&invite).unwrap();
        assert!(parse_invite(&encoded).is_ok());

        // wrong major
        invite.protocol_major = crate::PROTOCOL_MAJOR + 1;
        let encoded_wrong_major = encode_invite(&invite).unwrap();
        let err = parse_invite(&encoded_wrong_major).unwrap_err();
        assert!(matches!(err, InviteError::UnsupportedProtocol(_, _)));

        // future minor
        invite.protocol_major = crate::PROTOCOL_MAJOR;
        invite.protocol_minor = crate::PROTOCOL_MINOR + 1;
        let encoded_future_minor = encode_invite(&invite).unwrap();
        let err = parse_invite(&encoded_future_minor).unwrap_err();
        assert!(matches!(err, InviteError::UnsupportedProtocol(_, _)));
    }
}
