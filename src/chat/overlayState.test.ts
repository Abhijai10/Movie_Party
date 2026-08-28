import { describe, expect, it } from "vitest";
import {
  closeChatPreview,
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
