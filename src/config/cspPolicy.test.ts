import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * the explicit CSP is documented and tested. The policy
 * must allow exactly what the running app needs — 'self' for everything,
 * with the minimal documented exceptions:
 *   - style-src 'unsafe-inline' — Tailwind/JIT + framer-motion inline styles;
 *   - img-src data: blob:       — avatars, media posters, blob previews;
 *   - media-src blob: mediastream: — the call's MediaStreams + local media;
 *   - connect-src ipc: http://ipc.localhost — Tauri IPC on all platforms.
 * Anything broader (unsafe-eval, http:, https:, ws:) must fail this test.
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

  it("documents each exception the app genuinely needs", () => {
    expect(csp).toContain("script-src 'self'");
    expect(csp).toContain("style-src 'self' 'unsafe-inline'");
    expect(csp).toContain("media-src 'self' blob: mediastream:");
    expect(csp).toContain("connect-src 'self' ipc: http://ipc.localhost");
    expect(csp).toContain("font-src 'self' data:");
  });
});
