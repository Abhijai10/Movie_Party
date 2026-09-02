import type { TailscaleState } from "./appRuntime";

export const TAILSCALE_POLL_INTERVAL_READY_MS = 15_000;
export const TAILSCALE_POLL_INTERVAL_SETUP_MS = 4_000;

export function isTailscaleReady(state: TailscaleState): boolean {
  return state === "READY";
}

export function pollIntervalMs(isReady: boolean): number {
  return isReady
    ? TAILSCALE_POLL_INTERVAL_READY_MS
    : TAILSCALE_POLL_INTERVAL_SETUP_MS;
}

export function refreshLabelFor(state: TailscaleState): string {
  return state === "NEEDS_LOGIN" ? "I've signed in" : "Check again";
}