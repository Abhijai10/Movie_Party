pub const NOTIFICATION_APP_NAME: &str = "Move Party";

/// Dispatch a local desktop notification via osascript on macOS.
/// Returns Ok(()) if dispatched, or an error string.
pub fn send_local_notification(title: &str, body: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification \"{}\" with title \"{}\" sound name \"default\"",
            body.replace('"', "\\\""),
            title.replace('"', "\\\""),
        );
        let status = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("osascript exited with status {s}")),
            Err(e) => Err(format!("failed to run osascript: {e}")),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, body);
        eprintln!("MoveParty notification: {title}: {body}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_does_not_panic() {
        let _ = send_local_notification("Test", "Test body");
    }
}
