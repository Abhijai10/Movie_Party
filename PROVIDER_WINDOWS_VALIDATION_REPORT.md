# Batch 1 — supplemental Windows validation

Date: 2026-10-20 (IST). Starting commit:
`5205d4748fd6b86b056e7fdb172a3ec28c6d4912`. Validation branch:
`codex/provider-windows-validation`. The final commit is the commit containing
this report and will be stated in the completion message.

This follow-up retries the Windows checks left unavailable in
`PROVIDER_PROFILE_BATCH1_REPORT.md`. Historical macOS results remain in that
report. Provider Shared remains disabled; no tags, release, installer, protocol,
dependency or provider authentication changes are authorized by this validation.

## Environment and provenance

The real guest is Parallels **Windows 11 main**, Windows 11 ARM64, build
10.0.26200.9445, with four virtual CPUs and 4 GB RAM. The requested supported Windows
target is `x86_64-pc-windows-msvc`; execution therefore uses Windows x64 emulation
on ARM64, not a physical x64 machine. Tests run as the logged-in user in desktop
session 1, with `UserInteractive=True`, rather than the SYSTEM session.

The source was exported from the exact starting commit into a fresh
`C:\MoviePartyValidation\5205d47` checkout. Its archive SHA-256 is
`a9d0ef96489baed6666e2b0b3e8f17f9330f67292332443d4206d5a1f14ae05e`.
This is a committed-source archive without `.git`; a provenance file records the
full source SHA. Ordinary browser profiles were not copied, inspected or used for
CDP. Visible Chrome tests use disposable managed profiles and a local test origin.

Installed tools: Rust 1.98.1 (host `aarch64-pc-windows-msvc`, x64 target installed),
Visual Studio 2022 Build Tools 17.14.37516.0, Node 24.18.0 and the repository-pinned
pnpm 11.10.0. Rust's patch version differs from CI's 1.98.0. Public locked
crates.io archives/index entries were transferred from the host cache for offline
compilation; no Cargo credentials/configuration were copied.

The repository CI's pinned Windows libmpv development archive was downloaded from
the upstream release, hash-verified, and extracted inside the validation checkout:
`mpv-dev-x86_64-20261019-git-6a262d11f6.7z`, SHA-256
`ca8604754170db6f1b23139d8449782b1ae294a883211131954d30085985f77b`.
The same DLL is staged as `mpv_runtime\mpv-2.dll` for production discovery and
`mpv_runtime\libmpv.dll` for the existing integration-test prerequisite. No runtime
binary is committed.

## Recovery and test harness

The VM froze during initial concurrent setup/build work. Guest commands and
desktop control stopped responding; the user confirmed the freeze and explicitly
authorized a restart. A normal restart did not complete; Parallels force-stop then
start succeeded. The Windows desktop and saved frontend logs were verified after
recovery. The host has 8 GB RAM and had about 8.7 GB swap in use, so subsequent
compilation uses one job and debug information disabled. This interruption is not
reported as a passing Rust or Chrome test.

After a later idle/interrupted period, desktop input and even a basic guest CLI
command again stopped responding before a smoke-launch window could be confirmed.
A second recovery stop/start, under the user's existing restart authorization,
succeeded. All completed test logs had already been retrieved to the Mac. The
recurring VM-control instability is an environment limitation; no product crash
was established from those stalled launch attempts.

PowerShell 5 initially treated normal native-command stderr as terminating errors;
the wrapper was corrected to capture output through `cmd.exe` and preserve exit
codes. A long encoded guest command also stalled before creating the test process;
the script was transferred over a single fixed private-VM HTTP route and invoked
from disk. Transfer servers expose only named source/dependency/test artifacts,
not directories or browser data. These harness failures are distinct from product
failures.

## Checks and results

- Windows `pnpm install --frozen-lockfile`: passed; no dependency changes.
- Windows `pnpm lint`: passed, zero warnings.
- Windows `pnpm test`: **331 passed, zero failed**, 27 files.
- Windows `pnpm build`: passed, including TypeScript. The existing Vite chunk-size
  warning remains.

- Baseline Windows `cargo test --offline --locked --target
x86_64-pc-windows-msvc -- --test-threads=1`: **593 passed, zero failed, three
  ignored** (506 library tests and 87 integration tests). No runtime skip flags.
  The real Windows child-HWND libmpv test executed and passed. macOS-only test
  binaries select zero tests on Windows; their macOS results are in the original
  Batch 1 report.
- With `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1`, exact ignored test
  `providers::chrome::tests::real_chrome_launches_and_evaluates_cdp`: **passed**
  inside the logged-in Windows desktop session.
- Exact ignored test
  `providers::chrome::tests::real_chrome_profile_storage_survives_managed_close_and_relaunch`:
  **passed**, exercising real Chrome and local/sessionStorage sentinels across
  managed close and relaunch. No provider login or cookie extraction.
- Initial Windows formatting/Clippy invocations failed because the installed
  toolchain lacked those optional components. Matching official components were
  subsequently hash-verified and staged in the validation-only toolchain.
- macOS `cargo fmt --check` and `git diff --check`: passed for the added test.
- `pnpm exec prettier --check PROVIDER_WINDOWS_VALIDATION_REPORT.md`: passed
  after formatting the report.

- Final Windows suite with the added native-lock regression: **594 passed, zero
  failed, three default ignores** (507 library, 87 integration tests). The added
  `native_live_lock_prevents_reset_and_legacy_move` test and the Windows private
  job-object test both executed and passed. Both visible Chrome checks were
  repeated against this version and passed again.
- The Create Party crash regressions
  `provider_workers_replace_and_cleanup_without_entered_runtime`,
  `provider_workers_replace_and_cleanup_inside_tokio` and
  `scheduler_spawn_is_safe_without_entered_tokio_reactor` executed and passed.
- Windows `cargo fmt --check`: passed after installing the matching component.
- Windows `cargo clippy --offline --locked --target x86_64-pc-windows-msvc
--all-targets --all-features -- -D warnings`: passed.
- Windows `cargo build --offline --locked --target x86_64-pc-windows-msvc
--features tauri/custom-protocol`: passed. This is an unbundled debug app with
  embedded frontend assets, not an installer or release build.

The three default ignores are the two visible Chrome checks explicitly run above
and `real_youtube_provider_sync_uses_chrome_cdp`, which was not run. Authenticated
provider playback was not exercised.

The built debug app was launched in session 1 with the verified runtime staged
beside the executable and without the libmpv environment override. Its native
window, startup splash and full Movie Party home screen were visually confirmed
on Windows. A process probe reported the main window responsive. Screenshot:
`/tmp/movie-party-windows-home.png`. This confirms basic desktop startup, not a
complete two-device or authenticated provider workflow. Desktop input automation
failed with the Parallels control surface, so interactive navigation was not
verified. Cold startup was exceptionally slow in this VM: captures showed the
first window around 21:22 and the home screen by 21:30 IST. Those observation
times are not a measured startup benchmark, and the precise cause is unresolved.
The smoke app then closed normally through its main window, after verifying its
executable, PID and process start time; no ordinary Chrome process was targeted.

All Windows Rust commands ran with `CARGO_BUILD_JOBS=1`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, and the verified x64
runtime selected through `MOVIE_PARTY_LIBMPV_PATH`. Commands were issued from
`src-tauri` through the host's RTK-wrapped Parallels guest CLI. The exact separately
authorized Chrome invocations add `--offline --locked --target
x86_64-pc-windows-msvc --lib` before their full test names and
`-- --ignored --exact --test-threads=1 --nocapture` after them.

Final source differs from the starting archive only by the added Windows test in
`profile.rs`, whose transferred SHA-256 was verified as
`d25277aea1b906450f582185299d9b33f66492c2c41bec6d2dfc675b8d6a437d`.
Full logs and exit-code evidence were retrieved to
`/tmp/movie-party-windows-results/`; the guest copies remain in the validation
checkout. The baseline log is retained separately from the final run.
The task-owned temporary transfer servers were stopped after evidence collection.

## Files and regression scope

- `src-tauri/src/providers/chrome/profile.rs`: adds the Windows counterpart to
  the existing native-lock regression. A real Windows file handle denies sharing,
  migration/reset must refuse and retain a sentinel, and releasing the handle
  permits migration/reset. The test exercises actual filesystem behavior without
  automating provider authentication or changing production behavior.
- `PROVIDER_WINDOWS_VALIDATION_REPORT.md`: this supplemental report.

The pre-existing untracked
`FINAL_MAIN_RECONCILIATION_AND_RC_BUILD_REPORT.md` is left untouched.

## Compatibility, remaining risks and next batch

Production code is unchanged by this follow-up; the only Rust addition is a
Windows-only regression test. No wire format, protocol version, application
version, profile layout, dependencies or mandatory infrastructure changed. The
recent provider and Create Party fixes remain intact. Provider Shared stays OFF.
The existing v0.9.8 tag is preserved. No tags, releases, pushes or installers were
created; the debug executable stays inside the VM validation directory.

Remaining release gates: native physical x64 Windows desktop use, a complete
interactive Windows workflow, authenticated provider session expiry, real app
update/reboot persistence with a deliberately seeded provider session, and
physical two-device playback. The recovery reboots above were not a seeded
Chrome-persistence experiment. Neither storage sentinels nor compilation establish
authenticated cookie survival or provider authorization. The unusually slow
startup and recurring Parallels/guest-control stalls need a clean-environment
recheck before a Windows desktop reliability claim.

The next scoped provider implementation batch **can safely begin** from this
baseline: the final serialized suite, new Windows native-lock regression, both
visible Chrome checks and required frontend/Rust checks pass. This is not a
release-readiness approval. Later provider selection propagation, stronger video
readiness and semantic playback work remain ordered by `PROVIDER_BASELINE_AUDIT.md`.
