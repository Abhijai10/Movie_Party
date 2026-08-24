# Milestone 6 Worklog — Managed Chrome + Provider Sync

## Status: 🟨 IN PROGRESS

## Architecture

### Chrome Process Management (`src-tauri/src/providers/chrome/mod.rs`)
- ManagedChromeSession owns Chrome child process
- Drop kills the Chrome process
- Stored in AppRuntimeState.chrome_session (no leak)

### AppRuntime Integration
- `store_chrome_session()`: stores in AppRuntimeState, drops old session
- `launch_provider` Tauri command starts Chrome + CDP connection
- Health check method for Chrome process status

### Provider Adapters
- YouTube adapter (`src-tauri/src/providers/youtube/mod.rs`)
- Netflix adapter (`src-tauri/src/providers/netflix/mod.rs`)
- Prime adapter (`src-tauri/src/providers/prime/mod.rs`)
- JioHotstar adapter (`src-tauri/src/providers/hotstar/mod.rs`)
- Status: SUPPORTED/EXPERIMENTAL/SYNC_ONLY/UNSUPPORTED

### Frontend
- `launchProvider` function in appRuntime.ts

## Tests
- Chrome unit tests (health check, process lifecycle)
- Provider adapter unit tests

## Files
- `src-tauri/src/providers/chrome/mod.rs` — Chrome launch, CDP, session
- `src-tauri/src/providers/youtube/mod.rs` — YouTube adapter
- `src-tauri/src/providers/netflix/mod.rs` — Netflix adapter
- `src-tauri/src/providers/prime/mod.rs` — Prime adapter
- `src-tauri/src/providers/hotstar/mod.rs` — JioHotstar adapter
- `src-tauri/src/providers/sync.rs` — Provider sync engine
- `src-tauri/src/app_runtime.rs` — chrome_session field, store_chrome_session
- `src/backend/appRuntime.ts` — launchProvider

## External Verification Needed
1. Real Chrome installed → launch_provider → CDP connection
2. YouTube video → player detection → duration/position/play/pause/seek
3. M2 canonical operation → provider adapter → actual provider player
