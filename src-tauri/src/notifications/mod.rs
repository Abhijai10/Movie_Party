//! Clean notification abstraction for Move Party.
//!
//! Product targets: macOS and Windows (no Linux product work).
//!
//! [`Notifier`] is the seam between the scheduler and the OS. Production uses
//! [`NativeNotifier`], which dispatches through the native mechanism on each
//! platform (osascript on macOS, PowerShell toast on Windows). Tests use the
//! in-memory [`FakeNotifier`] or [`FailingNotifier`] — unit tests never
//! display real OS notifications.
//!
//! Notification failures are recoverable: every dispatch returns a
//! `Result`, and the scheduler treats an `Err` as a non-fatal event.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

pub const NOTIFICATION_APP_NAME: &str = "Move Party";

pub trait Notifier: Send + Sync + std::fmt::Debug {
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;
}

/// Production notifier: native dispatch per platform.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeNotifier;

impl Notifier for NativeNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            native_macos_notify(title, body)
        }
        #[cfg(target_os = "windows")]
        {
            native_windows_notify(title, body)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (title, body);
            Ok(())
        }
    }
}

/// In-memory notifier for unit tests. Records every notification.
#[derive(Debug, Default)]
pub struct FakeNotifier {
    notifications: Mutex<Vec<(String, String)>>,
    count: AtomicUsize,
}

impl FakeNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    pub fn notifications(&self) -> Vec<(String, String)> {
        self.notifications
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

impl Notifier for FakeNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        if let Ok(mut g) = self.notifications.lock() {
            g.push((title.to_string(), body.to_string()));
        }
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Test notifier that always fails — proves notification failure is
/// recoverable and never crashes the scheduler.
#[derive(Debug, Clone, Copy, Default)]
pub struct FailingNotifier;

impl Notifier for FailingNotifier {
    fn notify(&self, _title: &str, _body: &str) -> Result<(), String> {
        Err("MP-NOTIFY-001 simulated notification failure".to_string())
    }
}

#[cfg(target_os = "macos")]
fn native_macos_notify(title: &str, body: &str) -> Result<(), String> {
    let script = format!(
        "display notification \"{}\" with title \"{}\" sound name \"default\"",
        body.replace('"', "\\\""),
        title.replace('"', "\\\""),
    );
    let result = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status();
    match result {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("osascript exited with status {s}")),
        Err(e) => Err(format!("failed to run osascript: {e}")),
    }
}

#[cfg(target_os = "windows")]
fn native_windows_notify(title: &str, body: &str) -> Result<(), String> {
    // Self-contained PowerShell toast (WinRT) with no external modules.
    let script = format!(
        "Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
         $null = [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime]; \
         $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; \
         $xml.LoadXml('<toast><visual><binding template=\"ToastGeneric\"><text>{0}</text><text>{1}</text></binding></visual></toast>'); \
         $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Move Party').Show($toast)",
        title.replace('"', "'"),
        body.replace('"', "'"),
    );
    let result = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status();
    match result {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("powershell exited with status {s}")),
        Err(e) => Err(format!("failed to run powershell: {e}")),
    }
}

/// Convenience: dispatch through the native notifier.
pub fn send_local_notification(title: &str, body: &str) -> Result<(), String> {
    NativeNotifier.notify(title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_notifier_records_notifications() {
        let notifier = FakeNotifier::new();
        notifier.notify("Title", "Body").expect("ok");
        notifier.notify("Title2", "Body2").expect("ok");
        assert_eq!(notifier.count(), 2);
        assert_eq!(
            notifier.notifications(),
            vec![
                ("Title".to_string(), "Body".to_string()),
                ("Title2".to_string(), "Body2".to_string())
            ]
        );
    }

    #[test]
    fn failing_notifier_returns_recoverable_error() {
        let notifier = FailingNotifier;
        let err = notifier.notify("T", "B").expect_err("fails");
        assert!(err.contains("MP-NOTIFY"));
    }
}
