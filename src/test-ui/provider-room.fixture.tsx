// Isolated UI fixture with mocked IPC; never launches Chrome or joins a room.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { AppSnapshot } from "../backend/appRuntime";
import { LobbyView } from "../views/LobbyView";
import { createCallTileSessionState } from "../overlays/callTileState";
import "../styles.css";
const initial: AppSnapshot = {
  screen: "LOBBY",
  room: {
    roomId: "fixture",
    inviteCode: null,
    role: "GUEST",
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
      version: 1,
      provider: "youtube",
      content_id: "abcdefghijk",
      canonical_url: "https://www.youtube.com/watch?v=abcdefghijk",
      title: "Selected video — isolated fixture",
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
mockWindows("main");
let fail = false;
mockIPC((command) => {
  if (command === "retry_room_provider") {
    if (fail)
      throw new Error("Fixture: managed Chrome is unavailable. Retry after installing Chrome.");
    return {
      ...initial,
      provider: {
        ...initial.provider,
        state: "OPENING · Opening selected content",
        readiness: "NAVIGATING",
      },
      providerContent: { ...initial.providerContent, localState: "OPENING" },
    };
  }
  throw new Error(`Unexpected fixture IPC: ${command}`);
});
function Fixture() {
  const [snapshot, setSnapshot] = useState(initial);
  const [call, setCall] = useState({ ...createCallTileSessionState(), isHidden: true });
  return (
    <>
      <aside
        style={{
          position: "fixed",
          zIndex: 100,
          bottom: 0,
          left: 0,
          padding: 8,
          background: "#171322",
          color: "white",
        }}
      >
        Isolated mocked IPC fixture ·{" "}
        <button
          onClick={() => {
            fail = !fail;
          }}
        >
          Toggle retry failure
        </button>{" "}
        <button
          onClick={() => {
            setSnapshot(initial);
          }}
        >
          Reset authentication state
        </button>
      </aside>
      <LobbyView
        snapshot={snapshot}
        onBack={() => {}}
        onReady={() => {}}
        onCinema={() => {}}
        onToggleSharedControls={() => {}}
        onSnapshot={setSnapshot}
        callTileSession={call}
        onCallTileSessionChange={setCall}
      />
    </>
  );
}
const root = document.getElementById("root");
if (!root) throw new Error("Missing fixture root");
createRoot(root).render(<Fixture />);
