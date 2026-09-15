import { describe, expect, it } from "vitest";
import {
  applyChatArrival,
  canToggleChat,
  closeChatPreview,
  closedChatOverlay,
  dismissChatHistory,
  isChatOverlayOpen,
  openChatManually,
  openChatPreview,
} from "./overlayState";

describe("chat overlay visibility", () => {
  it("keeps a manually opened chat visible when a transient preview ends", () => {
    expect(closeChatPreview(openChatManually())).toEqual({
      manualOpen: true,
      transientOpen: false,
      historyDismissed: false,
    });
  });

  it("opens a transient preview without turning on the composer", () => {
    const preview = openChatPreview();
    expect(preview.manualOpen).toBe(false);
    expect(isChatOverlayOpen(preview)).toBe(true);
  });

  it("auto-hides the history panel without closing the composer (#2)", () => {
    const open = openChatManually();
    const dismissed = dismissChatHistory(open);
    // The composer stays up; only the panel steps aside.
    expect(dismissed).toEqual({ manualOpen: true, transientOpen: false, historyDismissed: true });
    expect(isChatOverlayOpen(dismissed)).toBe(true);
  });

  it("resets the dismissal the next time chat is opened", () => {
    const reopened = openChatManually();
    expect(reopened.historyDismissed).toBe(false);
  });
});

describe("applyChatArrival (incoming message semantics)", () => {
  it("reveals a transient preview when the overlay is hidden", () => {
    expect(applyChatArrival(closedChatOverlay, false)).toEqual({
      manualOpen: false,
      transientOpen: true,
      historyDismissed: false,
    });
  });

  it("does not disturb an already-open overlay (manual or transient)", () => {
    const manual = { manualOpen: true, transientOpen: false, historyDismissed: false };
    const transient = { manualOpen: false, transientOpen: true, historyDismissed: false };
    expect(applyChatArrival(manual, false)).toEqual(manual);
    expect(applyChatArrival(transient, false)).toEqual(transient);
  });

  it("never reveals the overlay while Ghost or Privacy Mode hides social UI", () => {
    expect(applyChatArrival(closedChatOverlay, true)).toEqual(closedChatOverlay);
    expect(applyChatArrival(openChatManually(), true)).toEqual(openChatManually());
  });
});

describe("canToggleChat", () => {
  it("allows the chat toggle in normal mode", () => {
    expect(canToggleChat(false)).toBe(true);
  });

  it("blocks the chat toggle while Ghost or Privacy Mode hides social UI", () => {
    expect(canToggleChat(true)).toBe(false);
  });
});
