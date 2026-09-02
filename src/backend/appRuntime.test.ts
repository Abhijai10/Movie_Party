import { describe, expect, it } from "vitest";
import {
  BackendCommandError,
  tailscaleSetupOpenErrorMessage,
} from "./appRuntime";

describe("BackendCommandError code extraction", () => {
  it.each([
    ["MP-NET-TS-001", "MP-NET-TS-001"],
    ["MP-NET-TS-002", "MP-NET-TS-002"],
    ["MP-NET-TS-003", "MP-NET-TS-003"],
    ["MP-NET-TS-004", "MP-NET-TS-004"],
    ["MP-NET-TS-005", "MP-NET-TS-005"],
    ["MP-NET-TS-006", "MP-NET-TS-006"],
    ["MP-ROOM-001", "MP-ROOM-001"],
    ["MP-PROVIDER-002", "MP-PROVIDER-002"],
  ])("extracts %s from a backend detail string", (code) => {
    const error = new BackendCommandError("get_tailscale_readiness", `${code} some detail`);
    expect(error.code).toBe(code);
  });

  it("falls back to MP-BACKEND-001 when no stable code is present", () => {
    const error = new BackendCommandError("get_tailscale_readiness", "unknown failure");
    expect(error.code).toBe("MP-BACKEND-001");
  });

  it("classifies Tailscale-specific failures as network failures", () => {
    const error = new BackendCommandError("create_local_party", "MP-NET-TS-004 no usable ip");
    expect(error.kind).toBe("network failure");
  });
});

describe("tailscaleSetupOpenErrorMessage", () => {
  it("maps MP-NET-TS-001 to an install-first message", () => {
    const message = tailscaleSetupOpenErrorMessage("MP-NET-TS-001 Tailscale executable was not found");
    expect(message).toContain("Install Tailscale");
  });

  it("maps MP-NET-TS-003 to an open-app message", () => {
    const message = tailscaleSetupOpenErrorMessage(
      "MP-NET-TS-003 could not open the Tailscale app",
    );
    expect(message).toContain("open the Tailscale app");
  });

  it("never leaks the raw backend detail to the user", () => {
    const raw = "MP-NET-TS-003 could not open the Tailscale app: /Users/me/secret";
    const message = tailscaleSetupOpenErrorMessage(raw);
    expect(message).not.toContain("/Users/me/secret");
    expect(message).not.toContain("MP-NET-TS-003");
  });

  it("returns a fallback for unknown errors", () => {
    const message = tailscaleSetupOpenErrorMessage("MP-BACKEND-001 something");
    expect(message.length).toBeGreaterThan(0);
  });
});