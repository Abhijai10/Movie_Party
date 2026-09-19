mod process;

use std::{
    fs,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use thiserror::Error;

use process::ChildPoll;
pub use process::{install_shutdown_hardening, ChromeProcessOwner};

/// How long a freshly launched Chrome gets to open its CDP endpoint before the
/// launch is declared failed. See [`launch_managed_chrome`].
const CDP_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

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
    #[error("MP-PROVIDER-003 Chrome process failed: {0}")]
    Process(String),
    #[error("MP-PROVIDER-003 CDP IO failed: {0}")]
    Io(String),
    #[error("MP-PROVIDER-003 CDP response was malformed")]
    MalformedCdpResponse,
    #[error("MP-PROVIDER-003 CDP timed out")]
    CdpTimeout,
    #[error("MP-PROVIDER-003 CDP command failed: {0}")]
    CdpCommand(String),
    #[error("MP-PROVIDER-004 invalid CDP port")]
    InvalidCdpPort,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpTarget {
    pub id: String,
    pub target_type: String,
    pub url: String,
    pub title: String,
    pub web_socket_debugger_url: String,
}

/// A live managed Chrome, owned end to end.
///
/// The browser is never held as a bare `Child`: [`ChromeProcessOwner`] owns the
/// whole process tree (browser + renderer/GPU/utility children) and is the only
/// thing allowed to signal or reap it. See `process.rs` for why that matters.
#[derive(Debug)]
pub struct ManagedChromeSession {
    process: ChromeProcessOwner,
    pub plan: ChromeLaunchPlan,
}

#[derive(Debug)]
pub struct CdpPageSession {
    stream: std::net::TcpStream,
    next_id: u64,
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
    if cdp_port == 0 {
        return Err(ManagedChromeError::InvalidCdpPort);
    }
    let profile_path = provider_profile_path(profiles_root, provider_id)?;

    Ok(ChromeLaunchPlan {
        executable,
        profile_path,
        cdp_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        cdp_port,
        url: url.to_owned(),
    })
}

pub fn allocate_local_cdp_port() -> Result<u16, ManagedChromeError> {
    let listener = std::net::TcpListener::bind((CDP_BIND_HOST, 0))
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|error| ManagedChromeError::Io(error.to_string()))
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

/// Launches a managed Chrome and waits for its CDP endpoint to come up.
///
/// **Every** failure after the spawn kills and reaps the tree (MP-20).
/// Previously the `Child` was dropped bare on the CDP-timeout and child-exit
/// paths, which leaked a running Chrome *and* left an unreaped zombie — because
/// `ManagedChromeSession`'s `Drop` never runs for a session that was never
/// constructed.
pub fn launch_managed_chrome(
    plan: ChromeLaunchPlan,
) -> Result<ManagedChromeSession, ManagedChromeError> {
    launch_managed_chrome_with_timeout(plan, CDP_STARTUP_TIMEOUT)
}

/// [`launch_managed_chrome`] with an explicit CDP startup budget, so the
/// failure paths can be tested without waiting the full production timeout.
pub fn launch_managed_chrome_with_timeout(
    plan: ChromeLaunchPlan,
    cdp_startup_timeout: Duration,
) -> Result<ManagedChromeSession, ManagedChromeError> {
    validate_cdp_bind(plan.cdp_host)?;
    fs::create_dir_all(&plan.profile_path)
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;

    // The owner takes responsibility the instant the process exists, so no
    // later `?` can drop a live tree on the floor. It also publishes the tree
    // for the signal/panic shutdown paths.
    let mut process = ChromeProcessOwner::spawn(&plan)?;

    if let Err(error) = wait_for_cdp_or_child_exit(&mut process, plan.cdp_port, cdp_startup_timeout)
    {
        // This launch owns the tree and nothing else has ever seen it, so it is
        // killed and reaped right here — the session that would have taken
        // ownership does not exist, so its `Drop` will never run.
        process.terminate_tree();
        return Err(error);
    }

    Ok(ManagedChromeSession { process, plan })
}

impl ManagedChromeSession {
    pub fn targets(&self) -> Result<Vec<CdpTarget>, ManagedChromeError> {
        fetch_targets(self.plan.cdp_port)
    }

    pub fn page_target(&self) -> Result<CdpTarget, ManagedChromeError> {
        self.targets()?
            .into_iter()
            .find(|target| {
                target.target_type == "page" && !target.web_socket_debugger_url.is_empty()
            })
            .ok_or(ManagedChromeError::MalformedCdpResponse)
    }

    pub fn connect_page(&self) -> Result<CdpPageSession, ManagedChromeError> {
        let target = self.page_target()?;
        CdpPageSession::connect(&target.web_socket_debugger_url)
    }

    /// Graceful shutdown in place: asks Chrome to close itself over CDP
    /// (`Browser.close` lets Chrome flush the dedicated profile — a hard kill
    /// can leave it locked and make the NEXT launch of that provider fail or
    /// show a "restore pages" banner), waits up to `GRACEFUL_CLOSE_TIMEOUT` for
    /// the browser to exit, then hands the whole tree to the owner, which
    /// guarantees a terminate **and a reap**.
    ///
    /// Never returns Err: teardown must not abort the caller's cleanup path.
    /// Safe to call repeatedly — the owner's teardown is idempotent.
    pub fn close_gracefully(&mut self) {
        if self.process.is_alive() {
            if let Ok(mut page) = self.connect_page() {
                let id = page.next_command_id();
                let _ = page.execute(&close_browser_command(id));
            }
            let start = Instant::now();
            while start.elapsed() < GRACEFUL_CLOSE_TIMEOUT {
                if !self.process.is_alive() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        // Unconditional, including when the browser had already exited: the
        // browser exiting does not mean its renderer/GPU children exited, and
        // those are exactly what a single-PID kill leaked (MP-22).
        self.process.terminate_tree();
    }

    pub fn close(mut self) -> Result<(), ManagedChromeError> {
        self.close_gracefully();
        Ok(())
    }
}

/// How long to wait for Chrome to exit on its own after `Browser.close`
/// before falling back to kill(). Generous: Chrome flushes the profile
/// on exit, and SIGKILL is what we are trying to avoid.
const GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

fn chrome_command(plan: &ChromeLaunchPlan) -> Command {
    let mut command = Command::new(&plan.executable);
    command.args(plan.args());
    configure_process_isolation(&mut command);
    command
}

/// Puts the browser in its own process group / job, so teardown can address the
/// whole Chrome tree and nothing else (MP-22).
///
/// POSIX: a new process group whose id is the child's pid. That is what makes
/// `killpg` reach every descendant without any risk of reaching a process Movie
/// Party does not own.
///
/// Windows: a new process group, so a console Ctrl+C never reaches Chrome, plus
/// — assigned at spawn time by `ChromeProcessOwner` — a kill-on-close job
/// object, which is the part that actually guarantees tree termination.
fn configure_process_isolation(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        command.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
}

impl ManagedChromeSession {
    /// Check if the Chrome process is still alive.
    pub fn is_alive(&mut self) -> bool {
        self.process.is_alive()
    }

    /// Full health check: verify Chrome is alive and CDP is responsive.
    pub fn health_check(&mut self) -> Result<(), ManagedChromeError> {
        if !self.is_alive() {
            return Err(ManagedChromeError::Process(
                "Chrome process has exited".to_string(),
            ));
        }
        // Verify CDP endpoint is reachable
        fetch_targets(self.plan.cdp_port)?;
        Ok(())
    }
}

impl Drop for ManagedChromeSession {
    fn drop(&mut self) {
        // Graceful by default: Browser.close + wait, kill only as the
        // fallback. A SIGKILL here is what left provider profiles in a
        // dirty state (locked/dirty profile → next launch misbehaves).
        self.close_gracefully();
    }
}

impl CdpPageSession {
    fn connect(web_socket_url: &str) -> Result<Self, ManagedChromeError> {
        let (host, port, path) = parse_ws_url(web_socket_url)?;
        if host != CDP_BIND_HOST && host != "localhost" {
            return Err(ManagedChromeError::NonLocalCdpBind);
        }

        let mut stream = std::net::TcpStream::connect((host.as_str(), port))
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(20)))
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        let key = STANDARD.encode(*b"MoviePartyCdpKey!");
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        let response = read_http_response(&mut stream)?;
        if !response.starts_with("HTTP/1.1 101") {
            return Err(ManagedChromeError::MalformedCdpResponse);
        }

        Ok(Self { stream, next_id: 1 })
    }

    pub fn navigate(&mut self, url: &str) -> Result<serde_json::Value, ManagedChromeError> {
        let id = self.next_command_id();
        self.execute(&navigate_command(id, url))
    }

    pub fn evaluate(&mut self, expression: &str) -> Result<serde_json::Value, ManagedChromeError> {
        let id = self.next_command_id();
        self.execute(&CdpCommand {
            id,
            method: "Runtime.evaluate",
            params: json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": true
            }),
        })
    }

    pub fn execute(
        &mut self,
        command: &CdpCommand,
    ) -> Result<serde_json::Value, ManagedChromeError> {
        write_ws_text(
            &mut self.stream,
            &serde_json::to_string(command)
                .map_err(|error| ManagedChromeError::Io(error.to_string()))?,
        )?;
        loop {
            let message = read_ws_text(&mut self.stream)?;
            let value: serde_json::Value = serde_json::from_str(&message)
                .map_err(|_| ManagedChromeError::MalformedCdpResponse)?;
            if value["id"].as_u64() == Some(command.id) {
                if let Some(error) = value.get("error") {
                    return Err(ManagedChromeError::CdpCommand(error.to_string()));
                }
                return Ok(value["result"].clone());
            }
        }
    }

    pub(crate) fn next_command_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
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

/// Waits for the CDP endpoint to come up, failing fast when the Chrome
/// child process exits first (macOS Chrome redirect behavior, a locked
/// profile, or a bad executable can make the child exit immediately —
/// polling for the full timeout after that only produces a misleading
/// CdpTimeout error 10 seconds later).
fn wait_for_cdp_or_child_exit(
    process: &mut ChromeProcessOwner,
    port: u16,
    timeout: Duration,
) -> Result<(), ManagedChromeError> {
    let start = Instant::now();
    loop {
        if http_get(port, "/json/version").is_ok() {
            return Ok(());
        }
        match process.poll() {
            // Chrome exited before CDP came up: surface the real cause, not a
            // timeout. This is the "Chrome exits unexpectedly" launch-time
            // symptom.
            ChildPoll::Exited(status) => {
                return Err(ManagedChromeError::Process(format!(
                    "Chrome exited during launch with status {status}"
                )));
            }
            ChildPoll::Running => {}
            ChildPoll::Failed(error) => {
                return Err(ManagedChromeError::Io(error));
            }
        }
        if start.elapsed() >= timeout {
            return Err(ManagedChromeError::CdpTimeout);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn fetch_targets(port: u16) -> Result<Vec<CdpTarget>, ManagedChromeError> {
    let body = http_get(port, "/json/list")?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| ManagedChromeError::MalformedCdpResponse)?;
    let targets = value
        .as_array()
        .ok_or(ManagedChromeError::MalformedCdpResponse)?
        .iter()
        .filter_map(|target| {
            Some(CdpTarget {
                id: target.get("id")?.as_str()?.to_string(),
                target_type: target.get("type")?.as_str()?.to_string(),
                url: target
                    .get("url")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                title: target
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                web_socket_debugger_url: target
                    .get("webSocketDebuggerUrl")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect();
    Ok(targets)
}

fn http_get(port: u16, path: &str) -> Result<String, ManagedChromeError> {
    let mut stream = std::net::TcpStream::connect((CDP_BIND_HOST, port))
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {CDP_BIND_HOST}:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    let headers = read_http_response(&mut stream)?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or(ManagedChromeError::MalformedCdpResponse)?;
    let mut body = vec![0; content_length];
    stream
        .read_exact(&mut body)
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    String::from_utf8(body).map_err(|_| ManagedChromeError::MalformedCdpResponse)
}

fn read_http_response(stream: &mut std::net::TcpStream) -> Result<String, ManagedChromeError> {
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        response.push(byte[0]);
        if response.len() > 16 * 1024 {
            return Err(ManagedChromeError::MalformedCdpResponse);
        }
    }
    String::from_utf8(response).map_err(|_| ManagedChromeError::MalformedCdpResponse)
}

fn write_ws_text(stream: &mut std::net::TcpStream, text: &str) -> Result<(), ManagedChromeError> {
    let payload = text.as_bytes();
    let mut frame = Vec::with_capacity(payload.len() + 16);
    frame.push(0x81);
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    let mask = [0x4d, 0x50, 0x43, 0x4b];
    frame.extend_from_slice(&mask);
    for (index, byte) in payload.iter().enumerate() {
        frame.push(byte ^ mask[index % mask.len()]);
    }
    stream
        .write_all(&frame)
        .map_err(|error| ManagedChromeError::Io(error.to_string()))
}

fn read_ws_text(stream: &mut std::net::TcpStream) -> Result<String, ManagedChromeError> {
    let mut header = [0_u8; 2];
    stream
        .read_exact(&mut header)
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    let mut len = u64::from(header[1] & 0x7f);
    if len == 126 {
        let mut bytes = [0_u8; 2];
        stream
            .read_exact(&mut bytes)
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        len = u64::from(u16::from_be_bytes(bytes));
    } else if len == 127 {
        let mut bytes = [0_u8; 8];
        stream
            .read_exact(&mut bytes)
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        len = u64::from_be_bytes(bytes);
    }
    let mut mask = [0_u8; 4];
    if masked {
        stream
            .read_exact(&mut mask)
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    }
    let mut payload = vec![0; len as usize];
    stream
        .read_exact(&mut payload)
        .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
    if masked {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % mask.len()];
        }
    }
    match opcode {
        0x1 => String::from_utf8(payload).map_err(|_| ManagedChromeError::MalformedCdpResponse),
        0x8 => Err(ManagedChromeError::Io("websocket closed".to_string())),
        _ => read_ws_text(stream),
    }
}

fn parse_ws_url(url: &str) -> Result<(String, u16, String), ManagedChromeError> {
    let rest = url
        .strip_prefix("ws://")
        .ok_or(ManagedChromeError::MalformedCdpResponse)?;
    let (authority, path) = rest
        .split_once('/')
        .ok_or(ManagedChromeError::MalformedCdpResponse)?;
    let (host, port) = authority
        .split_once(':')
        .ok_or(ManagedChromeError::MalformedCdpResponse)?;
    let port = port
        .parse::<u16>()
        .map_err(|_| ManagedChromeError::MalformedCdpResponse)?;
    Ok((host.to_string(), port, format!("/{path}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::youtube::{is_youtube_url, YoutubeAdapter};
    use std::process::{Child, Stdio};

    /// Spawns a child with the same process-group isolation production applies,
    /// so these tests exercise the real group rather than one that does not
    /// exist.
    #[cfg(unix)]
    fn spawn_isolated(program: &str, arg: &str) -> Child {
        let mut command = Command::new(program);
        command
            .arg(arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_isolation(&mut command);
        command.spawn().expect("spawn test child")
    }

    /// Wraps an isolated child in a session, standing in for a real Chrome.
    #[cfg(unix)]
    fn session_over(child: Child) -> ManagedChromeSession {
        ManagedChromeSession {
            process: ChromeProcessOwner::adopt(child),
            plan: build_launch_plan(
                PathBuf::from("/bin/true"),
                Path::new("/tmp/MoviePartyProfiles"),
                "youtube",
                9222,
                "about:blank",
            )
            .expect("plan"),
        }
    }

    /// True while `pid` exists. Signal 0 only performs the permission and
    /// existence check — it never delivers anything.
    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        // SAFETY: `kill` with signal 0 is a pure liveness probe.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(unix)]
    fn wait_for_process_exit(pid: i32, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if !process_exists(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        !process_exists(pid)
    }

    /// Writes an executable stand-in for Chrome. The Chrome flags arrive as
    /// `$@` and are ignored; only the scripted `body` matters.
    #[cfg(unix)]
    fn fake_chrome(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("fake-chrome");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write fake chrome");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake chrome");
        path
    }

    /// A dead child (Chrome exited/crashed) must be reaped, not kill'd — and the
    /// teardown must still run, because the browser exiting does not mean its
    /// children exited.
    #[cfg(unix)]
    #[test]
    fn close_gracefully_reaps_an_already_exited_child() {
        // A process that exits immediately on spawn (macOS `sleep 0`).
        let mut session = session_over(spawn_isolated("sleep", "0"));
        // Let the child exit before asserting.
        std::thread::sleep(Duration::from_millis(100));
        assert!(!session.is_alive());
        // Must not panic and must reap the zombie.
        session.close_gracefully();
        assert!(!session.is_alive());
    }

    /// A child that ignores Browser.close (CDP unreachable here) is still
    /// terminated by the kill fallback after the graceful timeout — the fallback
    /// is what guarantees the Chrome process never leaks.
    #[cfg(unix)]
    #[test]
    fn close_gracefully_falls_back_to_kill_when_cdp_is_unreachable() {
        // `sleep 30` never exits on its own and has no CDP endpoint, so the
        // graceful path times out and the kill fallback fires.
        let mut session = session_over(spawn_isolated("sleep", "30"));
        let started = Instant::now();
        session.close_gracefully();
        // Exited via the fallback, bounded by the graceful timeout.
        assert!(!session.is_alive());
        assert!(started.elapsed() >= GRACEFUL_CLOSE_TIMEOUT);
    }

    /// Repeated cleanup must be a no-op, not a panic or a second signal — every
    /// teardown path (leave, restart, shutdown, `Drop`) may run it.
    #[cfg(unix)]
    #[test]
    fn close_gracefully_is_idempotent() {
        let mut session = session_over(spawn_isolated("sleep", "30"));

        session.close_gracefully();
        session.close_gracefully();
        session.close_gracefully();

        assert!(!session.is_alive());
    }

    /// MP-20: a launch that never reaches CDP must not leave the browser or its
    /// children running. This is the path that used to drop the `Child` bare.
    #[cfg(unix)]
    #[test]
    fn launch_timeout_kills_and_reaps_the_whole_tree() {
        let root = std::env::temp_dir().join(format!(
            "movie-party-chrome-timeout-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&root).expect("profile dir");
        let pid_file = root.join("pids");

        // Records its own pid and its child's, then stays alive without ever
        // opening a CDP endpoint.
        let executable = fake_chrome(
            &root,
            &format!(
                "echo $$ > {pids}\nsleep 300 &\necho $! >> {pids}\nwait",
                pids = pid_file.display()
            ),
        );
        let plan = build_launch_plan(
            executable,
            &root,
            "youtube",
            allocate_local_cdp_port().expect("cdp port"),
            "about:blank",
        )
        .expect("plan");

        let error = launch_managed_chrome_with_timeout(plan, Duration::from_millis(800))
            .expect_err("a browser that never opens CDP must fail the launch");
        assert_eq!(error, ManagedChromeError::CdpTimeout);

        let pids = recorded_pids(&pid_file, 2);
        assert_eq!(
            pids.len(),
            2,
            "expected the browser pid and its child's, got {pids:?}"
        );
        for pid in pids {
            assert!(
                wait_for_process_exit(pid, Duration::from_secs(5)),
                "pid {pid} survived a failed launch — the tree leaked (MP-20)"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// MP-20: when the browser dies during launch, the real cause must surface
    /// and the tree must still be cleaned up — a browser that exits can leave a
    /// child behind, and the old code had no notion of a tree at all.
    #[cfg(unix)]
    #[test]
    fn launch_child_exit_is_reaped_not_leaked() {
        let root =
            std::env::temp_dir().join(format!("movie-party-chrome-exit-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("profile dir");
        let pid_file = root.join("pids");

        // Records itself and a child, then exits on its own. The child outlives
        // it — exactly the orphan a single-PID teardown leaves behind.
        let executable = fake_chrome(
            &root,
            &format!(
                "echo $$ > {pids}\nsleep 300 &\necho $! >> {pids}\nexit 3",
                pids = pid_file.display()
            ),
        );
        let plan = build_launch_plan(
            executable,
            &root,
            "youtube",
            allocate_local_cdp_port().expect("cdp port"),
            "about:blank",
        )
        .expect("plan");

        let error = launch_managed_chrome_with_timeout(plan, Duration::from_secs(5))
            .expect_err("a browser that exits immediately must fail the launch");
        assert!(
            matches!(error, ManagedChromeError::Process(_)),
            "the child-exit path must report the exit, not a timeout; got {error:?}"
        );

        let pids = recorded_pids(&pid_file, 2);
        assert_eq!(
            pids.len(),
            2,
            "expected the browser pid and its child's, got {pids:?}"
        );
        for pid in pids {
            assert!(
                wait_for_process_exit(pid, Duration::from_secs(5)),
                "pid {pid} survived a launch that failed because the browser exited (MP-20)"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// Reads the pids a fake Chrome recorded, tolerating a write that has not
    /// landed yet.
    #[cfg(unix)]
    fn recorded_pids(pid_file: &Path, expected: usize) -> Vec<i32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let recorded = std::fs::read_to_string(pid_file).unwrap_or_default();
            let pids: Vec<i32> = recorded
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect();
            if pids.len() >= expected || Instant::now() >= deadline {
                return pids;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn builds_dedicated_provider_profile_path() {
        let root = Path::new("/tmp/MoviePartyProfiles");

        assert_eq!(
            provider_profile_path(root, "netflix"),
            Ok(PathBuf::from("/tmp/MoviePartyProfiles/netflix"))
        );
    }

    #[test]
    fn rejects_profile_path_traversal() {
        let root = Path::new("/tmp/MoviePartyProfiles");

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
            Path::new("/tmp/MoviePartyProfiles"),
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
        // The profile dir arg must point at the dedicated provider profile —
        // asserted by parsed path, not by rendered string, because
        // Path::display() emits platform separators (backslashes on Windows)
        // while Chrome itself accepts native paths on every OS.
        let expected_profile = Path::new("/tmp/MoviePartyProfiles").join("prime");
        assert!(args.iter().any(|arg| arg
            .strip_prefix("--user-data-dir=")
            .is_some_and(|value| Path::new(value) == expected_profile)));
        assert!(!args
            .iter()
            .any(|arg| arg.to_ascii_lowercase().contains("cookie")));
    }

    #[test]
    fn launch_plan_rejects_ephemeral_cdp_port_zero() {
        let plan = build_launch_plan(
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            Path::new("/tmp/MoviePartyProfiles"),
            "youtube",
            0,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        );

        assert_eq!(plan, Err(ManagedChromeError::InvalidCdpPort));
    }

    #[test]
    fn chrome_command_owns_the_browser_executable() {
        let executable =
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        let plan = build_launch_plan(
            executable.clone(),
            Path::new("/tmp/MoviePartyProfiles"),
            "youtube",
            DEFAULT_CDP_PORT,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        )
        .expect("launch plan");
        let command = chrome_command(&plan);

        assert_eq!(command.get_program(), executable.as_os_str());
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

    #[test]
    #[ignore = "launches real Chrome and opens a localhost CDP session"]
    fn real_chrome_launches_and_evaluates_cdp() {
        let executable = find_chrome(&default_chrome_candidates()).expect("Chrome installed");
        let root =
            std::env::temp_dir().join(format!("movie-party-chrome-{}", uuid::Uuid::now_v7()));
        let plan = build_launch_plan(executable, &root, "youtube", 9333, "about:blank")
            .expect("launch plan");
        let session = launch_managed_chrome(plan).expect("launch Chrome");
        let targets = session.targets().expect("targets");
        assert!(targets.iter().any(|target| target.target_type == "page"));

        let mut page = session.connect_page().expect("page websocket");
        let result = page.evaluate("(() => 1 + 1)()").expect("evaluate");
        assert_eq!(result["result"]["value"].as_i64(), Some(2));
        page.navigate("data:text/html,<title>Movie Party CDP</title><video></video>")
            .expect("navigate");
        // Allow Chrome a moment to process the navigation
        std::thread::sleep(Duration::from_millis(500));
        let title = page.evaluate("document.title").expect("title evaluation");
        assert_eq!(title["result"]["value"].as_str(), Some("Movie Party CDP"));

        session.close().expect("close");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "launches real Chrome and uses the live YouTube page"]
    fn real_youtube_provider_sync_uses_chrome_cdp() {
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        assert!(is_youtube_url(url));
        let executable = find_chrome(&default_chrome_candidates()).expect("Chrome installed");
        let root =
            std::env::temp_dir().join(format!("movie-party-youtube-{}", uuid::Uuid::now_v7()));
        let plan = build_launch_plan(executable, &root, "youtube", 9336, "about:blank")
            .expect("launch plan");
        let session = launch_managed_chrome(plan).expect("launch Chrome");
        let mut page = session.connect_page().expect("page websocket");
        page.navigate(url).expect("navigate YouTube");
        std::thread::sleep(Duration::from_secs(8));

        let adapter = YoutubeAdapter::default();
        let detected = page.execute(&adapter.detect_player(100)).expect("detect");
        let detected = detected["result"]["value"].as_bool().unwrap_or(false);
        if !detected {
            eprintln!(
                "EXTERNAL PROVIDER VERIFICATION PENDING: YouTube loaded, but no HTML5 player was detected; automation, consent, or geography may have blocked playback"
            );
            session.close().expect("close");
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let position = page.execute(&adapter.get_position(101)).expect("position");
        assert!(position["result"].get("value").is_some());
        let pause = page.execute(&adapter.pause(102)).expect("pause");
        assert_eq!(pause["result"]["value"].as_bool(), Some(true));
        let seek = page.execute(&adapter.seek(103, 1.0)).expect("seek");
        assert_eq!(seek["result"]["value"].as_bool(), Some(true));
        let buffer = page
            .execute(&adapter.get_buffer_state(104))
            .expect("buffer");
        assert!(buffer.get("result").is_some());

        session.close().expect("close");
        let _ = std::fs::remove_dir_all(root);
    }
}
