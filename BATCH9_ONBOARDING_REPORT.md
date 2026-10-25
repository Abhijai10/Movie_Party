# Batch 9 — Unified friend + Tailscale onboarding

Date: 2026-10-26. Branch: `codex/integrated-youtube-player`. Built on Batch 8's
consent-based friendship (`src-tauri/src/friends`, `src-tauri/src/app_runtime/friendship.rs`).

One guided wizard now carries a user from "invite this person" to "both devices
are connected and verified", across the two ways Tailscale can make that
possible. Nothing here installs software, signs anyone in, or creates a share:
Movie Party observes, explains, and hands the user to Tailscale's own pages.

## Requirement by requirement

| # | Requirement | Where it lives | Evidence |
| --- | --- | --- | --- |
| 1 | Build on Batch 8's secure friendship invitation | `app_runtime/onboarding.rs` reads `friendship_state`, `friend_invite_signed`, and binds handshake proofs to the accepted device identity | AUTOMATED |
| 2 | Detect installation, authentication, backend state, tailnet, peer visibility, errors | `network/tailscale.rs`: `CurrentTailnet` (stable id + MagicDNS suffix), `User` record presence, `Self.CapMap` role, `Self.KeyExpiry`, `AuthURL`, `HaveNodeKey`, per-peer `Online`/`CurAddr`/`Relay`/`ShareeNode` | **LIVE on this Mac** + AUTOMATED |
| 3 | Two Tailscale paths: device sharing (preferred) and approved tailnet membership | `onboarding::ConnectionMethod` + `method_facts()` — grants and limits for both, taken from Tailscale's own sharing docs | AUTOMATED (copy), REASONED (behaviour) |
| 4 | One wizard: invite → choose method → Tailscale approval → reachability → identity → ready | `onboarding::WizardStage`, `WIZARD_STAGES`, and `resolved_stage()`; the stage cannot skip an unanswered invitation | AUTOMATED |
| 5 | Explain what each path grants, allow explicit choice | `MethodFacts.grants` / `.limits` (limits are required to be non-empty by test), rendered as two cards with a select action each | AUTOMATED + seen in-browser |
| 6 | Open the official Tailscale admin share/invite page | `onboarding_admin_url()` → `login.tailscale.com/admin/machines` or `/admin/users`, opened by `open_external_url` only on an explicit click | AUTOMATED (URL selection); UNVERIFIED (the page itself) |
| 7 | Securely paste/import a user-created share link | `PastedShareLink::capture` + `inspect_share_link` (https-only, `*.login.tailscale.com`, `/a/<token>`); the view shows only a fingerprint and clears the field | AUTOMATED + seen in-browser |
| 8 | Treat share links as sensitive bearer invites | Same path, plus `scrub_tailscale_secrets()` on every CLI-derived string, a `Debug` impl that redacts, no persistence, and a test that scans the **live SQLite files including the WAL** for the token | AUTOMATED |
| 9 | Invitation package: friend invite + setup instructions, limited-scope/expiring/one-time | `build_invitation_package()` — carries Batch 8's signed invite (7-day expiry, single-use nonce) and the method's steps; refuses to embed the Tailscale link | AUTOMATED |
| 10 | Acceptance is never automatic | `MethodFacts.recipient_must_consent`, `SHARE_LINK_APPLY_NOTE`, `InviteStage`, and the `MP-ONB-008` share-pending state that holds the wizard until the peer appears | AUTOMATED |
| 11 | Detect same-tailnet vs one host shared across tailnets | `tailscale::peer_dns_suffix` / `peer_is_same_tailnet` (+ `ShareeNode` corroboration) → `onboarding::PeerTopology` | AUTOMATED against a synthetic cross-tailnet status; UNVERIFIED on a real second tailnet |
| 12 | Test the device-sharing quarantine limitation | `classify_direction()` implements Tailscale's rule — "A shared machine can receive incoming connections but cannot start connections" — against Movie Party's own dial graph | REASONED (see below) |
| 13 | Verify QUIC, WebRTC signalling and media against that directionality | `TRANSPORT_FLOWS` names each flow, who starts it, and its evidence tier; four are `AUTOMATED_VERIFIED`, WebRTC **media** is `UNVERIFIED_NEEDS_TWO_DEVICES` | Mixed — see below |
| 14 | Offer mutual-sharing / approved-tailnet alternatives when two-way is needed | `INCOMPATIBLE_SHARE_REMEDY` lists all three fixes; the UI renders them under `MP-ONB-012` | AUTOMATED (the text), UNVERIFIED (that they work) |
| 15 | "Connected" only after a real authenticated Movie Party handshake | `HandshakeProof` recorded solely from `QuicHostEvent::PeerAuthenticated` (host) and a verified `AuthAccept` (guest). `Connectedness::TunnelReachableOnly` is a separate, non-connected state; `OnboardingProgress.has_handshake_proof` travels with the claim | AUTOMATED + seen in-browser |
| 16 | Missing Tailscale, expired keys, offline machines, policy denial, wrong tailnet, share pending, revoked invite, duplicate requests | `OnboardingIssue` — 14 states, `MP-ONB-001`…`MP-ONB-014`, each with code, headline, guidance, blocking flag and owning stage | AUTOMATED (reducer); UNVERIFIED (the real CLI's wording for each) |
| 17 | Never install software, change OS permissions or Tailscale identity without consent | No command installs or authorises anything. The affordance set is exactly: open a URL, read status, ping a peer, build a package. `onboarding_check_reachability` is user-initiated | AUTOMATED + reviewed |
| 18 | UI and integration tests for each onboarding state | 40 Rust reducer tests, 14 Rust runtime/integration tests, 32 frontend model tests, 7 UI-contract tests, plus a browser fixture with 12 named states | AUTOMATED |

## The directionality finding (requirements 12–14)

This is the part of Batch 9 that needed reading the code rather than writing it.

Movie Party has exactly one dial direction. `start_host` binds a `QuicServer` to
the device's own Tailscale IPv4, and `join_party` is the only production
`QuicClient::connect`. Everything else rides that single guest-initiated
connection:

- room control streams,
- the host's local movie file (the guest pulls it over the same connection; the
  range server is `127.0.0.1`-only and never crosses the tunnel),
- chat,
- live-call signalling — SDP offers, answers and ICE candidates are relayed as
  `ClientRequest::CallSignal` / `ServerEvent::CallSignal` frames on the same
  connection, and the frontend keeps exactly one `RTCPeerConnection` per device.

Tailscale's sharing quarantine — "a shared machine can receive incoming
connections but cannot start connections" — therefore matches Movie Party **only
when the host's machine is the one being shared**. If the guest shares instead,
the allowed direction is host→guest, which Movie Party never needs and the guest
cannot use to reach the host. `classify_direction()` encodes exactly that, and
`MP-ONB-012` holds the wizard at the reachability stage with the three remedies.

What is **not** settled is the fifth row of `TRANSPORT_FLOWS`. WebRTC media is
direct UDP between the two node addresses, and ICE verifies pairs from both
sides. Whether a share that permits guest→host is enough for the media leg
depends on how Tailscale's stateful handling treats a host-initiated check on
the same flow — that is not observable from one machine and I am not going to
claim it. The wizard therefore states it as "not verified — needs two devices",
and `docs/ONBOARDING_MANUAL_CHECKLIST.md` rows O23–O28 exist to settle it, with
mutual sharing and tailnet membership pre-declared as the fallbacks.

## What the wizard will not say

- A `tailscale ping` reply, or the peer simply appearing in `tailscale status`,
  produces `TunnelReachableOnly` and the label "Tunnel only — not Movie Party".
  Only a handshake proof observed **by this process** flips it to "Connected".
- Proofs are not persisted. After a restart the wizard stops claiming
  Connected until the transport authenticates again — that is deliberate: the
  claim is about a session this app actually verified.
- A tailnet role the CLI didn't report stays `UNKNOWN`. `LimitedUser` is only
  ever asserted when capabilities were reported and none of them was
  admin/owner/tailnet-lock/netflow/debug, because telling an administrator they
  are a limited user would hide the very path they need.
- The owner's tailnet login name (normally an email) is parsed out of
  `CurrentTailnet.Name` and then **discarded** — `TailnetIdentity` has no field
  for it, and a test asserts it cannot reach the IPC payload.

## Files

New: `src-tauri/src/onboarding/mod.rs` (pure model + 40 tests),
`src-tauri/src/app_runtime/onboarding.rs` (session, IPC surface, handshake proofs + 14 tests),
`src/onboarding/wizard.ts` (+ `wizard.test.ts`),
`src/views/OnboardingWizardView.tsx` (+ `onboardingUiContracts.test.ts`),
`src/test-ui/onboarding.fixture.tsx` / `onboarding.html`,
`docs/ONBOARDING_MANUAL_CHECKLIST.md`.

Changed: `network/tailscale.rs` (tailnet identity, role, node-key state, peer
suffix/shared-in, RFC3339 parsing), `storage/sqlite.rs` (friend identity
lookups), `app_runtime.rs` (state field + the two handshake hooks),
`app_runtime/friendship.rs` (shared peer-key normaliser), `lib.rs` (12 commands),
`companion/mod.rs` (a flaky-test fix, below), `backend/appRuntime.ts`,
`components/AppShell.tsx`, `components/appRouting.ts(+test)`,
`views/HomeView.tsx`, `views/FriendsView.tsx`, `styles.css`.

## Defects found while building this

1. **Batch 7's companion pairing test was a coin flip.**
   `pairing_verifies_the_holder_and_rejects_every_near_miss` built a "tampered"
   MAC as `mac[..len-1] + "A"` — which is the *original* string whenever the MAC
   happens to end in `A` (~1 run in 30). The full suite caught it. Replaced with
   a deterministic mutation helper plus a 64-character sweep test.
2. **`inspect_share_link` accepted `/a/` with no token.** Splitting on `/` and
   taking the last non-empty segment turned the literal path component `a` into
   the "token". Now the segment after `/a/` is required.
3. **`rfc3339_utc_to_ms` accepted impossible dates.** `2027-02-30` parsed and
   produced a real timestamp. Days-per-month and the leap rule (including
   century years) are now checked.
4. **Tailnet identity was dropped whenever the CLI reported no capabilities.**
   A device with `AuthURL` pending or no node key lost its whole identity. The
   guard now keeps any real signal.
5. **`onboarding_begin("")` created a session for peer key `"."`** because the
   emptiness check ran after normalisation appended the trailing dot.
6. **Stage actions were being dropped.** The runtime merged the session's own
   facts (chosen method, side, pasted link, probe) only during `refresh`, so
   `choose_method`/`set_side` re-rendered from stale inputs. Merging now happens
   at render time, and a test drives it: switching the side from guest to host
   re-classifies the share as `MP-ONB-012`.
7. **The probe was the one leak-shaped payload.** `verify_friend` returns CLI
   text, and a Tailscale failure line can carry an accept URL. Probes are now
   scrubbed at the moment they enter the session.
8. **The wizard could advance past an unanswered invitation.** Stage 1's gate was
   only checked when no method had been chosen; a user who picked a method first
   skipped the consent stage entirely.

9. **The fixture itself lied about state.** Two cases built their checklist by
   spreading the full row list and then re-adding an id, so React rendered two
   rows for one check — a healthy screen kept showing `MP-ONB-001` under a
   working Tailscale. Caught by driving the fixture in a browser rather than
   trusting the fixture's own shape; the helper now guarantees one row per id and
   a contract test forbids the spread pattern from coming back.

## Gates

All run locally on macOS (Apple Silicon) before committing, toolchain pinned to
the CI's `1.98.0`:

| Gate | Result |
| --- | --- |
| `cargo +1.98.0 fmt --check` | clean |
| `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo +1.98.0 test --all-targets -- --test-threads=1` | **827 passed, 0 failed, 5 ignored** across 20 test binaries (lib: 723) |
| `npx tsc --noEmit` | clean |
| `npx eslint . --max-warnings=0` | clean |
| `npx vitest run` | **460 passed in 41 files** (was 421; +39 from this batch) |
| `pnpm build` | built (`tsc --noEmit && vite build`) |
| Version consistency (the CI gate, run locally) | `0.9.9` × 4 — package.json, tauri.conf.json, src-tauri/Cargo.toml, Cargo.lock |
| Real Tailscale CLI on this Mac | `tests/tailscale_probe.rs` → 3 passed: status, ping shape, and the new tailnet-identity assertion against live output (`TnSLAyoyHo11CNTRL`, suffix `tailc930b7.ts.net`, role `Owner`, key expiry `2027-02-20`) |
| Wizard in a real browser (fixture harness) | All **12 states driven in Chromium**, reading back per state: the `MP-ONB-###` codes actually shown (each state showed exactly its own — 010, 011, 008, 001+002, 004, 012, 013, 014), the checklist row count, `data-state` rail colours and opacity, every control's computed `border-radius` (all `9999px` pills, no competing shape), and the two honesty invariants: the word "Connected" appeared **only** in the handshake-backed state, "Tunnel only" only in the ping-only state, and no state echoed a pasted token |

**Deliberately not run:** the packaged Tauri app (no end-to-end desktop session),
a second device or second tailnet, and Windows. Those are the unverified list
below, not an omission in the table.

## Not verified

- **Two machines.** Every claim about a real share — join, reach, stream, chat,
  call — is reasoned from this build's code plus Tailscale's published rules.
  Rows O17–O29 of the checklist are blank.
- **WebRTC media across a share** (requirement 13's hard part). Open.
- **Windows.** `candidate_executables()` and `open_tailscale_app()` have Windows
  paths and `open_external_url` handles `explorer.exe`, but no Windows run of
  any wizard state has happened.
- **The real CLI's wording per failure state.** The reducer maps text onto codes;
  the text samples come from earlier captures and one live machine, not from
  injecting all eight conditions.
- **The pasted link's tailnet handle comparison** is best-effort by construction
  and reports `None` when the shapes don't line up; I could not observe a real
  invite URL on this device to confirm the host format.
- **The live app route.** The wizard was exercised through the browser fixture
  (12 states, computed styles and text read back from the DOM). The packaged
  Tauri app was not run end-to-end with a real second device.

## Follow-ups

1. Run rows O17–O29 on two machines and fill in the direction/media verdicts —
   that is what turns requirement 13 from "reasoned" into "verified".
2. ~~Give the wizard a way to see the *invite* it already sent for this peer~~
   **Done (2026-10-26):** schema v7 records the recipient on the invite row, so
   `MP-ONB-010` is read from storage. See `BATCH5_9_CLOSURE_REPORT.md` §3.
3. ~~Revisit the legacy `accept_friend_invite` IPC command~~
   **Done (2026-10-26):** `accept_friend_invite`, `add_friend` and
   `friend_invite_link` are removed, and the Friends screen now hands out the
   signed invitation. See §1–2 of the closure report.
4. If O24 shows media cannot cross a one-way share, make the wizard recommend
   tailnet membership for calls specifically, rather than as a general fallback.
