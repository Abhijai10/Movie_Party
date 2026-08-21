type ControlDockProps = {
  isHost: boolean;
  sharedControls: boolean;
  isPlaying: boolean;
  microphoneEnabled: boolean;
  cameraEnabled: boolean;
  onSeekRelative: (deltaMs: number) => void;
  onTogglePlayback: () => void;
  onToggleMicrophone: () => void;
  onToggleCamera: () => void;
  onOpenChat: () => void;
  onReact: () => void;
  onToggleSharedControls: () => void;
  onLeave: () => void;
};

export function ControlDock({
  isHost,
  sharedControls,
  isPlaying,
  microphoneEnabled,
  cameraEnabled,
  onSeekRelative,
  onTogglePlayback,
  onToggleMicrophone,
  onToggleCamera,
  onOpenChat,
  onReact,
  onToggleSharedControls,
  onLeave,
}: ControlDockProps) {
  return (
    <nav className="control-dock" aria-label="Cinema controls">
      <button
        type="button"
        aria-label="Back 10 seconds"
        onClick={() => {
          onSeekRelative(-10_000);
        }}
      >
        -10
      </button>
      <button type="button" aria-label="Pause for both participants" onClick={onTogglePlayback}>
        {isPlaying ? "Pause" : "Resume"}
      </button>
      <button
        type="button"
        aria-label="Forward 10 seconds"
        onClick={() => {
          onSeekRelative(10_000);
        }}
      >
        +10
      </button>
      <button type="button" aria-label="Mute microphone" onClick={onToggleMicrophone}>
        {microphoneEnabled ? "Mic On" : "Mic"}
      </button>
      <button type="button" aria-label="Toggle camera" onClick={onToggleCamera}>
        {cameraEnabled ? "Camera" : "Camera Off"}
      </button>
      <button type="button" aria-label="Open chat" onClick={onOpenChat}>
        Chat
      </button>
      <button type="button" aria-label="Send reaction" onClick={onReact}>
        React
      </button>
      {isHost && (
        <button
          type="button"
          aria-label={sharedControls ? "Switch to Host Only" : "Switch to Shared Controls"}
          onClick={onToggleSharedControls}
        >
          {sharedControls ? "Shared" : "Host Only"}
        </button>
      )}
      <button type="button" onClick={onLeave} aria-label="Open party menu">
        More
      </button>
    </nav>
  );
}
