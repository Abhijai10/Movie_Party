import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { retryRoomProvider, focusProviderPlayback } from "../backend/appRuntime";
import { providerContentCanRetry, providerContentLabel } from "./roomProviderState";
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
