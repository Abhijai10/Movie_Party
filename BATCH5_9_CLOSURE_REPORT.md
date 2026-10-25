# Batches 5–9 — closure pass

Date: 2026-10-26. Branch: `codex/integrated-youtube-player`.

This pass went back over every batch report and checklist in the 5–9 series,
finished the work that was still marked as follow-up, and ran the verification
gates that had never been executed — including the two dependency-audit gates
CI marks advisory. What follows is what changed, what was run, and what is
still blocked and why. Nothing here is promoted past its evidence.

## 1. A consent hole that made the whole friend flow unreachable

Two defects that only make sense together:

1. **Friends handed out the wrong link.** `FriendsView` displayed
   `friendInviteLink()` — the pre-Batch-8 *unsigned* `{name, peer key}` card —
   while Batch 8's acceptance verifier rejects exactly that shape as
   `MP-FRIEND-002`. The signed invitation (`requestSignedFriendInvite`) existed
   and was never called from any screen.
2. **The screen pre-validated with its own parser.** Pasting or deep-linking a
   link ran the frontend's `parseFriendInvite` first, which only understood the
   unsigned shape. A correctly signed invitation was therefore refused by the
   client before the Rust verifier ever saw it.

So the only acceptance path in the UI could not accept the only valid link
format. Fixed by making the Friends screen issue the signed invitation (link,
QR, own fingerprint, expiry, one-time) and by deleting the frontend codec;
`previewFriendInvite` is now the single source of truth for whether a link is
acceptable. `friendErrorCopy` gained the whole `MP-FRIEND-00x` family, so a
legacy link says "from an older Movie Party, ask for a new one" instead of a
generic failure. Three source-contract tests pin all of it.

## 2. Retired the paths that created trust without a decision

Removed from the IPC surface, the runtime, and the TypeScript bridge:

| Removed | Why |
|---|---|
| `accept_friend_invite` | Parsed the unsigned link and saved a friend record with no signature check and no acceptance step. |
| `add_friend` | Created a "friend" from live tailnet visibility alone — reachability treated as trust, which Batch 8 exists to separate. |
| `friend_invite_link` (`friend_invite`) | Built the unsigned link in (1). |

The invariants those tests protected were moved onto the surviving path rather
than dropped: `an_unacceptable_link_saves_nothing_at_all` (malformed and legacy
links change nothing), `the_signed_invite_is_well_formed_and_carries_no_credentials`
(the payload is identity-only; no auth key, token, password or credential),
`friends_list_and_remove_round_trip`, and
`reaccepting_an_invite_never_demotes_a_verified_friend` (converted to the signed
flow, including "the locally chosen name wins over the one in the link").

## 3. "You already asked this friend" is now a stored fact

The invite ledger only recorded the *sender's* identity, so nothing could answer
"have I already invited this person?" — the wizard inferred it from a trust
column. Schema v7 adds `friend_invites.recipient_peer_key` (guarded ADD COLUMN +
index, resumable like v6), `set_invite_recipient`, and
`pending_sent_invite_for_recipient`. Building an invitation package now
attributes the invite to the chosen friend, and `MP-ONB-010` fires only when an
unexpired, unsettled invitation for *that* peer exists. Four new tests, including
one that scans expiry and settlement.

## 4. The dependency audits had never been run

Both are advisory in CI (`continue-on-error: true`), so their findings were
unknown rather than accepted. Running them locally:

**Rust — 10 errors, all in the transport stack.** Five in `quinn-proto` (the
crate that carries every peer byte): a remote peer can panic the stack with
zero-length STREAM frames during unordered reads, the stream-reassembly
memory guard could be bypassed, receive-buffer limits bypassed by zero-length
DATAGRAM frames, unbounded pending-connection-ID growth. For a two-machine app
where the peer is the attack surface, that is not a footnote. Four in `rustls`
(TLS-secret exposure through `Debug`, buffered data sent after ECH rejection),
one in `rustls-webpki`.

Lockfile-only patch updates — quinn-proto 0.11.16→0.11.19, rustls
0.23.45→0.23.46, rustls-webpki 0.103.14→0.103.16 — and `cargo audit` now
reports **0 vulnerabilities** (12 pre-existing allowed warnings unchanged). The
full serial suite was re-run afterwards to prove the patched stack still
handshakes, replays, reconnects and streams exactly as before.

**pnpm — 14 findings, 6 high, all dev/build tooling.** `pnpm audit --prod`
reports **none** in any shipped dependency. In-range updates cleared the rest:
vite 7.3.6→7.3.7, vitest 4.1.11→4.1.12. Seven remain (brace-expansion,
source-map-js), all transitive dev dependencies with no in-range fix; force-pinning
them via `pnpm.overrides` is a dependency-policy decision, so it is recorded here
rather than taken silently.

## 5. Verification that was run this pass

| Run | Result |
|---|---|
| `cargo test --all-targets -- --test-threads=1` (after every change) | **0 failed** throughout; final count below |
| Real libmpv playback (5 binaries: smoke, native surface, software render, EOF, audio) | **7 passed, 0 failed** — real decode, seek, pause, CALayer presentation, audio track, EOF transitions |
| Real Chrome, opt-in (`providers::chrome`, `--include-ignored`) | **40 passed, 0 failed** — CDP launch, tab following, closed-target detection, profile persistence across relaunch, launch-timeout tree reaping, and the live YouTube sync |
| `real_youtube_provider_sync_uses_chrome_cdp` | **PASS** — player detected, real `currentTime`, pause, seek moved the playhead. The first run of the day failed honestly; the player wait was a fixed 8 s sleep, now a 30 s poll (assertion untouched) |
| `tests/tailscale_probe.rs` | **3 passed** — status, ping shape, and live tailnet identity (stable id, suffix, role, key expiry) |
| Wizard fixture in Chromium, 12 states | Each state showed only its own `MP-ONB` codes; "Connected" only with a handshake proof; "Tunnel only" only in the ping-only state; no token echoed; uniform pill controls |
| `version-consistency` (the CI gate, run locally) | 0.9.9 × 4 |

## 6. Still blocked, with the reason

These are not "pending" — they are impossible on this machine, and each one
needs something specific:

| Item | Needs |
|---|---|
| Two-device rows O17–O29 (join, stream, chat, call across a Tailscale share) | A second machine on a second account/tailnet. This is also what would settle whether WebRTC media survives a one-way share — the one requirement-13 flow still labelled `UNVERIFIED_NEEDS_TWO_DEVICES`. |
| Windows rows (P6, C1–C12 on Windows, WebView2 player) | A Windows machine. |
| Netflix / Prime Video / JioHotstar rows S1–S16 | Your signed-in provider accounts. Every S row is marked `BLOCKED`, not blank. |
| Companion C1, C6, C10, C12 | The packaged app plus a real browser profile with the unpacked extension; C1/C6/C10/C12 cannot be driven from a test process. |
| Provider Shared (screen capture) | Product decision: it is disabled by design and the PRD still lists it. Unchanged by this pass. |

Everything else in the checklists is now marked with the tier that actually
supports it — `LIVE`, `AUTOMATED`, `PARTIAL` or `BLOCKED` — and each checklist
has a run log with the date and the command.

## 7. Gates for this pass

| Gate | Result |
|---|---|
| `cargo +1.98.0 fmt --check` | clean |
| `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo +1.98.0 test --all-targets -- --test-threads=1` | **829 passed, 0 failed, 5 ignored** across 20 test binaries |
| `npx tsc --noEmit` / `npx eslint . --max-warnings=0` | clean |
| `npx vitest run` | **453 passed in 41 files** |
| `pnpm build` | built |
| `cargo audit` | 0 vulnerabilities |
| `pnpm audit --prod` | 0 vulnerabilities (dev-only remainder documented above) |
| Version consistency | 0.9.9 × 4 |
