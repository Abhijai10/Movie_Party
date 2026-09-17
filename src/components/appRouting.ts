import { shouldShowPartnerConnectView } from "../backend/tailscaleOnboarding";

/**
 * Which screen the shell renders (F55).
 *
 * This is the application's routing decision, extracted from the if-chain that
 * used to live inline in `AppShell`. The extraction is deliberately
 * behaviour-preserving — the branch order below is the branch order that was
 * there before, and the tests pin that order — but it makes routing, which is
 * the one thing a broken change would brick the app with, actually testable.
 *
 * The precedence is not arbitrary and several entries are easy to get wrong:
 *
 *  - The boot gates come first: without a snapshot, or without a Tailscale
 *    readiness reading, there is nothing else to route on.
 *  - `devScreen` (a development override) outranks every `localScreen`.
 *  - `closePrompt` (§69 window-close-during-party) sits BETWEEN the local
 *    screens and `CREATE_PARTY` — so it outranks Create Party but is outranked
 *    by Settings / Schedule / Friends / First Run. That ordering is intentional
 *    (those are modal-free surfaces) and is pinned by test.
 *  - The backend screen is only consulted after every local surface, and
 *    `PARTY_END_CONFIRM` is checked last so it cannot shadow a real screen.
 */
export type AppScreen =
  | "BOOT_STARTING"
  | "BOOT_SLOW"
  | "TAILSCALE_CHECKING"
  | "TAILSCALE_SETUP"
  | "PARTNER_CONNECT"
  /** The dev override (`devScreen === "HOME"`). */
  | "HOME"
  /** The default landing — Home *with* the §52 retention prompt over it. */
  | "HOME_FALLBACK"
  | "FIRST_RUN"
  | "SETTINGS"
  | "SCHEDULE"
  | "FRIENDS"
  /** §69 window-close-during-party confirmation. */
  | "CLOSE_PROMPT"
  /** The backend's own `PARTY_END_CONFIRM` screen. */
  | "END_PARTY_CONFIRM"
  | "CREATE_PARTY"
  | "JOIN_PARTY"
  | "LOBBY"
  | "READY_CHECK"
  | "CINEMA";

export type AppRouteInput = {
  /** False until the runtime answers with a first snapshot. */
  hasSnapshot: boolean;
  /** Seconds spent waiting for that first snapshot (drives the boot copy). */
  secondsWaiting: number;
  /** False until the Tailscale readiness probe has answered at all. */
  hasTailscaleReadiness: boolean;
  /** Whether the tailnet is usable right now. */
  tailscaleReady: boolean;
  /** Set when a join failed with a stable MP code (drives Partner Connect). */
  joinFailureCode: string | null;
  /** Shell-owned screen — outranks the backend screen. */
  localScreen: string | null;
  /** Development override — outranks everything except the boot gates. */
  devScreen: string | null;
  /** §69: the window-close-during-party confirmation is showing. */
  closePromptVisible: boolean;
  /** The backend-reported screen. */
  backendScreen: string;
};

/** Boot copy escalates past this many seconds (honest, not an eternal spinner). */
export const BOOT_SLOW_AFTER_SECONDS = 8;

export function resolveAppScreen(input: AppRouteInput): AppScreen {
  // 1–2. Boot gates: nothing else can be decided yet.
  if (!input.hasSnapshot) {
    return input.secondsWaiting >= BOOT_SLOW_AFTER_SECONDS ? "BOOT_SLOW" : "BOOT_STARTING";
  }
  if (!input.hasTailscaleReadiness) {
    return "TAILSCALE_CHECKING";
  }

  // 3. Tailscale must be usable before anything party-related.
  if (!input.tailscaleReady) {
    return "TAILSCALE_SETUP";
  }

  // 4. A failed join with a stable code gets the Partner Connect escape hatch
  //    (F4) rather than a dead end.
  if (shouldShowPartnerConnectView(input.joinFailureCode)) {
    return "PARTNER_CONNECT";
  }

  // 5. The dev override wins over every local screen.
  if (input.devScreen === "HOME") {
    return "HOME";
  }

  // 6–9. Shell-owned surfaces.
  if (input.localScreen === "FIRST_RUN") {
    return "FIRST_RUN";
  }
  if (input.localScreen === "SETTINGS") {
    return "SETTINGS";
  }
  if (input.localScreen === "SCHEDULE") {
    return "SCHEDULE";
  }
  if (input.localScreen === "FRIENDS") {
    return "FRIENDS";
  }

  // 10. §69 close-during-party confirmation — outranks Create Party, but not
  //     the modal-free local surfaces above. (Distinct from the backend's own
  //     PARTY_END_CONFIRM screen below: the two have different handlers.)
  if (input.closePromptVisible) {
    return "CLOSE_PROMPT";
  }

  // 11. Create Party.
  if (input.localScreen === "CREATE_PARTY" || input.devScreen === "CREATE") {
    return "CREATE_PARTY";
  }

  // 12–16. Backend-driven party screens.
  if (input.backendScreen === "JOIN_PARTY" || input.devScreen === "JOIN") {
    return "JOIN_PARTY";
  }
  if (input.backendScreen === "LOBBY" || input.devScreen === "LOBBY") {
    return "LOBBY";
  }
  if (input.backendScreen === "READY_CHECK" || input.devScreen === "READY") {
    return "READY_CHECK";
  }
  if (input.backendScreen === "CINEMA" || input.devScreen === "CINEMA") {
    return "CINEMA";
  }
  if (input.backendScreen === "PARTY_END_CONFIRM") {
    return "END_PARTY_CONFIRM";
  }

  // 17. Fallback: Home with the §52 retention prompt rendered over it.
  return "HOME_FALLBACK";
}
