import { describe, expect, it } from "vitest";
import type { ProviderCapability } from "../backend/appRuntime";
import { isGenericLink, providerModeStatus } from "./providerSelection";

const netflix: ProviderCapability = {
  id: "netflix",
  displayName: "Netflix",
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
    expect(isGenericLink("moveparty://join/room")).toBe(false);
  });
});
