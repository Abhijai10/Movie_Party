import { describe, expect, it } from "vitest";
import {
  KINDS,
  MAX_MESSAGE_BYTES,
  PROTOCOL_VERSION,
  buildMessage,
  guidanceFor,
  isKnownKind,
  parseResponse,
  requiresUrl,
} from "./protocol.js";

const secret = new Uint8Array(32).fill(7);

describe("companion protocol", () => {
  it("accepts only the kinds the bridge knows", () => {
    for (const kind of Object.values(KINDS)) expect(isKnownKind(kind)).toBe(true);
    for (const kind of ["readCookies", "readHistory", "getProfile", "", "execute"]) {
      expect(isKnownKind(kind)).toBe(false);
    }
  });

  it("refuses to send a command without proof, but a handshake without a code", async () => {
    const token = "t".repeat(43);
    expect(() =>
      buildMessage({ kind: KINDS.reportContent, sequence: 1, token: "", url: "https://x" }),
    ).toThrow(/unauthenticated/);
    expect(() => buildMessage({ kind: KINDS.handshake, sequence: 1, token: "x" })).toThrow(
      /pairing code/,
    );
    const hello = buildMessage({ kind: KINDS.handshake, sequence: 1, code: "ABCDEF" });
    expect(hello.code).toBe("ABCDEF");
    expect(hello.version).toBe(PROTOCOL_VERSION);
    expect(hello.token).toBeUndefined();
    expect(
      buildMessage({ kind: KINDS.identifyTab, sequence: 2, token, url: "https://x" }).token,
    ).toBe(token);
  });

  it("binds a message to its kind and refuses a missing destination", async () => {
    const token = "t".repeat(43);
    expect(() => buildMessage({ kind: KINDS.identifyTab, sequence: 1, token })).toThrow(
      /must carry the tab URL/,
    );
    expect(requiresUrl(KINDS.identifyTab)).toBe(true);
    expect(requiresUrl(KINDS.reportPlayback)).toBe(false);
    const playback = buildMessage({ kind: KINDS.reportPlayback, sequence: 1, token });
    expect(playback.url).toBeUndefined();
  });

  it("rejects sequence numbers that cannot be trusted", () => {
    const token = "t".repeat(43);
    for (const sequence of [0, -1, 1.5, Number.NaN, Number.MAX_SAFE_INTEGER + 1, "3"]) {
      expect(() => buildMessage({ kind: KINDS.reportPlayback, sequence, token })).toThrow(
        /sequence/,
      );
    }
  });

  it("will not frame a message larger than the protocol allows", () => {
    const token = "t".repeat(43);
    expect(() =>
      buildMessage({
        kind: KINDS.reportContent,
        sequence: 1,
        token,
        url: "https://x",
        blob: "a".repeat(MAX_MESSAGE_BYTES),
      }),
    ).toThrow(/protocol limit/);
  });

  it("parses only well-formed replies", () => {
    expect(parseResponse({ kind: "identifyTab", ok: true })).toEqual({
      kind: "identifyTab",
      ok: true,
    });
    for (const junk of [
      null,
      undefined,
      "ok",
      7,
      [],
      [{ ok: true }],
      { kind: "x" },
      { ok: false },
    ]) {
      expect(parseResponse(junk)).toBeNull();
    }
  });

  it("answers every error code with words a person can act on", () => {
    const codes = [
      "ExtensionMissing",
      "PermissionDenied",
      "BrowserClosed",
      "TabChanged",
      "VersionMismatch",
      "NotPaired",
      "PairingFailed",
      "OriginNotAllowed",
      "FrameTooLarge",
      "MalformedFrame",
      "CapabilityDisabled",
      "HostBusy",
    ];
    const guidance = codes.map(guidanceFor);
    expect(new Set(guidance).size).toBe(codes.length);
    expect(guidance.every((text) => text.split(" ").length >= 4)).toBe(true);
    // An unknown code must still say something, never nothing.
    expect(guidanceFor("MadeUpCode")).toMatch(/could not complete/);
    expect(guidanceFor(undefined)).toMatch(/could not complete/);
  });

  it("derives a proof that never carries the secret itself", async () => {
    const { deriveToken } = await import("./security.js");
    const first = await deriveToken(secret, 1, KINDS.identifyTab);
    const replay = await deriveToken(secret, 1, KINDS.identifyTab);
    const nextSequence = await deriveToken(secret, 2, KINDS.identifyTab);
    const otherKind = await deriveToken(secret, 1, KINDS.reportContent);
    // 32 bytes of HMAC-SHA256, base64url without padding.
    expect(first).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(first).toBe(replay);
    expect(first).not.toBe(nextSequence);
    expect(first).not.toBe(otherKind);
    const encodedSecret = btoa(String.fromCharCode(...secret))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
    const message = JSON.stringify(
      buildMessage({ kind: KINDS.identifyTab, sequence: 1, token: first, url: "https://x" }),
    );
    expect(message).not.toContain(encodedSecret);
    expect(message).toContain(first);
  });
});
