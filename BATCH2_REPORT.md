# Batch 2 — Chrome/provider process lifecycle hardening

**Status:** complete, verified. `v0.9.8` (`bb22577`) untouched — no tag created, no tag moved,
no release edited, no force-push. Provider Shared untouched. No experimental capture/streaming
enabled. No unrelated P3 findings addressed.

---

## 1. Verified root cause

All three findings were confirmed against the real code **before** any change was made.

### MP-20 — the browser is leaked on the CDP-failure path

`src-tauri/src/providers/chrome/mod.rs`, `launch_managed_chrome`:

```rust
let mut child = chrome_command(&plan)
    .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
    .spawn()
    .map_err(|error| ManagedChromeError::Process(error.to_string()))?;
wait_for_cdp_or_child_exit(&mut child, plan.cdp_port, Duration::from_secs(10))?;  // <-- LEAK
Ok(ManagedChromeSession { child, plan })
```

The second `?` drops `child` **bare** — no `kill()`, no `wait()`. `ManagedChromeSession`'s `Drop`
impl never runs, because the session that would have owned the child was never constructed. So a
`CdpTimeout` (10 s) leaves a **running Chrome plus an unreaped zombie**.

Precision matters here: the *child-exit* branch of `wait_for_cdp_or_child_exit` already called
`try_wait()`, so that branch reaped its process. The genuine leak was the **timeout** branch, where
`try_wait()` returned `Ok(None)` on every poll and the process stayed alive until the `Child` was
dropped. The fix therefore covers both, but only one of them was a live leak.

### MP-22 — teardown was single-PID

`ManagedChromeSession` held one `Child`; `close_gracefully` ended with:

```rust
let _ = self.child.kill();
let _ = self.child.wait();
```

A crate-wide search found **zero** occurrences of `process_group`, `setpgid`, `killpg`,
`CREATE_NEW_PROCESS_GROUP`, `JobObject` or `taskkill`, and `chrome_command` was minimal:

```rust
fn chrome_command(plan: &ChromeLaunchPlan) -> Command {
    let mut command = Command::new(&plan.executable);
    command.args(plan.args());
    command
}
```

Chrome is never one process: the browser spawns renderer, GPU, utility, zygote and crashpad
children. Killing only the browser PID orphaned them, and orphaned renderers keep the dedicated
provider profile locked — which is precisely what makes the *next* launch of that provider fail or
show a "restore pages" banner.

### MP-23 — cleanup was reachable only from `RunEvent::Exit`

`lib.rs` had exactly one cleanup call site:

```rust
if matches!(event, tauri::RunEvent::Exit) {
    let runtime = app_handle.state::<app_runtime::AppRuntime>();
    runtime.close_provider_session();
}
```

SIGKILL, SIGTERM, an abort or a panic bypassed it entirely, leaving the whole tree running.

---

## 2. Lifecycle design chosen

One owner, one exit point. `src-tauri/src/providers/chrome/process.rs` (new) introduces
`ChromeProcessOwner`, and it is the only thing in the codebase permitted to signal or reap a Chrome
process. `ManagedChromeSession` now holds it instead of a bare `Child`.

**Ownership (requirement 1).** `ChromeProcessOwner::spawn` is the sole Chrome spawn path —
`launch_managed_chrome` is its only production caller, and `chrome_command` is private.

**Failure paths (requirement 2).** The owner takes responsibility the instant the process exists, so
no later `?` can drop a live tree on the floor. `launch_managed_chrome_with_timeout` terminates
explicitly when CDP never comes up, and `Drop for ChromeProcessOwner` is the backstop for any path
someone forgets later. The CDP startup budget is now an explicit parameter
(`launch_managed_chrome_with_timeout`) so both failure paths are testable in milliseconds rather
than by waiting out the production 10 s.

**Idempotency (requirement 8).** `terminate_tree` is the single exit point. Its latch is
`self.child.take()`: the first call owns the work, every later call returns immediately. `close()` /
`close_gracefully()` / `DeferredTeardown::run()` / `Drop` can all run in any order.

**Races (requirement 7).** The tree is published in a single
`static LIVE_CHROME_GROUP: AtomicI32`. One slot is *correct*, not a simplification:
`AppRuntimeState` holds at most one `chrome_session`, so at most one managed tree is ever live. It is
cleared with `compare_exchange` so a stale teardown cannot unpublish a replacement tree. Teardown
order is: take the child → unpublish → signal → reap, which is also why the direct child is killed
and reaped *after* the group signals (reaping first would close the window in which the group id is
still meaningful).

**Not killing unrelated processes (requirement 6).** On POSIX the group is signalled only after
rejecting `pgid <= 1` (init, and the "caller's own group" sentinel) and `pgid == getpgrp()` —
signalling our own group would take Movie Party down with it. On Windows the job object is scoped to
our own tree, and the fallback is `taskkill /PID <pid> /T /F` — deliberately **not**
`/IM chrome.exe`, which would also kill the user's own browser.

**Blocking teardown off the mutex (requirement 10).** Already satisfied for
`close_provider_session`, `store_chrome_session`, `store_launched_provider`,
`store_launched_generic_link` and `DeferredTeardown`. One violation remained and is fixed:
`AppRuntime::open_provider_browser` did `state.chrome_session = Some(session)` **while holding the
lock**, which drops the replaced session (a CDP round-trip plus a whole-tree teardown) under the
global mutex. It now uses the same take → drop-outside-the-lock → bump-generation pattern as its
three sibling functions.

**Obsolete sessions (requirement 9).** Already satisfied by the existing `chrome_generation` /
`chrome_session_may_be_reinserted` guard, and **not duplicated** — the `open_provider_browser` fix
above simply brings that call site into line with the other three.

---

## 3. Platform-specific behaviour

| Concern | POSIX (macOS) | Windows |
|---|---|---|
| Spawn isolation | `process_group(0)` — new group whose id is the child's pid | `CREATE_NEW_PROCESS_GROUP` + kill-on-close job object assigned immediately after spawn |
| Tree termination | `killpg(SIGTERM)` → 500 ms grace → `killpg(SIGKILL)` | `TerminateJobObject`; falls back to `taskkill /PID <pid> /T /F` when the job cannot be created/assigned |
| Reaping | `Child::kill()` + `Child::wait()` — always | same |
| "Movie Party died without cleaning up" | SIGINT / SIGTERM / SIGHUP handlers + panic hook, all killing the published group | `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` — the OS kills the tree whenever the last job handle closes, covering **every** exit path including a forced kill |
| Signal hardening install | `install_shutdown_hardening()` (Unix) | no-op (`install_shutdown_hardening()` empty) |

The Unix signal handler is deliberately minimal: it performs only async-signal-safe work (one
lock-free atomic swap plus `killpg`), then restores `SIG_DFL` and re-raises, so Movie Party still
dies *from the signal it received* and the exit status stays truthful.

`Taskkill` is spawned through `crate::process::quiet_command`, not `Command::new` — it is a console
binary, and a bare spawn flashes a black console window on the user's screen (an explicit
convention in this codebase).

### Dependencies

`Cargo.toml` gains target-gated `libc` (unix) and `windows-sys 0.61` (windows). **The lockfile diff
is exactly two lines** — both crates were already present in the graph as transitive dependencies, so
no new code entered the build; this only makes the parts we call explicit. The Windows feature set
(`Win32_Foundation`, `Win32_Security`, `Win32_System_JobObjects`, `Win32_System_Threading`) was
verified against the cached crate source, not guessed: `CreateJobObjectW` is
`#[cfg(feature = "Win32_Security")]`, and `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` is
`#[cfg(feature = "Win32_System_Threading")]` because it embeds `IO_COUNTERS`.

---

## 4. Tests added

Nine tests; the two that launch real Chrome remain `#[ignore]`d as before.

**`process.rs`**

| Test | Covers |
|---|---|
| `terminate_tree_kills_every_process_in_the_group` | MP-22 — a real group leader forks a grandchild; both must die |
| `control_single_pid_kill_leaves_the_grandchild_running` | **negative control** for the above — reproduces the old single-PID kill and asserts the grandchild *survives* |
| `terminate_tree_is_idempotent` | requirement 8 — repeated cleanup |
| `terminating_a_replaced_tree_leaves_the_new_tree_alone` | requirement 7 — a stale teardown must not kill the replacement tree nor unpublish it |
| `signal_group_refuses_to_signal_our_own_group` | requirement 6 — the guard that stops teardown killing Movie Party itself |
| `kill_live_chrome_tree_kills_the_published_group` | MP-23 — the signal/panic path, and that the owner still reaps afterwards |
| `job_object_is_assigned_and_terminating_it_ends_the_process` | Windows (`cfg(windows)`) — job created, assigned, and terminating ends the process |

**`chrome/mod.rs`**

| Test | Covers |
|---|---|
| `launch_timeout_kills_and_reaps_the_whole_tree` | MP-20 — a fake Chrome that never opens CDP; both the browser and its child must be gone |
| `launch_child_exit_is_reaped_not_leaked` | MP-20 — a fake browser that forks a child then exits; the orphan must still be killed |
| `close_gracefully_is_idempotent` | requirement 8 |

The fake Chrome records its own pid and its child's to a file, so the tests assert on **actual
process liveness** (`kill(pid, 0)`), not on internal state. The pre-existing
`close_gracefully_*` tests were rewritten to spawn through the same isolation helper production uses
and are now explicitly `#[cfg(unix)]` — they spawn POSIX programs (`sleep`), so they could never have
passed on the Windows leg.

---

## 5. Validation results

All Rust commands use the pinned `+1.98.0` toolchain, matching CI.

| Gate | Result |
|---|---|
| `cargo fmt --check` | **clean** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **clean** |
| `cargo test --no-fail-fast -- --test-threads=1` | **532 passed / 1 failed** |
| `cargo test --lib providers::chrome` | **18 passed / 0 failed** / 2 ignored |
| `vitest run` | **278 passed / 22 files** |
| `eslint . --max-warnings=0` | **clean** (no output) |
| `tsc --noEmit` | **clean** (no output) |
| `vite build` | **exit 0** |
| `cargo check --target x86_64-pc-windows-msvc` (scratch harness) | **compiles clean** |

### The one failure, and why it is not this change

`real_sw_render_test::bundled_libmpv_sw_render_api_produces_decoded_frames` — libmpv's SW render
produces frames but a zeroed pixel buffer in this sandbox. **Deterministic, not flaky**: re-run 3×,
it failed 3/3 with the identical message (*"rendered frame buffer must contain non-zero pixel data"*)
at `real_sw_render_test.rs:292`. A/B-verified pre-existing in Batch 1; the file contains no reference
to `AppRuntime` or to any Chrome code.

The contrast matters: one of these two failures is deterministic and one is a flake, and treating
them the same would have sent a future reader down the wrong path.

`real_native_surface_e2e::bundled_libmpv_renders_onto_real_calayer_through_production_player` is
**flaky** and is worth recording: it panics at `real_native_surface_e2e.rs:177` with
`seek: PlaybackError { message: "error running command" }`, *after* successfully printing
`RENDER PASS: 63 frames in 3.0s (21 fps)`. Observed **2 fail / 1 pass** with Batch 2 present, and
**2 fail / 1 pass with Batch 2 reverted** — identical line, identical error. It also passed on a
separate full-suite run. This is a pre-existing timing flake in the libmpv seek path.

### Negative controls (a test that cannot fail proves nothing)

- **MP-20.** Removing the explicit `terminate_tree()` from the failure path alone did **not** make
  the tests fail — the `Drop` backstop silently covered it. Disabling *both* the explicit call and
  the `Drop` backstop made both MP-20 tests FAIL with the intended
  `"the pid … survived a failed launch — the tree leaked (MP-20)"`. Both mechanisms had to be removed
  before the control had teeth.
- **MP-22.** The tree test ships with its own inline control (above) that proves a single-PID kill
  leaves the grandchild alive, so the tree test cannot pass vacuously.
- **Windows.** The real crate cannot be cross-checked here: `cargo check --target
  x86_64-pc-windows-msvc` dies inside `aws-lc-sys`' C build script with
  `fatal error: 'windows.h' file not found` — a build-script failure, not a Rust one. The Windows
  code was therefore verified in a standalone scratch crate (`/tmp/wincheck`) carrying a verbatim
  copy of the `#[cfg(windows)]` code with only `windows-sys` as a dependency. It **compiles clean for
  `x86_64-pc-windows-msvc`**, including `assert_send_sync::<ChromeProcessOwner>()` — which matters,
  because a raw `HANDLE` is not `Send`/`Sync` and the owner lives inside `AppRuntimeState`, so
  without the explicit `unsafe impl` the Windows leg would not have compiled at all.
- **A/B methodology trap.** A pristine `git worktree` **silently skipped** the libmpv tests
  (`ok` in 0.00 s) because `src-tauri/mpv_runtime/libmpv.dylib` is gitignored — a false green, not a
  control. The A/B was redone by reverting only Batch 2's files inside the real tree, with `/tmp`
  backups verified by `shasum` before and after.

---

## 6. Remaining limitations

1. **macOS cannot be made bulletproof against `SIGKILL` of Movie Party.** There is no kernel
   parent-death mechanism on macOS (Linux has `PR_SET_PDEATHSIG`, Windows has the job object). What
   is covered on macOS: `RunEvent::Exit`, SIGINT, SIGTERM, SIGHUP, and panics. A hard `kill -9` of
   Movie Party still leaves the tree running. Stated plainly rather than papered over.
2. **The POSIX group signal is addressed by the browser's pid.** It assumes that pid has not been
   recycled as a *new* group leader in the interval between reaping the browser and the `killpg` —
   microseconds on the teardown path, at most the crash watcher's 2 s poll on the crash path. macOS
   and Linux allocate pids sequentially, so a collision in that window is not reachable in practice.
   Signalling the group is nonetheless the only way to reach children that have already been
   reparented, so the alternative (leaving them running) is strictly worse. Documented on
   `signal_group`.
3. **Windows runtime behaviour is unverified.** The code compiles for `x86_64-pc-windows-msvc` and
   the job-object test will run on the Windows CI leg; it has not been *executed* on this machine.
   This report does not claim otherwise.
4. **If the job object cannot be created or assigned** (e.g. Movie Party is itself inside a job that
   forbids nesting), Windows falls back to `taskkill /T`. That is complete in practice but is not the
   OS-level guarantee, and it is a best-effort call whose failure is ignored.
5. **`real_native_surface_e2e` remains flaky** in this sandbox, independent of this batch.

---

## 7. Confirmation of the release freeze

- `HEAD` = `bb225778434e3f07923e03e85d7d7cc1db79146c` = `origin/main` = the commit `v0.9.8` resolves to.
- `v0.9.8` is an **annotated** tag (tag object `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3`, tagger date
  2026-09-17 19:29:15 +0530, unchanged). Note `git rev-parse v0.9.8` prints the *tag object*, not the
  commit — use `git rev-parse v0.9.8^{commit}` to compare against `HEAD`.
- No tag was created, moved, deleted or force-pushed. No branch was rewritten.
- `v0.9.0`, `v0.9.4`, `v0.9.5`, `v0.9.6` (lightweight) and `v0.9.8` all still present.
- The GitHub `v0.9.8` Release was not touched.
- Provider Shared was not implemented or modified.
- No experimental capture/streaming was enabled.
- No unrelated P3 findings were fixed.

All Batch 1 and Batch 2 work remains **uncommitted** in the working tree.

### Files changed by Batch 2

| File | Change |
|---|---|
| `src-tauri/src/providers/chrome/process.rs` | **new** — `ChromeProcessOwner`, job object, shutdown hardening, 7 tests |
| `src-tauri/src/providers/chrome/mod.rs` | session owns the tree; `launch_managed_chrome_with_timeout`; `close_gracefully` → `terminate_tree`; `configure_process_isolation`; 3 tests |
| `src-tauri/src/lib.rs` | `install_shutdown_hardening()` before the event loop |
| `src-tauri/src/app_runtime.rs` | `open_provider_browser` no longer tears down under the global lock |
| `src-tauri/Cargo.toml`, `Cargo.lock` | target-gated `libc` / `windows-sys` (+2 lockfile lines) |

### Side effects

- `/tmp/wincheck` — the Windows compile-check harness (scratch crate). Safe to delete; the recipe is
  recorded in the `movie-party-verify` skill.
- `/tmp/b2bak`, `/tmp/mod.rs.bak`, `/tmp/process.rs.bak` — A/B and negative-control backups, all
  verified by `shasum` and superseded.
- A temporary `git worktree` at `/tmp/mp-baseline` and its target dir were created for the A/B and
  have been removed (`git worktree list` shows only the main checkout plus a pre-existing, unrelated
  `prunable` entry at `/private/tmp/mp_head_check` that this batch did not create).
