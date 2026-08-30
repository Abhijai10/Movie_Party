# DOCUMENT 3 — `UI_UX_SPEC.md`


# Movie Party UI / UX Specification
## Version 1

---

# 1. DESIGN OBJECTIVE

Movie Party must feel like:

```text
a movie
with another person present
````

not:

```
a communication application
that happens to contain a movie
```

The movie always has visual priority.

---

# 2. DESIGN CHARACTER

V1 visual direction is locked as:

```
Cinematic
Minimal
Dark
Low-distraction
Soft-translucent overlays
High contrast where needed
Minimal permanent chrome
```

Avoid:

- bright dashboard panels;
- permanent navigation bars in Cinema Mode;
- Discord-like sidebars;
- dense controls;
- huge branding during playback.

---

# 3. COLOR SYSTEM

V1 neutral palette:

```
Background:
#090A0C

Surface:
rgba(18, 20, 24, 0.88)

Elevated Surface:
rgba(28, 31, 36, 0.92)

Primary Text:
#F5F7FA

Secondary Text:
#AAB0BB

Muted:
#707784

Success:
#55C77A

Warning:
#E8B44C

Error:
#EB6666

Overlay Backdrop:
rgba(0, 0, 0, 0.58)
```

Accent color may later become configurable.

Do not introduce multiple unrelated accent colors.

---

# 4. TYPOGRAPHY

Use system-native UI fonts.

Windows:

```
Segoe UI
```

macOS:

```
SF Pro / system-ui
```

Fallback:

```
font-family:
  system-ui,
  -apple-system,
  BlinkMacSystemFont,
  "Segoe UI",
  sans-serif;
```

Movie UI should use no decorative font.

---

# 5. SPACING GRID

Base unit:

```
4 px
```

Common spacing:

```
4
8
12
16
24
32
48
64
```

Avoid arbitrary spacing unless justified.

---

# 6. CORNER RADII

```
Small controls: 8px
Cards: 12px
Large dialogs: 16px
Camera card: 16px
Pills: 999px
```

---

# 7. ANIMATION

Animation should be subtle.

Default UI transition:

```
150–220 ms
```

Chat fade:

```
250 ms enter
400 ms exit
```

Controls auto-hide after:

```
3 seconds
```

No playful bouncing UI during serious playback states.

---

# 8. APPLICATION WINDOW MODES

Movie Party has:

```
STANDARD
CINEMA
FULLSCREEN_CINEMA
GHOST
PRIVACY
```

---

# 9. GLOBAL APPLICATION STATES

```
BOOTING
FIRST_RUN
HOME
CREATE_PARTY
JOIN_PARTY
SCHEDULE
LOBBY
PREPARING
READY_CHECK
CINEMA
BUFFERING
RECONNECTING
PARTY_ENDED
ERROR
```

Frontend must render according to backend state.

---

# 10. FIRST RUN

First launch screen:

```
Welcome to Movie Party

Watch together.
Stay synchronized.

[ Get Started ]
```

Then prerequisite checks.

---

# 11. PREREQUISITE CHECK

Check:

```
Tailscale installed
Tailscale connected
libmpv available/bundled
Chrome installed
camera permission
microphone permission
screen capture permission when required
notification permission
```

UI:

```
Setup

Tailscale             ✓
Google Chrome         ✓
Camera                Not requested
Microphone            Not requested
Notifications         Enable
Screen Recording      Only needed for Shared Mode

[ Continue ]
```

Do not request camera/screen permissions before necessary unless onboarding explicitly explains why.

---

# 12. HOME SCREEN

Desktop target minimum:

```
1024 × 700
```

Layout:

```
┌──────────────────────────────────────────┐
│ Movie Party                               │
│                                          │
│ What are we watching?                    │
│                                          │
│ ┌──────────────────────────────────────┐ │
│ │ Paste Netflix / Prime / Hotstar /   │ │
│ │ YouTube URL...                      │ │
│ └──────────────────────────────────────┘ │
│                                          │
│              [ Continue ]                │
│                                          │
│                    or                    │
│                                          │
│       [ Choose Downloaded Movie ]        │
│                                          │
│       [ Join Existing Party ]            │
│                                          │
│ Upcoming                                 │
│ Interstellar · Tonight 10:00 PM          │
└──────────────────────────────────────────┘
```

---

# 13. URL INPUT BEHAVIOR

On paste:

detect provider immediately.

Example:

```
Netflix detected

Interstellar
netflix.com

Preferred mode:
Shared Mode

Fallback:
Sync Mode
```

If unknown URL:

```
Unsupported or unknown video source.
```

Never pretend unknown URL is supported.

---

# 14. LOCAL FILE SELECTION

After file chosen:

show:

```
Interstellar.mkv

1080p
H.264
4.2 GB
2h 49m

Audio
English 5.1

Subtitles
English

[ Continue ]
```

Metadata should load asynchronously.

---

# 15. CREATE PARTY

Desktop card width:

```
560–680px
```

Fields:

```
Media
Guest
Call mode
Control mode
Start time
```

Call mode segmented control:

```
[ Video + Voice ] [ Voice Only ] [ Off ]
```

Default:

```
Video + Voice
```

Mic still starts muted.

Controls:

```
Host Only
Shared Controls
```

Default:

```
Host Only
```

Start:

```
Now
Schedule
```

---

# 16. INVITATION

V1 methods:

```
Copy invite code
Copy movieparty:// link
Show QR code
```

QR is useful for transferring invite between devices but mobile client is not required.

---

# 17. JOIN SCREEN

```
Join Movie Party

Invite code

[________________________]

[ Join ]
```

When invite parsed:

```
Abhijai invited you

Interstellar

Local Movie
Video call enabled
Strict Sync enabled

[ Join Party ]
```

---

# 18. SCHEDULE SCREEN

Fields:

```
Date
Time
Media
Guest
Call mode
```

For local media show:

```
Estimated transfer: 58 min

Recommended preload start:
7:42 PM

Scheduled movie:
10:00 PM
```

Allow user to move preload earlier.

Do not allow moving it later than estimated safe point without warning.

---

# 19. SCHEDULE OFFLINE WARNING

If guest currently offline:

```
Rahul is offline.

The schedule will still be saved.
Movie Party will begin transfer when both devices are online.

Rahul's local reminder will appear if their device is running.
```

---

# 20. LOBBY

Primary layout:

```
┌────────────────────────────────────────────┐
│ Interstellar                               │
│ Local Perfect Mode                         │
│                                            │
│ Participants                               │
│                                            │
│ Abhijai                                    │
│ Connected                     ✓            │
│ Media                         ✓            │
│ Camera                        ✓            │
│ Mic                           Muted         │
│                                            │
│ Rahul                                      │
│ Connected                     ✓            │
│ Media                         Preparing    │
│ Camera                        ✓            │
│ Mic                           Muted         │
│                                            │
│ Network                                    │
│ Direct · 8.4 Mbps · Good                   │
│                                            │
│ Guest Buffer                               │
│ ████████████░░░  2m 41s                   │
│                                            │
│             [ Start When Ready ]           │
└────────────────────────────────────────────┘
```

---

# 21. NETWORK STATUS LANGUAGE

Do not overwhelm normal users.

Use:

```
Excellent
Good
Unstable
Insufficient
```

Click to expand advanced values.

---

# 22. ADVANCED NETWORK DETAIL

Expandable:

```
Tailscale Path       Direct
RTT                  18 ms
Measured Goodput     8.4 Mbps
Movie Requirement    4.2 Mbps
Camera Budget        0.4 Mbps
Recommended Buffer   120 sec
```

---

# 23. PRELOAD UI

```
Preparing Interstellar

Rahul
████████████░░░░░░░░  62%

2.6 GB / 4.2 GB

Safe buffer:
18m 42s

You can start now,
but waiting longer will improve reliability.

[ Start Now ]
[ Keep Preloading ]
```

---

# 24. DOWNLOAD-FIRST UI

```
For the smoothest experience,
Movie Party recommends finishing the transfer first.

Estimated remaining:
31 minutes

[ Download First ]
[ Use Smart Preload Instead ]
```

---

# 25. READY CHECK

Full-window or centered overlay:

```
Almost ready

Abhijai    READY ✓
Rahul      READY ✓
Media      READY ✓
Network    GOOD ✓
Sync       LOCKED ✓

Starting in

3
```

Countdown timing must come from sync engine.

Frontend animation must not create its own unsynchronized countdown.

---

# 26. CINEMA MODE LAYOUT

Movie fills entire available region.

```
┌──────────────────────────────────────────────┐
│                                              │
│                                              │
│                  MOVIE                       │
│                                              │
│                                  ┌────────┐  │
│                                  │ Rahul  │  │
│                                  │ Camera │  │
│                                  └────────┘  │
│                                              │
│          Rahul: BRO WHAT 😭                  │
│                                              │
│               controls                       │
└──────────────────────────────────────────────┘
```

---

# 27. MOVIE SCALING

Default:

```
contain
```

Never crop cinematic content automatically.

Optional future "Fill" may crop.

Not part of V1 unless trivial.

---

# 28. CAMERA CARD

Default width:

```
220px
```

Default 16:9.

Minimum:

```
120px
```

Maximum:

```
360px
```

Default position:

```
top-right
24px margin
```

Draggable.

Persist location locally.

---

# 29. CAMERA MINIMIZED

Minimized camera becomes:

```
◯ Rahul
```

or avatar circle approximately:

```
48px
```

Click restores.

---

# 30. CAMERA OFF

If remote user disables camera:

show small status only when controls visible:

```
Rahul · Camera off
```

Do not permanently display empty black video rectangle.

---

# 31. CONTROL DOCK

Bottom-center floating control dock.

Only visible on:

- pointer movement;
- keyboard interaction;
- important state change.

Contains:

```
-10
Play/Pause
+10

Mic
Camera
Chat
Reaction
More
```

Strict Sync indicator appears in More or subtle status area.

---

# 32. HOST CONTROL VISUALS

Host:

playback buttons enabled.

Guest in Host Only mode:

playback buttons either:

- hidden;
- or disabled with tooltip:

```
Host controls playback.
```

Prefer disabled rather than showing useless interactive UI.

---

# 33. SHARED CONTROLS

When enabled:

show:

```
Shared Controls On
```

Guest actions may show a tiny state:

```
Requesting seek...
```

until coordinator confirms.

---

# 34. CHAT COMPOSE

Press Enter.

Input appears bottom center, above control dock.

Width:

```
min(560px, 70vw)
```

No big panel.

---

# 35. CHAT MESSAGE PRESENTATION

Message:

```
Rahul
bro that was insane 😭
```

Position:

lower third.

Avoid covering subtitles.

If subtitles detected/known:

move chat higher.

Default lifetime:

```
5 sec
```

Messages queue.

Maximum simultaneously visible:

```
3
```

Older messages fade sooner if necessary.

---

# 36. CHAT HISTORY

Press C.

Overlay width:

```
min(620px, 75vw)
```

Height:

```
min(560px, 70vh)
```

Centered translucent card.

Movie remains full size behind.

Do not pause automatically.

---

# 37. REACTIONS

Reaction originates near sender camera card if visible.

Then floats/fades toward center edge.

Lifetime:

```
2–3 sec
```

Do not flood.

Rate limit:

```
max 5 reactions / 3 seconds / participant
```

---

# 38. BUFFERING STATE

When strict sync pauses:

darken movie slightly.

Show centered status:

```
Paused to keep you together

Rahul is buffering

████████████░░░

Camera and chat are still available.
```

Do not use embarrassing language such as:

```
Rahul has bad internet
```

---

# 39. BUFFER RECOVERY

When safe:

```
Both ready

Resuming in

3
2
1
```

Again driven by backend timing.

---

# 40. DISCONNECT

Immediate state:

```
Rahul disconnected.

The movie has been paused.

Reconnecting...
```

Buttons after short grace:

```
[ Keep Waiting ]
[ Continue Without Rahul ]
```

Continue button host only.

---

# 41. SEEK PREPARATION

When host jumps into unavailable guest region:

```
Preparing new position...

Rahul
██████████░░░

Downloading required movie section.
```

Host does not see destination frames before readiness.

---

# 42. PROVIDER SHARED MODE PREPARATION

```
Preparing Netflix Shared Mode

Chrome                     ✓
Netflix Login              ✓
Video Capture              Testing...
Audio Capture              ✓
Encoder                    ✓
Guest Connection           ✓
```

Possible result:

```
Shared Mode unavailable

Netflix protected video could not be captured on this device.

[ Use Sync Mode ]
[ Cancel ]
```

No technical DRM jargon unless details expanded.

---

# 43. MANAGED CHROME

When provider interaction is required:

Movie Party may show provider Chrome window.

Example:

```
Netflix requires sign-in.

Sign in directly in the Netflix window.
Movie Party never receives your password.

[ Open Netflix Window ]
```

After login, Movie Party can minimize/position it as appropriate.

Never fake provider login inside Movie Party.

---

# 44. PROVIDER MODE BADGE

Lobby only:

```
Shared Mode · Experimental
```

or:

```
Sync Mode
```

During movie, hide badge unless controls visible.

---

# 45. GHOST MODE

Shortcut:

```
Ctrl/Cmd + Shift + M
```

Immediately hide:

- camera card;
- chat;
- control dock;
- status indicators;
- room branding;
- reaction animations.

Movie remains.

Ghost Mode indicator must NOT remain visible permanently.

Temporary confirmation may appear for:

```
800ms
```

then disappear.

---

# 46. EXIT GHOST MODE

Same shortcut.

Restore overlays according to their previous visible/hidden configuration.

---

# 47. PRIVACY MODE

Shortcut:

```
Ctrl/Cmd + Shift + P
```

Actions:

```
Ghost Mode ON
Camera OFF
Microphone OFF
```

On exit:

UI returns.

Camera and mic remain OFF.

Show:

```
Privacy Mode ended.
Camera and microphone remain disabled.
```

---

# 48. MIC DEFAULT

Every party:

```
Muted
```

Even if user unmuted in previous party.

This resets for each new room.

---

# 49. CALL MODE SELECTION

Room can be created with:

```
Video + Voice
Voice Only
No Call
```

Participant may independently downgrade:

```
Video + Voice → Voice Only
Video + Voice → Off
Voice Only → Off
```

They may re-enable with explicit action.

---

# 50. CALL NETWORK ADAPTATION UX

Do not show constant camera resolution changes.

Only show if camera is substantially degraded:

```
Camera quality reduced to protect movie playback.
```

Once per event.

---

# 51. PARTY END

Host ends:

confirmation:

```
End Movie Party for everyone?

[ Cancel ]
[ End Party ]
```

---

# 52. LOCAL MEDIA RETENTION PROMPT

Guest:

```
Keep Interstellar on this device?

The movie was transferred for this party.

[ Remove ]
[ Keep in Movie Party ]
[ Save As... ]
```

Default focus:

```
Remove
```

but no automatic deletion without answering if application remains open.

If user quits:

respect configured policy.

---

# 53. HOME UPCOMING PARTIES

Cards:

```
Interstellar
Tonight · 10:00 PM
Rahul

Preload
62% complete
```

or:

```
Waiting for Rahul to come online
```

---

# 54. NOTIFICATIONS

Examples:

```
Movie Party
Interstellar preload starts in 30 minutes.
Keep this computer online.
```

```
Movie Party
Rahul is online. Movie preload has started.
```

```
Movie Party
Interstellar is ready for tonight.
```

Do not spam.

---

# 55. SETTINGS — GENERAL

```
Display name
Start Movie Party at login
Minimize to tray
Notification preferences
```

Auto-start default:

```
OFF
```

---

# 56. SETTINGS — PLAYBACK

```
Subtitle preference
Preferred audio track
Default volume
10-second seek amount
```

Strict Sync cannot be disabled in V1 normal settings.

---

# 57. SETTINGS — NETWORK

```
Tailscale status
Connection diagnostics
Port
Run speed test
```

Advanced only.

---

# 58. SETTINGS — CALL

```
Camera device
Microphone device
Speaker/output
Mirror self preview
Noise suppression if available
```

---

# 59. SETTINGS — STORAGE

```
Cache location
Cache used
Clear cache
Post-party media policy
```

---

# 60. SETTINGS — PROVIDERS

Each provider card:

```
Netflix

Chrome profile:
Ready

Session:
Signed in / Unknown / Login required

Shared Mode:
Experimental / Unsupported / Available

[ Open Provider ]
[ Reset Provider Profile ]
```

Reset requires confirmation because it logs user out.

---

# 61. SETTINGS — PRIVACY

```
Ghost Mode shortcut
Privacy Mode shortcut
Chat retention
Diagnostic log retention
```

---

# 62. SETTINGS — DIAGNOSTICS

```
App Version
Protocol Version
Tailscale Path
Peer RTT
Open Debug HUD
Export Diagnostic Bundle
```

---

# 63. DEBUG HUD

Not visible to normal users.

Overlay:

```
Room        PLAYING
Media       LOCAL
Position    00:41:22.491
Drift       +18 ms
RTT         21 ms
Path        DIRECT
Goodput     8.2 Mbps
Guest Buf   182 sec
Camera      360p15 / 320 kbps
```

Toggle dev shortcut determined during implementation.

---

# 64. ERROR SCREEN

Error layout:

```
Movie Party couldn't continue this session.

MP-NET-003

The current connection is too slow for this movie.

Options:

[ Preload More ]
[ Disable Video Call ]
[ Return to Lobby ]

[ Technical Details ]
```

Every recoverable error should offer useful action.

---

# 65. ACCESSIBILITY

Keyboard navigation required outside Cinema Mode.

Controls require:

- accessible names;
- focus indicators;
- screen-reader labels.

Do not rely solely on color.

---

# 66. SUBTITLE PROTECTION AREA

Bottom:

```
15% of movie height
```

considered subtitle-sensitive zone.

Avoid placing persistent camera/chat there unless user moves camera intentionally.

---

# 67. RESPONSIVE DESKTOP RULES

Minimum supported app:

```
900 × 600
```

Cinema should work at smaller windows but minimum supported UX target remains 1024×700.

No mobile layouts required.

---

# 68. FULLSCREEN

Fullscreen should use OS fullscreen behavior.

When entering:

- control dock fades;
- camera remains;
- chat remains;
- Ghost/Privacy shortcuts continue.

---

# 69. WINDOW CLOSE DURING PARTY

Prompt:

```
You're still in a Movie Party.

[ Cancel ]
[ Leave Party ]
```

Host:

```
[ End Party For Everyone ]
```

---

# 70. UI ACCEPTANCE CRITERIA

V1 UI passes when:

- movie never shrinks for chat;
- movie never shrinks for camera;
- buffering state is obvious;
- guest knows why movie paused;
- host/guest control authority is obvious;
- Ghost Mode visually hides social context;
- Privacy Mode stops mic/camera;
- provider failures have clean fallback UI;
- scheduled-preload state is clear;
- all major actions are usable via keyboard;
- no raw debug errors leak into normal UI.

```

---
```