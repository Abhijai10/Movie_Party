import { describe, expect, it } from "vitest";
import type { ProviderCapability, ProviderReadiness } from "../backend/appRuntime";
import {
  canCreatePartyWithReadiness,
  isGenericLink,
  providerHomeUrl,
  providerModeStatus,
  providerReadinessAction,
  providerReadinessMessage,
  providerSearchUrl,
  providerSelectLabels,
  resolveProviderSelectValue,
} from "./providerSelection";

const netflix: ProviderCapability = {
  id: "netflix",
  displayName: "Netflix",
  supportLevel: "SUPPORTED",
  titleResolution: "PROVIDER_SEARCH",
  syncAvailable: true,
  sharedAvailable: false,
  sharedReason: "Provider Shared is experimental and unavailable until capture is verified on this device.",
  verification: "EXTERNAL_VERIFICATION_PENDING",
};

const youtube: ProviderCapability = {
  id: "youtube",
  displayName: "YouTube",
  supportLevel: "SUPPORTED",
  titleResolution: "DIRECT_URL",
  syncAvailable: true,
  sharedAvailable: false,
  sharedReason: "Provider Shared is experimental and unavailable until capture is verified on this device.",
  verification: "EXTERNAL_VERIFICATION_PENDING",
};

describe("provider source selection", () => {
  it("allows the existing Provider Sync path", () => {
    expect(providerModeStatus(netflix, "PROVIDER_SYNC")).toEqual({
      canPrepare: true,
      message: null,
    });
  });

  it("does not report unsupported Provider Shared as ready", () => {
    expect(providerModeStatus(netflix, "PROVIDER_SHARED")).toEqual({
      canPrepare: false,
      message: netflix.sharedReason,
    });
  });

  it("rejects an unknown provider selection", () => {
    expect(providerModeStatus(null, "PROVIDER_SYNC").canPrepare).toBe(false);
  });

  it("keeps generic links separate from provider selection", () => {
    expect(isGenericLink("https://example.com/movie.mp4")).toBe(true);
    expect(isGenericLink("movieparty://join/room")).toBe(false);
  });

  it("exposes provider home URLs", () => {
    expect(providerHomeUrl("netflix")).toContain("netflix.com");
    expect(providerHomeUrl("youtube")).toContain("youtube.com");
    expect(providerHomeUrl("unknown")).toContain("youtube.com");
  });

  it("returns search URL for non-empty title", () => {
    const url = providerSearchUrl("netflix", "Inception");
    expect(url).toContain("netflix.com/search");
    expect(url).toContain("Inception");
  });

  it("returns null for empty title", () => {
    expect(providerSearchUrl("netflix", "")).toBeNull();
    expect(providerSearchUrl("netflix", "   ")).toBeNull();
  });
});

describe("provider readiness state machine", () => {
  it("exposes truthful action for each readiness state", () => {
    expect(providerReadinessAction("NOT_STARTED").next).toBe("open");
    expect(providerReadinessAction("NOT_STARTED").enabled).toBe(true);
    expect(providerReadinessAction("LOGIN_REQUIRED").next).toBe("check");
    expect(providerReadinessAction("LOGIN_REQUIRED").enabled).toBe(true);
    expect(providerReadinessAction("READY").next).toBe("title");
    expect(providerReadinessAction("READY").enabled).toBe(true);
    expect(providerReadinessAction("NAVIGATING").next).toBe("check");
    expect(providerReadinessAction("NAVIGATING").enabled).toBe(true);
    expect(providerReadinessAction("PLAYBACK_READY").next).toBe("create");
    expect(providerReadinessAction("PLAYBACK_READY").enabled).toBe(true);
    expect(providerReadinessAction("UNAVAILABLE").enabled).toBe(false);
    expect(providerReadinessAction("ERROR").enabled).toBe(false);
  });

  it("provides non-empty messages for every readiness state", () => {
    const states: ProviderReadiness[] = [
      "NOT_STARTED",
      "LAUNCHING",
      "LOGIN_REQUIRED",
      "READY",
      "NAVIGATING",
      "PLAYBACK_READY",
      "UNAVAILABLE",
      "ERROR",
    ];
    for (const state of states) {
      const msg = providerReadinessMessage(state, "TestProvider");
      expect(msg).toBeTruthy();
      expect(msg.length).toBeGreaterThan(0);
    }
  });

  it("never instructs the user to type a password into Movie Party", () => {
    const msg = providerReadinessMessage("LOGIN_REQUIRED", "Netflix");
    expect(msg).not.toMatch(/enter (your )?password/i);
    expect(msg).not.toMatch(/type.*password/i);
    expect(msg).toMatch(/own page/i);
  });

  it("only PlaybackReady allows room creation", () => {
    const states: ProviderReadiness[] = [
      "NOT_STARTED",
      "LAUNCHING",
      "LOGIN_REQUIRED",
      "READY",
      "NAVIGATING",
      "UNAVAILABLE",
      "ERROR",
    ];
    for (const state of states) {
      expect(canCreatePartyWithReadiness(state)).toBe(false);
    }
    expect(canCreatePartyWithReadiness("PLAYBACK_READY")).toBe(true);
  });

  it("provider capabilities reflect real support levels without pretending", () => {
    expect(netflix.supportLevel).toBe("SUPPORTED");
    expect(youtube.titleResolution).toBe("DIRECT_URL");
    expect(netflix.titleResolution).toBe("PROVIDER_SEARCH");
    expect(netflix.verification).toBe("EXTERNAL_VERIFICATION_PENDING");
  });
});
describe("provider select labels (dark Emergent select)", () => {
  it("offers every capability with its display name as the option label", () => {
    const labels = providerSelectLabels([netflix, youtube]);
    expect(labels).toEqual([
      { value: "netflix", label: "Netflix", disabled: false },
      { value: "youtube", label: "YouTube", disabled: false },
    ]);
  });

  it("falls back to a single disabled placeholder while availability loads", () => {
    expect(providerSelectLabels([])).toEqual([
      { value: "", label: "Checking provider availability...", disabled: true },
    ]);
  });

  it("keeps the selected provider visible whenever it is offered", () => {
    expect(resolveProviderSelectValue([netflix, youtube], "youtube")).toBe("youtube");
  });

  it("never shows a blank selection: an unknown id resolves to the first option", () => {
    expect(resolveProviderSelectValue([netflix, youtube], "")).toBe("netflix");
    expect(resolveProviderSelectValue([netflix, youtube], "disney")).toBe("netflix");
    expect(resolveProviderSelectValue([], "netflix")).toBe("");
  });
});
