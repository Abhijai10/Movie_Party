import {
  Pause,
  Play,
  RotateCcw,
  RotateCw,
  Mic,
  MicOff,
  Video,
  VideoOff,
  MessageCircle,
  Smile,
  Users,
  LogOut,
} from "lucide-react";

type CinemaControlsProps = {
  visible: boolean;
  isPlaying: boolean;
  currentMs: number;
  durationMs: number | null;
  onSeekRelative: (deltaMs: number) => void;
  onTogglePlayback: () => void;
  microphoneEnabled: boolean;
  onToggleMicrophone: () => void;
  cameraEnabled: boolean;
  onToggleCamera: () => void;
  chatOpen: boolean;
  onToggleChat: () => void;
  onSendReaction: () => void;
  isHost: boolean;
  sharedControls: boolean;
  onToggleSharedControls: () => void;
  onLeave: () => void;
};

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

export function CinemaControls({
  visible,
  isPlaying,
  currentMs,
  durationMs,
  onSeekRelative,
  onTogglePlayback,
  microphoneEnabled,
  onToggleMicrophone,
  cameraEnabled,
  onToggleCamera,
  chatOpen,
  onToggleChat,
  onSendReaction,
  isHost,
  sharedControls,
  onToggleSharedControls,
  onLeave,
}: CinemaControlsProps) {
  const pct = durationMs && durationMs > 0 ? (currentMs / durationMs) * 100 : 0;

  return (
    <div
      className={`absolute inset-x-0 bottom-0 z-30 pointer-events-none transition-opacity duration-500 ${
        visible ? "opacity-100" : "opacity-0"
      }`}
      data-testid="cinema-controls"
    >
      <div
        className="absolute inset-x-0 bottom-0 h-48 pointer-events-none"
        style={{
          background:
            "linear-gradient(to top, rgba(5,5,11,0.95) 0%, rgba(5,5,11,0.6) 40%, transparent 100%)",
        }}
      />

      <div className="relative pointer-events-auto px-10 pb-8 pt-6">
        <div className="relative group cursor-pointer" data-testid="cinema-progress">
          <div className="h-[3px] rounded-full bg-white/12">
            <div
              className="h-full rounded-full transition-[width] duration-100"
              style={{
                width: `${String(pct)}%`,
                background: "linear-gradient(90deg, #9F7AEA, #6B46C1)",
                boxShadow: "0 0 10px rgba(159,122,234,0.6)",
              }}
            />
          </div>
          <div
            className="absolute top-1/2 -translate-y-1/2 w-3 h-3 rounded-full bg-white shadow-[0_0_12px_rgba(159,122,234,0.9)] opacity-0 group-hover:opacity-100 transition"
            style={{ left: `calc(${String(pct)}% - 6px)` }}
          />
        </div>

        <div className="mt-5 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={onTogglePlayback}
              className="w-12 h-12 rounded-full bg-white text-black flex items-center justify-center hover:scale-105 transition"
              data-testid="cinema-play-btn"
              aria-label={isPlaying ? "Pause" : "Play"}
            >
              {isPlaying ? (
                <Pause className="w-5 h-5" strokeWidth={2} />
              ) : (
                <Play className="w-5 h-5 translate-x-[1px]" strokeWidth={2} />
              )}
            </button>

            <IconBtn
              onClick={() => {
                onSeekRelative(-10_000);
              }}
              label="Back 10 seconds"
              testId="cinema-back-btn"
            >
              <RotateCcw className="w-4 h-4" strokeWidth={1.6} />
            </IconBtn>
            <IconBtn
              onClick={() => {
                onSeekRelative(10_000);
              }}
              label="Forward 10 seconds"
              testId="cinema-forward-btn"
            >
              <RotateCw className="w-4 h-4" strokeWidth={1.6} />
            </IconBtn>

            <IconBtn
              onClick={onToggleMicrophone}
              label={microphoneEnabled ? "Mute microphone" : "Unmute microphone"}
              testId="cinema-mic-btn"
            >
              {microphoneEnabled ? (
                <Mic className="w-4 h-4" strokeWidth={1.6} />
              ) : (
                <MicOff className="w-4 h-4 text-white/60" strokeWidth={1.6} />
              )}
            </IconBtn>
            <IconBtn
              onClick={onToggleCamera}
              label={cameraEnabled ? "Turn camera off" : "Turn camera on"}
              testId="cinema-camera-btn"
            >
              {cameraEnabled ? (
                <Video className="w-4 h-4" strokeWidth={1.6} />
              ) : (
                <VideoOff className="w-4 h-4 text-white/60" strokeWidth={1.6} />
              )}
            </IconBtn>

            <div className="font-mono-mp text-xs text-white/70 tracking-widest ml-2">
              {formatMs(currentMs)}
              <span className="text-white/30"> / </span>
              {durationMs == null ? "--:--" : formatMs(durationMs)}
            </div>
          </div>

          <div className="flex items-center gap-1.5">
            <IconBtn onClick={onToggleChat} label="Chat" active={chatOpen} testId="cinema-chat-btn">
              <MessageCircle className="w-4 h-4" strokeWidth={1.6} />
            </IconBtn>
            <IconBtn onClick={onSendReaction} label="Send reaction" testId="cinema-react-btn">
              <Smile className="w-4 h-4" strokeWidth={1.6} />
            </IconBtn>
            {isHost && (
              <IconBtn
                onClick={onToggleSharedControls}
                label={sharedControls ? "Switch to Host Only" : "Switch to Shared Controls"}
                active={sharedControls}
                testId="cinema-shared-btn"
              >
                <Users className="w-4 h-4" strokeWidth={1.6} />
              </IconBtn>
            )}
            <IconBtn onClick={onLeave} label="Leave cinema" testId="cinema-leave-btn">
              <LogOut className="w-4 h-4" strokeWidth={1.6} />
            </IconBtn>
          </div>
        </div>
      </div>
    </div>
  );
}

function IconBtn({
  children,
  onClick,
  label,
  active,
  testId,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  label: string;
  active?: boolean;
  testId?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      data-testid={testId}
      className={`w-10 h-10 rounded-full flex items-center justify-center transition ${
        active ? "bg-[#6B46C1] text-white" : "text-white/85 hover:bg-white/10"
      }`}
    >
      {children}
    </button>
  );
}

export default CinemaControls;
