import { describe, expect, it } from "vitest";
import { isWellFormedSignal, nextPendingSignals } from "./callSession";

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
