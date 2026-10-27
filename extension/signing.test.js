// The one test that would have caught the seam bugs in this protocol.
//
// The extension and the app each implement the signing rules from a different
// language. Testing each side against its own expectations is how the pairing
// path shipped with an `atob` that rejected three keys in four, and how the MAC
// came to cover `sequence:kind` while a comment claimed the payload was safe.
// This file pins BOTH sides to one stored vector.

import { readFileSync } from "node:fs";
import { createHmac } from "node:crypto";
import { describe, expect, it } from "vitest";
import { PROTOCOL_VERSION, SIGNED_FIELDS, canonicalPayload } from "./protocol.js";
import { decodeSecret, deriveToken } from "./security.js";

const vector = JSON.parse(
  readFileSync(new URL("./test-vectors/companion-signing.json", import.meta.url), "utf8"),
);

/** The fields the MAC covers, taken from the vector in the shared order. */
const signedFields = () =>
  Object.fromEntries(SIGNED_FIELDS.map((name) => [name, vector.request[name]]));

describe("the shared companion signing vector", () => {
  it("is a version-2 request with a base64url key atob cannot read", () => {
    expect(vector.request.version).toBe(PROTOCOL_VERSION);
    expect(vector.secret_base64url).toMatch(/^[A-Za-z0-9_-]{43}$/);
    // The property that made pairing fail: base64url characters the standard
    // alphabet does not contain.
    expect(() => atob(vector.secret_base64url)).toThrow();
    expect(decodeSecret(vector.secret_base64url)).toHaveLength(32);
  });

  it("canonicalises the payload exactly as the app does", () => {
    expect(SIGNED_FIELDS).toEqual([
      "url",
      "tabId",
      "action",
      "positionMs",
      "durationMs",
      "title",
      "paused",
    ]);
    expect(canonicalPayload(signedFields())).toBe(vector.canonical_payload);
  });

  it("produces the stored token, and the same token the app verifies", async () => {
    const secret = decodeSecret(vector.secret_base64url);
    const token = await deriveToken(
      secret,
      vector.request.sequence,
      vector.request.kind,
      canonicalPayload(signedFields()),
    );
    expect(token).toBe(vector.token);

    // ... and the vector matches an independent HMAC (OpenSSL), so all three
    // implementations agree rather than two agreeing on a shared mistake.
    const independent = createHmac("sha256", Buffer.from(secret))
      .update(`${vector.request.sequence}:${vector.request.kind}:${vector.canonical_payload}`)
      .digest("base64url");
    expect(independent).toBe(vector.token);
  });

  it("changes the token if any signed field changes", async () => {
    const secret = decodeSecret(vector.secret_base64url);
    const base = await deriveToken(secret, 7, "reportContent", canonicalPayload(signedFields()));
    for (const [name, value] of [
      ["url", "https://www.netflix.com/watch/99999999"],
      ["tabId", 13],
      ["title", "Amélie "],
      ["positionMs", 90_001],
      ["paused", true],
    ]) {
      const mutated = await deriveToken(
        secret,
        7,
        "reportContent",
        canonicalPayload({ ...signedFields(), [name]: value }),
      );
      expect(mutated, `changing ${name} did not change the token`).not.toBe(base);
    }
  });
});
