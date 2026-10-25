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
| O1 | Launch with Tailscale installed, signed in, connected | Wizard shows tailnet suffix, your role, and a valid node key | LIVE (single device, macOS) | |
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

## 8. Two-device and other-platform gaps carried forward

- O19–O24, O26–O29, O31, O35 are the batch's core unknowns: they need two
  machines and, for O24, a live WebRTC call across a share.
- Everything in section 7 marked UNVERIFIED needs a real injection; the reducer
  is tested, the CLI's own wording for each condition is not.
- Windows: `candidate_executables()` and `open_tailscale_app()` have Windows
  paths, but no wizard row above has been run there.
