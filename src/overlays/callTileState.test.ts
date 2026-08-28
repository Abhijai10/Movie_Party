import { describe, expect, it } from "vitest";
import { clampCallTilePosition, deriveRemoteCallPresentation } from "./callTileState";

describe("call tile state", () => {
  it("keeps the tile inside the available viewport", () => {
    expect(
      clampCallTilePosition({ x: -40, y: 900 }, { width: 800, height: 600 }, { width: 260, height: 220 }),
    ).toEqual({ x: 12, y: 368 });
  });

  it("derives peer presentation only from remote state", () => {
    expect(deriveRemoteCallPresentation(false, false, true)).toEqual({
      showVideo: false,
      showMutedIndicator: true,
      statusLabel: "Present",
    });
    expect(deriveRemoteCallPresentation(true, true, false)).toEqual({
      showVideo: false,
      showMutedIndicator: false,
      statusLabel: "Away",
    });
  });
});
