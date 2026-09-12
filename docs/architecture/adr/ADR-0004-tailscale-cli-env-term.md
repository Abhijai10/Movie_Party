# ADR-0004: Tailscale CLI Probe Environment (TERM) and the Friends Verification Layer

**Status:** Accepted  
**Date:** 2026-09-12  
**Author:** Movie Party maintainers  
**Supersedes:** N/A  
**Superseded by:** N/A

---

# 1. CONTEXT

Movie Party gates every screen behind `TailscaleReadiness.state === "READY"`
(MASTER_PRD §11 prerequisite checks). The readiness probe spawns the Tailscale
CLI (`tailscale status --json --peers`), parses the JSON, and derives the
state machine: NOT_INSTALLED → DAEMON_UNAVAILABLE → NEEDS_LOGIN → STOPPED →
NO_USABLE_ADDRESS → READY.

On macOS, the installed Tailscale GUI app (verified against 1.102.4) ships a
CLI at `/Applications/Tailscale.app/Contents/MacOS/Tailscale` plus a
`/usr/local/bin/tailscale` wrapper script. When this CLI is spawned from a
GUI-launched process — Finder, Dock, `open`, or a Tauri app bundled as a
`.app` — it **prints `The Tailscale GUI failed to start: The operation
couldn't be completed. (Tailscale.CLIError error 3.)` to STDOUT with EXIT
CODE 0** and performs no IPC at all. The same command, launched from a
terminal (or with any TERM value present in the environment), speaks the
IPC path and returns the real status JSON.

Root cause (verified by `env -i` bisect, 100% deterministic across many
repetitions): the CLI's AppleEvent/GUI-attach path behaves differently when
the environment carries **no `TERM` variable at all** — the state every
GUI-spawned process inherits. Setting `TERM` to ANY value (even `TERM=dumb`
or a nonsense string) restores the IPC path. This is a Tailscale-side
quirk; Movie Party must defend against it.

Observed production impact: the app reported "Tailscale is installed on
this device, but it isn't responding right now" (DAEMON_UNAVAILABLE /
MP-NET-TS-003) on every GUI launch even while Tailscale was Running and
connected — because the exit-0 garbage stdout failed JSON parsing, and the
app treated that as an unresponsive daemon. The gate blocked the entire
app. Launching the same binary from a terminal worked, which made the bug
look flaky and machine-dependent.

# 2. REQUIREMENT THAT TRIGGERED THIS ADR

The Tailscale path policy (MASTER_PRD §38) requires real, verified network
connectivity claims. DAEMON_UNAVAILABLE must mean "the daemon is genuinely
unreachable," never "the wrapper printed a banner we didn't read."

Additionally, the Add Friend surface (Home) requires a REAL verification of
the connection to a friend's device — not a status-table inference. The
user-facing contract: picking a friend runs an actual probe through the
tunnel and records path + latency; "connected" is claimed only when a
packet genuinely answered.

# 3. OPTIONS CONSIDERED

**A. Remove the readiness gate** so a broken probe never blocks the app.
Rejected: §11 mandates prerequisite checks; the app needs a working tailnet
address for its QUIC transport (UDP 47821 over Tailscale).

**B. Poll `tailscaled`'s UNIX socket directly** (`/var/run/tailscaled.socket`).
Rejected: the macOS GUI variant does not expose that socket at all — the
socket path is a tailscaled-systemd assumption. Ports the platform variance
it was meant to remove.

**C. Set a `TERM` value in the CLI's environment** before spawn, and treat
any stdout that carries the known wrapper banner (`The Tailscale GUI failed
to start` / `failed to connect to tailscaled`) as a probe failure regardless
of exit code — including exit-0 banners, which the old parser never saw
because it only read stdout on success.
Accepted: minimal, platform-safe (a TERM variable is inert on Windows and
Linux), and it fixes both the false DAEMON_UNAVAILABLE and the silent
exit-0 garbage-parse hazard.

**D. For friend verification: trust the tailnet status table's "online"
flag as "connected."**
Rejected: the status table reports the peer's presence in the tailnet, not
reachability of an actual packet exchange through the tunnel. A peer can be
"online" while its path is dead. §38 demands honest claims.

**E. For friend verification: run `tailscale ping` (a genuine WireGuard-layer
probe) on add and on demand.**
Accepted: `pong from <host> (<ip>) via <path> in <n>ms` is the only source
of truth for "the connection is actually established." The parse layer
treats exit 0 + `no reply`/`timed out` (offline peer), exit 1 + `no matching
peer` (unknown peer), and CLI-failure banners as distinct, honest failures
(MP-NET-TS-007 / MP-NET-TS-008 / MP-NET-TS-003).

# 4. DECISION

1. **Every spawn of the Tailscale CLI sets `TERM=dumb`** in the child
   environment (constant `GUI_ENV_TERM` in `src-tauri/src/network/tailscale.rs`).
   On macOS this defeats the GUI-attach quirk; elsewhere the variable is
   inert.
2. **Wrapper-error detection is exit-code independent.**
   `is_wrapper_error_stdout` flags the known banners before JSON parsing;
   a candidate executable that banners is skipped in favor of the next
   candidate rather than poisoning the parse.
3. **Friend verification is a real probe, persisted.** Add Friend saves a
   peer by its stable MagicDNS name (peer_key), then runs
   `tailscale ping --timeout=3s --c=1`. The result (path + latency, or the
   honest failure) is stored in the `friends` SQLite table (schema v4) and
   displayed. A friend shows "Connection verified · direct · 23 ms" only
   when a packet answered; otherwise "Online — not verified yet" or
   "Offline".
4. **Tailscale owns the tunnel; Movie Party verifies it.** The app never
   configures Tailscale itself (no bundling, no login driving) — the friend
   flow's job is selection + verification, matching the product's
   don't-bundle-Tailscale stance.

# 5. CONSEQUENCES

- The false DAEMON_UNAVAILABLE gate on GUI launches is gone: readiness
  reflects the daemon's real state.
- `tailscale ping` adds ~1–3 s to an Add Friend / Connect tap (bounded by
  `--timeout=3s` plus a 6 s outer bound). Acceptable for an explicit
  verification action; the Friends list itself renders from cached state.
- New stable error codes: **MP-NET-TS-007** (no answer through the tunnel)
  and **MP-NET-TS-008** (device not in the tailnet / friend not saved) —
  no conflicts with the existing registry (001–006).
- Schema v4 (`friends` table) upgrades in place via the existing
  `PRAGMA user_version` migration chain.
- Schedules gain a verified target: the guest picker offers saved friends
  by peer_key, so a schedule can address the friend's device across IP
  changes.
- If a future Tailscale version fixes the GUI-spawn quirk, `TERM=dumb`
  remains harmless; the wrapper-banner detection can also be extended with
  new banner strings if the wording changes.

# 6. VERIFICATION

- `src-tauri/tests/tailscale_probe.rs` runs the REAL `detect_status()` with
  TERM stripped from the test process (the GUI-launch simulation) and
  asserts a real status or a non-banner error — it fails if the TERM fix
  ever regresses. On this machine it returns `Some("Running")`.
- `verify_peer_connection_reports_cleanly_without_terminal_env` proves the
  ping probe surfaces a structured failure (never a banner leak, never a
  panic) for an unallocated CGNAT address.
- 40 unit tests in `network::tailscale` cover the banner detection, ping
  parsing (pong/no-reply/timeout/notices), candidate derivation, and probe
  serde shape; storage tests cover the friends table CRUD + the v3→v4
  migration on a real file.
