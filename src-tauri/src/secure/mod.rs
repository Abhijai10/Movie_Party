//! OS-protected secret storage for device private-key material.
//!
//! The device identity's 32-byte Ed25519 signing seed is NEVER written to
//! ordinary SQLite. It lives in platform-protected secret storage:
//!
//! - macOS: Keychain (via the `security` CLI)
//! - Windows: Credential Manager (via PowerShell WinRT vault)
//!
//! SQLite only stores the device id, public key, and metadata — plus, if
//! needed, a stable key-reference label identifying the secret entry.
//! Tests use [`FakeKeyStore`]; no private material is ever logged.

use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};

pub const DEFAULT_KEY_LABEL: &str = "movie-party-device-signing-key";

pub trait SecureKeyStore: Send + Sync + std::fmt::Debug {
    /// Persist a 32-byte secret under `label`.
    fn store_seed(&self, label: &str, seed: &[u8; 32]) -> Result<(), String>;
    /// Load the 32-byte secret stored under `label`, if any.
    fn load_seed(&self, label: &str) -> Result<Option<[u8; 32]>, String>;
    /// Remove the secret under `label`.
    fn delete_seed(&self, label: &str) -> Result<(), String>;
}

/// Production key store backed by the platform's protected secret storage.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeKeyStore;

impl SecureKeyStore for NativeKeyStore {
    fn store_seed(&self, label: &str, seed: &[u8; 32]) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            native_keychain_store(label, seed)
        }
        #[cfg(target_os = "windows")]
        {
            native_credential_manager_store(label, seed)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (label, seed);
            Err("MP-SECURE-001 unsupported platform".to_string())
        }
    }

    fn load_seed(&self, label: &str) -> Result<Option<[u8; 32]>, String> {
        #[cfg(target_os = "macos")]
        {
            native_keychain_load(label)
        }
        #[cfg(target_os = "windows")]
        {
            native_credential_manager_load(label)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = label;
            Err("MP-SECURE-001 unsupported platform".to_string())
        }
    }

    fn delete_seed(&self, label: &str) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            native_keychain_delete(label)
        }
        #[cfg(target_os = "windows")]
        {
            native_credential_manager_delete(label)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = label;
            Err("MP-SECURE-001 unsupported platform".to_string())
        }
    }
}

#[cfg(target_os = "macos")]
fn native_keychain_store(label: &str, seed: &[u8; 32]) -> Result<(), String> {
    // `security add-generic-password` writes to the login keychain. The seed
    // is base64 (not interpolated as raw bytes) to keep the CLI argument
    // free of shell metacharacters.
    let encoded = STANDARD_NO_PAD.encode(seed);
    let status = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            "Movie Party",
            "-s",
            label,
            "-w",
            &encoded,
            "-U",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("security add exited with status {s}")),
        Err(e) => Err(format!("failed to run security: {e}")),
    }
}

#[cfg(target_os = "macos")]
fn native_keychain_load(label: &str) -> Result<Option<[u8; 32]>, String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            "Movie Party",
            "-s",
            label,
            "-w",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => return Err(format!("failed to run security: {e}")),
    };
    if !output.status.success() {
        // Exit code 44 = item not found.
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let bytes = STANDARD_NO_PAD
        .decode(value)
        .map_err(|_| "MP-SECURE-002 stored key is corrupt".to_string())?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "MP-SECURE-002 stored key has invalid length".to_string())?;
    Ok(Some(seed))
}

#[cfg(target_os = "macos")]
fn native_keychain_delete(label: &str) -> Result<(), String> {
    let status = std::process::Command::new("security")
        .args(["delete-generic-password", "-a", "Movie Party", "-s", label])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Ok(()), // already absent
        Err(e) => Err(format!("failed to run security: {e}")),
    }
}

#[cfg(target_os = "windows")]
fn native_credential_manager_store(label: &str, seed: &[u8; 32]) -> Result<(), String> {
    // Write through the WinRT PasswordVault in a fixed format. The seed is
    // base64, so the PowerShell string literal is free of quotes/newlines.
    let encoded = STANDARD_NO_PAD.encode(seed);
    let script = format!(
        "Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
         [Windows.Security.Credentials.PasswordVault, Windows.Security.Credentials, ContentType=WindowsRuntime] | Out-Null; \
         $vault = New-Object Windows.Security.Credentials.PasswordVault; \
         $cred = $vault.RetrieveAll() | Where-Object {{ $_.Resource -eq 'Movie Party' -and $_.UserName -eq '{0}' }}; \
         if ($cred) {{ $vault.Remove($cred[0]) }}; \
         $new = New-Object Windows.Security.Credentials.PasswordCredential('Movie Party', '{0}', '{1}'); \
         $vault.Add($new)",
        label.replace('\'', "''"),
        encoded,
    );
    let result = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status();
    match result {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("PasswordVault exited with status {s}")),
        Err(e) => Err(format!("failed to run powershell: {e}")),
    }
}

#[cfg(target_os = "windows")]
fn native_credential_manager_load(label: &str) -> Result<Option<[u8; 32]>, String> {
    let script = format!(
        "Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
         [Windows.Security.Credentials.PasswordVault, Windows.Security.Credentials, ContentType=WindowsRuntime] | Out-Null; \
         $vault = New-Object Windows.Security.Credentials.PasswordVault; \
         try {{ $cred = $vault.RetrieveAll() | Where-Object {{ $_.Resource -eq 'Movie Party' -and $_.UserName -eq '{0}' }} }} catch {{ }}; \
         if ($cred -and $cred.Count -gt 0) {{ $cred[0].Password }} else {{ '' }}",
        label.replace('\'', "''"),
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => return Err(format!("failed to run powershell: {e}")),
    };
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    let bytes = STANDARD_NO_PAD
        .decode(value)
        .map_err(|_| "MP-SECURE-002 stored key is corrupt".to_string())?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "MP-SECURE-002 stored key has invalid length".to_string())?;
    Ok(Some(seed))
}

#[cfg(target_os = "windows")]
fn native_credential_manager_delete(label: &str) -> Result<(), String> {
    let script = format!(
        "Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
         [Windows.Security.Credentials.PasswordVault, Windows.Security.Credentials, ContentType=WindowsRuntime] | Out-Null; \
         $vault = New-Object Windows.Security.Credentials.PasswordVault; \
         $cred = $vault.RetrieveAll() | Where-Object {{ $_.Resource -eq 'Movie Party' -and $_.UserName -eq '{0}' }}; \
         if ($cred) {{ $vault.Remove($cred[0]) }}",
        label.replace('\'', "''"),
    );
    let result = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status();
    match result {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("PasswordVault exited with status {s}")),
        Err(e) => Err(format!("failed to run powershell: {e}")),
    }
}

/// In-memory key store for unit/integration tests. Never touches the OS.
#[derive(Debug, Default)]
pub struct FakeKeyStore {
    entries: Mutex<std::collections::HashMap<String, [u8; 32]>>,
}

impl FakeKeyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecureKeyStore for FakeKeyStore {
    fn store_seed(&self, label: &str, seed: &[u8; 32]) -> Result<(), String> {
        self.entries
            .lock()
            .map_err(|e| format!("MP-SECURE-002 lock poisoned: {e}"))?
            .insert(label.to_string(), *seed);
        Ok(())
    }

    fn load_seed(&self, label: &str) -> Result<Option<[u8; 32]>, String> {
        Ok(self
            .entries
            .lock()
            .map_err(|e| format!("MP-SECURE-002 lock poisoned: {e}"))?
            .get(label)
            .copied())
    }

    fn delete_seed(&self, label: &str) -> Result<(), String> {
        self.entries
            .lock()
            .map_err(|e| format!("MP-SECURE-002 lock poisoned: {e}"))?
            .remove(label);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_store_round_trips_secret() {
        let store = FakeKeyStore::new();
        let seed = [7; 32];
        store.store_seed(DEFAULT_KEY_LABEL, &seed).expect("store");
        let loaded = store
            .load_seed(DEFAULT_KEY_LABEL)
            .expect("load")
            .expect("some");
        assert_eq!(loaded, seed);
        store.delete_seed(DEFAULT_KEY_LABEL).expect("delete");
        assert!(store.load_seed(DEFAULT_KEY_LABEL).expect("load").is_none());
    }

    #[test]
    fn fake_store_missing_label_returns_none() {
        let store = FakeKeyStore::new();
        assert!(store.load_seed("absent").expect("load").is_none());
    }
}
