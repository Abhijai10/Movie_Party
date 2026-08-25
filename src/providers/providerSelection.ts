import type { ProviderCapability, ProviderMode } from "../backend/appRuntime";

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
