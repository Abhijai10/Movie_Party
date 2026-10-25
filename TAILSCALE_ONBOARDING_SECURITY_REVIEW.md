# Tailscale onboarding — security review (Batch 10)

Date: 2026-10-26. Status: **reviewed against the vendor's published documentation; no control-plane write is implemented, because none can be shown to be narrow enough.**

This is the required output for Batch 10. It records what was reviewed, what
Movie Party now does, what it deliberately does not do, and which steps remain a
human's. Every claim below is either a quote from a Tailscale page linked in this
document or a statement about code in this repository.

> **Headline finding.** There is no documented, narrowly-scoped API for creating a
> machine share. Tailscale's published trust-credential scope list contains scopes
> for devices, users, policy, keys and logs, and **none for sharing**; the sharing
> documentation describes the admin console (Machines page) as the place shares
> are created and revoked. The user-invite endpoints exist, but the published
> scope table names **no scope** for them. Under this batch's own rule — automate
> only when endpoint *and* narrow scope are documented — **the set of automatable
> operations is empty**. The correct implementation is therefore the consent-based
> guided flow, not an "automatic" button.

## 1. What was reviewed

| Topic | Source (fetched 2026-10-26) |
| --- | --- |
| Machine sharing: who may share/accept, expiry, scope of access, revocation | https://tailscale.com/docs/features/sharing |
| Roles and what each may do | https://tailscale.com/docs/reference/user-roles |
| Credential scopes → endpoints, API tokens vs OAuth | https://tailscale.com/docs/reference/trust-credentials |
| API overview and authentication | https://tailscale.com/docs/reference/tailscale-api |
| Existence of user-invite and device-invite endpoints | https://tailscale.com/changelog |
| Inviting a user vs sharing a device | https://tailscale.com/docs/reference/inviting-vs-sharing |
| Auth keys and their standing | https://tailscale.com/docs/features/access-control/auth-keys |

Limitation of this review: the API reference is a JavaScript-rendered page. Its
machine-readable schema was not retrievable from this machine (the spec endpoints
returned 404), so **individual invite endpoint paths were not verified verbatim**
and are not used in code. The matrix below is built from the scope table, the
role table, the sharing page and the changelog — all of which were readable.

## 2. Capability matrix

Implemented as data in [`src-tauri/src/tailscale_access.rs`](src-tauri/src/tailscale_access.rs)
(`CAPABILITY_MATRIX`), with each row carrying the quoted sentence and its URL, so
a stale claim fails a test rather than ageing silently.

| Operation | Verdict | Least role documented | Scope documented | Evidence |
| --- | --- | --- | --- | --- |
| Create device share (share one machine with an external user) | **Console only** | Owner, Admin or IT admin | **none** | "You need to be an Owner, Admin, or IT admin of a tailnet to share a machine." / "You can revoke a share or share invite from the Machines page of the admin console." |
| List device shares | **Console only** | Owner, Admin or IT admin | **none** | Scope table has no sharing scope |
| Revoke device share | **Console only** | Owner, Admin or IT admin | **none** | Machines page, per the sharing docs |
| Create user invite | Documented but unscoped | Owner or Admin | **none named** | User-invite endpoints in the changelog; scope table names no scope for them |
| List user invites | Documented but unscoped | Owner or Admin | **none named** | Same |
| Revoke user invite | Documented but unscoped | Owner or Admin | **none named** | Role table: inviting/removing users is an Owner/Admin action |
| Remove tailnet member | Documented but unscoped | Owner, Admin or IT admin | **none named** | "They can perform any action in the admin console, including inviting or removing users." |
| Create auth key | **Forbidden by Movie Party policy** | Owner/Admin/IT admin | `auth_keys` (exists) | Auth keys authenticate devices and may be reusable; using one would enrol the other person's machine under the host's authority |

Two vendor constraints that shape everything downstream:

- **"A recipient needs to be an Owner, Admin, or IT admin of a tailnet to accept."**
  So even with a perfect API, acceptance cannot be completed by Movie Party — the
  recipient must act, signed in as themselves.
- **"Sharing gives the recipient access to only the shared machine in your tailnet,
  and nothing else."** A share is never tailnet-wide, and **"A machine cannot be
  shared with a tag"** — recipient selection is one user, by construction.

Scopes relevant to the refusals: `devices_invites` (read/write) exists for device
invites; `auth_keys` exists; `users` covers user administration. No scope in the
published table maps to sharing or to user invitations.

## 3. What Movie Party implements

No HTTP client, no API credential, no token storage. The app never asks for a
Tailscale password, OAuth client secret, auth key or admin token, and none is
present in code, config, logs or tests.

1. **A capability matrix the UI renders verbatim** (`tailscale_access_matrix`),
   including the evidence quote and a source link for every row. The response also
   carries `anyAutomatable`, which is `false` — and a test fails if a row is ever
   marked automatable without a named scope.
2. **A share *plan*, not a share** (`tailscale_share_plan`): this device's own
   Tailscale name, exactly one recipient account, a lifetime of 1–30 days (the
   vendor's ceiling is 30), and the user's share role read from the CLI. The plan
   refuses a missing device name, multiple recipients, a tag-shaped recipient, a
   lifetime over 30 days or of zero, and a missing tailnet suffix. It names
   *this* machine and cannot be widened to another device or the tailnet.
3. **Role-aware guidance**: `Owner`/`Admin` → allowed; `LimitedUser` → refused with
   the sentence that says who can; `Unknown` → "check the console", never guessed
   in either direction.
4. **Revocation observation** (`tailscale_revocation_state`): "no longer visible"
   is only reported when this device previously saw the node *as a share*; absence
   without a prior sighting is `UNKNOWN`, and every state ends by pointing at the
   Machines page. Movie Party never claims it revoked anything.
5. **Three separate actions** (`DestructiveAction`): Remove Movie Party friend
   (local, performed by the app), Revoke Tailscale device share (console), Remove
   tailnet member (console). Each carries its own effect sentence, its own "what
   this is not", its own confirmation, and its own operator. A test asserts all
   three differ in label, effect and operator.
6. **Console URL allowlist** (`tailscale_open_console`): the UI passes an action
   *id*; the runtime maps it to one of two pages. It cannot be used to open an
   arbitrary URL, and the local removal has no URL at all.
7. **Failure taxonomy** (`AccessFailure`): insufficient privileges, unknown role,
   plan/policy unsupported, API failure, expired consent, local status
   unavailable — each with a stable `MP-ACCESS-0xx` code and guidance. HTTP status
   mapping (403 → privileges, 404/410 → expired, 422 → plan/policy) is implemented
   and tested ahead of any call existing.

The surfaces: `src-tauri/src/tailscale_access.rs` (model, 13 tests),
`src-tauri/src/app_runtime/tailscale_access.rs` (4 commands + 3 tests),
`src/onboarding/access.ts` + `access.test.ts` (11 tests),
`src/components/mp/TailscaleAccessPanel.tsx` (CinemaButton-only, rendered in the
wizard's approval stage), plus contract tests.

## 4. Explicit permissions

| Permission | Used? |
| --- | --- |
| Tailscale OAuth client secret | **No** — never requested or stored |
| Reusable auth key | **No** — never created or embedded |
| API access token with `all` / broad scopes | **No** |
| Narrow API scope | **None required**, because no call is made |
| Local `tailscale status --json` read | Yes (already used since Batch 9) |
| Opening Tailscale's admin page in the user's browser | Yes, on an explicit click only |

## 5. Human approval steps that remain

1. **Host**: open the Machines page, choose the device Movie Party names, choose
   "Share", enter the one recipient, pick a link expiry (≤ 30 days), send. Because
   the app cannot call an API for this, this is the whole implementation — not a
   fallback.
2. **Recipient**: sign in with their own Tailscale account and accept. They must be
   an Owner, Admin or IT admin of their own tailnet.
3. **Revocation**: the host (Owner/Admin/IT admin) revokes on the Machines page;
   Movie Party then reports whether the shared machine still appears.
4. **Removing a member**: the tailnet admin does it on the Users page. Movie Party
   does not offer this at all beyond the guided link.

## 6. Verified limitations and non-claims

- **No zero-click membership.** Acceptance requires the recipient's own sign-in and
  consent, quoted above. Any product claim to the contrary would be false.
- **No automation of sharing.** No documented endpoint, no scope. Implemented as a
  guided plan + observation.
- **No automation of user invites.** Endpoints exist but no published narrow scope;
  a token broad enough to call them would exceed the action's blast radius.
- **Device invites and auth keys are refused by policy**, even though scopes exist,
  because they enrol a machine under the host's authority instead of the person's.
- **Role unknown is not a refusal.** Movie Party reports "cannot tell" and sends the
  user to the console.
- **Revocation is observed, not confirmed authoritatively.** The local signal is the
  shared node leaving this device's tailnet view; the console remains the authority.
- **Reusable links exist** (up to 1,000 uses on Tailscale's side). Movie Party plans
  one recipient and a bounded lifetime; it cannot stop a user from choosing a
  reusable link in the console, and says so in the plan copy.
- **Not verified on a second machine, Windows, or a non-Owner tailnet.** Those gaps
  are carried in `docs/ONBOARDING_MANUAL_CHECKLIST.md`; the plan's role gate is
  tested against a `LimitedUser` status fixture, not a live limited-user account.

## 7. Requirement map

| # | Requirement | Where it is satisfied |
| --- | --- | --- |
| 1 | Review official docs for invites/shares/scopes/roles | §1 (docs fetched 2026-10-26) |
| 2 | Capability matrix from docs, not assumptions | §2 + `CAPABILITY_MATRIX` with quotes and tests |
| 3 | No embedded credentials/secrets/auth keys/tokens | §4; no credential type exists in the codebase |
| 4 | Never silently enrol a friend's machine with the host's key | Auth keys and device invites marked `FORBIDDEN_BY_POLICY` |
| 5 | Prefer delegated explicit authorization if usable | Delegation to the user's own console session is the only path; no stored secret |
| 6 | If automation is unsafe, keep the guided path first-class | Batch 9 wizard untouched; Batch 10 adds a plan + observation beside it |
| 7 | Add "Create Tailscale Share" only when authorized and narrow | Not added; matrix says console-only, `anyAutomatable: false` |
| 8 | Confirm which local machine is shared | `SharePlan.device_name` from this device's own status; single-machine `ShareScope` |
| 9 | One recipient, limited lifetime by default | `build_share_plan` rejects multi-recipient and lifetimes outside 1–30 days |
| 10 | Confirmation before any admin write | No write exists; every console action is behind a per-action confirm card |
| 11 | No sensitive links/tokens in logs, dumps, clipboard, telemetry | The plan contains no link; the app never receives one; Batch 9's scrubbing remains |
| 12 | Errors for privileges, plan/policy, API failure, expired consent | `AccessFailure` (6 states, status mapping, tests) |
| 13 | Separate the three removal actions | `DestructiveAction` + three cards + tests |
| 14 | Confirm revocation when authorized, else guide to console | `RevocationState` + console URL; never claims to have revoked |
| 15 | Test denial/expiry/replay/duplicate/network/revocation | Rust 13 + runtime 3 + TS 11 + contract tests; see §8 |
| 16 | Recheck whether automation needs a cloud backend, broad admin, permanent creds | It does — §2/§3: no narrow scope, so not enabled by default |
| 17 | Administrator/user documentation | This document + the panel's copy + `docs/ONBOARDING_MANUAL_CHECKLIST.md` |

## 8. Tests

| Layer | What it pins |
| --- | --- |
| `tailscale_access.rs` (13) | every operation has exactly one cited row; nothing automatable without a named scope; sharing rows are console-only; auth keys refused despite a scope; user invites unscoped; role verdicts (incl. Unknown ≠ refusal); plan bounds (device, one recipient, 1–30 days, suffix); revocation only when previously seen; three actions distinct; failure codes/status mapping |
| `app_runtime/tailscale_access.rs` (3) | matrix view never offers automation; only the two admin actions expose URLs; shared-in nodes are remembered so absence means something |
| `src/onboarding/access.test.ts` (11) | matrix copy, verdict labels, three distinct cards with distinct confirmations, plan lines, role blockers, revocation tones |
| `onboardingUiContracts.test.ts` (+4) | the panel has no raw `<button>` and no network call; it says it performs no write; the three confirmable actions exist; every row links its source |

Gates for this batch are recorded in the commit message and re-run with the full
suite; the project's serial Rust harness, tsc, eslint, vitest and build all pass.
