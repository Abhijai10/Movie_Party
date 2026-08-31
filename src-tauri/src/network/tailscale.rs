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

impl TailscalePeer {
    /// Returns the first usable Tailscale CGNAT IPv4 address, if any.
    pub fn usable_ipv4(&self) -> Option<Ipv4Addr> {
        self.tailscale_ips
            .iter()
            .copied()
            .find(|&ip| is_usable_tailscale_ipv4(ip))
    }
}

/// Returns the subset of peers that are online and have at least one usable
/// Tailscale CGNAT IPv4 address. Non-Tailscale or offline peers are excluded.
pub fn usable_peers<'a>(peers: &'a [TailscalePeer]) -> Vec<&'a TailscalePeer> {
    peers
        .iter()
        .filter(|p| p.online && p.usable_ipv4().is_some())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleStatus {
    pub backend_state: Option<String>,
    pub local_ipv4: Option<Ipv4Addr>,
    pub device_name: Option<String>,
    pub peers: Vec<TailscalePeer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TailscaleState {
    NotInstalled,
    DaemonUnavailable,
    NeedsLogin,
    Stopped,
    NoUsableAddress,
    Ready,
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
        self.state == TailscaleState::Ready
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
    #[cfg(target_os = "macos")]
    {
        vec![
            PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale"),
            PathBuf::from("/opt/homebrew/bin/tailscale"),
            PathBuf::from("/usr/local/bin/tailscale"),
            PathBuf::from("tailscale"),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        vec![
            PathBuf::from("tailscale.exe"),
            PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe"),
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec![PathBuf::from("tailscale")]
    }
}

pub async fn detect_status() -> Result<TailscaleStatus, TailscaleError> {
    let mut last_error = None;

    for executable in candidate_executables() {
        let output = tokio::time::timeout(
            Duration::from_secs(3),
            Command::new(&executable)
                .args(["status", "--json"])
                .output(),
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
            state: TailscaleState::Ready,
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
            state: TailscaleState::DaemonUnavailable,
            code: Some("MP-NET-TS-003".to_string()),
            ip: None,
            device_name: None,
            message: "Tailscale is installed but its daemon is not responding. Make sure Tailscale is running."
                .to_string(),
        },
    }
}

pub fn readiness_from_status(status: TailscaleStatus) -> TailscaleReadiness {
    match status.backend_state.as_deref() {
        Some("NoState") | Some("NeedsLogin") | Some("NeedsMachineAuth") => {
            TailscaleReadiness {
                state: TailscaleState::NeedsLogin,
                code: Some("MP-NET-TS-002".to_string()),
                ip: None,
                device_name: status.device_name,
                message: "Tailscale is installed but not signed in. Open Tailscale to sign in and connect."
                    .to_string(),
            }
        }
        Some("Stopped") => {
            let has_ip = status.local_ipv4.is_some();
            TailscaleReadiness {
                state: TailscaleState::Stopped,
                code: Some("MP-NET-TS-006".to_string()),
                ip: status.local_ipv4.map(|ip| ip.to_string()),
                device_name: status.device_name,
                message: if has_ip {
                    "Tailscale is installed and authenticated, but the connection is stopped. Open Tailscale to connect."
                        .to_string()
                } else {
                    "Tailscale connection is stopped. Open Tailscale to connect.".to_string()
                },
            }
        }
        Some("Starting") => TailscaleReadiness {
            state: TailscaleState::Stopped,
            code: Some("MP-NET-TS-006".to_string()),
            ip: status.local_ipv4.map(|ip| ip.to_string()),
            device_name: status.device_name,
            message: "Tailscale is starting. Please wait and check again.".to_string(),
        },
        Some("Running") => match status.local_ipv4 {
            Some(ip) if is_usable_tailscale_ipv4(ip) => TailscaleReadiness {
                state: TailscaleState::Ready,
                code: None,
                ip: Some(ip.to_string()),
                device_name: status.device_name,
                message: "Private connection ready.".to_string(),
            },
            _ => TailscaleReadiness {
                state: TailscaleState::NoUsableAddress,
                code: Some("MP-NET-TS-004".to_string()),
                ip: None,
                device_name: status.device_name,
                message:
                    "Tailscale is connected but has no usable private IPv4 address."
                        .to_string(),
            },
        },
        None => TailscaleReadiness {
            state: TailscaleState::DaemonUnavailable,
            code: Some("MP-NET-TS-003".to_string()),
            ip: None,
            device_name: status.device_name,
            message:
                "Tailscale returned a status without a backend state. Check that Tailscale is running."
                    .to_string(),
        },
        Some(other) => TailscaleReadiness {
            state: TailscaleState::DaemonUnavailable,
            code: Some("MP-NET-TS-003".to_string()),
            ip: None,
            device_name: status.device_name,
            message: format!(
                "Tailscale status is unknown ({}). Check that its service is running.",
                other
            ),
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

pub fn open_tailscale_app() -> Result<(), TailscaleError> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("open")
            .args(["-a", "Tailscale"])
            .output()
            .map_err(|e| TailscaleError::CommandFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(TailscaleError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        for executable in candidate_executables() {
            match std::process::Command::new(&executable).spawn() {
                Ok(_) => return Ok(()),
                Err(_) => continue,
            }
        }
        Err(TailscaleError::ExecutableNotFound)
    }
}

pub fn parse_status_json(bytes: &[u8]) -> Result<TailscaleStatus, TailscaleError> {
    let raw: RawStatus = serde_json::from_slice(bytes)
        .map_err(|error| TailscaleError::InvalidStatus(error.to_string()))?;

    let local_ipv4: Option<Ipv4Addr> = raw
        .self_node
        .as_ref()
        .map(|node: &RawNode| node.tailscale_ips.clone())
        .and_then(|ips: Vec<String>| first_usable_tailscale_ipv4(&ips));
    let device_name = raw
        .self_node
        .as_ref()
        .and_then(|node| node.dns_name.clone());

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
                .filter(|&ip| is_usable_tailscale_ipv4(ip))
                .collect(),
            online: peer.online.unwrap_or(false),
            path: classify_path(peer.cur_addr.as_deref(), peer.relay.as_deref(), peer.online),
        })
        .collect();

    Ok(TailscaleStatus {
        backend_state: raw.backend_state,
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

pub fn is_usable_tailscale_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

pub fn is_allowed_party_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || is_usable_tailscale_ipv4(ip)
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
    use super::{
        is_allowed_party_ipv4, is_usable_tailscale_ipv4, parse_status_json, readiness_from_error,
        readiness_from_status, required_ipv4, usable_peers, TailscaleError, TailscalePath,
        TailscalePeer, TailscaleState,
    };

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
    fn running_with_usable_ip_is_ready() {
        let status = parse_status_json(
            br#"{"BackendState":"Running","Self":{"DNSName":"cinema.tailnet.ts.net.","TailscaleIPs":["100.64.0.10"]}}"#,
        )
        .expect("valid connected status");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::Ready);
        assert_eq!(readiness.ip.as_deref(), Some("100.64.0.10"));
        assert_eq!(
            readiness.device_name.as_deref(),
            Some("cinema.tailnet.ts.net.")
        );
        assert!(readiness.code.is_none());
    }

    #[test]
    fn needs_login_returns_needs_login_not_unavailable() {
        let status = parse_status_json(
            br#"{"BackendState":"NeedsLogin","Self":{"TailscaleIPs":["100.64.0.10"]}}"#,
        )
        .expect("valid NeedsLogin status");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::NeedsLogin);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-002"));
    }

    #[test]
    fn no_state_maps_to_needs_login() {
        let status = parse_status_json(br#"{"BackendState":"NoState","Self":{"TailscaleIPs":[]}}"#)
            .expect("valid NoState status");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::NeedsLogin);
    }

    #[test]
    fn needs_machine_auth_maps_to_needs_login() {
        let status =
            parse_status_json(br#"{"BackendState":"NeedsMachineAuth","Self":{"TailscaleIPs":[]}}"#)
                .expect("valid NeedsMachineAuth status");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::NeedsLogin);
    }

    #[test]
    fn stopped_with_ips_is_stopped_not_unavailable() {
        let status = parse_status_json(
            br#"{"BackendState":"Stopped","Self":{"TailscaleIPs":["100.114.120.114"]}}"#,
        )
        .expect("valid stopped status with IPs");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::Stopped);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-006"));
        assert_eq!(readiness.ip.as_deref(), Some("100.114.120.114"));
    }

    #[test]
    fn real_macos_machine_state_stopped_with_ip_is_not_unavailable() {
        // Captured from `tailscale status --json` on a real macOS install where
        // Tailscale.app is running but the tunnel is stopped. The machine is
        // authenticated (HaveNodeKey, UserID, DNSName, TailscaleIPs all present).
        let status = parse_status_json(
            br#"{
              "BackendState": "Stopped",
              "HaveNodeKey": true,
              "AuthURL": "",
              "TailscaleIPs": ["100.114.120.114", "fd7a:115c:a1e0::3901:788a"],
              "Self": {
                "DNSName": "abhijais-macbook-air.tailc930b7.ts.net.",
                "UserID": 7429568193493714,
                "TailscaleIPs": ["100.114.120.114", "fd7a:115c:a1e0::3901:788a"],
                "Online": false
              },
              "Peer": null
            }"#,
        )
        .expect("real macOS status must parse");
        assert_eq!(status.backend_state.as_deref(), Some("Stopped"));
        assert_eq!(
            status.local_ipv4.map(|ip| ip.to_string()).as_deref(),
            Some("100.114.120.114")
        );
        assert_eq!(
            status.device_name.as_deref(),
            Some("abhijais-macbook-air.tailc930b7.ts.net.")
        );

        let readiness = readiness_from_status(status);
        assert_eq!(
            readiness.state,
            TailscaleState::Stopped,
            "a stopped-but-authenticated machine must NOT be reported as unavailable"
        );
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-006"));
        assert!(!readiness.is_usable());
    }

    #[test]
    fn ipv6_only_tailscale_address_is_no_usable_address() {
        // An authenticated tailnet node whose only Tailscale address is IPv6 has
        // no usable private IPv4 for the QUIC transport and must not be "ready".
        let status = parse_status_json(
            br#"{"BackendState":"Running","Self":{"TailscaleIPs":["fd7a:115c:a1e0::3901:788a"]}}"#,
        )
        .expect("valid status with only IPv6");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::NoUsableAddress);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-004"));
    }

    #[test]
    fn stopped_without_ips_is_stopped_not_unavailable() {
        let status = parse_status_json(br#"{"BackendState":"Stopped","Self":{"TailscaleIPs":[]}}"#)
            .expect("valid stopped status");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::Stopped);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-006"));
        assert!(readiness.ip.is_none());
    }

    #[test]
    fn starting_is_stopped_with_check_again_message() {
        let status = parse_status_json(
            br#"{"BackendState":"Starting","Self":{"TailscaleIPs":["100.64.0.10"]}}"#,
        )
        .expect("valid Starting status");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::Stopped);
        assert!(readiness.message.contains("starting"));
    }

    #[test]
    fn readiness_rejects_loopback_and_missing_tailscale_ip() {
        let status = parse_status_json(
            br#"{"BackendState":"Running","Self":{"TailscaleIPs":["127.0.0.1","192.168.1.12"]}}"#,
        )
        .expect("valid status without Tailscale IPv4");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::NoUsableAddress);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-004"));
    }

    #[test]
    fn running_without_self_is_no_usable_address() {
        let status = parse_status_json(br#"{"BackendState":"Running","Self":null}"#)
            .expect("valid status with null Self");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::NoUsableAddress);
    }

    #[test]
    fn malformed_status_is_rejected_without_panicking() {
        assert!(parse_status_json(b"not json").is_err());
    }

    #[test]
    fn executable_not_found_maps_to_not_installed() {
        let readiness = readiness_from_error(TailscaleError::ExecutableNotFound);
        assert_eq!(readiness.state, TailscaleState::NotInstalled);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-001"));
    }

    #[test]
    fn command_failed_maps_to_daemon_unavailable() {
        let readiness = readiness_from_error(TailscaleError::CommandFailed(
            "connection refused".to_string(),
        ));
        assert_eq!(readiness.state, TailscaleState::DaemonUnavailable);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-003"));
    }

    #[test]
    fn invalid_status_maps_to_daemon_unavailable() {
        let readiness = readiness_from_error(TailscaleError::InvalidStatus("bad json".to_string()));
        assert_eq!(readiness.state, TailscaleState::DaemonUnavailable);
    }

    #[test]
    fn unknown_backend_state_maps_to_daemon_unavailable() {
        let status =
            parse_status_json(br#"{"BackendState":"AwaitingKey","Self":{"TailscaleIPs":[]}}"#)
                .expect("valid status with unknown state");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::DaemonUnavailable);
    }

    #[test]
    fn missing_backend_state_maps_to_daemon_unavailable() {
        let status = parse_status_json(br#"{"Self":{"TailscaleIPs":["100.64.0.10"]}}"#)
            .expect("valid status without BackendState");
        let readiness = readiness_from_status(status);
        assert_eq!(readiness.state, TailscaleState::DaemonUnavailable);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-003"));
    }

    #[test]
    fn create_precondition_requires_usable_private_address() {
        let needs_login = readiness_from_status(
            parse_status_json(br#"{"BackendState":"NeedsLogin","Self":{"TailscaleIPs":[]}}"#)
                .expect("valid NeedsLogin status"),
        );
        assert!(required_ipv4(&needs_login)
            .expect_err("NeedsLogin must block create")
            .starts_with("MP-NET-TS-002"));

        let stopped = readiness_from_status(
            parse_status_json(
                br#"{"BackendState":"Stopped","Self":{"TailscaleIPs":["100.64.0.10"]}}"#,
            )
            .expect("valid Stopped status"),
        );
        assert!(required_ipv4(&stopped)
            .expect_err("Stopped must block create")
            .starts_with("MP-NET-TS-006"));

        let ready = readiness_from_status(
            parse_status_json(
                br#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.10"]}}"#,
            )
            .expect("valid ready status"),
        );
        assert_eq!(
            required_ipv4(&ready).expect("usable IP"),
            "100.64.0.10".parse::<std::net::Ipv4Addr>().unwrap()
        );
    }

    #[test]
    fn is_usable_returns_true_only_for_ready() {
        use super::TailscaleReadiness;
        for state in &[
            TailscaleState::NotInstalled,
            TailscaleState::DaemonUnavailable,
            TailscaleState::NeedsLogin,
            TailscaleState::Stopped,
            TailscaleState::NoUsableAddress,
        ] {
            let r = TailscaleReadiness {
                state: state.clone(),
                code: None,
                ip: None,
                device_name: None,
                message: String::new(),
            };
            assert!(!r.is_usable(), "{:?} should not be usable", state);
        }
        let r = TailscaleReadiness {
            state: TailscaleState::Ready,
            code: None,
            ip: Some("100.64.0.10".to_string()),
            device_name: None,
            message: String::new(),
        };
        assert!(r.is_usable());
    }

    #[test]
    fn usable_tailscale_ipv4_only_accepts_cgnat_range() {
        assert!(is_usable_tailscale_ipv4("100.64.0.1".parse().unwrap()));
        assert!(is_usable_tailscale_ipv4("100.127.255.254".parse().unwrap()));
        assert!(!is_usable_tailscale_ipv4("100.63.255.255".parse().unwrap()));
        assert!(!is_usable_tailscale_ipv4("100.128.0.1".parse().unwrap()));
        assert!(!is_usable_tailscale_ipv4("127.0.0.1".parse().unwrap()));
        assert!(!is_usable_tailscale_ipv4("192.168.1.12".parse().unwrap()));
        assert!(!is_usable_tailscale_ipv4("10.0.0.5".parse().unwrap()));
    }

    #[test]
    fn allowed_party_ipv4_is_loopback_or_tailscale_only() {
        assert!(is_allowed_party_ipv4("127.0.0.1".parse().unwrap()));
        assert!(is_allowed_party_ipv4("100.64.0.10".parse().unwrap()));
        assert!(!is_allowed_party_ipv4("192.168.1.12".parse().unwrap()));
        assert!(!is_allowed_party_ipv4("10.0.0.5".parse().unwrap()));
        assert!(!is_allowed_party_ipv4("0.0.0.0".parse().unwrap()));
    }

    #[test]
    fn peer_usable_ipv4_selects_first_cgnat_and_ignores_lan() {
        let peer = TailscalePeer {
            dns_name: "guest.tailnet.ts.net.".to_string(),
            tailscale_ips: vec![
                "127.0.0.1".parse().unwrap(),
                "192.168.1.12".parse().unwrap(),
                "100.64.0.42".parse().unwrap(),
            ],
            online: true,
            path: TailscalePath::Direct,
        };
        assert_eq!(
            peer.usable_ipv4().expect("usable ipv4").to_string(),
            "100.64.0.42"
        );
    }

    #[test]
    fn peer_parse_filters_out_non_tailscale_ipv4() {
        let status = parse_status_json(
            br#"{
              "BackendState": "Running",
              "Self": {"TailscaleIPs": ["100.64.0.10"]},
              "Peer": {
                "g": {
                  "DNSName": "guest.tailnet.ts.net.",
                  "TailscaleIPs": ["100.64.0.42", "192.168.1.12", "fd7a:115c:a1e0::2"],
                  "Online": true,
                  "CurAddr": "100.64.0.42:47821"
                }
              }
            }"#,
        )
        .expect("valid status");
        let peer = &status.peers[0];
        assert_eq!(peer.tailscale_ips.len(), 1);
        assert_eq!(peer.tailscale_ips[0].to_string(), "100.64.0.42");
        assert_eq!(peer.usable_ipv4().expect("ipv4").to_string(), "100.64.0.42");
    }

    #[test]
    fn usable_peers_excludes_offline_and_peer_without_cgnat() {
        let online = TailscalePeer {
            dns_name: "a".to_string(),
            tailscale_ips: vec!["100.64.0.10".parse().unwrap()],
            online: true,
            path: TailscalePath::Direct,
        };
        let offline = TailscalePeer {
            dns_name: "b".to_string(),
            tailscale_ips: vec!["100.64.0.11".parse().unwrap()],
            online: false,
            path: TailscalePath::Offline,
        };
        let lan_only = TailscalePeer {
            dns_name: "c".to_string(),
            tailscale_ips: vec!["192.168.1.12".parse().unwrap()],
            online: true,
            path: TailscalePath::Direct,
        };
        let all = [online.clone(), offline, lan_only];
        let result = usable_peers(&all);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].dns_name, "a");
    }
}
