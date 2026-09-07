import { Maximize2, MicOff, Minus, Video } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  canInitiateDragFrom,
  clampCallTilePosition,
  closeCallTileLocally,
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
  /** Live remote media from the cross-device session (Batch 12). */
  remoteStream: MediaStream | null;
  /** Live self-view media (muted; never plays locally). */
  localStream: MediaStream | null;
  session: CallTileSessionState;
  onSessionChange: (next: CallTileSessionState) => void;
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
}: CallTileProps) {
  const tileRef = useRef<HTMLElement>(null);
  const remoteVideoRef = useRef<HTMLVideoElement | null>(null);
  const remoteAudioRef = useRef<HTMLAudioElement | null>(null);
  const selfViewRef = useRef<HTMLVideoElement | null>(null);
  const dragOffset = useRef({ x: 0, y: 0 });
  const activePointerId = useRef<number | null>(null);
  const [isDragging, setIsDragging] = useState(false);
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
  const showLiveRemoteVideo = hasRemoteVideoTrack && remote.showVideo;

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

  const clampPosition = useCallback((x: number, y: number) => {
    const rect = tileRef.current?.getBoundingClientRect();
    return clampCallTilePosition(
      { x, y },
      {
        width: typeof window === "undefined" ? 1280 : window.innerWidth,
        height: typeof window === "undefined" ? 720 : window.innerHeight,
      },
      { width: rect?.width ?? 260, height: rect?.height ?? 220 },
    );
  }, []);

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
      style={{ left: session.position.x, top: session.position.y }}
      onPointerDown={startDrag}
      onPointerMove={moveDrag}
      onPointerUp={stopDrag}
      onPointerCancel={stopDrag}
    >
      {session.isMinimized ? null : (
        <header className="camera-card-header">
          <div>
            <strong>{peerName}</strong>
            <span>{remote.statusLabel}</span>
          </div>
          <div>
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
            <button
              type="button"
              onClick={() => {
                onSessionChange(closeCallTileLocally(session));
              }}
              aria-label="Hide call tile"
              title="Hide call tile"
              data-call-tile-control
            >
              <span aria-hidden="true">x</span>
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
        ) : remote.showVideo ? (
          <div className="camera-video-placeholder" aria-label={`${peerName} video`}>
            <Video className="w-7 h-7" strokeWidth={1.4} />
          </div>
        ) : (
          <div className="camera-avatar" aria-label={`${peerName} camera off`}>
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
