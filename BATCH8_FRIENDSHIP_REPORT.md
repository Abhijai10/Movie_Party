# Batch 8 — Friendship and invitation lifecycle

## What already worked (requirement 1)

Inspected before writing anything. Reused rather than replaced:

- **Local identity** — `identity/mod.rs`: ed25519 `DeviceIdentity`, UUIDv7 device id,
  key material in OS-protected storage (`secure/mod.rs`: Keychain on macOS,
  Credential Manager on Windows), persisted metadata in `device_identity`.
- **Room invitations** — `room/invite.rs` + `quic.rs`: 128-bit room id and 256-bit
  join secret from the CSPRNG, 24-hour TTL enforced host-side, canonical-message
  signature, nonce replay guard, `device_id → first key` pinning, one authenticated
  peer slot.
- **Friends persistence** — `storage/sqlite.rs`: `friends` table, promote-only state
  transitions, the v4/v5 migration discipline.
- **Deep links** — `movieparty://` registration, single-instance argv capture,
  `deep_link_opened` → `FriendsView`. The scheme and path were kept unchanged so
  this needed no OS re-registration.
- **The credential-smuggling guard** — the old link parser already rejected
  non-identity fields; that behaviour is preserved and extended.

## The defect this batch exists to close

A `movieparty://friend/…` link was an **unsigned** string of `{"n":name,"pk":dns}`,
it never expired, it carried no nonce, and `FriendsView` **accepted it on arrival**:
opening the screen at the other end of a pasted or scanned link wrote a friend row
with no decision from the user, and re-pasting an old link committed again.
Friendship was also keyed on a Tailscale MagicDNS name — a rename breaks it — while
`display_name` drove ordering, display and the invite payload.

## What was built

**`src-tauri/src/friends/mod.rs`** — `FriendshipState` with all seven positions, each
parsed strictly (`from_str` on an unknown value is an error; the old code coerced
anything unrecognised to `INVITED`, inventing trust). `FriendInvite` signed over
version, device id, public key, display name, **peer address**, nonce, issue and
expiry with a domain tag, so a friend-invite signature cannot be lifted into room
auth or the reverse. `InviteLedger` for in-session replay/block state.
`decide_trust` keeps _friendship_ and _room admission_ as separate questions.

**Schema v6** (`storage/sqlite.rs`) — friends gain `peer_device_id`, `public_key`,
`trust_state`, `state_changed_at_ms`; a new `friend_invites` table keys on the nonce,
which is what makes single-use a property of storage rather than an in-memory
promise. Every `ADD COLUMN` is individually guarded and the backfill only ever
touches rows without a position, following the F51 interrupted-migration rule.
Legacy rows become `ACCEPTED` with a **null** identity — the honest answer, since
this build never recorded a key for them; they must be re-invited before their key
is trusted.

**`app_runtime/friendship.rs`** — `preview_friend_invite` (verifies and records,
changes no trust), `accept_friend_invitation` (settles the nonce atomically, then
binds identity and sets `ACCEPTED`), `decline`, `block`, `unblock`,
`remove_friend_trust`, `friendship_states`. Accept re-checks outcome, expiry and
block state at decision time, and a link is never a ping: `last_verified_at_ms`
stays null.

**`identity/mod.rs`** — `sign_message` / `verify_message` and `identity_fingerprint`
(`MP-XXXX-XXXX-XXXX-XXXX`, eight bytes of SHA-256 over the public key).
`verify_auth_request_signature` now delegates to the same primitive rather than
duplicating the verify path.

**UI** — an invitation now opens a review card with the device, the fingerprint and
a read-aloud instruction, and nothing is written until **Accept** is pressed.
Trust is rendered beside reachability, because they answer different questions.
Block and remove take a confirmation step that says plainly what they do and do not
do; unblock lands on `REMOVED`, not on friendship, so the peer must invite again.

## Bugs found and fixed on the way

Two of these were pre-existing authorization defects, not new code:

1. **`AuthReplayGuard` called `clear()` at 4096 entries**, returning every spent
   nonce to circulation, and `bind_identity` cleared the key pins the same way.
   Both now evict only entries older than the longest possible invite lifetime and
   otherwise **fail closed**. Regression tests saturate each map and assert a spent
   nonce stays spent and a pinned device cannot be rebound.
2. **`update_friend_verification` used `COALESCE(?2, ip)`**, so a probe that found
   nothing still promoted `INVITED → TAILSCALE_JOINED` off the address a _previous_
   probe left behind. Promotion now requires new evidence; a test proves the cached
   address survives without the state changing.

Three were mine, caught by tests before they could ship:

3. The invite `INSERT` had `VALUES (… , 'PENDING', ?8, ?9)` against ten columns, so
   `expires_at_ms` received the literal `'PENDING'` and the row never landed — every
   acceptance failed. Nine tests caught it as "nothing was recorded".
4. My tests built invitations at a fixed 2023 timestamp while verification uses the
   real clock, so everything read as expired.
5. `decline_friend_invitation` returned `Ok(())` for an invite already decided,
   reporting a success that changed nothing. It now refuses exactly like a second
   acceptance.

Also corrected: two of my own assertions were wrong rather than the code (a
fingerprint has four dashes, not five; `clippy` rejects `chunks_exact` with a
constant because the suggested `as_chunks` is unstable — so that lint cannot be
followed on stable Rust).

## Requirement disposition

| #   | Requirement                                        | State                                                                            |
| --- | -------------------------------------------------- | -------------------------------------------------------------------------------- |
| 1   | Inspect and reuse                                  | Done — §What already worked                                                      |
| 2   | Seven states                                       | Done, strictly parsed                                                            |
| 3   | Represented by signed identity key                 | Done — `public_key` + `peer_device_id`; names are labels                         |
| 4   | Expiring, non-guessable tokens                     | Done — 128-bit CSPRNG nonce, 7-day TTL, signature over every field               |
| 5   | Copy/paste or registered deep link                 | Done — same scheme and path, no OS re-registration                               |
| 6   | Recipient's acceptance required                    | **Fixed** — review card; no path writes trust without it                         |
| 7   | Signature, identity, expiry, replay, revocation    | Done — all five, at preview and again at accept                                  |
| 8   | Out-of-band delivery, on-link acceptance           | Done — no server, no inbox, nothing invented                                     |
| 9   | Existing persistence                               | Done — v6 on the same SQLite file                                                |
| 10  | Fingerprint for first contact                      | Done — shown on the card and each row, with read-aloud copy                      |
| 11  | Remove, block, retry                               | Done — retry is the existing real-ping `verify_friend`                           |
| 12  | Removal blocks app-level trust, no Tailscale claim | Done — `REMOVED` refuses re-acceptance; copy names Tailscale explicitly          |
| 13  | Friend vs invited room guest                       | Done — `decide_trust` plus distinct copy; a one-off seat never minted friendship |
| 14  | Two-person room and host policy preserved          | Untouched — one-peer slot and host-signed auth unchanged                         |
| 15  | Tests                                              | 33 new Rust + 9 new TS; see below                                                |

## Verification

`cargo fmt --check` ✅ · `cargo clippy --all-targets --all-features -D warnings` ✅ zero
warnings · **`cargo test -- --test-threads=1` 757 passed, 0 failed, 5 ignored** across
20 suites · `tsc --noEmit` ✅ · `eslint --max-warnings=0` ✅ · **`vitest` 421 passed /
39 files** · `pnpm build` ✅.

`app_runtime::friendship` covers preview-does-nothing, single-use acceptance,
decline, expiry (including the state settling to `EXPIRED`), block outranking a
later link, removal leaving a position, unknown-peer refusal, self-addressed
refusal, and a legacy unsigned link. `friends::` covers signature coverage of every
field including the address, replay, revocation, blocking by key as well as id,
ledger eviction, and cross-domain signature rejection.

One caveat on how these were run: `cargo test --lib app_runtime::friends` failed
four of eleven tests when run in parallel and passes 11/11 with
`--test-threads=1`. That is the known keychain-contention behaviour this repo
already documents — CI runs serially for exactly that reason — not a defect, but
it means these tests must not be run in parallel.

## Not done, and what it needs

- **`accept_friend_invite` is still registered as an IPC command** and
  `removeFriend`/`acceptFriendInvite` are still exported in `appRuntime.ts`. The UI
  no longer calls them, but the old unconfirmed path is reachable from a renderer,
  which is the exact hole this batch closes. Removing the registration was blocked
  pending your explicit approval — it is a one-line change in
  [src-tauri/src/lib.rs](file:///Volumes/T7%20Shield/PROJECTS/Movie_Party/src-tauri/src/lib.rs)
  plus dropping two dead wrappers. Recommended.
- **Unsigned links from the previous build are now refused** with "send a new
  invite". Correct for trust, but an invitation in flight must be re-sent.
- **Nothing here has been run on two devices.** Every test is in-process or over
  loopback with a temporary database file. The OS keychain round trip, the deep
  link actually arriving from a browser, and the Windows Credential Manager path
  are unverified — Batch 5's requirement 16 (Windows) is still open and now also
  gates this.
- Legacy friends have no recorded identity, so their rows show a trust position but
  no key. They need one fresh signed invite each before an identity claim is made.
- Six commits on `codex/integrated-youtube-player` for this batch, plus this report. **Nothing pushed.**
