pub mod app_runtime;
pub mod call;
pub mod capture;
pub mod chat;
pub mod companion;
pub mod encode;
pub mod friends;
pub mod identity;
pub mod media;
pub mod network;
pub mod notifications;
pub mod onboarding;
pub mod privacy;
pub mod process;
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

#[derive(Default)]
struct ChromeOperations(Mutex<()>);

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
        .manage(ChromeOperations::default())
        .manage(media::player::native_surface::NativeVideoSurfaceState::default())
        .setup(|app| {
            // M4: Initialize the SQLite database on startup, restore identity,
            // detect overdue preloads (notification-only — never consumes
            // status), install the real preload executor, and start the
            // scheduler worker.
            let runtime = app.state::<app_runtime::AppRuntime>();
            runtime.init_db();
            let provider_app = app.handle().clone();
            runtime.set_room_browser_launcher(std::sync::Arc::new(move |provider, url| {
                let operations = provider_app.state::<ChromeOperations>();
                let _operation = operations.0.lock().unwrap_or_else(|e| e.into_inner());
                use crate::providers::chrome::{find_chrome, default_chrome_candidates, build_launch_plan, launch_managed_chrome};
                let root = prepare_managed_profile(&provider_app, provider)?;
                let executable = find_chrome(&default_chrome_candidates()).map_err(|e| e.to_string())?;
                let plan = build_launch_plan(executable, &root, provider, url).map_err(|e| e.to_string())?;
                launch_managed_chrome(plan).map_err(|e| e.to_string())
            }));

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
            tailnet_peers,
            remove_friend,
            list_friends,
            verify_friend,
            refresh_friend_states,
            rename_friend,
            show_home,
            show_join_party,
            request_end_party,
            init_listener,
            create_local_party,
            get_provider_capabilities,
            join_party,
            mark_ready,
            set_display_name,
            back_to_lobby,
            enter_cinema,
            request_play_countdown,
            continue_without_guest,
            run_provider_shared_diagnostic,
            list_provider_diagnostics,
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
            pick_save_folder,
            launch_provider,
            launch_generic_link,
            open_provider_browser,
            check_provider_status,
            navigate_provider_title,
            get_provider_content,
            confirm_provider_content,
            browse_provider_content,
            validate_provider_content,
            retry_room_provider,
            youtube_driver, youtube_attach, youtube_detach, youtube_event,
            companion_status, companion_start, companion_stop, companion_new_code,
            companion_unlink, companion_install_host, companion_remove_host,
            friend_invite_signed, preview_friend_invite, accept_friend_invitation,
            friendship_states,
            decline_friend_invitation, block_friend, unblock_friend, remove_friend_trust,
            friendship_state,
            onboarding_begin, onboarding_cancel, onboarding_view, onboarding_set_side,
            onboarding_choose_method, onboarding_open_admin, onboarding_paste_share_link,
            onboarding_open_share_link, onboarding_report_link_expired, onboarding_refresh,
            onboarding_check_reachability, onboarding_invitation_package,
            focus_provider_playback,
            reset_provider_session,
            create_schedule,
            create_and_broadcast_schedule,
            update_and_broadcast_schedule,
            cancel_and_broadcast_schedule,
            guest_accept_schedule,
            invite_qr_svg,
            list_schedules,
            update_schedule_media,
            update_schedule_preload,
            delete_schedule,
            retention_keep,
            retention_remove,
            retention_save_as,
        ])
        .build(tauri::generate_context!());

    // MP-23: Chrome cleanup used to run only from `RunEvent::Exit`, so a signal
    // or a panic could orphan a whole Chrome tree — and on macOS there is no
    // kernel-level parent-death signal to fall back on. Installing the hardening
    // before the event loop starts gives every shutdown path a way to find and
    // kill the live tree; on Windows the kill-on-close job object covers it
    // instead, so this is a no-op there.
    crate::providers::chrome::install_shutdown_hardening();

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
                if !crate::providers::chrome::wait_for_session_shutdown(
                    std::time::Duration::from_secs(25),
                ) {
                    tracing::warn!(
                        "Managed Chrome did not finish graceful shutdown within 25 seconds"
                    );
                    #[cfg(unix)]
                    crate::providers::chrome::kill_live_chrome_tree();
                }
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
        app_version: env!("CARGO_PKG_VERSION"),
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

/// Add Friend: tailnet peers (candidates) + saved friends with cached
/// verification results. Errors carry the stable MP-NET-TS codes so the UI
/// can show the appropriate Tailscale setup state.
#[tauri::command]
async fn tailnet_peers(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<crate::network::tailscale::TailnetPeersView, String> {
    runtime.tailnet_peers().await
}

/// Refresh saved friends against the live tailnet status: INVITED
/// friends whose device has since joined the tailnet are promoted to
/// TAILSCALE_JOINED (never demoting MOVIE_PARTY_VERIFIED).
#[tauri::command]
async fn refresh_friend_states(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<Vec<crate::storage::sqlite::StoredFriend>, String> {
    Ok(runtime.refresh_friend_states().await)
}

/// Rename a saved friend (friendly names over tailnet jargon).
#[tauri::command]
fn rename_friend(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
    display_name: String,
) -> Result<crate::storage::sqlite::StoredFriend, String> {
    runtime.rename_friend(&peer_key, &display_name)
}

/// Remove a saved friend.
#[tauri::command]
fn remove_friend(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
) -> Result<(), String> {
    runtime.remove_friend(&peer_key)
}

/// Saved friends with cached verification results.
#[tauri::command]
fn list_friends(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Vec<crate::storage::sqlite::StoredFriend> {
    runtime.list_friends()
}

/// Verify the real tunnel connection to a saved friend via tailscale ping
/// and persist the result. Returns the friend + the raw probe.
#[tauri::command]
async fn verify_friend(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
) -> Result<crate::network::tailscale::FriendVerification, String> {
    runtime.verify_friend(&peer_key).await
}

fn open_external_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = crate::process::quiet_command("explorer.exe")
        .arg(url)
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result: Result<std::process::Child, std::io::Error> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "external setup actions are unsupported on this platform",
    ));
    result
        .map(|mut child| {
            // F47: reap the launcher. Dropping a `Child` does not wait for it,
            // so the process stayed a zombie for the life of the app. The wait
            // runs on a detached thread so the IPC call never blocks on a slow
            // handler (a browser or the Tailscale app can take seconds to
            // start), and only the child we spawned is ever waited on.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        })
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
    // Optional so an older frontend build keeps working; absent means `false`,
    // which is the original tear-everything-down behaviour.
    keep_provider_session: Option<bool>,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime
        .create_local_party(media_path, keep_provider_session.unwrap_or(false))
        .await
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

/// Settings › General: rename this device's participant. Persists to the
/// existing identity row (device id + signing key untouched) so the peer
/// trust chain survives the rename.
#[tauri::command]
fn set_display_name(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    display_name: String,
) -> app_runtime::AppSnapshot {
    runtime.set_display_name(&display_name)
}

#[tauri::command]
fn back_to_lobby(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.back_to_lobby()
}

#[tauri::command]
fn request_play_countdown(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::AppSnapshot {
    runtime.request_play_countdown()
}

#[tauri::command]
fn continue_without_guest(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::AppSnapshot {
    runtime.continue_without_guest()
}

#[tauri::command]
fn run_provider_shared_diagnostic(
    provider_id: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<crate::storage::sqlite::StoredProviderDiagnostic, String> {
    runtime.run_provider_shared_diagnostic(provider_id)
}

#[tauri::command]
fn list_provider_diagnostics(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Vec<crate::storage::sqlite::StoredProviderDiagnostic> {
    runtime.list_provider_diagnostics()
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
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    surface: tauri::State<'_, media::player::native_surface::NativeVideoSurfaceState>,
) -> Result<app_runtime::AppSnapshot, String> {
    // Order matters: the player must drop its mpv/render contexts BEFORE
    // the surface releases the NSView, and the runtime call also refreshes
    // the snapshot the frontend receives.
    runtime.detach_native_video_surface();
    surface.detach(&app).map_err(|error| error.to_string())?;
    Ok(runtime.snapshot())
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

/// §52 Save As: pick the destination FOLDER for the exported movie.
/// The retention layer builds the final file path from the cached
/// filename — the picker returns a directory, never a file overwrite
/// decision made blind.
#[tauri::command]
fn pick_save_folder() -> Result<String, String> {
    let dialog = rfd::FileDialog::new().set_title("Choose where to save the movie");
    match dialog.pick_folder() {
        Some(path) => path
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Path is not valid UTF-8".to_string()),
        None => Err("Folder selection was cancelled".to_string()),
    }
}

#[tauri::command]
fn launch_provider(
    app: tauri::AppHandle,
    operations: tauri::State<'_, ChromeOperations>,
    provider_id: String,
    selection_key: Option<String>,
    url: String,
    mode: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    let _operation = operations.0.lock().unwrap_or_else(|e| e.into_inner());
    use crate::providers::sync::{provider_id_from_str, provider_mode_gate, ProviderModeGate};

    // F56/F57: the mode gate stays doubly enforced. The diagnostic is only
    // consulted for PROVIDER_SHARED (as before), and even a verified device is
    // then refused by the second check — the experimental path is unreachable
    // from a stable build.
    let shared_verified = mode == "PROVIDER_SHARED"
        && runtime
            .list_provider_diagnostics()
            .into_iter()
            .any(|record| record.provider_id == provider_id && record.shared_available);
    match provider_mode_gate(&mode, shared_verified) {
        ProviderModeGate::Stable => {}
        ProviderModeGate::SharedUnverified => {
            return Err(
                "MP-CAPTURE-001 Provider Shared is experimental and unavailable until the capture diagnostic is verified on this device. Run the Shared diagnostic in Settings › Providers, or use Provider Sync Mode."
                    .to_string(),
            );
        }
        ProviderModeGate::Unsupported => {
            return Err("MP-PROVIDER-002 unsupported provider mode".to_string());
        }
    }

    let provider = provider_id_from_str(&provider_id)
        .ok_or_else(|| "MP-PROVIDER-002 unsupported provider".to_string())?;

    let key = selection_key
        .as_deref()
        .ok_or("Preview and confirm content before creating a provider party.")?;
    runtime.validate_confirmed_provider_content(&provider_id, key)?;
    // Treat the observed canonical identity as authoritative, including for
    // direct IPC callers with a valid key but a different or unsafe URL.
    let url = runtime.confirmed_provider_url(&provider_id, key, &url)?;

    // Reuse an existing managed session for the same provider when the
    // browser is already open and ready; otherwise launch a fresh one.
    if runtime.session_matches_provider(&provider_id) {
        runtime.validate_provider_ready_for_room()?;
        let snapshot = runtime.attach_launched_provider(provider_id.clone(), url);
        return Ok(snapshot);
    }

    // The UI's copy of the page URL can be stale or empty by the time the user
    // presses "Create party" — see `resolve_provider_launch_url`. Fall back to
    // the page the backend itself opened for this provider rather than rejecting
    // a flow that is actually valid.
    let snapshot = runtime.snapshot();
    let url = crate::providers::sync::resolve_provider_launch_url(
        provider,
        &url,
        snapshot.provider.provider_id.as_deref(),
        snapshot.provider.url.as_deref(),
        &provider_id,
    )
    .ok_or_else(|| "MP-PROVIDER-002 provider URL mismatch".to_string())?;
    let session = launch_provider_chrome(&app, &runtime, &provider_id, &url)?;
    Ok(runtime.store_launched_provider(provider_id, url, session))
}

/// Finds Chrome, allocates a local CDP port, and launches a fresh managed
/// browser session for a provider URL. Closes any previous provider session.
fn launch_provider_chrome(
    app: &tauri::AppHandle,
    runtime: &app_runtime::AppRuntime,
    provider_id: &str,
    url: &str,
) -> Result<crate::providers::chrome::ManagedChromeSession, String> {
    use crate::providers::chrome::{
        build_launch_plan, default_chrome_candidates, find_chrome, launch_managed_chrome,
    };

    let chrome_path = find_chrome(&default_chrome_candidates()).map_err(|e| e.to_string())?;
    let profiles_root = prepare_managed_profile(app, provider_id)?;
    let plan = build_launch_plan(chrome_path, &profiles_root, provider_id, url)
        .map_err(|e| e.to_string())?;
    runtime.close_provider_session();
    launch_managed_chrome(plan).map_err(|e| e.to_string())
}

/// Stable across app updates; Chrome owns all credential encryption in this
/// dedicated non-default user-data directory. Never migrate ordinary profiles.
fn managed_profiles_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("managed-chrome").join("profiles"))
        .map_err(|e| format!("MP-PROVIDER-006 could not locate application data: {e}"))
}

fn prepare_managed_profile(
    app: &tauri::AppHandle,
    provider: &str,
) -> Result<std::path::PathBuf, String> {
    let root = managed_profiles_root(app)?;
    crate::providers::chrome::profile::migrate_legacy_profile(
        &root,
        &std::env::temp_dir().join("MoviePartyProfiles"),
        provider,
    )
    .map_err(|error| error.to_string())?;
    Ok(root)
}

#[tauri::command]
fn reset_provider_session(
    app: tauri::AppHandle,
    operations: tauri::State<'_, ChromeOperations>,
    provider_id: String,
    confirmed: bool,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    let _operation = operations.0.lock().unwrap_or_else(|e| e.into_inner());
    if !confirmed {
        return Err(crate::providers::chrome::ManagedChromeError::ConfirmationRequired.to_string());
    }
    if crate::providers::sync::provider_id_from_str(&provider_id).is_none() {
        return Err("MP-PROVIDER-002 unsupported provider".into());
    }
    runtime.prepare_provider_reset(&provider_id)?;
    crate::providers::chrome::profile::reset_profile(
        &managed_profiles_root(&app)?,
        &provider_id,
        confirmed,
    )
    .map_err(|e| e.to_string())?;
    Ok(runtime.provider_session_was_reset(&provider_id))
}

/// Opens the selected provider's home page in the managed browser so the user
/// can authenticate on the provider's own page. Reuses an existing session
/// for the same provider instead of spawning a duplicate browser process.
#[tauri::command]
fn open_provider_browser(
    app: tauri::AppHandle,
    operations: tauri::State<'_, ChromeOperations>,
    provider_id: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    let _operation = operations.0.lock().unwrap_or_else(|e| e.into_inner());
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

    let session = launch_provider_chrome(&app, &runtime, &provider_id, &url)?;
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
async fn youtube_driver(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    enabled: bool,
) -> Result<app_runtime::AppSnapshot, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            let operations = app.state::<ChromeOperations>();
            let _operation = operations.0.lock().unwrap_or_else(|e| e.into_inner());
            runtime.youtube_driver(true)
        } else {
            runtime.youtube_driver(false)
        }
    })
    .await
    .map_err(|_| "Player handoff failed".to_string())?
}
#[tauri::command]
fn youtube_attach(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::youtube::EmbedAttachment, String> {
    runtime.youtube_attach()
}
#[tauri::command]
fn youtube_detach(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    lease: String,
) -> Result<(), String> {
    runtime.youtube_detach(&lease)
}
#[tauri::command]
async fn friend_invite_signed(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::friendship::SentInvite, String> {
    runtime.friend_invite_signed().await
}

/// Read an invitation without acting on it. Nothing is committed until the user
/// accepts, so a deep link arriving from the OS cannot create a friendship.
#[tauri::command]
fn preview_friend_invite(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    link: String,
) -> Result<app_runtime::friendship::IncomingInvite, String> {
    runtime.preview_friend_invite(&link)
}

#[tauri::command]
async fn accept_friend_invitation(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    nonce: String,
) -> Result<crate::storage::sqlite::StoredFriend, String> {
    runtime.accept_friend_invitation(nonce).await
}

#[tauri::command]
fn decline_friend_invitation(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    nonce: String,
) -> Result<(), String> {
    runtime.decline_friend_invitation(nonce)
}

#[tauri::command]
fn block_friend(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
) -> Result<String, String> {
    runtime
        .block_friend(&peer_key)
        .map(|state| state.as_str().to_string())
}

#[tauri::command]
fn unblock_friend(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
) -> Result<String, String> {
    runtime
        .unblock_friend(&peer_key)
        .map(|state| state.as_str().to_string())
}

/// Removal records a position rather than deleting the record, so the revocation
/// outlives the link that created it.
#[tauri::command]
fn remove_friend_trust(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
) -> Result<String, String> {
    runtime
        .remove_friend_trust(&peer_key)
        .map(|state| state.as_str().to_string())
}

/// Every saved friend's trust position, in one call.
#[tauri::command]
fn friendship_states(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> Vec<(String, String)> {
    runtime
        .friendship_states()
        .into_iter()
        .map(|(key, state)| (key, state.as_str().to_string()))
        .collect()
}

#[tauri::command]
fn friendship_state(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
) -> Option<String> {
    runtime
        .friendship_state(&peer_key)
        .map(|state| state.as_str().to_string())
}

// ── Batch 9: guided friend + Tailscale onboarding ──────────────────────

/// Start the wizard for one peer. `my_side` decides which direction the
/// connection runs, so the sharing advice can be correct.
#[tauri::command]
fn onboarding_begin(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    peer_key: String,
    my_side: String,
) -> Result<app_runtime::onboarding::OnboardingView, String> {
    runtime.onboarding_begin(&peer_key, &my_side)
}

#[tauri::command]
fn onboarding_cancel(runtime: tauri::State<'_, app_runtime::AppRuntime>) {
    runtime.onboarding_cancel();
}

/// The current wizard state. Never fails: with no session it returns the idle
/// shape so the shell can route normally.
#[tauri::command]
fn onboarding_view(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::onboarding::OnboardingView {
    runtime
        .onboarding_view()
        .unwrap_or_else(|_| runtime.onboarding_idle_view())
}

#[tauri::command]
fn onboarding_set_side(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    my_side: String,
) -> Result<app_runtime::onboarding::OnboardingView, String> {
    runtime.onboarding_set_side(&my_side)
}

#[tauri::command]
fn onboarding_choose_method(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    method: String,
) -> Result<app_runtime::onboarding::OnboardingView, String> {
    runtime.onboarding_choose_method(&method)
}

/// Opens the official Tailscale admin page for the chosen method. Nothing is
/// created, installed or changed here — the user does that in Tailscale.
#[tauri::command]
fn onboarding_open_admin(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::onboarding::OnboardingView, String> {
    let url = runtime.onboarding_admin_url()?;
    open_external_url(&url)?;
    runtime.onboarding_view()
}

/// Pastes a share/invite link. Movie Party keeps it in memory only, returns its
/// fingerprint, and never stores or logs the token.
#[tauri::command]
fn onboarding_paste_share_link(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    link: String,
) -> Result<crate::onboarding::ShareLinkSummary, String> {
    runtime.onboarding_paste_share_link(&link)
}

/// Opens the pasted link in the user's own browser, on their explicit request.
#[tauri::command]
fn onboarding_open_share_link(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<(), String> {
    let url = runtime.onboarding_share_link_url()?;
    open_external_url(&url)
}

/// The user reports their partner's link never worked. Movie Party cannot see
/// another tailnet's shares, so this is the only honest source of that state.
#[tauri::command]
fn onboarding_report_link_expired(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::onboarding::OnboardingView, String> {
    runtime.onboarding_report_link_expired()
}

/// Re-reads the local Tailscale CLI and recomputes the wizard.
#[tauri::command]
async fn onboarding_refresh(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::onboarding::OnboardingView, String> {
    runtime.onboarding_refresh().await
}

/// A real `tailscale ping` toward the peer. Proves reachability only; "Connected"
/// still waits for a transport handshake.
#[tauri::command]
async fn onboarding_check_reachability(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::onboarding::OnboardingView, String> {
    runtime.onboarding_check_reachability().await
}

/// The copyable invitation: signed Movie Party invite + Tailscale steps. Never
/// carries the pasted share link.
#[tauri::command]
async fn onboarding_invitation_package(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<crate::onboarding::InvitationPackage, String> {
    runtime.onboarding_invitation_package().await
}

/// The opt-in existing-browser companion. Starting it writes the loopback
/// endpoint the native host reads, and nothing at all is installed here.
#[tauri::command]
fn companion_start(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::companion::CompanionStatus, String> {
    let port = runtime.companion_start()?;
    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "Could not locate Movie Party's data directory".to_string())?;
    std::fs::create_dir_all(&directory)
        .map_err(|_| "Could not create Movie Party's data directory".to_string())?;
    let body = serde_json::json!({
        "port": port,
        "protocol": crate::companion::PROTOCOL_VERSION,
    })
    .to_string();
    std::fs::write(directory.join("companion-endpoint.json"), body)
        .map_err(|_| "Could not publish the companion endpoint".to_string())?;
    Ok(runtime.companion_status())
}

#[tauri::command]
fn companion_status(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::companion::CompanionStatus {
    runtime.companion_status()
}

#[tauri::command]
fn companion_stop(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::companion::CompanionStatus, String> {
    runtime.companion_stop();
    if let Ok(directory) = app.path().app_local_data_dir() {
        // Leaving a stale endpoint behind would let a bridge keep dialling a port
        // that now belongs to something else.
        let _ = std::fs::remove_file(directory.join("companion-endpoint.json"));
    }
    Ok(runtime.companion_status())
}

#[tauri::command]
fn companion_new_code(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::companion::CompanionStatus, String> {
    runtime.companion_new_code()?;
    Ok(runtime.companion_status())
}

#[tauri::command]
fn companion_unlink(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::companion::CompanionStatus {
    runtime.companion_unlink()
}

#[tauri::command]
fn companion_install_host(extension_id: String) -> Result<String, String> {
    app_runtime::AppRuntime::companion_install_host(&extension_id)
}

#[tauri::command]
fn companion_remove_host() -> Result<(), String> {
    app_runtime::AppRuntime::companion_remove_host()
}

#[tauri::command]
fn youtube_event(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    context: providers::selection::SelectionContext,
    event: app_runtime::youtube::EmbedEvent,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.youtube_event(context, event)
}

#[tauri::command]
async fn focus_provider_playback(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.focus_provider_playback())
        .await
        .map_err(|_| "Could not focus managed Chrome".to_string())?
}

#[tauri::command]
fn retry_room_provider(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.retry_room_provider()
}

#[tauri::command]
async fn get_provider_content(
    provider_id: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<crate::providers::content::ChooserSnapshot, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.refresh_provider_content(&provider_id))
        .await
        .map_err(|_| "Browser observation failed. Refresh the chooser.".to_string())?
}

#[tauri::command]
async fn confirm_provider_content(
    provider_id: String,
    target_id: String,
    selection_key: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<crate::providers::content::ChooserSnapshot, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime.confirm_provider_content(&provider_id, &target_id, &selection_key)
    })
    .await
    .map_err(|_| "Content confirmation failed. Refresh the chooser.".to_string())?
}

#[tauri::command]
async fn browse_provider_content(
    provider_id: String,
    input: String,
    direct: bool,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<crate::providers::content::ChooserSnapshot, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime.browse_provider_content(&provider_id, &input, direct)
    })
    .await
    .map_err(|_| "Browser navigation failed. Retry in Chrome.".to_string())?
}

#[tauri::command]
async fn validate_provider_content(
    provider_id: String,
    selection_key: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime.validate_confirmed_provider_content(&provider_id, &selection_key)
    })
    .await
    .map_err(|_| "Content verification failed. Refresh the chooser.".to_string())?
}

#[tauri::command]
fn launch_generic_link(
    app: tauri::AppHandle,
    operations: tauri::State<'_, ChromeOperations>,
    url: String,
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    let _operation = operations.0.lock().unwrap_or_else(|e| e.into_inner());
    use crate::providers::chrome::{
        build_launch_plan, default_chrome_candidates, find_chrome, launch_managed_chrome,
    };

    if !crate::providers::sync::generic_link_accepts_url(&url) {
        return Err("MP-PROVIDER-002 invalid third-party link".to_string());
    }

    let chrome_path = match find_chrome(&default_chrome_candidates()) {
        Ok(path) => path,
        Err(error) => return Ok(runtime.generic_link_unavailable(url, error.to_string())),
    };
    let profiles_root = prepare_managed_profile(&app, "generic-link")?;
    let plan = match build_launch_plan(chrome_path, &profiles_root, "generic-link", &url) {
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

/// §16 invitation: render the invite link as a QR code (SVG string) so
/// it can be shown in-app and scanned across devices. The command is a
/// renderer only — it never generates or validates invite secrets; the
/// caller passes the host's canonical movieparty:// link.
#[tauri::command]
fn invite_qr_svg(invite_url: String) -> Result<String, String> {
    let code =
        qrcode::QrCode::with_error_correction_level(invite_url.as_bytes(), qrcode::EcLevel::M)
            .map_err(|e| format!("MP-ROOM-001 could not encode invite link: {e}"))?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .build();
    Ok(svg)
}

#[tauri::command]
fn guest_accept_schedule(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    schedule_id: String,
    accepted: bool,
) -> app_runtime::AppSnapshot {
    runtime.guest_accept_schedule(&schedule_id, accepted)
}

/// (UI_UX_SPEC §11): truthful prerequisite statuses for First Run.
/// Detection lives in Rust (§6 — native detection is backend work);
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
    /// The application version, taken from the crate's own package metadata
    /// (`src-tauri/Cargo.toml`). This is deliberately NOT a second hardcoded
    /// literal: `CARGO_PKG_VERSION` is the compile-time value of the same
    /// declaration the version-consistency gate compares against
    /// `package.json`, `tauri.conf.json` and `Cargo.lock`, so there is exactly
    /// one place to bump and no way for this field to drift from the others.
    app_version: &'static str,
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

    /// The Settings "App version" row used to render the app *name*, because
    /// `app_metadata()` carried no version field at all and the frontend fell
    /// back to `info.appName`. These two assertions are the regression guard
    /// for that defect: the reported version must be the crate's real package
    /// version, and it must never be the app name.
    #[test]
    fn metadata_reports_the_package_version_not_the_app_name() {
        let metadata = app_metadata();

        assert_eq!(metadata.app_version, env!("CARGO_PKG_VERSION"));
        assert!(
            !metadata.app_version.is_empty(),
            "the version reported to the UI must never be empty"
        );
        assert_ne!(
            metadata.app_version, metadata.app_name,
            "the version field must not echo the app name"
        );
        // A real semver-shaped version, not a placeholder.
        let parts: Vec<&str> = metadata.app_version.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "expected a three-part version, got {:?}",
            metadata.app_version
        );
        for part in parts {
            assert!(
                !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()),
                "every version component must be numeric, got {:?}",
                metadata.app_version
            );
        }
    }

    /// §16: the invite QR renderer produces a real scannable SVG for a
    /// canonical invite link — and rejects nothing it should accept (the
    /// renderer is content-agnostic; link validation lives in join_party).
    #[test]
    fn invite_qr_svg_renders_a_scannable_matrix() {
        let link = "movieparty://join/v1.1/EXAMPLE ROOM/abc123";
        let svg = invite_qr_svg(link.to_string()).expect("qr renders");

        assert!(svg.contains("<svg"), "must be an SVG document: {svg}");
        // A QR carries finder squares + the encoded payload — a real
        // render is hundreds of bytes, never a stub.
        assert!(svg.len() > 300, "rendered QR too small: {svg}");
        assert!(svg.contains("width="), "rendered at 240px scale");
        // Deterministic: same link, same render (stable UI, no flicker).
        let again = invite_qr_svg(link.to_string()).expect("qr renders again");
        assert_eq!(svg, again);
    }

    /// §16: the renderer accepts a real invite link at v1 size and
    /// produces the full finder/timing/data matrix — the QR is genuinely
    /// scannable, not a decorative stub.
    #[test]
    fn invite_qr_svg_encodes_a_real_invite_payload() {
        let link = "movieparty://join/v1.1/MOVIE ROOM 42/abcdefghij";
        let svg = invite_qr_svg(link.to_string()).expect("qr renders");
        // Structural guarantees of a real QR render (any version/size):
        // the module-carrying path element and the encoded viewBox.
        assert!(svg.contains("<path"), "module path present");
        assert!(svg.contains("viewBox="), "scaled viewBox present");
        // The payload length must be encoded — a longer link produces a
        // denser matrix (more path commands than a short one).
        let short = invite_qr_svg("movieparty://join/x".to_string()).expect("short link renders");
        assert!(svg.len() > short.len(), "longer invite → denser matrix");
    }
}
