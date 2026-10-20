import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { BackendCommandError, createLocalParty, launchProvider } from "./appRuntime";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => vi.restoreAllMocks());

describe("provider room command boundary", () => {
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
