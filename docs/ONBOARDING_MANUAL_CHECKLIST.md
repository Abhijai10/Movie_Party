# Batch 9 — Onboarding verification checklist

Every row is a claim Movie Party makes in the guided friend + Tailscale wizard.
Rows are only checked by a human on real machines; automated coverage is
listed separately in `BATCH9_ONBOARDING_REPORT.md`. Leave a row blank rather
than marking it passed on reasoning.

Evidence tiers used by the code and the UI:

| Tier | Meaning |
| --- | --- |
| **LIVE** | Watched happen on a real device/account pair. |
| **AUTOMATED** | Proven by a test in this repo, on macOS unless the row says otherwise. |
| **REASONED** | Derived from this build's code plus Tailscale's published behaviour. Not observed. |
| **UNVERIFIED** | Nothing supports it yet. The UI must say so. |

## 1. Detection (requirement 2)

| # | Step | Expected | Tier today | Result |
| --- | --- | --- | --- | --- |
| O1 | Launch with Tailscale installed, signed in, connected | Wizard shows tailnet suffix, your role, and a valid node key | LIVE (single device, macOS) | **PASS 2026-10-26** — `tests/tailscale_probe.rs` read the live CLI: stable id `TnSLAyoyHo11CNTRL`, suffix `tailc930b7.ts.net`, role `Owner`, key expiry `2027-02-20`, self DNS name under that suffix |
| O2 | Quit the Tailscale app, reopen the wizard | "Tailscale isn't answering", not "install Tailscale" | AUTOMATED | |
| O3 | Log out of Tailscale, reopen | "Tailscale needs you to sign in" | UNVERIFIED | |
| O4 | Uninstall Tailscale, reopen | "Tailscale isn't installed here" + it never offers to install it | UNVERIFIED | |
| O5 | Use a tailnet where your login is an ordinary member (not admin) | Wizard says you can't create the access, and names who can | UNVERIFIED | |
| O6 | Wait until within 30 days of node-key expiry | Key row reads "expires within 30 days", and is not an error | UNVERIFIED | |
| O7 | Let the node key expire, then reconnect in Tailscale | Expired state clears without restarting Movie Party | UNVERIFIED | |

## 2. The two methods (requirements 3, 5, 6)

| # | Step | Expected | Tier today | Result |
| --- | --- | --- | --- | --- |
| O8 | Read the device-sharing card | Grants and limits both shown; "cannot start connections" present | AUTOMATED | |
| O9 | Read the tailnet-membership card | Grants and limits both shown; ACL exposure stated | AUTOMATED | |
| O10 | Press "Open Tailscale's page" on each method | The correct admin page opens in your browser; nothing else happens | UNVERIFIED | |
| O11 | Confirm no button installs, signs in, or shares for you | None do | AUTOMATED | |

## 3. Share links as bearer credentials (requirements 7, 8)

| # | Step | Expected | Tier today | Result |
| --- | --- | --- | --- | --- |
| O12 | Paste a real Tailscale invite link | Wizard shows only a fingerprint; the input clears | UNVERIFIED | |
| O13 | Search Movie Party's database files (WAL included) for that token | Zero hits — proven by a test, but confirm on your machine too | AUTOMATED | |
| O14 | Restart Movie Party after pasting | The link is gone; nothing was persisted | UNVERIFIED | |
| O15 | Paste a non-Tailscale or http:// link | Rejected with a stable `MP-ONB-020/021` code and no echo of the link | AUTOMATED | |
| O16 | Press "Open the link in my browser" | Your browser opens Tailscale's own accept page | UNVERIFIED | |

## 4. Topology and direction (requirements 11, 12, 13, 14)

Two accounts and two machines are required. Do these in order and record which
account shared what.

| # | Setup | Expected | Tier today | Result |
| --- | --- | --- | --- | --- |
| O17 | Both devices are members of one tailnet | Topology: "same tailnet"; direction: "either direction works" | REASONED | |
| O18 | **Host** shares the host machine with the **guest** account | Topology: shared across tailnets; direction: "direction matches" | REASONED | |
| O19 | Guest dials the shared host, room opens | Join succeeds; wizard shows Connected | UNVERIFIED | |
| O20 | Host attempts anything that needs to start a connection to the guest | Nothing in Movie Party needs this; confirm no MP-NET-TS-005 appears | UNVERIFIED | |
| O21 | In-party chat both ways over O18 | Works (rides the guest-initiated connection) | UNVERIFIED | |
| O22 | Local movie streaming over O18 | Guest plays the host's file with no second connection | UNVERIFIED | |
| O23 | Live call over O18 — signalling | Offer/answer and ICE reach both sides | UNVERIFIED | |
| O24 | Live call over O18 — media | **This is the open question.** WebRTC media is direct UDP both ways; Tailscale's quarantine may block the host→guest leg. Record whether audio/video flows, and which path (`direct`/`relay`) the ICE pair used | UNVERIFIED | |
| O25 | **Guest** shares their machine with the **host** account instead | Wizard says the sharing points the wrong way and gives all three fixes | REASONED | |
| O26 | Follow fix 1 from O25 (share the other way) | Wizard moves to "direction matches" | UNVERIFIED | |
| O27 | Follow fix 2 from O25 (mutual sharing) | Live calls work if O24 failed | UNVERIFIED | |
| O28 | Follow fix 3 from O25 (join one tailnet) | Live calls work; the direction warning disappears | UNVERIFIED | |
| O29 | Revoke the share in the Tailscale console | Peer disappears from tailnet status; wizard reports the stale link path | UNVERIFIED | |

## 5. Consent, not automation (requirements 9, 10, 17)

| # | Step | Expected | Tier today | Result |
| --- | --- | --- | --- | --- |
| O30 | Create a share/invite, then do nothing on the second device | Wizard stays at "waiting for them to accept" | REASONED | |
| O31 | Accept as the second user with **their own** Tailscale account | Peer becomes visible; no step was skipped for them | UNVERIFIED | |
| O32 | Try to accept using the host's credentials | Movie Party never asks for or offers them | REASONED | |
| O33 | Copy the invitation package | Contains the signed Movie Party invite + steps, and **no** Tailscale link | AUTOMATED | |

## 6. "Connected" means a handshake (requirement 15)

| # | Step | Expected | Tier today | Result |
| --- | --- | --- | --- | --- |
| O34 | `tailscale ping` reaches the peer, no party has ever run | Wizard: "Tunnel only", stage stays at identity verification | AUTOMATED | |
| O35 | Host a party; the guest joins and authenticates | Wizard flips to Connected, names the mechanism | UNVERIFIED | |
| O36 | Restart Movie Party after O35, before any new party | Connected clears — proofs are per run on purpose | AUTOMATED | |
| O37 | Block the friend, then re-check | No Connected claim for a blocked identity | AUTOMATED | |

## 7. Failure states (requirement 16)

| # | Inject | Expected code | Tier today | Result |
| --- | --- | --- | --- | --- |
| O38 | Tailscale not installed | MP-ONB-001 | UNVERIFIED | |
| O39 | Daemon not answering | MP-ONB-002 | UNVERIFIED | |
| O40 | Needs login | MP-ONB-003 | UNVERIFIED | |
| O41 | Expired node key | MP-ONB-004 | UNVERIFIED | |
| O42 | Peer machine asleep | MP-ONB-005 | UNVERIFIED | |
| O43 | ACL denies the pair | MP-ONB-006 | UNVERIFIED | |
| O44 | Invite link points at another tailnet | MP-ONB-007 | UNVERIFIED | |
| O45 | Share created, not accepted | MP-ONB-008 | REASONED | |
| O46 | Share revoked/expired | MP-ONB-009 | UNVERIFIED | |
| O47 | Ask the same friend twice | MP-ONB-010 (notice, not a wall) | AUTOMATED | |
| O48 | Ordinary member tries to share | MP-ONB-011 | UNVERIFIED | |
| O49 | Wrong share direction | MP-ONB-012 | REASONED | |
| O50 | Tunnel fine, no Movie Party handshake | MP-ONB-013 | AUTOMATED | |
| O51 | Blocked identity | MP-ONB-014 | AUTOMATED | |

## Run log

| Date | What ran | Result |
|---|---|---|
| 2026-10-26 | `cargo test --test tailscale_probe` | PASS — status, ping shape, and the real tailnet-identity assertions (O1). |
| 2026-10-26 | `onboarding` reducer + runtime suites (54 tests) | PASS — all six stages, all fourteen `MP-ONB` states, the connectedness gate, and the bearer-link scans. |
| 2026-10-26 | Wizard fixture in a real browser, 12 states | PASS — each state showed only its own codes; "Connected" appeared only with a handshake proof; "Tunnel only" only in the ping-only state; no token echoed; all controls the same pill radius. |
| 2026-10-26 | Invitation hand-out | FIXED — the Friends screen now issues the *signed* invitation. Before this, it handed out the pre-Batch-8 unsigned identity card and pre-validated pasted links with a local parser, so the only acceptance path in the UI refused the only valid link format. |
| — | Rows O17–O29 (two devices) | Not run — one machine, one tailnet. |
| — | O19–O24 media/call across a share | Not run — same. These are what turn requirement 13 from "reasoned" into "verified". |

## 7b. Tailscale control-plane access (Batch 10)

See `TAILSCALE_ONBOARDING_SECURITY_REVIEW.md` for the evidence behind these rows.
No row here can be automated: the matrix marks every operation console-only or
policy-refused.

| # | Step | Expected | Tier today | Result |
| --- | --- | --- | --- | --- |
| A1 | Open the access panel as an Owner/Admin | Share plan can be prepared; role reads "allowed" | LIVE (single device, macOS — role read) | |
| A2 | Same panel on a non-admin account | Refused with the sentence naming who can share | UNVERIFIED | |
| A3 | Tailscale reports no role | Says "cannot tell", does not refuse | AUTOMATED | |
| A4 | Plan a share of "this machine" | Shows this device's tailnet name, one recipient, 1–30 days | AUTOMATED | |
| A5 | Try two recipients, a tag, or 31 days | Refused with a stable `MP-ACCESS` code | AUTOMATED | |
| A6 | Open the Machines page from the panel | Tailscale's own page; no share created by Movie Party | UNVERIFIED | |
| A7 | Revoke a share in the console, then "Check again" | "No longer visible" only if the node was seen as a share first | AUTOMATED (logic) / UNVERIFIED (live) | |
| A8 | Check revocation for a peer never seen as a share | `UNKNOWN`, pointing at the console | AUTOMATED | |
| A9 | Use the three removal cards | Three distinct effects, three distinct confirmations; only the friend removal is local | AUTOMATED | |
| A10 | Confirm with a real second person | They must sign in and accept, and be an admin of their own tailnet | UNVERIFIED | |

## 8. Two-device and other-platform gaps carried forward

- O19–O24, O26–O29, O31, O35 are the batch's core unknowns: they need two
  machines and, for O24, a live WebRTC call across a share.
- Everything in section 7 marked UNVERIFIED needs a real injection; the reducer
  is tested, the CLI's own wording for each condition is not.
- Windows: `candidate_executables()` and `open_tailscale_app()` have Windows
  paths, but no wizard row above has been run there.
