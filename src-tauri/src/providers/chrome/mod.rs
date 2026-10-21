mod content;
mod process;
pub mod profile;

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

#[cfg(unix)]
pub use process::kill_live_chrome_tree;
use process::ChildPoll;
pub use process::{install_shutdown_hardening, ChromeProcessOwner};

static ACTIVE_SESSIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[derive(Debug)]
struct SessionActivity;
impl SessionActivity {
    fn new() -> Self {
        ACTIVE_SESSIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}
impl Drop for SessionActivity {
    fn drop(&mut self) {
        ACTIVE_SESSIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A CDP worker may temporarily own the session outside AppRuntime's slot.
/// Give its stale-generation cleanup time to flush Chrome before app exit.
pub fn wait_for_session_shutdown(timeout: Duration) -> bool {
    let start = Instant::now();
    while ACTIVE_SESSIONS.load(std::sync::atomic::Ordering::SeqCst) != 0 {
        if start.elapsed() >= timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(25));
    }
    true
}

/// How long a freshly launched Chrome gets to open its CDP endpoint before the
/// launch is declared failed. See [`launch_managed_chrome`].
const CDP_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

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
    #[error("MP-PROVIDER-005 managed profile is busy. Close its Movie Party Chrome window and retry; your ordinary Chrome can stay open")]
    ProfileBusy,
    #[error("MP-PROVIDER-006 {0}")]
    Profile(String),
    #[error("MP-PROVIDER-007 confirm Reset provider session before removing sign-in data")]
    ConfirmationRequired,
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
            // Chrome chooses a port and writes its browser endpoint into this
            // profile's DevToolsActivePort. Never trust a pre-existing listener.
            "--remote-debugging-port=0".to_owned(),
            "--no-first-run".to_owned(),
            "--restore-last-session".to_owned(),
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
    selected_target: std::sync::Mutex<Option<(String, String)>>,
    process: ChromeProcessOwner,
    pub plan: ChromeLaunchPlan,
    lease: profile::ProfileLease,
    _activity: SessionActivity,
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
        let mut candidates = vec![
            PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
        ];
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join("Google/Chrome/Application/chrome.exe"));
        }
        candidates
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
    url: &str,
) -> Result<ChromeLaunchPlan, ManagedChromeError> {
    let profile_path = provider_profile_path(profiles_root, provider_id)?;

    Ok(ChromeLaunchPlan {
        executable,
        profile_path,
        cdp_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        cdp_port: 0, // Filled from this launch's DevToolsActivePort after validation.
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
    mut plan: ChromeLaunchPlan,
    cdp_startup_timeout: Duration,
) -> Result<ManagedChromeSession, ManagedChromeError> {
    validate_cdp_bind(plan.cdp_host)?;
    let mut lease = profile::ProfileLease::acquire(&plan.profile_path)?;
    lease.validate_data()?;
    lease.update(profile::LaunchState::Launching, None)?;
    let endpoint_file = plan.profile_path.join("DevToolsActivePort");
    match fs::remove_file(&endpoint_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(ManagedChromeError::Profile(error.to_string())),
    }

    // The owner takes responsibility the instant the process exists, so no
    // later `?` can drop a live tree on the floor. It also publishes the tree
    // for the signal/panic shutdown paths.
    let mut process = match ChromeProcessOwner::spawn(&plan) {
        Ok(process) => process,
        Err(error) => {
            let _ = lease.update(profile::LaunchState::LaunchFailed, None);
            return Err(error);
        }
    };
    // Record ownership before waiting on CDP. Keep the exclusive lease until
    // this exact process tree has been terminated and reaped.
    if let Err(error) = lease.update(profile::LaunchState::Launching, process.id()) {
        process.terminate_tree();
        return Err(error);
    }

    match wait_for_cdp_or_child_exit(&mut process, &endpoint_file, cdp_startup_timeout) {
        Ok(port) => plan.cdp_port = port,
        Err(error) => {
            // This launch owns the tree and nothing else has ever seen it, so it is
            // killed and reaped right here — the session that would have taken
            // ownership does not exist, so its `Drop` will never run.
            process.terminate_tree();
            let _ = lease.update(profile::LaunchState::LaunchFailed, None);
            return Err(error);
        }
    }

    if let Err(error) = lease.update(profile::LaunchState::Running, process.id()) {
        process.terminate_tree();
        return Err(error);
    }
    Ok(ManagedChromeSession {
        selected_target: std::sync::Mutex::new(None),
        process,
        plan,
        lease,
        _activity: SessionActivity::new(),
    })
}

impl ManagedChromeSession {
    pub fn targets(&self) -> Result<Vec<CdpTarget>, ManagedChromeError> {
        fetch_targets(self.plan.cdp_port)
    }

    pub fn page_target(&self) -> Result<CdpTarget, ManagedChromeError> {
        self.content_page_target()
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
        let unexpected = self.lease.record.state == profile::LaunchState::UnexpectedExit
            || (self.lease.record.state == profile::LaunchState::Running
                && !self.process.is_alive());
        if self.process.is_alive() {
            // Browser.close is independent of content tab choice, including
            // when the selected tab has closed or several windows are open.
            let browser = http_get(self.plan.cdp_port, "/json/version")
                .ok()
                .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
                .and_then(|v| v["webSocketDebuggerUrl"].as_str().map(str::to_owned))
                .and_then(|url| CdpPageSession::connect(&url).ok());
            if let Some(mut page) = browser {
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
        let state = if unexpected {
            profile::LaunchState::UnexpectedExit
        } else {
            profile::LaunchState::Closed
        };
        if let Err(error) = self.lease.update(state, None) {
            tracing::warn!(%error, "Could not persist managed Chrome close state");
        }
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
        let alive = self.process.is_alive();
        if !alive && matches!(self.lease.record.state, profile::LaunchState::Running) {
            let _ = self
                .lease
                .update(profile::LaunchState::UnexpectedExit, self.process.id());
        }
        alive
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

fn verify_navigation_result(result: &serde_json::Value) -> Result<(), ManagedChromeError> {
    if result.get("errorText").is_some()
        || result["isDownload"].as_bool() == Some(true)
        || result["frameId"].as_str().is_none_or(str::is_empty)
    {
        return Err(ManagedChromeError::CdpCommand(
            "Navigation was rejected or did not open a page.".into(),
        ));
    }
    Ok(())
}

impl CdpPageSession {
    pub fn bring_to_front(&mut self) -> Result<(), ManagedChromeError> {
        let id = self.next_command_id();
        self.execute(&CdpCommand {
            id,
            method: "Page.bringToFront",
            params: json!({}),
        })?;
        let visible = self.evaluate("document.visibilityState === 'visible'")?;
        if visible["result"]["value"].as_bool() != Some(true) {
            return Err(ManagedChromeError::CdpCommand(
                "The provider tab could not be activated. Open it in Chrome and refresh.".into(),
            ));
        }
        Ok(())
    }
    fn connect(web_socket_url: &str) -> Result<Self, ManagedChromeError> {
        let (host, port, path) = parse_ws_url(web_socket_url)?;
        if host != CDP_BIND_HOST && host != "localhost" {
            return Err(ManagedChromeError::NonLocalCdpBind);
        }

        let mut stream = std::net::TcpStream::connect((host.as_str(), port))
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| ManagedChromeError::Io(error.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
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
        let result = self.execute(&navigate_command(id, url))?;
        verify_navigation_result(&result)?;
        Ok(result)
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
                if value["result"].get("exceptionDetails").is_some() {
                    return Err(ManagedChromeError::CdpCommand(
                        "The browser rejected the page operation.".into(),
                    ));
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
    endpoint_file: &Path,
    timeout: Duration,
) -> Result<u16, ManagedChromeError> {
    let start = Instant::now();
    loop {
        match process.poll() {
            // Chrome exited before CDP came up: surface the real cause, not a
            // timeout. This is the "Chrome exits unexpectedly" launch-time
            // symptom.
            ChildPoll::Exited(status) => {
                return Err(ManagedChromeError::Process(format!(
                    "Chrome exited during launch with status {status}. Close any Movie Party browser using this profile and retry. If Chrome was updated, retry with the installed version; reset a corrupt session only after confirmation"
                )));
            }
            ChildPoll::Running => {}
            ChildPoll::Failed(error) => {
                return Err(ManagedChromeError::Io(error));
            }
        }
        if let Ok(contents) = fs::read_to_string(endpoint_file) {
            if let Ok((port, browser_path)) = parse_active_endpoint(&contents) {
                if let Ok(body) = http_get(port, "/json/version") {
                    if version_matches_endpoint(&body, port, browser_path) && process.is_alive() {
                        return Ok(port);
                    }
                }
            }
        }
        if start.elapsed() >= timeout {
            #[cfg(test)]
            if let Ok(contents) = fs::read_to_string(endpoint_file) {
                eprintln!("Startup endpoint diagnostic: {contents:?}");
                if let Ok((port, path)) = parse_active_endpoint(&contents) {
                    eprintln!(
                        "Version response diagnostic: {:?}; expected path {path}",
                        http_get(port, "/json/version")
                    );
                }
            }
            return Err(ManagedChromeError::CdpTimeout);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn parse_active_endpoint(contents: &str) -> Result<(u16, &str), ManagedChromeError> {
    let mut lines = contents.lines();
    let port = lines
        .next()
        .and_then(|v| v.parse::<u16>().ok())
        .filter(|v| *v != 0)
        .ok_or(ManagedChromeError::MalformedCdpResponse)?;
    let path = lines
        .next()
        .filter(|v| v.starts_with("/devtools/browser/") && !v.contains(['?', '#', '\\', '\r', ' ']))
        .ok_or(ManagedChromeError::MalformedCdpResponse)?;
    Ok((port, path))
}

fn version_matches_endpoint(body: &str, port: u16, browser_path: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["webSocketDebuggerUrl"].as_str().map(str::to_owned))
        .and_then(|url| parse_ws_url(&url).ok())
        .is_some_and(|(host, actual_port, path)| {
            host == CDP_BIND_HOST && actual_port == port && path == browser_path
        })
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
    // `Child`/`Stdio` are only used by the `#[cfg(unix)]` helpers below. Without
    // the gate this is an unused import on Windows, which `clippy -D warnings`
    // rejects — and that failure happens BEFORE the test step, so Windows would
    // run no tests at all. macOS clippy cannot see it.
    #[cfg(unix)]
    use std::process::{Child, Stdio};

    /// A temp Chrome profile directory that removes itself on drop.
    ///
    /// Two defects motivated this, both found while running the real-Chrome
    /// tests in Batch 13:
    ///
    /// * `let _ = std::fs::remove_dir_all(root)` **swallowed every failure**, so
    ///   a profile that could not be removed left no trace in the output.
    /// * Cleanup only ran on the success path. A test that panicked before its
    ///   last line — the sandboxed CDP failure did exactly that — never removed
    ///   anything at all.
    ///
    /// Together they let `movie-party-chrome-*` directories accumulate in
    /// `$TMPDIR` with nothing to say so. The guard also runs on unwind, retries
    /// briefly (Chrome may still be flushing when `close_gracefully` returns),
    /// and complains loudly if it still cannot remove the directory. It
    /// deliberately does **not** fail the test: the product is not what leaked.
    struct TempProfile {
        path: PathBuf,
    }

    impl TempProfile {
        fn new(prefix: &str) -> Self {
            let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::now_v7()));
            fs::create_dir_all(&path).expect("create temp profile dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempProfile {
        fn drop(&mut self) {
            for _ in 0..20 {
                match fs::remove_dir_all(&self.path) {
                    Ok(()) => return,
                    // Already gone counts as success.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                    Err(_) => thread::sleep(Duration::from_millis(50)),
                }
            }
            eprintln!(
                "warning: could not remove the temp Chrome profile at {} — \
                 it is still on disk and needs removing by hand",
                self.path.display()
            );
        }
    }

    /// Proves the guard above cleans up on the path that used to leak.
    ///
    /// The old cleanup sat on the last line of each test, so a panic skipped it
    /// entirely — which is how the `movie-party-chrome-*` directories dated
    /// 2026-09-19 came to be sitting in `$TMPDIR`. This panics *inside* the
    /// guard and asserts the directory is gone regardless.
    ///
    /// This is a real control, not decoration: delete the `Drop` impl and this
    /// test goes red.
    #[test]
    fn temp_profile_is_removed_even_when_the_test_panics() {
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let recorder = std::sync::Arc::clone(&observed);

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let profile = TempProfile::new("movie-party-chrome-panic");
            *recorder.lock().expect("recorder") = Some(profile.path().to_path_buf());
            panic!("simulated mid-test failure");
        }));

        assert!(outcome.is_err(), "the simulated failure must propagate");

        let path = observed
            .lock()
            .expect("recorder")
            .clone()
            .expect("the guard must have been constructed");
        assert!(
            !path.exists(),
            "a panicking test left its temp profile behind at {}",
            path.display()
        );
    }

    /// The two `real_chrome_*` tests open a **visible** Chrome window on
    /// whatever desktop runs them.
    ///
    /// That is not an accident of the test: [`ChromeLaunchPlan::args`] has no
    /// `--headless`, because the product needs the user to *see* the provider
    /// page. The tests therefore cannot be made quiet without changing the
    /// product, and must not be.
    ///
    /// They are `#[ignore]`d for that reason — but `--ignored` is one flag away,
    /// and running them without meaning to pops real browser windows onto the
    /// owner's screen and then closes them, which reads exactly like Chrome
    /// crashing. So they require an explicit opt-in, and **fail loudly** without
    /// it rather than quietly doing nothing — the AUD-07 rule: a test that was
    /// asked for must produce a verdict, not a shrug.
    fn require_visible_chrome_consent() {
        if std::env::var_os("MOVIE_PARTY_ALLOW_VISIBLE_CHROME").is_none() {
            panic!(
                "refusing to run: this test opens a REAL, VISIBLE Chrome window on the \
                 current desktop (the launch plan deliberately has no --headless, and \
                 must not gain one — the provider page has to be visible to the user). \
                 Set MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 to confirm you want that. \
                 CI never runs this test: it is #[ignore]d and CI does not pass --ignored."
            );
        }
    }

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
        let root = std::env::temp_dir().join(format!("mp-session-{}", uuid::Uuid::now_v7()));
        let lease = profile::ProfileLease::acquire(&root.join("youtube")).expect("profile lease");
        ManagedChromeSession {
            selected_target: std::sync::Mutex::new(None),
            lease,
            _activity: SessionActivity::new(),
            process: ChromeProcessOwner::adopt(child),
            plan: build_launch_plan(PathBuf::from("/bin/true"), &root, "youtube", "about:blank")
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
        let root = TempProfile::new("movie-party-chrome-timeout");
        let pid_file = root.path().join("pids");

        // Records its own pid and its child's, then stays alive without ever
        // opening a CDP endpoint.
        let executable = fake_chrome(
            root.path(),
            &format!(
                "echo $$ > {pids}\nsleep 300 &\necho $! >> {pids}\nwait",
                pids = pid_file.display()
            ),
        );
        let plan =
            build_launch_plan(executable, root.path(), "youtube", "about:blank").expect("plan");

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
    }

    /// MP-20: when the browser dies during launch, the real cause must surface
    /// and the tree must still be cleaned up — a browser that exits can leave a
    /// child behind, and the old code had no notion of a tree at all.
    #[cfg(unix)]
    #[test]
    fn launch_child_exit_is_reaped_not_leaked() {
        let root = TempProfile::new("movie-party-chrome-exit");
        let pid_file = root.path().join("pids");

        // Records itself and a child, then exits on its own. The child outlives
        // it — exactly the orphan a single-PID teardown leaves behind.
        let executable = fake_chrome(
            root.path(),
            &format!(
                "echo $$ > {pids}\nsleep 300 &\necho $! >> {pids}\nexit 3",
                pids = pid_file.display()
            ),
        );
        let plan =
            build_launch_plan(executable, root.path(), "youtube", "about:blank").expect("plan");

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
            "https://www.primevideo.com/",
        )
        .expect("launch plan");
        let args = plan.args();

        assert!(args
            .iter()
            .any(|arg| arg == "--remote-debugging-address=127.0.0.1"));
        assert!(args.iter().any(|arg| arg == "--remote-debugging-port=0"));
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
    fn launch_plan_uses_chrome_selected_ephemeral_port() {
        let plan = build_launch_plan(
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            Path::new("/tmp/MoviePartyProfiles"),
            "youtube",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        );

        assert_eq!(plan.expect("plan").cdp_port, 0);
    }

    #[test]
    fn chrome_command_owns_the_browser_executable() {
        let executable =
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        let plan = build_launch_plan(
            executable.clone(),
            Path::new("/tmp/MoviePartyProfiles"),
            "youtube",
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
    fn endpoint_must_match_this_profiles_new_browser_on_loopback() {
        let (port, path) =
            parse_active_endpoint("53121\n/devtools/browser/unique-launch\n").unwrap();
        let response = |host: &str, port: u16, path: &str| {
            serde_json::json!({
                "webSocketDebuggerUrl": format!("ws://{host}:{port}{path}")
            })
            .to_string()
        };
        assert!(version_matches_endpoint(
            &response("127.0.0.1", port, path),
            port,
            path
        ));
        assert!(!version_matches_endpoint(
            &response("100.64.0.1", port, path),
            port,
            path
        ));
        assert!(!version_matches_endpoint(
            &response("127.0.0.1", port + 1, path),
            port,
            path
        ));
        assert!(!version_matches_endpoint(
            &response("127.0.0.1", port, "/devtools/browser/other"),
            port,
            path
        ));
        assert!(!version_matches_endpoint("{}", port, path));
        for invalid in [
            "0\n/devtools/browser/id",
            "53121\nhttp://external",
            "bad",
            "65536\n/devtools/browser/id",
        ] {
            assert!(parse_active_endpoint(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn missing_executable_does_not_erase_profile_and_records_launch_failure() {
        let root = TempProfile::new("mp-missing-chrome");
        let profile = root.path().join("youtube");
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join("sentinel"), b"keep sign-in storage").unwrap();
        let plan = build_launch_plan(
            root.path().join("nonexistent-chrome"),
            root.path(),
            "youtube",
            "about:blank",
        )
        .unwrap();
        assert!(matches!(
            launch_managed_chrome(plan),
            Err(ManagedChromeError::Process(_))
        ));
        assert!(profile.join("sentinel").exists());
        let record: profile::OwnershipRecord =
            serde_json::from_slice(&fs::read(root.path().join(".youtube.ownership")).unwrap())
                .unwrap();
        assert_eq!(record.state, profile::LaunchState::LaunchFailed);
        assert_eq!(record.pid, None);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn create_party_keeps_the_owned_browser_until_leave() {
        struct LoopbackEnv(Option<std::ffi::OsString>);
        impl Drop for LoopbackEnv {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", value),
                    None => std::env::remove_var("MOVIE_PARTY_DEV_LOOPBACK"),
                }
            }
        }
        let _env = LoopbackEnv(std::env::var_os("MOVIE_PARTY_DEV_LOOPBACK"));
        std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");
        let session = session_over(spawn_isolated("/bin/sleep", "300"));
        let pid = session.process.id().unwrap() as i32;
        let profile = session.plan.profile_path.clone();
        fs::write(profile.join("session-sentinel"), b"persistent").unwrap();
        let runtime = crate::app_runtime::AppRuntime::new_without_emitter();
        runtime.open_provider_browser("youtube".into(), session);
        runtime.update_provider_readiness(crate::providers::sync::ProviderReadiness::PlaybackReady);
        runtime.create_local_party(None, true).await.unwrap();
        assert!(runtime.session_matches_provider("youtube"));
        assert!(process_exists(pid), "room creation killed its browser");
        runtime.attach_launched_provider(
            "youtube".into(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        );
        assert!(
            runtime.prepare_provider_reset("youtube").is_err(),
            "reset must refuse an active room"
        );
        runtime.leave_party();
        assert!(wait_for_process_exit(pid, Duration::from_secs(5)));
        assert_eq!(
            fs::read(profile.join("session-sentinel")).unwrap(),
            b"persistent"
        );
        let _ = fs::remove_dir_all(profile.parent().unwrap());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unexpected_browser_exit_releases_profile_and_requires_readiness_again() {
        let mut session = session_over(spawn_isolated("/bin/sleep", "300"));
        let profile = session.plan.profile_path.clone();
        // The fixture is an owned isolated process, not real authenticated Chrome.
        session
            .lease
            .update(profile::LaunchState::Running, session.process.id())
            .unwrap();
        let pid = session.process.id().unwrap() as i32;
        let runtime = crate::app_runtime::AppRuntime::new_without_emitter();
        runtime.open_provider_browser("youtube".into(), session);
        runtime.update_provider_readiness(crate::providers::sync::ProviderReadiness::PlaybackReady);
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        while runtime.snapshot().provider.readiness
            != crate::providers::sync::ProviderReadiness::NotStarted
        {
            assert!(
                Instant::now() < deadline,
                "crash watcher did not invalidate readiness"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(!runtime.snapshot().participants[0].media_ready);
        // Wait for the out-of-lock teardown to finish before testing the lease.
        while profile::ProfileLease::acquire(&profile).is_err() {
            assert!(
                Instant::now() < deadline,
                "dead browser kept its profile locked"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let record: profile::OwnershipRecord = serde_json::from_slice(
            &fs::read(profile.parent().unwrap().join(".youtube.ownership")).unwrap(),
        )
        .unwrap();
        assert_eq!(record.state, profile::LaunchState::UnexpectedExit);
        runtime.leave_party();
        let _ = fs::remove_dir_all(profile.parent().unwrap());
    }

    #[test]
    fn shutdown_wait_includes_sessions_held_outside_runtime_state() {
        let activity = SessionActivity::new();
        assert!(!wait_for_session_shutdown(Duration::ZERO));
        let cleanup = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(activity);
        });
        assert!(wait_for_session_shutdown(Duration::from_secs(2)));
        cleanup.join().unwrap();
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
    fn navigation_acknowledgements_require_a_real_page() {
        for rejected in [
            json!({}),
            json!({"frameId":""}),
            json!({"frameId":"a","errorText":"blocked"}),
            json!({"frameId":"a","isDownload":true}),
        ] {
            assert!(verify_navigation_result(&rejected).is_err());
        }
        assert!(verify_navigation_result(&json!({"frameId":"a","isDownload":false})).is_ok());
    }

    #[test]
    #[ignore = "launches real Chrome and opens a localhost CDP session"]
    fn real_chrome_launches_and_evaluates_cdp() {
        require_visible_chrome_consent();
        let executable = find_chrome(&default_chrome_candidates()).expect("Chrome installed");
        let root = TempProfile::new("movie-party-chrome");
        let plan = build_launch_plan(executable, root.path(), "youtube", "about:blank")
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

        let observation = page
            .evaluate(content::OBSERVE_PAGE)
            .expect("public metadata");
        let observed = &observation["result"]["value"];
        assert_eq!(observed["title"], "Movie Party CDP");
        assert_eq!(
            observed["ready"], false,
            "an empty video is not loaded content"
        );
        assert!(observed["documentId"]
            .as_str()
            .is_some_and(|v| !v.is_empty()));

        // Test-only public DOM identity around a real bundled video. A URL
        // alone is unchanged while the active player's declared ID changes.
        let media = STANDARD.encode(include_bytes!(
            "../../../tests/fixtures/movie_party_test_320x240.mp4"
        ));
        let html = format!("<title>Public video fixture</title><section data-video-id='abcdefghijk'><video preload='auto' style='width:320px;height:240px' src='data:video/mp4;base64,{media}'></video></section>");
        page.navigate(&format!("data:text/html;base64,{}", STANDARD.encode(html)))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let observed = loop {
            let value = page.evaluate(content::OBSERVE_PAGE).unwrap();
            let observed = value["result"]["value"].clone();
            if observed["ready"].as_bool() == Some(true) {
                break observed;
            }
            assert!(Instant::now() < deadline, "real fixture video did not load");
            std::thread::sleep(Duration::from_millis(50));
        };
        let evidence = crate::providers::selection::ContentEvidence {
            title: observed["title"].as_str().unwrap().into(),
            duration_ms: observed["durationMs"].as_u64().unwrap(),
            player_content_id: observed["playerContentId"].as_str().map(str::to_owned),
        };
        assert!(evidence.valid_for("youtube", "abcdefghijk"));
        page.evaluate(
            "document.querySelector('[data-video-id]').setAttribute('data-video-id','dQw4w9WgXcQ')",
        )
        .unwrap();
        let changed = page.evaluate(content::OBSERVE_PAGE).unwrap();
        assert_eq!(changed["result"]["value"]["playerContentId"], "dQw4w9WgXcQ");
        let changed_evidence = crate::providers::selection::ContentEvidence {
            player_content_id: Some("dQw4w9WgXcQ".into()),
            ..evidence.clone()
        };
        assert!(!evidence.matches(&changed_evidence, "youtube", "abcdefghijk"));

        session.close().expect("close");
    }

    #[test]
    #[ignore = "opens visible disposable Chrome tabs/windows; no provider authentication"]
    fn real_chrome_follows_active_tabs_and_detects_closed_targets() {
        require_visible_chrome_consent();
        let executable = find_chrome(&default_chrome_candidates()).expect("Chrome installed");
        let root = TempProfile::new("movie-party-tabs");
        let session = launch_managed_chrome(
            build_launch_plan(executable, root.path(), "youtube", "about:blank").unwrap(),
        )
        .unwrap();
        let first = session.page_target().unwrap().id;
        let mut page = session.connect_target(&first).unwrap();
        let id = page.next_command_id();
        let created = page
            .execute(&CdpCommand {
                id,
                method: "Target.createTarget",
                params: json!({"url":"about:blank"}),
            })
            .unwrap();
        let second = created["targetId"].as_str().unwrap();
        let mut second_page = session.connect_target(second).unwrap();
        second_page.bring_to_front().unwrap();
        assert_eq!(session.page_target().unwrap().id, second);
        assert_ne!(first, second);
        let id = page.next_command_id();
        let window = page
            .execute(&CdpCommand {
                id,
                method: "Target.createTarget",
                params: json!({"url":"about:blank", "newWindow":true}),
            })
            .unwrap();
        let window_id = window["targetId"].as_str().unwrap();
        session
            .connect_target(window_id)
            .unwrap()
            .bring_to_front()
            .unwrap();
        // OS focus may stay with the test runner. If no focused window is
        // observable, fail closed instead of selecting the first CDP target.
        match session.page_target() {
            Ok(target) => assert_eq!(target.id, window_id),
            Err(error) => assert!(error.to_string().contains("Several browser windows")),
        }
        let id = page.next_command_id();
        let closed = page
            .execute(&CdpCommand {
                id,
                method: "Target.closeTarget",
                params: json!({"targetId":window_id}),
            })
            .unwrap();
        assert_eq!(closed["success"].as_bool(), Some(true));
        // Target.closeTarget acknowledges the request before destruction is
        // necessarily visible in /json/list. Verify the observed outcome.
        let deadline = Instant::now() + Duration::from_secs(5);
        while session.connect_target(window_id).is_ok() {
            assert!(
                Instant::now() < deadline,
                "closed target remained available"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        session.close().unwrap();
    }

    #[test]
    #[ignore = "opens visible Chrome twice using only a disposable local-page profile"]
    fn real_chrome_profile_storage_survives_managed_close_and_relaunch() {
        require_visible_chrome_consent();
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        struct LocalPage {
            stop: Arc<AtomicBool>,
            thread: Option<std::thread::JoinHandle<()>>,
        }
        impl Drop for LocalPage {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::SeqCst);
                if let Some(thread) = self.thread.take() {
                    thread.join().unwrap();
                }
            }
        }
        let listener = std::net::TcpListener::bind((CDP_BIND_HOST, 0)).unwrap();
        let origin = format!(
            "http://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        );
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let running = stop.clone();
        let server = std::thread::spawn(move || {
            while !running.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut buffer = [0; 4096];
                        let _ = socket.read(&mut buffer);
                        let body = "<html><title>Movie Party profile test</title><body>Disposable local storage test only</body></html>";
                        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = socket.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("local fixture server: {error}"),
                }
            }
        });
        let _server = LocalPage {
            stop,
            thread: Some(server),
        };
        let root = TempProfile::new("movie-party-storage");
        let executable = find_chrome(&default_chrome_candidates()).unwrap();
        let mut plan = build_launch_plan(executable, root.path(), "youtube", &origin).unwrap();
        let session = launch_managed_chrome(plan.clone()).unwrap();
        let page_for_origin = |session: &ManagedChromeSession| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(target) = session
                    .targets()
                    .unwrap()
                    .into_iter()
                    .find(|target| target.target_type == "page" && target.url == origin)
                {
                    let mut page =
                        CdpPageSession::connect(&target.web_socket_debugger_url).unwrap();
                    if page.evaluate("document.title").unwrap()["result"]["value"]
                        == "Movie Party profile test"
                    {
                        break page;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "Chrome did not restore its local test tab"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        };
        let mut page = page_for_origin(&session);
        let saved = page.evaluate("localStorage.setItem('movie-party-test', 'local-sentinel'); sessionStorage.setItem('movie-party-test', 'session-sentinel'); true").unwrap();
        assert_eq!(saved["result"]["value"].as_bool(), Some(true));
        drop(page);
        session.close().unwrap();
        assert!(root.path().join("youtube/Local State").is_file());
        plan.url = "about:blank".into();
        let relaunched = launch_managed_chrome(plan).unwrap();
        let mut page = page_for_origin(&relaunched);
        assert_eq!(
            page.evaluate("localStorage.getItem('movie-party-test')")
                .unwrap()["result"]["value"],
            "local-sentinel"
        );
        assert_eq!(
            page.evaluate("sessionStorage.getItem('movie-party-test')")
                .unwrap()["result"]["value"],
            "session-sentinel"
        );
        drop(page);
        relaunched.close().unwrap();
    }

    #[test]
    #[ignore = "launches real Chrome and uses the live YouTube page"]
    fn real_youtube_provider_sync_uses_chrome_cdp() {
        require_visible_chrome_consent();
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        assert!(is_youtube_url(url));
        let executable = find_chrome(&default_chrome_candidates()).expect("Chrome installed");
        let root = TempProfile::new("movie-party-youtube");
        let plan = build_launch_plan(executable, root.path(), "youtube", "about:blank")
            .expect("launch plan");
        let session = launch_managed_chrome(plan).expect("launch Chrome");
        let mut page = session.connect_page().expect("page websocket");
        page.navigate(url).expect("navigate YouTube");
        std::thread::sleep(Duration::from_secs(8));

        let adapter = YoutubeAdapter::default();
        let detected = page.execute(&adapter.detect_player(100)).expect("detect");
        let detected = detected["result"]["value"].as_bool().unwrap_or(false);
        if !detected {
            session.close().expect("close");
            // AUD-07: this used to `return` here, and libtest reports an early
            // return as `ok` — so an explicitly-requested run could report
            // success without having verified anything at all. A test that was
            // asked for must produce a verdict, not a shrug. The failure is a
            // failure of the *verification*, not of the product.
            panic!(
                "no HTML5 player was detected on the live YouTube page, so provider \
                 synchronization was NOT verified; automation, consent or geography \
                 may have blocked playback"
            );
        }

        // A real position, not merely a present field.
        let position = page.execute(&adapter.get_position(101)).expect("position");
        let position_before = position["result"]["value"]
            .as_f64()
            .expect("get_position must return a numeric currentTime");
        assert!(
            position_before.is_finite() && position_before >= 0.0,
            "currentTime must be a real position; got {position_before}"
        );

        let pause = page.execute(&adapter.pause(102)).expect("pause");
        assert_eq!(pause["result"]["value"].as_bool(), Some(true));

        // The seek must actually move the playhead. Asserting only that CDP
        // returned `true` said nothing about the page — the old assertions here
        // could not distinguish a working seek from a no-op.
        let seek = page.execute(&adapter.seek(103, 1.0)).expect("seek");
        assert_eq!(seek["result"]["value"].as_bool(), Some(true));
        std::thread::sleep(Duration::from_millis(500));
        let after = page
            .execute(&adapter.get_position(105))
            .expect("position after seek");
        let position_after = after["result"]["value"]
            .as_f64()
            .expect("get_position must return a numeric currentTime");
        assert!(
            (position_after - 1.0).abs() < 1.0,
            "after seeking to 1.0 s the playhead must be near 1.0 s; it was \
             {position_after} (before the seek: {position_before})"
        );

        let buffer = page
            .execute(&adapter.get_buffer_state(104))
            .expect("buffer");
        assert!(buffer.get("result").is_some());

        session.close().expect("close");
    }
}
