use std::{net::Ipv4Addr, path::PathBuf};

use serde::Deserialize;
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
    pub signed_in: bool,
    pub local_ipv4: Option<Ipv4Addr>,
    pub peers: Vec<TailscalePeer>,
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
        let output = Command::new(&executable)
            .args(["status", "--json"])
            .output()
            .await;

        match output {
            Ok(output) if output.status.success() => {
                return parse_status_json(&output.stdout);
            }
            Ok(output) => {
                last_error = Some(String::from_utf8_lossy(&output.stderr).to_string());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                last_error = Some(error.to_string());
            }
        }
    }

    match last_error {
        Some(error) if !error.trim().is_empty() => Err(TailscaleError::CommandFailed(error)),
        _ => Err(TailscaleError::ExecutableNotFound),
    }
}

pub fn parse_status_json(bytes: &[u8]) -> Result<TailscaleStatus, TailscaleError> {
    let raw: RawStatus = serde_json::from_slice(bytes)
        .map_err(|error| TailscaleError::InvalidStatus(error.to_string()))?;

    let signed_in = matches!(
        raw.backend_state.as_deref(),
        Some("Running" | "Starting" | "NeedsLogin")
    ) && !matches!(raw.backend_state.as_deref(), Some("NeedsLogin"));

    let local_ipv4 = raw
        .self_node
        .as_ref()
        .and_then(|node| first_ipv4(&node.tailscale_ips));

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
        signed_in,
        local_ipv4,
        peers,
    })
}

fn first_ipv4(values: &[String]) -> Option<Ipv4Addr> {
    values
        .iter()
        .find_map(|value| value.parse::<Ipv4Addr>().ok())
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

#[derive(Debug, Deserialize)]
struct RawNode {
    #[serde(default)]
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
    use super::{parse_status_json, TailscalePath};

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
}
