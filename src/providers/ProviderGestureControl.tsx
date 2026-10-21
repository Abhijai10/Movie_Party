import { useState } from "react";
import { focusProviderPlayback } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";

export function ProviderGestureControl() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <div className="space-y-3">
      <p>
        Chrome needs your permission to play. Open the selected video and click its play button
        once, then pause it. Return here and retry Ready.
      </p>
      <CinemaButton
        variant="neutral"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          setError(null);
          void focusProviderPlayback()
            .catch((reason: unknown) => {
              setError(String(reason));
            })
            .finally(() => {
              setBusy(false);
            });
        }}
      >
        {busy ? "Opening Chrome…" : "Click to enable playback"}
      </CinemaButton>
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
