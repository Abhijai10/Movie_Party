import { enumerateCallDevices, type CallDeviceInventory } from "../call/mediaDevices";
import { runLocalPeerConnectionLoopback, type CallMode } from "../call/webrtc";
import {
  pausePlayback,
  resumePlayback,
  sendChatMessage,
  sendReaction as sendBackendReaction,
  seekRelative,
  setCallMode,
  setCameraEnabled,
  setGhostMode,
  setMicrophoneEnabled,
  setPrivacyMode,
  setSharedControls,
  submitCallSignal,
  type AppSnapshot,
} from "../backend/appRuntime";
import { CallTile } from "../overlays/CallTile";
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

type CinemaViewProps = {
  snapshot: AppSnapshot;
  onSnapshot: (snapshot: AppSnapshot) => void;
  onLeave: () => void;
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

export function CinemaView({ snapshot, onSnapshot, onLeave }: CinemaViewProps) {
  const [draft, setDraft] = useState("");
  const [isComposing, setIsComposing] = useState(false);
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const [reactionWarning, setReactionWarning] = useState("");
  const [callDevices, setCallDevices] = useState<CallDeviceInventory | null>(null);
  const [isCameraCardMinimized, setIsCameraCardMinimized] = useState(false);
  const [isCameraCardHidden, setIsCameraCardHidden] = useState(false);
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
  const isGhostMode = snapshot.ghostMode;
  const isPrivacyMode = snapshot.privacyMode;
  const isHost = snapshot.room.role === "HOST";
  const sharedControls = snapshot.room.sharedControls;
  const snapshotCameraEnabled = snapshot.call.camera.enabled;
  const snapshotMicrophoneEnabled = snapshot.call.microphone.enabled;
  const cameraEnabled = snapshotCameraEnabled && cameraManuallyEnabled;
  const microphoneEnabled = snapshotMicrophoneEnabled && microphoneManuallyEnabled;
  const callMode = snapshot.call.mode;
  const movieTitle = snapshot.media?.filename ?? snapshot.provider.url ?? "Movie";
  const playerHasFailed = snapshot.player.state === "PLAYER_ERROR";
  const playerIsPreparing =
    playerHasFailed ||
    snapshot.player.presentation.mode === "UNAVAILABLE" ||
    snapshot.player.state === "STOPPED";
  const peer = snapshot.participants.find((participant) => participant.role !== snapshot.room.role);
  const peerName = peer?.displayName ?? "Peer";
  const encodedDraftLength = useMemo(() => new TextEncoder().encode(draft).length, [draft]);
  const isDraftTooLong = encodedDraftLength > chatBodyLimitBytes;

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

    if (!hasNewMessage || isPrivacyMode || isComposing || isHistoryOpen) {
      return;
    }

    setHasUnreadChat(true);
    setIsHistoryOpen(true);

    if (chatPreviewTimer.current != null) {
      window.clearTimeout(chatPreviewTimer.current);
    }
    chatPreviewTimer.current = window.setTimeout(() => {
      setIsHistoryOpen(false);
      chatPreviewTimer.current = null;
    }, 5_000);
  }, [isComposing, isHistoryOpen, isPrivacyMode, snapshot.chat.length]);

  useEffect(() => {
    return () => {
      if (chatPreviewTimer.current != null) {
        window.clearTimeout(chatPreviewTimer.current);
      }
    };
  }, []);

  useEffect(() => {
    if (isPrivacyMode || (!isComposing && !isHistoryOpen) || draft.trim().length > 0) {
      return;
    }

    const inactivityTimer = window.setTimeout(() => {
      setIsComposing(false);
      setIsHistoryOpen(false);
    }, 8_000);

    return () => {
      window.clearTimeout(inactivityTimer);
    };
  }, [draft, isComposing, isHistoryOpen, isPrivacyMode, snapshot.chat.length]);

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
    let isMounted = true;

    void enumerateCallDevices().then((inventory) => {
      if (isMounted) {
        setCallDevices(inventory);
      }
    });

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    const nextKey = `${callMode}:${String(cameraEnabled)}:${String(microphoneEnabled)}:${String(isPrivacyMode)}`;
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
      cameraEnabled,
      microphoneEnabled,
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
  }, [callMode, cameraEnabled, microphoneEnabled, isPrivacyMode, onSnapshot]);

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
        setIsComposing(false);
        setIsHistoryOpen(false);
        return;
      }

      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "m") {
        event.preventDefault();
        if (isPrivacyMode) {
          showNotice("Privacy Mode is active", ghostNoticeMs);
          return;
        }

        const nextGhostMode = !isGhostMode;
        void setGhostMode(nextGhostMode).then((next) => {
          if (next) {
            onSnapshot(next);
          }
          showNotice(nextGhostMode ? "Ghost Mode on" : "Ghost Mode off", ghostNoticeMs);
        });
        setIsComposing(false);
        setIsHistoryOpen(false);
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

          setIsComposing(false);
          setIsHistoryOpen(false);
          showNotice("Privacy Mode on", ghostNoticeMs);
        });
        return;
      }

      if (isTextInput) {
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        setIsComposing(true);
      }

      if (event.key.toLowerCase() === "c") {
        event.preventDefault();
        setIsHistoryOpen((current) => !current);
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
        setIsComposing(false);
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

  const chooseCallMode = (mode: CallMode) => {
    void setCallMode(mode).then((next) => {
      if (next) {
        onSnapshot(next);
      }
    });
  };

  return (
    <main className="cinema-shell" aria-label="Cinema mode">
      <SilkBackground variant="dim" />
      <section
        className={[
          "movie-surface",
          isGhostMode ? "privacy-hidden" : "",
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
        <div className="movie-frame">
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
          callMode={callMode}
          callStatus={snapshot.call.status}
          cameraEnabled={cameraEnabled}
          microphoneEnabled={microphoneEnabled}
          callDevices={callDevices}
          isMinimized={isCameraCardMinimized}
          isHidden={isCameraCardHidden}
          onRestore={() => {
            setIsCameraCardHidden(false);
          }}
          onToggleMinimized={() => {
            setIsCameraCardMinimized((current) => !current);
          }}
          onHide={() => {
            setIsCameraCardHidden(true);
          }}
          onChooseCallMode={chooseCallMode}
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
            setIsComposing(false);
          }}
          onCloseHistory={() => {
            if (chatPreviewTimer.current != null) {
              window.clearTimeout(chatPreviewTimer.current);
              chatPreviewTimer.current = null;
            }
            setIsHistoryOpen(false);
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
          microphoneEnabled={microphoneEnabled}
          onToggleMicrophone={() => {
            const nextEnabled = !microphoneEnabled;
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
          cameraEnabled={cameraEnabled}
          onToggleCamera={() => {
            const nextEnabled = !cameraEnabled;
            setCameraManuallyEnabled(nextEnabled);
            if (snapshotCameraEnabled === nextEnabled) {
              if (nextEnabled) {
                setIsCameraCardHidden(false);
              }
              return;
            }
            void setCameraEnabled(nextEnabled).then((next) => {
              if (next) {
                onSnapshot(next);
                setIsCameraCardHidden(false);
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
              setIsComposing(false);
              setIsHistoryOpen(false);
              return;
            }
            setHasUnreadChat(false);
            setIsHistoryOpen(true);
            setIsComposing(true);
          }}
          onSendReaction={() => {
            sendReaction("👏");
          }}
          isHost={isHost}
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
