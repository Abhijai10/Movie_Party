//! Cross-platform process-spawn helpers.
//!
//! Movie Party spawns several CLI helpers (tailscale.exe, powershell.exe,
//! ffmpeg, explorer.exe). On Windows every `std::process::Command` spawn of
//! a console binary flashes a black console window on the user's screen —
//! repeated readiness polls made the app feel like something kept launching
//! over and over. All spawn sites route through [`quiet_command`] /
//! [`quiet_async_command`], which set `CREATE_NO_WINDOW` on Windows and are
//! plain constructors elsewhere.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Build a `std::process::Command` that never flashes a console window on
/// Windows. On other platforms this is identical to `Command::new`.
pub fn quiet_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(program)
    }
}

/// Build a `tokio::process::Command` that never flashes a console window on
/// Windows. On other platforms this is identical to `Command::new`.
pub fn quiet_async_command<S: AsRef<std::ffi::OsStr>>(program: S) -> tokio::process::Command {
    #[cfg(windows)]
    {
        // tokio::process::Command has an inherent `creation_flags` method on
        // Windows (no CommandExt import needed), unlike std::process::Command.
        let mut command = tokio::process::Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(windows))]
    {
        tokio::process::Command::new(program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_command_builds_on_every_platform() {
        let command = quiet_command("does-not-exist");
        let program: &str = command.get_program().to_str().unwrap_or("");
        assert_eq!(program, "does-not-exist");
    }

    #[test]
    fn quiet_async_command_builds_on_every_platform() {
        let command = quiet_async_command("does-not-exist");
        let program: &str = command.as_std().get_program().to_str().unwrap_or("");
        assert_eq!(program, "does-not-exist");
    }
}
