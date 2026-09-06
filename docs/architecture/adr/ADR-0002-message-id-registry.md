# ADR-0002: Single Authoritative Message-ID Registry With Reserved Ranges

**Status:** Accepted
**Date:** 2026-02-14
**Author:** Batch 11 (protocol truth & ADR foundation)
**Supersedes:** PROTOCOL_SPEC §11 registry (as used by code)
**Superseded by:** N/A

---

# 1. CONTEXT

- Current phase: V1 completion (batches 11+).
- Affected subsystem: `src-tauri/src/protocol/mod.rs` (`MessageType` enum).
- Expected behavior: PROTOCOL_SPEC §11 defines one numeric ID registry with
  reserved ranges (connection 1–6, clock 20–22, room 40–47, media 60–66,
  playback 80–88, shared-controls 100–103, transfer 120–125, network 140–141,
  call 160–163, social 180–181, scheduling 200–204, provider 220–223, error
  250) and §68 forbids redefining a registered ID.
- Observed behavior: the pre-Batch-11 `MessageType` enum had only 15
  variants and — critically — three ID collisions with the spec's
  scheduling range: `ReadyState = 200`, `BufferStatus = 201`,
  `ControlRequest = 202`, colliding with the spec's own scheduling IDs
  (ScheduledPartyCreated etc. at 200–204).
- The QUIC wire never carried these numeric IDs (it uses serde string tags,
  per ADR-0001), so the collision was latent — but any future tooling,
  log correlation, or third-party client reading the registry would see two
  meanings for the same ID, which is exactly what §68 prohibits.

# 2. REQUIREMENT THAT TRIGGERED THIS ADR

```text
§68: A registered message ID must never be redefined.
§11: The registry is the single source of truth for numeric IDs.
Audit finding P1/P11: registry drift + unused constants must be fixed.
```

Relevant documents:

```
PROTOCOL_SPEC.md §11 (registry), §68 (ID stability)
GLM audit P11
```

# 3. EVIDENCE

Pre-fix state of `src-tauri/src/protocol/mod.rs` (recovered via
`git show HEAD:src-tauri/src/protocol/mod.rs`):

```text
MessageType had 15 variants, including:
  ReadyState = 200, BufferStatus = 201, ControlRequest = 202
PROTOCOL_SPEC §11 (lines 233–361) assigns 200–204 to scheduling:
  ScheduledPartyCreated = 200, ScheduledPartyActivated = 201,
  ScheduledPartyCanceled = 202, ScheduledPartyEdited = 203,
  ScheduledPartyResynced = 204
→ Three IDs each had two meanings.
```

# 4. ROOT CAUSE

Confirmed:

- The enum was written as a minimal placeholder for early milestone tests
  and never reconciled with §11 as the protocol grew.
- Because the QUIC wire uses string tags (ADR-0001), nothing failed loudly,
  so the drift went unnoticed by tests.

Unknown:

- Nothing further: the fix is deterministic (align to §11).

# 5. CONSTRAINTS

- §68 forbids redefining registered IDs — scheduling's 200–204 ownership is
  locked by the spec; the connection-scoped variants must move instead.
- The wire format (ADR-0001) does not carry these IDs in V1, so moving them
  is zero-risk on the wire but must be recorded for future encodings.
- No new message types may be invented (AGENTS §9): only spec-registered IDs
  may appear.

# 6. OPTIONS CONSIDERED

## Option A — Keep code IDs, renumber the spec's scheduling range

Renumber §11 scheduling to e.g. 210–214 to free 200–204 for the call
readiness types.

Advantages:

- Code churn minimal.

Disadvantages:

- Violates §68 directly (redefining registered IDs is forbidden — the
  scheduling IDs were published first).
- Spec churn to accommodate code drift inverts the authority direction
  the protocol spec exists to enforce.

Risks: high — normalizes "spec follows code".

Impact: Low effort, wrong direction.

## Option B — Align the enum to §11 exactly and make it the enforced registry (chosen)

Rewrite `MessageType` as the complete 61-variant registry with IDs exactly
as §11 assigns them (ReadyState → 1-range per spec: connection-scoped
ready/buffer/…), add `from_u16` for parsing, a collision-detection unit test
(`registry_has_no_colliding_ids`), a spec-parity test
(`registry_matches_locked_spec_ids`), and round-trip tests over all IDs.

Advantages:

- §68-compliant: the spec's ownership stands; code conforms.
- The registry becomes machine-verified against itself (no duplicate
  discriminants possible — tests fail on any future collision).
- `from_u16` + tests give future wire versions (or tooling) a validated
  mapping.

Disadvantages:

- The pre-existing numeric values of the three colliding variants change —
  acceptable because they were never serialized (ADR-0001) and were never
  spec-legal in the first place.

Risks: minimal — gated by full test suite.

Impact:

```
Low-Medium (one module + tests; done in Batch 11)
```

## Option C — Do nothing / delete the enum

Impact: registry drift persists (audit P11), or we lose the typed registry
and re-introduce the "constants not actually used" problem from the other
side.

# 7. DECISION

```text
src-tauri/src/protocol/mod.rs::MessageType is the single authoritative
in-code registry, mirroring PROTOCOL_SPEC §11 exactly: 61 registered IDs,
zero collisions, exclusive scheduling range. Unknown IDs are rejected by
from_u16 (None → MP-PROTO-005-style handling upstream). Unit tests enforce
spec parity, uniqueness, and full round-trip. The three colliding variants
moved to their spec-assigned IDs; the wire is unaffected (ADR-0001).
Future message types must be added to §11 first (AGENTS §9), then here.
```

# 8. WHY THIS OPTION

- Priority 1–2 (sync/media correctness): no runtime behavior depends on the
  numeric values in V1, so alignment is zero-risk to sync.
- Priority 3 (security): a single validated registry prevents a future
  misrouted or double-meaning ID from becoming an attack/bug vector once
  IDs appear on any wire or tool surface.
- Spec authority (AGENTS §9): code follows the registry, not vice versa.
- Machine enforcement beats convention: the uniqueness test makes recurrence
  of the original defect impossible to merge.

# 9. CONSEQUENCES

## Positive

- Registry is complete, unique, spec-exact, and test-enforced.
- Future encodings/tooling can consume `MessageType::from_u16` safely.
- The "constants unused" audit smell is gone — the enum is now the source
  of truth the envelope metadata module uses.

## Negative

- None functional; the three renumbered variants are a recorded (but
  wire-invisible) breaking change to the enum's numeric values.

## Neutral

- The registry remains informational for the JSON wire (ADR-0001) until a
  decision puts numeric IDs on the wire (which would then require §11 +
  tests, per AGENTS §9).

# 10. AFFECTED COMPONENTS

```
src-tauri/src/protocol/mod.rs   (full registry rewrite + tests)
src-tauri/src/network/quic.rs  (imports; no behavior change)
docs/core_docs/PROTOCOL_SPEC.md (unchanged — code now matches it)
docs/core_docs/IMPLEMENTATION_TRACKER.md
```

# 11. PROTOCOL IMPACT

- No wire change in V1 (IDs not serialized; ADR-0001).
- Registry values now match §11; future numeric-ID encodings inherit the
  spec-legal mapping. §68 preserved.

# 12. DATABASE IMPACT

```
None
```

# 13. SECURITY IMPACT

- Positive: unknown/unregistered IDs cannot be constructed via
  `from_u16` — a malformed or future-numeric-ID input maps to None and is
  rejected upstream rather than misinterpreted.
- No new exposure; registry is in-process.

# 14. CROSS-PLATFORM IMPACT

```
Windows: identical (pure enum, no OS behavior).
macOS:   identical.
Windows ↔ macOS: IDs are protocol constants, identical both ends.
```

# 15. TEST PLAN

```
[x] Unit test — registry_matches_locked_spec_ids (61 IDs, exact values)
[x] Unit test — registry_has_no_colliding_ids (61 unique discriminants)
[x] Unit test — from_u16_round_trips_every_registered_id
[x] Unit test — from_u16_rejects_unknown_types
[x] Unit test — scheduling_range_is_exclusive_and_matches_spec
[x] Unit test — EnvelopeMetadata version/room validation (MP-PROTO-001/002/003)
[x] macOS manual — cargo test green (315 passed)
[ ] Windows manual — ⚠ EXTERNAL VERIFICATION PENDING (no Windows host here)
```

# 16. ROLLBACK PLAN

`git revert` of the registry commit restores the 15-variant enum. Because
nothing on the wire or in persisted state consumes the numeric values in
V1, rollback is compile-time-only.

# 17. DOCUMENTATION UPDATES

```
[x] PROTOCOL_SPEC.md      — no change needed (code now matches §11)
[x] IMPLEMENTATION_TRACKER.md — Batch 11 entry
[ ] MASTER_PRD.md / AGENTS.md / UI_UX_SPEC.md — none required
```
