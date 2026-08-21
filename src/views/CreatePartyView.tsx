import { pickMediaFile } from "../backend/appRuntime";
import { Button } from "../components/Button";
import { Card } from "../components/Card";
import { CinematicBackdrop } from "../components/CinematicBackdrop";
import { ErrorState } from "../components/ErrorState";
import { GlassPanel } from "../components/GlassPanel";
import { TextField } from "../components/TextField";
import type { AppSnapshot } from "../backend/appRuntime";
import { useState } from "react";

type SourceKind = "LOCAL" | "PROVIDER";

type CreatePartyViewProps = {
  snapshot: AppSnapshot;
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

  return (
    <main className="app-shell centered-shell flow-shell">
      <CinematicBackdrop />
      <GlassPanel className="setup-panel flow-panel" labelledBy="create-title">
        <Button variant="ghost" onClick={onBack}>
          Back
        </Button>
        <p className="brand">Create Party</p>
        <h1 id="create-title">Choose your movie source</h1>
        <p className="panel-copy">
          Start with a downloaded movie or a supported provider URL. Move Party will only create
          the room after the source is selected.
        </p>
        <ErrorState message={error} />

        <div className="source-grid" role="radiogroup" aria-label="Movie source">
          <SourceCard
            title="Local Movie"
            body="Watch a downloaded movie from this device."
            selected={sourceKind === "LOCAL"}
            onSelect={() => {
              setSourceKind("LOCAL");
            }}
          />
          <SourceCard
            title="Streaming Provider"
            body="Use a supported provider source without changing backend behavior."
            selected={sourceKind === "PROVIDER"}
            onSelect={() => {
              setSourceKind("PROVIDER");
            }}
          />
        </div>

        {sourceKind === "LOCAL" ? (
          <div className="flow-stack">
            <TextField
              label="Movie file path"
              value={pickedFile || mediaInput}
              disabled={isCreating}
              onChange={(event) => {
                setPickedFile("");
                setMediaInput(event.target.value);
              }}
              placeholder="/Users/you/Movies/movie.mkv"
              helperText="Choose a file or paste a local path."
            />
            <Button
              variant="secondary"
              disabled={isCreating}
              onClick={() => {
                void pickMediaFile().then((path) => {
                  if (path) {
                    setPickedFile(path);
                    setMediaInput(path);
                  }
                });
              }}
            >
              Choose Downloaded Movie
            </Button>
          </div>
        ) : (
          <TextField
            label="Provider URL"
            value={mediaInput}
            disabled={isCreating}
            onChange={(event) => {
              setMediaInput(event.target.value);
            }}
            placeholder="https://..."
            helperText="Paste a supported provider video URL."
          />
        )}

        <Button
          variant="primary"
          isLoading={isCreating}
          onClick={() => {
            void onCreateLocalParty(sourceKind === "LOCAL" ? pickedFile || mediaInput : mediaInput);
          }}
        >
          Continue
        </Button>
      </GlassPanel>
    </main>
  );
}

function SourceCard({
  title,
  body,
  selected,
  onSelect,
}: {
  title: string;
  body: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <Card className={selected ? "source-card selected" : "source-card"}>
      <button type="button" role="radio" aria-checked={selected} onClick={onSelect}>
        <strong>{title}</strong>
        <span>{body}</span>
      </button>
    </Card>
  );
}
