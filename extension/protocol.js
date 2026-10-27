// Companion protocol shared by the popup, the service worker and the tests.
// Chrome frames native messages itself, so this module owns meaning, not bytes.

export const PROTOCOL_VERSION = 2;

// Chrome caps a native message at 1 MB. Movie Party speaks far less; a bigger
// payload is a bug or an attempt to make the bridge carry somebody's page.
export const MAX_MESSAGE_BYTES = 32 * 1024;

export const HOST_NAME = "desktop.movieparty.companion";

/**
 * Fields covered by the message MAC, in the one order both ends must use.
 *
 * The token used to cover only `sequence:kind`, which left every payload field
 * rewritable in transit by anything holding a frame — including the native
 * bridge the protocol deliberately treats as plumbing. `sequence` and `kind`
 * already stop a captured message from being replayed or repurposed; this list
 * stops its *contents* from being changed without the key.
 *
 * The canonical form is `name:<utf8-byte-length>:<value>` joined by `|`.
 * Length prefixes make it unambiguous without an escaping scheme, and UTF-8
 * byte lengths keep this side computable the same way in Rust.
 */
export const SIGNED_FIELDS = Object.freeze([
  "url",
  "tabId",
  "action",
  "positionMs",
  "durationMs",
  "title",
  "paused",
]);

export function canonicalPayload(fields) {
  const encoder = new TextEncoder();
  return SIGNED_FIELDS.filter(
    (name) => fields != null && fields[name] !== undefined && fields[name] !== null,
  )
    .map((name) => {
      const value = String(fields[name]);
      return `${name}:${encoder.encode(value).length}:${value}`;
    })
    .join("|");
}

export const KINDS = Object.freeze({
  handshake: "handshake",
  identifyTab: "identifyTab",
  reportContent: "reportContent",
  reportPlayback: "reportPlayback",
  requestNavigation: "requestNavigation",
  controlPlayback: "controlPlayback",
});

// Each kind and the capability it needs. A kind absent here is refused before it
// reaches the bridge, which is what keeps the extension from becoming a proxy.
export const KIND_CAPABILITY = Object.freeze({
  handshake: null,
  identifyTab: "identify_tab",
  reportContent: "report_content",
  reportPlayback: "report_playback",
  requestNavigation: "request_navigation",
  controlPlayback: "control_playback",
});

// Only these kinds may name a destination, and only they must carry one.
const KINDS_WITH_URL = new Set([KINDS.identifyTab, KINDS.reportContent, KINDS.requestNavigation]);

export function isKnownKind(kind) {
  return Object.prototype.hasOwnProperty.call(KIND_CAPABILITY, kind);
}

export function requiresUrl(kind) {
  return KINDS_WITH_URL.has(kind);
}

/**
 * Build one outbound message. `sequence` only ever increases, and the token is
 * bound to both the sequence and the kind, so a captured message cannot be
 * replayed or pointed at a different operation. The handshake is the one
 * exception: it authenticates with the pairing code because no key exists yet.
 */
export function buildMessage({ kind, sequence, token, version = PROTOCOL_VERSION, ...fields }) {
  if (!isKnownKind(kind)) throw new Error(`unknown companion kind: ${String(kind)}`);
  if (!Number.isSafeInteger(sequence) || sequence < 1) {
    throw new Error("companion sequence must be a positive safe integer");
  }
  if (kind === KINDS.handshake) {
    if (typeof fields.code !== "string" || fields.code.length < 6) {
      throw new Error("a handshake must carry the pairing code");
    }
  } else if (typeof token !== "string" || token.length === 0) {
    throw new Error("companion messages are never sent unauthenticated");
  }
  if (requiresUrl(kind) && typeof fields.url !== "string") {
    throw new Error(`${kind} must carry the tab URL`);
  }
  const message = { kind, sequence, version };
  if (typeof token === "string" && token.length > 0) message.token = token;
  for (const [name, value] of Object.entries(fields)) {
    if (value !== undefined) message[name] = value;
  }
  if (JSON.stringify(message).length > MAX_MESSAGE_BYTES) {
    throw new Error("companion message exceeds the protocol limit");
  }
  return message;
}

/** Parse a bridge reply, refusing anything that is not a well-formed response. */
export function parseResponse(value) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return null;
  if (typeof value.kind !== "string" || typeof value.ok !== "boolean") return null;
  return value;
}

/** The error codes Movie Party can return, mapped to words a person can act on. */
export const ERROR_GUIDANCE = Object.freeze({
  ExtensionMissing:
    "Movie Party is not listening for the companion. Switch it on in Settings > Companion and keep the app open.",
  PermissionDenied:
    "The companion was not allowed to read this tab. Pick the tab again and accept the browser prompt.",
  BrowserClosed: "The browser closed while the companion was connected. Reopen it and try again.",
  TabChanged: "That tab navigated or closed. Select it again before continuing.",
  VersionMismatch:
    "The extension and Movie Party speak different companion protocols. Update both, then reconnect.",
  NotPaired:
    "Pair this extension first. The code is shown in Movie Party under Settings > Companion.",
  PairingFailed: "That pairing code was rejected. Generate a new one and try again.",
  OriginNotAllowed: "The companion only reports provider pages, so nothing from this tab was sent.",
  FrameTooLarge: "That companion message was larger than the protocol allows.",
  MalformedFrame: "The companion message was not well formed, so it was discarded.",
  CapabilityDisabled:
    "This browser or provider does not allow that operation, so it stays switched off.",
  HostBusy: "Another browser window already holds the companion link. Close it and try again.",
});

export function guidanceFor(error) {
  return ERROR_GUIDANCE[error] ?? "The companion could not complete that action.";
}
