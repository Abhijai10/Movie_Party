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
    /** Once-per-event camera degradation notice (PRD §41). */
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

// ── Friends (saved movie partners) ────────────────────────────────────────

/** A selectable tailnet peer from `tailscale status`. */
export type FriendCandidate = {
  /** Full MagicDNS name — the stable identity key. */
  peerKey: string;
  displayName: string;
  ip: string | null;
  online: boolean;
  /** direct | peerRelay | derpRelay | unknown | offline */
  path: string;
};

/** A saved friend with its cached connection verification. */
export type StoredFriend = {
  peerKey: string;
  displayName: string;
  ip: string | null;
  addedAtMs: number;
  lastVerifiedAtMs: number | null;
  lastPath: string | null;
  lastLatencyMs: number | null;
  /**
   * Persisted stage of the Tailscale friend architecture:
   * INVITED (accepted from a link, device not yet in this tailnet) →
   * TAILSCALE_JOINED (the friend's device joined with their own
   * Tailscale identity and appears in the live status) →
   * MOVIE_PARTY_VERIFIED (a real tailscale ping answered).
   * ONLINE/OFFLINE is derived live, never persisted.
   */
  connectionState: FriendConnectionState;
};

/** The persisted friend-flow states (see StoredFriend.connectionState). */
export type FriendConnectionState = "INVITED" | "TAILSCALE_JOINED" | "MOVIE_PARTY_VERIFIED";

/** Result of a real `tailscale ping` verification through the tunnel. */
export type PeerConnectionProbe = {
  reachable: boolean;
  path: string | null;
  latencyMs: number | null;
  message: string;
};

/** tailnet_peers payload. */
export type TailnetPeersView = {
  candidates: FriendCandidate[];
  saved: StoredFriend[];
};

/** Candidate → friend shape (display copy for the UI).
 *
 * The friend architecture's explicit states:
 *   INVITED → TAILSCALE_PENDING → TAILSCALE_JOINED →
 *   MOVIE_PARTY_VERIFIED → ONLINE/OFFLINE.
 *
 * INVITED/TAILSCALE_PENDING/TAILSCALE_JOINED/MOVIE_PARTY_VERIFIED are
 * persisted stages of the invitation flow; ONLINE/OFFLINE is a live
 * observation derived from the current tailnet status. TAILSCALE_PENDING
 * is the transient reading of INVITED (the friend's device hasn't been
 * observed in the tailnet yet), surfaced as its own label so the UI can
 * show "waiting for their device to join" rather than a flat offline.
 */
export type FriendFlowStatus =
  | "INVITED"
  | "TAILSCALE_PENDING"
  | "MOVIE_PARTY_VERIFIED"
  | "ONLINE"
  | "OFFLINE";

export function friendStatusFor(
  friend: StoredFriend,
  candidateOnline: boolean | undefined,
): FriendFlowStatus {
  // Verification is sticky: a real ping through the tunnel outranks a
  // momentary offline reading of the peer table.
  if (friend.lastVerifiedAtMs != null) return "MOVIE_PARTY_VERIFIED";
  // INVITED friends show the pending state until the tailnet observes
  // their device (they must join through Tailscale's external-user
  // invitation with their own identity — never via an auth key in the
  // invite link).
  if (friend.connectionState === "INVITED") return "TAILSCALE_PENDING";
  if (candidateOnline != null) return candidateOnline ? "ONLINE" : "OFFLINE";
  return "OFFLINE";
}

/** Human copy for a friend's connection state. */
export function friendStatusCopy(friend: StoredFriend, status: FriendFlowStatus): string {
  if (status === "MOVIE_PARTY_VERIFIED") {
    const via = friend.lastPath ? ` · ${summarizePath(friend.lastPath)}` : "";
    const ms = friend.lastLatencyMs != null ? ` · ${String(friend.lastLatencyMs)} ms` : "";
    return `Connection verified${via}${ms}`;
  }
  if (status === "TAILSCALE_PENDING") {
    return "Invite accepted — they still need to join your network in the Tailscale app (separate step)";
  }
  if (status === "ONLINE") return "Online — not verified yet";
  return "Offline";
}

/** "direct/ipv4 192.168.1.5:41641" → "direct"; "relay \"derp-3\"" → "relay". */
export function summarizePath(path: string): string {
  const head = path.split(/[\/ ]/)[0] ?? path;
  return head === "relay-late" ? "relay" : head;
}

export async function tailnetPeers(): Promise<TailnetPeersView | null> {
  try {
    return await invoke<TailnetPeersView>("tailnet_peers");
  } catch {
    return null;
  }
}

export async function tailnetPeersOrThrow(): Promise<TailnetPeersView> {
  return invoke<TailnetPeersView>("tailnet_peers");
}

export async function addFriend(peerKey: string, displayName?: string): Promise<StoredFriend> {
  return invoke<StoredFriend>("add_friend", {
    peerKey,
    displayName: displayName ?? null,
  });
}

/** friend_invite_link payload — your shareable identity. */
export type FriendInviteLink = {
  /** movieparty://friend/… deep link to hand out. */
  link: string;
  /** Your tailnet peer key (carried inside the link payload). */
  peerKey: string;
  /** The display name carried in the link. */
  displayName: string;
};

/** Your friend-invite link — the QR/link friends scan to add you.
 * The link is identity-only: it never carries a Tailscale auth key or
 * any credential. The friend joins your tailnet through Tailscale's own
 * external-user invitation with THEIR identity; Movie Party observes
 * and verifies the result. */
export async function friendInviteLink(): Promise<FriendInviteLink> {
  return invoke<FriendInviteLink>("friend_invite_link");
}

/** Accept a movieparty://friend/ invite — the receiving side of the
 * friend architecture. Saves the inviter as INVITED (or
 * TAILSCALE_JOINED when their device is already in the live tailnet
 * status); verification happens later via verifyFriend. */
export async function acceptFriendInvite(inviteLink: string): Promise<StoredFriend> {
  return invoke<StoredFriend>("accept_friend_invite", { inviteLink });
}

/** Refresh saved friends against the live tailnet status: INVITED
 * friends whose device has since joined the tailnet are promoted to
 * TAILSCALE_JOINED; MOVIE_PARTY_VERIFIED is never demoted. */
export async function refreshFriendStates(): Promise<StoredFriend[]> {
  return invoke<StoredFriend[]>("refresh_friend_states");
}
/** Rename a saved friend — friendly names instead of tailnet jargon. */
export async function renameFriend(peerKey: string, displayName: string): Promise<StoredFriend> {
  return invoke<StoredFriend>("rename_friend", { peerKey, displayName });
}

export async function removeFriend(peerKey: string): Promise<void> {
  await invoke("remove_friend", { peerKey });
}

export async function listFriends(): Promise<StoredFriend[]> {
  return invoke<StoredFriend[]>("list_friends");
}

export async function verifyFriend(
  peerKey: string,
): Promise<{ friend: StoredFriend; probe: PeerConnectionProbe }> {
  return invoke<{ friend: StoredFriend; probe: PeerConnectionProbe }>("verify_friend", {
    peerKey,
  });
}

/** Human copy for a friend-flow error (stable MP codes → honest text). */
export function friendErrorCopy(error: unknown, fallback: string): string {
  const detail =
    typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
  if (detail.includes("MP-NET-TS-001")) {
    return "Tailscale isn't installed on this device. Install it, then check again.";
  }
  if (detail.includes("MP-NET-TS-003")) {
    return "Tailscale isn't responding right now. Open the Tailscale app, then check again.";
  }
  if (detail.includes("MP-NET-TS-004")) {
    return "That device has no usable Tailscale address.";
  }
  if (detail.includes("MP-NET-TS-007")) {
    return "No answer through the tunnel. Ask your friend to turn on their device and Tailscale, then verify again.";
  }
  if (detail.includes("MP-NET-TS-008")) {
    return "That device isn't in your Tailscale network. Ask them to join your tailnet, then check again.";
  }
  if (detail.includes("MP-STORE-001")) {
    return "Movie Party couldn't save that on this device.";
  }
  if (detail.includes("MP-FRIEND-001")) {
    return "That friend link or name isn't valid — check it and try again.";
  }
  return fallback;
}

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

/** Ready Check "Back to lobby": retract readiness, return to the lobby. */
export async function backToLobby(): Promise<AppSnapshot | null> {
  return invokeSnapshot("back_to_lobby");
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

/** the persisted per-provider Shared diagnostic record. */
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

/** run the 30 s Provider Shared capture diagnostic (ffmpeg-CLI
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

// ── settings, first-run, and scheduling surfaces ───────────

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
