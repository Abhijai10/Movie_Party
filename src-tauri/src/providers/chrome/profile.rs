//! Durable, app-owned profiles. Locks live outside Chrome's data so resetting a
//! profile cannot remove a live ownership lock. Never import browser credentials.
use super::ManagedChromeError;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LaunchState {
    Launching,
    Running,
    Closed,
    UnexpectedExit,
    LaunchFailed,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OwnershipRecord {
    pub provider: String,
    pub profile: PathBuf,
    pub pid: Option<u32>,
    #[serde(default)]
    pub process_birth: Option<String>,
    pub state: LaunchState,
}

#[derive(Debug)]
pub(super) struct ProfileLease {
    _lock: File,
    record_path: PathBuf,
    pub record: OwnershipRecord,
}

fn profile_error(error: impl std::fmt::Display) -> ManagedChromeError {
    ManagedChromeError::Profile(format!(
        "{error}. Check app-data permissions; no sign-in data was removed"
    ))
}

fn directory(path: &Path) -> Result<(), ManagedChromeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(profile_error(
                "Managed profile path must be a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(profile_error)?;
        }
        Err(error) => return Err(profile_error(error)),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(profile_error)?;
    }
    Ok(())
}

impl ProfileLease {
    pub fn acquire(profile: &Path) -> Result<Self, ManagedChromeError> {
        Self::acquire_inner(profile, false)
    }

    fn acquire_inner(profile: &Path, confirmed_reset: bool) -> Result<Self, ManagedChromeError> {
        let provider = profile
            .file_name()
            .and_then(|v| v.to_str())
            .filter(|v| super::is_valid_provider_id(v))
            .ok_or(ManagedChromeError::InvalidProviderProfile)?;
        let root = profile
            .parent()
            .ok_or(ManagedChromeError::InvalidProviderProfile)?;
        directory(root)?;
        let lock = root.join(format!(".{provider}.lock"));
        let record_path = root.join(format!(".{provider}.ownership"));
        if fs::symlink_metadata(&lock).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(profile_error("Ownership file must not be a symbolic link"));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(&lock).map_err(profile_error)?;
        file.try_lock()
            .map_err(|_| ManagedChromeError::ProfileBusy)?;
        // A stale record never grants authority to kill a PID (it may have been
        // reused). Refuse while it may still own a browser; let the user close it.
        let previous = match fs::read_to_string(&record_path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(profile_error(error)),
        };
        if !previous.trim().is_empty() {
            let record = match serde_json::from_str::<OwnershipRecord>(&previous) {
                Ok(record) => Some(record),
                Err(_) if confirmed_reset => None,
                Err(_) => return Err(profile_error("Managed ownership record is corrupt; use confirmed Reset provider session or restore a backup")),
            };
            if record.as_ref().is_some_and(|record| {
                record.pid.is_some_and(|pid| {
                    process_may_exist(pid)
                        && match (&record.process_birth, process_birth(pid)) {
                            (Some(recorded), Some(actual)) => *recorded == actual,
                            _ => true, // A failed identity probe never grants deletion authority.
                        }
                })
            }) {
                return Err(ManagedChromeError::ProfileBusy);
            }
        }
        directory(profile)?;
        let profile = fs::canonicalize(profile).map_err(profile_error)?;
        Ok(Self {
            _lock: file,
            record_path,
            record: OwnershipRecord {
                provider: provider.into(),
                profile,
                pid: None,
                process_birth: None,
                state: LaunchState::Closed,
            },
        })
    }

    pub fn update(
        &mut self,
        state: LaunchState,
        pid: Option<u32>,
    ) -> Result<(), ManagedChromeError> {
        self.record.state = state;
        self.record.pid = pid;
        self.record.process_birth = pid.and_then(process_birth);
        let bytes = serde_json::to_vec(&self.record).map_err(profile_error)?;
        // Replace the record atomically: an interrupted app must not leave a
        // half-truncated PID record. The separate OS lock remains held.
        let temporary = self
            .record_path
            .with_extension(format!("{}.tmp", uuid::Uuid::now_v7()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(profile_error)?;
            file.write_all(&bytes).map_err(profile_error)?;
            file.sync_all().map_err(profile_error)?;
            fs::rename(&temporary, &self.record_path).map_err(profile_error)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn validate_data(&self) -> Result<(), ManagedChromeError> {
        let path = self.record.profile.join("Local State");
        match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| {
                    ManagedChromeError::Profile("Chrome's Local State is corrupt. Use the confirmed Reset provider session action, or restore a backup".into())
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(profile_error(error)),
        }
        Ok(())
    }
}

#[cfg(unix)]
fn process_birth(pid: u32) -> Option<String> {
    // Native process start time distinguishes PID reuse after a crash/reboot.
    // No shell, no credentials, no process signalling.
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !value.is_empty()).then_some(value)
}

#[cfg(windows)]
fn process_birth(pid: u32) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut created: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let ok = GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        (ok != 0).then(|| format!("{}:{}", created.dwHighDateTime, created.dwLowDateTime))
    }
}

#[cfg(unix)]
fn process_may_exist(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true;
    };
    if pid <= 1 {
        return true;
    }
    // The isolated group can outlive its leader. Signal zero never kills it.
    unsafe {
        libc::kill(pid, 0) == 0
            || libc::kill(-pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}

#[cfg(windows)]
fn process_may_exist(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return std::io::Error::last_os_error().raw_os_error() != Some(87);
        }
        let running = WaitForSingleObject(process, 0) != WAIT_OBJECT_0;
        CloseHandle(process);
        running
    }
}

/// Destructive reset is explicit, confirmed, offline, and limited to one managed
/// profile. Browser data is never removed as an automatic launch recovery step.
pub fn reset_profile(
    root: &Path,
    provider: &str,
    confirmed: bool,
) -> Result<(), ManagedChromeError> {
    if !confirmed {
        return Err(ManagedChromeError::ConfirmationRequired);
    }
    let path = super::provider_profile_path(root, provider)?;
    let mut lease = ProfileLease::acquire_inner(&path, true)?;
    ensure_chrome_released(&path)?;
    fs::remove_dir_all(&path).map_err(profile_error)?;
    lease.update(LaunchState::Closed, None)
}

fn ensure_chrome_released(path: &Path) -> Result<(), ManagedChromeError> {
    #[cfg(windows)]
    {
        // Chromium opens `lockfile` for write without sharing writes/deletes.
        // Probe without creating/truncating it, even if our PID record is stale.
        use std::os::windows::fs::OpenOptionsExt;
        match OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path.join("lockfile"))
        {
            Ok(file) => drop(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ManagedChromeError::ProfileBusy),
        }
    }
    // Chrome's own singleton is not ours to remove while it could be live.
    // A fresh managed launch lets Chrome recover stale singleton files itself.
    #[cfg(unix)]
    if let Ok(target) = fs::read_link(path.join("SingletonLock")) {
        if let Some(pid) = target
            .to_string_lossy()
            .rsplit('-')
            .next()
            .and_then(|v| v.parse::<u32>().ok())
        {
            if process_may_exist(pid) {
                return Err(ManagedChromeError::ProfileBusy);
            }
        } else {
            return Err(ManagedChromeError::ProfileBusy);
        }
    }
    Ok(())
}

/// Relocate only Movie Party's old managed directory. Never import an ordinary
/// Chrome profile or copy credentials. Failed renames leave legacy data intact.
pub fn migrate_legacy_profile(
    root: &Path,
    legacy_root: &Path,
    provider: &str,
) -> Result<(), ManagedChromeError> {
    let source = super::provider_profile_path(legacy_root, provider)?;
    let destination = super::provider_profile_path(root, provider)?;
    if !source.exists() {
        return Ok(());
    }
    if root.join(format!(".{provider}.ownership")).exists() {
        return Ok(());
    }
    if destination.exists()
        && fs::read_dir(&destination)
            .map_err(profile_error)?
            .next()
            .is_some()
    {
        return Ok(());
    }
    directory(legacy_root)?;
    directory(&source)?;
    let mut lease = ProfileLease::acquire(&destination)?;
    ensure_chrome_released(&source)?;
    fs::remove_dir(&destination).map_err(profile_error)?;
    if let Err(error) = fs::rename(&source, &destination) {
        let _ = directory(&destination);
        return Err(profile_error(format!("Could not move the previous Movie Party session: {error}. The original is still in place")));
    }
    lease.update(LaunchState::Closed, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("mp-profile-{}", uuid::Uuid::now_v7())))
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn profile_data_survives_close_relaunch_and_provider_isolation() {
        let root = Root::new();
        let path = root.0.join("youtube");
        let mut lease = ProfileLease::acquire(&path).unwrap();
        fs::write(path.join("session-sentinel"), b"opaque test data").unwrap();
        lease.update(LaunchState::Closed, None).unwrap();
        drop(lease);
        let _again = ProfileLease::acquire(&path).unwrap();
        let _other = ProfileLease::acquire(&root.0.join("netflix")).unwrap();
        assert_eq!(
            fs::read(path.join("session-sentinel")).unwrap(),
            b"opaque test data"
        );
        assert!(!root.0.join("netflix/session-sentinel").exists());
        assert_eq!(
            ProfileLease::acquire(&path).unwrap_err(),
            ManagedChromeError::ProfileBusy
        );
    }

    #[test]
    fn migrates_legacy_managed_data_without_resurrecting_a_reset_session() {
        let root = Root::new();
        let legacy = Root::new();
        fs::create_dir_all(legacy.0.join("youtube")).unwrap();
        fs::write(legacy.0.join("youtube/sentinel"), b"legacy").unwrap();
        migrate_legacy_profile(&root.0, &legacy.0, "youtube").unwrap();
        assert_eq!(
            fs::read(root.0.join("youtube/sentinel")).unwrap(),
            b"legacy"
        );
        assert!(!legacy.0.join("youtube").exists());
        fs::create_dir_all(legacy.0.join("youtube")).unwrap();
        fs::write(legacy.0.join("youtube/sentinel"), b"do not resurrect").unwrap();
        reset_profile(&root.0, "youtube", true).unwrap();
        migrate_legacy_profile(&root.0, &legacy.0, "youtube").unwrap();
        assert!(!root.0.join("youtube/sentinel").exists());
        assert!(legacy.0.join("youtube/sentinel").exists());
    }

    #[test]
    fn ownership_records_keep_failure_reason_and_allow_retry() {
        let root = Root::new();
        let path = root.0.join("youtube");
        let mut lease = ProfileLease::acquire(&path).unwrap();
        lease.update(LaunchState::Launching, None).unwrap();
        lease.update(LaunchState::LaunchFailed, None).unwrap();
        drop(lease);
        let record: OwnershipRecord =
            serde_json::from_slice(&fs::read(root.0.join(".youtube.ownership")).unwrap()).unwrap();
        assert_eq!(record.state, LaunchState::LaunchFailed);
        assert_eq!(record.provider, "youtube");
        let _retry = ProfileLease::acquire(&path).unwrap();
    }

    #[test]
    fn dead_pid_record_is_recoverable_without_deleting_storage() {
        let root = Root::new();
        let path = root.0.join("youtube");
        let mut lease = ProfileLease::acquire(&path).unwrap();
        #[cfg(unix)]
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        #[cfg(windows)]
        let mut child = crate::process::quiet_command("cmd")
            .args(["/C", "exit", "0"])
            .spawn()
            .unwrap();
        lease
            .update(LaunchState::Running, Some(child.id()))
            .unwrap();
        child.wait().unwrap();
        fs::write(path.join("sentinel"), b"keep").unwrap();
        drop(lease);
        let _recovered = ProfileLease::acquire(&path).unwrap();
        assert!(path.join("sentinel").exists());
    }

    #[cfg(unix)]
    #[test]
    fn native_live_lock_prevents_reset_and_legacy_move() {
        let root = Root::new();
        let legacy = Root::new();
        fs::create_dir_all(legacy.0.join("youtube")).unwrap();
        std::os::unix::fs::symlink(
            format!("machine-{}", std::process::id()),
            legacy.0.join("youtube/SingletonLock"),
        )
        .unwrap();
        assert_eq!(
            migrate_legacy_profile(&root.0, &legacy.0, "youtube"),
            Err(ManagedChromeError::ProfileBusy)
        );
        assert!(legacy.0.join("youtube").exists());
        fs::create_dir_all(root.0.join("netflix")).unwrap();
        std::os::unix::fs::symlink(
            format!("machine-{}", std::process::id()),
            root.0.join("netflix/SingletonLock"),
        )
        .unwrap();
        assert_eq!(
            reset_profile(&root.0, "netflix", true),
            Err(ManagedChromeError::ProfileBusy)
        );
    }
    #[cfg(windows)]
    #[test]
    fn native_live_lock_prevents_reset_and_legacy_move() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = Root::new();
        let legacy = Root::new();
        let source = legacy.0.join("youtube");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("sentinel"), b"keep legacy session").unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .share_mode(0)
            .open(source.join("lockfile"))
            .unwrap();
        assert_eq!(
            migrate_legacy_profile(&root.0, &legacy.0, "youtube"),
            Err(ManagedChromeError::ProfileBusy)
        );
        assert!(source.join("sentinel").exists());
        drop(lock);
        migrate_legacy_profile(&root.0, &legacy.0, "youtube").unwrap();
        let destination = root.0.join("youtube");
        assert_eq!(
            fs::read(destination.join("sentinel")).unwrap(),
            b"keep legacy session"
        );

        let lock = OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(destination.join("lockfile"))
            .unwrap();
        assert_eq!(
            reset_profile(&root.0, "youtube", true),
            Err(ManagedChromeError::ProfileBusy)
        );
        assert!(destination.join("sentinel").exists());
        drop(lock);
        reset_profile(&root.0, "youtube", true).unwrap();
        assert!(!destination.exists());
    }

    #[test]
    fn reset_requires_confirmation_and_exclusive_ownership() {
        let root = Root::new();
        let path = root.0.join("youtube");
        let lease = ProfileLease::acquire(&path).unwrap();
        fs::write(path.join("keep"), b"keep").unwrap();
        assert_eq!(
            reset_profile(&root.0, "youtube", false),
            Err(ManagedChromeError::ConfirmationRequired)
        );
        assert_eq!(
            reset_profile(&root.0, "youtube", true),
            Err(ManagedChromeError::ProfileBusy)
        );
        assert!(path.join("keep").exists());
        drop(lease);
        reset_profile(&root.0, "youtube", true).unwrap();
        assert!(!path.exists());
        assert!(root.0.join(".youtube.ownership").exists());
    }
    #[test]
    fn corrupt_profile_is_preserved_until_confirmed_reset() {
        let root = Root::new();
        let path = root.0.join("youtube");
        let lease = ProfileLease::acquire(&path).unwrap();
        fs::write(path.join("Local State"), b"broken").unwrap();
        assert!(matches!(
            lease.validate_data(),
            Err(ManagedChromeError::Profile(_))
        ));
        drop(lease);
        assert!(path.join("Local State").exists());
        reset_profile(&root.0, "youtube", true).unwrap();
    }

    #[test]
    fn corrupt_ownership_record_is_recoverable_only_by_confirmed_reset() {
        let root = Root::new();
        let path = root.0.join("youtube");
        let lease = ProfileLease::acquire(&path).unwrap();
        drop(lease);
        fs::write(
            root.0.join(".youtube.ownership"),
            b"interrupted or corrupt record",
        )
        .unwrap();
        assert!(ProfileLease::acquire(&path).is_err());
        assert_eq!(
            reset_profile(&root.0, "youtube", false),
            Err(ManagedChromeError::ConfirmationRequired)
        );
        reset_profile(&root.0, "youtube", true).unwrap();
        let _fresh = ProfileLease::acquire(&path).unwrap();
    }
    #[test]
    fn stale_records_never_authorize_killing_an_existing_process() {
        let root = Root::new();
        let path = root.0.join("youtube");
        let mut lease = ProfileLease::acquire(&path).unwrap();
        lease
            .update(LaunchState::Running, Some(std::process::id()))
            .unwrap();
        drop(lease);
        assert_eq!(
            ProfileLease::acquire(&path).unwrap_err(),
            ManagedChromeError::ProfileBusy
        );
    }
    #[test]
    fn data_path_and_traversal_errors_do_not_touch_other_profiles() {
        let root = Root::new();
        fs::create_dir_all(&root.0).unwrap();
        fs::write(root.0.join("youtube"), b"not a directory").unwrap();
        assert!(ProfileLease::acquire(&root.0.join("youtube")).is_err());
        assert_eq!(
            reset_profile(&root.0, "../other", true),
            Err(ManagedChromeError::InvalidProviderProfile)
        );
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symlink_profiles() {
        let root = Root::new();
        fs::create_dir_all(&root.0).unwrap();
        std::os::unix::fs::symlink(std::env::temp_dir(), root.0.join("youtube")).unwrap();
        assert!(ProfileLease::acquire(&root.0.join("youtube")).is_err());
    }
}
