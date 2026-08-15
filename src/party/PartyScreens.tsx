type PartyNavProps = {
  onBack: () => void;
};

type CreatePartyScreenProps = PartyNavProps & {
  onStart: () => void;
};

export function CreatePartyScreen({ onBack, onStart }: CreatePartyScreenProps) {
  return (
    <main className="app-shell centered-shell">
      <section className="setup-panel" aria-labelledby="create-title">
        <button className="text-button" type="button" onClick={onBack}>
          Back
        </button>
        <h1 id="create-title">Create Party</h1>
        <p className="panel-copy">
          Prepare a two-person room with host authority and strict synchronization.
        </p>
        <dl className="media-summary">
          <div>
            <dt>Media</dt>
            <dd>Interstellar.mkv - Local Perfect Mode</dd>
          </div>
          <div>
            <dt>Guest</dt>
            <dd>Rahul</dd>
          </div>
        </dl>
        <fieldset className="segmented-field">
          <legend>Call mode</legend>
          <button type="button" aria-pressed="true">
            Video + Voice
          </button>
          <button type="button" aria-pressed="false">
            Voice Only
          </button>
          <button type="button" aria-pressed="false">
            Off
          </button>
        </fieldset>
        <fieldset className="segmented-field">
          <legend>Controls</legend>
          <button type="button" aria-pressed="true">
            Host Only
          </button>
          <button type="button" aria-pressed="false">
            Shared Controls
          </button>
        </fieldset>
        <p className="sync-note">Strict Sync is on and locked for V1.</p>
        <button className="primary-action" type="button" onClick={onStart}>
          Start Now
        </button>
      </section>
    </main>
  );
}

type JoinPartyScreenProps = PartyNavProps & {
  onJoin: () => void;
};

export function JoinPartyScreen({ onBack, onJoin }: JoinPartyScreenProps) {
  return (
    <main className="app-shell centered-shell">
      <section className="setup-panel" aria-labelledby="join-title">
        <button className="text-button" type="button" onClick={onBack}>
          Back
        </button>
        <h1 id="join-title">Join Move Party</h1>
        <label className="url-field">
          <span>Invite code</span>
          <input placeholder="moveparty://join/..." />
        </label>
        <div className="invite-preview">
          <strong>Abhijai invited you</strong>
          <span>Interstellar - Local Movie - Strict Sync enabled</span>
        </div>
        <button className="primary-action" type="button" onClick={onJoin}>
          Join Party
        </button>
      </section>
    </main>
  );
}

type LobbyScreenProps = PartyNavProps & {
  onReady: () => void;
  onCinema: () => void;
};

export function LobbyScreen({ onBack, onReady, onCinema }: LobbyScreenProps) {
  return (
    <main className="app-shell centered-shell">
      <section className="lobby-panel" aria-labelledby="lobby-title">
        <div className="panel-header">
          <button className="text-button" type="button" onClick={onBack}>
            Back
          </button>
          <button className="text-button" type="button" onClick={onCinema}>
            Preview Cinema
          </button>
        </div>
        <h1 id="lobby-title">Interstellar</h1>
        <p className="mode-line">Local Perfect Mode</p>
        <div className="participant-grid">
          <Participant name="Abhijai" media="Ready" camera="On" mic="Muted" />
          <Participant name="Rahul" media="Preparing" camera="On" mic="Muted" />
        </div>
        <section className="network-summary" aria-label="Network">
          <strong>Network</strong>
          <span>Direct - 8.4 Mbps - Good</span>
        </section>
        <section className="buffer-summary" aria-label="Guest buffer">
          <strong>Guest Buffer</strong>
          <div className="meter">
            <span style={{ width: "72%" }} />
          </div>
          <span>2m 41s prepared</span>
        </section>
        <button className="primary-action" type="button" onClick={onReady}>
          Start When Ready
        </button>
      </section>
    </main>
  );
}

type ParticipantProps = {
  name: string;
  media: string;
  camera: string;
  mic: string;
};

function Participant({ name, media, camera, mic }: ParticipantProps) {
  return (
    <article className="participant">
      <h2>{name}</h2>
      <span>Connected</span>
      <span>Media {media}</span>
      <span>Camera {camera}</span>
      <span>Mic {mic}</span>
    </article>
  );
}

type ReadyCheckScreenProps = {
  onStart: () => void;
};

export function ReadyCheckScreen({ onStart }: ReadyCheckScreenProps) {
  return (
    <main className="app-shell centered-shell">
      <section className="ready-panel" aria-labelledby="ready-title">
        <h1 id="ready-title">Almost ready</h1>
        <ul>
          <li>
            <span>Abhijai</span>
            <strong>READY</strong>
          </li>
          <li>
            <span>Rahul</span>
            <strong>READY</strong>
          </li>
          <li>
            <span>Media</span>
            <strong>READY</strong>
          </li>
          <li>
            <span>Network</span>
            <strong>GOOD</strong>
          </li>
          <li>
            <span>Sync</span>
            <strong>LOCKED</strong>
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
