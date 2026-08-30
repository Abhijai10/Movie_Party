import type {
  ProviderCapability,
  ProviderMode,
  ProviderReadiness,
} from "../backend/appRuntime";

export function providerModeStatus(
  provider: ProviderCapability | null,
  mode: ProviderMode,
): { canPrepare: boolean; message: string | null } {
  if (!provider) {
    return { canPrepare: false, message: "Choose a supported streaming provider." };
  }
  if (mode === "PROVIDER_SYNC") {
    return provider.syncAvailable
      ? { canPrepare: true, message: null }
      : { canPrepare: false, message: "Provider Sync is not available for this provider." };
  }
  return provider.sharedAvailable
    ? { canPrepare: true, message: null }
    : { canPrepare: false, message: provider.sharedReason };
}

export function isGenericLink(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:";
  } catch {
    return false;
  }
}

// ── Provider readiness state machine ────────────────────────────────────

/**
 * Determines whether the user can perform the next action given the current
 * readiness state. Returns a label and whether the action button is enabled.
 */
export function providerReadinessAction(
  readiness: ProviderReadiness,
): { label: string; enabled: boolean; next: "open" | "check" | "title" | "create" | null } {
  switch (readiness) {
    case "NOT_STARTED":
      return { label: "Open provider", enabled: true, next: "open" };
    case "LAUNCHING":
      return { label: "Launching provider...", enabled: false, next: null };
    case "LOGIN_REQUIRED":
      return { label: "I've signed in — Check status", enabled: true, next: "check" };
    case "READY":
      return { label: "Enter title", enabled: true, next: "title" };
    case "NAVIGATING":
      return { label: "Check playback", enabled: true, next: "check" };
    case "PLAYBACK_READY":
      return { label: "Create party", enabled: true, next: "create" };
    case "UNAVAILABLE":
      return { label: "Provider unavailable", enabled: false, next: null };
    case "ERROR":
      return { label: "Provider error", enabled: false, next: null };
  }
}

/**
 * Returns a user-facing status message for the current readiness state.
 */
export function providerReadinessMessage(
  readiness: ProviderReadiness,
  displayName: string,
): string {
  switch (readiness) {
    case "NOT_STARTED":
      return `Open ${displayName} in the managed browser to get started.`;
    case "LAUNCHING":
      return `Starting the managed browser for ${displayName}...`;
    case "LOGIN_REQUIRED":
      return `Sign in to ${displayName} on its own page in the opened browser window. Movie Party never receives your password.`;
    case "READY":
      return `Authenticated and ready. Enter a movie or show title to continue.`;
    case "NAVIGATING":
      return `Navigating to the title in ${displayName}. Pick the result in the browser window, then check playback.`;
    case "PLAYBACK_READY":
      return `Playback is ready. Create the party room to share with your guest.`;
    case "UNAVAILABLE":
      return `${displayName} is not available right now.`;
    case "ERROR":
      return `An error occurred with ${displayName}. Try opening the provider again.`;
  }
}

/**
 * Provider home page URL opened in the managed browser.
 */
export function providerHomeUrl(providerId: string): string {
  const homes: Record<string, string> = {
    youtube: "https://www.youtube.com/",
    netflix: "https://www.netflix.com/browse",
    prime: "https://www.primevideo.com/",
    jiohotstar: "https://www.hotstar.com/in",
  };
  return homes[providerId] ?? "https://www.youtube.com/";
}

/**
 * Provider search URL for a user-entered title. Returns null for empty
 * titles. The provider's own search page is authoritative; Movie Party
 * never scrapes the catalogue.
 */
export function providerSearchUrl(
  providerId: string,
  title: string,
): string | null {
  const query = title.trim();
  if (!query) return null;
  const encoded = encodeURIComponent(query);
  const searches: Record<string, string> = {
    youtube: `https://www.youtube.com/results?search_query=${encoded}`,
    netflix: `https://www.netflix.com/search?q=${encoded}`,
    prime: `https://www.primevideo.com/search/ref=atv_nb_sr?phrase=${encoded}`,
    jiohotstar: `https://www.hotstar.com/in/search?q=${encoded}`,
  };
  return searches[providerId] ?? null;
}

/**
 * Returns true when the readiness state allows the party room to be created.
 * The room must not be created while login or playback preparation is still
 * required.
 */
export function canCreatePartyWithReadiness(readiness: ProviderReadiness): boolean {
  return readiness === "PLAYBACK_READY";
}