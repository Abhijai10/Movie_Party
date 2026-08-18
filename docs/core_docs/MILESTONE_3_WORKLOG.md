# Milestone 3 Worklog — Local Perfect E2E

## Status: 🟨 IN PROGRESS

---

## Architecture Implemented

### MpvPlayer Backend (`src-tauri/src/media/player/mpv_backend.rs`)
- Dynamic loading via libloading (dlopen/dlsym)
- Real mpv_create → mpv_initialize → mpv_command lifecycle
- vo=null/ao=null REMOVED — mpv uses default Cocoa window
- Play/Pause/Seek/Volume/Rate/Position/Duration/Buffered/Error
- unsafe Send+Sync with SAFETY comment (mpv handle serialized through Mutex)

### Player Event Loop (`src-tauri/src/app_runtime.rs`)
- `spawn_player_event_loop()`: polls live player every 200ms
- Reads position_ms, duration_ms, state, buffered_ahead_ms
- Updates `state.sync.position_ms` and `state.player_snapshot`
- Feeds PlayerState::Buffering → strict_sync_paused
- Emits updated snapshots to frontend
- Cleanup on leave_party via `player_event_task` abort

### Coordinator → Player Dispatch
- `dispatch_player_play()`: called on PlayCommit
- `dispatch_player_pause()`: called on PauseCommit
- `dispatch_player_seek()`: called on SeekCommit
- All update `state.player_snapshot` after dispatch

### Range Server (`src-tauri/src/media/stream/range_server.rs`)
- Binds to 127.0.0.1:random with session token
- Serves validated byte ranges from SparseCache
- Missing ranges trigger priority chunk fetch
- HTTP range semantics supported
- 4 tests

### Native File Picker
- `pick_media_file` Tauri command using rfd
- Frontend: HomeScreen file picker button
- Backend: receives path, builds manifest, creates player

---

## Tests

### Component Tests (m3_integration.rs — 18 tests)
1. test_1_player_initialization_returns_structured_result
2. test_2_invalid_media_returns_structured_error
3. test_3_play_command_transitions_to_playing
4. test_4_pause_command_transitions_to_paused
5. test_5_seek_command_updates_position
6. test_6_position_reporting_reflects_playback_state
7. test_7_duration_reporting_none_for_stub
8. test_8_close_resets_all_state
9. test_9_set_volume
10. test_10_set_playback_rate
11. test_11_buffered_ahead_reporting
12. test_12_error_state_after_invalid_media
13. test_13_full_lifecycle_open_ready_play_pause_seek_resume_close (coordinator)
14. test_14_buffer_low_triggers_strict_sync
15. test_15_full_lifecycle_open_ready_play_pause_seek_resume_close (player)
16. test_16_player_error_does_not_corrupt_coordinator
17. test_17_appruntime_player_dispatch (E2E: AppRuntime creates player, dispatches commands)
18. test_18_player_buffering_feeds_strict_sync (E2E: buffer stall → strict_sync_paused)

### Range Server Tests (4 tests in range_server.rs)
- Range request with valid token
- Missing token rejection
- Range beyond EOF
- Partial range

---

## Files

- `src-tauri/src/media/player/mpv_backend.rs` — Real libmpv FFI backend
- `src-tauri/src/media/player/mod.rs` — LocalPlayer trait, PlayerSnapshot, LibMpvPlayer stub
- `src-tauri/src/app_runtime.rs` — Player ownership, event loop, dispatch
- `src-tauri/src/media/stream/range_server.rs` — HTTP range server
- `src-tauri/src/lib.rs` — pick_media_file command
- `src/backend/appRuntime.ts` — PlayerSnapshot type, pickMediaFile
- `src/lobby/HomeScreen.tsx` — File picker button
- `src/cinema/CinemaMode.tsx` — Player position/duration display

---

## Decisions

### vo=null removal
- Decision: Remove vo=null/ao=null
- Why: mpv default Cocoa window provides visible video
- Post-V1: Tauri render API embedding for in-app rendering

### cfg-gated MpvPlayer
- Decision: #[cfg(feature = "mpv")] for real MpvPlayer, LibMpvPlayer stub otherwise
- Why: libmpv may not be installed on all systems

### 200ms polling
- Decision: Player event loop polls every 200ms
- Why: Balances responsiveness with CPU usage; position changes are smooth

---

## External Verification Needed

1. Run .app bundle → pick .mp4 → verify visible video + audio in mpv window
2. Verify mpv Cocoa window appears alongside Move Party cinema
