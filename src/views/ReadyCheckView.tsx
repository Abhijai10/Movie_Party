import { Button } from "../components/Button";
import { Card } from "../components/Card";
import { CinematicBackdrop } from "../components/CinematicBackdrop";
import type { AppSnapshot } from "../backend/appRuntime";

type ReadyCheckViewProps = {
  snapshot: AppSnapshot;
  onStart: () => void;
};

export function ReadyCheckView({ snapshot, onStart }: ReadyCheckViewProps) {
  return (
    <main className="app-shell centered-shell ready-shell">
      <CinematicBackdrop />
      <section className="ready-panel ready-stage" aria-labelledby="ready-title">
        <div className="ready-orb" aria-hidden="true"><span /></div>
        <div className="ready-heading"><p className="brand">Ready check</p><h1 id="ready-title">The lights are down.</h1><p>Move Party starts only once both sides are prepared to stay together.</p></div>
        <ul className="ready-list" aria-label="Room readiness">
          {snapshot.participants.map((participant) => (
            <li className="ready-row" key={participant.id}>
              <span>{participant.displayName}</span>
              <strong>{participant.mediaReady ? "Ready" : "Waiting"}</strong>
            </li>
          ))}
          <li className="ready-row">
            <span>Media</span>
            <strong>{snapshot.media || snapshot.provider.url ? "Ready" : "Waiting"}</strong>
          </li>
          <li className="ready-row">
            <span>Network</span>
            <strong>{snapshot.network.connected ? "Connected" : "Waiting"}</strong>
          </li>
          <li className="ready-row">
            <span>Sync</span>
            <strong>{snapshot.room.strictSync ? "Locked" : "Waiting"}</strong>
          </li>
        </ul>
        <Card className="sync-note">
          <strong>Strict sync is the room rule</strong>
          <span>Playback waits until both sides can stay together.</span>
        </Card>
        <Button variant="primary" onClick={onStart}>
          Enter Cinema
        </Button>
      </section>
    </main>
  );
}
