type HomeScreenProps = {
  onContinue: () => void;
  onChooseMovie: () => void;
  onJoin: () => void;
};

export function HomeScreen({ onContinue, onChooseMovie, onJoin }: HomeScreenProps) {
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
          <span>Movie or video URL</span>
          <input type="url" placeholder="Paste Netflix / Prime / Hotstar / YouTube URL..." />
        </label>
        <button className="primary-action" type="button" onClick={onContinue}>
          Continue
        </button>
        <div className="divider">or</div>
        <button className="secondary-action" type="button" onClick={onChooseMovie}>
          Choose Downloaded Movie
        </button>
        <button className="secondary-action" type="button" onClick={onJoin}>
          Join Existing Party
        </button>
        <section className="upcoming-strip" aria-label="Upcoming parties">
          <span>Upcoming</span>
          <strong>Interstellar - Tonight 10:00 PM</strong>
          <small>Waiting for Rahul to come online</small>
        </section>
      </div>
    </section>
  );
}
