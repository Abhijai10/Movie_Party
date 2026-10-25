# Batch 7 — Opt-in existing-browser companion

Adds an optional Chrome/Edge companion so a user can select provider content from a
tab they already have open and already signed in to, instead of managed Chrome. The
extension is opt-in and **managed Chrome remains the default**: nothing in this batch
starts a listener, installs a host, or loads an extension without an explicit click.

## Architecture

```
provider tab (user's own browser)
   └─ MV3 extension  ── native messaging (Chrome/Edge frames it) ──►  companion host binary
        (activeTab)                                                     (untrusted pipe)
                                                                             │ loopback TCP,
                                                                             │ ephemeral port,
                                                                             │ endpoint file
                                                                        Movie Party app_runtime
```

The host process holds **no secret**. It validates framing, size and message kind,
then relays. Every security decision lives in `authorize_handshake` /
`authorize_command`, which are pure and tested. Movie Party is the only component
that verifies a proof.

- `src-tauri/src/companion/mod.rs` — framing, `Capability`, `CompanionErrorCode`
  with user-facing recovery text, pairing codes, HMAC-SHA256, constant-time compare,
  version negotiation.
- `src-tauri/src/companion/host.rs` — native-messaging host manifest generation,
  request authorisation, endpoint file (loopback port only, version pinned).
- `src-tauri/src/bin/movie-party-companion-host.rs` — the stdio bridge.
- `src-tauri/src/app_runtime/companion.rs` — pairing session, the loopback listener,
  reported tabs feeding the existing chooser, and explicit host install/removal.
- `extension/` — MV3 manifest, service worker, popup, and pure `protocol.js` /
  `security.js` / `state.js` modules that carry the tests.
- `docs/COMPANION_SETUP.md` — install, connect and remove instructions, plus the
  twelve manual Chrome/Edge verification rows.

## Security decisions, and why

| Requirement | How it is met |
|---|---|
| 3 · Never attach CDP to the default profile | There is no debugger path at all. A test asserts no source file mentions `remote-debugging-port`, `DevToolsActivePort` or `devtools://`. |
| 5 · Only the official extension may connect | The host manifest's `allowed_origins` is built from a validated 32-character Chrome ID (`a–p` only). Anything else is refused; no wildcard, no path, no second entry. |
| 6 · Authenticate before content commands | Pairing: an 8-character code, typed by the user, compared in constant time, max five attempts, then the code is destroyed and a new one must be issued. Success returns a 32-byte session key over the channel the browser already pinned to that extension ID. Every later message carries an HMAC-SHA256 over `sequence:kind`, so a captured message cannot be replayed or retargeted at another operation. |
| 7 · Smallest permissions | `activeTab`, `scripting`, `storage`, `nativeMessaging`, plus exact HTTPS provider hosts. A test fails the build if `cookies`, `history`, `tabs`, `webRequest`, `debugger`, `management` or `<all_urls>` ever appear. |
| 9/10 · Never read credentials or unrelated tabs | No static `content_scripts`, so nothing is injected on any page until the user clicks. The observation function reads the address, `og:title` and one visible media element — and refuses when there is not exactly one. A source scan test bans `document.cookie`, `localStorage`, `sessionStorage`, `indexedDB`, `fetch(`, `XMLHttpRequest`, `eval(` and `Function(` in shipped files. |
| 14 · Ordinary browsing stays separate | The state machine ends the session when Movie Party closes, when the chosen tab leaves the provider allowlist, or when it closes. Off-allowlist URLs are refused *before* entering state, so an unrelated address is never stored or sent — asserted by a test that greps the state for the refused host. |
| 16 · No credentials stored or automated | Movie Party never sees a provider login. The session key authenticates the extension to Movie Party only; it is dropped on unlink. |
| 11 · Managed Chrome still works | Untouched. The companion reports a tab into the same chooser the managed path uses, so confirm/invite/start is one flow, not two. |

Hosts the companion may report on delegate to `providers::content::provider_page` —
the same authority the rest of the app already uses — with a test asserting the two
cannot drift apart.

## Capabilities are declared, not assumed

`GRANTED_CAPABILITIES` is exactly `identify_tab`, `report_content`,
`report_playback`. `control_playback` and `request_navigation` are **refused at the
protocol**, each with a reason that reaches the UI, because no provider's protected
player has ever been observed obeying an injected command — the same evidence rule
Batch 6 established. The extension can only narrow this list, never widen it: the
granted set comes from Movie Party's handshake reply and is stored with the key.

## Bugs found while building it

Four of these were real defects that shipped-in-progress code would have had:

1. **The kind gate rejected its own pairing message.** `capability_for("handshake")`
   is `None` by design, so `if capability_for(kind).is_none() → MalformedFrame`
   refused the one message that establishes the session. Found by the app-side
   handshake test, then fixed in **both** the app and the bridge, and pinned with a
   bridge test that asserts a handshake reaches the app rather than being refused at
   the gate.
2. **The host looked in the wrong directory.** The endpoint path hardcoded
   `com.movieparty.desktop`; the app identifier in `tauri.conf.json` is
   `app.movieparty.desktop`. The bridge would never have found the listener. Now
   `APP_IDENTIFIER` is asserted equal to the real config value.
3. **A refusal did not say which capability it denied**, so the extension could only
   guess. Refusals now carry the capability name.
4. **Only Chrome would have been registered.** Installing for one browser while
   documenting both is a half-truth; the host manifest is now written for Chrome
   *and* Edge, with a test requiring both.

Also corrected: three of my own test expectations were wrong rather than the code —
a reported tab must **not** auto-select (`reconcile` never auto-selects, by design),
a spent pairing budget legitimately keeps reporting `PairingFailed` with recovery
words, and a token-reuse test was re-signing a valid token instead of reusing a
foreign one.

## Honest status

The companion is **unverified end to end**. What is proven here is protocol and
policy behaviour, exercised in-process and over real loopback TCP:

- Pairing, replay rejection, kind-binding, sequence monotonicity, off-allowlist
  refusal, spent-attempt lockout, unlink, frame rejection, endpoint framing over a
  real socket, and manifest/permission invariants: **automated**.
- Loading the extension in actual Chrome and Edge, the browser's permission prompts,
  native messaging registration, and reading a real provider tab: **not run**. Those
  are rows C1–C12 in `docs/COMPANION_SETUP.md`, all blank.
- Windows has a second requirement no test can cover: Chrome and Edge resolve native
  hosts through the `HKCU` registry there. The exact command is documented; the
  registry write is deliberately left to the user, and **nothing here was executed on
  Windows at all** (Batch 5's requirement 16 is still open).

Consequences worth naming: this batch adds a local attack surface on a provider core
whose subscription paths have never loaded a real page (Batch 6). That trade was
explicitly chosen; the mitigations are that the default path is unchanged, the
capability set is read-only, and the host stores no secret.
