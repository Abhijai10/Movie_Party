use std::{
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};

use serde_json::json;
use thiserror::Error;

pub const CDP_BIND_HOST: &str = "127.0.0.1";
pub const DEFAULT_CDP_PORT: u16 = 9222;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManagedChromeError {
    #[error("MP-PROVIDER-001 Chrome executable unavailable")]
    ChromeUnavailable,
    #[error("MP-PROVIDER-002 invalid provider profile name")]
    InvalidProviderProfile,
    #[error("MP-PROVIDER-003 CDP must bind to localhost only")]
    NonLocalCdpBind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeLaunchPlan {
    pub executable: PathBuf,
    pub profile_path: PathBuf,
    pub cdp_host: IpAddr,
    pub cdp_port: u16,
    pub url: String,
}

impl ChromeLaunchPlan {
    pub fn args(&self) -> Vec<String> {
        vec![
            format!("--user-data-dir={}", self.profile_path.display()),
            format!("--remote-debugging-address={}", self.cdp_host),
            format!("--remote-debugging-port={}", self.cdp_port),
            "--no-first-run".to_owned(),
            "--new-window".to_owned(),
            self.url.clone(),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CdpCommand {
    pub id: u64,
    pub method: &'static str,
    pub params: serde_json::Value,
}

pub fn default_chrome_candidates() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from(
                "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            ),
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
        ]
    } else {
        vec![PathBuf::from("google-chrome"), PathBuf::from("chromium")]
    }
}

pub fn find_chrome(candidates: &[PathBuf]) -> Result<PathBuf, ManagedChromeError> {
    candidates
        .iter()
        .find(|candidate| candidate.exists())
        .cloned()
        .ok_or(ManagedChromeError::ChromeUnavailable)
}

pub fn provider_profile_path(
    root: &Path,
    provider_id: &str,
) -> Result<PathBuf, ManagedChromeError> {
    if !is_valid_provider_id(provider_id) {
        return Err(ManagedChromeError::InvalidProviderProfile);
    }

    Ok(root.join(provider_id))
}

pub fn build_launch_plan(
    executable: PathBuf,
    profiles_root: &Path,
    provider_id: &str,
    cdp_port: u16,
    url: &str,
) -> Result<ChromeLaunchPlan, ManagedChromeError> {
    let profile_path = provider_profile_path(profiles_root, provider_id)?;

    Ok(ChromeLaunchPlan {
        executable,
        profile_path,
        cdp_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        cdp_port,
        url: url.to_owned(),
    })
}

pub fn validate_cdp_bind(host: IpAddr) -> Result<(), ManagedChromeError> {
    if host.is_loopback() {
        Ok(())
    } else {
        Err(ManagedChromeError::NonLocalCdpBind)
    }
}

pub fn navigate_command(id: u64, url: &str) -> CdpCommand {
    CdpCommand {
        id,
        method: "Page.navigate",
        params: json!({ "url": url }),
    }
}

pub fn close_browser_command(id: u64) -> CdpCommand {
    CdpCommand {
        id,
        method: "Browser.close",
        params: json!({}),
    }
}

fn is_valid_provider_id(provider_id: &str) -> bool {
    !provider_id.is_empty()
        && provider_id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && !provider_id.contains("..")
        && !provider_id.contains('/')
        && !provider_id.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_dedicated_provider_profile_path() {
        let root = Path::new("/tmp/MovePartyProfiles");

        assert_eq!(
            provider_profile_path(root, "netflix"),
            Ok(PathBuf::from("/tmp/MovePartyProfiles/netflix"))
        );
    }

    #[test]
    fn rejects_profile_path_traversal() {
        let root = Path::new("/tmp/MovePartyProfiles");

        assert_eq!(
            provider_profile_path(root, "../netflix"),
            Err(ManagedChromeError::InvalidProviderProfile)
        );
        assert_eq!(
            provider_profile_path(root, "netflix/secrets"),
            Err(ManagedChromeError::InvalidProviderProfile)
        );
    }

    #[test]
    fn launch_args_use_non_default_profile_and_local_cdp() {
        let plan = build_launch_plan(
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            Path::new("/tmp/MovePartyProfiles"),
            "prime",
            DEFAULT_CDP_PORT,
            "https://www.primevideo.com/",
        )
        .expect("launch plan");
        let args = plan.args();

        assert!(args
            .iter()
            .any(|arg| arg == "--remote-debugging-address=127.0.0.1"));
        assert!(args.iter().any(|arg| arg == "--remote-debugging-port=9222"));
        assert!(args
            .iter()
            .any(|arg| arg == "--user-data-dir=/tmp/MovePartyProfiles/prime"));
        assert!(!args
            .iter()
            .any(|arg| arg.to_ascii_lowercase().contains("cookie")));
    }

    #[test]
    fn cdp_bind_must_be_loopback() {
        assert_eq!(validate_cdp_bind(IpAddr::V4(Ipv4Addr::LOCALHOST)), Ok(()));
        assert_eq!(
            validate_cdp_bind(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))),
            Err(ManagedChromeError::NonLocalCdpBind)
        );
    }

    #[test]
    fn cdp_navigation_and_close_commands_are_cookie_free() {
        let navigate = navigate_command(7, "https://www.netflix.com/watch/1");
        let close = close_browser_command(8);

        assert_eq!(navigate.method, "Page.navigate");
        assert_eq!(navigate.params["url"], "https://www.netflix.com/watch/1");
        assert_eq!(close.method, "Browser.close");
        assert!(!serde_json::to_string(&navigate)
            .expect("serialize")
            .to_ascii_lowercase()
            .contains("cookie"));
    }
}
