import { Maximize2, MicOff, Minus, Video } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  canInitiateDragFrom,
  clampCallTilePosition,
  clampCameraCardSize,
  clampCameraCardHeight,
  saveCallTilePosition,
  deriveRemoteCallPresentation,
  minimizeCallTile,
  restoreCallTile,
  type CallTileSessionState,
} from "./callTileState";

type CallTileProps = {
  peerName: string;
  remoteCameraEnabled: boolean;
  remoteMicrophoneEnabled: boolean;
  remoteConnected: boolean;
  /** Live remote media from the cross-device session. */
  remoteStream: MediaStream | null;
  /** Live self-view media (muted; never plays locally). */
  localStream: MediaStream | null;
  session: CallTileSessionState;
  onSessionChange: (next: CallTileSessionState) => void;
  /**
   * Bottom strip the tile must never cover (the cinema control dock with
   * the final action button). 0 = nothing reserved.
   */
  reservedBottomPx?: number;
};

export function CallTile({
  peerName,
  remoteCameraEnabled,
  remoteMicrophoneEnabled,
  remoteConnected,
  remoteStream,
  localStream,
  session,
  onSessionChange,
  reservedBottomPx = 0,
}: CallTileProps) {
  const tileRef = useRef<HTMLElement>(null);
  const remoteVideoRef = useRef<HTMLVideoElement | null>(null);
  const remoteAudioRef = useRef<HTMLAudioElement | null>(null);
  const selfViewRef = useRef<HTMLVideoElement | null>(null);
  const dragOffset = useRef({ x: 0, y: 0 });
  const activePointerId = useRef<number | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  // §4 call-tile diet: the header toggle that shows/hides the peer's
  // video. Suppressed shows the avatar instead — the feed is still
  // attached (audio keeps playing), just not displayed.
  const [videoSuppressed, setVideoSuppressed] = useState(false);
  const remote = deriveRemoteCallPresentation(
    remoteCameraEnabled,
    remoteMicrophoneEnabled,
    remoteConnected,
  );
  const hasRemoteVideoTrack =
    remoteStream?.getVideoTracks().some((track) => track.readyState === "live") ?? false;
  // Presentation matrix:
  // - wire video + remote camera on  → live <video>
  // - remote camera on, no wire yet  → placeholder icon (pre-connect)
  // - remote camera off             → avatar (element unmounts; the peer
  //   re-enabling remounts it and autoplay resumes immediately)
  const showLiveRemoteVideo = hasRemoteVideoTrack && remote.showVideo && !videoSuppressed;

  // Attach live media: the wire has media whenever the session exists, so
  // the remote video renders whenever the peer actually sends frames.
  // remoteCameraEnabled only governs presentation when no wire media
  // exists yet (keep the avatar/muted indicators meaningful pre-connect).
  // showLiveRemoteVideo is a dep because the <video> element UNMOUNTS on
  // camera-off and REMOUNTS on camera-back-on — a remounted element has
  // no srcObject until this effect re-runs, and remoteStream alone would
  // not change in that flip.
  useEffect(() => {
    const element = remoteVideoRef.current;
    if (!element) {
      return;
    }
    if (element.srcObject !== remoteStream) {
      element.srcObject = remoteStream;
    }
    if (remoteStream) {
      void element.play().catch(() => undefined);
    }
  }, [remoteStream, showLiveRemoteVideo]);

  useEffect(() => {
    const element = remoteAudioRef.current;
    if (!element) {
      return;
    }
    if (element.srcObject !== remoteStream) {
      element.srcObject = remoteStream;
    }
    if (remoteStream) {
      void element.play().catch(() => undefined);
    }
  }, [remoteStream]);

  useEffect(() => {
    const element = selfViewRef.current;
    if (!element) {
      return;
    }
    if (element.srcObject !== localStream) {
      element.srcObject = localStream;
    }
    if (localStream) {
      void element.play().catch(() => undefined);
    }
  }, [localStream]);

  const clampPosition = useCallback(
    (x: number, y: number) => {
      const rect = tileRef.current?.getBoundingClientRect();
      return clampCallTilePosition(
        { x, y },
        {
          width: typeof window === "undefined" ? 1280 : window.innerWidth,
          height: typeof window === "undefined" ? 720 : window.innerHeight,
        },
        { width: rect?.width ?? 260, height: rect?.height ?? 220 },
        reservedBottomPx,
      );
    },
    [reservedBottomPx],
  );

  const updateSession = useCallback(
    (next: Partial<CallTileSessionState>) => {
      onSessionChange({ ...session, ...next });
    },
    [onSessionChange, session],
  );

  useEffect(() => {
    const handleResize = () => {
      updateSession({ position: clampPosition(session.position.x, session.position.y) });
    };

    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
    };
  }, [clampPosition, session, updateSession]);

  useEffect(() => {
    document.body.classList.toggle("call-tile-dragging", isDragging);
    return () => {
      document.body.classList.remove("call-tile-dragging");
    };
  }, [isDragging]);

  const stopDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (activePointerId.current !== event.pointerId) {
      return;
    }
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    activePointerId.current = null;
    setIsDragging(false);
    // §28: persist location locally when a drag ends.
    saveCallTilePosition(session.position);
  };

  const startDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (!canInitiateDragFrom(event.target as Element | null)) {
      return;
    }

    const rect = event.currentTarget.getBoundingClientRect();
    dragOffset.current = { x: event.clientX - rect.left, y: event.clientY - rect.top };
    activePointerId.current = event.pointerId;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    setIsDragging(true);
  };

  const moveDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (activePointerId.current !== event.pointerId) {
      return;
    }
    event.preventDefault();
    updateSession({
      position: clampPosition(event.clientX - dragOffset.current.x, event.clientY - dragOffset.current.y),
    });
  };

  if (session.isHidden) {
    return null;
  }

  const initials =
    peerName
      .split(" ")
      .map((part) => part[0])
      .join("")
      .slice(0, 2)
      .toUpperCase() || "MP";

  return (
    <article
      ref={tileRef}
      className={`camera-card ${session.isMinimized ? "is-minimized" : ""} ${isDragging ? "is-dragging" : ""}`}
      aria-label={`${peerName} call`}
      style={
        session.isMinimized
          ? { left: session.position.x, top: session.position.y }
          : {
              left: session.position.x,
              top: session.position.y,
              width: `${String(session.sizePx)}px`,
              height: `${String(session.heightPx)}px`,
            }
      }
      title={session.isMinimized ? `${peerName} — click to restore` : undefined}
      onClick={
        session.isMinimized
          ? () => {
              onSessionChange(restoreCallTile(session));
            }
          : undefined
      }
      onPointerDown={startDrag}
      onPointerMove={moveDrag}
      onPointerUp={stopDrag}
      onPointerCancel={stopDrag}
    >
      {session.isMinimized ? null : (
        <button
          type="button"
          className="camera-card-resize"
          data-call-tile-control
          aria-label="Resize camera card"
          title="Drag to resize (width 120–360 px, height 150–480 px)"
          onPointerDown={(event) => {
            event.preventDefault();
            event.stopPropagation();
            const startX = event.clientX;
            const startY = event.clientY;
            const startSize = session.sizePx;
            const startHeight = session.heightPx;
            // Both axes resize together from the bottom-right grip: dx
            // drives width, dy drives height. Each is clamped in its own
            // spec range so the header always stays visible.
            const onMove = (moveEvent: PointerEvent) => {
              const sizePx = clampCameraCardSize(startSize + (moveEvent.clientX - startX));
              const heightPx = clampCameraCardHeight(
                startHeight + (moveEvent.clientY - startY),
              );
              // F12: growing the card must not push it over the reserved dock
              // strip. `clampPosition` reads the live DOM rect, which still
              // reports the PRE-resize size at this point, so clamp against the
              // incoming dimensions instead.
              updateSession({
                sizePx,
                heightPx,
                position: clampCallTilePosition(
                  session.position,
                  {
                    width: typeof window === "undefined" ? 1280 : window.innerWidth,
                    height: typeof window === "undefined" ? 720 : window.innerHeight,
                  },
                  { width: sizePx, height: heightPx },
                  reservedBottomPx,
                ),
              });
            };
            const onUp = () => {
              window.removeEventListener("pointermove", onMove);
              window.removeEventListener("pointerup", onUp);
            };
            window.addEventListener("pointermove", onMove);
            window.addEventListener("pointerup", onUp);
          }}
        />
      )}
      {session.isMinimized ? null : (
        <header className="camera-card-header">
          <strong>{peerName}</strong>
          <div>
            <button
              type="button"
              onClick={() => {
                setVideoSuppressed((current) => !current);
              }}
              aria-label={videoSuppressed ? "Show video" : "Hide video"}
              title={videoSuppressed ? "Show video" : "Hide video"}
              data-call-tile-control
              data-video-toggle
              data-active={videoSuppressed ? "off" : "on"}
            >
              <Video className="w-3.5 h-3.5" strokeWidth={1.7} />
            </button>
            <button
              type="button"
              onClick={() => {
                onSessionChange(minimizeCallTile(session));
              }}
              aria-label="Minimize call tile"
              title="Minimize call tile"
              data-call-tile-control
            >
              <Minus className="w-3.5 h-3.5" strokeWidth={1.7} />
            </button>
          </div>
        </header>
      )}

      <div className="camera-video-stage">
        {showLiveRemoteVideo ? (
          <video
            ref={remoteVideoRef}
            className="camera-remote-video"
            autoPlay
            playsInline
            aria-label={`${peerName} video`}
          />
        ) : remote.showVideo && !videoSuppressed ? (
          <div className="camera-video-placeholder" aria-label={`${peerName} video`}>
            <Video className="w-7 h-7" strokeWidth={1.4} />
          </div>
        ) : (
          <div className="camera-avatar" aria-label={videoSuppressed ? `${peerName} video hidden` : `${peerName} camera off`}>
            {initials}
          </div>
        )}
        {/* Remote audio: attached whenever wire media exists; autoplay
            policy is satisfied by the user gesture that started the call. */}
        <audio ref={remoteAudioRef} autoPlay aria-label={`${peerName} audio`} />
        {remote.showMutedIndicator ? (
          <span className="remote-muted-indicator" aria-label={`${peerName} microphone muted`}>
            <MicOff className="w-3.5 h-3.5" strokeWidth={1.8} />
          </span>
        ) : null}
        {localStream && localStream.getVideoTracks().length > 0 ? (
          <video
            ref={selfViewRef}
            className="camera-self-view"
            autoPlay
            playsInline
            muted
            aria-label="Your camera preview"
          />
        ) : null}
        {session.isMinimized ? (
          <button
            type="button"
            className="camera-restore-control"
            onClick={() => {
              onSessionChange(restoreCallTile(session));
            }}
            aria-label="Restore call tile"
            title="Restore call tile"
            data-call-tile-control
          >
            <Maximize2 className="w-3.5 h-3.5" strokeWidth={1.7} />
          </button>
        ) : null}
      </div>
    </article>
  );
}
