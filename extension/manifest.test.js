import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// `new URL().pathname` leaves a space percent-encoded, which breaks any path
// under "/Volumes/T7 Shield/".
const root = fileURLToPath(new URL(".", import.meta.url));
const manifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
const sources = readdirSync(root)
  .filter((name) => name.endsWith(".js") && !name.endsWith(".test.js"))
  .map((name) => ({ name, text: readFileSync(join(root, name), "utf8") }));

/**
 * The privacy promises are only real if the shipped files cannot break them, so
 * these assert on the artefacts rather than on intent.
 */
describe("extension manifest and privacy surface", () => {
  it("is Manifest V3 with no auto-injected scripts", () => {
    expect(manifest.manifest_version).toBe(3);
    // Static content scripts run on every matching page whether or not a party
    // is happening, which is exactly what "keep ordinary browsing separate" forbids.
    expect(manifest.content_scripts).toBeUndefined();
    expect(manifest.background?.type).toBe("module");
  });

  it("requests no permission that could read credentials or browsing", () => {
    expect(manifest.permissions).toEqual(["activeTab", "scripting", "storage", "nativeMessaging"]);
    for (const forbidden of [
      "cookies",
      "history",
      "webRequest",
      "webRequestBlocking",
      "debugger",
      "tabs",
      "alarms",
      "bookmarks",
      "downloads",
      "management",
      "privacy",
      "proxy",
      "topSites",
      "clipboardRead",
    ]) {
      expect(manifest.permissions, forbidden).not.toContain(forbidden);
      expect(manifest.optional_permissions ?? [], forbidden).not.toContain(forbidden);
    }
  });

  it("limits host access to the providers Movie Party already supports", () => {
    const allowed = [
      "youtube.com",
      "www.youtube.com",
      "m.youtube.com",
      "youtu.be",
      "netflix.com",
      "www.netflix.com",
      "primevideo.com",
      "www.primevideo.com",
      "amazon.com",
      "www.amazon.com",
      "hotstar.com",
      "www.hotstar.com",
      "jiocinema.com",
      "www.jiocinema.com",
    ];
    expect(manifest.host_permissions).toEqual(allowed.map((host) => `https://${host}/*`));
    const text = JSON.stringify(manifest);
    expect(text).not.toContain("<all_urls>");
    expect(text).not.toContain("http://");
    expect(manifest.host_permissions.every((p) => p.endsWith("/*"))).toBe(true);
  });

  it("holds its content security policy to its own files", () => {
    const csp = manifest.content_security_policy.extension_pages;
    expect(csp).toContain("script-src 'self'");
    expect(csp).not.toMatch(/unsafe-eval|unsafe-inline|https:/);
    expect(csp).toContain("object-src 'self'");
    expect(csp).toContain("connect-src 'self'");
  });

  it("ships no code that could read cookies, history or a debugger", () => {
    const banned = [
      "chrome.cookies",
      "chrome.history",
      "chrome.webRequest",
      "chrome.debugger",
      "chrome.management",
      "chrome.identity",
      "document.cookie",
      "localStorage",
      "sessionStorage",
      "indexedDB",
      "XMLHttpRequest",
      "fetch(",
      "importScripts",
      "eval(",
      "Function(",
    ];
    for (const { name, text } of sources) {
      for (const needle of banned) {
        expect(text, `${name} uses ${needle}`).not.toContain(needle);
      }
    }
    expect(sources.length).toBeGreaterThan(0);
  });

  it("attaches no debugger to the user's own browser", () => {
    // CDP on a default profile would expose every site the user has open, so the
    // companion must reach the browser only through the native messaging host.
    const text = sources.map((source) => source.text).join("\n");
    expect(text).not.toMatch(/remote-debugging-port|DevToolsActivePort|devtools:\/\//);
    expect(manifest.permissions.join()).not.toContain("debugger");
  });

  it("moves with the application version instead of drifting", () => {
    const app = JSON.parse(readFileSync(join(root, "..", "package.json"), "utf8"));
    expect(manifest.version).toBe(app.version);
  });
});
