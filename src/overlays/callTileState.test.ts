import { describe, expect, it } from "vitest";
import {
  clampCallTilePosition,
  closeCallTileLocally,
  deriveRemoteCallPresentation,
  minimizeCallTile,
  restoreCallTile,
  showCallTile,
  type CallTileSessionState,
} from "./callTileState";

const liveSession: CallTileSessionState = {
  position: { x: 200, y: 120 },
  isMinimized: false,
  isHidden: false,
};

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

  it("does not derive remote presentation from local camera state", () => {
    // The local participant's camera state must never influence the remote
    // tile; only the peer's own camera/microphone/connection may.
    expect(deriveRemoteCallPresentation(true, true, true)).toEqual({
      showVideo: true,
      showMutedIndicator: false,
      statusLabel: "Present",
    });
  });

  it("clamps a tile larger than the viewport to a reachable margin", () => {
    expect(
      clampCallTilePosition({ x: 500, y: 500 }, { width: 400, height: 300 }, { width: 600, height: 400 }),
    ).toEqual({ x: 12, y: 12 });
  });
});

describe("call tile close semantics", () => {
  it("closing the tile hides it locally without touching position or minimized state", () => {
    const closed = closeCallTileLocally(liveSession);
    expect(closed.isHidden).toBe(true);
    expect(closed.position).toEqual(liveSession.position);
    expect(closed.isMinimized).toBe(false);
  });

  it("closing never hangs up: the session stays alive and restorable", () => {
    const closed = closeCallTileLocally(liveSession);
    const restored = showCallTile(closed);
    expect(restored.isHidden).toBe(false);
    expect(restored.position).toEqual(liveSession.position);
    expect(restored.isMinimized).toBe(false);
  });

  it("showing a hidden tile preserves the user's position and minimized choice", () => {
    const minimizedElsewhere: CallTileSessionState = {
      position: { x: 64, y: 400 },
      isMinimized: true,
      isHidden: true,
    };
    expect(showCallTile(minimizedElsewhere)).toEqual({
      position: { x: 64, y: 400 },
      isMinimized: true,
      isHidden: false,
    });
  });
});

describe("call tile minimize semantics", () => {
  it("minimizing keeps the session alive (video stage stays draggable)", () => {
    const minimized = minimizeCallTile(liveSession);
    expect(minimized.isMinimized).toBe(true);
    expect(minimized.isHidden).toBe(false);
    expect(minimized.position).toEqual(liveSession.position);
  });

  it("minimize then restore returns the full tile without losing position", () => {
    const restored = restoreCallTile(minimizeCallTile(liveSession));
    expect(restored.isMinimized).toBe(false);
    expect(restored.position).toEqual(liveSession.position);
    expect(restored.isHidden).toBe(false);
  });

  it("minimized tile stays hidden when shown from an already-hidden state", () => {
    const hiddenAndMinimized = minimizeCallTile(closeCallTileLocally(liveSession));
    expect(showCallTile(hiddenAndMinimized)).toEqual({
      ...hiddenAndMinimized,
      isHidden: false,
      isMinimized: true,
    });
  });
});
