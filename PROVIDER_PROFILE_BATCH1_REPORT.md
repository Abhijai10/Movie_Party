# Batch 1 — persistent managed Chrome and recovery

Date: 2026-10-20 (IST).

Starting commit: `2444b9e8018a67591601b042211742438861e85b` on
`codex/provider-baseline-audit`. Implementation branch:
`codex/persistent-provider-profiles`. The final SHA is reported in the completion
message; it is the commit containing this report. The pre-existing untracked
`FINAL_MAIN_RECONCILIATION_AND_RC_BUILD_REPORT.md` was left untouched.

## Result and choices

Managed profiles now use Tauri's supported `app.path().app_local_data_dir()`:

- macOS: `~/Library/Application Support/app.movieparty.desktop/managed-chrome/profiles/<provider>`.
- Windows: `%LOCALAPPDATA%\app.movieparty.desktop\managed-chrome\profiles\<provider>`.

The application identifier, not the app version or installation directory, anchors
the path. Updating Movie Party or normally restarting the machine does not select
a new profile. Each provider has its own directory; generic links have a separate
`generic-link` directory. Chrome owns the cookies, encryption and storage in those
directories. No ordinary Chrome profiles, passwords, cookies or encryption keys
are imported or inspected.

The old `temp_dir()/MoviePartyProfiles/<provider>` path is used **only** for a
one-time relocation of Movie Party's legacy managed directory. A rename, rather
than a credential copy, preserves its contents when the destination has not been
used. Existing current data and reset markers take precedence. Migration refuses
a live native Chrome lock, symlinks and invalid paths; a failed rename leaves the
original intact. Cross-volume migration deliberately fails instead of copying
credential stores or overwriting data.

Chrome launches with a dedicated `--user-data-dir`, loopback debugging,
`--remote-debugging-port=0` and `--restore-last-session`. Chrome chooses the port;
the launcher removes only the old debugging endpoint file, then requires the new
`DevToolsActivePort` port/browser identity to match `/json/version` on loopback
while the child remains alive. An unrelated listener or arbitrary successful HTTP
response cannot satisfy startup. Startup has a bounded 30-second cold-launch
budget; launch errors kill/reap the newly owned tree and retain profile data.

## Ownership, shutdown and recovery

`chrome/profile.rs` holds an OS file lock for the entire managed session lifetime.
The lock lives outside Chrome's data directory, so resetting data cannot remove
the ownership lock. A separately replaced, flushed JSON record stores provider,
canonical profile path, launched PID, process birth identity and launch state:
Launching, Running, Closed, UnexpectedExit or LaunchFailed. Unix profile directories
are private (0700), metadata is private (0600), and profile/lock symlinks are refused.

The existing Unix process-group and Windows job ownership remain authoritative.
PID records never authorize killing a process. A dead/stale record permits retry;
a still-live matching process returns an actionable busy error. If an uncatchable
Unix app crash leaves its browser alive, the user closes that Movie Party browser
and retries; the new app does not risk signalling a reused PID or ordinary Chrome.
Windows launches now fail closed if a private job cannot be assigned. The existing
job-object test remains a Windows-only execution gate.

The crash watcher starts before party creation as well as after attachment. It
survives temporary session removal during CDP work, rejects replaced generations,
cleans the dead tree outside the state lock, invalidates readiness and pauses the
room through the existing coordinator/recovery path. An unexpected browser exit
includes the user closing the managed Chrome window; this differs from an
intentional close requested by Movie Party. Recovery requires fresh readiness;
it does not automatically resume either participant or automate authentication.

Normal app exit waits for sessions temporarily owned by CDP workers to finish
graceful closure. CDP read/write timeouts are bounded at five seconds; shutdown
waits at most 25 seconds before the existing owned-tree emergency cleanup on Unix
(Windows has job ownership). Cleanup never deletes profiles. Create Party still
preserves the active browser via `keepProviderSession`, then attaches it and
restores workers. The original no-entered-Tokio-runtime crash fix is preserved.

Missing Chrome, process launch failure, inaccessible data, corrupt Local State,
corrupt ownership metadata, busy profiles and invalid endpoints produce explicit
errors. Normal launch never repairs these by deleting sign-in data. Chrome updates
use the installed executable discovered on each fresh launch, with the same profile;
per-user Windows Chrome installations are also discovered. A bad update/unsupported
profile can fail visibly and be retried or reset deliberately.

## Confirmed reset

Settings → Providers now has a separate **Reset provider session…** control.
Opening/cancelling confirmation sends no reset request. Confirming explains loss of
the selected provider's sign-in and browser data, disables repeat submission and
reports success or failure. Re-check remains a non-destructive operation. Open and
re-check failures now produce visible provider-specific recovery messages.

The new Rust command requires explicit confirmation and a known provider ID,
refuses active rooms, closes only the matching managed session and obtains an
exclusive profile lease before deleting its current managed directory. Native
singleton checks refuse a live Chrome profile. A confirmed reset can recover corrupt
profile/ownership metadata; an unconfirmed request cannot. Resetting one provider
does not reset another, and a reset marker prevents re-importing legacy sign-ins.

The Windows `lockfile` sharing probe follows Chromium's own file ownership behavior:
[Chromium ProcessSingleton implementation](https://chromium.googlesource.com/chromium/src/+/master/chrome/browser/process_singleton_win.cc).
This is source-based Windows handling, not Windows execution evidence.

## Files changed

| File                                        | Purpose                                                                                                 |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/providers/chrome/profile.rs` | Persistent profile lease, atomic records, migration, confirmed reset, native locks and regression tests |
| `src-tauri/src/providers/chrome/mod.rs`     | Verified ephemeral CDP endpoint, session restore, shutdown accounting and lifecycle/live-Chrome tests   |
| `src-tauri/src/providers/chrome/process.rs` | Refuse a Windows launch without private job ownership                                                   |
| `src-tauri/src/lib.rs`                      | Tauri app-data paths, serialized Chrome mutations, reset IPC and graceful app exit wait                 |
| `src-tauri/src/app_runtime.rs`              | Generation-safe crash monitoring, readiness invalidation, reset isolation and runtime regressions       |
| `src-tauri/src/providers/sync.rs`           | Map new profile errors through existing runtime error handling                                          |
| `src/backend/appRuntime.ts`                 | Reset IPC wrapper and actionable provider error copy                                                    |
| `src/backend/providerLifecycle.test.ts`     | Confirmation arguments and recovery-copy regressions                                                    |
| `src/components/AppShell.tsx`               | Surface profile recovery errors during creation/open/status flows                                       |
| `src/views/SettingsView.tsx`                | Reset control and visible open/re-check outcomes                                                        |
| `src/views/ProviderSessionReset.tsx`        | Confirm/cancel/pending/error/success reset UI                                                           |
| `src/test-ui/provider-reset.fixture.tsx`    | Isolated mocked-IPC browser test fixture; never imported by the production entry point                  |
| `tests/ui/provider-reset.html`              | Repeatable local browser fixture entry point                                                            |
| `PROVIDER_PROFILE_BATCH1_REPORT.md`         | This focused report                                                                                     |

## Validation and evidence

Host: macOS arm64, with the bundled libmpv runtime available. Live diagnostic
Chrome identified itself as `Chrome/155.0.8059.85`. All shell commands used the RTK
wrapper. No install, tag, release or CI dispatch was performed.

- `rtk pnpm test`: **331 passed, 0 failed**, 27 files. Seven new frontend tests.
- `rtk pnpm lint`: passed after fixture typing/lint fixes.
- `rtk pnpm build`: TypeScript and Vite passed. Existing >500 kB JavaScript chunk
  warning remains; no unrelated bundle/dependency changes were made.
- `rtk cargo fmt --check`: passed.
- `rtk cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Authoritative `cargo test -- --test-threads=1` through `rtk proxy`:
  **613 passed, 0 failed, 3 ignored** (library: 520 passed; integration binaries:
  93 passed). Full output: `/tmp/movie-party-b1-rust-final.log`. No libmpv skip
  flags were used; real macOS audio, EOF, native CALayer, playback and software
  rendering tests all executed and passed. The three default ignores are the two
  Chrome tests explicitly run below and the unexecuted live YouTube test.
- User explicitly authorized visible Chrome tests. With
  `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1`, the exact ignored test
  `providers::chrome::tests::real_chrome_launches_and_evaluates_cdp` **passed**:
  launch, target discovery, actual CDP evaluation, navigation and close.
- The new exact ignored test
  `providers::chrome::tests::real_chrome_profile_storage_survives_managed_close_and_relaunch`
  **passed** against real visible Chrome: a disposable localhost page retained both
  `localStorage` and restored-tab `sessionStorage` sentinels after Browser.close and
  a second managed launch using the same directory. No provider authentication,
  cookies, passwords or ordinary browser profiles were read.
- Browser UI fixture at `/tests/ui/provider-reset.html`: verified initial state,
  confirmation with zero requests, Cancel with zero requests, pending state with
  disabled controls, busy-profile feedback, retry and success (two explicitly
  confirmed mock requests). Mocked IPC cannot touch actual profile data. Desktop
  confirmation layout was visually inspected. Mechanical UI detector returned `[]`.
- `rtk git diff --check`: passed. Focused Prettier checks of the new component,
  browser fixture and report passed. Existing Settings formatting was preserved
  outside the functional edits.

The separately authorized live commands were:

```sh
rtk proxy env MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 cargo test --lib providers::chrome::tests::real_chrome_launches_and_evaluates_cdp -- --ignored --exact --test-threads=1 --nocapture
rtk proxy env MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 cargo test --lib providers::chrome::tests::real_chrome_profile_storage_survives_managed_close_and_relaunch -- --ignored --exact --test-threads=1 --nocapture
```

For the isolated reset UI fixture, run `rtk pnpm dev --host 127.0.0.1` and open
`http://127.0.0.1:1420/tests/ui/provider-reset.html`. Its response buttons operate
mocked IPC only. The full application preview requires the native runtime; the
isolated component check is not presented as a signed Tauri desktop test.

Intermediate failures are not hidden: the first visible Chrome attempt timed out
under the old ten-second startup budget. A disposable diagnostic launch showed a
valid endpoint and the exact CDP test then passed; the precise initial delay was
not established. Production now allows 30 seconds for cold/update startup, while
the short explicit launch-failure test budgets remain unchanged. One Rust fixture
initially referenced nonexistent macOS `/bin/true`; corrected to `/usr/bin/true`
and rerun. The browser fixture initially lay outside the typed lint project, then
exposed payload-union typing/lint errors; it was moved under `src/test-ui` and fixed.

Windows cross-check attempted:
`rtk cargo check --all-targets --all-features --target x86_64-pc-windows-msvc`.
It **could not complete** on this Mac: native dependency compilation failed for
missing Windows C headers (`assert.h`, `windows.h`; no MSVC SDK). Windows desktop,
job, singleton, keychain/encryption and session persistence checks have **not**
been executed. The platform-gated Windows native player test runs zero tests here.

## Compatibility, remaining risks and next batch

No protocol version/wire format, room architecture, release version, mandatory
infrastructure or network permissions changed. New app-local IPC:
`reset_provider_session(providerId, confirmed)`. New profile error codes:
MP-PROVIDER-005 (busy), 006 (profile/storage), 007 (confirmation). Internal Rust
`build_launch_plan` now takes no caller-specified CDP port; the validated session
supplies the actual port. Provider Shared remains OFF with its existing double gate.

Unverified: actual machine reboot and app-update cycles, authenticated provider
session expiry/reauthentication behavior, Windows runtime behavior and physical
two-device provider playback. Keeping files and restoring session storage cannot
override a provider's own expiry or authorization decisions. First-tab targeting,
host-to-guest content propagation, stronger video readiness and semantic playback
verification remain the later-batch gaps documented in Batch 0.

The next scoped provider batch **can safely begin**: the completed serialized suite,
frontend checks and the two separately authorized real Chrome checks pass. Real
Windows validation remains a release gate. No release readiness claim follows from
this batch's macOS tests.

Impeccable reported legacy PRODUCT.md schema metadata. That unrelated document
was left unchanged; its `init` workflow can refresh it in a separate task.
