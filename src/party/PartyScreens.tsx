import type { AppSnapshot, ParticipantSnapshot } from "../backend/appRuntime";
import { useState } from "react";

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
  snapshot: AppSnapshot;
  onJoin: (inviteCode: string) => void;
};

export function JoinPartyScreen({ onBack, onJoin }: JoinPartyScreenProps) {
  const [inviteCode, setInviteCode] = useState("");

  return (
    <main className="app-shell centered-shell">
      <section className="setup-panel" aria-labelledby="join-title">
        <button className="text-button" type="button" onClick={onBack}>
          Back
        </button>
        <h1 id="join-title">Join Move Party</h1>
        <label className="url-field">
          <span>Invite code</span>
          <input
            value={inviteCode}
            onChange={(event) => {
              setInviteCode(event.target.value);
            }}
            placeholder="moveparty://join/..."
          />
        </label>
        <div className="invite-preview">
          <strong>Invite preview</strong>
          <span>{inviteCode.trim().length > 0 ? inviteCode : "Waiting for invite code"}</span>
        </div>
        <button
          className="primary-action"
          type="button"
          onClick={() => {
            onJoin(inviteCode);
          }}
        >
          Join Party
        </button>
      </section>
    </main>
  );
}

type LobbyScreenProps = PartyNavProps & {
  snapshot: AppSnapshot;
  onReady: () => void;
  onCinema: () => void;
  onToggleSharedControls: (enabled: boolean) => void;
};

export function LobbyScreen({
  snapshot,
  onBack,
  onReady,
  onCinema,
  onToggleSharedControls,
}: LobbyScreenProps) {
  const title = snapshot.media?.filename ?? snapshot.provider.url ?? "Local party";
  const mode = snapshot.provider.mode.replaceAll("_", " ");
  const bufferPercent = `${String(snapshot.buffer.percent)}%`;
  const bufferSeconds = Math.round(snapshot.buffer.guestBufferAheadMs / 1_000);
  const network = snapshot.network.connected
    ? `${snapshot.network.transport} - ${snapshot.network.path}`
    : snapshot.network.path;

  const isHost = snapshot.room.role === "HOST";
  const sharedControls = snapshot.room.sharedControls;

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
        <h1 id="lobby-title">{title}</h1>
        <p className="mode-line">{mode}</p>
        <div className="participant-grid">
          {snapshot.participants.map((participant) => (
            <Participant key={participant.id} participant={participant} />
          ))}
        </div>

        <fieldset className="segmented-field">
          <legend>Controls</legend>
          <button
            type="button"
            aria-pressed={!sharedControls}
            disabled={!isHost}
            onClick={() => {
              onToggleSharedControls(false);
            }}
          >
            Host Only
          </button>
          <button
            type="button"
            aria-pressed={sharedControls}
            disabled={!isHost}
            onClick={() => {
              onToggleSharedControls(true);
            }}
          >
            Shared Controls
          </button>
        </fieldset>

        <section className="network-summary" aria-label="Network">
          <strong>Network</strong>
          <span>{network}</span>
        </section>
        <section className="buffer-summary" aria-label="Guest buffer">
          <strong>Guest Buffer</strong>
          <div className="meter">
            <span style={{ width: bufferPercent }} />
          </div>
          <span>{bufferSeconds}s prepared</span>
        </section>
        <button className="primary-action" type="button" onClick={onReady}>
          Start When Ready
        </button>
      </section>
    </main>
  );
}

type ParticipantProps = {
  participant: ParticipantSnapshot;
};

function Participant({ participant }: ParticipantProps) {
  return (
    <article className="participant">
      <h2>{participant.displayName}</h2>
      <span>{participant.connected ? "Connected" : "Disconnected"}</span>
      <span>Media {participant.mediaReady ? "Ready" : "Preparing"}</span>
      <span>Camera {participant.cameraEnabled ? "On" : "Off"}</span>
      <span>Mic {participant.microphoneEnabled ? "On" : "Muted"}</span>
    </article>
  );
}

type ReadyCheckScreenProps = {
  snapshot: AppSnapshot;
  onStart: () => void;
};

export function ReadyCheckScreen({ snapshot, onStart }: ReadyCheckScreenProps) {
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
