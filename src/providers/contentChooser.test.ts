import { describe, expect, it, vi, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  browseProviderContent,
  canCreateSelectedContent,
  confirmProviderContent,
  createContentQueue,
  getProviderContent,
  validateProviderContent,
  type ContentChooserSnapshot,
  type ContentTab,
} from "./contentChooser";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => vi.resetAllMocks());
const tab: ContentTab = {
  targetId: "tab-b",
  title: "Fixture video",
  identity: {
    providerId: "youtube",
    contentId: "abcdefghijk",
    allowedUrl: "https://www.youtube.com/watch?v=abcdefghijk",
    playlistId: null,
  },
  thumbnailUrl: null,
  documentId: "document",
  focused: true,
  visible: true,
  playbackReady: true,
  issue: null,
  selectionKey: "key",
};
const ready: ContentChooserSnapshot = {
  browserReady: true,
  contentReady: true,
  tabs: [tab],
  selected: tab,
  confirmedKey: "key",
  issue: null,
  suggestedTargetId: "tab-b",
};

describe("provider selection creation gate", () => {
  it("requires current browser, selected content and explicit matching confirmation", () => {
    expect(canCreateSelectedContent(ready, tab, true)).toBe(true);
    expect(canCreateSelectedContent(ready, tab, false)).toBe(false);
    expect(canCreateSelectedContent(null, tab, true)).toBe(false);
    expect(canCreateSelectedContent(ready, null, true)).toBe(false);
    for (const changed of [
      { browserReady: false },
      { contentReady: false },
      { confirmedKey: null },
      { confirmedKey: "old" },
      { selected: null },
    ])
      expect(canCreateSelectedContent({ ...ready, ...changed }, tab, true)).toBe(false);
    for (const changed of [
      { targetId: "other" },
      { selectionKey: "changed" },
      { identity: null },
      { playbackReady: false },
    ])
      expect(canCreateSelectedContent(ready, { ...tab, ...changed }, true)).toBe(false);
  });
  it("serializes background observation and user confirmation, recovering after rejection", async () => {
    const queue = createContentQueue();
    const calls: string[] = [];
    let release: () => void = () => {};
    const first = queue(async () => {
      calls.push("observe");
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    });
    const second = queue(() => {
      calls.push("confirm");
      return Promise.reject(new Error("tab changed"));
    });
    const rejected = expect(second).rejects.toThrow("tab changed");
    const third = queue(() => {
      calls.push("retry");
      return Promise.resolve("ready");
    });
    await Promise.resolve();
    expect(calls).toEqual(["observe"]);
    release();
    await first;
    await rejected;
    await expect(third).resolves.toBe("ready");
    expect(calls).toEqual(["observe", "confirm", "retry"]);
  });
});
describe("host-local chooser IPC", () => {
  it("sends provider, selected tab and exact observed confirmation key", async () => {
    vi.mocked(invoke).mockResolvedValue(ready);
    await getProviderContent("youtube");
    expect(invoke).toHaveBeenLastCalledWith("get_provider_content", { providerId: "youtube" });
    await browseProviderContent("youtube", "a title", false);
    expect(invoke).toHaveBeenLastCalledWith("browse_provider_content", {
      providerId: "youtube",
      input: "a title",
      direct: false,
    });
    await browseProviderContent("youtube", "https://www.youtube.com/watch?v=abcdefghijk", true);
    expect(invoke).toHaveBeenLastCalledWith("browse_provider_content", {
      providerId: "youtube",
      input: "https://www.youtube.com/watch?v=abcdefghijk",
      direct: true,
    });
    await confirmProviderContent("youtube", "tab-b", "key");
    expect(invoke).toHaveBeenLastCalledWith("confirm_provider_content", {
      providerId: "youtube",
      targetId: "tab-b",
      selectionKey: "key",
    });
    await validateProviderContent("youtube", "key");
    expect(invoke).toHaveBeenLastCalledWith("validate_provider_content", {
      providerId: "youtube",
      selectionKey: "key",
    });
  });
  it("does not turn a failed browser check into a ready state", async () => {
    vi.mocked(invoke).mockRejectedValue("selected tab closed");
    await expect(getProviderContent("youtube")).rejects.toBe("selected tab closed");
  });
});
