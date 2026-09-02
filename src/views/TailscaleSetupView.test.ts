import { describe, expect, it } from "vitest";
import { contentFor } from "./TailscaleSetupView";
import { refreshLabelFor } from "../backend/tailscaleOnboarding";

const SETUP_STATES = [
  "NOT_INSTALLED",
  "DAEMON_UNAVAILABLE",
  "NEEDS_LOGIN",
  "STOPPED",
  "NO_USABLE_ADDRESS",
] as const;

describe("TailscaleSetupView state content", () => {
  it("tells the user to install Tailscale when it is not installed", () => {
    const content = contentFor("NOT_INSTALLED");
    expect(content.primaryLabel).toBe("Install Tailscale");
    expect(content.primaryAction).toBe("INSTALL");
    expect(content.description).toContain("isn't installed");
  });

  it("offers to open the Tailscale app when the daemon is unavailable", () => {
    const content = contentFor("DAEMON_UNAVAILABLE");
    expect(content.primaryLabel).toBe("Open Tailscale");
    expect(content.primaryAction).toBe("OPEN_APP");
    expect(content.description).toContain("Open the Tailscale app");
  });

  it("asks the user to sign in when authentication is missing", () => {
    const content = contentFor("NEEDS_LOGIN");
    expect(content.primaryLabel).toBe("Open Tailscale");
    expect(content.primaryAction).toBe("OPEN_APP");
    expect(content.description).toContain("sign in");
  });

  it("asks the user to turn the connection on when stopped", () => {
    const content = contentFor("STOPPED");
    expect(content.primaryLabel).toBe("Open Tailscale");
    expect(content.primaryAction).toBe("OPEN_APP");
    expect(content.description).toContain("turn it on");
  });

  it("asks the user to connect to a private network when no usable address exists", () => {
    const content = contentFor("NO_USABLE_ADDRESS");
    expect(content.primaryLabel).toBe("Open Tailscale");
    expect(content.primaryAction).toBe("OPEN_APP");
    expect(content.description).toContain("private address");
  });

  it("renders content for each non-ready state without throwing", () => {
    for (const state of SETUP_STATES) {
      expect(() => contentFor(state)).not.toThrow();
    }
  });

  it("throws when asked to render content for the READY state", () => {
    expect(() => contentFor("READY")).toThrow("READY is not a setup state");
  });

  it("reserves the install action for the not-installed state", () => {
    for (const state of SETUP_STATES) {
      const content = contentFor(state);
      if (state === "NOT_INSTALLED") {
        expect(content.primaryAction).toBe("INSTALL");
      } else {
        expect(content.primaryAction).not.toBe("INSTALL");
      }
    }
  });

  it("uses the Open App action whenever Tailscale is already installed", () => {
    for (const state of [
      "DAEMON_UNAVAILABLE",
      "NEEDS_LOGIN",
      "STOPPED",
      "NO_USABLE_ADDRESS",
    ] as const) {
      const content = contentFor(state);
      expect(content.primaryAction).toBe("OPEN_APP");
      expect(content.primaryLabel).toBe("Open Tailscale");
    }
  });

  it("never claims Tailscale is unavailable when a more precise state is known", () => {
    for (const state of [
      "DAEMON_UNAVAILABLE",
      "NEEDS_LOGIN",
      "STOPPED",
      "NO_USABLE_ADDRESS",
    ] as const) {
      const content = contentFor(state);
      const text = `${content.description} ${content.primaryLabel}`.toLowerCase();
      expect(text).not.toContain("unavailable");
      expect(text).not.toContain("service");
    }
  });

  it("never asks for credentials in any state", () => {
    for (const state of SETUP_STATES) {
      const content = contentFor(state);
      const text = `${content.description} ${content.primaryLabel}`.toLowerCase();
      expect(text).not.toContain("password");
      expect(text).not.toContain("auth key");
      expect(text).not.toContain("api key");
    }
  });

  describe("refreshLabelFor", () => {
    it('returns "I\'ve signed in" for NEEDS_LOGIN', () => {
      expect(refreshLabelFor("NEEDS_LOGIN")).toBe("I've signed in");
    });

    it('returns "Check again" for every other non-ready state', () => {
      for (const state of [
        "NOT_INSTALLED",
        "DAEMON_UNAVAILABLE",
        "STOPPED",
        "NO_USABLE_ADDRESS",
      ] as const) {
        expect(refreshLabelFor(state)).toBe("Check again");
      }
    });
  });

  it("offers a refresh/check-again affordance for every setup state", () => {
    for (const state of SETUP_STATES) {
      const label = refreshLabelFor(state);
      expect(label.length).toBeGreaterThan(0);
    }
  });

  it("clearly identifies the current state to the user", () => {
    const stateTitles: Record<string, string> = {
      NOT_INSTALLED: "Set up your",
      DAEMON_UNAVAILABLE: "Start Tailscale",
      NEEDS_LOGIN: "Sign in to",
      STOPPED: "Turn on",
      NO_USABLE_ADDRESS: "Connect to your",
    };
    for (const [state, keyword] of Object.entries(stateTitles)) {
      const content = contentFor(state as (typeof SETUP_STATES)[number]);
      expect(content.title.toLowerCase()).toContain(keyword.toLowerCase());
    }
  });

  it("does not mention localhost, LAN address, or public IP as usable addresses", () => {
    for (const state of SETUP_STATES) {
      const content = contentFor(state);
      const text = `${content.title} ${content.description}`.toLowerCase();
      expect(text).not.toContain("localhost");
      expect(text).not.toContain("127.0.0.1");
      expect(text).not.toContain("192.168");
      expect(text).not.toContain("public ip");
    }
  });

  it("explains that both people need a usable Tailscale connection", () => {
    const notInstalled = contentFor("NOT_INSTALLED");
    const combined = `${notInstalled.description} ${notInstalled.primaryLabel}`.toLowerCase();
    expect(combined).toContain("movie partner");
  });
});
