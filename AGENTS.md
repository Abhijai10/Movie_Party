# Move Party — AGENTS.md

## 0. PURPOSE

This file defines mandatory operating rules for any AI coding agent or human developer working on Move Party.

This document is subordinate only to:

1. `MASTER_PRD.md`
2. approved Architecture Decision Records in `docs/architecture/adr/`

If this file conflicts with the Master PRD, the Master PRD wins.

If an approved ADR explicitly changes an earlier architectural decision, the ADR wins for that specific decision.

---

# 1. PRIMARY RULE

DO NOT REDESIGN MOVE PARTY.

The architecture has already been chosen.

The developer's job is to IMPLEMENT and VERIFY it.

Do not silently:

- change frameworks;
- replace Tauri;
- replace Rust;
- introduce a cloud backend;
- introduce Firebase/Supabase/AWS;
- replace Tailscale;
- replace QUIC with WebSockets;
- introduce a paid TURN server;
- replace libmpv;
- convert the application into a web app;
- add a browser extension;
- change strict synchronization semantics;
- bypass DRM;
- change local-media transfer into screen sharing;
- make chat use a permanent sidebar;
- increase V1 party size beyond two;
- build music support before video V1 is complete.

If a locked decision cannot work:

STOP.

Create an ADR.

Provide evidence.

Do not invent a replacement architecture while coding.

---

# 2. REQUIRED READING BEFORE CODING

Before modifying the repository, read:

1. `MASTER_PRD.md`
2. `AGENTS.md`
3. `PROTOCOL_SPEC.md`
4. `UI_UX_SPEC.md`
5. `IMPLEMENTATION_TRACKER.md`
6. every accepted ADR relevant to the subsystem being modified.

Do not begin by browsing random source files.

Understand the current project phase first.

---

# 3. PHASE ORDER IS MANDATORY

Implementation must follow the roadmap order in the Master PRD.

The current active phase is defined in:

`IMPLEMENTATION_TRACKER.md`

An agent must not implement features from a future phase unless:

- required as a tiny compile-time stub;
- explicitly approved;
- documented in the tracker.

Example:

If Phase 5 is active, do not start implementing Netflix capture from Phase 20.

---

# 4. ONE PHASE AT A TIME

Each coding session should primarily target one phase.

A phase is complete only when all of the following are true:

- implementation complete;
- unit tests added;
- integration tests added where applicable;
- manual acceptance criteria executed;
- cross-platform status documented;
- known issues documented;
- documentation updated;
- implementation tracker updated.

"Code exists" does not mean "phase complete."

---
## EXTERNAL VERIFICATION EXCEPTION

Some acceptance criteria require hardware, operating systems, accounts,
networks, provider subscriptions, permissions, or a second physical device
that may not be available to the coding agent.

When this occurs:

1. NEVER claim that an unavailable manual test passed.
2. Mark the item in IMPLEMENTATION_TRACKER.md as:
   ⚠ EXTERNAL VERIFICATION PENDING
3. Complete every automated/unit/integration test that can be performed
   locally.
4. The phase must NOT be marked COMPLETE until the external test is
   eventually performed.
5. The agent MAY continue implementing later phases when the missing
   external test does not invalidate or determine their architecture.
6. If the missing verification is an architectural risk gate on which
   downstream implementation actually depends, stop only the dependent
   work and record the blocker.
7. Continue all independent work that can safely proceed.
8. Never bypass, fake, mock-as-passed, or fabricate a required external
   verification result.
   
---

# 5. BEFORE WRITING CODE

For every non-trivial task:

1. identify active phase;
2. identify relevant acceptance criteria;
3. identify files expected to change;
4. identify tests required;
5. identify platform-specific implications;
6. verify no architecture change is required.

If architecture change is required:

DO NOT CODE IT.

Create an ADR proposal.

---

# 6. REPOSITORY BOUNDARIES

The repository is logically separated into:

## Frontend

`src/`

Responsibilities:

- UI rendering;
- interaction;
- presentation state;
- user-visible error states;
- animation;
- accessibility;
- keyboard shortcuts;
- view-level coordination.

Frontend MUST NOT directly implement:

- raw network protocol;
- file transfer;
- DRM/provider instrumentation;
- hashing;
- native video encoding;
- Tailscale detection;
- room authority logic.

Those belong in Rust.

---

## Native core

`src-tauri/src/`

Responsibilities:

- network transport;
- room state;
- protocol;
- file transfer;
- cache;
- synchronization;
- scheduler;
- provider management;
- native capture;
- encoding;
- SQLite;
- native notifications;
- diagnostics.

---

# 7. MODULE BOUNDARIES

Required major Rust modules:

```text
identity/
room/
network/
protocol/
sync/
media/
providers/
capture/
encode/
call/
scheduling/
notifications/
storage/
telemetry/
```

Do not create a single giant:

`core.rs`

or:

`utils.rs`

containing unrelated behavior.

---

# 8. PROVIDER ISOLATION RULE

Provider-specific logic belongs ONLY inside:

```
src-tauri/src/providers/<provider>/
```

The generic sync engine must never contain:

```
if provider == "netflix" { ... }
```

Instead use the provider adapter interface.

Correct:

```
adapter.pause().await?;
```

Incorrect:

```
if netflix {
   query_selector(".netflix-player...");
}
```

outside the Netflix adapter.

---

# 9. NETWORK PROTOCOL RULE

All peer communication must follow:

`PROTOCOL_SPEC.md`

Do not invent new wire messages ad hoc.

If a new protocol message is necessary:

1. add it to `PROTOCOL_SPEC.md`;
2. assign a message ID;
3. define payload schema;
4. define valid room states;
5. define error behavior;
6. define backward-compatibility behavior;
7. implement tests.

---

# 10. NO TRUST IN PEER INPUT

All network data is untrusted.

Every incoming message must validate:

- protocol version;
- room ID;
- sender identity;
- authentication;
- message type;
- payload length;
- field ranges;
- state validity;
- sequence ordering.

Never deserialize and blindly execute peer input.

---

# 11. FILE TRANSFER SAFETY

Never trust:

- filename;
- advertised file size;
- chunk index;
- chunk length;
- hash;
- path.

Guest cache paths must be generated locally.

Never write using a remote-provided absolute path.

Reject path traversal:

```
../
..\
/etc/
C:\
```

---

# 12. PROVIDER CREDENTIAL RULE

Move Party must NEVER:

- ask for Netflix password;
- ask for Prime password;
- ask for JioHotstar password;
- scrape password inputs;
- transmit cookies;
- copy provider cookies to the guest;
- store provider cookies in SQLite;
- log authentication tokens.

Provider authentication remains inside the dedicated Chrome profile.

---

# 13. DRM RULE

Never implement:

- Widevine key extraction;
- CDM hooking;
- license interception;
- decrypted frame extraction from DRM internals;
- HDCP bypass;
- encrypted provider-packet redistribution requiring copied decryption keys;
- DRM circumvention.

Provider Shared Mode may use legitimate OS capture APIs only when they expose usable frames.

If protected video produces black frames:

mark the mode unsupported for that environment.

Do not attempt to defeat the protection.

---

# 14. STRICT SYNC IS CORE PRODUCT BEHAVIOR

The following behavior is mandatory:

If Guest cannot continue playback:

Host must stop.

This includes:

- buffer underrun;
- transfer starvation;
- significant desynchronization;
- player failure;
- peer disconnect.

Do not "optimize" this away.

---

# 15. HOST AUTHORITY

Default authority:

```
Host only
```

Guest controls must not change canonical playback state unless Shared Controls is enabled.

Even with Shared Controls enabled:

Guest sends REQUEST.

Host coordinator produces canonical COMMIT.

Never allow two authoritative playback clocks.

---

# 16. MONOTONIC CLOCKS ONLY

Synchronization code must use monotonic clocks.

Never use:

```
Date.now()
SystemTime::now()
wall clock time
timezone
```

to schedule synchronized playback.

Wall clock may be used for:

- scheduled movie time;
- UI timestamps;
- logs.

Playback synchronization uses monotonic time only.

---

# 17. NO FLOATING-POINT MEDIA IDENTITY

Media identity must not depend on approximate:

```
duration
bitrate
filename
```

Local media identity ultimately depends on full hash + size.

---

# 18. TESTING REQUIREMENT

Every subsystem requires:

## Unit tests

For deterministic algorithms.

## Integration tests

For component boundaries.

## Manual platform tests

For:

- Chrome;
- capture;
- Tailscale;
- libmpv;
- camera;
- microphone;
- native notifications.

---

# 19. NO "WORKS ON MY MACHINE"

For cross-platform phases, record results separately:

```
Windows host → Windows guest
Windows host → macOS guest
macOS host → Windows guest
macOS host → macOS guest
```

A successful Mac-only test is not cross-platform completion.

---

# 20. REQUIRED CODE QUALITY

Rust:

```
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Frontend:

```
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

All must pass before a phase is marked complete.

---

# 21. TYPESCRIPT RULES

Use strict TypeScript.

Required:

```
{
  "strict": true,
  "noUncheckedIndexedAccess": true
}
```

Avoid:

```
any
```

unless interacting with an unavoidable untyped external boundary.

If `any` is used, include a comment explaining why.

---

# 22. RUST ERROR HANDLING

Do not use:

```
unwrap()
expect()
```

in runtime production paths unless a logically impossible invariant is documented.

Use typed errors.

Major subsystems should define explicit error enums.

---

# 23. ERROR CODES

User-visible failures must map to stable Move Party codes.

Pattern:

```
MP-<SUBSYSTEM>-<NUMBER>
```

Examples:

```
MP-NET-001
MP-SYNC-004
MP-PROVIDER-003
```

Raw internal error messages may be attached to diagnostic logs but must not replace the stable code.

---

# 24. LOGGING

Structured logging only.

Every important log should include context such as:

```
room_id
peer_id
phase
message_type
provider
```

Never log secrets.

---

# 25. PERFORMANCE

Do not optimize before measurement.

However, do not implement obviously wasteful media copies.

For large movie chunks:

prefer streaming/buffer reuse over unnecessary:

```
Vec → copy → Vec → copy → Vec
```

Track large allocations where practical.

---

# 26. DATABASE MIGRATIONS

SQLite schema changes must use migrations.

Do not modify production schema ad hoc.

Migration files must be versioned.

Example:

```
001_initial.sql
002_add_network_history.sql
```

---

# 27. UI ARCHITECTURE RULE

Cinema Mode must never permanently shrink the movie to show:

- chat;
- participants;
- settings;
- call.

Use overlays.

See:

`UI_UX_SPEC.md`

---

# 28. UI STATE MUST FOLLOW BACKEND STATE

Do not create fake frontend states that contradict the room coordinator.

Example:

If backend room state is:

```
BUFFERING
```

frontend must not independently label the room:

```
PLAYING
```

The Rust core is authoritative for room state.

---

# 29. NO SILENT FALLBACKS

Bad:

```
Shared Mode failed → secretly switch to Sync Mode
```

Correct:

```
Shared Mode unavailable.

Reason:
Protected video could not be captured.

Available fallback:
Provider Sync Mode

[Use Sync Mode]
```

User must know when media mode changes.

---

# 30. FEATURE FLAGS

Experimental systems must use feature flags/config flags.

Examples:

```
provider_shared_mode
vbrowser_rnd
native_call_transport
```

Do not expose unfinished features by default.

---

# 31. PROTOTYPES VS PRODUCTION CODE

R&D spikes may exist under:

```
experiments/
```

Do not mix disposable spike code into production modules.

After an experiment succeeds:

implement production version cleanly.

---

# 32. ARCHITECTURE DECISION RECORDS

ADR required when changing:

- framework;
- transport;
- media engine;
- sync semantics;
- provider architecture;
- capture architecture;
- cache format;
- security assumptions;
- call transport;
- database.

Use:

`docs/architecture/adr/ADR_TEMPLATE.md`

---

# 33. IMPLEMENTATION TRACKER

At the end of every meaningful coding session:

update:

`IMPLEMENTATION_TRACKER.md`

Required updates:

- current phase;
- completed tasks;
- failing tests;
- open blockers;
- manual tests completed;
- next permitted task.

---

# 34. COMMIT DISCIPLINE

Prefer commits aligned to architectural units.

Good:

```
feat(sync): add host monotonic clock calibration
test(sync): simulate 150ms jitter
```

Bad:

```
changes
stuff
fix
final
```

---

# 35. NO PREMATURE PUBLIC RELEASE WORK

Do not spend V1 engineering time on:

- payments;
- public user accounts;
- marketing website;
- analytics platform;
- auto-updater;
- notarization;
- app-store distribution.

Those belong to release-hardening phase.

---

# 36. DEVELOPMENT PRIORITY ORDER

When trade-offs occur:

1. Correct synchronization
2. Media continuity
3. Data integrity
4. Security/privacy
5. Cross-platform correctness
6. Network efficiency
7. Video-call quality
8. UI polish
9. Animations

Never sacrifice synchronization for prettier UI.

---

# 37. MOVIE-FIRST BANDWIDTH POLICY

When bandwidth is constrained:

Reduce:

1. camera bitrate;
2. camera resolution;
3. camera FPS;
4. camera off.

Before reducing movie quality where possible.

Voice and sync consume little bandwidth and should be preserved.

---

# 38. DO NOT GUESS PROVIDER SUPPORT

Provider support must be empirical.

Never write:

```
Netflix supported
```

unless compatibility testing has passed.

Use statuses:

```
SUPPORTED
EXPERIMENTAL
SYNC_ONLY
UNSUPPORTED
```

---

# 39. DONE DEFINITION

A feature is DONE only when:

- code implemented;
- tests written;
- errors handled;
- UI failure state exists;
- documentation updated;
- tracker updated;
- manual test performed where required.

---

# 40. FINAL AGENT INSTRUCTION

When uncertain whether a requested implementation conflicts with this specification:

STOP.

Explain the conflict.

Do not guess.
