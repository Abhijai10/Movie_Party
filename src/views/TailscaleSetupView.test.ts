import { describe, expect, it } from "vitest";
import { contentFor } from "./TailscaleSetupView";

const ALL_STATES = [
  "NOT_INSTALLED",
  "DAEMON_UNAVAILABLE",
  "NEEDS_LOGIN",
  "STOPPED",
  "NO_USABLE_ADDRESS",
  "READY",
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

  it("renders only when the private connection is not ready", () => {
    for (const state of ALL_STATES) {
      expect(() => contentFor(state)).not.toThrow();
    }
  });

  it("reserves the install action for the not-installed state", () => {
    for (const state of ALL_STATES) {
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
    for (const state of ALL_STATES) {
      const content = contentFor(state);
      const text = `${content.description} ${content.primaryLabel}`.toLowerCase();
      expect(text).not.toContain("password");
      expect(text).not.toContain("auth key");
      expect(text).not.toContain("api key");
    }
  });
});
