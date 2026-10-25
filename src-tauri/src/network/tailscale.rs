use std::{net::Ipv4Addr, path::PathBuf, time::Duration};

use serde::{self, Deserialize, Serialize};

pub const DEFAULT_TAILSCALE_PORT: u16 = 47_821;

/// The macOS Tailscale CLI bundled with the GUI app is an IPC client, not a
/// standalone daemon client. When it is spawned by a GUI application
/// (Finder/Dock launch → no terminal ancestry), `TERM` is absent from the
/// environment and the CLI falls back to a GUI-attachment path that fails
/// with `Tailscale.CLIError error 3` — printed to **stdout with exit code
/// 0**, which previously masqueraded as valid status output and surfaced as
/// the eternal "Tailscale is installed but not responding" gate. Setting any
/// `TERM` value restores the socket/IPC path used from terminals. This is
/// verified experimentally against Tailscale 1.102.4 on macOS.
const GUI_ENV_TERM: (&str, &str) = ("TERM", "dumb");

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
    /// The MagicDNS suffix part of `dns_name` ("tailabc123.ts.net"). A peer
    /// whose suffix differs from this device's own tailnet suffix reached the
    /// tailnet through sharing rather than membership.
    pub dns_suffix: String,
    /// The CLI's `ShareeNode` flag: a node shared in from another tailnet.
    /// Absent (older CLIs / own-tailnet peers) is `false`, which is why the
    /// suffix comparison is the primary signal and this only corroborates it.
    pub shared_in: bool,
    /// RFC3339 expiry of that node's key, when the CLI reports one.
    pub key_expiry: Option<String>,
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
pub fn usable_peers(peers: &[TailscalePeer]) -> Vec<&TailscalePeer> {
    peers
        .iter()
        .filter(|p| p.online && p.usable_ipv4().is_some())
        .collect()
}

/// A tailnet peer presented for friend selection: stable key (DNS name),
/// display name, usable IP, online flag, and the current path classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendCandidate {
    /// Stable identity key: the peer's full Tailscale DNS name (ends with
    /// `.<tailnet>.ts.net.`). Survives IP changes and renames of the label.
    pub peer_key: String,
    /// Human-facing device label (hostname part of the DNS name).
    pub display_name: String,
    /// First usable Tailscale CGNAT IPv4, when present.
    pub ip: Option<String>,
    pub online: bool,
    /// direct | peerRelay | derpRelay | unknown | offline
    pub path: String,
    /// `Some(true)` same tailnet, `Some(false)` reached through sharing from
    /// another tailnet, `None` when the CLI did not report enough to compare.
    pub same_tailnet: Option<bool>,
}

/// Human label for a peer's DNS name: strips the trailing dot and the
/// MagicDNS suffix so "rahul-mac.tailc930b7.ts.net." reads "rahul-mac".
pub fn peer_display_name(dns_name: &str) -> String {
    let trimmed = dns_name.trim_end_matches('.');
    if let Some(stripped) = trimmed.strip_suffix(".ts.net") {
        // "rahul-mac.tailc930b7" → "rahul-mac" (first label = device name)
        return stripped.split('.').next().unwrap_or(stripped).to_string();
    }
    trimmed.to_string()
}

fn path_label(path: &TailscalePath) -> &'static str {
    match path {
        TailscalePath::Direct => "direct",
        TailscalePath::PeerRelay => "peerRelay",
        TailscalePath::DerpRelay => "derpRelay",
        TailscalePath::Unknown => "unknown",
        TailscalePath::Offline => "offline",
    }
}

/// Maps a parsed status into friend candidates, ordered: online first, then
/// by display name. Includes ALL peers with a usable IP (the UI shows the
/// online flag so offline friends are visible but visually distinct).
pub fn friend_candidates(status: &TailscaleStatus) -> Vec<FriendCandidate> {
    let mut candidates: Vec<FriendCandidate> = status
        .peers
        .iter()
        .filter(|p| p.usable_ipv4().is_some() && !p.dns_name.trim().is_empty())
        .map(|p| FriendCandidate {
            peer_key: p.dns_name.clone(),
            display_name: peer_display_name(&p.dns_name),
            ip: p.usable_ipv4().map(|ip| ip.to_string()),
            online: p.online,
            path: path_label(&p.path).to_string(),
            same_tailnet: peer_is_same_tailnet(status, p),
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    candidates
}

/// The friends surface's payload: selectable tailnet peers plus the saved
/// friends (with their cached verification results).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailnetPeersView {
    pub candidates: Vec<FriendCandidate>,
    pub saved: Vec<crate::storage::sqlite::StoredFriend>,
    /// Which tailnet this device is on and what role its login holds there —
    /// the onboarding wizard needs it to decide who can create a share.
    pub tailnet: Option<TailnetIdentity>,
}

/// verify_friend payload: the refreshed friend record + the raw probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendVerification {
    pub friend: crate::storage::sqlite::StoredFriend,
    pub probe: PeerConnectionProbe,
}

/// Compact base64url (unpadded) decode — the inverse of
/// `base64url_encode`, for parsing received friend invites.
pub fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    if input.is_empty() || input.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for ch in input.chars() {
        let value = ALPHABET.iter().position(|&b| b as char == ch)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Compact base64url (unpadded). Kept for the few places that still encode a
/// token-shaped value; the unsigned friend-link format it was written for is
/// gone (Batch 8 signs invitations, and rejects the old shape as MP-FRIEND-002).
pub fn base64url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map(|b| u32::from(*b)).unwrap_or(0);
        let b2 = chunk.get(2).map(|b| u32::from(*b)).unwrap_or(0);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3F) as usize] as char);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleStatus {
    pub backend_state: Option<String>,
    pub local_ipv4: Option<Ipv4Addr>,
    pub device_name: Option<String>,
    pub peers: Vec<TailscalePeer>,
    pub tailnet: Option<TailnetIdentity>,
}

/// What role this device's Tailscale login holds on its own tailnet, taken
/// from the capability list the CLI reports for `Self`.
///
/// This exists because Movie Party must not assume every user administers
/// their tailnet: creating a share or inviting a user is an administrative
/// action, and a `LimitedUser` cannot do it from their own device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TailnetRole {
    /// The CLI reported the owner capability.
    Owner,
    /// The CLI reported the admin capability (not owner).
    Admin,
    /// Signed in, but the CLI reported capabilities and none was admin/owner.
    LimitedUser,
    /// The CLI reported no capability list at all (older version, or a
    /// key-only node). Movie Party treats this as "unknown", never as
    /// "not an admin".
    Unknown,
}

impl TailnetRole {
    /// True only when this device is *known* to be able to create shares and
    /// invitations itself. `Unknown` is deliberately excluded: the wizard
    /// offers the "ask your tailnet admin" path instead of assuming.
    pub fn can_administer(self) -> bool {
        matches!(self, TailnetRole::Owner | TailnetRole::Admin)
    }
}

/// The tailnet this device belongs to, as reported by the local CLI. Read-only
/// identification — nothing here is a credential, and it never leaves the
/// device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailnetIdentity {
    /// `CurrentTailnet.StableID` — the tailnet's stable id (`Tn…`). The
    /// owner's `Name` is deliberately not captured: for a personal account it
    /// is an email address, and the onboarding UI has no need for it.
    pub stable_id: Option<String>,
    /// MagicDNS suffix of this tailnet, e.g. `tailc930b7.ts.net`.
    pub magic_dns_suffix: Option<String>,
    /// This device's own role on that tailnet.
    pub role: TailnetRole,
    /// RFC3339 expiry of this device's node key, when reported.
    pub key_expiry: Option<String>,
    /// `HaveNodeKey` as reported; `None` means the CLI did not say.
    pub have_node_key: Option<bool>,
    /// `AuthURL` was non-empty: the backend wants an interactive sign-in.
    pub login_url_pending: bool,
}

/// State of a Tailscale node key at a given moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NodeKeyState {
    /// The CLI reported no expiry.
    Unknown,
    Valid,
    /// Expires within `NODE_KEY_WARNING_DAYS` days.
    ExpiringSoon,
    /// The reported expiry is in the past.
    Expired,
}

/// How close to expiry a node key starts warning the user. Tailscale's own
/// key-expiry default is 180 days, so 30 days leaves a full month to act.
pub const NODE_KEY_WARNING_DAYS: i64 = 30;

const SECONDS_PER_DAY: i64 = 86_400;

/// Maps an RFC3339 UTC timestamp (`2027-02-20T20:43:23Z`, as emitted by
/// `tailscale status --json`) to unix milliseconds. Hand-rolled because the
/// crate has no date library and adding one for two fields is not worth it.
pub fn rfc3339_utc_to_ms(value: &str) -> Option<i64> {
    let value = value.trim();
    let (date, time) = value.split_once('T')?;
    let time = time.strip_suffix('Z')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // Day-of-month must be legal for that month, so "2027-02-30" is a failure
    // rather than a silently normalised date 30 days past February 1st.
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some((days * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second) * 1_000)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days since the unix epoch for a proleptic-Gregorian date (Howard Hinnant's
/// `days_from_civil`, shifted so March is the first month of the year).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let month_shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

impl NodeKeyState {
    /// Classify a reported expiry against `now_ms`.
    pub fn from_expiry(expiry: Option<&str>, now_ms: i64) -> Self {
        let Some(ms) = expiry.and_then(rfc3339_utc_to_ms) else {
            return NodeKeyState::Unknown;
        };
        if ms <= now_ms {
            return NodeKeyState::Expired;
        }
        let days_left = (ms - now_ms) / (SECONDS_PER_DAY * 1_000);
        if days_left <= NODE_KEY_WARNING_DAYS {
            NodeKeyState::ExpiringSoon
        } else {
            NodeKeyState::Valid
        }
    }

    pub fn is_blocking(self) -> bool {
        self == NodeKeyState::Expired
    }

    pub fn warns(self) -> bool {
        matches!(self, NodeKeyState::Expired | NodeKeyState::ExpiringSoon)
    }
}

/// The MagicDNS suffix part of a peer DNS name. `host.tailabc.ts.net.` →
/// `tailabc.ts.net`; a single-label name has no suffix and yields "".
pub fn peer_dns_suffix(dns_name: &str) -> String {
    let trimmed = dns_name.trim().trim_end_matches('.');
    match trimmed.split_once('.') {
        Some((_, suffix)) => suffix.to_string(),
        None => String::new(),
    }
}

/// `Some(true)` when the peer shares this device's tailnet suffix, `Some(false)`
/// when the suffixes differ (the peer came from another tailnet through
/// sharing), `None` when either side's suffix is unknown.
pub fn peer_is_same_tailnet(status: &TailscaleStatus, peer: &TailscalePeer) -> Option<bool> {
    let own = status
        .tailnet
        .as_ref()
        .and_then(|t| t.magic_dns_suffix.as_deref())
        .map(|s| s.trim().trim_end_matches('.'))
        .filter(|s| !s.is_empty())?;
    let theirs = peer.dns_suffix.trim().trim_end_matches('.');
    if theirs.is_empty() {
        return None;
    }
    if peer.shared_in {
        return Some(false);
    }
    Some(theirs.eq_ignore_ascii_case(own))
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
    #[error("MP-NET-TS-001 Tailscale executable was not found")]
    ExecutableNotFound,
    #[error("MP-NET-TS-003 Tailscale status command failed: {0}")]
    CommandFailed(String),
    #[error("MP-NET-TS-003 Tailscale output could not be parsed: {0}")]
    InvalidStatus(String),
    #[error("MP-NET-TS-003 could not open the Tailscale app: {0}")]
    OpenFailed(String),
}

/// Errors that the macOS Tailscale GUI-CLI wrapper prints to **stdout**
/// while still exiting 0. `tailscale status --json` must return a JSON
/// document; any of these prefixes means the probe actually failed even
/// though the exit status lies. Without this check the wrapper's error text
/// reached `parse_status_json`, failed, and was reported as
/// DAEMON_UNAVAILABLE ("installed but not responding") on every GUI launch.
const KNOWN_WRAPPER_ERRORS: [&str; 2] = [
    "The Tailscale GUI failed to start",
    "failed to connect to tailscaled",
];

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

/// True when the CLI's stdout is a known wrapper/daemon failure banner
/// rather than status JSON (some of these exit 0 — exit status alone lies).
pub fn is_wrapper_error_stdout(stdout: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stdout);
    let trimmed = text.trim();
    if trimmed.starts_with('{') || trimmed.is_empty() {
        return false;
    }
    KNOWN_WRAPPER_ERRORS
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

pub async fn detect_status() -> Result<TailscaleStatus, TailscaleError> {
    let mut last_error = None;

    for executable in candidate_executables() {
        let mut command = crate::process::quiet_async_command(&executable);
        command.env(GUI_ENV_TERM.0, GUI_ENV_TERM.1);
        let output = tokio::time::timeout(
            Duration::from_secs(3),
            command.args(["status", "--json"]).output(),
        )
        .await;

        match output {
            Ok(Ok(output)) if output.status.success() => {
                if is_wrapper_error_stdout(&output.stdout) {
                    // The wrapper failed but lied with exit code 0 — treat
                    // exactly like a failed run and try the next candidate.
                    last_error = Some(first_line_of(&output.stdout));
                    continue;
                }
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

fn first_line_of(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .next()
        .unwrap_or("unknown CLI failure")
        .trim()
        .to_string()
}

/// A real reachability probe for one tailnet peer, as reported by
/// `tailscale ping` — an actual WireGuard-level packet exchange through the
/// tunnel, not a status-table lookup. This is the "is the connection really
/// established" check behind Add Friend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerConnectionProbe {
    pub reachable: bool,
    /// direct | relay | relay-late | none — from the ping's route line.
    pub path: Option<String>,
    /// Round-trip latency in milliseconds, when the peer answered.
    pub latency_ms: Option<u32>,
    pub message: String,
}

impl PeerConnectionProbe {
    fn ok(path: String, latency_ms: u32) -> Self {
        Self {
            reachable: true,
            path: Some(path),
            latency_ms: Some(latency_ms),
            message: String::new(),
        }
    }

    fn fail(message: String) -> Self {
        Self {
            reachable: false,
            path: None,
            latency_ms: None,
            message,
        }
    }
}

/// Runs `tailscale ping --timeout --c 1 <ip>` and parses the route+latency
/// line. Output formats (verified against 1.102.4):
///   pong from <host> (<ip>) via <path> in <n>ms
///   pong from <host> (<ip>) via <path>-late in <n>ms   (relay warming up)
///   ping "<ip>": timed out / no reply
pub async fn verify_peer_connection(ip: Ipv4Addr) -> PeerConnectionProbe {
    let mut last_error: Option<String> = None;

    for executable in candidate_executables() {
        let mut command = crate::process::quiet_async_command(&executable);
        command.env(GUI_ENV_TERM.0, GUI_ENV_TERM.1);
        let output = tokio::time::timeout(
            Duration::from_secs(6),
            command
                .args(["ping", "--timeout=3s", "--c=1", &ip.to_string()])
                .output(),
        )
        .await;

        match output {
            Ok(Ok(out)) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                if let Some(probe) = parse_ping_stdout(&stdout) {
                    return probe;
                }
                // Unparseable success output — fall through to next candidate.
                last_error = Some(first_line_of(&out.stdout));
            }
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                // `tailscale ping` exit flavors (verified against 1.102.4):
                //   unreachable peer → exit 1, "no matching peer" on stderr;
                //   offline peer     → exit 1, "timed out" on stdout +
                //                       "no reply" on stderr;
                //   reachable peer   → exit 0, "pong from … via … in Nms".
                if stderr.contains("no matching peer") {
                    return PeerConnectionProbe::fail(
                        "MP-NET-TS-008 that device is not in your Tailscale network".to_string(),
                    );
                }
                if stdout.contains("no reply")
                    || stdout.contains("timed out")
                    || stderr.contains("no reply")
                {
                    return PeerConnectionProbe::fail(format!(
                        "MP-NET-TS-007 no answer from {ip} through the tunnel"
                    ));
                }
                last_error = Some(if stderr.trim().is_empty() {
                    first_line_of(&out.stdout)
                } else {
                    stderr.trim().to_string()
                });
            }
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(Err(error)) => last_error = Some(error.to_string()),
            Err(_) => last_error = Some("ping check timed out".to_string()),
        }
    }

    PeerConnectionProbe::fail(format!(
        "MP-NET-TS-003 Tailscale ping could not run: {}",
        last_error.unwrap_or_else(|| "executable was not found".to_string())
    ))
}

/// Parses the first meaningful line of `tailscale ping` output.
/// Returns `None` when the output matches nothing known.
pub fn parse_ping_stdout(stdout: &str) -> Option<PeerConnectionProbe> {
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.contains("no reply") || line.contains("timed out") {
            return Some(PeerConnectionProbe::fail(
                "MP-NET-TS-007 no answer from the peer through the tunnel".to_string(),
            ));
        }
        // pong from host-name (100.x.y.z) via direct/relay in 123ms
        if let Some(rest) = line.strip_prefix("pong from ") {
            if let Some((path, latency)) = parse_pong_route_latency(rest) {
                return Some(PeerConnectionProbe::ok(path, latency));
            }
        }
        // Unrecognized but non-empty line — keep scanning; the ping command
        // may print a leading notice (e.g. "passthrough mode") before pongs.
    }
    None
}

/// Extracts `via <path> in <n>ms` from the tail of a pong line.
fn parse_pong_route_latency(rest: &str) -> Option<(String, u32)> {
    let via_pos = rest.find("via ")?;
    let after_via = &rest[via_pos + 4..];
    let in_pos = after_via.find(" in ")?;
    let path = after_via[..in_pos].trim().to_string();
    let latency_str = after_via[in_pos + 4..].trim();
    let digits: String = latency_str
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let latency = digits.parse::<u32>().ok()?;
    if path.is_empty() {
        return None;
    }
    Some((path, latency))
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
        TailscaleError::CommandFailed(_) | TailscaleError::InvalidStatus(_) | TailscaleError::OpenFailed(_) => {
            TailscaleReadiness {
                state: TailscaleState::DaemonUnavailable,
                code: Some("MP-NET-TS-003".to_string()),
                ip: None,
                device_name: None,
                message: "Tailscale is installed but its daemon is not responding. Make sure Tailscale is running."
                    .to_string(),
            }
        }
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
            .map_err(|e| TailscaleError::OpenFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(TailscaleError::OpenFailed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        for executable in candidate_executables() {
            match crate::process::quiet_command(executable.as_os_str()).spawn() {
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

    let tailnet = build_tailnet_identity(&raw);

    let peers = raw
        .peer
        .unwrap_or_default()
        .into_values()
        .map(|peer| TailscalePeer {
            dns_name: peer.dns_name.clone().unwrap_or_default(),
            tailscale_ips: peer
                .tailscale_ips
                .iter()
                .filter_map(|value| value.parse::<Ipv4Addr>().ok())
                .filter(|&ip| is_usable_tailscale_ipv4(ip))
                .collect(),
            online: peer.online.unwrap_or(false),
            path: classify_path(peer.cur_addr.as_deref(), peer.relay.as_deref(), peer.online),
            dns_suffix: peer_dns_suffix(peer.dns_name.as_deref().unwrap_or_default()),
            shared_in: peer.sharee_node.unwrap_or(false),
            key_expiry: peer.key_expiry,
        })
        .collect();

    Ok(TailscaleStatus {
        backend_state: raw.backend_state,
        local_ipv4,
        device_name,
        peers,
        tailnet,
    })
}

/// Assembles the tailnet identity from the status document. Present on
/// `tailscale status --json` since at least 1.36; every field is individually
/// optional because a stopped or key-only backend omits blocks.
fn build_tailnet_identity(raw: &RawStatus) -> Option<TailnetIdentity> {
    let self_node = raw.self_node.as_ref()?;
    let role = role_from_caps(self_node.cap_map.as_ref());
    // The `User` block only tells us the node has a real signed-in user behind
    // it; the login name itself is never read into the struct.
    let has_user_record = self_node
        .user_id
        .and_then(|id| raw.users.as_ref().and_then(|u| u.get(&id.to_string())))
        .is_some();
    let tailnet = raw.current_tailnet.as_ref();
    let login_url_pending = raw
        .auth_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    // When the CLI reported nothing about the network or this node's standing
    // in it, say "unknown" rather than inventing an identity.
    if role == TailnetRole::Unknown
        && tailnet.is_none()
        && !has_user_record
        && !login_url_pending
        && raw.have_node_key.is_none()
        && self_node.key_expiry.is_none()
    {
        return None;
    }
    let reported_suffix = tailnet.and_then(|t| t.magic_dns_suffix.clone());
    let own_suffix_from_dns = self_node
        .dns_name
        .as_deref()
        .map(|dns| peer_dns_suffix(dns).trim_end_matches('.').to_string());
    Some(TailnetIdentity {
        stable_id: tailnet.and_then(|t| t.stable_id.clone()),
        magic_dns_suffix: reported_suffix
            .or(own_suffix_from_dns)
            .filter(|s| !s.is_empty()),
        role,
        key_expiry: self_node.key_expiry.clone(),
        have_node_key: raw.have_node_key,
        login_url_pending,
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
    #[serde(rename = "CurrentTailnet")]
    current_tailnet: Option<RawTailnet>,
    /// Keyed by user id. Only the presence of a record is used (the login name
    /// inside it is an email address Movie Party never reads).
    #[serde(rename = "User")]
    users: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    #[serde(rename = "AuthURL")]
    auth_url: Option<String>,
    #[serde(rename = "HaveNodeKey")]
    have_node_key: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawTailnet {
    #[serde(rename = "StableID")]
    stable_id: Option<String>,
    #[serde(rename = "MagicDNSSuffix")]
    magic_dns_suffix: Option<String>,
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
    #[serde(rename = "UserID")]
    user_id: Option<i64>,
    #[serde(rename = "KeyExpiry")]
    key_expiry: Option<String>,
    #[serde(rename = "ShareeNode")]
    sharee_node: Option<bool>,
    /// The capability buckets granted to this node. A key-only/older CLI omits
    /// it, which is why `TailnetRole::Unknown` exists.
    #[serde(rename = "CapMap")]
    cap_map: Option<std::collections::BTreeMap<String, serde_json::Value>>,
}

/// Capability strings the Tailscale CLI uses for tailnet administration.
const CAP_IS_OWNER: &str = "https://tailscale.com/cap/is-owner";
const CAP_IS_ADMIN: &str = "https://tailscale.com/cap/is-admin";
const CAP_TAILNET_LOCK: &str = "https://tailscale.com/cap/tailnet-lock";
const CAP_NETWORK_FLOW: &str = "https://tailscale.com/cap/netflow";
const CAP_DEBUG: &str = "https://tailscale.com/cap/debug";

/// Owner/admin/LimitedUser from the node's capability buckets. Tailnet-lock,
/// netflow and debug are admin-only capabilities, so they let Movie Party
/// recognise an administrator even when the coarse `is-admin` bucket is absent.
fn role_from_caps(
    cap_map: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
) -> TailnetRole {
    let Some(map) = cap_map else {
        return TailnetRole::Unknown;
    };
    if map.contains_key(CAP_IS_OWNER) {
        return TailnetRole::Owner;
    }
    if map.contains_key(CAP_IS_ADMIN)
        || map.contains_key(CAP_TAILNET_LOCK)
        || map.contains_key(CAP_NETWORK_FLOW)
        || map.contains_key(CAP_DEBUG)
    {
        return TailnetRole::Admin;
    }
    TailnetRole::LimitedUser
}

#[cfg(test)]
mod tests {
    use super::{
        friend_candidates, is_allowed_party_ipv4, is_usable_tailscale_ipv4,
        is_wrapper_error_stdout, parse_ping_stdout, parse_status_json, peer_display_name,
        peer_dns_suffix, peer_is_same_tailnet, readiness_from_error, readiness_from_status,
        required_ipv4, rfc3339_utc_to_ms, usable_peers, FriendCandidate, NodeKeyState,
        PeerConnectionProbe, TailnetRole, TailscaleError, TailscalePath, TailscalePeer,
        TailscaleState, NODE_KEY_WARNING_DAYS,
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
    fn open_failed_maps_to_daemon_unavailable() {
        let readiness = readiness_from_error(TailscaleError::OpenFailed(
            "application not found".to_string(),
        ));
        assert_eq!(readiness.state, TailscaleState::DaemonUnavailable);
        assert_eq!(readiness.code.as_deref(), Some("MP-NET-TS-003"));
    }

    #[test]
    fn open_failed_displays_stable_mp_net_ts_code() {
        let error = TailscaleError::OpenFailed("application not found".to_string());
        let text = error.to_string();
        assert!(
            text.starts_with("MP-NET-TS-003"),
            "Open Tailscale failures must surface a stable MP-NET-TS code, got: {text}"
        );
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
            dns_suffix: "tailnet.ts.net".to_string(),
            shared_in: false,
            key_expiry: None,
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
            dns_suffix: "tailnet.ts.net".to_string(),
            shared_in: false,
            key_expiry: None,
        };
        let offline = TailscalePeer {
            dns_name: "b".to_string(),
            tailscale_ips: vec!["100.64.0.11".parse().unwrap()],
            online: false,
            path: TailscalePath::Offline,
            dns_suffix: "tailnet.ts.net".to_string(),
            shared_in: false,
            key_expiry: None,
        };
        let lan_only = TailscalePeer {
            dns_name: "c".to_string(),
            tailscale_ips: vec!["192.168.1.12".parse().unwrap()],
            online: true,
            path: TailscalePath::Direct,
            dns_suffix: "tailnet.ts.net".to_string(),
            shared_in: false,
            key_expiry: None,
        };
        let all = [online.clone(), offline, lan_only];
        let result = usable_peers(&all);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].dns_name, "a");
    }

    // ── GUI-env TERM fix ────────────────────────────────────────────

    #[test]
    fn wrapper_error_stdout_is_detected_even_with_exit_zero_shape() {
        // Exact banner reproduced from a GUI-launched Movie Party spawning
        // the macOS Tailscale CLI (1.102.4) with no TERM in the environ.
        let stdout = "The Tailscale GUI failed to start: The operation couldn\u{2019}t be completed. (Tailscale.CLIError error 3.)\n";
        assert!(is_wrapper_error_stdout(stdout.as_bytes()));
        // JSON status documents must never be classified as wrapper errors.
        assert!(!is_wrapper_error_stdout(br#"{"BackendState":"Running"}"#));
        assert!(!is_wrapper_error_stdout(b""));
        assert!(!is_wrapper_error_stdout(
            b"failed to connect: permission denied"
        ));
    }

    #[test]
    fn tailscaled_socket_failure_is_detected() {
        // Linux/standalone-daemon flavor of the same lie (exit 0, banner).
        assert!(is_wrapper_error_stdout(
            b"failed to connect to tailscaled: dial unix /var/run/tailscaled.socket: connect: no such file or directory"
        ));
    }

    // ── Add Friend: candidate mapping ───────────────────────────────

    #[test]
    fn peer_display_name_strips_magicdns_suffix() {
        assert_eq!(
            peer_display_name("rahul-mac.tailc930b7.ts.net."),
            "rahul-mac"
        );
        assert_eq!(peer_display_name("pc.tailnet.ts.net."), "pc");
        // No MagicDNS suffix → returned without the trailing dot only.
        assert_eq!(peer_display_name("plain-host."), "plain-host");
        assert_eq!(peer_display_name(""), "");
    }

    #[test]
    fn friend_candidates_online_first_with_display_names() {
        let status = parse_status_json(
            br#"{
              "BackendState": "Running",
              "Self": {"TailscaleIPs": ["100.64.0.10"]},
              "Peer": {
                "off": {
                  "DNSName": "zed.tailc930b7.ts.net.",
                  "TailscaleIPs": ["100.64.0.3"],
                  "Online": false
                },
                "on": {
                  "DNSName": "rahul-mac.tailc930b7.ts.net.",
                  "TailscaleIPs": ["100.64.0.42"],
                  "Online": true,
                  "CurAddr": "100.64.0.42:47821"
                },
                "lan": {
                  "DNSName": "lan-only.tailc930b7.ts.net.",
                  "TailscaleIPs": ["192.168.1.9"],
                  "Online": true
                },
                "unnamed": {
                  "DNSName": "",
                  "TailscaleIPs": ["100.64.0.7"],
                  "Online": true
                }
              }
            }"#,
        )
        .expect("valid status");

        let candidates = friend_candidates(&status);
        // lan-only (no CGNAT) and unnamed peers are excluded.
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].display_name, "rahul-mac");
        assert!(candidates[0].online);
        assert_eq!(candidates[0].ip.as_deref(), Some("100.64.0.42"));
        assert_eq!(candidates[0].path, "direct");
        assert_eq!(candidates[0].peer_key, "rahul-mac.tailc930b7.ts.net.");
        assert!(!candidates[1].online);
        assert_eq!(candidates[1].display_name, "zed");
    }

    // ── Add Friend: real ping probe parsing ────────────────────────

    #[test]
    fn parse_ping_stdout_direct_pong() {
        let probe = parse_ping_stdout(
            "pong from rahul-mac.tailc930b7.ts.net. (100.64.0.42) via direct/ipv4 192.168.1.5:41641 in 23ms\n",
        )
        .expect("parsed");
        assert!(probe.reachable);
        assert_eq!(probe.path.as_deref(), Some("direct/ipv4 192.168.1.5:41641"));
        assert_eq!(probe.latency_ms, Some(23));
    }

    #[test]
    fn parse_ping_stdout_relay_pong() {
        let probe =
            parse_ping_stdout("pong from host (100.64.0.42) via relay \"derp-3\" in 118ms\n")
                .expect("parsed");
        assert!(probe.reachable);
        assert_eq!(probe.path.as_deref(), Some("relay \"derp-3\""));
        assert_eq!(probe.latency_ms, Some(118));
    }

    #[test]
    fn parse_ping_stdout_no_reply_is_failure() {
        let probe = parse_ping_stdout("ping \"100.64.0.42\": timed out\n2026/09/12 no reply\n")
            .expect("parsed");
        assert!(!probe.reachable);
        assert!(probe.message.starts_with("MP-NET-TS-007"));
    }

    #[test]
    fn parse_ping_stdout_empty_is_none() {
        assert!(parse_ping_stdout("\n \n").is_none());
    }

    #[test]
    fn parse_ping_stdout_ignores_leading_notices() {
        let probe = parse_ping_stdout(
            "passthrough mode engaged\npong from pc (100.64.0.9) via direct in 12ms\n",
        )
        .expect("parsed");
        assert!(probe.reachable);
        assert_eq!(probe.latency_ms, Some(12));
    }

    #[test]
    fn probe_fail_shape_is_stable() {
        let probe = PeerConnectionProbe::fail("MP-NET-TS-007 down".to_string());
        assert!(!probe.reachable);
        assert!(probe.path.is_none());
        assert!(probe.latency_ms.is_none());
        let ok = PeerConnectionProbe::ok("direct".to_string(), 40);
        assert!(ok.reachable);
        assert_eq!(ok.path.as_deref(), Some("direct"));
        assert_eq!(ok.latency_ms, Some(40));
    }

    #[test]
    fn friend_candidate_sorts_offline_last() {
        // The type round-trips through serde for the IPC boundary; verify
        // camelCase field names survive so the frontend contract holds.
        let candidate = FriendCandidate {
            peer_key: "k".to_string(),
            display_name: "d".to_string(),
            ip: Some("100.64.0.1".to_string()),
            online: true,
            path: "direct".to_string(),
            same_tailnet: Some(false),
        };
        let json = serde_json::to_string(&candidate).expect("serializes");
        assert!(json.contains("\"peerKey\""));
        assert!(json.contains("\"displayName\""));
        assert!(json.contains("\"sameTailnet\":false"));
        let back: FriendCandidate = serde_json::from_str(&json).expect("round-trips");
        assert_eq!(back, candidate);
    }

    // ── Batch 9: tailnet identity, role, key expiry, topology ────────

    /// Shape captured from a real `tailscale status --json` on macOS
    /// (Tailscale 1.102.x), reduced to the blocks Batch 9 reads.
    const REAL_SHAPE: &str = r#"{
      "Version": "1.102.4",
      "BackendState": "Running",
      "AuthURL": "",
      "HaveNodeKey": true,
      "MagicDNSSuffix": "tailc930b7.ts.net",
      "CurrentTailnet": {
        "Name": "someone@example.com",
        "StableID": "TnSLAyoyHo11CNTRL",
        "MagicDNSSuffix": "tailc930b7.ts.net",
        "MagicDNSEnabled": true
      },
      "User": {
        "7429568193493714": {"ID": 7429568193493714, "LoginName": "someone@example.com"}
      },
      "Self": {
        "ID": "nodekey:1111111111111111",
        "DNSName": "some-mac.tailc930b7.ts.net.",
        "HostName": "some-mac",
        "UserID": 7429568193493714,
        "TailscaleIPs": ["100.64.0.10"],
        "Online": true,
        "KeyExpiry": "2027-02-20T20:43:23Z",
        "CapMap": {
          "https://tailscale.com/cap/is-owner": [[]],
          "https://tailscale.com/cap/file-sharing": [[]]
        }
      },
      "Peer": {
        "nodekey:2222222222222222": {
          "DNSName": "rahul-mac.tailc930b7.ts.net.",
          "TailscaleIPs": ["100.64.0.42"],
          "Online": true,
          "CurAddr": "100.64.0.42:41641",
          "UserID": 7429568193493714
        },
        "nodekey:3333333333333333": {
          "DNSName": "priya-win.tail99aa00.ts.net.",
          "TailscaleIPs": ["100.64.0.77"],
          "Online": true,
          "UserID": 5000000000000000,
          "ShareeNode": true
        }
      }
    }"#;

    #[test]
    fn real_shape_yields_tailnet_identity_and_owner_role() {
        let status = parse_status_json(REAL_SHAPE.as_bytes()).expect("real shape parses");
        let tailnet = status.tailnet.clone().expect("tailnet identity present");
        assert_eq!(tailnet.stable_id.as_deref(), Some("TnSLAyoyHo11CNTRL"));
        assert_eq!(
            tailnet.magic_dns_suffix.as_deref(),
            Some("tailc930b7.ts.net")
        );
        assert_eq!(tailnet.role, TailnetRole::Owner);
        assert!(tailnet.role.can_administer());
        assert_eq!(tailnet.key_expiry.as_deref(), Some("2027-02-20T20:43:23Z"));
        assert_eq!(tailnet.have_node_key, Some(true));
        assert!(!tailnet.login_url_pending);
    }

    #[test]
    fn tailnet_identity_never_carries_the_owner_login_name() {
        // `CurrentTailnet.Name` is normally an email address. Movie Party has no
        // need for it, and the wizard renders this struct — so it must not be
        // reachable through the IPC payload at all.
        let status = parse_status_json(REAL_SHAPE.as_bytes()).expect("real shape parses");
        let json = serde_json::to_string(&status.tailnet.expect("identity")).expect("serializes");
        assert!(
            !json.contains("example.com"),
            "tailnet identity leaked a login name: {json}"
        );
    }

    #[test]
    fn role_distinguishes_admin_limited_user_and_unknown() {
        // Built with the JSON macro so the capability buckets are data, not
        // brace-escaped string literals.
        let parse_with_caps = |caps: serde_json::Value| {
            let doc = serde_json::json!({
                "BackendState": "Running",
                "CurrentTailnet": { "StableID": "Tn1", "MagicDNSSuffix": "t.ts.net" },
                "Self": {
                    "DNSName": "a.t.ts.net.",
                    "TailscaleIPs": ["100.64.0.1"],
                    "CapMap": caps
                }
            });
            parse_status_json(serde_json::to_vec(&doc).expect("json").as_slice())
                .expect("parses")
                .tailnet
                .expect("identity from CurrentTailnet")
        };
        assert_eq!(
            parse_with_caps(serde_json::json!({ "https://tailscale.com/cap/is-admin": [[]] })).role,
            TailnetRole::Admin
        );
        // Tailnet-lock and netflow are admin-only, so they identify an
        // administrator even without the coarse is-admin bucket.
        assert_eq!(
            parse_with_caps(serde_json::json!({ "https://tailscale.com/cap/tailnet-lock": [[]] }))
                .role,
            TailnetRole::Admin
        );
        let limited =
            parse_with_caps(serde_json::json!({ "https://tailscale.com/cap/file-sharing": [[]] }));
        assert_eq!(limited.role, TailnetRole::LimitedUser);
        assert!(!limited.role.can_administer());
        let empty = parse_with_caps(serde_json::json!({}));
        assert_eq!(empty.role, TailnetRole::LimitedUser);
    }

    #[test]
    fn missing_capmap_is_unknown_role_not_limited_user() {
        // A CLI that reports no capability buckets must not be told it is
        // "just a limited user" — that would hide the admin path from someone
        // who can in fact share.
        let status = parse_status_json(
            br#"{"BackendState":"Running","CurrentTailnet":{"StableID":"Tn1","MagicDNSSuffix":"t.ts.net"},
                 "Self":{"DNSName":"a.t.ts.net.","TailscaleIPs":["100.64.0.1"]}}"#,
        )
        .expect("parses");
        let tailnet = status.tailnet.expect("identity from CurrentTailnet");
        assert_eq!(tailnet.role, TailnetRole::Unknown);
        assert!(!tailnet.role.can_administer());
    }

    #[test]
    fn login_url_present_means_backend_wants_interactive_signin() {
        let status = parse_status_json(
            br#"{"BackendState":"NeedsLogin","AuthURL":"https://login.tailscale.com/a/xyz",
                 "Self":{"DNSName":"a.t.ts.net.","TailscaleIPs":[]}}"#,
        )
        .expect("parses");
        assert!(status.tailnet.expect("identity").login_url_pending);
    }

    #[test]
    fn peer_on_another_tailnet_is_detected_by_dns_suffix() {
        let status = parse_status_json(REAL_SHAPE.as_bytes()).expect("real shape parses");
        let own = status
            .peers
            .iter()
            .find(|p| p.dns_name.starts_with("rahul-mac"))
            .expect("same-tailnet peer");
        let shared = status
            .peers
            .iter()
            .find(|p| p.dns_name.starts_with("priya-win"))
            .expect("shared-in peer");
        assert_eq!(peer_is_same_tailnet(&status, own), Some(true));
        assert_eq!(peer_is_same_tailnet(&status, shared), Some(false));
        assert!(shared.shared_in);
        assert_eq!(shared.dns_suffix, "tail99aa00.ts.net");
        assert_eq!(own.dns_suffix, "tailc930b7.ts.net");
    }

    #[test]
    fn topology_is_unknown_when_the_cli_reported_no_suffix() {
        // An older CLI without CurrentTailnet and a peer without a suffixable
        // DNS name must yield `None` ("cannot tell"), never a confident
        // "same tailnet".
        let status = parse_status_json(
            br#"{"BackendState":"Running",
                 "Self":{"DNSName":"plain","TailscaleIPs":["100.64.0.1"]},
                 "Peer":{"k":{"DNSName":"other.t.ts.net.","TailscaleIPs":["100.64.0.2"],"Online":true}}}"#,
        )
        .expect("parses");
        assert!(status.tailnet.is_none());
        assert_eq!(peer_is_same_tailnet(&status, &status.peers[0]), None);
    }

    #[test]
    fn shared_in_flag_overrides_a_matching_suffix() {
        // If the CLI explicitly flags a node as shared in, believe it even when
        // the suffix happens to match (renamed/legacy tailnet suffixes).
        let mut status = parse_status_json(REAL_SHAPE.as_bytes()).expect("real shape parses");
        let index = status
            .peers
            .iter()
            .position(|p| p.dns_name.starts_with("rahul-mac"))
            .expect("peer");
        status.peers[index].shared_in = true;
        assert_eq!(
            peer_is_same_tailnet(&status, &status.peers[index]),
            Some(false)
        );
    }

    #[test]
    fn peer_dns_suffix_splits_the_host_label_off() {
        assert_eq!(
            peer_dns_suffix("rahul-mac.tailc930b7.ts.net."),
            "tailc930b7.ts.net"
        );
        assert_eq!(
            peer_dns_suffix("rahul-mac.tailnet.ts.net"),
            "tailnet.ts.net"
        );
        // A bare host name has no tailnet part to compare.
        assert_eq!(peer_dns_suffix("plain-host"), "");
        assert_eq!(peer_dns_suffix(""), "");
        assert_eq!(peer_dns_suffix("."), "");
    }

    #[test]
    fn friend_candidates_expose_per_peer_topology() {
        let status = parse_status_json(REAL_SHAPE.as_bytes()).expect("real shape parses");
        let candidates = friend_candidates(&status);
        assert_eq!(candidates.len(), 2);
        let rahul = candidates
            .iter()
            .find(|c| c.display_name == "rahul-mac")
            .expect("candidate");
        let priya = candidates
            .iter()
            .find(|c| c.display_name == "priya-win")
            .expect("candidate");
        assert_eq!(rahul.same_tailnet, Some(true));
        assert_eq!(priya.same_tailnet, Some(false));
    }

    #[test]
    fn rfc3339_utc_to_ms_matches_known_epochs() {
        assert_eq!(rfc3339_utc_to_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            rfc3339_utc_to_ms("2027-02-20T20:43:23Z"),
            Some(1_803_156_203_000)
        );
        assert_eq!(
            rfc3339_utc_to_ms("2026-01-01T00:00:00Z"),
            Some(1_767_225_600_000)
        );
        assert_eq!(
            rfc3339_utc_to_ms("2000-02-29T12:00:00Z"),
            Some(951_825_600_000)
        );
        // 2024 is a leap year; 2100 is not (divisible by 100, not 400).
        assert_eq!(
            rfc3339_utc_to_ms("2024-02-29T00:00:00Z"),
            Some(1_709_164_800_000)
        );
        assert_eq!(
            rfc3339_utc_to_ms("2024-03-01T00:00:00Z"),
            Some(1_709_251_200_000)
        );
        assert_eq!(
            rfc3339_utc_to_ms("2100-03-01T00:00:00Z"),
            Some(4_107_542_400_000)
        );
    }

    #[test]
    fn rfc3339_utc_to_ms_rejects_anything_it_cannot_trust() {
        for bad in [
            "",
            "2027-02-20",
            "2027-02-20T20:43:23",
            "2027-02-20T20:43:23+05:30",
            "2027-02-30T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2027-13-01T00:00:00Z",
            "2027-00-10T00:00:00Z",
            "2027-01-00T00:00:00Z",
            "not a date",
        ] {
            assert_eq!(rfc3339_utc_to_ms(bad), None, "must be rejected: {bad:?}");
        }
        // 2024 and 2000 are leap years; the day must be accepted.
        assert!(rfc3339_utc_to_ms("2024-02-29T00:00:00Z").is_some());
        assert!(rfc3339_utc_to_ms("2000-02-29T00:00:00Z").is_some());
    }

    #[test]
    fn node_key_state_classifies_expiry() {
        let now = 1_767_225_600_000_i64; // 2026-01-01T00:00:00Z
        let day = 86_400_000_i64;
        assert_eq!(NodeKeyState::from_expiry(None, now), NodeKeyState::Unknown);
        assert_eq!(
            NodeKeyState::from_expiry(Some("nonsense"), now),
            NodeKeyState::Unknown
        );
        assert_eq!(
            NodeKeyState::from_expiry(Some("2025-12-31T00:00:00Z"), now),
            NodeKeyState::Expired
        );
        assert_eq!(
            NodeKeyState::from_expiry(Some("2026-01-15T00:00:00Z"), now),
            NodeKeyState::ExpiringSoon
        );
        assert_eq!(
            NodeKeyState::from_expiry(Some("2027-01-01T00:00:00Z"), now),
            NodeKeyState::Valid
        );
        // Exactly the warning horizon counts as "soon", one day past it does not.
        assert_eq!(
            NodeKeyState::from_expiry(Some(&rfc3339_add_ms(now, NODE_KEY_WARNING_DAYS * day)), now),
            NodeKeyState::ExpiringSoon
        );
        assert_eq!(
            NodeKeyState::from_expiry(
                Some(&rfc3339_add_ms(now, (NODE_KEY_WARNING_DAYS + 1) * day)),
                now
            ),
            NodeKeyState::Valid
        );
        assert!(NodeKeyState::Expired.is_blocking());
        assert!(!NodeKeyState::ExpiringSoon.is_blocking());
        assert!(NodeKeyState::ExpiringSoon.warns());
        assert!(!NodeKeyState::Valid.warns());
        assert!(!NodeKeyState::Unknown.warns());
    }

    /// Test helper: format unix ms back into the UTC form Tailscale reports.
    fn rfc3339_add_ms(base_ms: i64, add_ms: i64) -> String {
        let total = base_ms + add_ms;
        let days = total.div_euclid(86_400_000);
        let secs_of_day = total.rem_euclid(86_400_000) / 1_000;
        let (y, m, d) = civil_from_days(days);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
            secs_of_day / 3600,
            (secs_of_day % 3600) / 60,
            secs_of_day % 60
        )
    }

    /// Inverse of `days_from_civil`, for building expiry strings in tests.
    fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
        let z = days_since_epoch + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (if m <= 2 { y + 1 } else { y }, m, d)
    }

    #[test]
    fn civil_round_trip_agrees_with_known_epochs() {
        // The inverse helper must agree with the forward one, otherwise the
        // boundary assertions above are testing a different calendar.
        for ms in [
            0_i64,
            1_709_164_800_000,
            1_709_251_200_000,
            4_107_542_400_000,
        ] {
            let text = rfc3339_add_ms(0, ms);
            assert_eq!(rfc3339_utc_to_ms(&text), Some(ms), "round trip of {ms}");
        }
    }
}
