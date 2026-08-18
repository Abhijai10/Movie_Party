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
import type { KeyboardEvent as ReactKeyboardEvent, SyntheticEvent } from "react";
import { useEffect, useMemo, useRef, useState } from "react";

type CinemaModeProps = {
  snapshot: AppSnapshot;
  onSnapshot: (snapshot: AppSnapshot) => void;
  onLeave: () => void;
};

const reactions = ["😂", "❤️", "😮", "🔥", "😭", "👏"] as const;
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

export function CinemaMode({ snapshot, onSnapshot, onLeave }: CinemaModeProps) {
  const [draft, setDraft] = useState("");
  const [isComposing, setIsComposing] = useState(false);
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const [reactionWarning, setReactionWarning] = useState("");
  const [callDevices, setCallDevices] = useState<CallDeviceInventory | null>(null);
  const [isCameraCardMinimized, setIsCameraCardMinimized] = useState(false);
  const [isCameraCardHidden, setIsCameraCardHidden] = useState(false);
  const [privacyNotice, setPrivacyNotice] = useState("");
  const callSessionKey = useRef("");
  const messages = snapshot.chat;
  const floatingReactions = snapshot.reactions.slice(-4);
  const isGhostMode = snapshot.ghostMode;
  const isPrivacyMode = snapshot.privacyMode;
  const isHost = snapshot.room.role === "HOST";
  const sharedControls = snapshot.room.sharedControls;
  const cameraEnabled = snapshot.call.camera.enabled;
  const microphoneEnabled = snapshot.call.microphone.enabled;
  const callMode = snapshot.call.mode;
  const movieTitle = snapshot.media?.filename ?? snapshot.provider.url ?? "Movie";
  const peer = snapshot.participants.find((participant) => participant.role !== snapshot.room.role);
  const peerName = peer?.displayName ?? "Peer";
  const visibleMessages = messages.slice(-3);
  const encodedDraftLength = useMemo(() => new TextEncoder().encode(draft).length, [draft]);
  const isDraftTooLong = encodedDraftLength > chatBodyLimitBytes;

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
    if (callMode === "OFF" || isPrivacyMode || callSessionKey.current === nextKey) {
      return;
    }

    callSessionKey.current = nextKey;
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
    ).then((result) => {
      if (!result.connected) {
        setPrivacyNotice("Call signalling is ready. Waiting for media connection.");
      }
    });
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
        onSnapshot(next);
        setDraft("");
        setIsComposing(false);
      }
    });
  };

  const handleDraftKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      setIsComposing(false);
    }
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
      <section
        className={isGhostMode ? "movie-surface privacy-hidden" : "movie-surface"}
        aria-label="Movie"
      >
        <div className="movie-frame">
          <div className="movie-light" />
          <span>{movieTitle}</span>
          <span className="player-position">
            {formatMs(snapshot.player.positionMs)}
            {snapshot.player.durationMs != null
              ? ` / ${formatMs(snapshot.player.durationMs)}`
              : ""}
            {snapshot.player.state === "BUFFERING" && " (Buffering...)"}
            {snapshot.player.errorMessage && (
              <span className="player-error">{snapshot.player.errorMessage}</span>
            )}
          </span>
        </div>
        {isCameraCardHidden ? (
          <button
            className="camera-restore"
            type="button"
            onClick={() => {
              setIsCameraCardHidden(false);
            }}
          >
            Show Call
          </button>
        ) : (
          <article className="camera-card" aria-label={`${peerName} camera`} draggable>
            <div className="camera-card-header">
              <strong>{peerName}</strong>
              <div>
                <button
                  type="button"
                  onClick={() => {
                    setIsCameraCardMinimized((current) => !current);
                  }}
                  aria-label={isCameraCardMinimized ? "Expand camera card" : "Minimize camera card"}
                >
                  {isCameraCardMinimized ? "Expand" : "Min"}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setIsCameraCardHidden(true);
                  }}
                  aria-label="Hide camera card"
                >
                  Hide
                </button>
              </div>
            </div>
            <span>
              {callMode === "OFF"
                ? "Call off"
                : `Camera ${cameraEnabled ? "on" : "off"} - Mic ${microphoneEnabled ? "on" : "muted"}`}
            </span>
            {isCameraCardMinimized ? null : (
              <>
                <div className="call-mode-row" aria-label="Call mode">
                  <button
                    type="button"
                    aria-pressed={callMode === "VIDEO_VOICE"}
                    onClick={() => {
                      chooseCallMode("VIDEO_VOICE");
                    }}
                  >
                    Video
                  </button>
                  <button
                    type="button"
                    aria-pressed={callMode === "VOICE_ONLY"}
                    onClick={() => {
                      chooseCallMode("VOICE_ONLY");
                    }}
                  >
                    Voice
                  </button>
                  <button
                    type="button"
                    aria-pressed={callMode === "OFF"}
                    onClick={() => {
                      chooseCallMode("OFF");
                    }}
                  >
                    Off
                  </button>
                </div>
                <small>
                  {callDevices?.available
                    ? `${String(callDevices.cameras.length)} camera(s), ${String(callDevices.microphones.length)} mic(s)`
                    : (callDevices?.errorCode ?? "Checking devices...")}
                </small>
              </>
            )}
          </article>
        )}
        <article className="chat-bubble" aria-label="Recent chat message">
          {visibleMessages.map((message) => (
            <p key={message.id}>
              <strong>{message.sender}</strong>
              <span>{message.body}</span>
            </p>
          ))}
        </article>
        <div className="reaction-float-layer" aria-live="polite">
          {floatingReactions.map((reaction) => (
            <span
              key={reaction.id}
              className="floating-reaction"
              aria-label={`${reaction.sender} reacted`}
            >
              {reaction.reaction}
            </span>
          ))}
        </div>
        {snapshot.sync.strictSyncPaused || snapshot.buffer.bufferingParticipant ? (
          <section className="buffer-overlay" aria-live="polite">
            <h1>Paused to keep you together</h1>
            <p>{snapshot.buffer.bufferingParticipant ?? peerName} is buffering</p>
            <div className="meter">
              <span style={{ width: `${String(snapshot.buffer.percent)}%` }} />
            </div>
            <small>Camera and chat are still available.</small>
          </section>
        ) : null}
        {snapshot.sync.roomState === "RECONNECTING" ? (
          <section className="reconnect-overlay" aria-live="polite">
            <h2>{peerName} disconnected.</h2>
            <p>The movie has been paused. Reconnecting...</p>
          </section>
        ) : null}
        {privacyNotice ? (
          <div className="privacy-toast" role="status" aria-live="polite">
            {privacyNotice}
          </div>
        ) : null}
        {isComposing ? (
          <form className="chat-compose" aria-label="Chat compose" onSubmit={sendMessage}>
            <input
              autoFocus
              value={draft}
              aria-invalid={isDraftTooLong}
              onChange={(event) => {
                setDraft(event.target.value);
              }}
              onKeyDown={handleDraftKeyDown}
              placeholder="Type message..."
            />
            <button type="submit" disabled={draft.trim().length === 0 || isDraftTooLong}>
              Send
            </button>
            <small>
              {isDraftTooLong ? "Message is over 2000 bytes." : "Enter sends. Esc closes."}
            </small>
          </form>
        ) : null}
        {isHistoryOpen ? (
          <section className="chat-history" aria-labelledby="chat-history-title">
            <div className="history-header">
              <h2 id="chat-history-title">Chat</h2>
              <button
                type="button"
                onClick={() => {
                  setIsHistoryOpen(false);
                }}
              >
                Close
              </button>
            </div>
            <ol>
              {messages.map((message) => (
                <li key={message.id}>
                  <strong>{message.sender}</strong>
                  <span>{message.body}</span>
                </li>
              ))}
            </ol>
          </section>
        ) : null}
        <div className="reaction-tray" aria-label="Reactions">
          {reactions.map((reaction) => (
            <button
              key={reaction}
              type="button"
              aria-label={`Send ${reaction} reaction`}
              onClick={() => {
                sendReaction(reaction);
              }}
            >
              {reaction}
            </button>
          ))}
          {reactionWarning ? <span role="status">{reactionWarning}</span> : null}
        </div>
        <nav className="control-dock" aria-label="Cinema controls">
          <button
            type="button"
            aria-label="Back 10 seconds"
            onClick={() => {
              void seekRelative(-10_000).then((next) => {
                if (next) {
                  onSnapshot(next);
                }
              });
            }}
          >
            -10
          </button>
          <button
            type="button"
            aria-label="Pause for both participants"
            onClick={() => {
              const action =
                snapshot.sync.roomState === "PLAYING" ? pausePlayback : resumePlayback;
              void action().then((next) => {
                if (next) {
                  onSnapshot(next);
                }
              });
            }}
          >
            {snapshot.sync.roomState === "PLAYING" ? "Pause" : "Resume"}
          </button>
          <button
            type="button"
            aria-label="Forward 10 seconds"
            onClick={() => {
              void seekRelative(10_000).then((next) => {
                if (next) {
                  onSnapshot(next);
                }
              });
            }}
          >
            +10
          </button>
          <button
            type="button"
            aria-label="Mute microphone"
            onClick={() => {
              void setMicrophoneEnabled(!microphoneEnabled).then((next) => {
                if (next) {
                  onSnapshot(next);
                }
              });
            }}
          >
            {microphoneEnabled ? "Mic On" : "Mic"}
          </button>
          <button
            type="button"
            aria-label="Toggle camera"
            onClick={() => {
              void setCameraEnabled(!cameraEnabled).then((next) => {
                if (next) {
                  onSnapshot(next);
                  setIsCameraCardHidden(false);
                }
              });
            }}
          >
            {cameraEnabled ? "Camera" : "Camera Off"}
          </button>
          <button
            type="button"
            aria-label="Open chat"
            onClick={() => {
              setIsComposing(true);
            }}
          >
            Chat
          </button>
          <button
            type="button"
            aria-label="Send reaction"
            onClick={() => {
              sendReaction("👏");
            }}
          >
            React
          </button>
          {isHost && (
            <button
              type="button"
              aria-label={sharedControls ? "Switch to Host Only" : "Switch to Shared Controls"}
              onClick={() => {
                void setSharedControls(!sharedControls).then((next) => {
                  if (next) {
                    onSnapshot(next);
                  }
                });
              }}
            >
              {sharedControls ? "Shared" : "Host Only"}
            </button>
          )}
          <button type="button" onClick={onLeave} aria-label="Open party menu">
            More
          </button>
        </nav>
      </section>
    </main>
  );
}
