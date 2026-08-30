use std::{net::Ipv4Addr, path::PathBuf, time::Duration};

use serde::{self, Deserialize, Serialize};
use tokio::process::Command;

pub const DEFAULT_TAILSCALE_PORT: u16 = 47_821;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailscalePath {
    Direct,
    PeerRelay,
    DerpRelay,
    Unknown,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscalePeer {
    pub dns_name: String,
    pub tailscale_ips: Vec<Ipv4Addr>,
    pub online: bool,
    pub path: TailscalePath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleStatus {
    pub backend_state: Option<String>,
    pub signed_in: bool,
    pub local_ipv4: Option<Ipv4Addr>,
    pub device_name: Option<String>,
    pub peers: Vec<TailscalePeer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TailscaleState {
    NotInstalled,
    SignedOut,
    Connected,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleReadiness {
    pub state: TailscaleState,
    pub code: Option<String>,
    pub ip: Option<String>,
    pub device_name: Option<String>,
    pub message: String,
}

impl TailscaleReadiness {
    pub fn is_usable(&self) -> bool {
        self.state == TailscaleState::Connected
    }

    pub fn stable_error(&self) -> Option<String> {
        self.code
            .as_ref()
            .map(|code| format!("{code} {}", self.message))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TailscaleError {
    #[error("MP-NET-001 Tailscale executable was not found")]
    ExecutableNotFound,
    #[error("MP-NET-001 Tailscale status command failed: {0}")]
    CommandFailed(String),
    #[error("MP-NET-001 Tailscale output could not be parsed: {0}")]
    InvalidStatus(String),
}

pub fn candidate_executables() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("tailscale")];

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        ));
        candidates.push(PathBuf::from("/opt/homebrew/bin/tailscale"));
        candidates.push(PathBuf::from("/usr/local/bin/tailscale"));
    }

    #[cfg(target_os = "windows")]
    {
        candidates.push(PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe"));
    }

    candidates
}

pub async fn detect_status() -> Result<TailscaleStatus, TailscaleError> {
    let mut last_error = None;

    for executable in candidate_executables() {
        let output = tokio::time::timeout(
            Duration::from_secs(3),
            Command::new(&executable).args(["status", "--json"]).output(),
        )
        .await;

        match output {
            Ok(Ok(output)) if output.status.success() => {
                return parse_status_json(&output.stdout);
            }
            Ok(Ok(output)) => {
                last_error = Some(String::from_utf8_lossy(&output.stderr).to_string());
            }
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(Err(error)) => {
                last_error = Some(error.to_string());
            }
            Err(_) => last_error = Some("status check timed out".to_string()),
        }
    }

    match last_error {
        Some(error) if !error.trim().is_empty() => Err(TailscaleError::CommandFailed(error)),
        _ => Err(TailscaleError::ExecutableNotFound),
    }
}

pub async fn local_readiness() -> TailscaleReadiness {
    if dev_loopback_enabled() {
        return TailscaleReadiness {
            state: TailscaleState::Connected,
            code: None,
            ip: Some("127.0.0.1".to_string()),
            device_name: Some("Development loopback".to_string()),
            message: "Development loopback is enabled.".to_string(),
        };
    }

    match detect_status().await {
        Ok(status) => readiness_from_status(status),
        Err(error) => readiness_from_error(error),
    }
}

pub fn readiness_from_error(error: TailscaleError) -> TailscaleReadiness {
    match error {
        TailscaleError::ExecutableNotFound => TailscaleReadiness {
            state: TailscaleState::NotInstalled,
            code: Some("MP-NET-TS-001".to_string()),
            ip: None,
            device_name: None,
            message: "Install Tailscale to create or join a private cinema.".to_string(),
        },
        TailscaleError::CommandFailed(_) | TailscaleError::InvalidStatus(_) => TailscaleReadiness {
            state: TailscaleState::Unavailable,
            code: Some("MP-NET-TS-003".to_string()),
            ip: None,
            device_name: None,
            message: "Tailscale is installed but unavailable. Check that its service is running."
                .to_string(),
        },
    }
}

pub fn readiness_from_status(status: TailscaleStatus) -> TailscaleReadiness {
    if !status.signed_in {
        if !matches!(status.backend_state.as_deref(), Some("NeedsLogin")) {
            return TailscaleReadiness {
                state: TailscaleState::Unavailable,
                code: Some("MP-NET-TS-003".to_string()),
                ip: None,
                device_name: status.device_name,
                message: "Tailscale is installed but its service is unavailable.".to_string(),
            };
        }
        return TailscaleReadiness {
            state: TailscaleState::SignedOut,
            code: Some("MP-NET-TS-002".to_string()),
            ip: None,
            device_name: status.device_name,
            message: "Tailscale is installed but not signed in.".to_string(),
        };
    }

    match status.local_ipv4 {
        Some(ip) if is_usable_tailscale_ipv4(ip) => TailscaleReadiness {
            state: TailscaleState::Connected,
            code: None,
            ip: Some(ip.to_string()),
            device_name: status.device_name,
            message: "Private connection ready.".to_string(),
        },
        _ => TailscaleReadiness {
            state: TailscaleState::Unavailable,
            code: Some("MP-NET-TS-004".to_string()),
            ip: None,
            device_name: status.device_name,
            message: "Tailscale is connected but has no usable private IPv4 address.".to_string(),
        },
    }
}

pub fn required_ipv4(readiness: &TailscaleReadiness) -> Result<Ipv4Addr, String> {
    if !readiness.is_usable() {
        return Err(readiness.stable_error().unwrap_or_else(|| {
            "MP-NET-TS-004 Tailscale has no usable private IPv4 address".to_string()
        }));
    }
    readiness
        .ip
        .as_deref()
        .and_then(|value| value.parse().ok())
        .filter(|ip| is_usable_tailscale_ipv4(*ip))
        .ok_or_else(|| "MP-NET-TS-004 Tailscale has no usable private IPv4 address".to_string())
}

pub fn dev_loopback_enabled() -> bool {
    std::env::var("MOVIE_PARTY_DEV_LOOPBACK")
        .map(|value| value == "1")
        .unwrap_or(false)
}

pub async fn begin_sign_in() -> Result<(), TailscaleError> {
    for executable in candidate_executables() {
        match Command::new(&executable).arg("up").spawn() {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(TailscaleError::CommandFailed(error.to_string())),
        }
    }
    Err(TailscaleError::ExecutableNotFound)
}

pub fn parse_status_json(bytes: &[u8]) -> Result<TailscaleStatus, TailscaleError> {
    let raw: RawStatus = serde_json::from_slice(bytes)
        .map_err(|error| TailscaleError::InvalidStatus(error.to_string()))?;

    let signed_in = matches!(
        raw.backend_state.as_deref(),
        Some("Running" | "Starting" | "NeedsLogin")
    ) && !matches!(raw.backend_state.as_deref(), Some("NeedsLogin"));

    let local_ipv4: Option<Ipv4Addr> = raw
        .self_node
        .as_ref()
        .map(|node: &RawNode| node.tailscale_ips.clone())
        .and_then(|ips: Vec<String>| first_usable_tailscale_ipv4(&ips));
    let device_name = raw.self_node.as_ref().and_then(|node| node.dns_name.clone());

    let peers = raw
        .peer
        .unwrap_or_default()
        .into_values()
        .map(|peer| TailscalePeer {
            dns_name: peer.dns_name.unwrap_or_default(),
            tailscale_ips: peer
                .tailscale_ips
                .iter()
                .filter_map(|value| value.parse::<Ipv4Addr>().ok())
                .collect(),
            online: peer.online.unwrap_or(false),
            path: classify_path(peer.cur_addr.as_deref(), peer.relay.as_deref(), peer.online),
        })
        .collect();

    Ok(TailscaleStatus {
        backend_state: raw.backend_state,
        signed_in,
        local_ipv4,
        device_name,
        peers,
    })
}

fn first_usable_tailscale_ipv4(values: &[String]) -> Option<Ipv4Addr> {
    values
        .iter()
        .filter_map(|value| value.parse::<Ipv4Addr>().ok())
        .find(|ip| is_usable_tailscale_ipv4(*ip))
}

fn is_usable_tailscale_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn classify_path(
    current_address: Option<&str>,
    relay: Option<&str>,
    online: Option<bool>,
) -> TailscalePath {
    if online == Some(false) {
        return TailscalePath::Offline;
    }

    if relay.is_some_and(|value| value.contains("peer-relay")) {
        return TailscalePath::PeerRelay;
    }

    if relay.is_some() || current_address.is_some_and(|value| value.starts_with("derp-")) {
        return TailscalePath::DerpRelay;
    }

    if current_address.is_some() {
        return TailscalePath::Direct;
    }

    TailscalePath::Unknown
}

#[derive(Debug, Deserialize)]
struct RawStatus {
    #[serde(rename = "BackendState")]
    backend_state: Option<String>,
    #[serde(rename = "Self")]
    self_node: Option<RawNode>,
    #[serde(rename = "Peer")]
    peer: Option<std::collections::BTreeMap<String, RawNode>>,
}

fn option_vec_string_null_as_empty<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Vec<String>> = Deserialize::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct RawNode {
    #[serde(default, deserialize_with = "option_vec_string_null_as_empty")]
    #[serde(rename = "TailscaleIPs")]
    tailscale_ips: Vec<String>,
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
    #[serde(rename = "Online")]
    online: Option<bool>,
    #[serde(rename = "CurAddr")]
    cur_addr: Option<String>,
    #[serde(rename = "Relay")]
    relay: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{parse_status_json, readiness_from_error, readiness_from_status, required_ipv4, TailscaleError, TailscalePath, TailscaleState};

    #[test]
    fn parses_local_ipv4_and_peer_path() {
        let status = parse_status_json(
            br#"{
              "BackendState": "Running",
              "Self": {"TailscaleIPs": ["100.64.0.10", "fd7a:115c:a1e0::1"]},
              "Peer": {
                "peer": {
                  "DNSName": "rahul.tailnet.ts.net.",
                  "TailscaleIPs": ["100.64.0.11"],
                  "Online": true,
                  "CurAddr": "192.0.2.1:41641"
                }
              }
            }"#,
        )
        .expect("valid Tailscale status");

        assert!(status.signed_in);
        assert_eq!(status.local_ipv4.expect("ipv4").to_string(), "100.64.0.10");
        assert_eq!(status.peers[0].path, TailscalePath::Direct);
    }

    #[test]
    fn parses_real_macos_tailscale_json_with_nulls() {
        let json_str = r#"{"BackendState": "Running", "TailscaleIPs": null, "Self": {"TailscaleIPs": null, "DNSName": "", "Online": false, "CurAddr": "", "Relay": ""}, "Peer": null}"#;
        let result = parse_status_json(json_str.as_bytes());
        assert!(
            result.is_ok(),
            "null-valued fields must not fail to parse: {:?}",
            result.err()
        );
        let status = result.unwrap();
        assert!(
            status.signed_in,
            "BackendState=Running means tailscale is signed in"
        );
        assert!(status.local_ipv4.is_none(), "null IPs → None");
        assert!(status.peers.is_empty());
    }

    #[test]
    fn classifies_derp_and_offline() {
        let status = parse_status_json(
            br#"{
              "BackendState": "Running",
              "Self": {"TailscaleIPs": ["100.64.0.10"]},
              "Peer": {
                "derp": {"DNSName": "derp.", "Online": true, "CurAddr": "derp-3"},
                "off": {"DNSName": "off.", "Online": false}
              }
            }"#,
        )
        .expect("valid Tailscale status");

        assert_eq!(status.peers[0].path, TailscalePath::DerpRelay);
        assert_eq!(status.peers[1].path, TailscalePath::Offline);
    }

    #[test]
    fn readiness_distinguishes_signed_out_from_connected() {
        let signed_out = parse_status_json(
            br#"{"BackendState":"NeedsLogin","Self":{"TailscaleIPs":["100.64.0.10"]}}"#,
        )
        .expect("valid signed-out status");
        assert_eq!(readiness_from_status(signed_out).state, TailscaleState::SignedOut);

        let connected = parse_status_json(
            br#"{"BackendState":"Running","Self":{"DNSName":"cinema.tailnet.ts.net.","TailscaleIPs":["100.64.0.10"]}}"#,
        )
        .expect("valid connected status");
        let readiness = readiness_from_status(connected);
        assert_eq!(readiness.state, TailscaleState::Connected);
        assert_eq!(readiness.ip.as_deref(), Some("100.64.0.10"));
        assert_eq!(readiness.device_name.as_deref(), Some("cinema.tailnet.ts.net."));
    }

    #[test]
    fn readiness_rejects_loopback_and_missing_tailscale_ip() {
        let status = parse_status_json(
            br#"{"BackendState":"Running","Self":{"TailscaleIPs":["127.0.0.1","192.168.1.12"]}}"#,
        )
        .expect("valid status without Tailscale IPv4");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::Unavailable);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-004"));
    }

    #[test]
    fn stopped_daemon_is_unavailable_not_signed_out() {
        let status = parse_status_json(
            br#"{"BackendState":"Stopped","Self":{"TailscaleIPs":[]}}"#,
        )
        .expect("valid stopped status");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::Unavailable);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-003"));
    }

    #[test]
    fn malformed_status_is_rejected_without_panicking() {
        assert!(parse_status_json(b"not json").is_err());
    }

    #[test]
    fn executable_missing_maps_to_not_installed() {
        let readiness = readiness_from_error(TailscaleError::ExecutableNotFound);
        assert_eq!(readiness.state, TailscaleState::NotInstalled);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-001"));
    }

    #[test]
    fn create_precondition_requires_a_usable_private_address() {
        let signed_out = readiness_from_status(
            parse_status_json(br#"{"BackendState":"NeedsLogin","Self":{"TailscaleIPs":[]}}"#)
                .expect("valid signed-out status"),
        );
        assert!(required_ipv4(&signed_out)
            .expect_err("signed-out state must block create")
            .starts_with("MP-NET-TS-002"));

        let connected = readiness_from_status(
            parse_status_json(br#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.10"]}}"#)
                .expect("valid connected status"),
        );
        assert_eq!(
            required_ipv4(&connected).expect("usable IP"),
            "100.64.0.10".parse::<std::net::Ipv4Addr>().unwrap()
        );
    }
}
