import { pickMediaFile } from "../backend/appRuntime";
import { Button } from "../components/Button";
import { Card } from "../components/Card";
import { CinematicBackdrop } from "../components/CinematicBackdrop";
import { ErrorState } from "../components/ErrorState";
import { GlassPanel } from "../components/GlassPanel";
import { TextField } from "../components/TextField";
import { useState } from "react";

type SourceKind = "LOCAL" | "PROVIDER";
type SourceIcon = "film" | "play";
type SetupStage = "SOURCE" | "PREPARE";

type CreatePartyViewProps = {
  isCreating: boolean;
  error: string | null;
  onBack: () => void;
  onCreateLocalParty: (mediaPath: string | null) => Promise<boolean>;
};

export function CreatePartyView({
  isCreating,
  error,
  onBack,
  onCreateLocalParty,
}: CreatePartyViewProps) {
  const [mediaInput, setMediaInput] = useState("");
  const [sourceKind, setSourceKind] = useState<SourceKind>("LOCAL");
  const [pickedFile, setPickedFile] = useState("");
  const [stage, setStage] = useState<SetupStage>("SOURCE");
  const selectedMovieName = fileLabel(pickedFile || mediaInput);
  const hasSelection = Boolean((pickedFile || mediaInput).trim());

  return (
    <main className="app-shell centered-shell flow-shell">
      <CinematicBackdrop />
      <GlassPanel className="setup-panel setup-wizard" labelledBy="create-title">
        <Button className="back-button" variant="ghost" onClick={onBack}>
          ← Back
        </Button>
        <div className="wizard-heading">
          <p className="brand">New cinema</p>
          <h1 id="create-title">Set the room.</h1>
          <p className="panel-copy">Choose the film first. The invitation comes once your room is ready.</p>
        </div>
        <ErrorState
          message={error}
          onRetry={() => {
            if (hasSelection) {
              void onCreateLocalParty(sourceKind === "LOCAL" ? pickedFile || mediaInput : mediaInput);
            }
          }}
        />

        <ol className="step-indicator" aria-label="Create party steps">
          <li className={stage === "SOURCE" ? "active" : "complete"}><span>1</span>Choose source</li>
          <li className={stage === "PREPARE" ? "active" : ""}><span>2</span>Prepare room</li>
          <li>Invite Friend</li>
        </ol>

        {stage === "SOURCE" ? (
          <div className="setup-source-stage">
            <div className="setup-source-column">
              <div className="flow-section-label">
                <span>Choose your source</span>
                <small>Downloaded media or a supported provider link.</small>
              </div>
              <div className="source-grid" role="radiogroup" aria-label="Movie source">
                <SourceCard title="Local movie" icon="film" body="A downloaded film on this Mac or PC." selected={sourceKind === "LOCAL"} onSelect={() => setSourceKind("LOCAL")} />
                <SourceCard title="Streaming provider" icon="play" body="A supported video URL you can open locally." selected={sourceKind === "PROVIDER"} onSelect={() => setSourceKind("PROVIDER")} />
              </div>
            </div>
            <div className="setup-preview-column">
              {sourceKind === "LOCAL" ? (
                <div className="movie-picker">
                  <button className="movie-picker-action" type="button" disabled={isCreating} onClick={() => { void pickMediaFile().then((path) => { if (path) { setPickedFile(path); setMediaInput(path); } }); }}>
                    <span className="movie-picker-icon" aria-hidden="true" />
                    <span><strong>{selectedMovieName || "Choose downloaded movie"}</strong><small>{selectedMovieName ? "Ready to prepare your room" : "Select a file from this device"}</small></span>
                  </button>
                  <TextField label="Or paste a local path" value={pickedFile || mediaInput} disabled={isCreating} onChange={(event) => { setPickedFile(""); setMediaInput(event.target.value); }} placeholder="/Users/you/Movies/movie.mkv" />
                </div>
              ) : (
                <TextField label="Provider URL" value={mediaInput} disabled={isCreating} onChange={(event) => setMediaInput(event.target.value)} placeholder="https://..." helperText="The provider opens from your own signed-in browser profile." />
              )}
              <Button variant="primary" disabled={!hasSelection || isCreating} onClick={() => setStage("PREPARE")}>Continue to room setup</Button>
            </div>
          </div>
        ) : (
          <section className="room-preparation" aria-labelledby="prepare-title">
            <div className="movie-selected-card">
              <span className="movie-selected-poster" aria-hidden="true" />
              <div><span>Selected movie</span><strong>{selectedMovieName || "Provider source"}</strong><small>{sourceKind === "LOCAL" ? "Local playback" : "Provider sync"}</small></div>
            </div>
            <div className="prepare-copy"><p id="prepare-title">Your room is almost ready.</p><span>Move Party will create the private room, then show your invite inside the lobby.</span></div>
            <div className="wizard-actions"><Button variant="ghost" disabled={isCreating} onClick={() => setStage("SOURCE")}>Change source</Button><Button variant="primary" isLoading={isCreating} onClick={() => { void onCreateLocalParty(sourceKind === "LOCAL" ? pickedFile || mediaInput : mediaInput); }}>Create cinema room</Button></div>
          </section>
        )}
      </GlassPanel>
    </main>
  );
}

function fileLabel(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return "";
  }

  const parts = trimmed.split(/[\\/]/);
  return parts.at(-1) ?? trimmed;
}

function SourceCard({
  title,
  icon,
  body,
  selected,
  onSelect,
}: {
  title: string;
  icon: SourceIcon;
  body: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <Card className={selected ? "source-card selected" : "source-card"}>
      <button type="button" role="radio" aria-checked={selected} onClick={onSelect}>
        <span className={`source-icon source-icon-${icon}`} aria-hidden="true" />
        <strong>{title}</strong>
        <span>{body}</span>
      </button>
    </Card>
  );
}
