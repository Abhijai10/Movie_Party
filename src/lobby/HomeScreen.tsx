import type { AppSnapshot } from "../backend/appRuntime";
import { pickMediaFile } from "../backend/appRuntime";
import { useState } from "react";

type HomeScreenProps = {
  snapshot: AppSnapshot;
  onCreateLocalParty: (mediaPath: string | null) => void;
  onJoin: () => void;
};

export function HomeScreen({ snapshot, onCreateLocalParty, onJoin }: HomeScreenProps) {
  const [mediaInput, setMediaInput] = useState("");
  const upcomingLabel = snapshot.media?.filename ?? snapshot.provider.url ?? "No scheduled party";
  const readiness = snapshot.network.connected ? snapshot.network.path : "Waiting for setup";

  return (
    <section className="home-screen" aria-labelledby="home-title">
      <div className="home-preview" aria-hidden="true">
        <div className="preview-screen">
          <div className="preview-letterbox" />
          <div className="preview-caption">Strict Sync</div>
        </div>
      </div>
      <div className="home-stack">
        <p className="brand">Move Party</p>
        <h1 id="home-title">What are we watching?</h1>
        <label className="url-field">
          <span>Movie file path or video URL</span>
          <input
            value={mediaInput}
            onChange={(event) => {
              setMediaInput(event.target.value);
            }}
            placeholder="Paste a local movie path or provider URL..."
          />
        </label>
        <button
          className="primary-action"
          type="button"
          onClick={() => {
            onCreateLocalParty(mediaInput);
          }}
        >
          Continue
        </button>
        <div className="divider">or</div>
        <button
          className="secondary-action"
          type="button"
          onClick={() => {
            void pickMediaFile().then((path) => {
              if (path) {
                onCreateLocalParty(path);
              }
            });
          }}
        >
          Choose Downloaded Movie
        </button>
        <button className="secondary-action" type="button" onClick={onJoin}>
          Join Existing Party
        </button>
        <section className="upcoming-strip" aria-label="Upcoming parties">
          <span>Current setup</span>
          <strong>{upcomingLabel}</strong>
          <small>{readiness}</small>
        </section>
      </div>
    </section>
  );
}
