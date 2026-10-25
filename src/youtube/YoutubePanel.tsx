import { ProviderContentChooser } from "../views/ProviderContentChooser";
import { getProviderCapabilities } from "../backend/appRuntime";
import { useEffect } from "react";
import type { ProviderCapability } from "../backend/appRuntime";
import { useState } from "react";
import type { AppSnapshot } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import { setYoutubeDriver } from "./YoutubeRuntime";
import { youtubeError } from "./types";
export function YoutubeSlot() {
  return (
    <div className="youtube-player-slot" data-youtube-slot aria-label="YouTube player area">
      <p>The YouTube player appears here when fully visible. Keep call and chat beside it.</p>
    </div>
  );
}
export function YoutubePanel({
  snapshot,
  onSnapshot,
}: {
  snapshot: AppSnapshot;
  onSnapshot: (next: AppSnapshot) => void;
}) {
  const [capabilities, setCapabilities] = useState<ProviderCapability[]>([]);
  useEffect(() => {
    void getProviderCapabilities()
      .then(setCapabilities)
      .catch(() => undefined);
  }, []);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<string | null>(null);
  const youtube = snapshot.youtube,
    selection = snapshot.providerContent?.selection;
  if (!youtube || selection?.provider !== "youtube") return null;
  const message = youtubeError(youtube.error);
  return (
    <section className="youtube-panel" aria-label="YouTube playback details">
      {youtube.metadata?.thumbnail && (
        <img
          width="120"
          height="70"
          src={youtube.metadata.thumbnail}
          alt={`Thumbnail for ${youtube.metadata.title}`}
          referrerPolicy="no-referrer"
        />
      )}
      <h2>{youtube.metadata?.title ?? selection.title}</h2>
      <p>{youtube.metadata?.channel ?? "Channel details are not available yet"} · YouTube</p>
      <p role="status">
        You: {snapshot.providerContent?.localState.replaceAll("_", " ")} · Partner:{" "}
        {snapshot.providerContent?.peerState.replaceAll("_", " ")}
      </p>
      {message && <p role="alert">{message}</p>}
      <p>
        {youtube.enabled
          ? "Use YouTube’s play button once to enable playback, then pause and mark Ready. Ads and playback restrictions remain controlled by YouTube."
          : "The selected video plays in your managed Chrome. Each participant uses their own authorized playback."}
      </p>
      <CinemaButton
        variant="neutral"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          setError(null);
          void setYoutubeDriver(!youtube.enabled)
            .then(onSnapshot)
            .catch(() => {
              setError(
                "Could not change players. Check Chrome and retry; playback remains paused.",
              );
            })
            .finally(() => {
              setBusy(false);
            });
        }}
      >
        {busy
          ? "Changing player…"
          : youtube.enabled
            ? "Use managed Chrome fallback"
            : "Use in-app YouTube player"}
      </CinemaButton>
      {snapshot.room.role === "HOST" && (
        <details>
          <summary>Choose another YouTube video</summary>
          <ProviderContentChooser
            inRoom
            capabilities={capabilities.filter((p) => p.id === "youtube")}
            providerId="youtube"
            onProviderChange={() => undefined}
            onOpenProvider={async () => {
              try {
                onSnapshot(await setYoutubeDriver(false));
                return true;
              } catch {
                return false;
              }
            }}
            isCreating={busy}
          />
        </details>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
