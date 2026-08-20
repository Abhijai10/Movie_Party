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
pub mod storage;
pub mod sync;
pub mod telemetry;

pub const APP_NAME: &str = "Move Party";
pub const PROTOCOL_MAJOR: u16 = 1;
pub const PROTOCOL_MINOR: u16 = 0;

use tauri::Manager;

pub fn run() {
    let result = tauri::Builder::default()
        .manage(app_runtime::AppRuntime::new_without_emitter())
        .setup(|app| {
            // M4: Initialize the SQLite database on startup, restore identity,
            // detect overdue preloads, and start the real scheduler worker.
            let runtime = app.state::<app_runtime::AppRuntime>();
            runtime.init_db();
            runtime.check_overdue_schedules();
            runtime.spawn_scheduler_worker();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_metadata,
            get_app_snapshot,
            init_listener,
            create_local_party,
            join_party,
            mark_ready,
            enter_cinema,
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
            pick_media_file,
            launch_provider,
            create_schedule,
            list_schedules,
            update_schedule_media,
            update_schedule_preload,
            delete_schedule,
            retention_keep,
            retention_remove,
            retention_save_as,
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("Move Party failed to start: {error}");
        std::process::exit(1);
    }
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
fn get_app_snapshot(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> app_runtime::AppSnapshot {
    runtime.snapshot()
}

#[tauri::command]
async fn create_local_party(
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
    media_path: Option<String>,
) -> Result<app_runtime::AppSnapshot, String> {
    runtime.create_local_party(media_path).await
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
fn enter_cinema(runtime: tauri::State<'_, app_runtime::AppRuntime>) -> app_runtime::AppSnapshot {
    runtime.enter_cinema()
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
    runtime: tauri::State<'_, app_runtime::AppRuntime>,
) -> Result<app_runtime::AppSnapshot, String> {
    use crate::providers::chrome::{
        build_launch_plan, default_chrome_candidates, find_chrome, launch_managed_chrome,
    };

    let candidates = default_chrome_candidates();
    let chrome_path = find_chrome(&candidates).map_err(|e| e.to_string())?;
    let profiles_root = std::env::temp_dir().join("MovePartyProfiles");
    let plan = build_launch_plan(chrome_path, &profiles_root, &provider_id, 0, &url)
        .map_err(|e| e.to_string())?;
    let session = launch_managed_chrome(plan).map_err(|e| e.to_string())?;

    let cdp_port = session.plan.cdp_port;

    // M6: Store the session in AppRuntime instead of leaking
    runtime.store_chrome_session(session);

    // Return a snapshot indicating the provider is launched
    let mut snap = runtime.snapshot();
    snap.provider.mode = "PROVIDER_SYNC".to_string();
    snap.provider.provider_id = Some(provider_id);
    snap.provider.url = Some(url);
    snap.provider.state = format!("Chrome launched on CDP port {}", cdp_port);

    Ok(snap)
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
