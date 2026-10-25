// Isolated visual fixture; no real room, Chrome, player or provider results.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { AppSnapshot } from "../backend/appRuntime";
import { CinemaView } from "../views/CinemaView";
import { LobbyView } from "../views/LobbyView";
import { ReadyCheckView } from "../views/ReadyCheckView";
import { createCallTileSessionState } from "../overlays/callTileState";
import "../styles.css";
const initial: AppSnapshot = {
  screen: "CINEMA",
  room: {
    roomId: "fixture",
    inviteCode: null,
    role: "HOST",
    state: "LOBBY",
    hostOnlyControls: true,
    sharedControls: false,
    strictSync: true,
  },
  participants: [
    {
      id: "host",
      displayName: "Host fixture",
      role: "HOST",
      connected: true,
      mediaReady: false,
      cameraEnabled: false,
      microphoneEnabled: false,
      bufferAheadMs: 0,
    },
    {
      id: "guest",
      displayName: "Guest fixture",
      role: "GUEST",
      connected: true,
      mediaReady: false,
      cameraEnabled: false,
      microphoneEnabled: false,
      bufferAheadMs: 0,
    },
  ],
  media: null,
  transfer: null,
  buffer: { guestBufferAheadMs: 0, percent: 0, bufferingParticipant: null },
  sync: { roomState: "LOBBY", strictSyncPaused: true, positionMs: 0, pendingOperation: null },
  network: {
    transport: "QUIC",
    path: "Isolated fixture",
    goodputBps: 0,
    rttMs: null,
    connected: true,
  },
  call: {
    mode: "OFF",
    status: "unavailable",
    connected: false,
    camera: { enabled: false, tier: "D", width: 0, height: 0, fps: 0, targetBitrateBps: 0 },
    microphone: { enabled: false },
    cameraNotice: null,
  },
  callSignals: [],
  provider: {
    mode: "PROVIDER_SYNC",
    providerId: "youtube",
    url: "https://www.youtube.com/watch?v=abcdefghijk",
    state: "AUTH_REQUIRED · Sign in on the provider's Chrome page, then open the host's title",
    readiness: "LOGIN_REQUIRED",
  },
  providerContent: {
    selection: {
      version: 2,
      provider: "youtube",
      content_id: "abcdefghijk",
      canonical_url: "https://www.youtube.com/watch?v=abcdefghijk",
      title: "YouTube player — isolated layout fixture",
      state_epoch: "fixture",
      host_sequence: 2,
      content_revision: 2,
    },
    localState: "AUTH_REQUIRED",
    peerState: "CONTENT_CONFIRMED",
  },
  chat: [],
  reactions: [],
  ghostMode: false,
  privacyMode: false,
  lastRecovery: null,
  error: null,
  player: {
    state: "IDLE",
    positionMs: 0,
    durationMs: null,
    volume: 1,
    playbackRate: 1,
    bufferedAheadMs: null,
    errorMessage: null,
    presentation: {
      mode: "UNAVAILABLE",
      platform: "fixture",
      bridge: "fixture",
      resizeManaged: false,
      ipcManaged: false,
      lifecycleManaged: false,
      message: "Isolated fixture",
    },
  },
  pendingGuestSchedule: null,
  retentionPrompt: null,
};

initial.youtube = {
  enabled: true,
  chosen: true,
  lease: null,
  command: null,
  metadata: null,
  error: 153,
};
const captureStyle = document.createElement("style");
captureStyle.textContent =
  "* { animation: none !important; transition: none !important; } [style*=opacity] { opacity: 1 !important; }";
document.head.appendChild(captureStyle);
mockWindows("main");
mockIPC((command) => {
  if (command === "get_provider_capabilities")
    return [{ id: "youtube", displayName: "YouTube", syncSupported: true, sharedSupported: false }];
  if (command === "youtube_driver")
    return { ...initial, youtube: { ...initial.youtube, enabled: false } };
  return null;
});
function Fixture() {
  const [snapshot, setSnapshot] = useState(initial),
    [call, setCall] = useState({ ...createCallTileSessionState(), isHidden: true });
  const page = new URLSearchParams(location.search).get("page");
  const props = {
    snapshot,
    onSnapshot: setSnapshot,
    callTileSession: call,
    onCallTileSessionChange: setCall,
  };
  return (
    <>
      {page === "lobby" ? (
        <LobbyView
          {...props}
          onBack={() => undefined}
          onReady={() => undefined}
          onCinema={() => undefined}
          onToggleSharedControls={() => undefined}
        />
      ) : page === "ready" ? (
        <ReadyCheckView
          {...props}
          onBack={() => undefined}
          onStarted={() => undefined}
          onRequestCountdown={() => undefined}
        />
      ) : (
        <CinemaView {...props} onLeave={() => undefined} />
      )}
    </>
  );
}
const root = document.getElementById("root");
if (root) createRoot(root).render(<Fixture />);
