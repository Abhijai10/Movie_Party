import type { AppSnapshot } from "../backend/appRuntime";
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
export function providerContentCanRetry(role: string, state: LoadingState): boolean {
  return role === "GUEST" && (state === "ERROR" || state === "AUTH_REQUIRED");
}
