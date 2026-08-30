# DOCUMENT 4 — `docs/architecture/adr/ADR_TEMPLATE.md`

# ADR-XXXX: <Decision Title>

**Status:** Proposed | Accepted | Rejected | Superseded  
**Date:** YYYY-MM-DD  
**Author:** <name/agent>  
**Supersedes:** <ADR or N/A>  
**Superseded by:** <ADR or N/A>

---

# 1. CONTEXT

Describe the existing architecture and the specific problem.

Include:

- current phase;
- affected subsystem;
- expected behavior from Master PRD;
- observed behavior;
- why existing architecture cannot satisfy the requirement.

Do not begin with the proposed solution.

---

# 2. REQUIREMENT THAT TRIGGERED THIS ADR

Quote or reference the exact locked requirement.

Example:

```text
Guest buffering must pause host playback.
````

Relevant documents:

```
MASTER_PRD.md §...
PROTOCOL_SPEC.md §...
```

---

# 3. EVIDENCE

Provide reproducible evidence.

Examples:

```
OS:
Windows 11 25H2

Chrome:
142.x

Provider:
Netflix

Steps:
1.
2.
3.

Expected:
captured movie frames

Observed:
video region is black while Chrome UI is visible
```

Attach:

- logs;
- test output;
- screenshots where appropriate;
- benchmarks;
- reproduction code.

Do not claim "doesn't work" without evidence.

---

# 4. ROOT CAUSE

State what is known.

Distinguish:

```
Confirmed
Likely
Unknown
```

Example:

```
Confirmed:
Windows Graphics Capture returns valid window frames.

Confirmed:
Netflix playback region remains black.

Unknown:
Whether behavior differs under alternate GPU drivers.
```

---

# 5. CONSTRAINTS

List constraints that still cannot change.

Example:

```
No rented server.
No DRM circumvention.
Windows + macOS required.
Strict Sync required.
```

---

# 6. OPTIONS CONSIDERED

## Option A — <name>

Description.

Advantages:

- ...

Disadvantages:

- ...

Risks:

- ...

Estimated implementation impact:

```
Low / Medium / High
```

---

## Option B — <name>

...

---

## Option C — Do Nothing

Describe impact of retaining current behavior.

---

# 7. DECISION

State the selected option clearly.

Example:

```
Provider Shared Mode will be marked UNSUPPORTED
for Netflix on Windows when protected video capture
cannot expose movie frames.

Provider Sync Mode remains the fallback.
```

No ambiguous language.

---

# 8. WHY THIS OPTION

Explain why it best respects the project's locked priorities.

Priority order:

1. synchronization correctness;
2. media continuity;
3. security;
4. cross-platform correctness;
5. zero paid infrastructure;
6. quality;
7. convenience.

---

# 9. CONSEQUENCES

## Positive

- ...

## Negative

- ...

## Neutral

- ...

---

# 10. AFFECTED COMPONENTS

List files/modules likely impacted.

```
src-tauri/src/providers/
src-tauri/src/capture/
src/cinema/
PROTOCOL_SPEC.md
UI_UX_SPEC.md
```

---

# 11. PROTOCOL IMPACT

```
None
```

or list:

- new message ID;
- changed payload;
- protocol version change.

If incompatible:

major protocol bump required.

---

# 12. DATABASE IMPACT

```
None
```

or migration required.

Specify migration.

---

# 13. SECURITY IMPACT

Explain:

- new attack surface;
- credentials;
- network exposure;
- file access;
- provider session access.

---

# 14. CROSS-PLATFORM IMPACT

Explicitly evaluate:

```
Windows
macOS
Windows → macOS
macOS → Windows
```

---

# 15. TEST PLAN

Required tests before ADR can become Accepted.

```
[ ] Unit test
[ ] Integration test
[ ] Windows manual test
[ ] macOS manual test
[ ] Cross-platform test
[ ] Network degradation test
```

---

# 16. ROLLBACK PLAN

If decision causes regression:

describe exactly how to restore previous architecture.

---

# 17. DOCUMENTATION UPDATES

List documents requiring changes:

```
[ ] MASTER_PRD.md
[ ] AGENTS.md
[ ] PROTOCOL_SPEC.md
[ ] UI_UX_SPEC.md
[ ] IMPLEMENTATION_TRACKER.md
```
---