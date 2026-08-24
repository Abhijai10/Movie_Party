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

  return (
    <section className="presentation-overlay" aria-live="polite">
      <h2>
        {presentation.mode === "UNAVAILABLE"
          ? "Player unavailable"
          : "Player controlled outside the app"}
      </h2>
      <p>{presentation.message}</p>
      <small>{presentation.bridge}</small>
    </section>
  );
}
