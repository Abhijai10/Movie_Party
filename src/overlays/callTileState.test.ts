import { describe, expect, it } from "vitest";
import {
  CAMERA_CARD_DEFAULT_PX,
  clampCallTilePosition,
  clampCameraCardHeight,
  clampCameraCardSize,
  closeCallTileLocally,
  deriveRemoteCallPresentation,
  loadCallTilePosition,
  minimizeCallTile,
  restoreCallTile,
  saveCallTilePosition,
  showCallTile,
  type CallTileSessionState,
} from "./callTileState";

const liveSession: CallTileSessionState = {
  position: { x: 200, y: 120 },
  isMinimized: false,
  isHidden: false,
  sizePx: 220,
  heightPx: 190,
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

  it("keeps the tile out of the reserved bottom strip (final action button)", () => {
    // The cinema control dock sits at the bottom; a tile dropped onto it
    // must be pushed back above the reserved strip.
    expect(
      clampCallTilePosition(
        { x: 300, y: 520 },
        { width: 800, height: 600 },
        { width: 220, height: 180 },
        132,
      ),
    ).toEqual({ x: 300, y: 276 }); // 600 - 180 - 12 - 132
  });

  it("a zero reserved bottom keeps the previous behaviour", () => {
    expect(
      clampCallTilePosition(
        { x: 300, y: 520 },
        { width: 800, height: 600 },
        { width: 220, height: 180 },
        0,
      ),
    ).toEqual({ x: 300, y: 408 });
  });

  it("never lets a huge reservation strand the tile below the top margin", () => {
    // If the reserved strip were bigger than the free space, the clamp
    // must still leave the tile reachable at the top margin.
    expect(
      clampCallTilePosition(
        { x: 300, y: 500 },
        { width: 800, height: 400 },
        { width: 220, height: 180 },
        5_000,
      ),
    ).toEqual({ x: 300, y: 12 });
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
      sizePx: 220,
      heightPx: 190,
    };
    expect(showCallTile(minimizedElsewhere)).toEqual({
      position: { x: 64, y: 400 },
      isMinimized: true,
      isHidden: false,
      sizePx: 220,
      heightPx: 190,
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

// ── §28/§29: camera card spec values ─────────────────────

describe("camera card size + persistence (UI_UX_SPEC §28)", () => {
  it("defaults to 220px and clamps user resizes to 120–360", () => {
    expect(CAMERA_CARD_DEFAULT_PX).toBe(220);
    expect(clampCameraCardSize(CAMERA_CARD_DEFAULT_PX)).toBe(220);
    expect(clampCameraCardSize(90)).toBe(120);
    expect(clampCameraCardSize(500)).toBe(360);
    expect(clampCameraCardSize(240.6)).toBe(241);
    expect(clampCameraCardSize(Number.NaN)).toBe(220);
  });

  it("clamps vertical resizes to 150–480 with the same default recovery", () => {
    expect(clampCameraCardHeight(190)).toBe(190);
    expect(clampCameraCardHeight(90)).toBe(150);
    expect(clampCameraCardHeight(900)).toBe(480);
    expect(clampCameraCardHeight(220.4)).toBe(220);
    expect(clampCameraCardHeight(Number.NaN)).toBe(190);
    expect(clampCameraCardHeight(Number.POSITIVE_INFINITY)).toBe(190);
  });

  it("persists the position to localStorage and restores it (§28)", () => {
    const stored: Record<string, string> = {};
    const storage = {
      getItem: (key: string) => stored[key] ?? null,
      setItem: (key: string, value: string) => {
        stored[key] = value;
      },
    } as Pick<Storage, "getItem" | "setItem">;
    saveCallTilePosition({ x: 321, y: 45 }, storage);
    expect(loadCallTilePosition({ x: 0, y: 0 }, storage)).toEqual({ x: 321, y: 45 });
  });

  it("ignores corrupt persisted positions and falls back (§28)", () => {
    const stored: Record<string, string> = { mp_camera_card_position: "{oops" };
    const storage = {
      getItem: (key: string) => stored[key] ?? null,
    } as Pick<Storage, "getItem">;
    expect(loadCallTilePosition({ x: 10, y: 20 }, storage)).toEqual({ x: 10, y: 20 });
    const stored2: Record<string, string> = { mp_camera_card_position: '{"x":"left","y":2}' };
    const storage2 = { getItem: (key: string) => stored2[key] ?? null } as Pick<
      Storage,
      "getItem"
    >;
    expect(loadCallTilePosition({ x: 10, y: 20 }, storage2)).toEqual({ x: 10, y: 20 });
  });

  it("without storage (SSR/test) the fallback position is used", () => {
    expect(loadCallTilePosition({ x: 5, y: 6 }, null)).toEqual({ x: 5, y: 6 });
  });
});

describe("F12 — growing the call tile must not cover the reserved dock strip", () => {
  const viewport = { width: 1280, height: 800 };
  const reservedBottomPx = 132;
  const small = { width: 260, height: 220 };
  const large = { width: 360, height: 480 };

  it("moves a grown tile up out of the strip", () => {
    // Park the tile as low as the SMALL size allows…
    const parked = clampCallTilePosition({ x: 900, y: 620 }, viewport, small, reservedBottomPx);
    // …then grow it, re-clamping against the NEW dimensions (what the resize
    // handler now does). It must move up, not overlap the dock.
    const grown = clampCallTilePosition(parked, viewport, large, reservedBottomPx);
    expect(grown.y).toBeLessThan(parked.y);
    expect(grown.y + large.height).toBeLessThanOrEqual(viewport.height - reservedBottomPx + 1);
  });

  it("keeps the tile fully inside the viewport after a grow", () => {
    const grown = clampCallTilePosition({ x: 2000, y: 2000 }, viewport, large, reservedBottomPx);
    expect(grown.x).toBeGreaterThanOrEqual(0);
    expect(grown.x + large.width).toBeLessThanOrEqual(viewport.width);
    expect(grown.y).toBeGreaterThanOrEqual(0);
  });

  it("without a reserved strip the tile may sit at the bottom", () => {
    const grown = clampCallTilePosition({ x: 900, y: 620 }, viewport, large, 0);
    expect(grown.y).toBeGreaterThan(viewport.height - large.height - 60);
  });
});
