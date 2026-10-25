import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { unobscured } from "./visibility";

type Box = { left: number; right: number; top: number; bottom: number };

/** The surfaces the Cinema guard is required to watch. */
const WATCHED = [
  ".camera-card",
  ".chat-compose",
  ".chat-bubble-stack",
  ".reaction-float-layer",
  ".ready-cinema-transition",
  ".lobby-social-dock",
  '[role="dialog"]',
];

const styles = new WeakMap<object, Record<string, string>>();

function rect(box: Box) {
  return { ...box, width: box.right - box.left, height: box.bottom - box.top };
}

function element(
  box: Box,
  opts: { style?: Record<string, string>; parent?: HTMLElement; mounted?: boolean } = {},
): HTMLElement {
  const rects = () => (opts.mounted === false ? [] : [rect(box)]) as unknown as DOMRectList;
  const node = {
    getBoundingClientRect: () => rect(box),
    getClientRects: rects,
    parentElement: opts.parent ?? null,
  };
  styles.set(node, { visibility: "visible", opacity: "1", contain: "none", ...opts.style });
  return node as unknown as HTMLElement;
}

/** The Cinema player rectangle, with room beside it for social surfaces. */
const SLOT: Box = { left: 20, right: 900, top: 20, bottom: 500 };

let overlayNodes: HTMLElement[];
let seenSelectors: string[];

beforeEach(() => {
  overlayNodes = [];
  seenSelectors = [];
  vi.stubGlobal("getComputedStyle", (node: object) => styles.get(node) ?? {});
  vi.stubGlobal("document", {
    querySelectorAll: (selector: string) => {
      seenSelectors.push(selector);
      return WATCHED.every((part) => selector.includes(part)) ? overlayNodes : [];
    },
  });
  vi.stubGlobal(
    "DOMRect",
    class {
      left: number;
      top: number;
      width: number;
      height: number;
      right: number;
      bottom: number;
      constructor(x: number, y: number, width: number, height: number) {
        this.left = x;
        this.top = y;
        this.width = width;
        this.height = height;
        this.right = x + width;
        this.bottom = y + height;
      }
    },
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("player-obstruction guard", () => {
  it("watches every social surface", () => {
    unobscured(element(SLOT));
    expect(seenSelectors).toHaveLength(1);
    for (const part of WATCHED) expect(seenSelectors[0]).toContain(part);
  });

  it("allows playback when social surfaces sit beside the player", () => {
    overlayNodes = [element({ left: 920, right: 1200, top: 20, bottom: 500 })];
    expect(unobscured(element(SLOT))).toBe(true);
  });

  it("pauses when a chat bubble stack overlaps the official player rectangle", () => {
    overlayNodes = [element({ left: 700, right: 1000, top: 100, bottom: 300 })];
    expect(unobscured(element(SLOT))).toBe(false);
  });

  it("pauses when the player is clipped by a scrolling ancestor", () => {
    const clip = element(
      { left: 0, right: 500, top: 0, bottom: 900 },
      { style: { overflowY: "auto" } },
    );
    expect(unobscured(element(SLOT, { parent: clip }))).toBe(false);
  });

  it("ignores overlays that are hidden or transparent", () => {
    overlayNodes = [
      element(
        { left: 100, right: 400, top: 100, bottom: 400 },
        { style: { visibility: "hidden" } },
      ),
      element({ left: 100, right: 400, top: 100, bottom: 400 }, { style: { opacity: "0" } }),
    ];
    expect(unobscured(element(SLOT))).toBe(true);
  });

  it("ignores an unmounted overlay that reports no client rects", () => {
    overlayNodes = [element({ left: 100, right: 400, top: 100, bottom: 400 }, { mounted: false })];
    expect(unobscured(element(SLOT))).toBe(true);
  });

  it("respects a paint-contained ancestor clipping the overlay off the player", () => {
    const dockClip = element(
      { left: 900, right: 1400, top: 0, bottom: 900 },
      { style: { contain: "paint" } },
    );
    overlayNodes = [
      element({ left: 100, right: 1400, top: 100, bottom: 400 }, { parent: dockClip }),
    ];
    expect(unobscured(element(SLOT))).toBe(true);
  });
});
