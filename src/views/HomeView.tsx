import { Button } from "../components/Button";
import { useReducedMotion } from "../hooks/useReducedMotion";
import { lazy, Suspense } from "react";

const Silk = lazy(() => import("../components/Silk").then((module) => ({ default: module.Silk })));
const FilmReelScene = lazy(() =>
  import("../components/FilmReelScene").then((module) => ({ default: module.FilmReelScene })),
);

type HomeViewProps = {
  onCreate: () => void;
  onJoin: () => void;
};

export function HomeView({ onCreate, onJoin }: HomeViewProps) {
  const prefersReducedMotion = useReducedMotion();
  return (
    <main className="app-shell home-shell">
      {!prefersReducedMotion ? (
        <div className="home-silk" aria-hidden="true">
          <Suspense fallback={null}>
            <Silk speed={0.34} scale={1.24} color="#5B21FF" noiseIntensity={0.52} rotation={0.18} />
          </Suspense>
        </div>
      ) : null}
      <section className="home-screen" aria-labelledby="home-title">
        <div className="home-intro">
          <div className="home-brand-lockup">
            <span className="home-brand-mark" aria-hidden="true" />
            <p>Move Party</p>
          </div>
          <p className="home-kicker">Welcome back</p>
          <h1 id="home-title">What are we watching today?</h1>
          <p className="home-summary">
            A private movie night where playback stays with both of you.
          </p>
          <div className="hero-actions">
            <Button variant="primary" onClick={onCreate}>
              Create Party
            </Button>
            <Button variant="secondary" onClick={onJoin}>
              Join Party
            </Button>
          </div>
          <p className="home-footnote">
            <span aria-hidden="true" /> Strict sync enabled for every room.
          </p>
        </div>
        <Suspense fallback={<div className="film-scene-fallback" aria-hidden="true" />}>
          <FilmReelScene reducedMotion={prefersReducedMotion} />
        </Suspense>
      </section>
    </main>
  );
}
