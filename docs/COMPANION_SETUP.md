# Existing-browser companion — install, connect, remove

The companion is **opt-in**. Managed Chrome remains the default provider path, and
nothing in this document happens unless you choose it.

The companion lets Movie Party use a provider tab you already have open, in the
browser you already use, instead of launching a separate managed browser. It is
built for the case where a provider works better in your normal profile.

## What it can and cannot do

| | |
|---|---|
| **Can** | Recognise the provider tab you choose, report the title and the playback position of that tab |
| **Cannot** | Play, pause or seek for you; move your tabs; read any tab you did not choose; read cookies, passwords, auth headers or history |
| **Default** | Off. Movie Party never starts the listener, installs a host, or loads an extension by itself |

Playback control and navigation are switched off in the protocol itself, with the
reason shown in Settings, because neither has ever been observed working on a real
provider page. Nothing pretends to work.

**The companion has not been verified against a real provider page or a second
device.** If it fails, use managed Chrome — that path is the supported one.

## Requirements

- Google Chrome or Microsoft Edge, current stable, on **Windows 11 x64** or **macOS**.
- Movie Party running, with a provider party in progress or about to start.
- You are already signed in to the provider in that browser. Movie Party never
  handles, stores or automates provider credentials.

## 1. Load the extension

The extension is not in a store. Load it as an unpacked extension:

**Chrome** — open `chrome://extensions` → switch on **Developer mode** (top right) →
**Load unpacked** → choose the `extension/` folder of this repository (or the folder
next to the Movie Party app).

**Edge** — open `edge://extensions` → switch on **Developer mode** → **Load unpacked**.

After loading, the extensions list shows the extension with an **ID** — 32 lowercase
letters. Copy it. That ID is what pins the local bridge to your extension, so it is
deliberately not guessed or wildcarded.

## 2. Turn the companion on

Movie Party → **Settings → Providers → Companion** → **Use my existing browser**.

Until you click that, nothing is listening and the browser cannot reach the app.

## 3. Install the browser host

Paste the extension ID into the field and choose **Install browser host**. This
registers one native messaging host, named `desktop.movieparty.companion`, whose
`allowed_origins` contains exactly the one extension ID you pasted. It writes no
other browser setting and removes nothing.

On macOS the manifest is written for **both** Chrome
(`~/Library/Application Support/Google/Chrome/NativeMessagingHosts`) and Edge
(`~/Library/Application Support/Microsoft Edge/NativeMessagingHosts`), because
supporting one and documenting the other would be a half-truth. **Remove browser
host** deletes both.

**Windows only:** Chrome and Edge look up native hosts in the registry, so after
installing, add the key that points at the manifest Movie Party reports:

```bat
reg add "HKCU\Software\Google\Chrome\NativeMessagingHosts\desktop.movieparty.companion" /ve /t REG_SZ /d "C:\Users\<you>\AppData\Local\Google\Chrome\NativeMessagingHosts\desktop.movieparty.companion.json" /f
```

For Edge, use `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\...` with Edge's
own path. Only your own registry hive (`HKCU`) is touched.

## 4. Pair

Movie Party shows an 8-character pairing code under Settings → Providers →
Companion. Open the extension's popup, paste or type that code, and choose
**Pair with Movie Party**.

The code is short-lived and stops working after five attempts; **New pairing code**
issues another. On success Movie Party returns a session key over the channel the
browser already pinned to your extension ID. Every later message is signed with it,
bound to its sequence number and to the operation it performs, so a captured message
cannot be replayed or pointed at a different action.

## 5. Use it

Open your provider title in a normal browser tab on a supported host
(`youtube.com`, `netflix.com`, `primevideo.com`, `amazon.com`, `hotstar.com`,
`jiocinema.com`), then click the extension → **Use this tab**.

Nothing is read until that click. A tab on any other site is refused before Movie
Party sees its address, and the refusal is shown instead of a silent failure. If the
tab navigates or closes, the session ends and says so.

## 6. Remove it

- **Remove browser host** in Settings deletes the manifest files.
- **Unlink browser** drops the session key, so an extension left loaded keeps no
  access.
- To finish removal, remove the extension in `chrome://extensions` or
  `edge://extensions` (your browser's action, not ours), and on Windows delete the
  `HKCU` key you added.

Uninstalling Movie Party leaves the browser host manifest behind until you remove it
— that is deliberate, because silently editing a browser's configuration from an
uninstaller is not something this app does.

## Manual verification rows

Tiers follow `PROVIDER_VERIFICATION_CHECKLIST.md`. `AUTOMATED` means a test in
this repo drives the real code path; `LIVE` means it was watched through a real
browser; `BLOCKED` names what has to exist before the row can be answered.

| # | Check | Chrome macOS | Chrome Windows | Edge macOS | Edge Windows |
|---|---|---|---|---|---|
| C1 | Load unpacked, host installed, pairing succeeds | BLOCKED — needs the packaged app plus a loaded unpacked extension (never attempted end-to-end) | — | — | — |
| C2 | A provider tab is recognised and reported | AUTOMATED (`a_reported_provider_tab_becomes_a_real_selection`) — not yet driven from a real extension page | — | — | — |
| C3 | A non-provider tab is refused and nothing is sent | PASS, AUTOMATED (`off_allowlist_tabs_and_unsupported_providers_are_refused_nothing_stored`, `content_reports_are_limited_to_supported_provider_hosts`) | — | — | — |
| C4 | Play/pause/seek stay switched off with their reason | PASS, AUTOMATED (`a_capability_the_user_never_granted_stays_switched_off`, `capabilities_that_were_never_observed_stay_switched_off`) | — | — | — |
| C5 | Extension absent → `ExtensionMissing` wording, not a hang | PARTIAL — the code and its guidance exist and are unit-tested; the in-app wording path (`companionState.test.ts`) has not been seen with the extension genuinely missing | — | — | — |
| C6 | Permission prompt declined → `PermissionDenied` recovery | BLOCKED — needs a live browser permission prompt | — | — | — |
| C7 | Tab closed or navigated → `TabChanged`, session ends cleanly | PASS, LIVE (macOS, 2026-10-26, real Chrome: `real_chrome_follows_active_tabs_and_detects_closed_targets`) | — | — | — |
| C8 | Wrong extension version → `VersionMismatch` | PASS, AUTOMATED (`a_version_disagreement_fails_closed`, `version_negotiation_fails_closed`) | — | — | — |
| C9 | Six bad pairing codes → locked out, new code required | PASS, AUTOMATED (`a_guessed_code_is_cut_off_rather_than_offered_forever`) | — | — | — |
| C10 | Movie Party closed → ordinary browsing unaffected | BLOCKED — needs a real managed-Chrome session to close under | — | — | — |
| C11 | Removal leaves no host manifest and no access | PARTIAL — manifest shape and per-browser registration are tested (`the_host_manifest_never_trusts_more_than_one_extension_id`, `the_host_is_registered_for_every_supported_browser_not_just_one`, `unlinking_stops_a_left_behind_extension_from_keep_reporting`); removal from the real profile directories was not inspected | — | — | — |
| C12 | Selected content reaches the guest over the existing sync path | BLOCKED — needs two devices and a running party | — | — | — |

## Run log

| Date | What ran | Result |
|---|---|---|
| 2026-10-26 | `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 cargo test --lib providers::chrome -- --include-ignored` | Real Chrome launched and driven over CDP on a managed profile: launch/evaluate, active-tab following, closed-target detection, profile persistence across close/relaunch, launch-timeout tree reaping. 40/40 after the YouTube player-wait fix. |
| 2026-10-26 | Companion unit + loopback integration suites (`companion::*`, `app_runtime::companion::*`) | All green; the loopback handshake test opens a real TCP listener and refuses off-origin/off-allowlist frames. |
| — | Anything requiring the extension UI in a real browser profile | Not performed. C1, C6, C10, C12 stay blocked. |
