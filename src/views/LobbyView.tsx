import { motion } from "framer-motion";
import { ArrowLeft, ArrowRight, Check, Copy, Film, MessageCircle, Users, Video } from "lucide-react";
import { useMemo, useState, type SyntheticEvent } from "react";
import type { AppSnapshot } from "../backend/appRuntime";
import { sendChatMessage } from "../backend/appRuntime";
import { ChatOverlay } from "../components/mp/ChatOverlay";
import { CinemaButton } from "../components/mp/CinemaButton";
import { ParticipantCard } from "../components/mp/ParticipantCard";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import { CallTile } from "../overlays/CallTile";
import type { CallTileSessionState } from "../overlays/callTileState";

type LobbyViewProps = {
  snapshot: AppSnapshot;
  onBack: () => void;
  onReady: () => void;
  onCinema: () => void;
  onToggleSharedControls: (enabled: boolean) => void;
  onSnapshot: (snapshot: AppSnapshot | null) => void;
  callTileSession: CallTileSessionState;
  onCallTileSessionChange: (next: CallTileSessionState) => void;
};

function formatBytes(bytes: number): string {
  if (bytes < 1_000_000) return `${String(Math.round(bytes / 1_000))} KB`;
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

export function LobbyView({
  snapshot,
  onBack,
  onReady,
  onCinema,
  onToggleSharedControls,
  onSnapshot,
  callTileSession,
  onCallTileSessionChange,
}: LobbyViewProps) {
  const [copied, setCopied] = useState(false);
  const [draft, setDraft] = useState("");
  const [isChatOpen, setIsChatOpen] = useState(false);
  const encodedDraftLength = useMemo(() => new TextEncoder().encode(draft).length, [draft]);
  const isDraftTooLong = encodedDraftLength > 2_000;

  const title = snapshot.media?.filename ?? snapshot.provider.url ?? "A Private Cinema";
  const source =
    snapshot.media != null
      ? "Local file"
      : snapshot.provider.providerId != null
        ? "Streaming provider"
        : "Direct link";
  const mediaState = snapshot.media
    ? `${snapshot.media.filename} · ${formatBytes(snapshot.media.fileSize)}`
    : snapshot.provider.url
      ? snapshot.provider.url
      : "Waiting for movie source";
  const bufferSeconds = Math.round(snapshot.buffer.guestBufferAheadMs / 1_000);
  const network = snapshot.network.connected
    ? `${snapshot.network.transport} - ${snapshot.network.path}`
    : snapshot.network.path;
  const isHost = snapshot.room.role === "HOST";
  const sharedControls = snapshot.room.sharedControls;
  const host = snapshot.participants.find((participant) => participant.role === "HOST");
  const guest = snapshot.participants.find((participant) => participant.role === "GUEST");
  const peer = snapshot.participants.find((participant) => participant.role !== snapshot.room.role);
  const peerName = peer?.displayName ?? guest?.displayName ?? host?.displayName ?? "Movie partner";
  const inviteCode = snapshot.room.inviteCode ?? "••••••";
  const everyoneReady =
    snapshot.participants.length > 0 &&
    snapshot.participants.every((participant) => participant.mediaReady) &&
    (snapshot.media != null || snapshot.provider.url != null) &&
    snapshot.network.connected;

  const copyInvite = async () => {
    try {
      await navigator.clipboard.writeText(inviteCode);
      setCopied(true);
      setTimeout(() => {
        setCopied(false);
      }, 1600);
    } catch {
      /* clipboard unavailable */
    }
  };

  const sendLobbyMessage = (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (draft.trim().length === 0 || isDraftTooLong) {
      return;
    }

    void sendChatMessage(draft.trim()).then((next) => {
      if (next) {
        onSnapshot(next);
        setDraft("");
      }
    });
  };

  return (
    <div
      className={`relative w-screen h-screen overflow-hidden ${
        snapshot.ghostMode || snapshot.privacyMode ? "social-hidden" : ""
      }`}
    >
      <SilkBackground variant="calm" />

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider"
          data-testid="lobby-back-btn"
        >
          <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Leave lobby
        </button>
        <div className="flex items-center gap-6">
          <StatusIndicator state="sync" label="Strict sync" />
          <span className="w-px h-4 bg-white/15" />
          <span className="text-[11px] tracking-[0.28em] uppercase text-white/50">
            Cinema lobby
          </span>
        </div>
      </header>

      <main className="relative z-10 max-w-[1300px] mx-auto px-12 mt-6 h-[calc(100vh-120px)] grid grid-cols-12 gap-10">
        <motion.section
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7 }}
          className="col-span-12 lg:col-span-7 flex flex-col justify-center"
        >
          <span className="text-[11px] tracking-[0.32em] uppercase text-white/50">
            Tonight's feature
          </span>
          <h1 className="font-serif-display text-white text-[44px] xl:text-[56px] leading-[0.98] tracking-tight mt-4 max-w-3xl">
            {title}
          </h1>

          <div className="mt-6 flex items-center gap-4 text-white/50 text-sm">
            <div className="flex items-center gap-2">
              <Film className="w-4 h-4" strokeWidth={1.5} />
              <span className="uppercase tracking-[0.2em] text-xs">{source}</span>
            </div>
            <span className="w-1 h-1 rounded-full bg-white/25" />
            <div className="flex items-center gap-2">
              <Users className="w-4 h-4" strokeWidth={1.5} />
              <span className="uppercase tracking-[0.2em] text-xs">
                {snapshot.participants.length} seat{snapshot.participants.length === 1 ? "" : "s"}
              </span>
            </div>
          </div>

          <div
            className="mt-10 relative rounded-2xl overflow-hidden h-[220px] w-full max-w-xl"
            style={{
              background: "linear-gradient(135deg, #1A0F2E 0%, #0A0616 100%)",
              border: "1px solid rgba(159,122,234,0.15)",
            }}
          >
            <div
              className="absolute inset-0"
              style={{
                background:
                  "radial-gradient(ellipse at 30% 40%, rgba(159,122,234,0.25) 0%, transparent 60%), radial-gradient(ellipse at 80% 70%, rgba(107,70,193,0.2) 0%, transparent 60%)",
              }}
            />
            <div className="relative h-full flex items-end p-6">
              <div>
                <p className="text-[10px] tracking-[0.32em] uppercase text-white/40 mb-2">
                  Ready to roll
                </p>
                <p className="font-serif-display text-white text-3xl leading-tight">
                  Dim the lights.
                </p>
              </div>
            </div>
          </div>

          <div className="mt-6 flex items-center gap-6 text-[11px] tracking-[0.2em] uppercase text-white/40">
            <span>{mediaState}</span>
            <span className="w-px h-3 bg-white/15" />
            <span>{bufferSeconds}s prepared</span>
            <span className="w-px h-3 bg-white/15" />
            <span>{network}</span>
          </div>
        </motion.section>

        <motion.section
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.15 }}
          className="col-span-12 lg:col-span-5 flex flex-col justify-center gap-6"
        >
          <div>
            <span className="text-[11px] tracking-[0.28em] uppercase text-white/50">
              People in room
            </span>
            <div className="mt-4 space-y-3">
              <ParticipantCard
                name={host?.displayName}
                role="Host · You"
                ready={host?.mediaReady}
                isYou
                testId="participant-host"
              />
              <ParticipantCard
                name={guest?.displayName}
                role="Guest"
                ready={guest?.mediaReady}
                testId="participant-guest"
              />
            </div>
          </div>

          <div
            className="rounded-2xl p-5"
            style={{
              background: "rgba(13, 11, 20, 0.7)",
              border: "1px solid rgba(159,122,234,0.18)",
              backdropFilter: "blur(14px)",
            }}
          >
            <span className="text-[11px] tracking-[0.28em] uppercase text-white/50">Room code</span>
            <div className="mt-3 flex items-center justify-between gap-3 min-w-0">
              <span
                className="min-w-0 flex-1 truncate font-mono-mp text-white text-sm tracking-[0.12em]"
                data-testid="lobby-room-code"
                title={inviteCode}
              >
                {inviteCode}
              </span>
              <button
                type="button"
                onClick={() => void copyInvite()}
                className="flex items-center gap-2 px-4 py-2 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-xs tracking-widest uppercase text-white/80"
                data-testid="lobby-copy-invite-btn"
              >
                {copied ? (
                  <Check className="w-3.5 h-3.5 text-[#34D399]" />
                ) : (
                  <Copy className="w-3.5 h-3.5" />
                )}
                {copied ? "Copied" : "Copy invite"}
              </button>
            </div>
          </div>

          {isHost ? (
            <div className="flex items-center justify-between rounded-2xl p-4 bg-white/[0.02] border border-white/10">
              <span className="text-[11px] tracking-[0.22em] uppercase text-white/60">
                Controls
              </span>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  aria-pressed={!sharedControls}
                  onClick={() => {
                    onToggleSharedControls(false);
                  }}
                  className={`px-4 py-2 rounded-full text-[11px] tracking-widest uppercase border transition ${
                    !sharedControls
                      ? "bg-[#6B46C1] text-white border-[#9F7AEA]/60"
                      : "bg-white/5 text-white/70 border-white/10 hover:bg-white/10"
                  }`}
                  data-testid="lobby-host-only-btn"
                >
                  Host Only
                </button>
                <button
                  type="button"
                  aria-pressed={sharedControls}
                  onClick={() => {
                    onToggleSharedControls(true);
                  }}
                  className={`px-4 py-2 rounded-full text-[11px] tracking-widest uppercase border transition ${
                    sharedControls
                      ? "bg-[#6B46C1] text-white border-[#9F7AEA]/60"
                      : "bg-white/5 text-white/70 border-white/10 hover:bg-white/10"
                  }`}
                  data-testid="lobby-shared-controls-btn"
                >
                  Shared
                </button>
              </div>
            </div>
          ) : null}

          <div className="lobby-ready-actions">
            <StatusIndicator
              state={everyoneReady ? "ready" : "waiting"}
              label={everyoneReady ? "Everyone ready" : "Waiting for everyone"}
            />
            <div className="lobby-ready-actions-buttons">
              <CinemaButton
                variant="ghost"
                className="lobby-ready-action"
                onClick={onCinema}
                data-testid="lobby-preview-btn"
              >
                Preview
              </CinemaButton>
              <CinemaButton
                className="lobby-ready-action"
                onClick={onReady}
                icon={ArrowRight}
                data-testid="lobby-continue-btn"
              >
                Ready check
              </CinemaButton>
            </div>
          </div>
        </motion.section>
      </main>

      <div className="lobby-social-dock" aria-label="Lobby social controls">
        <button
          type="button"
          onClick={() => {
            setIsChatOpen((current) => !current);
          }}
          className={isChatOpen ? "is-active" : ""}
          data-testid="lobby-chat-btn"
          aria-label="Toggle lobby chat"
        >
          <MessageCircle className="w-4 h-4" strokeWidth={1.7} />
          Chat
        </button>
        <button
          type="button"
          onClick={() => {
            onCallTileSessionChange({ ...callTileSession, isHidden: false });
          }}
          data-testid="lobby-call-btn"
          aria-label="Show lobby call"
        >
          <Video className="w-4 h-4" strokeWidth={1.7} />
          Call
        </button>
      </div>

      <ChatOverlay
        snapshot={snapshot}
        draft={draft}
        isComposing={isChatOpen}
        isHistoryOpen={isChatOpen}
        isDraftTooLong={isDraftTooLong}
        onDraftChange={setDraft}
        onSubmit={sendLobbyMessage}
        onCloseCompose={() => {
          setIsChatOpen(false);
        }}
        onCloseHistory={() => {
          setIsChatOpen(false);
        }}
      />

      <CallTile
        peerName={peerName}
        remoteCameraEnabled={peer?.cameraEnabled ?? false}
        remoteMicrophoneEnabled={peer?.microphoneEnabled ?? false}
        remoteConnected={peer?.connected ?? false}
        session={callTileSession}
        onSessionChange={onCallTileSessionChange}
      />
    </div>
  );
}
