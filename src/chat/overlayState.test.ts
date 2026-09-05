import { describe, expect, it } from "vitest";
import {
  applyChatArrival,
  canToggleChat,
  closeChatPreview,
  closedChatOverlay,
  isChatOverlayOpen,
  openChatManually,
  openChatPreview,
} from "./overlayState";

describe("chat overlay visibility", () => {
  it("keeps a manually opened chat visible when a transient preview ends", () => {
    expect(closeChatPreview(openChatManually())).toEqual({ manualOpen: true, transientOpen: false });
  });

  it("opens a transient preview without turning on the composer", () => {
    const preview = openChatPreview();
    expect(preview.manualOpen).toBe(false);
    expect(isChatOverlayOpen(preview)).toBe(true);
  });
});

describe("applyChatArrival (incoming message semantics)", () => {
  it("reveals a transient preview when the overlay is hidden", () => {
    expect(applyChatArrival(closedChatOverlay, false)).toEqual({
      manualOpen: false,
      transientOpen: true,
    });
  });

  it("does not disturb an already-open overlay (manual or transient)", () => {
    const manual = { manualOpen: true, transientOpen: false };
    const transient = { manualOpen: false, transientOpen: true };
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
