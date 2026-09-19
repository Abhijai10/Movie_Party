import { afterEach, describe, expect, it, vi } from "vitest";
import { isWellFormedSignal, nextPendingSignals, startRealCallSession } from "./callSession";

const offer = (data = '{"type":"offer","sdp":"v=0\\r\\n"}') => ({
  signalType: "OFFER",
  data,
});
const answer = (data = '{"type":"answer","sdp":"v=0\\r\\n"}') => ({
  signalType: "ANSWER",
  data,
});
const ice = (data = '{"candidate":"candidate:1 1 UDP 2130706431 192.168.1.4 8443 typ host"}') => ({
  signalType: "ICE",
  data,
});
const marker = (data = '{"request":"renegotiate","v":1}') => ({
  signalType: "RENEGOTIATE",
  data,
});

const types = (signals: ReadonlyArray<{ signalType: string }>) =>
  signals.map((signal) => signal.signalType);

describe("nextPendingSignals — cursor semantics", () => {
  it("returns nothing when the cursor matches the list length", () => {
    const signals = [offer(), answer()];
    const { pending, nextCursor } = nextPendingSignals(signals, 2, "GUEST");
    expect(pending).toEqual([]);
    expect(nextCursor).toBe(2);
  });

  it("returns only entries after the cursor, in order", () => {
    const signals = [marker(), offer(), ice()];
    const { pending, nextCursor } = nextPendingSignals(signals, 1, "GUEST");
    expect(types(pending)).toEqual(["OFFER", "ICE"]);
    expect(nextCursor).toBe(3);
  });

  it("returns nothing when the cursor is past a shrunk list (caller resets)", () => {
    // set_call_mode cleared the list: the view detects the shrink and
    // resets its cursor before calling; the function itself is a pure
    // slice and stays quiet past the end.
    const signals = [offer()];
    const { pending, nextCursor } = nextPendingSignals(signals, 3, "GUEST");
    expect(pending).toEqual([]);
    expect(nextCursor).toBe(3);
  });

  it("consumes malformed entries without applying them (§67: skip, never crash)", () => {
    const signals = [
      { signalType: "NOT_A_TYPE", data: "{}" },
      { signalType: "OFFER", data: "" },
      { signalType: "OFFER", data: "x".repeat(64 * 1024 + 1) },
      marker(),
      offer(),
    ];
    const { pending, nextCursor } = nextPendingSignals(signals, 0, "GUEST");
    expect(types(pending)).toEqual(["OFFER"]);
    expect(nextCursor).toBe(5);
  });

  it("never re-applies already-consumed entries (duplicate relay is a no-op)", () => {
    const signals = [marker(), offer(), ice()];
    const first = nextPendingSignals(signals, 0, "GUEST");
    const second = nextPendingSignals(signals, first.nextCursor, "GUEST");
    expect(second.pending).toEqual([]);
  });
});

describe("nextPendingSignals — guest coalescing", () => {
  it("keeps only the last OFFER at/after the last marker", () => {
    // Session-restart race: a stale pre-marker offer and a fresh
    // post-marker offer in one delta. Applying both would have the
    // guest answer twice (MP-CALL-004 at the host).
    const signals = [offer("old"), marker(), offer("new")];
    const { pending } = nextPendingSignals(signals, 0, "GUEST");
    expect(types(pending)).toEqual(["OFFER"]);
    expect(pending[0]?.data).toBe("new");
  });

  it("keeps ICE following the kept offer", () => {
    const signals = [marker(), offer("new"), ice()];
    const { pending } = nextPendingSignals(signals, 0, "GUEST");
    expect(types(pending)).toEqual(["OFFER", "ICE"]);
  });

  it("applies a lone OFFER when no marker is in the batch (live-session re-offer)", () => {
    // The guest's own marker was consumed by an earlier batch; the host's
    // re-offer (host restart or poke response) arrives alone and MUST be
    // applied — dropping it would break live renegotiation. Stale pre-
    // marker offers are only dropped relative to a marker IN THIS BATCH;
    // older history is skipped by the view's base cursor instead.
    const signals = [offer("fresh")];
    const { pending } = nextPendingSignals(signals, 0, "GUEST");
    expect(types(pending)).toEqual(["OFFER"]);
    expect(pending[0]?.data).toBe("fresh");
  });

  it("drops ANSWER and RENEGOTIATE markers at the guest (it never applies them)", () => {
    const signals = [marker(), offer(), answer(), marker()];
    const { pending } = nextPendingSignals(signals, 0, "GUEST");
    expect(types(pending)).toEqual([]);
  });
});

describe("nextPendingSignals — host coalescing", () => {
  it("applies the ANSWER and ICE, drops guest OFFERs (protocol violation)", () => {
    const signals = [marker(), answer(), ice(), offer()];
    const { pending } = nextPendingSignals(signals, 0, "HOST");
    expect(types(pending)).toEqual(["ANSWER", "ICE"]);
  });

  it("keeps the last marker to poke a re-offer", () => {
    const signals = [marker(), marker()];
    const { pending } = nextPendingSignals(signals, 0, "HOST");
    expect(types(pending)).toEqual(["RENEGOTIATE"]);
  });

  it("drops the marker when an ANSWER follows it (exchange already progressing)", () => {
    const signals = [marker(), answer()];
    const { pending } = nextPendingSignals(signals, 0, "HOST");
    expect(types(pending)).toEqual(["ANSWER"]);
  });
});

describe("isWellFormedSignal", () => {
  it("accepts OFFER, ANSWER, ICE, and RENEGOTIATE with non-empty data under 64 KiB", () => {
    expect(isWellFormedSignal(offer())).toBe(true);
    expect(isWellFormedSignal(answer())).toBe(true);
    expect(isWellFormedSignal(ice())).toBe(true);
    expect(isWellFormedSignal(marker())).toBe(true);
  });

  it("rejects unknown types", () => {
    expect(isWellFormedSignal({ signalType: "TOTALLY_UNKNOWN", data: "{}" })).toBe(false);
  });

  it("rejects empty and oversized data (§52 limit)", () => {
    expect(isWellFormedSignal({ signalType: "OFFER", data: "" })).toBe(false);
    expect(isWellFormedSignal({ signalType: "OFFER", data: "x".repeat(64 * 1024 + 1) })).toBe(
      false,
    );
  });

  it("accepts data at exactly the 64 KiB boundary", () => {
    expect(isWellFormedSignal({ signalType: "OFFER", data: "x".repeat(64 * 1024) })).toBe(true);
  });
});

// ── adaptive camera tier → sender/track mappings (PRD §41) ────────

import {
  senderParametersForTier,
  trackConstraintsForTier,
  type CameraTierState,
} from "./callSession";

const tierState = (overrides: Partial<CameraTierState>): CameraTierState => ({
  enabled: true,
  tier: "B",
  width: 640,
  height: 360,
  fps: 15,
  targetBitrateBps: 350_000,
  ...overrides,
});

describe("senderParametersForTier — movie-first encoder caps", () => {
  const baseParameters: RTCRtpSendParameters = {
    transactionId: "tx-1",
    codecs: [],
    headerExtensions: [],
    rtcp: { cname: "movie-party", reducedSize: true },
    encodings: [{ rid: "0", active: true, maxBitrate: 5_500_000, maxFramerate: 30 }],
  };

  it("caps bitrate and framerate to the tier's ladder values", () => {
    const next = senderParametersForTier(tierState({}), baseParameters);
    expect(next.encodings[0]?.maxBitrate).toBe(350_000);
    expect(next.encodings[0]?.maxFramerate).toBe(15);
  });

  it("maps every encoder, not just the first (simulcast-proof)", () => {
    const two: RTCRtpSendParameters = {
      transactionId: "tx-2",
      codecs: [],
      headerExtensions: [],
      rtcp: { cname: "movie-party", reducedSize: true },
      encodings: [
        { rid: "0", active: true },
        { rid: "1", active: true },
      ],
    };
    const next = senderParametersForTier(tierState({ tier: "C", fps: 12, targetBitrateBps: 180_000 }), two);
    expect(next.encodings.map((encoding) => encoding.maxBitrate)).toEqual([180_000, 180_000]);
    expect(next.encodings.map((encoding) => encoding.maxFramerate)).toEqual([12, 12]);
  });

  it("never sets maxBitrate to 0 (0 reads as unlimited in some stacks)", () => {
    const next = senderParametersForTier(tierState({ enabled: false, tier: "D", targetBitrateBps: 0 }), baseParameters);
    expect(next.encodings[0]?.maxBitrate).toBeGreaterThanOrEqual(1);
  });

  it("preserves non-tier fields (transactionId, rid, active)", () => {
    const next = senderParametersForTier(tierState({}), baseParameters);
    expect(next.transactionId).toBe("tx-1");
    expect(next.encodings[0]?.rid).toBe("0");
    expect(next.encodings[0]?.active).toBe(true);
  });
});

describe("trackConstraintsForTier — capture downscale", () => {
  it("requests the tier's frame with ideal (nearest-safe) values", () => {
    const constraints = trackConstraintsForTier(tierState({}));
    expect(constraints.width).toEqual({ ideal: 640 });
    expect(constraints.height).toEqual({ ideal: 360 });
    expect(constraints.frameRate).toEqual({ ideal: 15 });
  });

  it("clamps a zero-fps disabled tier to ≥1 (constraints must stay valid)", () => {
    const constraints = trackConstraintsForTier(
      tierState({ enabled: false, tier: "D", width: 0, height: 0, fps: 0 }),
    );
    expect(constraints.width).toEqual({ ideal: 1 });
    expect(constraints.height).toEqual({ ideal: 1 });
    expect(constraints.frameRate).toEqual({ ideal: 1 });
  });
});

// ── MP-03: acquired media must never leak when startup throws ──────────────
//
// `startRealCallSession` acquires the camera/mic and only later returns the
// session that owns them. If anything in between throws, the caller receives no
// handle at all, so nothing downstream can release the devices — the OS camera
// indicator stays lit with no call. These tests pin the ownership contract:
// startup failure releases, success transfers ownership to `close()`.

class FakeMediaTrack {
  readonly kind: string;
  enabled = true;
  readyState: "live" | "ended" = "live";
  stopped = false;

  constructor(kind: string) {
    this.kind = kind;
  }

  stop(): void {
    this.stopped = true;
    this.readyState = "ended";
  }
}

class FakeMediaStream {
  private readonly tracks: FakeMediaTrack[];

  constructor(tracks: FakeMediaTrack[] = []) {
    this.tracks = tracks;
  }

  getTracks(): FakeMediaTrack[] {
    return [...this.tracks];
  }

  getAudioTracks(): FakeMediaTrack[] {
    return this.tracks.filter((track) => track.kind === "audio");
  }

  getVideoTracks(): FakeMediaTrack[] {
    return this.tracks.filter((track) => track.kind === "video");
  }

  addTrack(track: FakeMediaTrack): void {
    this.tracks.push(track);
  }
}

class FakePeerConnection {
  connectionState = "new";
  iceGatheringState = "complete";
  signalingState = "stable";
  localDescription: unknown = null;
  ontrack: unknown = null;
  onicecandidate: unknown = null;
  closed = false;

  addEventListener(): void {}
  removeEventListener(): void {}

  addTrack(track: FakeMediaTrack): { track: FakeMediaTrack } {
    return { track };
  }

  getSenders(): Array<{ track: FakeMediaTrack }> {
    return [];
  }

  createOffer(): Promise<{ type: string; sdp: string }> {
    return Promise.resolve({ type: "offer", sdp: "v=0\r\n" });
  }

  setLocalDescription(description: unknown): Promise<void> {
    this.localDescription = description;
    return Promise.resolve();
  }

  close(): void {
    this.closed = true;
    this.connectionState = "closed";
  }
}

/** Install the minimal browser surface `startRealCallSession` touches. */
function stubCallEnvironment(acquired: FakeMediaStream): void {
  vi.stubGlobal("MediaStream", FakeMediaStream);
  vi.stubGlobal("RTCPeerConnection", FakePeerConnection);
  vi.stubGlobal("window", {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
  });
  vi.stubGlobal("navigator", {
    mediaDevices: { getUserMedia: () => Promise.resolve(acquired) },
  });
}

describe("MP-03 — acquired media ownership is exception-safe", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("stops every acquired track when startup throws after acquisition", async () => {
    const video = new FakeMediaTrack("video");
    const audio = new FakeMediaTrack("audio");
    stubCallEnvironment(new FakeMediaStream([video, audio]));

    const failure = new Error("submit_call_signal failed");
    const error = await startRealCallSession(
      "HOST",
      "VIDEO_VOICE",
      true,
      true,
      {
        // The kickoff publishes the RENEGOTIATE marker; failing here is the
        // real-world "startup throws after getUserMedia succeeded" path.
        onSignal: () => Promise.reject(failure),
        onStatusChange: () => undefined,
      },
    ).then(
      () => null,
      (thrown: unknown) => thrown,
    );

    expect(error).toBe(failure);
    expect(video.stopped).toBe(true);
    expect(audio.stopped).toBe(true);
  });

  it("transfers ownership on success: tracks stay live until close()", async () => {
    const video = new FakeMediaTrack("video");
    const audio = new FakeMediaTrack("audio");
    stubCallEnvironment(new FakeMediaStream([video, audio]));

    const session = await startRealCallSession("HOST", "VIDEO_VOICE", true, true, {
      onSignal: () => Promise.resolve(),
      onStatusChange: () => undefined,
    });

    // A successful start must NOT tear down what it just acquired.
    expect(session.usedRealMedia).toBe(true);
    expect(video.stopped).toBe(false);
    expect(audio.stopped).toBe(false);

    session.close();
    expect(video.stopped).toBe(true);
    expect(audio.stopped).toBe(true);
  });
});
