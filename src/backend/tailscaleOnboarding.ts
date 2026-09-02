import type { TailscaleState } from "./appRuntime";

export const TAILSCALE_POLL_INTERVAL_READY_MS = 15_000;
export const TAILSCALE_POLL_INTERVAL_SETUP_MS = 4_000;

export function isTailscaleReady(state: TailscaleState): boolean {
  return state === "READY";
}

export function shouldShowTailscaleSetup(state: TailscaleState): boolean {
  return state !== "READY";
}

export function shouldShowPartnerConnectView(joinFailureCode: string | null): boolean {
  return joinFailureCode === "MP-NET-TS-005";
}

export function pollIntervalMs(isReady: boolean): number {
  return isReady
    ? TAILSCALE_POLL_INTERVAL_READY_MS
    : TAILSCALE_POLL_INTERVAL_SETUP_MS;
}

export function refreshLabelFor(state: TailscaleState): string {
  return state === "NEEDS_LOGIN" ? "I've signed in" : "Check again";
}

/**
 * Runs one asynchronous task at a time. While a task is in flight, further
 * calls are dropped (they resolve immediately to `undefined`) instead of
 * overlapping the running request.
 */
export function createSingleFlight<T = undefined>(): (
  task: () => Promise<T>,
) => Promise<T | undefined> {
  let inFlight = false;
  return async (task) => {
    if (inFlight) return undefined;
    inFlight = true;
    try {
      return await task();
    } finally {
      inFlight = false;
    }
  };
}
