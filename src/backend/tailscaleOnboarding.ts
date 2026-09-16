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

/**
 * The join-attempt state discarded when the user escapes the Partner Connect
 * screen (F4).
 *
 * The screen is gated purely on the join failure code, and it used to replace
 * the whole app with no way out. `AppShell.goHome` is the canonical "return
 * to a clean Home" path and already clears exactly these three values, so the
 * escape button reuses it instead of introducing a second navigation route.
 * Exported so the contract can be asserted without mounting the shell.
 */
export type JoinAttemptReset = {
  joinError: null;
  joinFailureCode: null;
  pendingInvite: "";
};

export const clearedJoinAttempt: JoinAttemptReset = {
  joinError: null,
  joinFailureCode: null,
  pendingInvite: "",
};

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
