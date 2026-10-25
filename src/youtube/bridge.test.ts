import { readFileSync } from "node:fs";
import vm from "node:vm";
import { afterEach, describe, expect, it, vi } from "vitest";
import { intersects } from "./visibility";
import { acceptsMessage, contextKey, youtubeError } from "./types";
const source = readFileSync(new URL("./bridge.js", import.meta.url), "utf8");
function harness() {
  const messages: Record<string, unknown>[] = [],
    listeners: Record<string, (e: unknown) => unknown> = {};
  let state = 2,
    position = 1,
    destroyed = false,
    blocked = false;
  const parent = { postMessage: (m: Record<string, unknown>) => messages.push(m) };
  let events: Record<string, (e?: { data: number }) => void> = {};
  const api = {
    getVideoUrl: () => "https://www.youtube.com/watch?v=abcdefghijk",
    getDuration: () => 90,
    getCurrentTime: () => position,
    getVideoLoadedFraction: () => 0.8,
    getPlayerState: () => state,
    pauseVideo: vi.fn(() => {
      state = 2;
    }),
    playVideo: vi.fn(() => {
      if (!blocked) {
        state = 1;
        position += 0.1;
      }
    }),
    seekTo: vi.fn((n: number) => {
      position = n;
    }),
    cueVideoById: vi.fn(),
    destroy: vi.fn(() => {
      destroyed = true;
    }),
  };
  const window: {
    YT: unknown;
    onYouTubeIframeAPIReady?: () => void;
    addEventListener: (n: string, f: (e: unknown) => unknown) => void;
  } = {
    YT: {
      Player: function (_id: string, options: { events: typeof events }) {
        events = options.events;
        return api;
      },
    },
    addEventListener: (n, f) => {
      listeners[n] = f;
    },
  };
  vm.runInNewContext(source, {
    window,
    parent,
    location: {
      hash: "#video=abcdefghijk&lease=lease&parent=http%3A%2F%2Flocalhost%3A1420",
      origin: "http://127.0.0.1:1234",
    },
    URL,
    URLSearchParams,
    Date,
    Promise,
    Number,
    Math,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    document: { createElement: () => ({}), head: { appendChild: vi.fn() } },
    fetch: () =>
      Promise.resolve({
        ok: true,
        json: () =>
          Promise.resolve({
            title: "Real API fixture",
            author_name: "Channel",
            thumbnail_url: "https://i.ytimg.com/vi/abcdefghijk/hqdefault.jpg",
          }),
      }),
  });
  window.onYouTubeIframeAPIReady?.();
  events.onReady?.();
  const send = (data: object, origin = "http://localhost:1420", sender: unknown = parent) =>
    listeners.message?.({ source: sender, origin, data: { lease: "lease", ...data } });
  return {
    messages,
    api,
    events,
    send,
    setState: (s: number) => {
      state = s;
      events.onStateChange?.({ data: s });
    },
    block: () => {
      blocked = true;
      events.onAutoplayBlocked?.();
    },
    get destroyed() {
      return destroyed;
    },
  };
}
afterEach(() => vi.useRealTimers());
describe("official YouTube bridge", () => {
  it("keeps overlays outside the official player rectangle", () => {
    const a = { left: 0, right: 480, top: 0, bottom: 270 };
    expect(intersects(a, { left: 480, right: 700, top: 0, bottom: 200 })).toBe(false);
    expect(intersects(a, { left: 450, right: 700, top: 0, bottom: 200 })).toBe(true);
  });
  it("a rejected seek never reports success", async () => {
    vi.useFakeTimers();
    const h = harness();
    h.api.seekTo.mockImplementation(() => undefined);
    void h.send({ type: "command", serial: 1, action: "SEEK", target_ms: 50000 });
    await vi.advanceTimersByTimeAsync(2700);
    expect(h.messages.at(-1)?.result).toMatchObject({ outcome: "REJECTED" });
  });
  it("cued content is not playback ready and real metadata is forwarded", async () => {
    vi.useFakeTimers();
    const h = harness();
    await vi.advanceTimersByTimeAsync(300);
    expect(h.messages.at(-1)).toMatchObject({
      videoId: "abcdefghijk",
      playable: false,
      metadata: { channel: "Channel" },
    });
    h.send({ type: "visible", visible: true });
    h.setState(1);
    expect(h.messages.at(-1)).toMatchObject({ playable: true, player: { ready_state: 4 } });
  });
  it.each([101, 150, 153, 100, 5])("reports embed error %s", (code) => {
    vi.useFakeTimers();
    const h = harness();
    h.events.onError?.({ data: code });
    expect(h.messages.at(-1)?.error).toBe(code);
    expect(youtubeError(code)).toBeTruthy();
    h.send({ type: "dispose" });
  });
  it("ignores forged origins, senders and duplicate commands", async () => {
    vi.useFakeTimers();
    const h = harness();
    h.send({ type: "visible", visible: true });
    const cmd = {
      type: "command",
      serial: 1,
      action: "PLAY",
      target_ms: 1000,
      expires_at_ms: Date.now() + 500,
    };
    h.send(cmd, "https://evil.test");
    h.send(cmd, undefined, {});
    expect(h.api.playVideo).not.toHaveBeenCalled();
    void h.send(cmd);
    await vi.advanceTimersByTimeAsync(300);
    void h.send(cmd);
    expect(h.api.playVideo).toHaveBeenCalledTimes(1);
    expect(h.messages.at(-1)?.result).toMatchObject({ outcome: "APPLIED" });
  });
  it("rejects an expired synchronized play", () => {
    vi.useFakeTimers();
    const h = harness();
    h.send({ type: "visible", visible: true });
    void h.send({
      type: "command",
      serial: 1,
      action: "PLAY",
      target_ms: 1000,
      expires_at_ms: Date.now() - 1,
    });
    expect(h.api.playVideo).not.toHaveBeenCalled();
    expect(h.messages.at(-1)?.result).toMatchObject({ outcome: "TIMED_OUT" });
  });
  it("reports autoplay blocking without pretending to play", () => {
    vi.useFakeTimers();
    const h = harness();
    h.block();
    expect(h.messages.at(-1)?.error).toBe(1000);
  });
  it("repeated hidden visibility updates do not cancel verified recovery pause", async () => {
    vi.useFakeTimers();
    const h = harness();
    h.send({ type: "visible", visible: true });
    h.setState(1);
    h.send({ type: "visible", visible: false });
    void h.send({ type: "command", serial: 1, action: "PAUSE", target_ms: 1000 });
    await vi.advanceTimersByTimeAsync(100);
    h.send({ type: "visible", visible: false });
    await vi.advanceTimersByTimeAsync(300);
    expect(h.messages.at(-1)?.result).toMatchObject({ outcome: "APPLIED" });
  });
  it("hiding pauses and disposal destroys the only player and stops events", async () => {
    vi.useFakeTimers();
    const h = harness();
    h.send({ type: "visible", visible: true });
    h.send({ type: "visible", visible: false });
    expect(h.api.pauseVideo).toHaveBeenCalled();
    h.send({ type: "dispose" });
    expect(h.destroyed).toBe(true);
    const n = h.messages.length;
    await vi.advanceTimersByTimeAsync(20000);
    expect(h.messages).toHaveLength(n);
  });
  it("validates iframe identity, lease and monotonic event sequence", () => {
    const frame = {} as Window;
    const event = {
      source: frame,
      origin: "http://127.0.0.1:123",
      data: { type: "movieparty-youtube", lease: "x", seq: 2, player: {} },
    } as MessageEvent;
    expect(acceptsMessage(event, frame, event.origin, "x", 1)).toBe(true);
    expect(acceptsMessage(event, frame, event.origin, "x", 2)).toBe(false);
    expect(acceptsMessage(event, frame, event.origin, "old", 1)).toBe(false);
    expect(acceptsMessage(event, {} as Window, event.origin, "x", 1)).toBe(false);
    expect(contextKey({ state_epoch: "room", host_sequence: 2, content_revision: 3 })).toBe(
      "room:2:3",
    );
  });
});
