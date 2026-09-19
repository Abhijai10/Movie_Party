//! OS-level ownership of the managed Chrome process *tree*.
//!
//! A managed Chrome is never one process. The browser spawns renderer, GPU,
//! utility, zygote and crashpad children that share its lifetime, while
//! `std::process` models only the direct child — so teardown built on
//! `Child::kill()` + `Child::wait()` terminated the browser and orphaned
//! everything else. Orphaned renderers keep the dedicated provider profile
//! locked, which is what makes the *next* launch of that provider fail or show
//! a "restore pages" banner.
//!
//! This module gives Movie Party exactly one owner for the whole tree:
//!
//! * the browser is spawned as its own **process group** (POSIX) or inside a
//!   **kill-on-close job object** (Windows), so teardown can address every
//!   descendant and can never reach a process Movie Party does not own;
//! * [`ChromeProcessOwner::terminate_tree`] is the single, idempotent exit
//!   point: it signals the group, waits briefly, then hard-kills and reaps, so
//!   no path can leave a live tree or an unreaped zombie behind;
//! * the live group id is published in a process-global slot so the shutdown
//!   paths that never run `Drop` — signals and panics — can still kill it.
//!
//! ## Coverage of "Movie Party died without running cleanup"
//!
//! | Platform | Mechanism | Covered |
//! | --- | --- | --- |
//! | Windows | job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` | every exit path, including a forced kill |
//! | POSIX | process group + SIGINT/SIGTERM/SIGHUP + panic hook | `Exit`, those signals, and panics |
//!
//! macOS has no kernel-level parent-death mechanism, so a `SIGKILL` of Movie
//! Party there still leaves the tree behind. That is the one residual gap and
//! it is reported as a limitation rather than papered over.

use std::process::{Child, Stdio};

#[cfg(unix)]
use std::{
    sync::atomic::{AtomicI32, Ordering},
    thread,
    time::{Duration, Instant},
};

use super::{ChromeLaunchPlan, ManagedChromeError};

/// How long a signalled tree gets to exit on its own before the hard kill.
/// Deliberately short: `ManagedChromeSession::close_gracefully` has already
/// spent up to `GRACEFUL_CLOSE_TIMEOUT` asking Chrome to close over CDP, so by
/// the time we are here the polite option has been exhausted.
#[cfg(unix)]
const TREE_EXIT_GRACE: Duration = Duration::from_millis(500);

/// Poll interval while waiting for the tree to exit.
#[cfg(unix)]
const TREE_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[cfg(unix)]
const POLITE_SIGNAL: i32 = libc::SIGTERM;
#[cfg(unix)]
const HARD_SIGNAL: i32 = libc::SIGKILL;

/// Process-group id of the one live managed Chrome tree, or `0` when there is
/// none.
///
/// A single slot rather than a registry is correct, not a shortcut:
/// `AppRuntimeState` owns at most one `chrome_session` at a time, so at most
/// one managed tree is ever live. Keeping it to one `AtomicI32` is also what
/// makes it usable from a signal handler — an atomic load plus `killpg` are
/// both async-signal-safe, whereas locking a registry inside a handler is not.
#[cfg(unix)]
static LIVE_CHROME_GROUP: AtomicI32 = AtomicI32::new(0);

/// What a poll of the browser process found.
#[derive(Debug)]
pub(super) enum ChildPoll {
    Running,
    Exited(String),
    Failed(String),
}

/// Owns the OS-level lifecycle of one managed Chrome process tree.
///
/// Every managed Chrome process has exactly one of these, and it is the only
/// thing in the codebase allowed to signal or reap a Chrome process.
#[derive(Debug)]
pub struct ChromeProcessOwner {
    /// `None` once the tree has been terminated. Taking it is the idempotency
    /// latch: every later `terminate_tree` call is a no-op, which is what makes
    /// repeated cleanup (leave, restart, shutdown, `Drop`) safe.
    child: Option<Child>,
    /// Process-group id of the tree; equals the browser's pid because the child
    /// is spawned with `process_group(0)`.
    #[cfg(unix)]
    process_group: i32,
    /// Kill-on-close job object holding every process in the tree.
    #[cfg(windows)]
    job: Option<JobObject>,
}

impl ChromeProcessOwner {
    /// Spawns `plan` with process-group/job isolation already applied.
    pub(super) fn spawn(plan: &ChromeLaunchPlan) -> Result<Self, ManagedChromeError> {
        let child = super::chrome_command(plan)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| ManagedChromeError::Process(error.to_string()))?;
        Ok(Self::adopt(child))
    }

    /// Takes ownership of an already-spawned child.
    ///
    /// The caller must have spawned it through
    /// [`super::configure_process_isolation`], or the recorded group is not the
    /// child's real group. Production only reaches this via `spawn`; tests use
    /// it to exercise teardown without launching real Chrome.
    pub(super) fn adopt(child: Child) -> Self {
        #[cfg(unix)]
        let process_group = child.id() as i32;

        #[cfg(windows)]
        let job = JobObject::create_for(&child);

        let owner = Self {
            child: Some(child),
            #[cfg(unix)]
            process_group,
            #[cfg(windows)]
            job,
        };
        #[cfg(unix)]
        publish_live_group(process_group);
        owner
    }

    /// True while the browser process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.as_mut().map(Child::try_wait), Some(Ok(None)))
    }

    /// The browser's pid, while this owner still holds the tree.
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Polls the browser process, reaping it when it has already exited.
    pub(super) fn poll(&mut self) -> ChildPoll {
        let Some(child) = self.child.as_mut() else {
            return ChildPoll::Exited("tree already terminated".to_string());
        };
        match child.try_wait() {
            Ok(Some(status)) => ChildPoll::Exited(status.to_string()),
            Ok(None) => ChildPoll::Running,
            Err(error) => ChildPoll::Failed(error.to_string()),
        }
    }

    /// Terminates the whole tree and reaps it. Idempotent.
    ///
    /// This is the only correct way to end a managed Chrome: it kills the
    /// browser *and* its descendants, never anything outside the tree, and it
    /// always reaps the direct child so no zombie survives.
    pub fn terminate_tree(&mut self) {
        // Take the child first. This is the idempotency latch, and it also
        // guarantees a second call cannot signal a group we no longer own.
        let Some(mut child) = self.child.take() else {
            return;
        };
        self.unpublish_live_group();

        // Already gone (the user closed the window, or a crash): there is
        // nothing to signal politely, but the tree may still hold live
        // descendants, so the hard group signal below still has to run.
        #[cfg(unix)]
        let already_exited = matches!(child.try_wait(), Ok(Some(_)));

        #[cfg(unix)]
        if !already_exited {
            // Polite first, so Chrome can still flush the dedicated profile.
            self.signal_group(POLITE_SIGNAL);
            let start = Instant::now();
            while start.elapsed() < TREE_EXIT_GRACE {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                thread::sleep(TREE_POLL_INTERVAL);
            }
        }

        // Unconditional, and deliberately *not* skipped when the browser had
        // already exited: the browser exiting does not mean its renderer/GPU
        // children exited, and those are exactly what used to leak.
        #[cfg(unix)]
        self.signal_group(HARD_SIGNAL);

        #[cfg(windows)]
        match self.job.take() {
            // TerminateJobObject kills every process in the job — the browser
            // and all of its children — and nothing outside it, so the user's
            // own Chrome windows are never touched.
            Some(job) => job.terminate(),
            // No job object (creation or assignment failed, e.g. Movie Party is
            // itself inside a job that forbids nesting). Fall back to the
            // Windows tree kill: `/T` walks the child tree of THIS pid only.
            // Deliberately not `/IM chrome.exe`, which would also kill the
            // user's own browser.
            None => {
                let pid = child.id().to_string();
                // `quiet_command`, not `Command::new`: taskkill is a console
                // binary and a bare spawn flashes a black window on the user's
                // screen (see crate::process).
                let _ = crate::process::quiet_command("taskkill")
                    .args(["/PID", pid.as_str(), "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }

        // Belt and braces on the direct child, then always reap: a failure path
        // that spawns without reaping leaves a zombie behind.
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Clears the process-global slot, but only while it still names *our*
    /// tree — a replacement session may have published its own group in the
    /// meantime (launch / leave / restart races).
    fn unpublish_live_group(&self) {
        #[cfg(unix)]
        {
            let _ = LIVE_CHROME_GROUP.compare_exchange(
                self.process_group,
                0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
    }
}

impl Drop for ChromeProcessOwner {
    /// Last-resort ownership transfer: every failure path either hands
    /// ownership on safely or kills and reaps. Reaching here without an
    /// explicit `terminate_tree` means somebody forgot — the tree is still
    /// killed rather than leaked.
    fn drop(&mut self) {
        self.terminate_tree();
    }
}

#[cfg(unix)]
impl ChromeProcessOwner {
    /// Signals every process in our Chrome tree, and nothing else.
    ///
    /// Caveat, recorded rather than hidden: the group is addressed by the
    /// browser's pid, so this relies on that pid not having been recycled as a
    /// *new* group leader since the browser exited. The window is the moment
    /// between reaping the browser and this call — microseconds on the teardown
    /// path, and at most the crash watcher's 2s poll on the crash path — while
    /// macOS and Linux allocate pids sequentially, so a collision in that window
    /// is not reachable in practice. Signalling the group is nonetheless the
    /// only way to reach children that have already been reparented, so the
    /// alternative (leaving them running) is strictly worse.
    fn signal_group(&self, signal: i32) {
        let pgid = self.process_group;
        // Guard rails — the "never kill an unrelated process" requirement:
        //   * 0 means "the caller's own group" and 1 is init; neither is ours;
        //   * signalling our OWN group would take Movie Party down with it.
        if pgid <= 1 || pgid == own_process_group() {
            return;
        }
        // SAFETY: `killpg` on a group we created ourselves via
        // `process_group(0)`. The guards above exclude init and our own group,
        // so no process outside this Chrome tree can be reached.
        unsafe {
            libc::killpg(pgid, signal);
        }
    }
}

#[cfg(unix)]
fn own_process_group() -> i32 {
    // SAFETY: `getpgrp` takes no arguments and cannot fail.
    unsafe { libc::getpgrp() }
}

#[cfg(unix)]
fn publish_live_group(process_group: i32) {
    LIVE_CHROME_GROUP.store(process_group, Ordering::SeqCst);
}

/// Kills the live managed Chrome tree, if any, without needing its owner.
///
/// Used by the shutdown paths that never run `Drop`: the signal handlers and
/// the panic hook. Safe to call at any time, and a no-op when no tree is live.
///
/// `SIGKILL` rather than a polite signal on purpose — by the time one of these
/// paths is running Movie Party is already going away, so there is nobody left
/// to wait for a clean `Browser.close` and no point pretending otherwise.
#[cfg(unix)]
pub fn kill_live_chrome_tree() {
    // `swap` both reads and claims the slot: a concurrent `terminate_tree` then
    // sees `0` and cannot double-signal.
    let pgid = LIVE_CHROME_GROUP.swap(0, Ordering::SeqCst);
    if pgid <= 1 || pgid == own_process_group() {
        return;
    }
    // SAFETY: async-signal-safe — one `killpg` on a group we created, whose id
    // was validated against init and our own group above.
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
}

/// Installs the POSIX shutdown hardening: SIGINT/SIGTERM/SIGHUP handlers and a
/// panic hook that kill the live managed Chrome tree before Movie Party goes
/// away.
///
/// Without this, cleanup ran *only* from `tauri::RunEvent::Exit`, so a signal
/// or a panic could orphan a whole Chrome tree. Idempotent: the second call
/// does nothing.
#[cfg(unix)]
pub fn install_shutdown_hardening() {
    use std::sync::Once;

    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // SAFETY: the handler only performs async-signal-safe work — one
        // lock-free atomic load and `killpg` — and then restores the default
        // disposition and re-raises, so Movie Party still dies *from the signal
        // it received* and the exit status stays truthful.
        unsafe {
            let handler = handle_shutdown_signal as extern "C" fn(libc::c_int);
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::signal(signal, handler as libc::sighandler_t);
            }
        }

        // A panic unwinds out of the app without ever dropping the runtime's
        // state, so `Drop` would never run. Killing the tree here is the only
        // way a panicking Movie Party does not leave Chrome behind.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            kill_live_chrome_tree();
            previous(info);
        }));
    });
}

#[cfg(unix)]
extern "C" fn handle_shutdown_signal(signal: libc::c_int) {
    kill_live_chrome_tree();
    // Restore the default disposition and re-raise. A bare `return` would
    // swallow the signal and leave Movie Party running with no browser.
    // SAFETY: both calls are async-signal-safe.
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

/// Windows needs no in-process signal hardening: the job object's
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the OS terminate the whole tree
/// when Movie Party's last job handle closes, which covers every exit path
/// including a crash or a forced kill. Kept as a no-op so the app entry point
/// can call it unconditionally.
#[cfg(windows)]
pub fn install_shutdown_hardening() {}

/// A Windows job object holding the whole Chrome tree.
///
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the important part: when the last
/// handle to the job closes — including when Movie Party dies without running
/// any cleanup — Windows terminates every process still in the job. That is the
/// only mechanism that can cover a forced kill, and because it is scoped to our
/// own job it can never reach the user's own Chrome windows.
#[cfg(windows)]
#[derive(Debug)]
struct JobObject {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: a job-object handle is an owned OS handle with no thread affinity —
// Windows lets job objects be manipulated from any thread. Access is serialized
// by the AppRuntime state mutex, and `Drop` closes it exactly once.
#[cfg(windows)]
unsafe impl Send for JobObject {}
#[cfg(windows)]
unsafe impl Sync for JobObject {}

#[cfg(windows)]
impl JobObject {
    /// Creates a kill-on-close job and puts `child` in it. Returns `None` when
    /// any step fails, which leaves the caller to fall back to `taskkill /T`.
    fn create_for(child: &Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        // SAFETY: the job is created unnamed and with no security attributes, so
        // the returned handle is exclusively ours; it is closed exactly once in
        // `Drop`. Every call below is checked before its result is used, and an
        // early return drops `job`, which closes the handle.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let job = Self { handle };

            let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    ..Default::default()
                },
                ..Default::default()
            };
            let configured = SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast::<std::ffi::c_void>(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if configured == 0 {
                return None;
            }

            // Assigning the browser also enrols every process it spawns
            // *afterwards*, which is what makes the tree kill complete.
            if AssignProcessToJobObject(job.handle, child.as_raw_handle()) == 0 {
                return None;
            }
            Some(job)
        }
    }

    /// Kills every process in the job. Scoped to the job, so nothing outside our
    /// Chrome tree is affected.
    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        // SAFETY: `self.handle` is a live job handle for as long as `self` is
        // alive, and `TerminateJobObject` only ever affects members of that job.
        unsafe {
            TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for JobObject {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;

        // SAFETY: `self.handle` came from `CreateJobObjectW` and is closed
        // exactly once, here.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::process::Command;

    /// True while `pid` exists. Signal 0 only performs the permission and
    /// existence check — it never delivers anything.
    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        // SAFETY: `kill` with signal 0 is a pure liveness probe.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// Waits for `pid` to disappear, allowing for orphan reaping.
    #[cfg(unix)]
    fn wait_for_exit(pid: i32, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if !process_exists(pid) {
                return true;
            }
            thread::sleep(TREE_POLL_INTERVAL);
        }
        !process_exists(pid)
    }

    /// Spawns an isolated group leader that forks a long-lived grandchild and
    /// reports both. This is the shape of a real Chrome launch — one browser
    /// plus renderer/GPU children — and it is what a single-PID kill leaks.
    ///
    /// The isolation is applied through the same helper production uses, so the
    /// group really is the leader's own; a test that skipped it would be
    /// asserting on a group that does not exist.
    #[cfg(unix)]
    fn spawn_tree_with_grandchild() -> (ChromeProcessOwner, i32) {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("sleep 300 & echo $!; wait")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        super::super::configure_process_isolation(&mut command);

        let mut child = command.spawn().expect("spawn tree");

        // The grandchild reports its own pid before the shell blocks in `wait`.
        let stdout = child.stdout.take().expect("stdout pipe");
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        std::io::BufRead::read_line(&mut reader, &mut line).expect("read grandchild pid");
        let grandchild: i32 = line.trim().parse().expect("grandchild pid");

        let process_group = child.id() as i32;
        // Via `adopt`, so the live-group slot is published exactly as it is in
        // production — the slot is what the signal/panic paths rely on.
        let owner = ChromeProcessOwner::adopt(child);
        assert_eq!(owner.process_group, process_group);
        (owner, grandchild)
    }

    /// The MP-22 regression: terminating a managed tree must kill the
    /// descendants, not just the browser process.
    #[cfg(unix)]
    #[test]
    fn terminate_tree_kills_every_process_in_the_group() {
        let (mut owner, grandchild) = spawn_tree_with_grandchild();
        let leader = owner.id().expect("leader pid") as i32;

        assert!(process_exists(leader), "leader should be running");
        assert!(process_exists(grandchild), "grandchild should be running");

        owner.terminate_tree();

        assert!(!owner.is_alive(), "browser must be reaped");
        assert!(
            wait_for_exit(leader, Duration::from_secs(5)),
            "leader survived teardown"
        );
        assert!(
            wait_for_exit(grandchild, Duration::from_secs(5)),
            "grandchild survived — teardown is still single-PID"
        );
    }

    /// Negative control for the test above.
    ///
    /// Reproduces the *old* behaviour — kill only the direct child — and asserts
    /// the grandchild **survives**. If this ever fails, the tree test is not
    /// measuring tree termination and proves nothing.
    #[cfg(unix)]
    #[test]
    fn control_single_pid_kill_leaves_the_grandchild_running() {
        let (mut owner, grandchild) = spawn_tree_with_grandchild();

        let child = owner.child.as_mut().expect("child");
        child.kill().expect("kill leader only");
        let _ = child.wait();

        assert!(
            process_exists(grandchild),
            "control failed: the grandchild died without a group kill, so the \
             tree test cannot be measuring what it claims"
        );

        // Clean up the control's orphan so it cannot outlive the suite.
        // SAFETY: killing a pid this test spawned and just verified.
        unsafe {
            libc::kill(grandchild, libc::SIGKILL);
        }
        assert!(wait_for_exit(grandchild, Duration::from_secs(5)));
    }

    /// Cleanup must be idempotent: every teardown path (leave, restart,
    /// shutdown, `Drop`) may run it, and repeated runs must be no-ops.
    #[cfg(unix)]
    #[test]
    fn terminate_tree_is_idempotent() {
        let (mut owner, grandchild) = spawn_tree_with_grandchild();

        owner.terminate_tree();
        owner.terminate_tree();
        owner.terminate_tree();

        assert!(!owner.is_alive());
        assert!(wait_for_exit(grandchild, Duration::from_secs(5)));
        assert!(owner.id().is_none(), "a finished owner holds no child");
    }

    /// A replacement session must not let a stale teardown clear the live-tree
    /// slot, and must not be killed by it (the launch/leave/restart race).
    #[cfg(unix)]
    #[test]
    fn terminating_a_replaced_tree_leaves_the_new_tree_alone() {
        let (mut first, first_grandchild) = spawn_tree_with_grandchild();
        let (mut second, second_grandchild) = spawn_tree_with_grandchild();
        assert_ne!(first.process_group, second.process_group);

        // The second launch published its own group; tearing down the first must
        // not unpublish it.
        assert_eq!(
            LIVE_CHROME_GROUP.load(Ordering::SeqCst),
            second.process_group
        );
        first.terminate_tree();
        assert_eq!(
            LIVE_CHROME_GROUP.load(Ordering::SeqCst),
            second.process_group,
            "a stale teardown cleared the live slot"
        );
        assert!(second.is_alive(), "the replacement tree was killed");

        second.terminate_tree();
        assert_eq!(LIVE_CHROME_GROUP.load(Ordering::SeqCst), 0);

        assert!(wait_for_exit(first_grandchild, Duration::from_secs(5)));
        assert!(wait_for_exit(second_grandchild, Duration::from_secs(5)));
    }

    /// The group signal must never be aimed at Movie Party's own group, or a
    /// teardown would kill the app itself.
    #[cfg(unix)]
    #[test]
    fn signal_group_refuses_to_signal_our_own_group() {
        let (mut owner, grandchild) = spawn_tree_with_grandchild();
        let mut impostor = ChromeProcessOwner {
            child: owner.child.take(),
            process_group: own_process_group(),
        };

        // Would take the test runner down if the guard were missing.
        impostor.signal_group(HARD_SIGNAL);
        assert!(impostor.is_alive(), "the guard did not hold");

        // Hand the tree back and clean it up properly.
        owner.child = impostor.child.take();
        owner.terminate_tree();
        assert!(wait_for_exit(grandchild, Duration::from_secs(5)));
    }

    /// `kill_live_chrome_tree` is the signal/panic path: it must kill the group
    /// the slot names and clear the slot.
    #[cfg(unix)]
    #[test]
    fn kill_live_chrome_tree_kills_the_published_group() {
        let (mut owner, grandchild) = spawn_tree_with_grandchild();
        let leader = owner.id().expect("leader pid") as i32;
        assert_eq!(
            LIVE_CHROME_GROUP.load(Ordering::SeqCst),
            owner.process_group
        );

        kill_live_chrome_tree();

        assert_eq!(LIVE_CHROME_GROUP.load(Ordering::SeqCst), 0);
        assert!(
            wait_for_exit(grandchild, Duration::from_secs(5)),
            "the published group survived the emergency kill"
        );

        // The leader is a zombie until somebody reaps it — the owner still
        // holds it, and reaping is the owner's job. That must still work.
        owner.terminate_tree();
        assert!(!owner.is_alive());
        assert!(
            wait_for_exit(leader, Duration::from_secs(5)),
            "the emergency path left an unreaped leader"
        );
    }

    /// Windows smoke test. The *completeness* of the Windows tree kill rests on
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (an OS guarantee, not something a
    /// unit test can out-observe); what this pins down is that the job is really
    /// created and assigned, and that terminating it ends the process.
    #[cfg(windows)]
    #[test]
    fn job_object_is_assigned_and_terminating_it_ends_the_process() {
        let mut command = crate::process::quiet_command("cmd");
        command
            .args(["/C", "ping -n 30 127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("spawn tree");

        let job = JobObject::create_for(&child);
        assert!(
            job.is_some(),
            "kill-on-close job object could not be created/assigned; \
             terminate_tree would fall back to `taskkill /T`"
        );

        job.expect("job").terminate();

        let start = std::time::Instant::now();
        let mut exited = false;
        while start.elapsed() < std::time::Duration::from_secs(10) {
            if matches!(child.try_wait(), Ok(Some(_))) {
                exited = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(exited, "job termination did not end the process");
        let _ = child.wait();
    }
}
