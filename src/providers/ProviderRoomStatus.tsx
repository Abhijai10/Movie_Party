import { useState } from "react";
import { retryRoomProvider, type AppSnapshot } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
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
  const content = snapshot.providerContent;
  if (!content?.selection) return null;
  return (
    <section aria-label="Provider content loading" className="space-y-3 text-sm text-white/80">
      <p role="status">
        {providerContentLabel(content.localState)} · Partner:{" "}
        {providerContentLabel(content.peerState)}
      </p>
      <p>{snapshot.provider.state.replace(/^[A-Z_]+ · /, "")}</p>
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
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
