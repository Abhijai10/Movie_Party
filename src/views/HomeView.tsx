import { Button } from "../components/Button";
import { Card } from "../components/Card";
import { CinematicBackdrop } from "../components/CinematicBackdrop";
import { EmptyState } from "../components/EmptyState";
import type { AppSnapshot } from "../backend/appRuntime";

type HomeViewProps = {
  snapshot: AppSnapshot;
  onCreate: () => void;
  onJoin: () => void;
};

export function HomeView({ snapshot, onCreate, onJoin }: HomeViewProps) {
  const upcomingLabel = snapshot.media?.filename ?? snapshot.provider.url ?? "No scheduled party";
  const readiness = snapshot.network.connected ? snapshot.network.path : "Waiting for setup";
  const providerLabel =
    snapshot.provider.providerId != null ? snapshot.provider.providerId : "No provider selected";

  return (
    <main className="app-shell home-shell">
      <CinematicBackdrop />
      <section className="home-screen" aria-labelledby="home-title">
        <div className="home-preview" aria-hidden="true">
          <div className="preview-screen">
            <div className="preview-letterbox" />
            <div className="preview-caption">Strict Sync</div>
          </div>
        </div>
        <div className="home-stack">
          <p className="brand">Move Party</p>
          <h1 id="home-title">Welcome back. What are we watching today?</h1>
          <p className="panel-copy">
            Start a private two-person cinema session with strict sync and low-distraction overlays.
          </p>
          <div className="hero-actions">
            <Button variant="primary" onClick={onCreate}>
              Create Party
            </Button>
            <Button variant="secondary" onClick={onJoin}>
              Join Party
            </Button>
          </div>
          <div className="home-meta-grid">
            <Card className="home-meta-card">
              <span>Last setup</span>
              <strong>{upcomingLabel}</strong>
              <small>{readiness}</small>
              <small>{providerLabel}</small>
            </Card>
            <Card className="home-meta-card">
              <EmptyState
                title="Recent watches"
                message="Watch history needs backend support before real entries appear here."
              />
            </Card>
          </div>
        </div>
      </section>
    </main>
  );
}
