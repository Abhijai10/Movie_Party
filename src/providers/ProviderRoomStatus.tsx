import { useEffect, useState } from "react";
import {
  getProviderCapabilities,
  openProviderBrowser,
  retryRoomProvider,
  type AppSnapshot,
  type ProviderCapability,
} from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import { ProviderContentChooser } from "../views/ProviderContentChooser";
import { providerContentLabel, providerContentCanRetry } from "./roomProviderState";
import { ProviderGestureControl } from "./ProviderGestureControl";

export function ProviderRoomStatus({
  snapshot,
  onSnapshot,
}: {
  snapshot: AppSnapshot;
  onSnapshot: (next: AppSnapshot) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [capabilities, setCapabilities] = useState<ProviderCapability[]>([]);
  const content = snapshot.providerContent;
  const providerId = content?.selection?.provider ?? null;
  // YouTube owns its own panel and in-app player; this surface is the companion
  // for the managed-browser providers, which had no host controls at all.
  const companionProvider = providerId && providerId !== "youtube" ? providerId : null;
  useEffect(() => {
    if (!companionProvider) return;
    let active = true;
    void getProviderCapabilities()
      .then((value) => {
        if (active) setCapabilities(value);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [companionProvider]);
  if (!content?.selection) return null;
  const needsAttention = content.localState === "ERROR" || content.localState === "AUTH_REQUIRED";
  return (
    <section aria-label="Provider content loading" className="space-y-3 text-sm text-white/80">
      <p role="status">
        {providerContentLabel(content.localState)} · Partner:{" "}
        {providerContentLabel(content.peerState)}
      </p>
      <p>{snapshot.provider.state.replace(/^[A-Z_]+ · /, "")}</p>
      <p className="provider-companion-title">
        Watching: <strong>{content.selection.title}</strong> ·{" "}
        {content.selection.provider.toUpperCase()}
      </p>
      {content.playbackPhase === "VERIFYING" && (
        <p role="status">Checking both players before confirming playback…</p>
      )}
      {content.playbackPhase === "FAILED" && (
        <p>Playback was not confirmed. Check the selected video in Chrome, then retry Ready.</p>
      )}
      <p>
        Seek precision and synchronization depend on the provider. Provider playback is not yet
        verified across devices.
      </p>
      {content.gestureRequired && !snapshot.youtube?.enabled && <ProviderGestureControl />}
      <p>
        Use your own provider account in managed Chrome. Both players must match before playback.
      </p>
      {providerContentCanRetry(snapshot.room.role, content.localState) && (
        <CinemaButton
          variant="neutral"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            setError(null);
            void retryRoomProvider()
              .then(onSnapshot)
              .catch((reason: unknown) => {
                setError(String(reason));
              })
              .finally(() => {
                setBusy(false);
              });
          }}
        >
          {busy ? "Reopening content…" : "Reopen selected content"}
        </CinemaButton>
      )}
      {companionProvider && snapshot.room.role === "HOST" && needsAttention && (
        <CinemaButton
          variant="neutral"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            setError(null);
            void openProviderBrowser(companionProvider)
              .then(onSnapshot)
              .catch(() => {
                setError(
                  "Managed Chrome could not be reopened. Sign in on the provider's own page, then choose the title again.",
                );
              })
              .finally(() => {
                setBusy(false);
              });
          }}
        >
          Reopen in my managed Chrome
        </CinemaButton>
      )}
      {companionProvider && snapshot.room.role === "HOST" && (
        <details className="provider-companion-change">
          <summary>Change title</summary>
          <ProviderContentChooser
            inRoom
            capabilities={capabilities.filter((record) => record.id === companionProvider)}
            providerId={companionProvider}
            onProviderChange={() => undefined}
            onOpenProvider={() => Promise.resolve(true)}
            isCreating={busy}
          />
        </details>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
