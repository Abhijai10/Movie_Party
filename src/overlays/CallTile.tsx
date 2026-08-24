import type { CallDeviceInventory } from "../call/mediaDevices";
import type { CallMode } from "../call/webrtc";
import { Maximize2, Mic, MicOff, Minus, PhoneOff, Video, VideoOff, X } from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";

type CallTileProps = {
  peerName: string;
  callMode: CallMode;
  callStatus: string;
  cameraEnabled: boolean;
  microphoneEnabled: boolean;
  callDevices: CallDeviceInventory | null;
  isMinimized: boolean;
  isHidden: boolean;
  onRestore: () => void;
  onToggleMinimized: () => void;
  onHide: () => void;
  onChooseCallMode: (mode: CallMode) => void;
};

export function CallTile({
  peerName,
  callMode,
  callStatus,
  cameraEnabled,
  microphoneEnabled,
  callDevices,
  isMinimized,
  isHidden,
  onRestore,
  onToggleMinimized,
  onHide,
  onChooseCallMode,
}: CallTileProps) {
  const tileRef = useRef<HTMLElement>(null);
  const dragOffset = useRef({ x: 0, y: 0 });
  const [position, setPosition] = useState(() => ({
    x: Math.max(16, (typeof window === "undefined" ? 1280 : window.innerWidth) - 320),
    y: 32,
  }));
  const [isDragging, setIsDragging] = useState(false);
  const statusLabel =
    callMode === "OFF"
      ? "Call off"
      : callStatus === "connected"
        ? "Connected"
        : callStatus === "connecting"
          ? "Connecting"
          : callStatus === "reconnecting"
            ? "Reconnecting"
            : callStatus === "degraded"
              ? "Call degraded"
              : callStatus === "unavailable"
                ? "Call failed"
                : "Disconnected";
  const technicalDetail =
    callDevices?.available === false
      ? (callDevices.errorCode ?? "Device check unavailable")
      : `Camera ${cameraEnabled ? "on" : "off"} · Mic ${microphoneEnabled ? "on" : "muted"}`;

  const clampPosition = useCallback((x: number, y: number) => {
    const rect = tileRef.current?.getBoundingClientRect();
    const width = rect?.width ?? 260;
    const height = rect?.height ?? 190;
    const viewportWidth = typeof window === "undefined" ? 1280 : window.innerWidth;
    const viewportHeight = typeof window === "undefined" ? 720 : window.innerHeight;
    return {
      x: Math.min(Math.max(12, x), Math.max(12, viewportWidth - width - 12)),
      y: Math.min(Math.max(12, y), Math.max(12, viewportHeight - height - 12)),
    };
  }, []);

  useEffect(() => {
    const handleResize = () => {
      setPosition((current) => clampPosition(current.x, current.y));
    };

    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
    };
  }, [clampPosition]);

  const startDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.target instanceof HTMLElement && event.target.closest("button")) {
      return;
    }

    const rect = event.currentTarget.getBoundingClientRect();
    dragOffset.current = {
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    setIsDragging(true);
  };

  const moveDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (!isDragging) {
      return;
    }
    setPosition(
      clampPosition(event.clientX - dragOffset.current.x, event.clientY - dragOffset.current.y),
    );
  };

  const stopDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setIsDragging(false);
  };

  if (isHidden) {
    return (
      <button className="camera-restore" type="button" onClick={onRestore}>
        Show Call
      </button>
    );
  }

  return (
    <article
      ref={tileRef}
      className={`camera-card ${isDragging ? "is-dragging" : ""}`}
      aria-label={`${peerName} camera`}
      style={{ left: position.x, top: position.y }}
      onPointerDown={startDrag}
      onPointerMove={moveDrag}
      onPointerUp={stopDrag}
      onPointerCancel={stopDrag}
    >
      <div className="camera-card-header">
        <div>
          <strong>{peerName}</strong>
          <span>{statusLabel}</span>
        </div>
        <div>
          <button
            type="button"
            onClick={onToggleMinimized}
            aria-label={isMinimized ? "Expand camera card" : "Minimize camera card"}
          >
            {isMinimized ? (
              <Maximize2 className="w-3.5 h-3.5" strokeWidth={1.7} />
            ) : (
              <Minus className="w-3.5 h-3.5" strokeWidth={1.7} />
            )}
          </button>
          <button type="button" onClick={onHide} aria-label="Hide camera card">
            <X className="w-3.5 h-3.5" strokeWidth={1.7} />
          </button>
        </div>
      </div>
      <div className="camera-video-stage">
        {cameraEnabled && callMode === "VIDEO_VOICE" ? (
          <div className="camera-video-placeholder">
            <Video className="w-7 h-7" strokeWidth={1.4} />
          </div>
        ) : (
          <div className="camera-avatar" aria-hidden="true">
            {peerName
              .split(" ")
              .map((part) => part[0])
              .join("")
              .slice(0, 2)
              .toUpperCase() || "MP"}
          </div>
        )}
      </div>
      {isMinimized ? null : (
        <>
          <div className="call-quick-state">
            <span>
              {cameraEnabled ? (
                <Video className="w-3.5 h-3.5" strokeWidth={1.7} />
              ) : (
                <VideoOff className="w-3.5 h-3.5" strokeWidth={1.7} />
              )}
              Camera {cameraEnabled ? "on" : "off"}
            </span>
            <span>
              {microphoneEnabled ? (
                <Mic className="w-3.5 h-3.5" strokeWidth={1.7} />
              ) : (
                <MicOff className="w-3.5 h-3.5" strokeWidth={1.7} />
              )}
              Mic {microphoneEnabled ? "on" : "muted"}
            </span>
          </div>
          <div className="call-mode-row" aria-label="Call mode">
            <button
              type="button"
              aria-pressed={callMode === "VIDEO_VOICE"}
              onClick={() => {
                onChooseCallMode("VIDEO_VOICE");
              }}
            >
              Video
            </button>
            <button
              type="button"
              aria-pressed={callMode === "VOICE_ONLY"}
              onClick={() => {
                onChooseCallMode("VOICE_ONLY");
              }}
            >
              Voice
            </button>
            <button
              type="button"
              aria-pressed={callMode === "OFF"}
              onClick={() => {
                onChooseCallMode("OFF");
              }}
            >
              <PhoneOff className="w-3.5 h-3.5" strokeWidth={1.7} />
              Off
            </button>
          </div>
          <small>
            {callDevices?.available
              ? `${String(callDevices.cameras.length)} camera(s), ${String(callDevices.microphones.length)} mic(s)`
              : technicalDetail}
          </small>
        </>
      )}
    </article>
  );
}
