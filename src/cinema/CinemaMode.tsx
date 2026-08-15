type CinemaModeProps = {
  onLeave: () => void;
};

export function CinemaMode({ onLeave }: CinemaModeProps) {
  return (
    <main className="cinema-shell" aria-label="Cinema mode">
      <section className="movie-surface" aria-label="Movie">
        <div className="movie-frame">
          <div className="movie-light" />
          <span>Interstellar</span>
        </div>
        <article className="camera-card" aria-label="Rahul camera">
          <strong>Rahul</strong>
          <span>Camera on - Mic muted</span>
        </article>
        <article className="chat-bubble" aria-label="Recent chat message">
          <strong>Rahul</strong>
          <span>BRO WHAT</span>
        </article>
        <section className="buffer-overlay" aria-live="polite">
          <h1>Paused to keep you together</h1>
          <p>Rahul is buffering</p>
          <div className="meter">
            <span style={{ width: "64%" }} />
          </div>
          <small>Camera and chat are still available.</small>
        </section>
        <section className="reconnect-overlay" aria-live="polite">
          <h2>Rahul disconnected.</h2>
          <p>The movie has been paused. Reconnecting...</p>
        </section>
        <nav className="control-dock" aria-label="Cinema controls">
          <button type="button" aria-label="Back 10 seconds">
            -10
          </button>
          <button type="button" aria-label="Pause for both participants">
            Pause
          </button>
          <button type="button" aria-label="Forward 10 seconds">
            +10
          </button>
          <button type="button" aria-label="Mute microphone">
            Mic
          </button>
          <button type="button" aria-label="Toggle camera">
            Camera
          </button>
          <button type="button" aria-label="Open chat">
            Chat
          </button>
          <button type="button" aria-label="Send reaction">
            React
          </button>
          <button type="button" onClick={onLeave} aria-label="Open party menu">
            More
          </button>
        </nav>
      </section>
    </main>
  );
}
