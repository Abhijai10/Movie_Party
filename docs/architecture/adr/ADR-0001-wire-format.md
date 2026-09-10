# ADR-0001: Canonical Wire Format Is JSON With Tagged Envelopes, Not CBOR

**Status:** Accepted
**Date:** 2026-02-14
**Author:** Movie Party maintainers
**Supersedes:** PROTOCOL_SPEC §3 (as written)
**Superseded by:** N/A

---

# 1. CONTEXT

- Current phase: V1 completion (post-M8).
- Affected subsystem: `src-tauri/src/network/quic.rs` framing layer and every
  `ClientRequest` / `ServerResponse` / `ServerEvent` message.
- Expected behavior from MASTER_PRD / PROTOCOL_SPEC: §3 of the protocol spec
  locks "Canonical CBOR (RFC 8949)" as the sole wire encoding, with numeric
  message-type IDs carried in every envelope.
- Observed behavior: the implementation serializes all control traffic as
  JSON with a `u32` big-endian length prefix on each QUIC stream. Message
  discrimination happens via serde's tag-like enum representation
  (`{"type":"HelloAuth","payload":{...}}`), not via spec'd numeric IDs.
- Why the existing spec text cannot stand: the spec-vs-code divergence is a
  violation. Shipping V1 with the spec asserting
  CBOR while every byte on the wire is JSON makes the protocol documentation
  unusable as a source of truth — and §69's definition of done explicitly
  requires protocol docs to match implementation.

# 2. REQUIREMENT THAT TRIGGERED THIS ADR

The locked requirement is protocol documentation integrity:

```text
Protocol docs must match implementation (§69 DoD).
All peer communication must follow PROTOCOL_SPEC.md.
```

Relevant documents:

```
PROTOCOL_SPEC.md §3 (encoding), §5 (size limits), §10 (envelope)
```

# 3. EVIDENCE

- `src-tauri/src/network/quic.rs::write_json` / `read_json_bytes`:
  `serde_json::to_vec` + `u32::to_be_bytes` length prefix (unchanged since
  the QUIC transport landed in M4).
- Every message type in the wire inventory serializes as JSON today;
  no `serde_cbor`/`ciborium` dependency exists in `Cargo.toml`.
- Tests in `src-tauri/src/network/quic.rs` (300+) parse JSON frames; a CBOR
  migration would rewrite every framing assertion.
- Wire capture evidence: not needed — the code path is the only serializer
  the QUIC transport has ever used.

# 4. ROOT CAUSE

Confirmed:

- The transport was implemented with `serde_json` because both ends are our
  code, both are Rust+serde, and QUIC already provides the framing,
  ordering, TLS, and integrity guarantees that CBOR's compactness would
  additionally serve.
- The spec's §3 CBOR lock was written aspirationally, before the transport
  code existed; the two never converged.

Likely:

- JSON was chosen for debuggability of early QUIC streams (wireshark-able,
  human-readable hexdumps) during M4 bring-up.

Unknown:

- Nothing material: both encodings satisfy all functional requirements.

# 5. CONSTRAINTS

- No rented servers, no cloud relay — wire format must work peer-to-peer.
- Two-person V1; wire traffic is small (chat, control, call signaling, tiny
  manifests) except chunk/throughput bytes, which are raw binary and exempt.
- Both ends ship from the same repo on the same cadence: there is no
  independent client to interop with during V1.
- §5's 256 KiB control-message limit must be enforceable at the framing
  layer regardless of encoding.
- Windows + macOS required.

# 6. OPTIONS CONSIDERED

## Option A — Migrate to CBOR + numeric IDs (spec-faithful)

Rewrite `write_json`/`read_json_bytes` to CBOR (RFC 8949), switch enum tags
from string `"type"` discriminants to the spec'd numeric IDs, update ~300
test assertions plus both protocol doc tables.

Advantages:

- Zero divergence from §3 as written.
- ~20–40% smaller control frames; integer IDs are compact.
- Deterministic encoding (RFC 8949 canonical form) aids future hashing of
  messages if ever needed.

Disadvantages:

- ~2–3 days of work touching every wire test with zero user-visible gain.
- Loses human-readable packet dumps during debugging.
- High regression surface in the sync/call/relay paths weeks before V1
  feature completion (the call relay must stay stable).

Risks:

- Subtle map-ordering/canonicalization bugs in a hurry.
- A late CBOR bug could destabilize the cross-device call path.

Estimated implementation impact:

```
High (2–3 focused days + full regression pass)
```

## Option B — Amend the spec: JSON + tagged envelope is canonical (chosen)

Keep JSON as the wire format; amend §3 to document reality, and keep the
numeric ID registry (see ADR-0002) as the authoritative registry even
though the wire carries serde tags. Add the §5 256 KiB limit enforcement
and the §10 envelope version/room fields so the envelope contract becomes
enforced rather than aspirational.

Advantages:

- Zero wire-behavior change: existing transport, tests, and any on-disk
  captured test vectors remain valid.
- Debuggability preserved.
- The numeric ID registry still gets its collision fix (ADR-0002), which is
  the actual interoperability hazard.

Disadvantages:

- Larger frames than CBOR (irrelevant at V1 control sizes).
- The spec must be edited to match code — the direction of that edit is
  documented here so it is an explicit, reviewed decision, not silent
  drift.

Risks:

- If a third-party client ever appears, it must parse JSON, not CBOR. V1 is
  two-person, same-repo; risk accepted.

Estimated implementation impact:

```
Low (docs + the envelope/limit work)
```

## Option C — Do Nothing

Leave §3 asserting CBOR while code speaks JSON.

Impact: the protocol spec is known-false on its most fundamental layer;
every future reader re-discovers the contradiction. Violates §69 DoD.

# 7. DECISION

```text
The canonical Movie Party wire encoding is JSON (UTF-8) with a u32
big-endian length prefix per QUIC stream message. Serde enum tags
("type": "<Name>") discriminate messages on the wire. PROTOCOL_SPEC §3
is amended accordingly. The numeric message-ID registry remains the
authoritative cross-version registry (ADR-0002) but is not carried
on the wire during V1.

The 256 KiB control-message limit (§5) is enforced at the framing layer
(MP-PROTO-004 MESSAGE_TOO_LARGE) and every EventEnvelope carries
v_major / v_minor / room_id (§10), validated on receive.
```

# 8. WHY THIS OPTION

Locked priority order is (1) synchronization correctness, (2) media
continuity, (3) security, (4) cross-platform correctness, then efficiency.

- Option B keeps synchronization and relay behavior byte-identical — zero
  regression risk to the call paths (priority 1, 2).
- The security-relevant protocol properties (size limit, version gate, room
  binding, sequence ordering) are enforced by framing+envelope rules that are
  encoding-agnostic (priority 3).
- Cross-platform: JSON framing is OS-independent serde behavior (priority 4).
- The only thing sacrificed is byte efficiency (priority 7-8), which V1
  control traffic makes irrelevant (chat and call-signal frames are <1 KiB).

# 9. CONSEQUENCES

## Positive

- Protocol spec becomes truthful; §69 DoD satisfiable.
- No destabilizing wire rewrite during feature-completion phase.
- Envelope version + room binding + size limit now enforced (was: absent).
- Numeric registry fix (ADR-0002) lands independently of encoding choice.

## Negative

- Frames are larger than CBOR (bounded by §5's 256 KiB cap).
- Any future third-party client must speak JSON (acceptable: V1 is
  two-person, both ends same repo).

## Neutral

- Wireshark QUIC decryption already needed for inspection; JSON payload
  remains readable once decrypted.

# 10. AFFECTED COMPONENTS

```
src-tauri/src/network/quic.rs          (framing limit + envelope fields)
src-tauri/src/protocol/mod.rs          (registry — ADR-0002)
src-tauri/src/app_runtime.rs           (envelope construction/validation sites)
docs/core_docs/PROTOCOL_SPEC.md        (§3, §5, §10 amended)
```

# 11. PROTOCOL IMPACT

- Encoding description changes (CBOR → JSON) — **documented amendment**,
  no wire-incompatibility because no CBOR bytes were ever exchanged.
- EventEnvelope gains `v_major`, `v_minor`, `room_id` — additive fields.
  Both ends ship together in V1; a V2 receiving a V1 envelope (missing
  fields) rejects at parse time, which is the correct major-version
  behavior per §68 (never redefine message IDs; bump major on breaking).
- Size enforcement: frames > 256 KiB now rejected with MP-PROTO-004
  (previously the silent cap was 2 MiB).

# 12. DATABASE IMPACT

```
None
```

# 13. SECURITY IMPACT

- New enforcement (positive surface): the 256 KiB limit bounds per-frame
  allocations on both ends; oversized length prefixes are rejected before
  any allocation — protects against memory-exhaustion peers.
- Envelope validation rejects wrong-version and wrong-room envelopes before
  they touch runtime state (defense-in-depth on top of QUIC's TLS
  authentication and per-sender sequence tracking).
- No credentials, cookies, or secrets cross the control channel in either
  encoding; join secret hash + device signature remain as-is.

# 14. CROSS-PLATFORM IMPACT

```
Windows: no OS-specific framing behavior; serde_json identical.
macOS:   no OS-specific framing behavior; serde_json identical.
Windows → macOS / macOS → Windows: same bytes; QUIC streams carry the
         length prefix so endianness handling is explicit (u32 BE).
```

# 15. TEST PLAN

```
[x] Unit test     — protocol/mod.rs registry + EnvelopeMetadata tests (61 IDs,
                     no collisions, version/room rejection)
[x] Unit test     — quic.rs §67 malformed-input suite: oversized length prefix
                     (MP-PROTO-004), exact-limit boundary, unknown type tag,
                     missing required fields, negative unsigned values,
                     legacy-envelope parse rejection, serde round-trip
[x] Integration   — quic.rs seq-gate tests over live loopback QUIC
                     (seq=0 rejected, stale seq rejected)
[x] Network deg.  — size-gate test at 256 KiB + 1 rejection path
[x] macOS manual   — full cargo test suite green locally (315 passed)
[ ] Windows manual — ⚠ EXTERNAL VERIFICATION PENDING (no Windows host here)
[ ] Cross-platform — ⚠ EXTERNAL VERIFICATION PENDING (two-device QUIC session)
```

# 16. ROLLBACK PLAN

The wire bytes never changed under this ADR; rollback is documentation-only:
restore §3's CBOR text and delete the §10 envelope-field requirement.
The code changes (256 KiB gate, envelope fields) are independently safe and
independently revertible (`git revert` of the framing commits) if a framing
regression appears.

# 17. DOCUMENTATION UPDATES

```
[x] PROTOCOL_SPEC.md      — §3 amended (JSON canonical), §5 enforcement note,
                             §10 envelope fields documented
[ ] MASTER_PRD.md          — none required
[ ] UI_UX_SPEC.md           — none required
```
