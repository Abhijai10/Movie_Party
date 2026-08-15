use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
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

    pub fn public_key_base64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.signing_key.verifying_key().as_bytes())
    }

    pub fn sign_auth_request(
        &self,
        room_id: &str,
        join_secret_hash: &str,
        invite_nonce: &str,
    ) -> String {
        let message =
            canonical_auth_message(room_id, join_secret_hash, invite_nonce, &self.device_id);
        URL_SAFE_NO_PAD.encode(self.signing_key.sign(message.as_bytes()).to_bytes())
    }
}

pub fn verify_auth_request_signature(
    public_key_base64: &str,
    signature_base64: &str,
    room_id: &str,
    join_secret_hash: &str,
    invite_nonce: &str,
    device_id: &str,
) -> Result<(), IdentityError> {
    let public_key = decode_public_key(public_key_base64)?;
    let signature = decode_signature(signature_base64)?;
    let message = canonical_auth_message(room_id, join_secret_hash, invite_nonce, device_id);

    public_key
        .verify(message.as_bytes(), &signature)
        .map_err(|_| IdentityError::InvalidSignature)
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
    use super::{verify_auth_request_signature, DeviceIdentity, IdentityError, Platform};

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
