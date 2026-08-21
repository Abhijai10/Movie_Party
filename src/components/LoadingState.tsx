type LoadingStateProps = {
  title: string;
  message: string;
};

import { CinematicBackdrop } from "./CinematicBackdrop";

export function LoadingState({ title, message }: LoadingStateProps) {
  return (
    <main className="app-shell centered-shell">
      <CinematicBackdrop />
      <section className="setup-panel" aria-labelledby="loading-title">
        <h1 id="loading-title">{title}</h1>
        <p className="panel-copy">{message}</p>
      </section>
    </main>
  );
}
