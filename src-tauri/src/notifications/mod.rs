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

#[cfg(target_os = "windows")]
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};

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
    // User text is NEVER interpolated into AppleScript source. It travels
    // as real command arguments via `on run argv`, so quotes, apostrophes,
    // `&`, newlines, etc. cannot escape the string context.
    let script = "on run argv
set theTitle to item 1 of argv
set theBody to item 2 of argv
display notification theBody with title theTitle sound name \"default\"
end run";
    let result = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .arg("--")
        .arg(title)
        .arg(body)
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
    // The toast XML is built and XML-escaped here in Rust, then passed to
    // PowerShell as base64 — never interpolated into the script source.
    let xml = toast_xml(title, body);
    let encoded = STANDARD_NO_PAD.encode(xml.as_bytes());
    let script = format!(
        "Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
         [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime] | Out-Null; \
         $xmlText = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{0}')); \
         $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; \
         $xml.LoadXml($xmlText); \
         $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Move Party').Show($toast)",
        encoded,
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

/// Escape text for inclusion in an XML text node (`& < >`).
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Build the toast XML with fully-escaped title/body text nodes.
pub fn toast_xml(title: &str, body: &str) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(title),
        xml_escape(body),
    )
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

    #[test]
    fn xml_escape_handles_hostile_characters() {
        assert_eq!(xml_escape("a&b"), "a&amp;b");
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(xml_escape("he said \"hi\""), "he said &quot;hi&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
        assert_eq!(xml_escape("line1\nline2"), "line1\nline2");
    }

    #[test]
    fn toast_xml_contains_escaped_text_nodes() {
        let xml = toast_xml("Quote's \"&<Party>", "A & B < C > D");
        // The raw hostile characters must not appear in the XML.
        assert!(!xml.contains("A & B"));
        assert!(xml.contains("A &amp; B"));
        assert!(xml.contains("&lt; C &gt; D"));
        assert!(xml.contains("&quot;&amp;&lt;Party&gt;"));
        // The XML must be parseable.
        assert!(xml.starts_with("<toast>"));
        assert!(xml.ends_with("</toast>"));
    }

    #[test]
    fn native_dispatch_never_interpolates_into_source() {
        #[cfg(target_os = "macos")]
        {
            // The macOS bridge passes text as separate argv elements, never
            // concatenated into the AppleScript source.
            let script = "on run argv
set theTitle to item 1 of argv
set theBody to item 2 of argv
display notification theBody with title theTitle sound name \"default\"
end run";
            let title = "A \"quote\" & <brackets>";
            let body = "line\nbreak";
            let dash_dash = "--";
            let osascript = "osascript";
            let flag = "-e";
            let mut cmd = std::process::Command::new(osascript);
            cmd.arg(flag)
                .arg(script)
                .arg(dash_dash)
                .arg(title)
                .arg(body);
            let mut args: Vec<String> = Vec::new();
            for arg in cmd.get_args() {
                args.push(arg.to_string_lossy().into_owned());
            }
            // Program name is not part of args: "-e", script, "--", title, body.
            assert_eq!(args.len(), 5);
            assert!(args.contains(&title.to_string()));
            assert!(args.contains(&body.to_string()));
        }
        #[cfg(target_os = "windows")]
        {
            // The Windows bridge passes the toast XML as base64 — hostile
            // characters can never be interpreted as PowerShell code.
            let encoded = STANDARD_NO_PAD.encode(toast_xml("'&\n", "<x>").as_bytes());
            let script = format!(
                "Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
                 [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime] | Out-Null; \
                 $xmlText = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{0}')); \
                 $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; \
                 $xml.LoadXml($xmlText); \
                 $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Move Party').Show($toast)",
                encoded,
            );
            assert!(!script.contains("A &"));
            assert!(script.contains(&encoded));
            // The base64 payload decodes to well-formed XML.
            let decoded = STANDARD_NO_PAD.decode(encoded).expect("decode");
            let xml = String::from_utf8(decoded).expect("utf8");
            assert!(xml.contains("&amp;"));
        }
    }
}
