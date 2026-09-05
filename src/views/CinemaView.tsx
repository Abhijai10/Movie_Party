import { runLocalPeerConnectionLoopback } from "../call/webrtc";
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
import { ReconnectOverlay } from "../overlays/ReconnectOverlay";
import { ChatOverlay } from "../components/mp/ChatOverlay";
import { CinemaControls } from "../components/mp/CinemaControls";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import type { MouseEvent, SyntheticEvent } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useReducedMotion } from "../hooks/useReducedMotion";
import {
  applyChatArrival,
  canToggleChat,
  closeChatPreview,
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
  const [controlsVisible, setControlsVisible] = useState(true);
  const [privacyNotice, setPrivacyNotice] = useState("");
  const [hasUnreadChat, setHasUnreadChat] = useState(false);
  const [cameraManuallyEnabled, setCameraManuallyEnabled] = useState(false);
  const [microphoneManuallyEnabled, setMicrophoneManuallyEnabled] = useState(false);
  const prefersReducedMotion = useReducedMotion();
  const callSessionKey = useRef("");
  const controlsTimer = useRef<number | null>(null);
  const previousChatLength = useRef(snapshot.chat.length);
  const chatPreviewTimer = useRef<number | null>(null);
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
      const bounds = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
      const request = attached ? resizeNativeVideoSurface(bounds) : attachNativeVideoSurface(bounds);
      attached = true;
      void request.then((next) => {
        if (next) {
          onSnapshot(next);
        }
      });
    };
    updateSurface();
    const observer = new ResizeObserver(updateSurface);
    observer.observe(frame);
    return () => {
      observer.disconnect();
      void detachNativeVideoSurface();
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

    // Outside the modes the message is visible one way or another (open
    // overlay or transient preview); the badge persists past a preview
    // auto-close until the user opens the chat manually.
    setHasUnreadChat(true);
    setChatVisibility(applyChatArrival(chatVisibility, false));

    if (chatPreviewTimer.current != null) {
      window.clearTimeout(chatPreviewTimer.current);
    }
    chatPreviewTimer.current = window.setTimeout(() => {
      setChatVisibility((current) => closeChatPreview(current));
      chatPreviewTimer.current = null;
    }, 5_000);
  }, [chatVisibility, isGhostMode, isPrivacyMode, snapshot.chat.length]);

  useEffect(() => {
    return () => {
      if (chatPreviewTimer.current != null) {
        window.clearTimeout(chatPreviewTimer.current);
      }
    };
  }, []);

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
      // a lingering transient preview would otherwise peek out later.
      if (chatPreviewTimer.current != null) {
        window.clearTimeout(chatPreviewTimer.current);
        chatPreviewTimer.current = null;
      }
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

  useEffect(() => {
    const nextKey = `${callMode}:${String(localCameraEnabled)}:${String(localMicrophoneEnabled)}:${String(isPrivacyMode)}`;
    if (callMode === "OFF" || isPrivacyMode) {
      callSessionKey.current = "";
      return;
    }
    if (callSessionKey.current === nextKey) {
      return;
    }

    callSessionKey.current = nextKey;
    const controller = new AbortController();
    void runLocalPeerConnectionLoopback(
      callMode,
      localCameraEnabled,
      localMicrophoneEnabled,
      async (signal) => {
        const next = await submitCallSignal(signal);
        if (next) {
          onSnapshot(next);
        }
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
  }, [callMode, localCameraEnabled, localMicrophoneEnabled, isPrivacyMode, onSnapshot]);

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
          if (next) {
            onSnapshot(next);
          }
          showNotice(!isGhostMode ? "Ghost Mode on" : "Ghost Mode off", ghostNoticeMs);
        });
        return;
      }

      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        void setPrivacyMode(!isPrivacyMode).then((next) => {
          if (next) {
            onSnapshot(next);
          }
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
        setChatVisibility((current) => (current.manualOpen ? closedChatOverlay : openChatManually()));
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.clearTimeout(noticeTimeout);
    };
  }, [isGhostMode, isPrivacyMode, onSnapshot]);

  const sendMessage = (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (draft.trim().length === 0 || isDraftTooLong) {
      return;
    }

    void sendChatMessage(draft.trim()).then((next) => {
      if (next) {
        previousChatLength.current = next.chat.length;
        onSnapshot(next);
        setDraft("");
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
        if (next) {
          onSnapshot(next);
          return;
        }
        setReactionWarning("Reaction limit reached");
      },
      () => {
        setReactionWarning("Reaction limit reached");
      },
    );
  };

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
          session={callTileSession}
          onSessionChange={onCallTileSessionChange}
        />
        <ChatOverlay
          snapshot={snapshot}
          draft={draft}
          isComposing={isComposing}
          isHistoryOpen={isHistoryOpen}
          isDraftTooLong={isDraftTooLong}
          onDraftChange={setDraft}
          onSubmit={sendMessage}
          onCloseCompose={() => {
            setChatVisibility((current) => ({ ...current, manualOpen: false }));
          }}
          onCloseHistory={() => {
            if (chatPreviewTimer.current != null) {
              window.clearTimeout(chatPreviewTimer.current);
              chatPreviewTimer.current = null;
            }
            setChatVisibility(closedChatOverlay);
          }}
        />
        <FloatingReactions reactions={snapshot.reactions.slice(-4)} />
        <BufferingOverlay
          bufferingParticipant={snapshot.buffer.bufferingParticipant}
          peerName={peerName}
          percent={snapshot.buffer.percent}
          strictSyncPaused={snapshot.sync.strictSyncPaused}
        />
        <ReconnectOverlay
          peerName={peerName}
          isReconnecting={snapshot.sync.roomState === "RECONNECTING"}
        />
        <ProviderStatusOverlay snapshot={snapshot} />
        {privacyNotice ? (
          <div className="privacy-toast" role="status" aria-live="polite">
            {privacyNotice}
          </div>
        ) : null}
        <ReactionTray warning={reactionWarning} onSendReaction={sendReaction} />
        <CinemaControls
          visible={controlsVisible}
          isPlaying={snapshot.sync.roomState === "PLAYING"}
          currentMs={snapshot.player.positionMs}
          durationMs={snapshot.player.durationMs}
          onSeekRelative={(deltaMs) => {
            void seekRelative(deltaMs).then((next) => {
              if (next) {
                onSnapshot(next);
              }
            });
          }}
          onTogglePlayback={() => {
            const action = snapshot.sync.roomState === "PLAYING" ? pausePlayback : resumePlayback;
            void action().then((next) => {
              if (next) {
                onSnapshot(next);
              }
            });
          }}
          microphoneEnabled={localMicrophoneEnabled}
          onToggleMicrophone={() => {
            const nextEnabled = !localMicrophoneEnabled;
            setMicrophoneManuallyEnabled(nextEnabled);
            if (snapshotMicrophoneEnabled === nextEnabled) {
              return;
            }
            void setMicrophoneEnabled(nextEnabled).then((next) => {
              if (next) {
                onSnapshot(next);
              }
            });
          }}
          cameraEnabled={localCameraEnabled}
          onToggleCamera={() => {
            const nextEnabled = !localCameraEnabled;
            setCameraManuallyEnabled(nextEnabled);
            if (snapshotCameraEnabled === nextEnabled) {
              return;
            }
            void setCameraEnabled(nextEnabled).then((next) => {
              if (next) {
                onSnapshot(next);
              }
            });
          }}
          chatOpen={isComposing || isHistoryOpen}
          hasUnreadChat={hasUnreadChat}
          onToggleChat={() => {
            if (chatPreviewTimer.current != null) {
              window.clearTimeout(chatPreviewTimer.current);
              chatPreviewTimer.current = null;
            }
            if (isComposing) {
              setChatVisibility(closedChatOverlay);
              return;
            }
            setHasUnreadChat(false);
            setChatVisibility(openChatManually());
          }}
          onSendReaction={() => {
            sendReaction("👏");
          }}
          isHost={isHost}
          socialControlsHidden={isGhostMode || isPrivacyMode}
          callTileHidden={callTileSession.isHidden}
          onShowCallTile={() => {
            onCallTileSessionChange({ ...callTileSession, isHidden: false });
          }}
          sharedControls={sharedControls}
          onToggleSharedControls={() => {
            void setSharedControls(!sharedControls).then((next) => {
              if (next) {
                onSnapshot(next);
              }
            });
          }}
          onLeave={onLeave}
        />
      </section>
    </main>
  );
}
