import { describe, expect, it } from "vitest";
import { closedChatOverlay, openChatManually, openChatPreview } from "../chat/overlayState";
import { captureGhostUiSnapshot, restoreGhostUiSnapshot } from "./ghostUiState";

describe("ghost UI snapshot", () => {
  it("captures the chat visibility as it was before the mode hid it", () => {
    const snapshot = captureGhostUiSnapshot(openChatManually());
    expect(snapshot.chatVisibility).toEqual({
      manualOpen: true,
      transientOpen: false,
      historyDismissed: false,
    });
  });

  it("restores a manually opened chat after Ghost/Privacy Mode ends", () => {
    const restored = restoreGhostUiSnapshot(captureGhostUiSnapshot(openChatManually()));
    expect(restored).toEqual({ manualOpen: true, transientOpen: false, historyDismissed: false });
  });

  it("keeps the chat closed after the mode ends when it was closed before", () => {
    const restored = restoreGhostUiSnapshot(captureGhostUiSnapshot(closedChatOverlay));
    expect(restored).toEqual(closedChatOverlay);
  });

  it("does not revive an expired transient preview after the mode ends", () => {
    const restored = restoreGhostUiSnapshot(captureGhostUiSnapshot(openChatPreview()));
    expect(restored).toEqual({ manualOpen: false, transientOpen: false, historyDismissed: false });
  });

  it("returns defensive copies so later UI changes cannot mutate a capture", () => {
    const before = openChatManually();
    const snapshot = captureGhostUiSnapshot(before);
    const restored = restoreGhostUiSnapshot(snapshot);
    expect(snapshot).not.toBe(before);
    expect(snapshot.chatVisibility).not.toBe(before);
    expect(restored).not.toBe(snapshot);
  });
});
