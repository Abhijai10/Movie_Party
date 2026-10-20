import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  BackendCommandError,
  createLocalParty,
  launchProvider,
  resetProviderSession,
  providerSessionErrorMessage,
} from "./appRuntime";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => vi.restoreAllMocks());

describe("provider room command boundary", () => {
  it("carries content confirmation into the backend attachment gate", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ screen: "LOBBY" });
    await launchProvider(
      "youtube",
      "https://www.youtube.com/watch?v=abcdefghijk",
      "PROVIDER_SYNC",
      "observed-key",
    );
    expect(invoke).toHaveBeenLastCalledWith("launch_provider", {
      providerId: "youtube",
      url: "https://www.youtube.com/watch?v=abcdefghijk",
      mode: "PROVIDER_SYNC",
      selectionKey: "observed-key",
    });
  });
  it.each([
    "MP-PROVIDER-001",
    "MP-PROVIDER-003",
    "MP-PROVIDER-005",
    "MP-PROVIDER-006",
    "MP-PROVIDER-007",
  ])("surfaces actionable profile recovery for %s without raw paths", (code) => {
    const message = providerSessionErrorMessage(
      new BackendCommandError("open_provider_browser", `${code} /private/sensitive/path`),
    );
    expect(message).toBeTruthy();
    expect(message).not.toContain("/private/sensitive/path");
  });
  it("does not imply confirmation when requesting a reset", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("MP-PROVIDER-007 confirmation required");
    vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(resetProviderSession("youtube", false)).rejects.toMatchObject({
      code: "MP-PROVIDER-007",
    });
    expect(invoke).toHaveBeenLastCalledWith("reset_provider_session", {
      providerId: "youtube",
      confirmed: false,
    });
  });

  it("sends the explicitly confirmed reset to the selected provider only", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ screen: "SETTINGS" });
    await resetProviderSession("netflix", true);
    expect(invoke).toHaveBeenLastCalledWith("reset_provider_session", {
      providerId: "netflix",
      confirmed: true,
    });
  });
  it("carries the signed-in browser preservation flag into room creation", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ screen: "LOBBY" });
    await createLocalParty(null, true);
    expect(invoke).toHaveBeenLastCalledWith("create_local_party", {
      mediaPath: null,
      keepProviderSession: true,
    });
  });

  it("keeps browser preservation opt-in for ordinary local rooms", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ screen: "LOBBY" });
    await createLocalParty("/movies/film.mkv");
    expect(invoke).toHaveBeenLastCalledWith("create_local_party", {
      mediaPath: "/movies/film.mkv",
      keepProviderSession: false,
    });
  });

  it("rejects a failed provider attachment with its actionable code", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(invoke).mockRejectedValueOnce("MP-PROVIDER-004 sign in before creating the room");
    const result = launchProvider("youtube", "https://www.youtube.com/", "PROVIDER_SYNC");
    await expect(result).rejects.toBeInstanceOf(BackendCommandError);
    await expect(result).rejects.toMatchObject({
      command: "launch_provider",
      code: "MP-PROVIDER-004",
    });
  });
});
