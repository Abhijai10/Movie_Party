import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { ProviderAction } from "../backend/appRuntime";
import { retryRoomProvider, focusProviderPlayback } from "../backend/appRuntime";
import {
  providerContentCanRetry,
  providerContentLabel,
  providerEvidence,
  providerEvidenceLabel,
  providerEvidenceTierLabel,
} from "./roomProviderState";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => vi.resetAllMocks());
describe("authoritative provider loading UI", () => {
  it("distinguishes opening, confirmation and actual playback readiness", () => {
    expect(providerContentLabel("OPENING")).toBe("Opening selected content");
    expect(providerContentLabel("CONTENT_CONFIRMED")).toBe("Content identity confirmed");
    expect(providerContentLabel("PLAYBACK_READY")).toBe("Video ready");
    expect(providerContentLabel("AUTH_REQUIRED")).toBe("Sign in in Chrome");
    expect(providerContentLabel("UNSELECTED")).toBe("Waiting for selection");
  });
  it("only offers browser recovery to the guest, without changing the host selection", () => {
    expect(providerContentCanRetry("GUEST", "ERROR")).toBe(true);
    expect(providerContentCanRetry("GUEST", "AUTH_REQUIRED")).toBe(true);
    for (const state of ["UNSELECTED", "OPENING", "CONTENT_CONFIRMED", "PLAYBACK_READY"] as const)
      expect(providerContentCanRetry("GUEST", state)).toBe(false);
    expect(providerContentCanRetry("HOST", "ERROR")).toBe(false);
  });
  it("retries the local selected browser without sending identity or login data", async () => {
    vi.mocked(invoke).mockResolvedValue({});
    await retryRoomProvider();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("retry_room_provider");
  });
  it("does not hide a retry rejection", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("no selected room"));
    await expect(retryRoomProvider()).rejects.toThrow("no selected room");
  });
  it("focuses only managed playback without faking a browser user gesture", async () => {
    vi.mocked(invoke).mockResolvedValue({});
    await focusProviderPlayback();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("focus_provider_playback");
  });
  it("preserves a focus rejection so the recovery UI can show it", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("Chrome is busy"));
    await expect(focusProviderPlayback()).rejects.toThrow("Chrome is busy");
  });
});

const action = (
  operation: string,
  evidence: "LIVE_VERIFIED" | "AUTOMATED_ONLY" | "UNVERIFIED",
): ProviderAction => ({ operation, evidence, note: "a reason" });

describe("provider evidence disclosure", () => {
  it("counts each tier from the operation list, not the summary", () => {
    const tally = providerEvidence([
      action("seek_command", "LIVE_VERIFIED"),
      action("consensus_playback", "AUTOMATED_ONLY"),
      action("two_device_sync", "UNVERIFIED"),
      action("session_expiry_recovery", "UNVERIFIED"),
    ]);
    expect(tally).toEqual({ live: 1, automated: 1, unverified: 2, total: 4 });
  });

  it("states plainly when a provider has never been driven on its real page", () => {
    const label = providerEvidenceLabel(
      providerEvidence([
        action("play_command", "AUTOMATED_ONLY"),
        action("seek_command", "UNVERIFIED"),
      ]),
    );
    expect(label).toContain("never been driven on its real page");
    expect(label).not.toContain("live-verified");
  });

  it("does not let one live operation read as a verified provider", () => {
    const label = providerEvidenceLabel(
      providerEvidence([
        action("seek_command", "LIVE_VERIFIED"),
        action("two_device_sync", "UNVERIFIED"),
      ]),
    );
    expect(label).toContain("1 live-verified");
    expect(label).toContain("1 unverified");
    expect(label).not.toMatch(/^\s*verified/i);
  });

  it("names every tier in words because colour cannot carry status", () => {
    expect(providerEvidenceTierLabel("LIVE_VERIFIED")).toBe("Live-verified");
    expect(providerEvidenceTierLabel("AUTOMATED_ONLY")).toBe("Automated tests only");
    expect(providerEvidenceTierLabel("UNVERIFIED")).toBe("Unverified");
  });

  it("reports no evidence as no evidence", () => {
    expect(providerEvidence([]).total).toBe(0);
    expect(providerEvidenceLabel(providerEvidence([]))).toBe(
      "No provider operations are evidenced yet.",
    );
  });
});
