import { useCallback, useEffect, useMemo, useState, type SyntheticEvent } from "react";
import { runLocalPeerConnectionLoopback } from "../call/webrtc";
import {
  nextPendingSignals,
  startRealCallSession,
  type LiveCallSession,
} from "../call/callSession";
import {
  pausePlayback,
  attachNativeVideoSurface,
  detachNativeVideoSurface,
  resumePlayback,
  resizeNativeVideoSurface,
  sendChatMessage,
  sendReaction as sendBackendReaction,
  seekRelative,
  setCameraEnabled,
  setGhostMode,
  setMicrophoneEnabled,
  setPrivacyMode,
  setSharedControls,
  submitCallSignal,
  type AppSnapshot,
} from "../backend/appRuntime";
import { CallTile } from "../overlays/CallTile";
import type { CallTileSessionState } from "../overlays/callTileState";
import { BufferingOverlay } from "../overlays/BufferingOverlay";
import { ProviderStatusOverlay } from "../overlays/ProviderStatusOverlay";
import { FloatingReactions, ReactionTray } from "../overlays/ReactionTray";
import { ReconnectOverlay, reconnectLatchAfter } from "../overlays/ReconnectOverlay";
import {
  backToLobby,
  continueWithoutGuest,
  createLocalParty,
  pickMediaFile,
} from "../backend/appRuntime";
import { ErrorScreenView, extractMpCode } from "./ErrorScreenView";
import { ChatCompose, ChatHistoryCard } from "../components/mp/ChatOverlay";
import {
  emptyBubbleQueue,
  enqueueChatBubble,
  type ChatBubbleQueue,
} from "../chat/bubbleQueue";
import { ChatBubbles } from "../overlays/ChatBubbles";
import { CinemaControls } from "../components/mp/CinemaControls";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import type { MouseEvent } from "react";
import { useRef } from "react";
import { useReducedMotion } from "../hooks/useReducedMotion";
import {
  canToggleChat,
  closedChatOverlay,
  isChatOverlayOpen,
  openChatManually,
  type ChatOverlayVisibility,
} from "../chat/overlayState";
import { captureGhostUiSnapshot, restoreGhostUiSnapshot, type GhostUiSnapshot } from "../social/ghostUiState";

type CinemaViewProps = {
  snapshot: AppSnapshot;
  onSnapshot: (snapshot: AppSnapshot) => void;
  onLeave: () => void;
  callTileSession: CallTileSessionState;
  onCallTileSessionChange: (next: CallTileSessionState) => void;
};

const chatBodyLimitBytes = 2_000;
const privacyNoticeMs = 2_200;
const ghostNoticeMs = 800;
/** camera-degradation notice duration (movie-first policy). */
const cameraNoticeMs = 3_200;
/** How long a failed control action's explanation stays on screen. */
const controlNoticeMs = 2_600;
/**
 * F30: bounded retry for a failed call start. One automatic retry covers the
 * transient cases (a device briefly busy, a dismissed-then-granted prompt)
 * without looping against a permanent one such as denied permission.
 */
const CALL_START_MAX_RETRIES = 1;
const CALL_START_RETRY_DELAY_MS = 1_500;
/**
 * The floating call/chat tile must never cover the control dock's final
 * action row (Leave / play / chat buttons at the bottom of the screen).
 * The dock occupies ~112px (progress + buttons + paddings); 132px adds
 * the dock's breathing room so a dropped tile always lands above it.
 */
const CINEMA_DOCK_RESERVED_BOTTOM_PX = 132;

/**
 * the production call path is the real cross-device session.
 * The loopback self-test stays available only as a dev diagnostic on
 * localhost behind an explicit opt-in flag (§30: unfinished/
 * diagnostic features behind flags), so default behavior never silently
 * falls back to a fake call (§29).
 */
function isCallLoopbackSelfTestEnabled(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  const isLocalDev =
    window.location.hostname === "localhost" || window.location.hostname === "127.0.0.1";
  if (!isLocalDev) {
    return false;
  }
  try {
    return window.localStorage.getItem("mp:call-loopback-selftest") === "1";
  } catch {
    return false;
  }
}

function formatMs(ms: number): string {
  const totalSeconds = Math.floor(ms / 1_000);
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${String(hours)}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }
  return `${String(minutes)}:${String(seconds).padStart(2, "0")}`;
}

export function CinemaView({
  snapshot,
  onSnapshot,
  onLeave,
  callTileSession,
  onCallTileSessionChange,
}: CinemaViewProps) {
  const [draft, setDraft] = useState("");
  const [chatVisibility, setChatVisibility] = useState<ChatOverlayVisibility>(closedChatOverlay);
  const [reactionWarning, setReactionWarning] = useState("");
  // §Reactions: the tray is CLOSED by default. The dock's emoji button
  // toggles it; an open tray auto-hides after 5s of no interaction so the
  // movie surface stays clean (Movie-first, PRD §1).
  const [reactionTrayOpen, setReactionTrayOpen] = useState(false);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [privacyNotice, setPrivacyNotice] = useState("");
  const [hasUnreadChat, setHasUnreadChat] = useState(false);
  // ADR-0003 / §35: lower-third ephemeral bubble queue.
  const [bubbleQueue, setBubbleQueue] = useState<ChatBubbleQueue>(emptyBubbleQueue);
  // §66: measured movie height drives the subtitle-zone bubble offset.
  const [movieHeightPx, setMovieHeightPx] = useState(0);
  // §40: Keep Waiting dismissal for the disconnect decision buttons.
  const [reconnectDismissed, setReconnectDismissed] = useState(false);
  const [cameraManuallyEnabled, setCameraManuallyEnabled] = useState(false);
  const [microphoneManuallyEnabled, setMicrophoneManuallyEnabled] = useState(false);
  const [callLocalStream, setCallLocalStream] = useState<MediaStream | null>(null);
  const [callRemoteStream, setCallRemoteStream] = useState<MediaStream | null>(null);
  /**
   * Mirrors `callSessionRef.current !== null` as state so the signal-cursor
   * effect re-runs when a held batch (signals that arrived while the
   * session was still starting) must flush the moment the session goes
   * live — callSignals alone would not change at that instant.
   */
  const [callSessionLive, setCallSessionLive] = useState(false);
  const prefersReducedMotion = useReducedMotion();
  const callSessionKey = useRef("");
  const callSessionRef = useRef<LiveCallSession | null>(null);
  const callSignalCursorRef = useRef(0);
  /**
   * While true, the signal-cursor effect holds: a session is starting and
   * signals arriving in the interim (e.g. the host's OFFER while the
   * guest's session is still acquiring media) must land on the fresh
   * session rather than being skipped as stale.
   */
  const callSessionStartingRef = useRef(false);
  /**
   * F30: a transient start failure must not permanently disable the call.
   *
   * `callSessionKey` gates the start effect (`=== nextKey` returns early), and
   * the failure path used to leave it set — so one getUserMedia/start error
   * disabled the call for that (mode, role, privacy) combination until
   * something else happened to change the key, with no retry and no way out.
   * The key is now cleared on failure and ONE bounded retry is scheduled, so
   * recovery is deterministic and cannot loop against a permanent failure
   * (e.g. permission denied).
   */
  const [callRetryNonce, setCallRetryNonce] = useState(0);
  const callRetryCountRef = useRef(0);
  const controlsTimer = useRef<number | null>(null);
  const previousChatLength = useRef(snapshot.chat.length);
  const movieFrameRef = useRef<HTMLDivElement | null>(null);
  const ghostUiSnapshotRef = useRef<GhostUiSnapshot | null>(null);
  const isGhostMode = snapshot.ghostMode;
  const isPrivacyMode = snapshot.privacyMode;
  const isHost = snapshot.room.role === "HOST";
  const sharedControls = snapshot.room.sharedControls;
  const snapshotCameraEnabled = snapshot.call.camera.enabled;
  const snapshotMicrophoneEnabled = snapshot.call.microphone.enabled;
  const localCameraEnabled = snapshotCameraEnabled && cameraManuallyEnabled;
  const localMicrophoneEnabled = snapshotMicrophoneEnabled && microphoneManuallyEnabled;
  const callMode = snapshot.call.mode;
  const movieTitle = snapshot.media?.filename ?? snapshot.provider.url ?? "Movie";
  const localMediaId = snapshot.media?.mediaId;
  const playerHasFailed = snapshot.player.state === "PLAYER_ERROR";
  const playerIsPreparing =
    playerHasFailed ||
    snapshot.player.presentation.mode === "UNAVAILABLE" ||
    snapshot.player.state === "STOPPED";
  const peer = snapshot.participants.find((participant) => participant.role !== snapshot.room.role);
  const peerName = peer?.displayName ?? "Peer";
  const isComposing = chatVisibility.manualOpen;
  const isHistoryOpen = isChatOverlayOpen(chatVisibility);
  // The history panel renders unless its 5 s auto-hide has dismissed it
  // (#2) — the compose bar is unaffected and stays for typing.
  const isHistoryVisible = isHistoryOpen && !chatVisibility.historyDismissed;

  // The chat toggle ("c" and the dock button): open chat, or — when chat
  // is already open — re-summon an auto-hidden history panel before
  // closing anything (so a 5 s auto-hide is not a dead end).
  const toggleChatVisibility = useCallback(
    (current: ChatOverlayVisibility): ChatOverlayVisibility => {
      if (!current.manualOpen) {
        return openChatManually();
      }
      return current.historyDismissed
        ? { ...current, historyDismissed: false }
        : closedChatOverlay;
    },
    [],
  );

  const encodedDraftLength = useMemo(() => new TextEncoder().encode(draft).length, [draft]);
  const isDraftTooLong = encodedDraftLength > chatBodyLimitBytes;

  useEffect(() => {
    if (snapshot.provider.mode !== "LOCAL_PERFECT" || localMediaId == null) {
      return;
    }
    const frame = movieFrameRef.current;
    if (!frame) {
      return;
    }
    let attached = false;
    const updateSurface = () => {
      const rect = frame.getBoundingClientRect();
      setMovieHeightPx(rect.height);
      const bounds = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
      const request = attached ? resizeNativeVideoSurface(bounds) : attachNativeVideoSurface(bounds);
      attached = true;
      void request
        .then(onSnapshot)
        .catch((error: unknown) => {
          // A failed surface update is retried on the next observer tick, so it
          // is logged rather than surfaced — but it is no longer silently
          // treated as "nothing happened".
          console.error("native video surface update failed", error);
        });
    };
    updateSurface();
    const observer = new ResizeObserver(updateSurface);
    observer.observe(frame);
    return () => {
      observer.disconnect();
      // The detach resets the player's mpv contexts (the media is
      // remembered); the returned snapshot keeps AppShell in sync so a
      // later re-attach — StrictMode remount, re-entering Cinema — is a
      // clean handoff instead of "cannot move to another native surface".
      void detachNativeVideoSurface()
        .then(onSnapshot)
        .catch((error: unknown) => {
          // Best-effort cleanup on unmount: the surface is going away either
          // way, so this is logged rather than surfaced.
          console.error("detach_native_video_surface failed", error);
        });
    };
  }, [localMediaId, onSnapshot, snapshot.provider.mode]);

  useEffect(() => {
    if (isComposing || isHistoryOpen || isPrivacyMode) {
      setControlsVisible(true);
      return;
    }
    if (!controlsVisible) {
      return;
    }

    controlsTimer.current = window.setTimeout(() => {
      setControlsVisible(false);
    }, 3_200);

    return () => {
      if (controlsTimer.current != null) {
        window.clearTimeout(controlsTimer.current);
        controlsTimer.current = null;
      }
    };
  }, [isComposing, isHistoryOpen, isPrivacyMode, controlsVisible]);

  useEffect(() => {
    const nextLength = snapshot.chat.length;
    const hasNewMessage = nextLength > previousChatLength.current;
    previousChatLength.current = nextLength;

    if (!hasNewMessage || chatVisibility.manualOpen) {
      return;
    }

    // While social UI is suppressed (Ghost/Privacy), a message never
    // reveals the overlay — the unread flag alone survives the mode, so
    // the chat button badge tells the user once it ends.
    if (isGhostMode || isPrivacyMode) {
      setHasUnreadChat(true);
      return;
    }

    // §35: the message renders as a lower-third ephemeral bubble. The
    // unread badge persists until the user opens the history manually.
    setHasUnreadChat(true);
    setBubbleQueue((current) =>
      enqueueChatBubble(
        current,
        snapshot.chat[snapshot.chat.length - 1] ?? { id: "unknown", sender: "Peer", body: "" },
        Date.now(),
      ),
    );
  }, [chatVisibility.manualOpen, isGhostMode, isPrivacyMode, snapshot.chat.length, snapshot.chat]);

  // Ghost/Privacy entry-exit bookkeeping. The chat visibility is captured
  // as it was *before* the mode hid it, so ending the mode can put the
  // prior UI state back. The call tile lives in AppShell session state
  // and is only visually hidden by the `social-hidden` class, so its
  // position/visibility needs no manual restore. Devices are governed by
  // the backend: Ghost never touches them; Privacy disables both and
  // never re-enables on exit.
  const isSocialHiddenMode = isGhostMode || isPrivacyMode;
  useEffect(() => {
    if (isSocialHiddenMode) {
      if (ghostUiSnapshotRef.current == null) {
        ghostUiSnapshotRef.current = captureGhostUiSnapshot(chatVisibility);
      }
      // Hide the chat immediately (the CSS class hides everything else);
      // lingering bubbles are pruned by their own lifetimes.
      setChatVisibility(closedChatOverlay);
      return;
    }

    const snapshotToRestore = ghostUiSnapshotRef.current;
    ghostUiSnapshotRef.current = null;
    if (snapshotToRestore == null) {
      return;
    }
    setChatVisibility(restoreGhostUiSnapshot(snapshotToRestore));
    // `chatVisibility` and the timer refs are read only on the mode
    // transition itself; re-running on every chat keystroke would
    // re-capture mid-mode state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isSocialHiddenMode]);

  // §Reactions: an open tray auto-hides after 5s so the movie stays
  // visually dominant, and Ghost/Privacy close it outright — the modes
  // hide every social affordance (PRD §4), and the floating reactions
  // layer is already suppressed by `social-hidden`.
  useEffect(() => {
    if (!reactionTrayOpen) {
      return undefined;
    }
    if (isSocialHiddenMode) {
      setReactionTrayOpen(false);
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setReactionTrayOpen(false);
    }, 5_000);
    return () => {
      window.clearTimeout(timer);
    };
  }, [reactionTrayOpen, isSocialHiddenMode]);

  const revealControls = (event?: MouseEvent<HTMLElement>) => {
    setControlsVisible(true);
    if (event && !prefersReducedMotion) {
      const bounds = event.currentTarget.getBoundingClientRect();
      event.currentTarget.style.setProperty(
        "--cinema-pointer-x",
        `${String(event.clientX - bounds.left)}px`,
      );
      event.currentTarget.style.setProperty(
        "--cinema-pointer-y",
        `${String(event.clientY - bounds.top)}px`,
      );
    }
  };

  // ── real cross-device call session ───────────────────────────
  //
  // Lifecycle effect: starts ONE RTCPeerConnection per call configuration.
  // Host offers; guest answers. The key deliberately EXCLUDES camera/mic
  // toggles — PRD §41 keeps those local-only (track.enabled, no
  // renegotiation). Mode change or privacy tears the session down; a
  // restart asks the offerer for a fresh exchange via a RENEGOTIATE
  // marker (the guest may restart without the host touching anything,
  // so "guest always answers" cannot alone cover restarts).
  //
  // Media effects after start (acquire, publish OFFER / RENEGOTIATE) go
  // through the session module; this effect only owns create/teardown.
  useEffect(() => {
    const nextKey = `${callMode}:${isHost ? "HOST" : "GUEST"}:${String(isPrivacyMode)}`;
    // A call session needs someone to call. Solo preview (no peer in the
    // room yet) must not acquire camera/mic or publish an offer — the
    // acquisition failure would surface a scary "Call unavailable" toast
    // over a preview that is about the MOVIE, not the call. The CallTile
    // keeps its honest "Peer / Away" placeholder; the real session starts
    // the moment the peer actually joins (participants gains the peer).
    if (callMode === "OFF" || isPrivacyMode || peer == null) {
      callSessionKey.current = "";
      callSessionRef.current?.close();
      callSessionRef.current = null;
      callSessionStartingRef.current = false;
      setCallSessionLive(false);
      setCallLocalStream(null);
      setCallRemoteStream(null);
      return;
    }
    if (callSessionKey.current === nextKey) {
      return;
    }

    callSessionKey.current = nextKey;
    callSessionRef.current?.close();
    callSessionRef.current = null;
    setCallSessionLive(false);
    setCallLocalStream(null);
    setCallRemoteStream(null);
    // Hold the signal cursor from the current list length: everything
    // already in the list belongs to the previous exchange. Signals
    // arriving while THIS session is starting must apply to it, so the
    // cursor effect must not advance past this base until the session is
    // live.
    callSignalCursorRef.current = snapshot.callSignals.length;
    callSessionStartingRef.current = true;

    if (isCallLoopbackSelfTestEnabled()) {
      // Dev diagnostic path (localhost + explicit opt-in flag): the legacy
      // two-connection loopback self-test, unchanged. No live session
      // exists, so release the startup hold immediately.
      callSessionStartingRef.current = false;
      const controller = new AbortController();
      void runLocalPeerConnectionLoopback(
        callMode,
        localCameraEnabled,
        localMicrophoneEnabled,
        async (signal) => {
          const next = await submitCallSignal(signal);
          onSnapshot(next);
        },
        controller.signal,
      ).then((result) => {
        if (controller.signal.aborted) {
          return;
        }
        if (!result.connected) {
          setPrivacyNotice(
            result.status === "unavailable"
              ? `Call unavailable${result.errorCode ? ` (${result.errorCode})` : ""}.`
              : "Call degraded. Movie playback remains prioritized.",
          );
        }
      });
      return () => {
        controller.abort();
      };
    }

    let cancelled = false;
    // F30: the single scheduled retry timer, cleared with the effect.
    let retryTimer: number | null = null;
    void startRealCallSession(
      isHost ? "HOST" : "GUEST",
      callMode,
      localCameraEnabled,
      localMicrophoneEnabled,
      {
        onSignal: async (signal) => {
          const next = await submitCallSignal(signal);
          onSnapshot(next);
        },
        onStatusChange: (status) => {
          if (status === "degraded") {
            setPrivacyNotice("Call degraded. Movie playback remains prioritized.");
          }
        },
      },
    )
      .then((session) => {
        if (cancelled) {
          session.close();
          return;
        }
        callSessionRef.current = session;
        // A successful start clears the retry budget, so a later genuine
        // failure (F30) can retry again rather than being refused for good.
        callRetryCountRef.current = 0;
        setCallSessionLive(true);
        // Startup is complete: the next signal-cursor run may advance
        // freely; entries at/before the base were skipped on purpose.
        callSessionStartingRef.current = false;
        setCallLocalStream(session.localStream);
        setCallRemoteStream(session.remoteStream);
        if (!session.usedRealMedia && session.mediaErrorCode) {
          setPrivacyNotice(
            session.mediaErrorCode === "MP-CALL-011"
              ? "Camera/microphone permission denied. Voice/video call is off; movie playback is unaffected."
              : `Call unavailable (${session.mediaErrorCode}). Movie playback remains prioritized.`,
          );
        }
      })
      .catch((error: unknown) => {
        if (cancelled) {
          return;
        }
        callSessionStartingRef.current = false;
        setCallSessionLive(false);
        // F30: clear the key so this attempt is no longer treated as "already
        // tried", then retry once. Any duplicate attempt is prevented by the
        // effect's own teardown (it closes the previous session and clears the
        // refs before starting), so a retry cannot stack connections or tracks.
        callSessionKey.current = "";
        if (callRetryCountRef.current < CALL_START_MAX_RETRIES) {
          callRetryCountRef.current += 1;
          retryTimer = window.setTimeout(() => {
            setCallRetryNonce((nonce) => nonce + 1);
          }, CALL_START_RETRY_DELAY_MS);
        }
        const errorCode =
          typeof error === "object" && error !== null && "errorCode" in error
            ? String(error.errorCode)
            : "MP-CALL-015";
        setPrivacyNotice(`Call unavailable (${errorCode}). Movie playback remains prioritized.`);
      });

    return () => {
      cancelled = true;
      callSessionStartingRef.current = false;
      if (retryTimer != null) {
        window.clearTimeout(retryTimer);
      }
      setCallSessionLive(false);
      callSessionRef.current?.close();
      callSessionRef.current = null;
    };
    // onSnapshot is a stable AppShell callback; the deps are the session
    // identity. localCameraEnabled/localMicrophoneEnabled are read at
    // session start (initial track.enabled), then toggles stay local.
    // callRetryNonce is F30's single scheduled retry.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [callMode, isHost, isPrivacyMode, peer != null, callRetryNonce]);

  // Signal cursor effect: applies peer-originated signals from the
  // snapshot to the live session. The cursor is snapshot-index-based —
  // never timestamps (submit uses host_time_us, receive uses
  // quic::monotonic_us — mixed clock domains) and never string equality
  // (a renegotiation legitimately repeats a signal type). The host
  // appends monotonically; a SHRINK means set_call_mode cleared the list
  // and the cursor resets with it.
  useEffect(() => {
    const cursor = callSignalCursorRef.current;
    const signals = snapshot.callSignals;
    const session = callSessionRef.current;

    if (signals.length < cursor) {
      // The backend cleared the list (mode change): start over.
      callSignalCursorRef.current = 0;
      return;
    }
    if (!session) {
      // No live session (off/privacy/still starting): keep the cursor
      // pinned to the list so stale signals never replay into a future
      // session — EXCEPT hold during session startup, when the cursor
      // must not advance past signals the fresh session needs (the
      // lifecycle effect pinned it at session start).
      if (callSessionStartingRef.current) {
        return;
      }
      callSignalCursorRef.current = signals.length;
      return;
    }

    const { pending, nextCursor } = nextPendingSignals(
      signals,
      cursor,
      isHost ? "HOST" : "GUEST",
      { ownMarkerId: session.markerId },
    );
    callSignalCursorRef.current = nextCursor;
    if (pending.length > 0) {
      // Apply the coalesced batch SEQUENTIALLY: a marker poke must land on
      // the session before a later OFFER it triggered, and the guest's
      // answer to an OFFER must not race the ICE batch that follows it.
      const applySequentially = async () => {
        for (const signal of pending) {
          try {
            await session.applyRemoteSignal(signal);
          } catch {
            setPrivacyNotice("Call degraded. Movie playback remains prioritized.");
          }
        }
      };
      void applySequentially();
    }
    // callSessionLive re-triggers the held-batch flush the moment a
    // starting session goes live (callSignals alone would not change).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snapshot.callSignals, callSessionLive]);

  // Local media toggle sync (PRD §41): camera/mic toggles are LOCAL-ONLY
  // track.enabled flips — they never renegotiate the wire. The session
  // reads the enabled state at start (initial track.enabled in
  // acquireRealCallMedia); every later change must be pushed to the live
  // session's tracks or the remote peer keeps receiving stale media (the
  // backend state update only drives the remote INDICATOR, not media).
  // callSessionLive re-runs this at session start so the fresh session
  // immediately reflects any toggle made while it was starting.
  useEffect(() => {
    callSessionRef.current?.setLocalTracksEnabled(
      localCameraEnabled,
      localMicrophoneEnabled,
    );
  }, [localCameraEnabled, localMicrophoneEnabled, callSessionLive]);

  // (PRD §41 movie-first): the Rust ladder rewrites the camera
  // tier under network pressure; apply it to the LIVE session's sender
  // (encoder caps + track constraints — no SDP renegotiation). Keyed on
  // the full tier parameter set so re-renders with an unchanged tier
  // never re-apply constraints.
  const cameraTierParams = [
    snapshot.call.camera.tier,
    snapshot.call.camera.width,
    snapshot.call.camera.height,
    snapshot.call.camera.fps,
    snapshot.call.camera.targetBitrateBps,
  ].join(":");
  useEffect(() => {
    if (!callSessionLive) {
      return;
    }
    void (async () => {
      await callSessionRef.current?.applyCameraTier(snapshot.call.camera);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cameraTierParams, callSessionLive]);

  // once-per-event degradation notice from the backend ladder
  // ("Camera quality reduced to protect movie playback"). Displayed via
  // the existing cinematic notice surface; auto-clears like other
  // transient notices. The backend clears it on upgrade; this side only
  // shows what the snapshot carries.
  useEffect(() => {
    if (snapshot.call.cameraNotice) {
      setPrivacyNotice(snapshot.call.cameraNotice);
      const timer = window.setTimeout(() => {
        setPrivacyNotice("");
      }, cameraNoticeMs);
      return () => {
        window.clearTimeout(timer);
      };
    }
    return undefined;
  }, [snapshot.call.cameraNotice]);

  /**
   * Surfaces a failed control action through the existing cinematic notice
   * surface, so a button that could not do its job says so instead of looking
   * like it did nothing. Auto-clears like the other transient notices, and only
   * if the same message is still showing (a newer notice is left alone).
   */
  const showControlNotice = (message: string) => {
    setPrivacyNotice(message);
    window.setTimeout(() => {
      setPrivacyNotice((current) => (current === message ? "" : current));
    }, controlNoticeMs);
  };

  useEffect(() => {
    let noticeTimeout: number | undefined;
    const showNotice = (message: string, durationMs: number) => {
      setPrivacyNotice(message);
      window.clearTimeout(noticeTimeout);
      noticeTimeout = window.setTimeout(() => {
        setPrivacyNotice("");
      }, durationMs);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      const isTextInput =
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement;

      if (event.key === "Escape") {
        setChatVisibility(closedChatOverlay);
        return;
      }

      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "m") {
        event.preventDefault();
        if (isPrivacyMode) {
          showNotice("Privacy Mode is active", ghostNoticeMs);
          return;
        }

        void setGhostMode(!isGhostMode).then((next) => {
          onSnapshot(next);
          showNotice(!isGhostMode ? "Ghost Mode on" : "Ghost Mode off", ghostNoticeMs);
        });
        return;
      }

      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        void setPrivacyMode(!isPrivacyMode).then((next) => {
          onSnapshot(next);
          if (isPrivacyMode) {
            showNotice(
              "Privacy Mode ended. Camera and microphone remain disabled.",
              privacyNoticeMs,
            );
            return;
          }

          showNotice("Privacy Mode on", ghostNoticeMs);
        });
        return;
      }

      if (isTextInput) {
        return;
      }

      // Enter / "c" toggle chat. While Ghost or Privacy Mode hides the
      // social layer, the toggle must not reveal it.
      if (!canToggleChat(isGhostMode || isPrivacyMode)) {
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        setChatVisibility(openChatManually());
      }

      if (event.key.toLowerCase() === "c") {
        event.preventDefault();
        setChatVisibility(toggleChatVisibility);
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.clearTimeout(noticeTimeout);
    };
  }, [isGhostMode, isPrivacyMode, onSnapshot, toggleChatVisibility]);

  const sendMessage = (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (draft.trim().length === 0 || isDraftTooLong) {
      return;
    }

    const sentBody = draft.trim();
    void sendChatMessage(sentBody).then((next) => {
      previousChatLength.current = next.chat.length;
      onSnapshot(next);
      setDraft("");
      // §34/§35: the sender must SEE where the message went. The
      // incoming-message effect skips while the compose bar is open
      // (manualOpen), so the sender's own words would otherwise vanish
      // until they opened history. Echo the sent message as a lower-third
      // ephemeral bubble — same surface the peer's messages use.
      const lastMessage = next.chat[next.chat.length - 1];
      if (lastMessage != null && lastMessage.body === sentBody) {
        setBubbleQueue((current) => enqueueChatBubble(current, lastMessage, Date.now()));
      }
    });
  };

  const sendReaction = (reaction: string) => {
    if (isGhostMode) {
      return;
    }

    setReactionWarning("");
    void sendBackendReaction(reaction).then(
      (next) => {
        onSnapshot(next);
      },
      () => {
        // The backend drops a reaction over the rate limit; that rejection is
        // still the "limit reached" signal (the old `if (next)` branch never
        // fired, because a failed command resolved to null only on IPC failure).
        setReactionWarning("Reaction limit reached");
      },
    );
  };

  // F7: the reconnect-dismissal latch is scoped to one disconnect episode.
  // It used to be cleared only by "Continue without guest", so once dismissed
  // it stayed dismissed and every later disconnect showed no overlay at all.
  // Clearing it the moment the room leaves RECONNECTING gives the next
  // episode its own overlay, while holding it mid-episode prevents duplicates.
  useEffect(() => {
    setReconnectDismissed((current) =>
      reconnectLatchAfter(snapshot.sync.roomState, current),
    );
  }, [snapshot.sync.roomState]);

  return (
    <main className="cinema-shell" aria-label="Cinema mode">
      <SilkBackground variant="dim" />
      <section
        className={[
          "movie-surface",
          isGhostMode || isPrivacyMode ? "social-hidden" : "",
          controlsVisible ? "controls-visible" : "controls-idle",
        ]
          .filter(Boolean)
          .join(" ")}
        aria-label="Movie"
        onMouseMove={revealControls}
        onFocus={() => {
          revealControls();
        }}
      >
        <div className="cinema-dot-grid" aria-hidden="true" />
        <div
          ref={movieFrameRef}
          className={`movie-frame ${snapshot.player.presentation.mode === "EMBEDDED_NATIVE" ? "native-video-active" : ""}`}
        >
          <div className="movie-light" />
          <div className="movie-prep-state" aria-live="polite">
            <span className="movie-prep-kicker">
              {playerHasFailed
                ? "Playback unavailable"
                : playerIsPreparing
                  ? "Preparing playback"
                  : "Now screening"}
            </span>
            <h1>{movieTitle}</h1>
            {playerHasFailed ? (
              <p>The local player reported an error. Playback is paused for the room.</p>
            ) : playerIsPreparing ? (
              <p>Setting up the local player and synchronizing the room.</p>
            ) : null}
          </div>
          <span className="player-position">
            {formatMs(snapshot.player.positionMs)}
            {snapshot.player.durationMs != null ? ` / ${formatMs(snapshot.player.durationMs)}` : ""}
            {snapshot.player.state === "BUFFERING" && " (Buffering...)"}
            {snapshot.player.errorMessage && (
              <span className="player-error">{snapshot.player.errorMessage}</span>
            )}
          </span>
        </div>
        <div className="sync-indicator" aria-live="polite">
          <span aria-hidden="true" />
          {snapshot.sync.roomState === "PLAYING" ? (
            <StatusIndicator state="sync" label="In sync" showLabel={false} />
          ) : (
            <StatusIndicator state="waiting" label="Syncing" showLabel={false} />
          )}
        </div>
        <CallTile
          peerName={peerName}
          remoteCameraEnabled={peer?.cameraEnabled ?? false}
          remoteMicrophoneEnabled={peer?.microphoneEnabled ?? false}
          remoteConnected={peer?.connected ?? false}
          remoteStream={callRemoteStream}
          localStream={callLocalStream}
          session={callTileSession}
          onSessionChange={onCallTileSessionChange}
          reservedBottomPx={CINEMA_DOCK_RESERVED_BOTTOM_PX}
        />
        <ChatBubbles
          queue={bubbleQueue}
          movieHeightPx={movieHeightPx}
          // §28: subtitle presence is not on the authoritative snapshot
          // yet; false keeps the lower-third default until it is.
          subtitlesActive={false}
          onQueueChange={setBubbleQueue}
        />
        {isComposing ? (
          <ChatCompose
            draft={draft}
            isDraftTooLong={isDraftTooLong}
            onDraftChange={setDraft}
            onSubmit={sendMessage}
            onClose={() => {
              setChatVisibility((current) => ({ ...current, manualOpen: false }));
            }}
          />
        ) : null}
        {isHistoryVisible ? (
          <ChatHistoryCard
            snapshot={snapshot}
            onClose={() => {
              // The panel's X and its 5 s auto-hide both dismiss ONLY
              // the history — the composer stays open for typing.
              setChatVisibility((current) => ({
                ...current,
                historyDismissed: true,
              }));
            }}
          />
        ) : null}
        <FloatingReactions reactions={snapshot.reactions.slice(-4)} />
        <BufferingOverlay
          bufferingParticipant={snapshot.buffer.bufferingParticipant}
          peerName={peerName}
          percent={snapshot.buffer.percent}
          strictSyncPaused={snapshot.sync.strictSyncPaused}
        />
        {snapshot.error != null &&
        snapshot.error.startsWith("MP-MEDIA-002") ? (
          <ErrorScreenView
            code={extractMpCode(snapshot.error)}
            message="The movie file moved or was renamed on this device."
            actions={[
              {
                label: "Locate the file",
                onClick: () => {
                  void pickMediaFile().then((path) => {
                    if (path == null) {
                      return;
                    }
                    void createLocalParty(path)
                      .then(onSnapshot)
                      .catch((error: unknown) => {
                        console.error("create_local_party failed", error);
                        showControlNotice("Movie Party could not start that party.");
                      });
                  });
                },
              },
            ]}
            technicalDetails={snapshot.error}
            onDismiss={() => {
              // F14: the button reads "Back to lobby", so it must actually go
              // back to the lobby. It used to call `onLeave`, which raised the
              // Leave / End-for-everyone confirmation — a destructive action
              // behind a non-destructive label. `back_to_lobby` is the same
              // retreat the Ready Check uses: it retracts readiness, clears any
              // pending countdown and returns the room to the lobby without
              // ending the party.
              void backToLobby()
                .then(onSnapshot)
                .catch((error: unknown) => {
                  console.error("back_to_lobby failed", error);
                  showControlNotice("Movie Party could not return to the lobby.");
                });
            }}
          />
        ) : null}
        <ReconnectOverlay
          peerName={peerName}
          isReconnecting={
            snapshot.sync.roomState === "RECONNECTING" && !reconnectDismissed
          }
          isHost={snapshot.room.role === "HOST"}
          onKeepWaiting={() => {
            setReconnectDismissed(true);
          }}
          onContinueWithoutGuest={() => {
            void continueWithoutGuest()
              .then((next) => {
                setReconnectDismissed(false);
                onSnapshot(next);
              })
              .catch((error: unknown) => {
                console.error("continue_without_guest failed", error);
                showControlNotice("Movie Party could not resume without your guest.");
              });
          }}
        />
        <ProviderStatusOverlay snapshot={snapshot} />
        {privacyNotice ? (
          <div className="privacy-toast" role="status" aria-live="polite">
            {privacyNotice}
          </div>
        ) : null}
        {reactionTrayOpen ? (
          <ReactionTray warning={reactionWarning} onSendReaction={sendReaction} />
        ) : null}
        <CinemaControls
          visible={controlsVisible}
          isPlaying={snapshot.sync.roomState === "PLAYING"}
          currentMs={snapshot.player.positionMs}
          durationMs={snapshot.player.durationMs}
          onSeekRelative={(deltaMs) => {
            void seekRelative(deltaMs)
              .then(onSnapshot)
              .catch((error: unknown) => {
                console.error("seek failed", error);
                showControlNotice("Movie Party could not seek.");
              });
          }}
          onTogglePlayback={() => {
            const action = snapshot.sync.roomState === "PLAYING" ? pausePlayback : resumePlayback;
            void action()
              .then(onSnapshot)
              .catch((error: unknown) => {
                console.error("playback toggle failed", error);
                showControlNotice("Movie Party could not change playback.");
              });
          }}
          microphoneEnabled={localMicrophoneEnabled}
          onToggleMicrophone={() => {
            const nextEnabled = !localMicrophoneEnabled;
            setMicrophoneManuallyEnabled(nextEnabled);
            if (snapshotMicrophoneEnabled === nextEnabled) {
              return;
            }
            void setMicrophoneEnabled(nextEnabled)
              .then(onSnapshot)
              .catch((error: unknown) => {
                console.error("set_microphone_enabled failed", error);
                showControlNotice("Movie Party could not change the microphone.");
              });
          }}
          cameraEnabled={localCameraEnabled}
          onToggleCamera={() => {
            const nextEnabled = !localCameraEnabled;
            setCameraManuallyEnabled(nextEnabled);
            if (snapshotCameraEnabled === nextEnabled) {
              return;
            }
            void setCameraEnabled(nextEnabled)
              .then(onSnapshot)
              .catch((error: unknown) => {
                console.error("set_camera_enabled failed", error);
                showControlNotice("Movie Party could not change the camera.");
              });
          }}
          chatOpen={isComposing || isHistoryOpen}
          hasUnreadChat={hasUnreadChat}
          onToggleChat={() => {
            // If the composer is up but the history panel has
            // auto-hidden, re-summon the panel instead of closing chat.
            if (isComposing) {
              setChatVisibility(toggleChatVisibility);
              return;
            }
            setHasUnreadChat(false);
            setChatVisibility(openChatManually());
          }}
          onSendReaction={() => {
            // The dock emoji TOGGLES the tray (it no longer fires a
            // blind 👏): open → pick a reaction, close → movie surface
            // is clean. The tray auto-hides 5s after opening.
            setReactionTrayOpen((current) => !current);
          }}
          reactionTrayOpen={reactionTrayOpen}
          isHost={isHost}
          socialControlsHidden={isGhostMode || isPrivacyMode}
          callTileHidden={callTileSession.isHidden}
          onShowCallTile={() => {
            // Toggle: the floating video tile hides from the dock (the
            // tile itself no longer carries a close button — see #4).
            onCallTileSessionChange({
              ...callTileSession,
              isHidden: !callTileSession.isHidden,
            });
          }}
          sharedControls={sharedControls}
          onToggleSharedControls={() => {
            void setSharedControls(!sharedControls)
              .then(onSnapshot)
              .catch((error: unknown) => {
                console.error("set_shared_controls failed", error);
                showControlNotice("Movie Party could not change who can control playback.");
              });
          }}
          onLeave={onLeave}
        />
      </section>
    </main>
  );
}
