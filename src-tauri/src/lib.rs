pub mod app_runtime;
pub mod call;
pub mod capture;
pub mod chat;
pub mod encode;
pub mod identity;
pub mod media;
pub mod network;
pub mod notifications;
pub mod privacy;
pub mod protocol;
pub mod providers;
pub mod resilience;
pub mod room;
pub mod scheduling;
pub mod secure;
pub mod storage;
pub mod sync;
pub mod telemetry;

pub const APP_NAME: &str = "Movie Party";
pub const PROTOCOL_MAJOR: u16 = 1;
pub const PROTOCOL_MINOR: u16 = 0;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

const DEEP_LINK_OPENED_EVENT: &str = "deep_link_opened";

#[derive(serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum TailscaleSetupAction {
    Install,
    OpenApp,
    PartnerHelp,
}

#[derive(Default)]
struct PendingDeepLinks(Mutex<Vec<String>>);

impl PendingDeepLinks {
    fn push(&self, url: String) {
        let mut links = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        links.push(url);
    }

    fn take(&self) -> Vec<String> {
        let mut links = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *links)
    }
}

pub fn run() {
    // MASTER_PRD §87: structured logs at INFO by default, local stdout only.
    // RUST_LOG overrides; nothing is ever sent to a cloud service.
    telemetry::init_local_logging();
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {}))
        .manage(app_runtime::AppRuntime::new_without_emitter())
        .manage(PendingDeepLinks::default())
        .manage(media::player::native_surface::NativeVideoSurfaceState::default())
        .setup(|app| {
            // M4: Initialize the SQLite database on startup, restore identity,
            // detect overdue preloads (notification-only — never consumes
            // status), install the real preload executor, and start the
            // scheduler worker.
            let runtime = app.state::<app_runtime::AppRuntime>();
            runtime.init_db();
            runtime.check_overdue_schedules();
            runtime.setup_real_preload_executor();
            runtime.spawn_scheduler_worker();

            let pending_links = app.state::<PendingDeepLinks>();
            for arg in std::env::args().skip(1) {
                if is_movie_party_deep_link(&arg) {
                    pending_links.push(arg.trim().to_string());
                }
            }

            #[cfg(windows)]
            if let Err(error) = app.deep_link().register_all() {
                tracing::warn!(error = %error, "Movie Party could not register the Windows invite protocol");
            }

            let app_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let value = url.to_string();
                    if is_movie_party_deep_link(&value) {
                        let _ = app_handle.emit(DEEP_LINK_OPENED_EVENT, value);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_metadata,
            get_prerequisite_statuses,
            take_pending_deep_links,
            get_app_snapshot,
            get_tailscale_readiness,
            open_tailscale_setup,
            show_home,
            show_join_party,
            request_end_party,
            init_listener,
            create_local_party,
            get_provider_capabilities,
            join_party,
            mark_ready,
            enter_cinema,
            request_play_countdown,
            pause_playback,
            resume_playback,
            seek_relative,
            handle_failure_event,
            leave_party,
            send_chat_message,
            send_reaction,
            set_shared_controls,
            report_buffer_status,
            set_call_mode,
            submit_call_signal,
            set_microphone_enabled,
            set_camera_enabled,
            set_privacy_mode,
            set_ghost_mode,
            attach_native_video_surface,
            resize_native_video_surface,
            detach_native_video_surface,
            pick_media_file,
            launch_provider,
            launch_generic_link,
            open_provider_browser,
            check_provider_status,
            navigate_provider_title,
            create_schedule,
            create_and_broadcast_schedule,
            update_and_broadcast_schedule,
            cancel_and_broadcast_schedule,
            guest_accept_schedule,
            list_schedules,
            update_schedule_media,
            update_schedule_preload,
            delete_schedule,
            retention_keep,
            retention_remove,
            retention_save_as,
        ])
        .build(tauri::generate_context!());

    match result {
        Ok(app) => app.run(|app_handle, event| {
            // Graceful teardown on app exit: close the managed Chrome
            // session (Browser.close flushes the provider profile) before
            // the process dies. Without this, the Chrome child is either
            // orphaned or SIGKILL'd by the OS, leaving the dedicated
            // profile in a dirty state — the "Chrome exits unexpectedly"
            // symptom on the next launch.
            if matches!(event, tauri::RunEvent::Exit) {
                let runtime = app_handle.state::<app_runtime::AppRuntime>();
                runtime.close_provider_session();
            }
        }),
        Err(error) => {
            eprintln!("Movie Party failed to start: {error}");
            std::process::exit(1);
        }
    }
}

fn is_movie_party_deep_link(value: &str) -> bool {
    value
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("movieparty://")
}

#[tauri::command]
fn init_listener(app: tauri::AppHandle, runtime: tauri::State<'_, app_runtime::AppRuntime>) {
    let sink = app_runtime::TauriSink::new(app);
    runtime.swap_emitter(Some(std::sync::Arc::new(sink)));
}

#[tauri::command]
fn app_metadata() -> AppMetadata {
    AppMetadata {
        app_name: APP_NAME,
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
    }
}

#[tauri::command]
fn take_pending_deep_links(pending_links: tauri::State<'_, PendingDeepLinks>) -> Vec<String> {
    pending_links.take()
}

#[tauri::command]
fn get_app_snapshot(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::AppSnapshot {
    runtime.snapshot()
}

#[tauri::command]
async fn get_tailscale_readiness() -> crate::network::tailscale::TailscaleReadiness {
    crate::network::tailscale::local_readiness().await
}

#[tauri::command]
async fn open_tailscale_setup(action: TailscaleSetupAction) -> Result<(), String> {
    match action {
        TailscaleSetupAction::OpenApp => {
            crate::network::tailscale::open_tailscale_app().map_err(|error| error.to_string())
        }
        TailscaleSetupAction::Install => open_external_url("https://tailscale.com/download"),
        TailscaleSetupAction::PartnerHelp => {
            open_external_url("https://tailscale.com/kb/1084/sharing")
        }
    }
}

fn open_external_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result: Result<std::process::Child, std::io::Error> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "external setup actions are unsupported on this platform",
    ));
    result
        .map(|_| ())
        .map_err(|error| format!("MP-NET-TS-003 could not open Tailscale setup: {error}"))
}

#[tauri::command]
fn show_home(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.return_home()
}

#[tauri::command]
fn show_join_party(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.show_join_party()
}

#[tauri::command]
fn request_end_party(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::AppSnapshot {
    runtime.request_end_party()
}

#[tauri::command]
async fn create_local_party(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    media_path: Option<String>,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.create_local_party(media_path).await
}

#[tauri::command]
fn get_provider_capabilities() -> Vec<crate::providers::sync::ProviderCapability> {
    crate::providers::sync::provider_capabilities()
}

#[tauri::command]
async fn join_party(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    invite_code: String,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.join_party(invite_code).await
}

#[tauri::command]
fn mark_ready(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.set_ready()
}

#[tauri::command]
fn request_play_countdown(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::AppSnapshot {
    runtime.request_play_countdown()
}

#[tauri::command]
fn enter_cinema(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.enter_cinema()
}

#[tauri::command]
fn attach_native_video_surface(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    surface: tauri::State<'_, media::player::native_surface::NativeVideoSurfaceState>,
    bounds: media::player::native_surface::NativeVideoBounds,
) -> Result<app_runtime::AppSnapshot, String> {
    let handle = surface
        .attach(&app, bounds)
        .map_err(|error| error.to_string())?;
    Ok(runtime.attach_native_video_surface(handle))
}

#[tauri::command]
fn resize_native_video_surface(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    surface: tauri::State<'_, media::player::native_surface::NativeVideoSurfaceState>,
    bounds: media::player::native_surface::NativeVideoBounds,
) -> Result<app_runtime::AppSnapshot, String> {
    let handle = surface
        .attach(&app, bounds)
        .map_err(|error| error.to_string())?;
    Ok(runtime.attach_native_video_surface(handle))
}

#[tauri::command]
fn detach_native_video_surface(
    app: tauri::AppHandle,
    surface: tauri::State<'_, media::player::native_surface::NativeVideoSurfaceState>,
) -> Result<(), String> {
    surface.detach(&app).map_err(|error| error.to_string())
}

#[tauri::command]
fn pause_playback(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.pause_playback()
}

#[tauri::command]
fn resume_playback(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.resume_playback()
}

#[tauri::command]
fn seek_relative(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    delta_ms: i64,
) -> app_runtime::AppSnapshot {
    runtime.seek_relative(delta_ms)
}

#[tauri::command]
fn handle_failure_event(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    event: resilience::FailureEvent,
) -> app_runtime::AppSnapshot {
    runtime.handle_failure_event(event)
}

#[tauri::command]
fn leave_party(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.leave_party()
}

#[tauri::command]
fn send_chat_message(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    body: String,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.send_chat_message(body)
}

#[tauri::command]
fn send_reaction(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    reaction: String,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.send_reaction(reaction)
}

#[tauri::command]
fn set_shared_controls(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    enabled: bool,
) -> app_runtime::AppSnapshot {
    runtime.set_shared_controls(enabled)
}

#[tauri::command]
fn report_buffer_status(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    position_ms: u64,
    buffer_ahead_ms: u64,
    stalled: bool,
) -> app_runtime::AppSnapshot {
    runtime.report_buffer_status(position_ms, buffer_ahead_ms, stalled)
}

#[tauri::command]
fn set_call_mode(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    mode: call::CallMode,
) -> app_runtime::AppSnapshot {
    runtime.set_call_mode(mode)
}

#[tauri::command]
fn submit_call_signal(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    signal: call::CallSignal,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.submit_call_signal(signal)
}

#[tauri::command]
fn set_microphone_enabled(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    enabled: bool,
) -> app_runtime::AppSnapshot {
    runtime.set_microphone_enabled(enabled)
}

#[tauri::command]
fn set_camera_enabled(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    enabled: bool,
) -> app_runtime::AppSnapshot {
    runtime.set_camera_enabled(enabled)
}

#[tauri::command]
fn set_privacy_mode(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    enabled: bool,
) -> app_runtime::AppSnapshot {
    runtime.set_privacy_mode(enabled)
}

#[tauri::command]
fn set_ghost_mode(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    enabled: bool,
) -> app_runtime::AppSnapshot {
    runtime.set_ghost_mode(enabled)
}

#[tauri::command]
fn pick_media_file() -> Result<String, String> {
    let dialog = rfd::FileDialog::new()
        .set_title("Choose a movie file")
        .add_filter(
            "Media Files",
            &["mp4", "mkv", "webm", "avi", "mov", "m4v", "ts", "flv"],
        );

    match dialog.pick_file() {
        Some(path) => path
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Path is not valid UTF-8".to_string()),
        None => Err("File selection was cancelled".to_string()),
    }
}

#[tauri::command]
fn launch_provider(
    provider_id: String,
    url: String,
    mode: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    use crate::providers::sync::{provider_accepts_url, provider_id_from_str};

    if mode == "PROVIDER_SHARED" {
        return Err(
            "MP-CAPTURE-001 Provider Shared is experimental and unavailable until capture is verified on this device."
                .to_string(),
        );
    }
    if mode != "PROVIDER_SYNC" {
        return Err("MP-PROVIDER-002 unsupported provider mode".to_string());
    }

    let provider = provider_id_from_str(&provider_id)
        .ok_or_else(|| "MP-PROVIDER-002 unsupported provider".to_string())?;

    // Reuse an existing managed session for the same provider when the
    // browser is already open and ready; otherwise launch a fresh one.
    if runtime.session_matches_provider(&provider_id) {
        runtime.validate_provider_ready_for_room()?;
        let snapshot = runtime.attach_launched_provider(provider_id.clone(), url);
        return Ok(snapshot);
    }

    if !provider_accepts_url(provider, &url) {
        return Err("MP-PROVIDER-002 provider URL mismatch".to_string());
    }
    let session = launch_provider_chrome(&runtime, &provider_id, &url)?;
    Ok(runtime.store_launched_provider(provider_id, url, session))
}

/// Finds Chrome, allocates a local CDP port, and launches a fresh managed
/// browser session for a provider URL. Closes any previous provider session.
fn launch_provider_chrome(
    runtime: &app_runtime::AppRuntime,
    provider_id: &str,
    url: &str,
) -> Result<crate::providers::chrome::ManagedChromeSession, String> {
    use crate::providers::chrome::{
        allocate_local_cdp_port, build_launch_plan, default_chrome_candidates, find_chrome,
        launch_managed_chrome,
    };

    let chrome_path = find_chrome(&default_chrome_candidates()).map_err(|e| e.to_string())?;
    let profiles_root = std::env::temp_dir().join("MoviePartyProfiles");
    let cdp_port = allocate_local_cdp_port().map_err(|e| e.to_string())?;
    let plan = build_launch_plan(chrome_path, &profiles_root, provider_id, cdp_port, url)
        .map_err(|e| e.to_string())?;
    runtime.close_provider_session();
    launch_managed_chrome(plan).map_err(|e| e.to_string())
}

/// Opens the selected provider's home page in the managed browser so the user
/// can authenticate on the provider's own page. Reuses an existing session
/// for the same provider instead of spawning a duplicate browser process.
#[tauri::command]
fn open_provider_browser(
    provider_id: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    use crate::providers::sync::{provider_home_url, provider_id_from_str};

    let provider = provider_id_from_str(&provider_id)
        .ok_or_else(|| "MP-PROVIDER-002 unsupported provider".to_string())?;
    let url = provider_home_url(provider).to_string();

    // Reuse the existing session when the browser is already running for this
    // provider; never spawn a duplicate browser process for the same session.
    if runtime.session_matches_provider(&provider_id) {
        let (login_required, media_detected) =
            runtime.detect_provider_session_status(&provider_id)?;
        let readiness =
            crate::providers::sync::readiness_from_detection(login_required, media_detected);
        return Ok(runtime.update_provider_readiness(readiness));
    }

    let session = launch_provider_chrome(&runtime, &provider_id, &url)?;
    Ok(runtime.open_provider_browser(provider_id, session))
}

/// Checks the current provider session's login and media state over CDP and
/// updates the truthful readiness. Never inspects or exposes credentials.
#[tauri::command]
fn check_provider_status(
    provider_id: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    if !runtime.session_matches_provider(&provider_id) {
        return Ok(runtime
            .update_provider_readiness(crate::providers::sync::ProviderReadiness::NotStarted));
    }
    let (login_required, media_detected) = runtime.detect_provider_session_status(&provider_id)?;
    let readiness =
        crate::providers::sync::readiness_from_detection(login_required, media_detected);
    Ok(runtime.update_provider_readiness(readiness))
}

/// Navigates the managed provider browser to the provider's own search page
/// for a user-entered title. The provider remains authoritative for catalogue
/// results; Movie Party never scrapes the catalogue.
#[tauri::command]
fn navigate_provider_title(
    provider_id: String,
    title: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    use crate::providers::sync::{provider_id_from_str, provider_search_url};

    let provider = provider_id_from_str(&provider_id)
        .ok_or_else(|| "MP-PROVIDER-002 unsupported provider".to_string())?;
    if !runtime.session_matches_provider(&provider_id) {
        return Err(
            "MP-PROVIDER-003 open the provider browser before choosing a title".to_string(),
        );
    }
    let url = provider_search_url(provider, &title)
        .ok_or_else(|| "MP-PROVIDER-003 enter a movie or show title".to_string())?;

    runtime.navigate_provider_to(&provider_id, &url)?;
    Ok(runtime.update_provider_readiness(crate::providers::sync::ProviderReadiness::Navigating))
}

#[tauri::command]
fn launch_generic_link(
    url: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    use crate::providers::chrome::{
        allocate_local_cdp_port, build_launch_plan, default_chrome_candidates, find_chrome,
        launch_managed_chrome,
    };

    if !crate::providers::sync::generic_link_accepts_url(&url) {
        return Err("MP-PROVIDER-002 invalid third-party link".to_string());
    }

    let chrome_path = match find_chrome(&default_chrome_candidates()) {
        Ok(path) => path,
        Err(error) => return Ok(runtime.generic_link_unavailable(url, error.to_string())),
    };
    let profiles_root = std::env::temp_dir().join("MoviePartyProfiles");
    let cdp_port = match allocate_local_cdp_port() {
        Ok(port) => port,
        Err(error) => return Ok(runtime.generic_link_unavailable(url, error.to_string())),
    };
    let plan = match build_launch_plan(chrome_path, &profiles_root, "generic-link", cdp_port, &url)
    {
        Ok(plan) => plan,
        Err(error) => return Ok(runtime.generic_link_unavailable(url, error.to_string())),
    };
    runtime.close_provider_session();
    let session = match launch_managed_chrome(plan) {
        Ok(session) => session,
        Err(error) => return Ok(runtime.generic_link_unavailable(url, error.to_string())),
    };

    Ok(runtime.store_launched_generic_link(url, session))
}

#[tauri::command]
fn create_schedule(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    room_id: String,
    media_id: String,
    scheduled_start_utc_ms: i64,
    planned_preload_utc_ms: i64,
    guest_device_id: String,
) -> Result<String, String> {
    runtime.create_schedule(
        &room_id,
        &media_id,
        scheduled_start_utc_ms,
        planned_preload_utc_ms,
        &guest_device_id,
    )
}

#[tauri::command]
fn create_and_broadcast_schedule(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    room_id: String,
    media_id: String,
    scheduled_start_utc_ms: i64,
    planned_preload_utc_ms: i64,
    guest_device_id: String,
    call_mode: String,
) -> Result<String, String> {
    runtime.create_and_broadcast_schedule(
        &room_id,
        &media_id,
        scheduled_start_utc_ms,
        planned_preload_utc_ms,
        &guest_device_id,
        &call_mode,
    )
}

#[tauri::command]
fn update_and_broadcast_schedule(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    schedule_id: String,
    media_id: String,
    planned_preload_utc_ms: i64,
    scheduled_start_utc_ms: i64,
) -> Result<(), String> {
    runtime.update_and_broadcast_schedule_media(
        &schedule_id,
        &media_id,
        planned_preload_utc_ms,
        scheduled_start_utc_ms,
    )
}

#[tauri::command]
fn cancel_and_broadcast_schedule(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    schedule_id: String,
) -> Result<(), String> {
    runtime.cancel_and_broadcast_schedule(&schedule_id)
}

#[tauri::command]
fn guest_accept_schedule(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    schedule_id: String,
    accepted: bool,
) -> app_runtime::AppSnapshot {
    runtime.guest_accept_schedule(&schedule_id, accepted)
}

/// Batch 15 (UI_UX_SPEC §11): truthful prerequisite statuses for First Run.
/// Detection lives in Rust (AGENTS §6 — native detection is backend work);
/// permissions are reported as NOT_REQUESTED unless the call-mode has
/// already exercised them (no premature prompts, §11 rule).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrerequisiteStatus {
    pub id: String,
    pub label: String,
    pub state: String, // "OK" | "NOT_REQUESTED" | "MISSING" | "OPTIONAL"
    pub detail: String,
}

#[tauri::command]
fn get_prerequisite_statuses() -> Vec<PrerequisiteStatus> {
    let libmpv = crate::media::player::detect_libmpv();
    let chrome = crate::providers::chrome::find_chrome(
        &crate::providers::chrome::default_chrome_candidates(),
    );
    vec![
        PrerequisiteStatus {
            id: "libmpv".to_string(),
            label: "Movie player (libmpv)".to_string(),
            state: if libmpv.available {
                "OK".into()
            } else {
                "MISSING".into()
            },
            detail: if libmpv.available {
                "Player runtime found".to_string()
            } else {
                "Bundled player runtime not found — local playback needs it".to_string()
            },
        },
        PrerequisiteStatus {
            id: "chrome".to_string(),
            label: "Google Chrome".to_string(),
            state: if chrome.is_ok() {
                "OK".into()
            } else {
                "MISSING".into()
            },
            detail: if chrome.is_ok() {
                "Provider browser found".to_string()
            } else {
                "Needed for Netflix/Prime/Hotstar sync mode".to_string()
            },
        },
        PrerequisiteStatus {
            id: "camera".to_string(),
            label: "Camera".to_string(),
            state: "NOT_REQUESTED".to_string(),
            detail: "Requested when you enable the video call".to_string(),
        },
        PrerequisiteStatus {
            id: "microphone".to_string(),
            label: "Microphone".to_string(),
            state: "NOT_REQUESTED".to_string(),
            detail: "Requested when you enable the call".to_string(),
        },
        PrerequisiteStatus {
            id: "notifications".to_string(),
            label: "Notifications".to_string(),
            state: "NOT_REQUESTED".to_string(),
            detail: "Enable for preload reminders".to_string(),
        },
        PrerequisiteStatus {
            id: "screen".to_string(),
            label: "Screen Recording".to_string(),
            state: "OPTIONAL".to_string(),
            detail: "Only needed for Shared Mode".to_string(),
        },
    ]
}

#[tauri::command]
fn list_schedules(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<Vec<crate::storage::sqlite::StoredSchedule>, String> {
    runtime.list_schedules()
}

#[tauri::command]
fn update_schedule_media(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    schedule_id: String,
    media_id: String,
) -> Result<(), String> {
    runtime.update_schedule_media(&schedule_id, &media_id)
}

#[tauri::command]
fn update_schedule_preload(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    schedule_id: String,
    planned_preload_utc_ms: i64,
) -> Result<(), String> {
    runtime.update_schedule_preload(&schedule_id, planned_preload_utc_ms)
}

#[tauri::command]
fn delete_schedule(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    schedule_id: String,
) -> Result<(), String> {
    runtime.delete_schedule(&schedule_id)
}

#[tauri::command]
fn retention_keep(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    media_id: String,
) -> Result<(), String> {
    runtime.retention_keep(&media_id)
}

#[tauri::command]
fn retention_remove(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    media_id: String,
) -> Result<(), String> {
    runtime.retention_remove(&media_id)
}

#[tauri::command]
fn retention_save_as(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    media_id: String,
    destination: String,
) -> Result<std::path::PathBuf, String> {
    runtime.retention_save_as(&media_id, std::path::Path::new(&destination))
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
struct AppMetadata {
    app_name: &'static str,
    protocol_major: u16,
    protocol_minor: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_protocol_v1() {
        let metadata = app_metadata();

        assert_eq!(metadata.app_name, APP_NAME);
        assert_eq!(metadata.protocol_major, 1);
        assert_eq!(metadata.protocol_minor, 0);
    }
}
