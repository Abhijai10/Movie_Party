// Everything the extension is allowed to look at, in one place.

// Mirrors `providers::content::provider_page`. A tab on any other host is
// invisible to the companion: not read, not reported, not stored.
export const PROVIDER_HOSTS = Object.freeze([
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
]);

/**
 * True only for an HTTPS provider page with no credentials and no custom port.
 * Hosts are compared exactly, so `netflix.com.evil.test` never qualifies.
 */
export function isProviderUrl(raw) {
  if (typeof raw !== "string" || raw.length === 0 || raw.length > 4096) return false;
  let url;
  try {
    url = new URL(raw);
  } catch {
    return false;
  }
  return (
    url.protocol === "https:" &&
    url.username === "" &&
    url.password === "" &&
    url.port === "" &&
    PROVIDER_HOSTS.includes(url.hostname)
  );
}

/** The pairing secret is 32 bytes; anything else cannot be a key. */
export function decodeSecret(text) {
  if (typeof text !== "string") return null;
  const trimmed = text.trim();
  if (trimmed.length !== 43 || !/^[A-Za-z0-9_-]+$/.test(trimmed)) return null;
  const binary = atob(trimmed.replace(/-/g, "+").replace(/_/g, "/") + "=");
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  return bytes.length === 32 ? bytes : null;
}

function base64UrlNoPad(bytes) {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/**
 * Token for one message, bound to its sequence, kind **and payload**. Derived
 * with WebCrypto HMAC-SHA256, which is the same construction the app verifies
 * against the published RFC 4231 vectors. `payload` is the canonical form from
 * `protocol.js`; leaving it out signs an empty payload, which only the tests do.
 */
export async function deriveToken(secret, sequence, kind, payload = "") {
  const key = await crypto.subtle.importKey(
    "raw",
    secret.slice(0, 32),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const signature = await crypto.subtle.sign(
    "HMAC",
    key,
    new TextEncoder().encode(`${String(sequence)}:${String(kind)}:${String(payload)}`),
  );
  return base64UrlNoPad(new Uint8Array(signature));
}

/** The smallest set of permissions that can do this job. */
export const REQUIRED_PERMISSIONS = Object.freeze({
  permissions: ["activeTab", "scripting", "storage", "nativeMessaging"],
  hostPermissions: PROVIDER_HOSTS.filter((host) => host.includes("."))
    .map((host) => `https://${host}/*`)
    .filter((pattern, index, all) => all.indexOf(pattern) === index),
});

export function redactUrl(raw) {
  if (!isProviderUrl(raw)) return "off-allowlist";
  const url = new URL(raw);
  return `${url.host}${url.pathname}`;
}
