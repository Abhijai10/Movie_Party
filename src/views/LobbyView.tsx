import { Button } from "../components/Button";
import { Card } from "../components/Card";
import { CinematicBackdrop } from "../components/CinematicBackdrop";
import type { AppSnapshot, ParticipantSnapshot } from "../backend/appRuntime";

type LobbyViewProps = {
  snapshot: AppSnapshot;
  onBack: () => void;
  onReady: () => void;
  onCinema: () => void;
  onToggleSharedControls: (enabled: boolean) => void;
};

export function LobbyView({
  snapshot,
  onBack,
  onReady,
  onCinema,
  onToggleSharedControls,
}: LobbyViewProps) {
  const title = snapshot.media?.filename ?? snapshot.provider.url ?? "Local party";
  const mode = snapshot.provider.mode.replaceAll("_", " ");
  const bufferPercent = `${String(snapshot.buffer.percent)}%`;
  const bufferSeconds = Math.round(snapshot.buffer.guestBufferAheadMs / 1_000);
  const network = snapshot.network.connected
    ? `${snapshot.network.transport} - ${snapshot.network.path}`
    : snapshot.network.path;

  const isHost = snapshot.room.role === "HOST";
  const sharedControls = snapshot.room.sharedControls;
  const mediaState = snapshot.media
    ? `${snapshot.media.filename} · ${formatBytes(snapshot.media.fileSize)}`
    : snapshot.provider.url
      ? snapshot.provider.url
      : "Waiting for movie source";

  return (
    <main className="app-shell centered-shell lobby-shell">
      <CinematicBackdrop />
      <section className="lobby-panel cinema-lobby" aria-labelledby="lobby-title">
        <div className="lobby-topbar">
          <Button variant="ghost" onClick={onBack}>
            Back
          </Button>
          <Button variant="ghost" onClick={onCinema}>
            Preview Cinema
          </Button>
        </div>
        <div className="lobby-hero">
          <div><p className="brand">Private screening room</p><h1 id="lobby-title">{title}</h1><p className="mode-line">{snapshot.room.state} · {mode}</p></div>
          <div className="lobby-invite"><span>Room invite</span><strong>{snapshot.room.inviteCode ?? "Preparing invite"}</strong><small>Share with one trusted friend</small></div>
        </div>
        <div className="lobby-room-grid">
          <section className="lobby-seating" aria-label="Participants"><p>In the room</p><div className="participant-grid">{snapshot.participants.map((participant) => <Participant key={participant.id} participant={participant} />)}</div></section>
          <aside className="room-readiness"><div className="room-signal"><span>Connection</span><strong>{network}</strong><small>Provider: {snapshot.provider.state}</small></div><div className="room-signal"><span>Movie source</span><strong>{mediaState}</strong></div><div className="buffer-summary" aria-label="Guest buffer"><strong>Guest buffer</strong><div className="meter"><span style={{ width: bufferPercent }} /></div><span>{bufferSeconds}s prepared</span></div></aside>
        </div>
        <div className="lobby-footer">
          <fieldset className="segmented-field lobby-controls">
            <legend>Controls</legend>
            <button
              type="button"
              aria-pressed={!sharedControls}
              disabled={!isHost}
              onClick={() => {
                onToggleSharedControls(false);
              }}
            >
              Host Only
            </button>
            <button
              type="button"
              aria-pressed={sharedControls}
              disabled={!isHost}
              onClick={() => {
                onToggleSharedControls(true);
              }}
            >
              Shared Controls
            </button>
          </fieldset>
          <Button variant="primary" onClick={onReady}>Start when ready</Button>
        </div>
      </section>
    </main>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1_000_000) {
    return `${Math.round(bytes / 1_000)} KB`;
  }

  if (bytes < 1_000_000_000) {
    return `${(bytes / 1_000_000).toFixed(1)} MB`;
  }

  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

function Participant({ participant }: { participant: ParticipantSnapshot }) {
  return (
    <article className="participant">
      <div className="participant-avatar" aria-hidden="true">
        {participant.displayName.slice(0, 1).toUpperCase()}
      </div>
      <div className="participant-identity">
        <h2>{participant.displayName}</h2>
        <span>{participant.role}</span>
      </div>
      <div className="participant-state"><StatusPill label={participant.connected ? "Connected" : "Disconnected"} ok={participant.connected} /><StatusPill label={participant.mediaReady ? "Media ready" : "Preparing media"} ok={participant.mediaReady} /></div>
    </article>
  );
}

function StatusPill({ label, ok }: { label: string; ok: boolean }) {
  return <span className={ok ? "status-pill ok" : "status-pill"}>{label}</span>;
}
