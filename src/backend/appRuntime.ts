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

export type TailscaleState =
  | "NOT_INSTALLED"
  | "DAEMON_UNAVAILABLE"
  | "NEEDS_LOGIN"
  | "STOPPED"
  | "NO_USABLE_ADDRESS"
  | "READY";

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
    /** §25: backend-scheduled operation; the countdown animates from it. */
    pendingOperation: {
      kind: string;
      targetPositionMs: number;
      executeAtHostMonoUs: number;
      executeAtWallMs: number;
    } | null;
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
      tier: "A" | "B" | "C" | "D";
      width: number;
      height: number;
      fps: number;
      targetBitrateBps: number;
    };
    microphone: {
      enabled: boolean;
    };
    /** Once-per-event camera degradation notice (Batch 13 / PRD §41). */
    cameraNotice: string | null;
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
  /** §56: schedule awaiting the guest's explicit accept/decline. */
  pendingGuestSchedule: {
    scheduleId: string;
    mediaId: string;
    scheduledStartUtcMs: number;
  } | null;
  /** §52: post-party retention question for a guest's transferred movie. */
  retentionPrompt: {
    mediaId: string;
    filename: string;
  } | null;
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
      state: "DAEMON_UNAVAILABLE",
      code: "MP-NET-TS-003",
      ip: null,
      deviceName: null,
      message: "Tailscale status could not be checked. Make sure its service is running.",
    };
  }
}

export type TailscaleSetupAction = "INSTALL" | "OPEN_APP" | "PARTNER_HELP";

/** Returns `null` on success or a user-readable failure message. */
export async function openTailscaleSetup(action: TailscaleSetupAction): Promise<string | null> {
  try {
    await invoke("open_tailscale_setup", { action });
    return null;
  } catch (error) {
    console.error("open_tailscale_setup failed", error);
    return tailscaleSetupOpenErrorMessage(error);
  }
}

export function tailscaleSetupOpenErrorMessage(error: unknown): string {
  const detail =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : safeStringify(error);
  const sanitized = sanitizeErrorDetail(detail);
  const code = extractErrorCode(sanitized);

  if (code === "MP-NET-TS-001") {
    return "Movie Party could not find the Tailscale app on this device. Install Tailscale, then check again.";
  }
  if (code === "MP-NET-TS-003") {
    return "Movie Party could not open the Tailscale app. Open it from your Applications folder, then check again.";
  }
  return "Movie Party could not open Tailscale setup. If Tailscale is installed, open it yourself, then check again.";
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

export async function openProviderBrowser(providerId: string): Promise<AppSnapshot | null> {
  return invokeSnapshotOrThrow("open_provider_browser", { providerId });
}

export async function checkProviderStatus(providerId: string): Promise<AppSnapshot | null> {
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

/** §16 invitation: render the invite link as a scannable QR (SVG). */
export async function inviteQrSvg(inviteUrl: string): Promise<string | null> {
  try {
    return await invoke<string>("invite_qr_svg", { inviteUrl });
  } catch (error) {
    console.error("invite_qr_svg failed", error);
    return null;
  }
}

/** §52 retention decision: keep the cached movie on this device. */
export async function retentionKeep(mediaId: string): Promise<boolean> {
  try {
    await invoke("retention_keep", { mediaId });
    return true;
  } catch (error) {
    console.error("retention_keep failed", error);
    return false;
  }
}

/** §52 retention decision: remove Movie Party's cache (never the host
 *  source file). */
export async function retentionRemove(mediaId: string): Promise<boolean> {
  try {
    await invoke("retention_remove", { mediaId });
    return true;
  } catch (error) {
    console.error("retention_remove failed", error);
    return false;
  }
}

/** §52 retention decision: save the cached movie to a chosen folder. */
export async function retentionSaveAs(
  mediaId: string,
  destination: string,
): Promise<string | null> {
  try {
    return await invoke<string>("retention_save_as", { mediaId, destination });
  } catch (error) {
    console.error("retention_save_as failed", error);
    return null;
  }
}

export async function takePendingDeepLinks(): Promise<string[]> {
  try {
    return await invoke<string[]>("take_pending_deep_links");
  } catch (error) {
    console.error("take_pending_deep_links failed", error);
    return [];
  }
}

export async function listenToDeepLinks(onDeepLink: (url: string) => void): Promise<UnlistenFn> {
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


/** §25: host starts the backend-driven 3-2-1 countdown. The snapshot
 *  returns the pending operation whose deadline the UI animates from. */
export async function requestPlayCountdown(): Promise<AppSnapshot | null> {
  return invoke<AppSnapshot>("request_play_countdown");
}

/** §40: host-only Continue Without Guest — the explicit user override of
 *  the strict-sync guest gate. */
export async function continueWithoutGuest(): Promise<AppSnapshot | null> {
  return invoke<AppSnapshot>("continue_without_guest");
}

/** Batch 19 (D3-B): the persisted per-provider Shared diagnostic record. */
export type StoredProviderDiagnostic = {
  providerId: string;
  displayName: string;
  sharedAvailable: boolean;
  sharedReason: string;
  verifiedAtMs: number | null;
  sampleSeconds: number;
};

export async function listProviderDiagnostics(): Promise<
  StoredProviderDiagnostic[]
> {
  return invoke<StoredProviderDiagnostic[]>("list_provider_diagnostics");
}

/** Batch 19: run the 30 s Provider Shared capture diagnostic (ffmpeg-CLI
 *  bridge) and persist the empirical classification. */
export async function runProviderSharedDiagnostic(
  providerId: string,
): Promise<StoredProviderDiagnostic | null> {
  try {
    return await invoke<StoredProviderDiagnostic>("run_provider_shared_diagnostic", {
      providerId,
    });
  } catch (error) {
    console.error("run_provider_shared_diagnostic failed", error);
    return null;
  }
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

// ── Batch 15/16: settings, first-run, and scheduling surfaces ───────────

export type PrerequisiteStatus = {
  id: string;
  label: string;
  state: "OK" | "NOT_REQUESTED" | "MISSING" | "OPTIONAL";
  detail: string;
};

export type StoredSchedule = {
  scheduleId: string;
  roomId: string;
  mediaId: string;
  scheduledStartUtcMs: number;
  plannedPreloadUtcMs: number;
  guestDeviceId: string;
  status: string;
  createdAtMs: number;
};

export type AppMetadataInfo = {
  appName: string;
  protocolMajor: number;
  protocolMinor: number;
};

export async function getPrerequisiteStatuses(): Promise<PrerequisiteStatus[]> {
  try {
    return await invoke<PrerequisiteStatus[]>("get_prerequisite_statuses");
  } catch (error) {
    const commandError = toBackendCommandError("get_prerequisite_statuses", error);
    console.error(commandError.message, commandError);
    return [];
  }
}

export async function getAppMetadataInfo(): Promise<AppMetadataInfo | null> {
  try {
    return await invoke<AppMetadataInfo>("app_metadata");
  } catch (error) {
    const commandError = toBackendCommandError("app_metadata", error);
    console.error(commandError.message, commandError);
    return null;
  }
}

export async function listSchedules(): Promise<StoredSchedule[]> {
  try {
    return await invoke<StoredSchedule[]>("list_schedules");
  } catch (error) {
    const commandError = toBackendCommandError("list_schedules", error);
    console.error(commandError.message, commandError);
    return [];
  }
}

export async function createAndBroadcastSchedule(input: {
  roomId: string;
  mediaId: string;
  scheduledStartUtcMs: number;
  plannedPreloadUtcMs: number;
  guestDeviceId: string;
  callMode: string;
}): Promise<string | null> {
  try {
    return await invoke<string>("create_and_broadcast_schedule", input);
  } catch (error) {
    const commandError = toBackendCommandError("create_and_broadcast_schedule", error);
    console.error(commandError.message, commandError);
    return null;
  }
}

export async function cancelAndBroadcastSchedule(scheduleId: string): Promise<boolean> {
  try {
    await invoke("cancel_and_broadcast_schedule", { scheduleId });
    return true;
  } catch (error) {
    const commandError = toBackendCommandError("cancel_and_broadcast_schedule", error);
    console.error(commandError.message, commandError);
    return false;
  }
}

export async function guestAcceptSchedule(
  scheduleId: string,
  accepted: boolean,
): Promise<AppSnapshot | null> {
  return invokeSnapshot("guest_accept_schedule", { scheduleId, accepted });
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
  return detail.match(/\bMP-[A-Z]+(?:-[A-Z0-9]+)*-\d{3}\b/)?.[0] ?? "MP-BACKEND-001";
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
