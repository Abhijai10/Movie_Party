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

pub fn run() {
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_metadata])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("Move Party failed to start: {error}");
        std::process::exit(1);
    }
}

#[tauri::command]
fn app_metadata() -> AppMetadata {
    AppMetadata {
        app_name: APP_NAME,
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
    }
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
