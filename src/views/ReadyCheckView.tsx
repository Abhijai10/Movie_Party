import type { AppSnapshot } from "../backend/appRuntime";

type ReadyCheckViewProps = {
  snapshot: AppSnapshot;
  onStart: () => void;
};

export function ReadyCheckView({ snapshot, onStart }: ReadyCheckViewProps) {
  return (
    <main className="app-shell centered-shell">
      <section className="ready-panel" aria-labelledby="ready-title">
        <h1 id="ready-title">Almost ready</h1>
        <ul>
          {snapshot.participants.map((participant) => (
            <li key={participant.id}>
              <span>{participant.displayName}</span>
              <strong>{participant.mediaReady ? "READY" : "WAITING"}</strong>
            </li>
          ))}
          <li>
            <span>Media</span>
            <strong>{snapshot.media || snapshot.provider.url ? "READY" : "WAITING"}</strong>
          </li>
          <li>
            <span>Network</span>
            <strong>{snapshot.network.connected ? "CONNECTED" : "WAITING"}</strong>
          </li>
          <li>
            <span>Sync</span>
            <strong>{snapshot.room.strictSync ? "LOCKED" : "WAITING"}</strong>
          </li>
        </ul>
        <div className="countdown">3</div>
        <button className="primary-action" type="button" onClick={onStart}>
          Enter Cinema
        </button>
      </section>
    </main>
  );
}
