import { describe, expect, it } from "vitest";
import { BOOT_SLOW_AFTER_SECONDS, resolveAppScreen, type AppRouteInput, type AppScreen } from "./appRouting";

/**
 * F55: the application shell's routing decision.
 *
 * These pin the branch ORDER, which is the part that is easy to break and
 * expensive to get wrong — a wrong route is an unusable app. The order was
 * taken from the if-chain that used to live inline in `AppShell`.
 */

/** A ready-to-use route input; every test overrides only what it is about.
 *
 * The default `backendScreen` is the backend's own "HOME", which falls through
 * the whole chain to the final fallback — i.e. the default route is
 * `HOME_FALLBACK`, not `HOME`. (`HOME` is only the dev override.) */
function route(overrides: Partial<AppRouteInput> = {}): AppScreen {
  return resolveAppScreen({
    hasSnapshot: true,
    secondsWaiting: 0,
    hasTailscaleReadiness: true,
    tailscaleReady: true,
    joinFailureCode: null,
    localScreen: null,
    devScreen: null,
    closePromptVisible: false,
    backendScreen: "HOME",
    ...overrides,
  });
}

describe("boot gates", () => {
  it("shows the starting screen until the runtime answers", () => {
    expect(route({ hasSnapshot: false })).toBe("BOOT_STARTING");
  });

  it("escalates to the honest slow copy past the threshold", () => {
    expect(route({ hasSnapshot: false, secondsWaiting: BOOT_SLOW_AFTER_SECONDS - 1 })).toBe(
      "BOOT_STARTING",
    );
    expect(route({ hasSnapshot: false, secondsWaiting: BOOT_SLOW_AFTER_SECONDS })).toBe("BOOT_SLOW");
  });

  it("outranks every other input — nothing can be routed without a snapshot", () => {
    expect(
      route({
        hasSnapshot: false,
        secondsWaiting: 99,
        localScreen: "SETTINGS",
        devScreen: "CINEMA",
        backendScreen: "CINEMA",
        closePromptVisible: true,
      }),
    ).toBe("BOOT_SLOW");
  });

  it("waits for the Tailscale reading before routing anywhere", () => {
    expect(route({ hasTailscaleReadiness: false })).toBe("TAILSCALE_CHECKING");
    // …even when a screen is otherwise ready to go.
    expect(route({ hasTailscaleReadiness: false, backendScreen: "CINEMA" })).toBe(
      "TAILSCALE_CHECKING",
    );
  });
});

describe("Tailscale and the join escape path", () => {
  it("gates on Tailscale readiness", () => {
    expect(route({ tailscaleReady: false })).toBe("TAILSCALE_SETUP");
  });

  it("gates BEFORE any party screen — an unusable tailnet wins", () => {
    expect(route({ tailscaleReady: false, backendScreen: "LOBBY" })).toBe("TAILSCALE_SETUP");
    expect(route({ tailscaleReady: false, devScreen: "CINEMA" })).toBe("TAILSCALE_SETUP");
  });

  it("F4: a failed join with a stable code reaches Partner Connect", () => {
    expect(route({ joinFailureCode: "MP-NET-TS-005" })).toBe("PARTNER_CONNECT");
  });

  it("F4: Partner Connect outranks the backend screen, so the escape is reachable", () => {
    expect(route({ joinFailureCode: "MP-NET-TS-005", backendScreen: "JOIN_PARTY" })).toBe(
      "PARTNER_CONNECT",
    );
  });

  it("F4: clearing the code leaves Partner Connect (that is how the escape works)", () => {
    expect(route({ joinFailureCode: "MP-NET-TS-005" })).toBe("PARTNER_CONNECT");
    expect(route({ joinFailureCode: null })).toBe("HOME_FALLBACK");
  });

  it("an unrelated failure code does not trigger Partner Connect", () => {
    expect(route({ joinFailureCode: "MP-PROVIDER-002" })).toBe("HOME_FALLBACK");
  });
});

describe("local (shell-owned) screens", () => {
  it("routes each local screen", () => {
    expect(route({ localScreen: "FIRST_RUN" })).toBe("FIRST_RUN");
    expect(route({ localScreen: "SETTINGS" })).toBe("SETTINGS");
    expect(route({ localScreen: "SCHEDULE" })).toBe("SCHEDULE");
    expect(route({ localScreen: "FRIENDS" })).toBe("FRIENDS");
    expect(route({ localScreen: "CREATE_PARTY" })).toBe("CREATE_PARTY");
  });

  it("a local screen outranks the backend screen", () => {
    expect(route({ localScreen: "SETTINGS", backendScreen: "CINEMA" })).toBe("SETTINGS");
    expect(route({ localScreen: "FRIENDS", backendScreen: "LOBBY" })).toBe("FRIENDS");
  });

  it("the dev override wins over a local screen", () => {
    expect(route({ devScreen: "HOME", localScreen: "SETTINGS" })).toBe("HOME");
  });

  it("the dev override reaches every party screen", () => {
    expect(route({ devScreen: "CREATE" })).toBe("CREATE_PARTY");
    expect(route({ devScreen: "JOIN" })).toBe("JOIN_PARTY");
    expect(route({ devScreen: "LOBBY" })).toBe("LOBBY");
    expect(route({ devScreen: "READY" })).toBe("READY_CHECK");
    expect(route({ devScreen: "CINEMA" })).toBe("CINEMA");
  });

  it("the dev override cannot bypass the Tailscale gate", () => {
    expect(route({ devScreen: "CINEMA", tailscaleReady: false })).toBe("TAILSCALE_SETUP");
  });
});

describe("§69 close prompt — the non-obvious precedence", () => {
  it("outranks Create Party", () => {
    expect(route({ closePromptVisible: true, localScreen: "CREATE_PARTY" })).toBe("CLOSE_PROMPT");
  });

  it("outranks every backend screen", () => {
    for (const backendScreen of ["JOIN_PARTY", "LOBBY", "READY_CHECK", "CINEMA"]) {
      expect(route({ closePromptVisible: true, backendScreen })).toBe("CLOSE_PROMPT");
    }
  });

  it("is outranked by the modal-free local surfaces", () => {
    // Settings / Schedule / Friends / First Run are reachable during a party
    // and must not be replaced by the close confirmation.
    expect(route({ closePromptVisible: true, localScreen: "SETTINGS" })).toBe("SETTINGS");
    expect(route({ closePromptVisible: true, localScreen: "SCHEDULE" })).toBe("SCHEDULE");
    expect(route({ closePromptVisible: true, localScreen: "FRIENDS" })).toBe("FRIENDS");
    expect(route({ closePromptVisible: true, localScreen: "FIRST_RUN" })).toBe("FIRST_RUN");
  });

  it("is distinct from the backend's own PARTY_END_CONFIRM screen", () => {
    // They render the same view with different handlers, so they must not
    // collapse into one route.
    expect(route({ closePromptVisible: true })).toBe("CLOSE_PROMPT");
    expect(route({ closePromptVisible: false, backendScreen: "PARTY_END_CONFIRM" })).toBe(
      "END_PARTY_CONFIRM",
    );
  });
});

describe("backend party screens", () => {
  it("routes each backend screen", () => {
    expect(route({ backendScreen: "JOIN_PARTY" })).toBe("JOIN_PARTY");
    expect(route({ backendScreen: "LOBBY" })).toBe("LOBBY");
    expect(route({ backendScreen: "READY_CHECK" })).toBe("READY_CHECK");
    expect(route({ backendScreen: "CINEMA" })).toBe("CINEMA");
    expect(route({ backendScreen: "PARTY_END_CONFIRM" })).toBe("END_PARTY_CONFIRM");
  });

  it("PARTY_END_CONFIRM is checked last, so it cannot shadow a real screen", () => {
    expect(route({ backendScreen: "CINEMA", closePromptVisible: false })).toBe("CINEMA");
  });

  it("an unknown backend screen falls back to Home", () => {
    expect(route({ backendScreen: "SOMETHING_NEW" })).toBe("HOME_FALLBACK");
    expect(route({ backendScreen: "" })).toBe("HOME_FALLBACK");
  });

  it("distinguishes the dev-override Home from the fallback Home", () => {
    // The fallback renders the §52 retention prompt over Home; the dev
    // override does not. Collapsing them would change that behaviour.
    expect(route({ devScreen: "HOME" })).toBe("HOME");
    expect(route({})).toBe("HOME_FALLBACK");
  });
});

describe("the route space is fully covered", () => {
  it("every declared screen is reachable", () => {
    const reached = new Set<AppScreen>([
      route({ hasSnapshot: false }),
      route({ hasSnapshot: false, secondsWaiting: 99 }),
      route({ hasTailscaleReadiness: false }),
      route({ tailscaleReady: false }),
      route({ joinFailureCode: "MP-NET-TS-005" }),
      route({ devScreen: "HOME" }),
      route({}),
      route({ localScreen: "FIRST_RUN" }),
      route({ localScreen: "SETTINGS" }),
      route({ localScreen: "SCHEDULE" }),
      route({ localScreen: "FRIENDS" }),
      route({ closePromptVisible: true }),
      route({ localScreen: "CREATE_PARTY" }),
      route({ backendScreen: "JOIN_PARTY" }),
      route({ backendScreen: "LOBBY" }),
      route({ backendScreen: "READY_CHECK" }),
      route({ backendScreen: "CINEMA" }),
      route({ backendScreen: "PARTY_END_CONFIRM" }),
    ]);
    for (const screen of [
      "BOOT_STARTING",
      "BOOT_SLOW",
      "TAILSCALE_CHECKING",
      "TAILSCALE_SETUP",
      "PARTNER_CONNECT",
      "HOME",
      "HOME_FALLBACK",
      "FIRST_RUN",
      "SETTINGS",
      "SCHEDULE",
      "FRIENDS",
      "CLOSE_PROMPT",
      "END_PARTY_CONFIRM",
      "CREATE_PARTY",
      "JOIN_PARTY",
      "LOBBY",
      "READY_CHECK",
      "CINEMA",
    ] satisfies AppScreen[]) {
      expect(reached, `${screen} is unreachable`).toContain(screen);
    }
  });

  it("is deterministic — the same input always routes the same way", () => {
    const input: Partial<AppRouteInput> = {
      localScreen: "SETTINGS",
      backendScreen: "CINEMA",
      closePromptVisible: true,
    };
    const first = route(input);
    for (let i = 0; i < 20; i++) {
      expect(route(input)).toBe(first);
    }
  });
});
