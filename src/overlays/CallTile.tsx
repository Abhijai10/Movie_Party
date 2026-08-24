import type { CallDeviceInventory } from "../call/mediaDevices";
import type { CallMode } from "../call/webrtc";

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
  if (isHidden) {
    return (
      <button className="camera-restore" type="button" onClick={onRestore}>
        Show Call
      </button>
    );
  }

  return (
    <article className="camera-card" aria-label={`${peerName} camera`} draggable>
      <div className="camera-card-header">
        <strong>{peerName}</strong>
        <div>
          <button
            type="button"
            onClick={onToggleMinimized}
            aria-label={isMinimized ? "Expand camera card" : "Minimize camera card"}
          >
            {isMinimized ? "Expand" : "Min"}
          </button>
          <button type="button" onClick={onHide} aria-label="Hide camera card">
            Hide
          </button>
        </div>
      </div>
      <span>
        {callMode === "OFF"
          ? "Call off"
          : `Call ${callStatus} - Camera ${cameraEnabled ? "on" : "off"} - Mic ${
              microphoneEnabled ? "on" : "muted"
            }`}
      </span>
      {isMinimized ? null : (
        <>
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
  );
}
