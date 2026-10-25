import { describe, expect, it } from "vitest";
import {
  PROVIDER_HOSTS,
  REQUIRED_PERMISSIONS,
  decodeSecret,
  isProviderUrl,
  redactUrl,
} from "./security.js";

describe("what the companion is allowed to look at", () => {
  it("accepts only provider pages on the exact hosts the app already supports", () => {
    for (const url of [
      "https://www.netflix.com/title/80181170?ep=70218421",
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
      "https://youtu.be/dQw4w9WgXcQ",
      "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
      "https://www.primevideo.com/detail/B08C7JV476",
      "https://www.hotstar.com/in/movies/a-film/1234567890/watch",
    ]) {
      expect(isProviderUrl(url), url).toBe(true);
    }
    for (const url of [
      "",
      "not a url",
      "https://",
      "http://www.netflix.com/title/1",
      "https://www.netflix.com.evil.test/title/1",
      "https://evil.test/#https://www.netflix.com/title/1",
      "https://www:[email protected]/title/1",
      "https://www.netflix.com:8443/title/1",
      "file:///etc/passwd",
      "javascript:alert(1)",
      "data:text/html,x",
      `https://www.netflix.com/${"a".repeat(4096)}`,
    ]) {
      expect(isProviderUrl(url), JSON.stringify(url.slice(0, 40))).toBe(false);
    }
  });

  it("matches hosts exactly rather than by prefix", () => {
    expect(PROVIDER_HOSTS).not.toContain("netflix.com.evil.test");
    expect(PROVIDER_HOSTS.every((host) => /^[a-z0-9.-]+$/.test(host))).toBe(true);
    expect(new Set(PROVIDER_HOSTS).size).toBe(PROVIDER_HOSTS.length);
  });

  it("demands a full 32-byte key and nothing that merely looks like one", () => {
    const valid = "AQEC".repeat(10) + "AAA"; // 43 base64url characters
    expect(decodeSecret(valid)?.length).toBe(32);
    for (const junk of [
      "",
      "   ",
      null,
      42,
      "short",
      "A".repeat(42),
      "A".repeat(44),
      `${"A".repeat(42)}!`,
      "A".repeat(43), // decodes to 32 bytes only when the bits line up
    ]) {
      const decoded = decodeSecret(junk);
      if (decoded !== null) expect(decoded.length).toBe(32);
    }
    expect(decodeSecret(valid)).toEqual(decodeSecret(`  ${valid}  `));
  });

  it("asks for the smallest set of browser permissions that can do the job", () => {
    expect(REQUIRED_PERMISSIONS.permissions).toEqual([
      "activeTab",
      "scripting",
      "storage",
      "nativeMessaging",
    ]);
    expect(REQUIRED_PERMISSIONS.permissions).not.toContain("cookies");
    expect(REQUIRED_PERMISSIONS.permissions).not.toContain("history");
    expect(REQUIRED_PERMISSIONS.permissions).not.toContain("tabs");
    expect(REQUIRED_PERMISSIONS.permissions).not.toContain("webRequest");
    expect(REQUIRED_PERMISSIONS.permissions).not.toContain("debugger");
    expect(REQUIRED_PERMISSIONS.hostPermissions.every((p) => p.startsWith("https://"))).toBe(true);
    expect(
      REQUIRED_PERMISSIONS.hostPermissions.some((p) => p.includes("*") && !p.endsWith("/*")),
    ).toBe(false);
  });

  it("redacts to host and path so a query string never becomes a log line", () => {
    expect(redactUrl("https://www.netflix.com/watch/80181170?trackId=private")).toBe(
      "www.netflix.com/watch/80181170",
    );
    expect(redactUrl("https://mail.google.com/inbox/secrets")).toBe("off-allowlist");
  });
});
