import { Button } from "../components/Button";
import { Card } from "../components/Card";
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
    <main className="app-shell centered-shell">
      <section className="lobby-panel" aria-labelledby="lobby-title">
        <div className="panel-header">
          <Button variant="ghost" onClick={onBack}>
            Back
          </Button>
          <Button variant="ghost" onClick={onCinema}>
            Preview Cinema
          </Button>
        </div>
        <p className="brand">Lobby</p>
        <h1 id="lobby-title">{title}</h1>
        <p className="mode-line">
          {snapshot.room.state} · {mode}
        </p>
        <div className="lobby-status-grid">
          <Card className="status-card">
            <span>Invite</span>
            <strong>{snapshot.room.inviteCode ?? "Waiting for invite"}</strong>
          </Card>
          <Card className="status-card">
            <span>Media</span>
            <strong>{mediaState}</strong>
          </Card>
          <Card className="status-card">
            <span>Network</span>
            <strong>{network}</strong>
          </Card>
          <Card className="status-card">
            <span>Provider</span>
            <strong>{snapshot.provider.state}</strong>
          </Card>
        </div>
        <div className="participant-grid">
          {snapshot.participants.map((participant) => (
            <Participant key={participant.id} participant={participant} />
          ))}
        </div>

        <fieldset className="segmented-field">
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

        <section className="network-summary" aria-label="Network">
          <strong>Network</strong>
          <span>{network}</span>
        </section>
        <section className="buffer-summary" aria-label="Guest buffer">
          <strong>Guest Buffer</strong>
          <div className="meter">
            <span style={{ width: bufferPercent }} />
          </div>
          <span>{bufferSeconds}s prepared</span>
        </section>
        <Button variant="primary" onClick={onReady}>
          Start When Ready
        </Button>
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
      <h2>{participant.displayName}</h2>
      <span>{participant.connected ? "Connected" : "Disconnected"}</span>
      <span>Media {participant.mediaReady ? "Ready" : "Preparing"}</span>
      <span>Camera {participant.cameraEnabled ? "On" : "Off"}</span>
      <span>Mic {participant.microphoneEnabled ? "On" : "Muted"}</span>
    </article>
  );
}
