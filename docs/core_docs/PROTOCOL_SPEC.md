
# DOCUMENT 2 — `PROTOCOL_SPEC.md`

# Movie Party Network Protocol Specification
## Protocol Version 1

Status: LOCKED FOR INITIAL IMPLEMENTATION

---

# 1. PURPOSE

This document defines the exact peer-to-peer control and coordination protocol used by Movie Party V1.

V1 supports exactly:


1 Host
1 Guest


but all messages use Device IDs to preserve future multi-user extensibility.

---

# 2. TRANSPORT

Primary transport:

```
QUIC over Tailscale
```

Default UDP port:

```
47821
```

Host binds to the Tailscale address only.

---

# 3. SERIALIZATION

Production serialization (amended by ADR-0001):

```
JSON (UTF-8) with a u32 big-endian length prefix per QUIC stream message
```

Serde enum tags (`"type": "<MessageName>"`) discriminate messages on the
wire. The numeric message-ID registry (§11) is the authoritative
cross-version registry but is not carried on the wire in V1.

Reason:

- both ends ship from this single repository on the same cadence, so
  interop with an independent CBOR client is not a V1 constraint;
- JSON keeps packet dumps human-readable during QUIC bring-up debugging;
- protocol traffic volume is tiny relative to movie traffic, so encoding
  size is irrelevant at V1 scale.

Future protocol versions may introduce compact binary encodings and/or
numeric keys; doing so requires a protocol minor bump plus §11-first
registry planning, and per §68 no registered ID may ever be
redefined.

Original V1 draft locked Canonical CBOR (RFC 8949); the implementation
shipped JSON from its first QUIC transport milestone and the divergence
was ratified as the canonical format by ADR-0001 (Option B: amend the
spec to match audited reality rather than destabilize the wire during
V1 feature completion).

---

# 4. BYTE ORDER

Any explicit fixed-width binary integers outside the serialized payload
(length prefixes, magic numbers, chunk indices):

```
network byte order / big endian
```

---

# 5. MAXIMUM CONTROL MESSAGE SIZE

Control message maximum:

```
256 KiB
```

Any control message larger than this must be rejected:

```
MP-PROTO-004 MESSAGE_TOO_LARGE
```

Enforcement (implemented): the length prefix is checked against
the limit BEFORE any allocation on both ends — a peer advertising an
oversized frame is rejected at the framing layer without reading the body.
The same gate applies to locally-built frames on the send path.

Movie/file chunks are transported on dedicated streams and are not subject
to this control-message limit.

---

# 6. PROTOCOL VERSION

V1:

```
major = 1
minor = 0
```

Compatibility:

- same major required;
- receiver may tolerate newer minor if unknown fields are safely ignorable.

Different major:

reject connection.

---

# 7. DEVICE ID

`device_id`

Format:

```
UUIDv7 string
```

Example:

```
0198c3d0-7c55-7f82-9af2-36c9946b2974
```

---

# 8. ROOM ID

`room_id`

128 cryptographically random bits encoded:

```
Base64URL without padding
```

---

# 9. JOIN SECRET

256 cryptographically random bits.

Never log it.

Never store permanently after room expiration unless required for active scheduled room restoration.

---

# 10. ENVELOPE

Every application message uses:

```
{
  "v_major": 1,
  "v_minor": 0,
  "room_id": "string",
  "seq": 42,
  "sender": "device-id",
  "sent_mono_us": 125992311,
  "type": "ReadyState",
  "payload": {}
}
```

Per ADR-0001, `type` is a serde string tag in V1 (the §11 registry
remains the authoritative numeric mapping for future encodings).

Fields:

## `v_major`

Unsigned integer.

Required.

## `v_minor`

Unsigned integer.

Required.

## `room_id`

String.

Required after authentication.

During initial HELLO:

may be empty.

## `seq`

Unsigned 64-bit integer.

Monotonically increasing per sender per connection.

Starts at:

```
1
```

Never reuse sequence number on same connection.

## `sender`

Device ID.

## `sent_mono_us`

Sender's local monotonic clock reading in microseconds.

Used for diagnostics.

Not directly assumed to be comparable across devices.

## `type`

Message type discriminator.

V1 wire format: serde string tag (ADR-0001), e.g. `"ReadyState"`.

The §11 numeric registry is authoritative for future numeric encodings;
unknown discriminators (string or numeric) must fail parsing and be
rejected per §67.

## `payload`

JSON object (V1 wire format per ADR-0001).

---

# 11. MESSAGE TYPE REGISTRY

## Connection / authentication

```
1   HELLO
2   AUTH_REQUEST
3   AUTH_ACCEPT
4   AUTH_REJECT
5   HEARTBEAT
6   GOODBYE
```

## Clock

```
20  CLOCK_PING
21  CLOCK_PONG
22  CLOCK_RESULT
```

## Room

```
40  JOIN_REQUEST
41  JOIN_ACCEPT
42  JOIN_REJECT
43  ROOM_STATE
44  LEAVE_ROOM
45  DISCONNECT_NOTICE
46  RECONNECT_REQUEST
47  RECONNECT_COMPLETE
```

## Media

```
60  MEDIA_ANNOUNCE
61  MEDIA_READY
62  MEDIA_NOT_READY
63  PLAYER_STATE
64  BUFFER_STATUS
65  BUFFER_LOW
66  BUFFER_RECOVERED
```

## Playback

```
80  PLAY_PREPARE
81  PLAY_READY
82  PLAY_COMMIT

83  PAUSE_PREPARE
84  PAUSE_READY
85  PAUSE_COMMIT

86  SEEK_PREPARE
87  SEEK_READY
88  SEEK_COMMIT
```

## Shared controls

```
100 CONTROL_REQUEST
101 CONTROL_GRANT
102 CONTROL_REVOKE
103 CONTROL_DENY
```

## File transfer

```
120 TRANSFER_MANIFEST
121 CHUNK_REQUEST
122 CHUNK_ACK
123 TRANSFER_PROGRESS
124 TRANSFER_COMPLETE
125 TRANSFER_ERROR
```

Actual chunk bytes use dedicated QUIC streams.

## Network

```
140 NETWORK_STATS
141 PATH_STATE
```

## Call

```
160 CALL_STATE
161 CAMERA_STATE
162 MIC_STATE
163 CALL_SIGNAL
```

## Social

```
180 CHAT_MESSAGE
181 REACTION
```

## Scheduling

```
200 SCHEDULE_CREATE
201 SCHEDULE_ACCEPT
202 SCHEDULE_UPDATE
203 SCHEDULE_CANCEL
204 PRELOAD_STATE
```

## Provider

```
220 PROVIDER_STATE
221 PROVIDER_ERROR
222 PROVIDER_MODE_REQUEST
223 PROVIDER_MODE_RESULT
224 PROVIDER_SELECTION
225 PROVIDER_SELECTION_GET
226 PROVIDER_SELECTION_ACK
227 PROVIDER_PLAYBACK
```

## Errors

```
250 ERROR
```

---

# 12. HELLO

Type:

```
1
```

Payload:

```
{
  "device_id": "uuid",
  "display_name": "Abhijai Mac",
  "platform": "macos",
  "arch": "arm64",
  "app_version": "0.1.0",
  "protocol_major": 1,
  "protocol_minor": 0,
  "public_key": "base64url"
}
```

`platform` enum:

```
windows
macos
```

Invalid other platform in V1:

reject.

---

# 13. AUTH_REQUEST

```
{
  "room_id": "...",
  "join_secret": "...",
  "invite_nonce": "...",
  "device_signature": "..."
}
```

Signature covers canonical serialized:

```
room_id
join_secret_hash
invite_nonce
device_id
```

Do not transmit private key.

---

# 14. AUTH_ACCEPT

```
{
  "host_device_id": "...",
  "guest_device_id": "...",
  "session_id": "...",
  "room_role": "guest"
}
```

Host receives corresponding local state without sending itself AUTH_ACCEPT.

---

# 15. AUTH_REJECT

```
{
  "code": "INVALID_SECRET",
  "message": "The invitation is invalid or has expired."
}
```

Codes:

```
INVALID_SECRET
INVITE_EXPIRED
ROOM_CLOSED
PROTOCOL_MISMATCH
DEVICE_NOT_ALLOWED
INTERNAL_ERROR
```

---

# 16. HEARTBEAT

Interval:

```
2 seconds
```

Payload:

```
{
  "room_state": "PLAYING",
  "last_seen_peer_seq": 991
}
```

Consider connection degraded:

```
no heartbeat for 6 seconds
```

Disconnected:

```
no heartbeat for 10 seconds
```

---

# 17. CLOCK_PING

Payload:

```
{
  "probe_id": 12,
  "t0_us": 99231993
}
```

Guest usually sends to host.

---

# 18. CLOCK_PONG

Host responds immediately:

```
{
  "probe_id": 12,
  "t0_us": 99231993,
  "host_receive_us": 191288833,
  "host_send_us": 191288921
}
```

Guest records local receive time separately.

---

# 19. CLOCK_RESULT

Guest informs host:

```
{
  "offset_to_host_us": 91236762,
  "rtt_us": 17322,
  "sample_count": 20,
  "quality": "GOOD"
}
```

Quality:

```
EXCELLENT
GOOD
POOR
UNUSABLE
```

---

# 20. JOIN_REQUEST

```
{
  "desired_display_name": "Rahul",
  "capabilities": {
    "video_call": true,
    "camera": true,
    "microphone": true,
    "libmpv": true,
    "provider_shared_decode_h264": true
  }
}
```

---

# 21. JOIN_ACCEPT

```
{
  "room_state": "LOBBY",
  "strict_sync": true,
  "shared_controls": false,
  "active_media": null,
  "call_mode": "VIDEO_VOICE"
}
```

---

# 22. ROOM_STATE

Payload:

```
{
  "revision": 51,
  "state": "PLAYING",
  "host_device_id": "...",
  "guest_device_id": "...",
  "strict_sync": true,
  "shared_controls": false,
  "call_mode": "VIDEO_VOICE",
  "media_id": "...",
  "canonical_position_ms": 1039212,
  "presentation_epoch": 8
}
```

`revision` increases whenever canonical state changes.

Ignore stale room-state revision.

---

# 23. MEDIA TYPES

```
LOCAL
PROVIDER_SHARED
PROVIDER_SYNC
YOUTUBE
GENERIC_WEB
```

YouTube may internally use provider infrastructure while retaining explicit media type for diagnostics.

---

# 24. MEDIA_ANNOUNCE

Local:

```
{
  "media_id": "...",
  "media_type": "LOCAL",
  "title": "Interstellar",
  "duration_ms": 10140000,
  "local": {
    "file_size": 4500000000,
    "container": "mkv",
    "video_codec": "h264",
    "average_bitrate_bps": 4100000,
    "chunk_size": 1048576,
    "chunk_count": 4292,
    "quick_fingerprint": "...",
    "full_hash": "..."
  }
}
```

Provider:

```
{
  "media_id": "...",
  "media_type": "PROVIDER_SHARED",
  "title": "Interstellar",
  "provider": "netflix",
  "source_url": "https://...",
  "provider_content_id": "optional"
}
```

URLs must be sanitized before logging.

---

# 25. MEDIA_READY

```
{
  "media_id": "...",
  "ready": true,
  "buffer_ahead_ms": 180000,
  "player_ready": true
}
```

---

# 26. MEDIA_NOT_READY

```
{
  "media_id": "...",
  "reason": "BUFFERING",
  "buffer_ahead_ms": 1100
}
```

Reasons:

```
NO_MEDIA
BUFFERING
MISSING_CHUNK
PLAYER_ERROR
PROVIDER_ERROR
NETWORK_INSUFFICIENT
DECODER_NOT_READY
```

---

# 27. PLAYER_STATE

Sent every:

```
500ms while playing
1000ms while paused
```

Payload:

```
{
  "position_ms": 512491,
  "state": "PLAYING",
  "playback_rate": 1.0,
  "buffer_ahead_ms": 93421,
  "dropped_frames": 0,
  "player_clock_us": 818291889
}
```

State:

```
STOPPED
READY
PLAYING
PAUSED
BUFFERING
SEEKING
ERROR
```

---

# 28. BUFFER_STATUS

```
{
  "position_ms": 512491,
  "buffer_ahead_ms": 93421,
  "cache_ahead_bytes": 214748364,
  "transport_goodput_bps": 7200000,
  "stalled": false
}
```

---

# 29. BUFFER_LOW

```
{
  "position_ms": 512491,
  "buffer_ahead_ms": 2200,
  "cause": "NETWORK"
}
```

Host response:

initiate PAUSE_PREPARE.

---

# 30. BUFFER_RECOVERED

```
{
  "buffer_ahead_ms": 12000
}
```

This does not automatically resume playback.

Coordinator still performs READY consensus.

---

# 31. PLAY_PREPARE

Host only.

```
{
  "operation_id": "uuid",
  "target_position_ms": 512491,
  "minimum_buffer_ms": 5000
}
```

Guest validates readiness.

---

# 32. PLAY_READY

```
{
  "operation_id": "...",
  "ready": true,
  "position_ms": 512490,
  "buffer_ahead_ms": 19321
}
```

---

# 33. PLAY_COMMIT

Host only.

```
{
  "operation_id": "...",
  "target_position_ms": 512491,
  "execute_at_host_mono_us": 9182811991,
  "presentation_epoch": 9
}
```

Both host and guest schedule execution.

---

# 34. PAUSE_PREPARE

```
{
  "operation_id": "...",
  "reason": "GUEST_BUFFER_LOW",
  "target_position_ms": 519201
}
```

Pause target must be at or slightly ahead of current canonical presentation position.

---

# 35. PAUSE_READY

```
{
  "operation_id": "...",
  "ready": true
}
```

---

# 36. PAUSE_COMMIT

```
{
  "operation_id": "...",
  "target_position_ms": 519201,
  "execute_at_host_mono_us": 9183811991
}
```

---

# 37. SEEK_PREPARE

```
{
  "operation_id": "...",
  "target_position_ms": 3120000,
  "initiator": "host"
}
```

Guest must not commit until required media is available.

---

# 38. SEEK_READY

```
{
  "operation_id": "...",
  "ready": true,
  "buffer_ahead_ms": 30000
}
```

---

# 39. SEEK_COMMIT

```
{
  "operation_id": "...",
  "target_position_ms": 3120000,
  "execute_at_host_mono_us": 1192828311,
  "resume_after_seek": true
}
```

---

# 40. DUPLICATE OPERATION HANDLING

All coordinated playback operations use:

```
operation_id
```

If the same operation is received twice:

do not execute twice.

Return same readiness/result.

---

# 41. CONTROL_REQUEST

Guest only.

```
{
  "request_id": "...",
  "action": "PAUSE",
  "parameters": {}
}
```

Actions:

```
PLAY
PAUSE
SEEK
SEEK_RELATIVE
```

If Shared Controls false:

host sends CONTROL_DENY.

---

# 42. CONTROL_GRANT

```
{
  "request_id": "...",
  "action": "PAUSE"
}
```

Grant does not mean execute directly.

Host then creates normal authoritative playback operation.

---

# 43. TRANSFER_MANIFEST

```
{
  "media_id": "...",
  "file_size": 4500000000,
  "chunk_size": 1048576,
  "chunk_count": 4292,
  "full_hash": "...",
  "chunk_hash_algorithm": "BLAKE3"
}
```

---

# 44. CHUNK_REQUEST

Control request:

```
{
  "request_id": "...",
  "media_id": "...",
  "chunks": [
    {
      "index": 120,
      "priority": 0
    },
    {
      "index": 121,
      "priority": 0
    }
  ]
}
```

Maximum chunk entries per request:

```
256
```

---

# 45. CHUNK DATA STREAM FORMAT

Each requested chunk may be transported on dedicated QUIC stream.

Binary header:

```
Magic              4 bytes = MPCK
Version            1 byte
Media ID length    1 byte
Media ID           variable UTF-8
Chunk index        uint32
Payload length     uint32
Chunk hash         32 bytes BLAKE3
Payload            N bytes
```

Reject chunk if:

- index out of range;
- payload length > configured chunk size except final chunk;
- hash mismatch;
- media ID mismatch.

---

# 46. CHUNK_ACK

```
{
  "media_id": "...",
  "chunk_index": 120,
  "status": "OK"
}
```

Statuses:

```
OK
HASH_MISMATCH
REJECTED
```

---

# 47. TRANSFER_PROGRESS

Sent maximum once per second.

```
{
  "media_id": "...",
  "bytes_available": 1200000000,
  "bytes_total": 4500000000,
  "buffer_ahead_ms": 420000,
  "goodput_bps": 8100000
}
```

---

# 48. NETWORK_STATS

```
{
  "rtt_ms": 18.2,
  "loss_estimate": 0.002,
  "goodput_up_bps": 8100000,
  "goodput_down_bps": 8200000,
  "path": "DIRECT"
}
```

Path:

```
DIRECT
PEER_RELAY
DERP_RELAY
UNKNOWN
```

---

# 49. CALL_STATE

```
{
  "mode": "VIDEO_VOICE",
  "connected": true
}
```

Call mode:

```
VIDEO_VOICE
VOICE_ONLY
OFF
```

---

# 50. CAMERA_STATE

```
{
  "enabled": true,
  "tier": "B",
  "width": 640,
  "height": 360,
  "fps": 15,
  "target_bitrate_bps": 350000
}
```

---

# 51. MIC_STATE

```
{
  "enabled": false
}
```

Initial state MUST be false.

---

# 52. CALL_SIGNAL

Opaque WebRTC/native-call signalling payload.

```
{
  "signal_type": "OFFER",
  "data": "..."
}
```

Allowed:

```
OFFER
ANSWER
ICE
RENEGOTIATE
```

Maximum payload:

```
64 KiB
```

## 52.1 RENEGOTIATE semantics

`RENEGOTIATE` is NOT an SDP. It is a session-restart REQUEST marker —
a small JSON object, never a session description:

```
{
  "request": "renegotiate",
  "v": 1,
  "id": "s3-x7q9z"
}
```

Rules:

1. `RENEGOTIATE` payloads are capped at **2 KiB** (all other signal types
   may use the full 64 KiB). A session description can never fit 2 KiB,
   so a mislabeled SDP is rejected and the two meanings cannot be
   confused. Oversized markers fail with `MP-CALL-001`.
2. `id` is a session-unique identity string generated locally (monotonic
   counter + random suffix). A device that sees its OWN marker echoed
   back (the local snapshot contains every locally-published signal)
   MUST NOT apply it; only a peer's marker is applied.
3. A valid marker conditionally resets the call signal ledger: a
   reset happens ONLY when an exchange was started but never completed
   (`offer_pending` and no answer seen). A completed exchange is left
   intact so an in-flight ANSWER cannot be poisoned (MP-CALL-003).
4. Both devices publish their marker at call-session start (host and
   guest), then the host publishes the OFFER. This unblocks a fresh
   exchange regardless of what a previous session left pending and
   pokes a still-live peer whose wire looks connected.

## 52.2 Validation summary

| Signal      | Valid when                                    | Ledger effect                    |
| ----------- | --------------------------------------------- | -------------------------------- |
| OFFER       | no unanswered offer pending                   | marks offer pending             |
| ANSWER      | an offer is pending                           | marks exchange completed        |
| ICE         | any time (late/duplicate candidates are no-op at the session layer) | none |
| RENEGOTIATE | payload ≤ 2 KiB                               | conditional reset (see 52.1.3)  |

All violations fail with `MP-CALL-001` (invalid call signal).

---

# 53. CHAT_MESSAGE

```
{
  "message_id": "uuid",
  "body": "That scene was insane 😭",
  "created_host_time_us": 9382828172
}
```

Maximum UTF-8 body:

```
2000 bytes
```

Messages larger than limit rejected.

---

# 54. REACTION

```
{
  "reaction_id": "uuid",
  "reaction": "😂"
}
```

Allowed V1:

```
😂 ❤️ 😮 🔥 😭 👏
```

Unknown reaction ignored.

---

# 55. SCHEDULE_CREATE

```
{
  "schedule_id": "...",
  "scheduled_start_utc_ms": 1786811400000,
  "media_id": "...",
  "call_mode": "VIDEO_VOICE",
  "planned_preload_utc_ms": 1786800600000
}
```

Scheduling uses wall-clock UTC.

Playback synchronization does not.

---

# 56. SCHEDULE_ACCEPT

```
{
  "schedule_id": "...",
  "accepted": true
}
```

Upon acceptance:

guest must persist schedule locally and register notifications.

---

# 57. PRELOAD_STATE

```
{
  "schedule_id": "...",
  "state": "TRANSFERRING",
  "progress": 0.43,
  "estimated_ready_utc_ms": 1786807112345
}
```

States:

```
WAITING_FOR_HOST
WAITING_FOR_GUEST
TRANSFERRING
READY
FAILED
CANCELLED
```

---

# 58. PROVIDER_STATE

```
{
  "provider": "netflix",
  "mode": "PROVIDER_SHARED",
  "state": "PLAYING",
  "position_ms": 512491,
  "buffering": false,
  "capture_available": true
}
```

---

# 59. PROVIDER_ERROR

```
{
  "provider": "netflix",
  "code": "PROTECTED_CAPTURE_UNAVAILABLE",
  "recoverable": true,
  "suggested_fallback": "PROVIDER_SYNC"
}
```

---

# 60. PROVIDER_MODE_REQUEST

Host proposes:

```
{
  "provider": "netflix",
  "requested_mode": "PROVIDER_SHARED"
}
```

---

# 61. PROVIDER_MODE_RESULT

```
{
  "provider": "netflix",
  "requested_mode": "PROVIDER_SHARED",
  "available": false,
  "reason": "PROTECTED_CAPTURE_UNAVAILABLE"
}
```

No automatic invisible fallback.

---

# 62. ERROR MESSAGE

```
{
  "code": "MP-SYNC-004",
  "severity": "ERROR",
  "message": "Playback synchronization could not be restored.",
  "operation_id": "optional"
}
```

Severity:

```
INFO
WARNING
ERROR
FATAL
```

---

# 63. STATE VALIDATION

Each message is valid only in appropriate room states.

Example:

`PLAY_COMMIT`

valid:

```
READY_CHECK
PAUSED
BUFFERING
```

Invalid during:

```
ENDED
ERROR
WAITING_FOR_GUEST
```

Invalid command:

respond ERROR and do not execute.

---

# 64. SEQUENCE HANDLING

Maintain:

```
last_seq_received
```

If:

```
seq <= last_seq_received
```

and message is not part of explicitly replay-safe recovery:

discard as duplicate/stale.

---

# 65. RECONNECTION

A reconnect establishes new QUIC connection.

Peer sends:

```
RECONNECT_REQUEST
```

including:

```
{
  "previous_session_id": "...",
  "last_room_revision": 59,
  "last_seen_peer_seq": 1091
}
```

Host responds with current canonical ROOM_STATE.

Do not replay entire historical stream.

---

# 66. SECURITY REQUIREMENTS

QUIC encryption is mandatory.

Application-level room authentication remains mandatory even over Tailscale.

Tailscale membership alone does not grant room access.

---

# 67. FUZZ TEST REQUIREMENT

Protocol parser must be fuzz/property tested for (V1 wire format is JSON
per ADR-0001; a future binary encoding inherits the same requirement):

- malformed frames (invalid JSON / malformed CBOR in future encodings);
- oversized frames (length prefix above §5 limit → MP-PROTO-004);
- missing fields;
- negative values where unsigned required;
- very large integers;
- malformed UTF-8;
- unknown message types;
- duplicate keys.

Malformed peer input must not crash application.

---

# 68. PROTOCOL VERSIONING RULE

Never redefine the meaning of an existing message ID.

If semantics change incompatibly:

increment protocol major.

If adding optional fields:

increment minor if appropriate.

## Provider selection extension, version 1 (Batch 3)

The existing JSON envelope remains version 1.0. This extension is negotiated
independently: authenticated `HelloPayload` advertises optional
`provider_selection_version: 1`; `AuthAccept` carries optional `provider_room`
only for peers advertising that version. A provider room rejects absent or
unsupported capabilities with `PROVIDER_SELECTION_VERSION_UNSUPPORTED` after
room authentication. Local Perfect accepts legacy Hello/AuthAccept shapes;
old peers never receive provider-only events. A Local Perfect room with an
incompatible connected peer cannot convert to Provider Sync. No existing ID
or message meaning is reassigned. Registry IDs remain descriptive; the wire
uses Serde string tags as specified in ADR-0001.

`ProviderRoomDescriptor` contains `version`, UUID `state_epoch`, and nullable
`selection`. The host alone publishes `ProviderSelection`: `version`,
`room_id`, `state_epoch`, positive `host_sequence`, positive `content_revision`,
canonical `provider`, `content_id`, validated `canonical_url`, bounded `title`,
and public player `evidence` (title, finite positive duration, optional public
player content ID). Changes increase both counters; duplicate unchanged
selections do not. Guests validate room, immutable epoch, supported version,
canonical identity/URL, counters and evidence before applying. An older
selection, same-sequence conflict, revision rewind, epoch replacement or
selection clearing cannot overwrite accepted content. Joining and reconnecting
use AuthAccept, then authenticated `ProviderSelectionGet` repairs lost events.

`ProviderSelection` server events are accepted only from the pinned host.
There is no guest selection-write request. `ProviderSelectionAck` is an
authenticated guest request containing exact epoch/host sequence/content
revision, monotonic `acknowledgement_sequence`, state and observed evidence.
States are `UNSELECTED`, `OPENING`, `CONTENT_CONFIRMED`, `PLAYBACK_READY`,
`ERROR`, `AUTH_REQUIRED`. Confirmation and readiness require matching actual
player evidence, not merely an equal URL or open homepage. YouTube requires
the public player content ID; subscription fallback compares the observed
title and duration within 1.5 seconds, plus public ID where exposed. This
fallback is not cryptographic asset verification. Missing or mismatched
evidence fails closed; live/infinite-duration playback cannot become ready.

The guest checks selection and resends its current acknowledgement every two
seconds, with five-second request timeouts. Identical acknowledgement payloads
may be repeated on fresh authenticated transport sequences. Host readiness
expires twelve seconds after the last valid receipt; generic ReadyState,
PlayReady and SeekReady cannot bypass it. A replacement authenticated
connection clears readiness and retires the old connection's provider RPC
authority. Unverified loading reports error after 45 seconds; authentication
remains manual and explicit retry reopens only the authoritative selection.

`ProviderPlayback` wraps playback prepare/commit and coordinator/room state
events with `SelectionContext` (epoch, host sequence, content revision).
Provider guests reject plain or mismatched-context playback events; Local
Perfect continues using its existing messages. Selection changes and loss of
verified readiness cancel pending operations and pause strict synchronization.
Host-only controls remain the default; existing explicitly shared controls
and explicit guest-abandon/solo policy are retained.

No provider cookies, credentials, login tokens, browser storage or encryption
keys appear in this protocol. Both peers independently use their own managed
browser and authorized provider account. Provider Shared remains disabled.

## Provider playback consensus extension, version 2 (Batch 4)

Current Provider Sync requires `provider_selection_version: 2` in Hello and
version 2 in the selection/descriptor. Version 1 provider peers receive the
existing explicit incompatibility rejection. Local Perfect still uses envelope
1.0 and accepts legacy peers. Registry IDs 224–227 and existing enum meanings
are retained; no additional IDs or top-level message tags are introduced.
The following fields are optional in serialization, but their behavior is
protected by the provider capability version rather than silently offered to
version 1 clients.

`ProviderRoomDescriptor.playback` holds a host-owned `Operation`: selection
`context`, UUID `operation_id`, monotonic room operation `sequence`, `action`
(`PLAY`, `PAUSE`, `SEEK`), `target_ms`, seek `resume` policy, `phase`
(`VERIFYING`, `APPLIED`, `FAILED`) and optional `host`/`guest` reports.
`SelectionAck.playback` carries only the guest's `Report`: exact context,
operation ID/action, observed position and closed-enum outcome (`APPLIED`,
`GESTURE_REQUIRED`, `REJECTED`, `TIMED_OUT`, `BUFFERING`, `ENDED`). It never
contains arbitrary browser errors, URLs, titles, account state or credentials.

The host reserves an operation when broadcasting the existing canonical
commit. Each side executes at the existing host-clock deadline. Selected
provider rooms remain unconfirmed while commands are executing. CDP Boolean
success alone cannot move the room to PLAYING: actual public player identity,
position, seek completion, paused/playing state, buffer and ready state are
observed again. Play additionally requires observed position advancement.
Commands target the unique visible video on the pinned managed page; ambiguous
or missing media fails closed. The operation accepts position error up to
1.5 seconds; this tolerance is an acceptance bound, not a synchronization
precision guarantee. A play dispatch more than 500 ms late is rejected instead
of claiming a synchronized start after a delayed browser borrow.

Both reports must belong to the current selection and same operation, and
must report APPLIED before a two-person room confirms PLAYING. The existing
explicit guest-abandon policy can authorize solo playback. Reports from older
operations/contexts cannot complete a new operation. Identical reports are
idempotent, terminal decisions cannot rewind, and duplicate commits cannot
schedule a second provider command. A late join or reconnect cannot inherit
PLAYING merely by reading an old successful decision: its own current player
readiness and matching locally observed result are required.

Authenticated selection polling and acknowledgement retry remain every two
seconds. Preparation without commit, or verification without both successful
reports, fails after fifteen seconds (plus polling/scheduling delay). A failed
command, buffering, identity/authentication loss or disconnected peer keeps
strict synchronization paused; pause attempts also run when host state pulls
fail. Failure to confirm an actual pause instructs the user to pause Chrome
manually. Recovery never silently resumes: new readiness and a fresh canonical
play cycle are required. A verified seek pauses both players first; an explicitly
requested resume starts the ordinary play protocol only after seek verification.

Host-observed EOF ends the room; guest-only EOF retracts readiness and pauses
for host confirmation. Ended rooms do not automatically restart. A newly
selected revision clears old EOF/readiness, pending starts and operations.

NotAllowedError produces GESTURE_REQUIRED. The in-app action focuses the owned
Chrome tab; the user must click the actual video play button. Movie Party never
sets CDP `userGesture`, disables autoplay restrictions, or performs login.
Observed manual playback clears the restriction and returns to preparation.
Provider capabilities are labelled PARTIAL with external verification pending.
Provider APIs/DOM behavior, ads, browser policy and DRM may prevent precise
seeks or synchronization; no universal or frame-accurate guarantee is made.

Local-only diagnostic events have a closed schema: event name, action enum,
outcome enum and numeric revision. Provider URLs, account data, raw page errors
and browser storage are excluded rather than logged and subsequently scrubbed.

---

# 69. PROTOCOL V1 DEFINITION OF DONE

Protocol V1 is complete when:

- all message structs exist;
- round-trip serialization tests pass (JSON per ADR-0001);
- malformed message tests pass (§67 suite);
- host/guest simulator passes;
- playback operation idempotency passes;
- reconnect protocol passes;
- chunk integrity tests pass;
- protocol docs match implementation.

```

---
```
