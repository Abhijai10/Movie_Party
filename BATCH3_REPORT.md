# Batch 3 — Database write honesty and frontend failure propagation

**Status:** complete, verified. `v0.9.8` (`bb22577`) untouched — no tag created or moved, no release
edited, no force-push. Provider Shared untouched. MP-35…MP-49 untouched. No unrelated P3 cleanup.

---

## 1. Findings verified

Every finding was confirmed against the real code before anything changed.

### Frontend

**MP-02 — a failed leave navigated as if it had succeeded. CONFIRMED**
`src/components/AppShell.tsx`, `confirmEndParty`:

```tsx
const confirmEndParty = () => {
  void leaveParty().then((ended) => {
    applySnapshot(ended);
    void showHome().then(applySnapshot);   // runs unconditionally
  });
};
```

`leaveParty` is `invokeSnapshot("leave_party")`, which **caught the error and returned `null`**.
`applySnapshot` was `if (next) setSnapshot(next)` — so a failure applied nothing and the very next
statement navigated Home anyway. The user landed on the home screen with the room possibly still
live, and nothing anywhere said the leave had failed.

Consistent with the earlier triage (P1→P2): the impact is silent failure plus wrong navigation, and
routing is backend-screen-first, so it is not automatically a ghost party — but it is unambiguously
"navigated on unconfirmed success".

**MP-05 — late create/join results overwrote newer intent. CONFIRMED**
A search of `AppShell.tsx` for `AbortController`, request ids, epochs or cancellation found **none**.
`isCreating`/`isJoining` only drive button state. `createParty` and `joinParty` `await` and then call
`applySnapshot(...)` with no check that the user is still interested — so a create that resolves after
the user has gone Home drags them back to the LOBBY.

**MP-06 — raw `invoke` with callers that do not handle rejection. CONFIRMED**
`appRuntime.ts` had **two** wrappers: `invokeSnapshot` (swallowed → `null`) and
`invokeSnapshotOrThrow` (rejected). Alongside them, `requestPlayCountdown` and `continueWithoutGuest`
called `invoke<AppSnapshot>` **directly with no try/catch at all**, so they rejected — and the caller

```tsx
onRequestCountdown={() => { void requestPlayCountdown().then(applySnapshot); }}
```

had no `.catch`. Pressing Start on Ready Check with a failing command produced an **unhandled promise
rejection** and a button that appeared to do nothing.

The same shape appeared in views: `SettingsView` used the null-return as its *error signal* —
`if (next) { onSnapshot(next) } else { setNameError(...) }` — which is the pattern stated most
plainly: **a failed command and a successful one were indistinguishable.**

### Database

**MP-11 — a failing `PRAGMA user_version` read became 0. CONFIRMED**
`storage/sqlite.rs`, `run_migrations`:

```rust
let current: i32 = conn
    .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
    .unwrap_or(0);

if current > CURRENT_SCHEMA_VERSION {          // SchemaTooNew guard
    return Err(StorageError::SchemaTooNew { .. });
}
```

`0` is *below* every supported version, so a database whose header could not be read would sail
straight past the `SchemaTooNew` guard and have migrations run against a schema this build does not
understand. The tell: the function's own doc comment, four lines above, reads **"Errors are
surfaced, never swallowed"**. Note `schema_version()` already propagated correctly — only the
migration path swallowed.

**MP-13 — identity deletion errors swallowed, and the lookup was not deterministic. CONFIRMED**
`app_runtime.rs`, `rotate_identity`:

```rust
let _ = db.delete_identity();                       // error discarded
self.persist_identity(db, &identity, display_name, &key_label);
```

`upsert_identity` is `INSERT OR REPLACE` keyed on `device_id`, which is `TEXT PRIMARY KEY`. Writing a
**new** device id while the old row survives therefore inserts a **second row** rather than replacing
anything. And `get_identity` was:

```sql
SELECT device_id, ... FROM device_identity LIMIT 1
```

— **no `ORDER BY`**. With two rows the winner is whatever SQLite visits first. So a failed rotation
delete could leave two rows and a **stale identity could win the lookup on the next launch**: exactly
"identity rotation silently resurrects stale identity".

**MP-10 — meaningful DB write errors swallowed. CONFIRMED**
Nine `let _ = db.<write>(…)` sites, of which the meaningful ones are:

| Site | Swallowed write | Consequence if it fails |
|---|---|---|
| `app_runtime.rs:1112` | `delete_identity` | two identity rows (MP-13) |
| `app_runtime.rs:3384/3386` | `update_schedule_status(Accepted/Declined)` | host keeps showing "awaiting confirmation"; the guest's answer disagrees with storage |
| `app_runtime.rs:4634-4636` | `update_schedule_media/preload/start` | guest's Upcoming card shows a title or time the host never set |
| `app_runtime.rs:4641` | `update_schedule_status(Cancelled)` | a cancelled schedule stays live on the guest |
| `app_runtime.rs:1492` | `mark_friend_joined` | a joined friend stays "Invited" (self-heals next poll) |

---

## 2. Fixes implemented

### Centralised the null-as-success trap (the requested inspection)

**It could be centralised safely, and that is what I did.** The decisive fact: every snapshot command
on the Rust side returns a bare `AppSnapshot` — not `Option`, not `Result`. Verified across
`get_app_snapshot`, `show_home`, `show_join_party`, `request_end_party`, `mark_ready`, `back_to_lobby`,
`request_play_countdown`, `continue_without_guest`, `enter_cinema`, `leave_party`,
`set_shared_controls`. A *resolved* invoke therefore always yields an object, which makes a missing
result unambiguous: **`null` could only ever mean "the command failed".**

So:

- **`invokeSnapshot` now returns `Promise<AppSnapshot>` and rejects** with a `BackendCommandError`.
  `invokeSnapshotOrThrow` was deleted — one wrapper, one behaviour, no variant a caller can pick
  wrongly.
- **`applySnapshot` now takes a non-null `AppSnapshot`.** The `if (next)` no-op is gone, so the
  silent half of the bug cannot be reintroduced at the call site either.
- Raw-invoke snapshot commands (`requestPlayCountdown`, `continueWithoutGuest`) route through the
  wrapper, and `detachNativeVideoSurface` no longer catches-and-returns-`null`.

The type tightening made TypeScript and ESLint point at **every** caller that had been relying on the
null return — 19 lint findings, each a place where a failure had been silently absorbed. That
cascade is the clearest evidence the pattern was real and systemic rather than a one-off.

### Shell

- **`runSnapshotCommand(command, message)`** — the single path for fire-and-forget commands: apply
  the confirmed result (clearing a stale notice), or surface the failure.
- **`runConfirmedCommand(command, followUp, message)`** — MP-02: the follow-up runs **only** on a
  confirmed result. `confirmEndParty` uses it, so a failed leave now shows an error and **stays on
  the confirm screen** instead of navigating Home.
- **`createFlowToken()`** (`src/views/commandOutcome.ts`) — MP-05. `createParty`/`joinParty` claim a
  token before starting and re-check it after **every** `await`; navigating anywhere or starting a
  new attempt invalidates it, so an abandoned attempt's result is discarded rather than applied.
- **`commandError` state**, rendered by `LobbyView`, `ReadyCheckView` and `EndPartyConfirmView`
  (new optional `error` prop), so a failed operation says so instead of doing nothing.

Deliberately **not** folded into `applySnapshot`: snapshots *pushed* by the backend are authoritative
and must always apply however late they arrive. Staleness is specific to *command results*, which
represent an intent the user may since have abandoned. That reasoning is recorded on the type.

### Database

- **`read_schema_version`** extracted as its own function and **propagates** the read failure. It is a
  separate function specifically so the contract is directly testable (see the negative control).
- **`get_identity` gained `ORDER BY created_at_ms DESC, device_id ASC`** — deterministic, newest-first,
  with `device_id` as a stable tie-break for two rows written in the same millisecond.
- **`rotate_identity` aborts** when `delete_identity` fails, using the existing `MP-STORE-001`
  vocabulary, instead of proceeding to create a second row.
- **`storage_failure_message(context, error)`** — one helper for the sites that cannot return an error
  to a caller. It **logs the full error and keeps the raw SQLite text out of the user-facing
  message**, satisfying both "propagate real errors" and "avoid exposing raw SQLite internals".
  This mattered concretely: the negative control below shows the old message was
  `MP-STORE-001 failed to persist identity: MP-MEDIA-002 SQLite error: no such table: device_identity`.
- The remaining swallowed writes now record on `state.error` (schedule paths, where the divergence is
  user-visible) or log (the friend refresh, which self-heals on the next poll — logged rather than
  surfaced so a transient failure does not make the UI flap).

`SchemaTooNew` startup behaviour is unchanged and still covered by the existing
`f51_f_newer_schema_version_is_refused_not_downgraded` test, which passes.

---

## 3. Tests added

**Rust (4)** — plus a test-only `execute_sql_for_test` seam so failure-path tests induce a *genuine*
storage failure rather than asserting a happy path.

| Test | Covers |
|---|---|
| `storage::sqlite::schema_version_read_failure_is_propagated_not_defaulted_to_zero` | MP-11 |
| `storage::sqlite::identity_lookup_is_deterministic_and_prefers_the_newest_row` | MP-13 (lookup) |
| `app_runtime::identity_rotation_aborts_when_the_previous_row_cannot_be_cleared` | MP-13 (rotation) |
| `app_runtime::schedule_update_write_failure_is_surfaced_not_swallowed` | MP-10 |

**Frontend (9)** — `src/views/commandOutcome.test.ts`, the pure-logic convention this project already
uses (`resolveAppScreen`, `endPartyConfirmCopy`). There is no jsdom or `@testing-library` here, so
component rendering is not testable; the decision logic is extracted instead.

| Test | Covers |
|---|---|
| a returned snapshot is success | happy path preserved |
| **a null result is a failure, not a no-op** | MP-02 |
| an undefined result is a failure too | MP-02 |
| the command's own error beats the fallback | MP-06 |
| an error is ignored when a snapshot came back | no false failures |
| newest flow claim stays current | MP-05 |
| **a new attempt invalidates the earlier one** | MP-05 |
| **abandoning invalidates every outstanding token** | MP-05 |
| an unrelated token number is not current | MP-05 |

### Negative controls (a test that cannot fail proves nothing)

- **MP-11** — reinstating the old `.unwrap_or(0)` made the test fail with
  `an unreadable schema header must be an error, not version 0 (got Ok(0))`. It discriminates.
- **MP-10 + MP-13** — reverting both swallowing fixes made both tests fail with the intended
  messages: `a failed schedule write must surface MP-STORE-001; got ""`, and
  `the delete guard specifically must have fired; got "MP-STORE-001 failed to persist identity"`.
  The second failure also **demonstrates the raw-SQLite leak** the task asked about, since the old
  message interpolated the engine's own text.

---

## 4. Validation

All Rust commands use the pinned `+1.98.0`; frontend gates match CI.

| Gate | Result |
|---|---|
| `tsc --noEmit` | **clean** |
| `eslint . --max-warnings=0` | **clean** (no output) |
| `vitest run` | **287 passed / 23 files** (was 278/22) |
| `vite build` | **exit 0** |
| `cargo fmt --check` | **clean** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **clean** |
| `cargo test --no-fail-fast -- --test-threads=1` | **535 passed / 2 failed** |

The 2 Rust failures are the pre-existing ones A/B-verified in Batch 2 and unrelated to this batch:

- `real_sw_render_test::bundled_libmpv_sw_render_api_produces_decoded_frames` — **deterministic**
  (3/3), libmpv SW render produces a zeroed buffer in this sandbox.
- `real_native_surface_e2e::bundled_libmpv_renders_onto_real_calayer_through_production_player` —
  **flaky** (2 fail / 1 pass both with and without Batch 2), a libmpv `seek` race.

Both files contain no reference to storage, identity, scheduling or the shell.

### One operational note

The first `vitest` attempt failed to start **any** worker forks ("Timeout waiting for worker to
respond", 23 errors, no tests run) because it was running concurrently with `eslint` and the full
`cargo test`. That is resource contention, not a test failure — re-run alone, all 287 passed. Worth
knowing: do not run the frontend suite alongside the Rust suite on this machine.

---

## 5. Remaining risks

1. **Visible-error coverage is not uniform.** `commandError` is rendered on LOBBY, READY_CHECK and
   END_PARTY_CONFIRM — the screens where a fire-and-forget command is triggered. A failed `showHome`
   (from Home/Settings/Schedule) or a failed failure-event revalidation is **logged but not shown**,
   because those land on screens with no shell error slot and the shell renders each screen from its
   own early return. Adding a global banner would mean touching all 15 return paths; I judged that
   disproportionate, but it is a real gap rather than a solved one.
2. **`CinemaView` chat-send and call-signal failures are logged only.** The control actions
   (play/pause, seek, camera, microphone, shared controls) do surface a notice via the existing
   cinematic notice surface; chat send does not.
3. **No component render tests exist** (no jsdom, no testing-library), so the shell wiring —
   `runConfirmedCommand` actually preventing navigation, `commandError` actually reaching the three
   views — is verified by types, lint and the pure-logic tests, **not** by rendering. The pure
   decisions are tested; the plumbing between them is not.
4. **`MP-11`'s practical blast radius is smaller than it sounds.** A failing `PRAGMA` read on a
   *valid* database is not reachable in practice, so the real risk was guard integrity plus a
   confusing downstream error, not data loss. The fix is still right — the guard should not be
   bypassable — but this is error honesty, not a data-corruption path.
5. **Identity lookup is now deterministic, but the table can still in principle hold two rows.** The
   rotation fix prevents the known way to create that state; the `ORDER BY` makes the outcome defined
   if it ever happens another way. A uniqueness constraint would be stronger but needs a schema
   migration, which I did not add to a frozen release.
6. **`test-only` seam added.** `MoviePartyDb::execute_sql_for_test` is `#[cfg(test)]` and exists so
   failure paths can be tested with real errors. It is compiled out of release builds.

---

## 6. Confirmation of the release freeze

- `HEAD` = `bb225778434e3f07923e03e85d7d7cc1db79146c` = `origin/main` = the commit `v0.9.8` resolves to.
- `v0.9.8` is an annotated tag (object `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3`, tagger date
  2026-09-17 19:29:15 +0530, unchanged). Use `git rev-parse v0.9.8^{commit}` to compare with `HEAD` —
  a bare `git rev-parse v0.9.8` prints the tag object.
- No tag created, moved, deleted or force-pushed; no branch rewritten.
- The GitHub `v0.9.8` Release was not touched.
- **Provider Shared was not implemented or modified.** The one Provider-Shared-adjacent string
  touched is an existing UI error message in `createParty`, unchanged in meaning.
- MP-35…MP-49 were not touched.
- No unrelated P3 findings were fixed.

All Batch 1, 2 and 3 work remains **uncommitted** in the working tree.

### Files changed by Batch 3

| File | Change |
|---|---|
| `src/backend/appRuntime.ts` | one throwing `invokeSnapshot`; `invokeSnapshotOrThrow` removed; non-null snapshot types; raw invokes routed |
| `src/views/commandOutcome.ts` | **new** — outcome classifier + flow token |
| `src/views/commandOutcome.test.ts` | **new** — 9 tests |
| `src/components/AppShell.tsx` | `applySnapshot` non-null; `runSnapshotCommand`/`runConfirmedCommand`; `abandonFlows`; flow tokens in create/join; MP-02 `confirmEndParty` |
| `src/views/CinemaView.tsx` | rejection handling for every control command; `showControlNotice` |
| `src/views/LobbyView.tsx` | `error` prop; chat-send rejection handling |
| `src/views/ReadyCheckView.tsx` | `error` prop |
| `src/views/EndPartyConfirmView.tsx` | `error` prop |
| `src/views/SettingsView.tsx` | rename error moved from the dead `else` to a real `.catch` |
| `src-tauri/src/storage/sqlite.rs` | `read_schema_version`; deterministic identity lookup; `execute_sql_for_test`; 2 tests |
| `src-tauri/src/app_runtime.rs` | rotation aborts on delete failure; `storage_failure_message`; 9 swallowed writes fixed; 2 tests |

### Side effects

- No scratch directories or temporary files were left behind. The Batch 2 `/tmp/wincheck` harness is
  untouched and unrelated to this batch.
