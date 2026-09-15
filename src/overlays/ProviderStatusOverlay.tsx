import type { AppSnapshot } from "../backend/appRuntime";

type ProviderStatusOverlayProps = {
  snapshot: AppSnapshot;
};

export function ProviderStatusOverlay({ snapshot }: ProviderStatusOverlayProps) {
  const presentation = snapshot.player.presentation;
  const shouldShowPresentationNotice =
    presentation.mode !== "EMBEDDED_NATIVE" && snapshot.media != null;

  if (!shouldShowPresentationNotice) {
    return null;
  }

  // The player reports "not initialized" while Cinema is still ATTACHING
  // the native video surface — that is the normal pre-attach transient,
  // not a failure. Surfacing the raw backend line ("libmpv is not
  // initialized.") reads as a crash to users; the honest-but-calm copy
  // below covers the transition. A real failure arrives with the
  // PLAYER_ERROR state and keeps its diagnostic message.
  const isAttaching =
    presentation.mode === "UNAVAILABLE" &&
    snapshot.player.state !== "PLAYER_ERROR" &&
    snapshot.player.errorMessage == null;

  return (
    <section className="presentation-overlay" aria-live="polite">
      <h2>
        {presentation.mode === "UNAVAILABLE"
          ? isAttaching
            ? "Setting up the player"
            : "Preparing player"
          : "Player controlled outside the app"}
      </h2>
      <p>
        {isAttaching
          ? "Connecting the movie window — this takes a moment."
          : presentation.message}
      </p>
      {isAttaching ? null : <small>{presentation.bridge}</small>}
    </section>
  );
}
