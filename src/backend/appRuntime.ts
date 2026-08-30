import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { CallMode } from "../call/webrtc";

export type BackendFailureKind = "backend failure" | "network failure" | "invalid state";

export class BackendCommandError extends Error {
  readonly command: string;
  readonly code: string;
  readonly kind: BackendFailureKind;

  constructor(command: string, detail: string) {
    const sanitized = sanitizeErrorDetail(detail);
    const code = extractErrorCode(sanitized);
    super(`${command} failed: ${sanitized}`);
    this.name = "BackendCommandError";
    this.command = command;
    this.code = code;
    this.kind = classifyFailure(code);
  }
}

export type ParticipantSnapshot = {
  id: string;
  displayName: string;
  role: string;
  connected: boolean;
  mediaReady: boolean;
  cameraEnabled: boolean;
  microphoneEnabled: boolean;
  bufferAheadMs: number;
};

export type MediaManifest = {
  mediaId: string;
  filename: string;
  fileSize: number;
  container: string | null;
  fullHash: string;
  chunkSize: number;
  chunkCount: number;
};

export type TransferProgress = {
  mediaId: string;
  bytesAvailable: number;
  bytesTotal: number;
  bufferAheadMs: number;
  goodputBps: number;
};

export type ProviderMode = "PROVIDER_SYNC" | "PROVIDER_SHARED";

export type ProviderReadiness =
  | "NOT_STARTED"
  | "LAUNCHING"
  | "LOGIN_REQUIRED"
  | "READY"
  | "NAVIGATING"
  | "PLAYBACK_READY"
  | "UNAVAILABLE"
  | "ERROR";

export type ProviderSupportLevel = "SUPPORTED" | "PARTIAL" | "UNAVAILABLE";

export type ProviderTitleResolution = "DIRECT_URL" | "PROVIDER_SEARCH";

export type TailscaleState = "NOT_INSTALLED" | "SIGNED_OUT" | "CONNECTED" | "UNAVAILABLE";

export type TailscaleReadiness = {
  state: TailscaleState;
  code: string | null;
  ip: string | null;
  deviceName: string | null;
  message: string;
};

export type ProviderCapability = {
  id: string;
  displayName: string;
  supportLevel: ProviderSupportLevel;
  titleResolution: ProviderTitleResolution;
  syncAvailable: boolean;
  sharedAvailable: boolean;
  sharedReason: string;
  verification: "EXTERNAL_VERIFICATION_PENDING" | "VERIFIED";
};

export type AppSnapshot = {
  screen: string;
  room: {
    roomId: string | null;
    inviteCode: string | null;
    role: string;
    state: string;
    hostOnlyControls: boolean;
    sharedControls: boolean;
    strictSync: boolean;
  };
  participants: ParticipantSnapshot[];
  media: MediaManifest | null;
  transfer: TransferProgress | null;
  buffer: {
    guestBufferAheadMs: number;
    percent: number;
    bufferingParticipant: string | null;
  };
  sync: {
    roomState: string;
    strictSyncPaused: boolean;
    positionMs: number;
  };
  network: {
    transport: string;
    path: string;
    goodputBps: number;
    rttMs: number | null;
    connected: boolean;
  };
  call: {
    mode: CallMode;
    status: "connecting" | "connected" | "degraded" | "reconnecting" | "unavailable" | "ended";
    connected: boolean;
    camera: {
      enabled: boolean;
      width: number;
      height: number;
      fps: number;
      targetBitrateBps: number;
    };
    microphone: {
      enabled: boolean;
    };
  };
  callSignals: Array<{
    signalType: string;
    data: string;
    createdHostTimeUs: number;
  }>;
  provider: {
    mode: string;
    providerId: string | null;
    url: string | null;
    state: string;
    readiness: ProviderReadiness;
  };
  chat: Array<{
    id: string;
    sender: string;
    body: string;
    createdHostTimeUs: number;
  }>;
  reactions: Array<{
    id: string;
    sender: string;
    reaction: string;
    createdHostTimeUs: number;
  }>;
  ghostMode: boolean;
  privacyMode: boolean;
  lastRecovery: {
    event: RuntimeFailureEvent;
    action: string;
    pausesPlaybackForBoth: boolean;
    requiresUserAction: boolean;
  } | null;
  error: string | null;
  player: {
    state: string;
    positionMs: number;
    durationMs: number | null;
    volume: number;
    playbackRate: number;
    bufferedAheadMs: number | null;
    errorMessage: string | null;
    presentation: {
      mode: "EMBEDDED_NATIVE" | "EXTERNAL_NATIVE_WINDOW" | "UNAVAILABLE";
      platform: string;
      bridge: string;
      resizeManaged: boolean;
      ipcManaged: boolean;
      lifecycleManaged: boolean;
      message: string;
    };
  };
};

export type RuntimeFailureEvent =
  | "CHROME_CRASH"
  | "HOST_CRASH"
  | "GUEST_CRASH"
  | "TRANSFER_INTERRUPTED"
  | "TRANSFER_RESUMED"
  | "TAILSCALE_DISCONNECT"
  | "TAILSCALE_RECONNECT"
  | "NETWORK_CHANGE"
  | "WIFI_DISCONNECT"
  | "SLEEP_WAKE"
  | "PROVIDER_LOGOUT"
  | "PROVIDER_PAGE_CLOSED"
  | "PLAYER_FAILURE"
  | "CACHE_CORRUPTION"
  | "MISSING_LOCAL_FILE";

export async function getAppSnapshot(): Promise<AppSnapshot | null> {
  return invokeSnapshot("get_app_snapshot");
}

export async function getTailscaleReadiness(): Promise<TailscaleReadiness> {
  try {
    return await invoke<TailscaleReadiness>("get_tailscale_readiness");
  } catch {
    return {
      state: "UNAVAILABLE",
      code: "MP-NET-TS-003",
      ip: null,
      deviceName: null,
      message: "Tailscale status could not be checked. Make sure its service is running.",
    };
  }
}

export async function openTailscaleSetup(
  action: "INSTALL" | "SIGN_IN" | "PARTNER_HELP",
): Promise<boolean> {
  try {
    await invoke("open_tailscale_setup", { action });
    return true;
  } catch (error) {
    console.error("open_tailscale_setup failed", error);
    return false;
  }
}

export async function showHome(): Promise<AppSnapshot | null> {
  return invokeSnapshot("show_home");
}

export async function showJoinParty(): Promise<AppSnapshot | null> {
  return invokeSnapshot("show_join_party");
}

export async function requestEndParty(): Promise<AppSnapshot | null> {
  return invokeSnapshot("request_end_party");
}

export async function createLocalParty(mediaPath: string | null): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("create_local_party", {
    mediaPath: mediaPath && mediaPath.trim().length > 0 ? mediaPath : null,
  });
}

export async function pickMediaFile(): Promise<string | null> {
  try {
    return await invoke<string>("pick_media_file");
  } catch {
    return null;
  }
}

export async function getProviderCapabilities(): Promise<ProviderCapability[]> {
  try {
    return await invoke<ProviderCapability[]>("get_provider_capabilities");
  } catch (error) {
    console.error("get_provider_capabilities failed", error);
    return [];
  }
}

export async function launchProvider(
  providerId: string,
  url: string,
  mode: ProviderMode,
): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("launch_provider", { providerId, url, mode });
}

export async function openProviderBrowser(
  providerId: string,
): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("open_provider_browser", { providerId });
}

export async function checkProviderStatus(
  providerId: string,
): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("check_provider_status", { providerId });
}

export async function navigateProviderTitle(
  providerId: string,
  title: string,
): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("navigate_provider_title", {
    providerId,
    title,
  });
}

export async function launchGenericLink(url: string): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("launch_generic_link", { url });
}

export async function joinParty(inviteCode: string): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("join_party", { inviteCode });
}

export async function takePendingDeepLinks(): Promise<string[]> {
  try {
    return await invoke<string[]>("take_pending_deep_links");
  } catch (error) {
    console.error("take_pending_deep_links failed", error);
    return [];
  }
}

export async function listenToDeepLinks(
  onDeepLink: (url: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("deep_link_opened", (event) => {
    onDeepLink(event.payload);
  });
}

export async function markReady(): Promise<AppSnapshot | null> {
  return invokeSnapshot("mark_ready");
}

export async function enterCinema(): Promise<AppSnapshot | null> {
  return invokeSnapshot("enter_cinema");
}

export type NativeVideoBounds = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export async function attachNativeVideoSurface(
  bounds: NativeVideoBounds,
): Promise<AppSnapshot | null> {
  return invokeSnapshot("attach_native_video_surface", { bounds });
}

export async function resizeNativeVideoSurface(
  bounds: NativeVideoBounds,
): Promise<AppSnapshot | null> {
  return invokeSnapshot("resize_native_video_surface", { bounds });
}

export async function detachNativeVideoSurface(): Promise<void> {
  try {
    await invoke("detach_native_video_surface");
  } catch (error) {
    console.error("detach_native_video_surface failed", error);
  }
}

export async function pausePlayback(): Promise<AppSnapshot | null> {
  return invokeSnapshot("pause_playback");
}

export async function resumePlayback(): Promise<AppSnapshot | null> {
  return invokeSnapshot("resume_playback");
}

export async function seekRelative(deltaMs: number): Promise<AppSnapshot | null> {
  return invokeSnapshot("seek_relative", { deltaMs });
}

export async function handleFailureEvent(event: RuntimeFailureEvent): Promise<AppSnapshot | null> {
  return invokeSnapshot("handle_failure_event", { event });
}

export async function leaveParty(): Promise<AppSnapshot | null> {
  return invokeSnapshot("leave_party");
}

export async function sendChatMessage(body: string): Promise<AppSnapshot | null> {
  return invokeSnapshot("send_chat_message", { body });
}

export async function sendReaction(reaction: string): Promise<AppSnapshot | null> {
  return invokeSnapshot("send_reaction", { reaction });
}

export async function setSharedControls(enabled: boolean): Promise<AppSnapshot | null> {
  return invokeSnapshot("set_shared_controls", { enabled });
}

export async function reportBufferStatus(
  positionMs: number,
  bufferAheadMs: number,
  stalled: boolean,
): Promise<AppSnapshot | null> {
  return invokeSnapshot("report_buffer_status", {
    positionMs,
    bufferAheadMs,
    stalled,
  });
}

export async function setCallMode(mode: CallMode): Promise<AppSnapshot | null> {
  return invokeSnapshot("set_call_mode", { mode });
}

export async function submitCallSignal(signal: {
  signalType: "OFFER" | "ANSWER" | "ICE" | "RENEGOTIATE";
  data: string;
}): Promise<AppSnapshot | null> {
  return invokeSnapshot("submit_call_signal", { signal });
}

export async function setMicrophoneEnabled(enabled: boolean): Promise<AppSnapshot | null> {
  return invokeSnapshot("set_microphone_enabled", { enabled });
}

export async function setCameraEnabled(enabled: boolean): Promise<AppSnapshot | null> {
  return invokeSnapshot("set_camera_enabled", { enabled });
}

export async function setPrivacyMode(enabled: boolean): Promise<AppSnapshot | null> {
  return invokeSnapshot("set_privacy_mode", { enabled });
}

export async function setGhostMode(enabled: boolean): Promise<AppSnapshot | null> {
  return invokeSnapshot("set_ghost_mode", { enabled });
}

async function invokeSnapshot(
  command: string,
  args?: Record<string, unknown>,
): Promise<AppSnapshot | null> {
  try {
    return await invoke<AppSnapshot>(command, args);
  } catch (error) {
    const commandError = toBackendCommandError(command, error);
    console.error(commandError.message, commandError);
    return null;
  }
}

async function invokeSnapshotOrThrow(
  command: string,
  args?: Record<string, unknown>,
): Promise<AppSnapshot | null> {
  try {
    return await invoke<AppSnapshot>(command, args);
  } catch (error) {
    const commandError = toBackendCommandError(command, error);
    console.error(commandError.message, commandError);
    throw commandError;
  }
}

function toBackendCommandError(command: string, error: unknown): BackendCommandError {
  const detail =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : safeStringify(error);
  return new BackendCommandError(command, detail || "backend exception");
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function sanitizeErrorDetail(detail: string): string {
  return detail
    .replace(/([A-Za-z]:\\|\/Users\/|\/Volumes\/|\/private\/|\/var\/|\/tmp\/)[^\s"'`]+/g, "[path]")
    .trim();
}

function extractErrorCode(detail: string): string {
  return detail.match(/\bMP-[A-Z]+-\d{3}\b/)?.[0] ?? "MP-BACKEND-001";
}

function classifyFailure(code: string): BackendFailureKind {
  if (code.startsWith("MP-NET-")) {
    return "network failure";
  }
  if (code.startsWith("MP-SYNC-") || code.startsWith("MP-ROOM-") || code.startsWith("MP-CTRL-")) {
    return "invalid state";
  }
  return "backend failure";
}

export function commandErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof BackendCommandError) {
    return `${error.kind}: ${error.message}`;
  }

  if (error instanceof Error && error.message.trim().length > 0) {
    return sanitizeErrorDetail(error.message);
  }

  return fallback;
}

export async function initListener(): Promise<void> {
  await invoke("init_listener");
}

export async function listenToSnapshots(
  onSnapshot: (snapshot: AppSnapshot) => void,
): Promise<UnlistenFn> {
  await initListener();
  return listen<AppSnapshot>("app_snapshot_pushed", (e) => {
    onSnapshot(e.payload);
  });
}
