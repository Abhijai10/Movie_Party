import { expect, it, vi } from "vitest";
import type { AppSnapshot } from "../backend/appRuntime";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
import { setYoutubeDriver, stopYoutubeFrame, shouldAutoStart } from "./YoutubeRuntime";
it("serializes player handoff and coalesces StrictMode duplicate requests", async () => {
  let finish: ((value: AppSnapshot) => void) | undefined;
  ipc.mockReturnValueOnce(
    new Promise<AppSnapshot>((resolve) => {
      finish = resolve;
    }),
  );
  const first = setYoutubeDriver(true),
    duplicate = setYoutubeDriver(true);
  expect(duplicate).toBe(first);
  expect(ipc).toHaveBeenCalledTimes(1);
  await expect(setYoutubeDriver(false)).rejects.toThrow("handoff");
  const snapshot = { screen: "LOBBY" } as AppSnapshot;
  finish?.(snapshot);
  await expect(first).resolves.toBe(snapshot);
  ipc.mockResolvedValueOnce(snapshot);
  await setYoutubeDriver(false);
  expect(ipc).toHaveBeenLastCalledWith("youtube_driver", { enabled: false });
  stopYoutubeFrame();
});

it("keeps managed Chrome alive until Create Party validation and attachment finish", () => {
  expect(shouldAutoStart(true, false, false)).toBe(false);
  expect(shouldAutoStart(true, false, true)).toBe(true);
  expect(shouldAutoStart(true, true, true)).toBe(false);
  expect(shouldAutoStart(false, false, true)).toBe(false);
});
