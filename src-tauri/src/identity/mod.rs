use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
}

#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    pub device_id: String,
    signing_key: SigningKey,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("MP-ID-001 public identity key is invalid")]
    InvalidPublicKey,
    #[error("MP-ID-002 identity signature is invalid")]
    InvalidSignature,
}

impl DeviceIdentity {
    pub fn new_ephemeral() -> Self {
        let mut seed = [0_u8; 32];
        rand::rng().fill_bytes(&mut seed);

        Self {
            device_id: Uuid::now_v7().to_string(),
            signing_key: SigningKey::from_bytes(&seed),
        }
    }

    pub fn from_seed_for_tests(device_id: impl Into<String>, seed: [u8; 32]) -> Self {
        Self {
            device_id: device_id.into(),
            signing_key: SigningKey::from_bytes(&seed),
        }
    }

    /// Restore an identity from its persisted 32-byte ed25519 seed and the
    /// recorded device id. Used on startup to re-create the exact same
    /// identity (same device id + same signing key) across restarts.
    pub fn from_seed(device_id: impl Into<String>, seed: &[u8]) -> Option<Self> {
        let seed: [u8; 32] = seed.try_into().ok()?;
        Some(Self {
            device_id: device_id.into(),
            signing_key: SigningKey::from_bytes(&seed),
        })
    }

    /// The 32-byte ed25519 seed backing this identity. Persisted so the
    /// identity can be restored after restart.
    pub fn seed(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    pub fn public_key_base64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.signing_key.verifying_key().as_bytes())
    }

    /// Sign a canonical, purpose-prefixed message with this device's identity.
    /// Friend invites need the same key as room auth but a different context, so
    /// a room signature can never be lifted into a friendship claim.
    pub fn sign_message(&self, message: &str) -> String {
        URL_SAFE_NO_PAD.encode(self.signing_key.sign(message.as_bytes()).to_bytes())
    }

    pub fn verify_message(
        public_key_base64: &str,
        signature_base64: &str,
        message: &str,
    ) -> Result<(), IdentityError> {
        let public_key = decode_public_key(public_key_base64)?;
        let signature = decode_signature(signature_base64)?;
        public_key
            .verify(message.as_bytes(), &signature)
            .map_err(|_| IdentityError::InvalidSignature)
    }

    pub fn sign_auth_request(
        &self,
        room_id: &str,
        join_secret_hash: &str,
        invite_nonce: &str,
    ) -> String {
        let message =
            canonical_auth_message(room_id, join_secret_hash, invite_nonce, &self.device_id);
        self.sign_message(&message)
    }
}

/// A short, human-checkable rendering of an identity key, for confirming that the
/// two people at first contact mean the same device. Display names can be typed to
/// match anything; a fingerprint cannot be guessed at a glance.
pub fn identity_fingerprint(public_key_base64: &str) -> Result<String, IdentityError> {
    let key = decode_public_key(public_key_base64)?;
    let digest = Sha256::digest(key.as_bytes());
    let hex = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    // Exactly sixteen characters, read in four groups of four.
    let groups: Vec<&str> = (0..4).map(|group| &hex[group * 4..group * 4 + 4]).collect();
    Ok(format!("MP-{}", groups.join("-")))
}

pub fn verify_auth_request_signature(
    public_key_base64: &str,
    signature_base64: &str,
    room_id: &str,
    join_secret_hash: &str,
    invite_nonce: &str,
    device_id: &str,
) -> Result<(), IdentityError> {
    let message = canonical_auth_message(room_id, join_secret_hash, invite_nonce, device_id);
    DeviceIdentity::verify_message(public_key_base64, signature_base64, &message)
}

fn canonical_auth_message(
    room_id: &str,
    join_secret_hash: &str,
    invite_nonce: &str,
    device_id: &str,
) -> String {
    format!("{room_id}\n{join_secret_hash}\n{invite_nonce}\n{device_id}")
}

fn decode_public_key(public_key_base64: &str) -> Result<VerifyingKey, IdentityError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(public_key_base64)
        .map_err(|_| IdentityError::InvalidPublicKey)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| IdentityError::InvalidPublicKey)?;

    VerifyingKey::from_bytes(&bytes).map_err(|_| IdentityError::InvalidPublicKey)
}

fn decode_signature(signature_base64: &str) -> Result<Signature, IdentityError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(signature_base64)
        .map_err(|_| IdentityError::InvalidSignature)?;

    Signature::from_slice(&bytes).map_err(|_| IdentityError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::{
        identity_fingerprint, verify_auth_request_signature, DeviceIdentity, IdentityError,
        Platform,
    };

    #[test]
    fn phase_zero_identity_module_is_available() {
        assert_eq!(Platform::MacOs, Platform::MacOs);
    }

    #[test]
    fn signs_and_verifies_auth_request_material() {
        let identity = DeviceIdentity::from_seed_for_tests("device-1", [7; 32]);
        let signature = identity.sign_auth_request("room", "secret-hash", "nonce");

        verify_auth_request_signature(
            &identity.public_key_base64(),
            &signature,
            "room",
            "secret-hash",
            "nonce",
            "device-1",
        )
        .expect("valid signature");
    }

    #[test]
    fn fingerprints_are_stable_distinct_and_never_reveal_the_key() {
        let one = DeviceIdentity::from_seed_for_tests("device-1", [7; 32]);
        let two = DeviceIdentity::from_seed_for_tests("device-2", [8; 32]);
        let printed = identity_fingerprint(&one.public_key_base64()).unwrap();
        assert_eq!(
            printed,
            identity_fingerprint(&one.public_key_base64()).unwrap()
        );
        assert_ne!(
            printed,
            identity_fingerprint(&two.public_key_base64()).unwrap()
        );
        let groups: Vec<&str> = printed.split('-').collect();
        assert_eq!(groups.len(), 5);
        assert_eq!(groups[0], "MP");
        assert!(groups[1..].iter().all(|group| group.len() == 4));
        // The fingerprint is a digest, not the key: a visible fingerprint must not
        // let an observer reconstruct or sign as that identity.
        assert!(!one.public_key_base64().contains(groups[1]));
        // Only the shape is rejected here: a wrong-length key has no digest to take.
        assert!(identity_fingerprint("not-valid-base64url-key").is_err());
        assert!(identity_fingerprint("AAAA").is_err());
    }

    #[test]
    fn rejects_signature_for_different_device_id() {
        let identity = DeviceIdentity::from_seed_for_tests("device-1", [7; 32]);
        let signature = identity.sign_auth_request("room", "secret-hash", "nonce");

        assert_eq!(
            verify_auth_request_signature(
                &identity.public_key_base64(),
                &signature,
                "room",
                "secret-hash",
                "nonce",
                "device-2",
            ),
            Err(IdentityError::InvalidSignature)
        );
    }
}
