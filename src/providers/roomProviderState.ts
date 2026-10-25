import type { AppSnapshot } from "../backend/appRuntime";
import type { ProviderAction } from "../backend/appRuntime";
type LoadingState = NonNullable<AppSnapshot["providerContent"]>["localState"];
export function providerContentLabel(state: LoadingState): string {
  const labels: Record<LoadingState, string> = {
    UNSELECTED: "Waiting for selection",
    OPENING: "Opening selected content",
    CONTENT_CONFIRMED: "Content identity confirmed",
    PLAYBACK_READY: "Video ready",
    ERROR: "Content needs attention",
    AUTH_REQUIRED: "Sign in in Chrome",
  };
  return labels[state];
}
/**
 * Retry re-opens the host's chosen content in the local managed browser, so the
 * backend accepts it for a guest only (`retry_room_provider` refuses a host).
 * The host's recovery is its own browser or a different title instead.
 */
export function providerContentCanRetry(role: string, state: LoadingState): boolean {
  return role === "GUEST" && (state === "ERROR" || state === "AUTH_REQUIRED");
}

export type ProviderEvidence = {
  live: number;
  automated: number;
  unverified: number;
  total: number;
};

/**
 * Tiers an honest provider summary must separate. `supportLevel` is a coarse
 * label; the operation list is what actually carries evidence, so the UI counts
 * from it instead of repeating the summary back.
 */
export function providerEvidence(actions: ProviderAction[]): ProviderEvidence {
  const evidence = actions.reduce<ProviderEvidence>(
    (tally, action) => {
      if (action.evidence === "LIVE_VERIFIED") tally.live += 1;
      else if (action.evidence === "AUTOMATED_ONLY") tally.automated += 1;
      else tally.unverified += 1;
      return tally;
    },
    { live: 0, automated: 0, unverified: 0, total: actions.length },
  );
  return evidence;
}

/** Naming the tier is required; colour alone cannot carry status. */
export function providerEvidenceTierLabel(evidence: ProviderAction["evidence"]): string {
  return evidence === "LIVE_VERIFIED"
    ? "Live-verified"
    : evidence === "AUTOMATED_ONLY"
      ? "Automated tests only"
      : "Unverified";
}

export function providerEvidenceLabel(evidence: ProviderEvidence): string {
  if (!evidence.total) return "No provider operations are evidenced yet.";
  const parts = [
    evidence.live ? `${String(evidence.live)} live-verified` : null,
    evidence.automated ? `${String(evidence.automated)} automated only` : null,
    evidence.unverified ? `${String(evidence.unverified)} unverified` : null,
  ].filter(Boolean);
  const headline = parts.join(" · ");
  return evidence.live === 0
    ? `${headline} — this provider has never been driven on its real page.`
    : `${headline} across ${String(evidence.total)} operations.`;
}
