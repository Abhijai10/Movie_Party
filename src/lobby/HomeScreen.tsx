export function HomeScreen() {
  return (
    <section className="home-screen" aria-labelledby="home-title">
      <div className="home-stack">
        <p className="brand">Move Party</p>
        <h1 id="home-title">What are we watching?</h1>
        <label className="url-field">
          <span>Movie or video URL</span>
          <input type="url" placeholder="Paste Netflix / Prime / Hotstar / YouTube URL..." />
        </label>
        <button className="primary-action" type="button">
          Continue
        </button>
        <div className="divider">or</div>
        <button className="secondary-action" type="button">
          Choose Downloaded Movie
        </button>
        <button className="secondary-action" type="button">
          Join Existing Party
        </button>
      </div>
    </section>
  );
}
