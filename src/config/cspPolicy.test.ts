import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * the explicit CSP is documented and tested. The policy
 * must allow exactly what the running app needs — 'self' for everything,
 * with the minimal documented exceptions:
 *   - style-src 'unsafe-inline' — Tailwind/JIT + framer-motion inline styles;
 *   - img-src data: blob: https://image.tmdb.org — avatars, blob previews,
 *     and the Home hero's TMDB poster/backdrop images;
 *   - media-src blob: mediastream: — the call's MediaStreams + local media;
 *   - connect-src ipc: http://ipc.localhost — Tauri IPC on all platforms;
 *   - connect-src https://api.themoviedb.org — the Home hero's optional
 *     trending feed (only used when a user-configured token is present).
 * Anything broader (unsafe-eval, http:, ws:) must fail this test.
 */
const config = JSON.parse(
  readFileSync(resolve(__dirname, "../../src-tauri/tauri.conf.json"), "utf8"),
) as { app: { security: { csp?: string } } };

describe("explicit CSP", () => {
  const csp = config.app.security.csp ?? "";

  it("is present and locked to self by default", () => {
    expect(csp.length).toBeGreaterThan(0);
    expect(csp).toContain("default-src 'self'");
  });

  it("never allows the dangerous escapes", () => {
    expect(csp).not.toContain("unsafe-eval");
    expect(csp).not.toContain("http://*");
    expect(csp).not.toContain("ws:");
  });

  it("scopes the TMDB hero feed to exactly two pinned hosts", () => {
    const imgSrc = csp
      .split(";")
      .map((directive) => directive.trim())
      .find((directive) => directive.startsWith("img-src "));
    const connectSrc = csp
      .split(";")
      .map((directive) => directive.trim())
      .find((directive) => directive.startsWith("connect-src "));
    expect(imgSrc).toContain("https://image.tmdb.org");
    expect(connectSrc).toContain("https://api.themoviedb.org");
    // No wildcard hosts anywhere — the TMDB exceptions are pinned, not open.
    expect(csp).not.toContain("https://*");
    expect(csp).not.toContain("*.");
  });

  it("documents each exception the app genuinely needs", () => {
    expect(csp).toContain("script-src 'self'");
    expect(csp).toContain("style-src 'self' 'unsafe-inline'");
    expect(csp).toContain("media-src 'self' blob: mediastream:");
    expect(csp).toContain(
      "connect-src 'self' ipc: http://ipc.localhost https://api.themoviedb.org",
    );
    expect(csp).toContain("font-src 'self' data:");
  });
});
