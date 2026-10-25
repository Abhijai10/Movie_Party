use super::*;
use crate::companion::host::{CompanionRequest, CompanionResponse};
use crate::companion::{self, Capability, CompanionErrorCode, MAX_FRAME_BYTES, PROTOCOL_VERSION};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};

/// What the companion can honestly do today. Reading a tab the user pointed at is
/// the whole feature; controlling playback is not, because no provider's protected
/// player has ever been observed obeying an injected command.
pub const GRANTED_CAPABILITIES: [Capability; 3] = [
    Capability::IdentifyTab,
    Capability::ReportContent,
    Capability::ReportPlayback,
];

#[derive(Default)]
pub(super) struct CompanionState {
    pub listening: bool,
    pub port: u16,
    pub pairing: Option<companion::Pairing>,
    pub attempts: u8,
    pub secret: Option<[u8; 32]>,
    pub sequence: Option<u64>,
    pub browser: Option<String>,
    pub last_error: Option<CompanionErrorCode>,
    pub stop: Option<Arc<AtomicBool>>,
    pub task: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionStatus {
    pub listening: bool,
    pub port: u16,
    pub pairing_code: Option<String>,
    pub paired: bool,
    pub browser: Option<String>,
    pub protocol: u8,
    pub granted: Vec<String>,
    pub refused: Vec<RefusedCapability>,
    pub error: Option<String>,
    pub guidance: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefusedCapability {
    pub capability: String,
    pub reason: String,
}

impl AppRuntime {
    pub fn companion_status(&self) -> CompanionStatus {
        let state = self.lock();
        CompanionStatus {
            listening: state.companion.listening,
            port: state.companion.port,
            pairing_code: state.companion.pairing.as_ref().map(|p| p.code.clone()),
            paired: state.companion.secret.is_some(),
            browser: state.companion.browser.clone(),
            protocol: PROTOCOL_VERSION,
            granted: GRANTED_CAPABILITIES
                .iter()
                .map(|capability| capability.as_str().to_string())
                .collect(),
            refused: refused_capabilities(),
            error: state.companion.last_error.map(|error| format!("{error:?}")),
            guidance: state
                .companion
                .last_error
                .map(|error| error.guidance().to_string()),
        }
    }

    /// Capabilities that stay off, each with the reason it is off. Reported to the
    /// extension so the UI can say why instead of hiding a button.
    pub fn companion_refused() -> Vec<RefusedCapability> {
        refused_capabilities()
    }

    /// Begin accepting companion connections. The port is ephemeral and loopback
    /// only; the endpoint file that advertises it is written by the caller, which
    /// is the only place that knows the application data directory.
    pub fn companion_start(&self) -> Result<u16, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| "Could not open the companion listener".to_string())?;
        let port = listener
            .local_addr()
            .map_err(|_| "Could not read the companion port".to_string())?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|_| "Could not configure the companion listener".to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let quit = stop.clone();
        let runtime = self.clone();
        let task = std::thread::spawn(move || {
            while !quit.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        runtime.serve_companion_connection(stream);
                        if quit.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                    Err(_) => break,
                }
            }
        });
        let mut state = self.lock();
        state.companion.stop = Some(stop);
        state.companion.task = Some(task);
        state.companion.listening = true;
        state.companion.port = port;
        state.companion.secret = None;
        state.companion.sequence = None;
        state.companion.pairing = Some(companion::generate_pairing());
        state.companion.attempts = 0;
        state.companion.last_error = None;
        Ok(port)
    }

    pub fn companion_stop(&self) {
        let task = {
            let mut state = self.lock();
            state.companion.listening = false;
            state.companion.port = 0;
            state.companion.pairing = None;
            state.companion.secret = None;
            state.companion.sequence = None;
            state.companion.browser = None;
            if let Some(stop) = state.companion.stop.take() {
                stop.store(true, Ordering::Release);
            }
            state.companion.task.take()
        };
        if let Some(task) = task {
            let _ = task.join();
        }
    }

    pub fn companion_new_code(&self) -> Result<String, String> {
        let mut state = self.lock();
        if !state.companion.listening {
            return Err("Turn the companion on first.".into());
        }
        let pairing = companion::generate_pairing();
        let code = pairing.code.clone();
        state.companion.pairing = Some(pairing);
        state.companion.attempts = 0;
        state.companion.secret = None;
        state.companion.sequence = None;
        Ok(code)
    }

    /// Unlink the browser. The pairing key is dropped, so a stale extension left
    /// installed cannot keep reporting tabs after the user stops.
    pub fn companion_unlink(&self) -> CompanionStatus {
        {
            let mut state = self.lock();
            state.companion.secret = None;
            state.companion.sequence = None;
            state.companion.browser = None;
            state.companion.pairing = Some(companion::generate_pairing());
            state.companion.attempts = 0;
        }
        self.companion_status()
    }

    /// Where the native host manifest must go for this OS. Writing it is always an
    /// explicit user action — Movie Party never installs anything into a browser by
    /// itself.
    pub fn companion_host_manifest(extension_id: &str) -> Result<(String, String), String> {
        let json = Self::companion_host_payload(extension_id)?;
        let directory = companion_host_directories()?
            .into_iter()
            .next()
            .ok_or("No supported browser directory".to_string())?;
        Ok((
            directory
                .join(format!("{HOST_FILE_NAME}.json"))
                .display()
                .to_string(),
            json,
        ))
    }

    fn companion_host_payload(extension_id: &str) -> Result<String, String> {
        let executable = std::env::current_exe()
            .map_err(|_| "Could not locate the Movie Party companion host".to_string())?
            .to_string_lossy()
            .to_string();
        companion::host::host_manifest(&executable, extension_id)
            .map_err(|error| error.guidance().to_string())
    }

    /// Write the host manifest after the user pasted their own extension id and
    /// clicked Install. Both are deliberate, so this is an opt-in installation and
    /// not a silent one; it registers one origin, and nothing else.
    pub fn companion_install_host(extension_id: &str) -> Result<String, String> {
        let (path, json) = Self::companion_host_manifest(extension_id)?;
        let directory = std::path::Path::new(&path)
            .parent()
            .ok_or("Could not resolve the browser host directory".to_string())?;
        std::fs::create_dir_all(directory)
            .map_err(|_| "Could not create the browser host directory".to_string())?;
        std::fs::write(&path, json).map_err(|_| {
            "Could not write the browser host manifest. Check disk access.".to_string()
        })?;
        Ok(path)
    }

    /// Removal is reversible and leaves the extension itself alone: unloading an
    /// extension is the user's action in their browser, never ours.
    pub fn companion_remove_host() -> Result<(), String> {
        for directory in companion_host_directories()? {
            let path = directory.join(format!("{HOST_FILE_NAME}.json"));
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("Could not remove the browser host manifest.".to_string()),
            }
        }
        Ok(())
    }

    fn serve_companion_connection(&self, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));
        let mut header = [0u8; 4];
        if stream.read_exact(&mut header).is_err() {
            return;
        }
        let length = u32::from_le_bytes(header) as usize;
        if length == 0 || length > MAX_FRAME_BYTES {
            let _ = write_companion(&mut stream, &refuse("", CompanionErrorCode::FrameTooLarge));
            return;
        }
        let mut body = vec![0u8; length];
        if stream.read_exact(&mut body).is_err() {
            return;
        }
        let reply = self.handle_companion_frame(&body);
        let _ = write_companion(&mut stream, &reply);
    }

    /// Every companion decision lives here, away from sockets, so the tests below
    /// exercise the same code the listener does.
    pub fn handle_companion_frame(&self, bytes: &[u8]) -> Vec<u8> {
        let Ok(value) = companion::decode_frame(bytes) else {
            return refuse("", CompanionErrorCode::MalformedFrame);
        };
        let Some(kind) = value
            .get("kind")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        else {
            return refuse("", CompanionErrorCode::MalformedFrame);
        };
        // The handshake is the one message with no capability of its own: it is
        // what establishes the key every later message is signed with, so it must
        // be allowed through the kind gate rather than refused by it.
        if kind != "handshake" && companion::host::capability_for(&kind).is_none() {
            return refuse(&kind, CompanionErrorCode::MalformedFrame);
        }
        let Ok(request) = serde_json::from_value::<CompanionRequest>(value) else {
            return refuse(&kind, CompanionErrorCode::MalformedFrame);
        };
        if kind == "handshake" {
            return self.companion_handshake(&request);
        }
        self.companion_command(&request, &kind)
    }

    fn companion_handshake(&self, request: &CompanionRequest) -> Vec<u8> {
        let (expected, secret_offered) = {
            let mut state = self.lock();
            if !state.companion.listening {
                return refuse("handshake", CompanionErrorCode::ExtensionMissing);
            }
            state.companion.attempts += 1;
            if state.companion.attempts > companion::MAX_PAIRING_ATTEMPTS {
                state.companion.last_error = Some(CompanionErrorCode::PairingFailed);
                state.companion.pairing = None;
                return refuse("handshake", CompanionErrorCode::PairingFailed);
            }
            let Some(pairing) = state.companion.pairing.clone() else {
                return refuse("handshake", CompanionErrorCode::NotPaired);
            };
            (pairing.code, pairing.secret)
        };
        if let Err(error) = companion::host::authorize_handshake(request, &expected, 512) {
            self.lock().companion.last_error = Some(error);
            return refuse("handshake", error);
        }
        let mut state = self.lock();
        state.companion.secret = Some(secret_offered);
        state.companion.sequence = Some(request.sequence);
        state.companion.attempts = 0;
        state.companion.browser = Some("browser".into());
        let reply = serde_json::json!({
            "kind": "handshake",
            "ok": true,
            "secret": URL_SAFE_NO_PAD.encode(secret_offered),
            "capabilities": GRANTED_CAPABILITIES
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>(),
            "refused": Self::companion_refused(),
        });
        serde_json::to_vec(&reply)
            .unwrap_or_else(|_| refuse("handshake", CompanionErrorCode::MalformedFrame))
    }

    fn companion_command(&self, request: &CompanionRequest, kind: &str) -> Vec<u8> {
        let granted = GRANTED_CAPABILITIES.to_vec();
        let outcome = {
            let state = self.lock();
            match (state.companion.secret, state.companion.sequence) {
                (Some(secret), previous) => companion::host::authorize_command(
                    request,
                    &secret,
                    previous,
                    &granted,
                    MAX_FRAME_BYTES / 2,
                ),
                (None, _) => Err(CompanionErrorCode::NotPaired),
            }
        };
        let authorised = match outcome {
            Ok(value) => value,
            Err(error) => {
                self.lock().companion.last_error = Some(error);
                return refuse(kind, error);
            }
        };
        let Some(url) = request.url.as_deref() else {
            {
                let mut state = self.lock();
                state.companion.sequence = Some(request.sequence);
            }
            return refuse(kind, CompanionErrorCode::MalformedFrame);
        };
        // Reuse the provider the room is already about, so a companion tab cannot
        // quietly retarget a session that is under way.
        // The room decides which provider this is. A companion message cannot
        // retarget a session that is already under way, so with no provider chosen
        // yet the reported host decides nothing on its own.
        let provider = self
            .lock()
            .provider
            .provider_id
            .clone()
            .ok_or(CompanionErrorCode::NotPaired);
        let provider = match provider {
            Ok(provider) => provider,
            Err(error) => {
                self.lock().companion.last_error = Some(error);
                return refuse(kind, error);
            }
        };
        let identity = match crate::providers::content::parse_content(&provider, url) {
            Ok(identity) => identity,
            Err(_) => {
                self.lock().companion.last_error = Some(CompanionErrorCode::OriginNotAllowed);
                return refuse(kind, CompanionErrorCode::OriginNotAllowed);
            }
        };
        let playback_ready = request.position_ms.is_some() && authorised != Capability::IdentifyTab;
        let tab = crate::providers::content::ContentTab {
            player: request.position_ms.map(|position_ms| {
                crate::providers::playback::Observation {
                    position_ms,
                    duration_ms: request.duration_ms.unwrap_or(position_ms.saturating_add(1)),
                    buffer_ms: 0,
                    ready_state: 4,
                    paused: request.paused.unwrap_or(true),
                    ended: false,
                    seeking: false,
                }
            }),
            selection_key: String::new(),
            target_id: format!("companion:{}", request.tab_id.unwrap_or(0)),
            title: request
                .title
                .clone()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| identity.content_id.clone()),
            identity: Some(identity),
            thumbnail_url: None,
            document_id: format!("v{}", request.sequence),
            focused: true,
            visible: true,
            playback_ready,
            evidence: None,
            issue: None,
        };
        {
            let mut state = self.lock();
            state.companion.sequence = Some(request.sequence);
            state.companion.browser = Some("browser".into());
            state.provider_chooser.reconcile(vec![tab]);
        }
        let reply = serde_json::json!({
            "kind": kind,
            "ok": true,
            "contentReady": self.lock().provider_chooser.content_ready,
            "title": request.title.clone(),
        });
        serde_json::to_vec(&reply)
            .unwrap_or_else(|_| refuse(kind, CompanionErrorCode::MalformedFrame))
    }
}

const HOST_FILE_NAME: &str = "desktop.movieparty.companion";

fn refused_capabilities() -> Vec<RefusedCapability> {
    [Capability::RequestNavigation, Capability::ControlPlayback]
        .iter()
        .map(|capability| RefusedCapability {
            capability: capability.as_str().to_string(),
            reason: capability.reason_when_disabled().to_string(),
        })
        .collect()
}

/// Every browser directory the host manifest has to exist in. The companion is
/// documented for Chrome and Edge, so installing for only one of them would be a
/// silent half-support.
fn companion_host_directories() -> Result<Vec<std::path::PathBuf>, String> {
    #[cfg(target_os = "macos")]
    let home: Option<std::path::PathBuf> = std::env::var_os("HOME").map(std::path::PathBuf::from);
    #[cfg(target_os = "windows")]
    let home: Option<std::path::PathBuf> =
        std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let home: Option<std::path::PathBuf> = None;

    let base = home.ok_or("Could not find the browser profile directory".to_string())?;
    #[cfg(target_os = "macos")]
    let vendors = ["Google/Chrome", "Microsoft Edge"];
    #[cfg(target_os = "windows")]
    let vendors = [r"Google\Chrome", r"Microsoft\Edge"];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = base;
        return Err(
            "The companion is only supported in Chrome and Edge on Windows and macOS".into(),
        );
    }
    Ok(vendors
        .iter()
        .map(|vendor| base.join(vendor).join("NativeMessagingHosts"))
        .collect())
}

fn write_companion(stream: &mut TcpStream, body: &[u8]) -> std::io::Result<()> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    stream.write_all(&out)?;
    stream.flush()
}

fn refuse(kind: &str, error: CompanionErrorCode) -> Vec<u8> {
    // Naming the capability is what lets the extension switch off one control
    // and keep the rest, instead of guessing from an error string.
    serde_json::to_vec(&CompanionResponse::refused(
        kind,
        error,
        companion::host::capability_for(kind),
    ))
    .unwrap_or_else(|_| br#"{"kind":"","ok":false}"#.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_runtime::AppRuntime;

    /// A runtime with the companion switched on and a provider already chosen.
    fn companion_ready(provider: &str) -> (AppRuntime, String) {
        let runtime = AppRuntime::new();
        let port = runtime.companion_start().expect("listener");
        assert!(port > 0);
        runtime.lock().provider.provider_id = Some(provider.into());
        let code = runtime
            .lock()
            .companion
            .pairing
            .clone()
            .expect("pairing")
            .code
            .clone();
        (runtime, code)
    }

    fn paired(runtime: &AppRuntime, code: &str) -> ([u8; 32], u64) {
        let hello = serde_json::json!({
            "kind": "handshake",
            "sequence": 1,
            "version": PROTOCOL_VERSION,
            "code": code,
        });
        let reply = runtime.handle_companion_frame(&serde_json::to_vec(&hello).unwrap());
        let value: serde_json::Value = serde_json::from_slice(&reply).unwrap();
        assert_eq!(value["ok"], serde_json::Value::Bool(true), "{value}");
        let secret = URL_SAFE_NO_PAD
            .decode(value["secret"].as_str().expect("secret"))
            .expect("key bytes");
        let secret: [u8; 32] = secret.try_into().expect("32-byte key");
        (secret, 1)
    }

    fn command(
        runtime: &AppRuntime,
        secret: &[u8; 32],
        sequence: u64,
        kind: &str,
        fields: serde_json::Value,
    ) -> serde_json::Value {
        let mut message = serde_json::json!({
            "kind": kind,
            "sequence": sequence,
            "version": PROTOCOL_VERSION,
            "token": companion::host::token_for(secret, sequence, kind),
        });
        if let Some(map) = fields.as_object() {
            for (key, value) in map {
                message[key] = value.clone();
            }
        }
        let reply = runtime.handle_companion_frame(&serde_json::to_vec(&message).unwrap());
        serde_json::from_slice(&reply).expect("a json reply")
    }

    #[test]
    fn the_companion_is_off_until_started_and_leaves_nothing_behind() {
        let runtime = AppRuntime::new();
        let before = runtime.companion_status();
        assert!(!before.listening);
        assert!(!before.paired);
        assert_eq!(before.port, 0);
        // Even a frame that cannot be understood answers in words, not silence.
        let reply = serde_json::from_slice::<serde_json::Value>(
            &runtime.handle_companion_frame(b"{not json"),
        )
        .unwrap();
        assert_eq!(reply["ok"], serde_json::Value::Bool(false));
        assert!(
            reply["guidance"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .count()
                > 3
        );

        let (runtime, _) = companion_ready("netflix");
        assert!(runtime.companion_status().listening);
        // The granted set is a promise about evidence, not about what a button
        // could do, so the two that have never been observed must be refused.
        let status = runtime.companion_status();
        assert_eq!(
            status.granted,
            vec![
                "identify_tab".to_string(),
                "report_content".to_string(),
                "report_playback".to_string()
            ]
        );
        assert_eq!(status.refused.len(), 2);
        runtime.companion_stop();
        let after = runtime.companion_status();
        assert!(!after.listening);
        assert!(!after.paired);
        assert!(after.pairing_code.is_none());
    }

    #[test]
    fn only_the_right_code_exchanges_for_a_session_key() {
        let (runtime, code) = companion_ready("netflix");
        let hello = |code: Option<&str>, sequence: u64| {
            let mut message =
                serde_json::json!({"kind": "handshake", "sequence": sequence, "version": 1});
            if let Some(code) = code {
                message["code"] = serde_json::Value::String(code.into());
            }
            runtime.handle_companion_frame(&serde_json::to_vec(&message).unwrap())
        };
        let parse = |bytes: Vec<u8>| serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();

        assert_eq!(
            parse(hello(Some("WRONGCODE1"), 1))["error"],
            "PairingFailed".to_string()
        );
        assert_eq!(parse(hello(None, 2))["error"], "NotPaired".to_string());
        assert!(runtime.companion_status().pairing_code.is_some());
        let accepted = parse(hello(Some(&code), 3));
        assert_eq!(accepted["ok"], serde_json::Value::Bool(true));
        assert!(accepted["secret"].is_string());
    }

    #[test]
    fn a_guessed_code_is_cut_off_rather_than_offered_forever() {
        let (runtime, code) = companion_ready("netflix");
        let attempt = |sequence: u64| {
            let message = serde_json::json!({
                "kind": "handshake", "sequence": sequence, "version": 1, "code": "AAAA1111"
            });
            let bytes = runtime.handle_companion_frame(&serde_json::to_vec(&message).unwrap());
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"]
                .as_str()
                .unwrap_or("")
                .to_string()
        };
        for sequence in 1..=companion::MAX_PAIRING_ATTEMPTS as u64 {
            assert_eq!(attempt(sequence), "PairingFailed", "sequence {sequence}");
        }
        assert_eq!(
            attempt(companion::MAX_PAIRING_ATTEMPTS as u64 + 1),
            "PairingFailed"
        );
        // Even the real code no longer works once the budget is spent, and a new
        // code has to be issued by the user in the app.
        assert_eq!(attempt(99), "PairingFailed".to_string());
        // The guarantee is that the code is gone, so no further guess can land.
        assert!(runtime.companion_status().pairing_code.is_none());
        let renewed = runtime.companion_new_code().expect("a fresh code");
        assert_ne!(renewed, code);
        assert_eq!(renewed.len(), 8);
    }

    #[test]
    fn commands_need_a_paired_key_and_cannot_be_replayed() {
        let (runtime, code) = companion_ready("netflix");
        let unpaired = runtime.handle_companion_frame(
            &serde_json::to_vec(&serde_json::json!({
                "kind": "reportContent",
                "sequence": 1,
                "version": 1,
                "token": "not-a-real-token-but-long-enough",
                "url": "https://www.netflix.com/title/80181170",
            }))
            .unwrap(),
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&unpaired).unwrap()["error"],
            "NotPaired".to_string()
        );

        let (secret, sequence) = paired(&runtime, &code);
        let url = serde_json::json!({"url": "https://www.netflix.com/title/80181170"});
        assert!(command(
            &runtime,
            &secret,
            sequence + 1,
            "reportContent",
            url.clone()
        )["ok"]
            .as_bool()
            .unwrap());
        // The same sequence again is a replay, not a second look at the tab.
        assert_eq!(
            command(
                &runtime,
                &secret,
                sequence + 1,
                "reportContent",
                url.clone(),
            )["error"],
            "MalformedFrame".to_string()
        );
        // A token minted for one operation cannot be spent on another: the kind
        // is inside the signed challenge, so swapping operations is a signature
        // failure, not a permission lookup the attacker can race.
        let foreign = companion::sign(&secret, &format!("{}:reportContent", sequence + 2));
        let swapped = serde_json::json!({
            "kind": "identifyTab",
            "sequence": sequence + 2,
            "version": PROTOCOL_VERSION,
            "token": foreign,
            "url": "https://www.netflix.com/title/80181170",
        });
        let reply = runtime.handle_companion_frame(&serde_json::to_vec(&swapped).unwrap());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&reply).unwrap()["error"],
            "NotPaired".to_string()
        );
    }

    #[test]
    fn a_reported_provider_tab_becomes_a_real_selection() {
        let (runtime, code) = companion_ready("netflix");
        let (secret, sequence) = paired(&runtime, &code);
        let reply = command(
            &runtime,
            &secret,
            sequence + 1,
            "reportContent",
            serde_json::json!({
                "url": "https://www.netflix.com/watch/80181170?ep=70218421&t=256",
                "title": "A Real Series",
                "tabId": 12,
                "positionMs": 90_000,
                "durationMs": 1_800_000,
                "paused": false,
            }),
        );
        assert_eq!(reply["ok"], serde_json::Value::Bool(true));
        let state = runtime.lock();
        // A reported tab becomes a candidate, never an automatic choice: the host
        // still has to preview and confirm it, so nothing the browser shows can
        // start a party by itself.
        assert!(state.provider_chooser.selected.is_none());
        assert_eq!(
            state.provider_chooser.suggested_target_id.as_deref(),
            Some("companion:12")
        );
        let identity = state.provider_chooser.tabs[0]
            .identity
            .as_ref()
            .expect("identity");
        assert_eq!(identity.content_id, "80181170");
        assert_eq!(
            identity.allowed_url,
            "https://www.netflix.com/watch/80181170?ep=70218421"
        );
        assert_eq!(
            state.provider_chooser.tabs[0]
                .player
                .as_ref()
                .expect("observation")
                .position_ms,
            90_000
        );
        assert!(!state.provider_chooser.content_ready, "never confirmed");
    }

    #[test]
    fn off_allowlist_tabs_and_unsupported_providers_are_refused_nothing_stored() {
        let (runtime, code) = companion_ready("netflix");
        let (secret, sequence) = paired(&runtime, &code);
        for url in [
            "https://mail.google.com/inbox",
            "https://www.netflix.com.evil.test/title/1",
            "http://www.netflix.com/title/1",
            "file:///etc/passwd",
        ] {
            let reply = command(
                &runtime,
                &secret,
                sequence + 1,
                "reportContent",
                serde_json::json!({ "url": url }),
            );
            assert_eq!(
                reply["error"],
                "OriginNotAllowed".to_string(),
                "accepted {url}"
            );
            assert_eq!(runtime.lock().provider_chooser.tabs.len(), 0, "{url}");
            assert!(
                !serde_json::to_string(&reply).unwrap().contains(url),
                "echoed {url}"
            );
        }
        // A provider nobody chose must not be able to start a selection either.
        runtime.lock().provider.provider_id = None;
        let reply = command(
            &runtime,
            &secret,
            sequence + 2,
            "reportContent",
            serde_json::json!({ "url": "https://www.netflix.com/title/80181170" }),
        );
        assert_eq!(reply["error"], "NotPaired".to_string());
    }

    #[test]
    fn capabilities_that_were_never_observed_stay_switched_off() {
        let (runtime, code) = companion_ready("youtube");
        let (secret, sequence) = paired(&runtime, &code);
        for (kind, capability) in [
            ("controlPlayback", "control_playback"),
            ("requestNavigation", "request_navigation"),
        ] {
            let reply = command(
                &runtime,
                &secret,
                sequence + 1,
                kind,
                serde_json::json!({"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"}),
            );
            assert_eq!(
                reply["error"],
                "CapabilityDisabled".to_string(),
                "{kind} was allowed"
            );
            assert_eq!(reply["capability"], capability.to_string());
            assert!(reply["guidance"].as_str().unwrap().contains("switched off"));
        }
    }

    #[test]
    fn frames_the_bridge_cannot_understand_are_discarded() {
        let (runtime, _) = companion_ready("netflix");
        for bytes in [
            b"".to_vec(),
            b"{not json".to_vec(),
            serde_json::to_vec(&serde_json::json!({"sequence": 1})).unwrap(),
            serde_json::to_vec(&serde_json::json!({"kind": "readCookies", "sequence": 1})).unwrap(),
            vec![0xff, 0xfe, 0x00],
        ] {
            let reply = runtime.handle_companion_frame(&bytes);
            let value = serde_json::from_slice::<serde_json::Value>(&reply).unwrap();
            assert_eq!(value["ok"], serde_json::Value::Bool(false), "{bytes:?}");
        }
    }

    #[test]
    fn the_listener_speaks_the_bridge_framing_over_real_loopback() {
        let (runtime, code) = companion_ready("netflix");
        let port = runtime.companion_status().port;
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream =
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(2))
                .expect("loopback connect");
        let message = serde_json::to_vec(&serde_json::json!({
            "kind": "handshake",
            "sequence": 1,
            "version": PROTOCOL_VERSION,
            "code": code,
        }))
        .unwrap();
        let mut request = Vec::from((message.len() as u32).to_le_bytes());
        request.extend_from_slice(&message);
        stream.write_all(&request).expect("write");
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).expect("reply header");
        let length = u32::from_le_bytes(header) as usize;
        let mut body = vec![0u8; length];
        stream.read_exact(&mut body).expect("reply body");
        let reply: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(reply["ok"], serde_json::Value::Bool(true));
        assert!(reply["capabilities"].is_array());
        assert!(!reply["refused"].as_array().unwrap().is_empty());

        runtime.companion_stop();
        // Stopping really stops: nothing is left listening on that port.
        assert!(std::net::TcpStream::connect_timeout(
            &address,
            std::time::Duration::from_millis(400)
        )
        .is_err());
    }

    #[test]
    fn unlinking_stops_a_left_behind_extension_from_keep_reporting() {
        let (runtime, code) = companion_ready("netflix");
        let (secret, sequence) = paired(&runtime, &code);
        assert!(runtime.companion_status().paired);
        let status = runtime.companion_unlink();
        assert!(!status.paired);
        let reply = command(
            &runtime,
            &secret,
            sequence + 1,
            "reportContent",
            serde_json::json!({"url": "https://www.netflix.com/title/80181170"}),
        );
        assert_eq!(reply["error"], "NotPaired".to_string());
        runtime.companion_stop();
    }

    #[test]
    fn the_host_is_registered_for_every_supported_browser_not_just_one() {
        let directories = companion_host_directories().expect("browser directories");
        assert!(
            directories.len() >= 2,
            "Chrome and Edge must both be covered"
        );
        let joined = directories
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(joined.contains("NativeMessagingHosts"), "{joined}");
        assert!(joined.contains("Chrome"), "{joined}");
        assert!(joined.contains("Edge"), "{joined}");
    }

    #[test]
    fn the_host_manifest_never_trusts_more_than_one_extension_id() {
        let (path, json) = AppRuntime::companion_host_manifest(&"k".repeat(32)).expect("manifest");
        assert!(
            path.ends_with("desktop.movieparty.companion.json"),
            "{path}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["allowed_origins"].as_array().unwrap().len(), 1);
        assert_eq!(value["name"], "desktop.movieparty.companion".to_string());
        for bogus in ["", &"k".repeat(31), "*", "chrome-extension://x/"] {
            assert!(
                AppRuntime::companion_host_manifest(bogus).is_err(),
                "accepted {bogus}"
            );
        }
    }
}
