// Drives the real service worker with a chrome stub.
//
// The pure modules were well covered and the wiring between them was not, which
// is exactly where the pairing path broke: the app encodes the session key as
// unpadded base64url and the worker decoded it with `atob()`, which throws on
// `-` and `_` — roughly three keys in four. Nothing here mocks that decode.

import { createHmac } from "node:crypto";
import { beforeEach, describe, expect, it, vi } from "vitest";

const PROTOCOL_VERSION = 2;
const PROVIDER_URL = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

function base64url(bytes) {
  return Buffer.from(bytes)
    .toString("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

/**
 * A 32-byte key whose base64url form contains both `-` and `_`. That is the
 * shape the old `atob()` call rejected, and it is the common case: a random
 * 32-byte key contains at least one of those characters ~74% of the time.
 */
function urlCharSecret() {
  for (let seed = 1; seed < 4096; seed++) {
    const bytes = Uint8Array.from({ length: 32 }, (_, index) => (seed * 37 + index * 11) & 0xff);
    const text = base64url(bytes);
    if (text.includes("-") && text.includes("_")) return { bytes, text };
  }
  throw new Error("no candidate key contained both base64url characters");
}

function chromeStub({ storage = new Map(), nativeReply, storageWrite } = {}) {
  const sent = [];
  let onMessage = null;

  const chrome = {
    storage: {
      local: {
        async get(keys) {
          const found = {};
          for (const key of keys) if (storage.has(key)) found[key] = storage.get(key);
          return found;
        },
        async set(values) {
          if (storageWrite) storageWrite(values);
          for (const [key, value] of Object.entries(values)) storage.set(key, value);
        },
        async remove(keys) {
          for (const key of keys) storage.delete(key);
        },
      },
    },
    runtime: {
      async sendNativeMessage(host, message) {
        sent.push({ host, message });
        return nativeReply(message);
      },
      onMessage: {
        addListener(listener) {
          onMessage = listener;
        },
      },
    },
    tabs: {
      onUpdated: { addListener: () => undefined },
      onRemoved: { addListener: () => undefined },
      async query() {
        return [{ id: 4, url: PROVIDER_URL }];
      },
      async get() {
        return { id: 4, url: PROVIDER_URL };
      },
    },
    scripting: {
      async executeScript() {
        return [{ result: null }];
      },
    },
  };

  return {
    chrome,
    sent,
    storage,
    ask(message) {
      return new Promise((resolve) => {
        if (!onMessage) throw new Error("the worker never registered a message listener");
        onMessage(message, {}, resolve);
      });
    },
  };
}

const handshakeReply = (secretText, capabilities = ["identify_tab", "report_content"]) => () => ({
  kind: "handshake",
  ok: true,
  secret: secretText,
  capabilities,
});

let mock;

async function startWorker(options, existingStorage) {
  mock = chromeStub({ ...options, storage: existingStorage ?? options?.storage });
  vi.stubGlobal("chrome", mock.chrome);
  return import("./background.js");
}

describe("companion service worker", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
  });

  it("pairs with a base64url key that atob rejects", async () => {
    const { text } = urlCharSecret();
    // The premise, asserted rather than assumed: this is the string the old
    // code handed to atob() and the popup never saw a reply.
    expect(() => atob(text)).toThrow();

    await startWorker({ nativeReply: handshakeReply(text) });
    const reply = await mock.ask({ type: "PAIR", code: "ABCD2345" });

    expect(reply.ok).toBe(true);
    expect(reply.phase).toBe("READY");
    expect(mock.storage.get("companionSecret")).toBe(text);
    expect(mock.storage.get("companionProtocol")).toBe(PROTOCOL_VERSION);
    // The handshake spent sequence 1; the next command must be 2.
    expect(mock.storage.get("companionSequence")).toBe(1);
  });

  it("signs sequence, kind and payload the way the app verifies them", async () => {
    const { bytes, text } = urlCharSecret();
    await startWorker({ nativeReply: handshakeReply(text) });
    await mock.ask({ type: "PAIR", code: "ABCD2345" });
    const selected = await mock.ask({ type: "USE_ACTIVE_TAB" });
    expect(selected.ok).toBe(true);

    const { message } = mock.sent.at(-1);
    expect(message.kind).toBe("identifyTab");
    expect(message.url).toBe(PROVIDER_URL);

    // Recomputed here with OpenSSL, independently of the worker's WebCrypto.
    const canonical =
      `url:${Buffer.byteLength(message.url)}:${message.url}` +
      `|tabId:${Buffer.byteLength(String(message.tabId))}:${message.tabId}`;
    const expected = createHmac("sha256", Buffer.from(bytes))
      .update(`${message.sequence}:identifyTab:${canonical}`)
      .digest("base64url");
    expect(message.token).toBe(expected);

    // The MAC really covers the payload: the same sequence and kind with a
    // different URL is a different signature.
    const other = createHmac("sha256", Buffer.from(bytes))
      .update(
        `${message.sequence}:identifyTab:` +
          `url:${Buffer.byteLength("https://www.netflix.com/title/1")}:https://www.netflix.com/title/1` +
          `|tabId:${Buffer.byteLength(String(message.tabId))}:${message.tabId}`,
      )
      .digest("base64url");
    expect(other).not.toBe(message.token);
  });

  it("resumes the sequence after the service worker is suspended and revived", async () => {
    const { text } = urlCharSecret();
    await startWorker({ nativeReply: handshakeReply(text) });
    await mock.ask({ type: "PAIR", code: "ABCD2345" });
    await mock.ask({ type: "USE_ACTIVE_TAB" });
    const firstSequence = mock.sent.at(-1).message.sequence;
    expect(mock.storage.get("companionSequence")).toBe(firstSequence);

    // Chrome evicts an idle MV3 worker: module state is gone, storage is not.
    vi.resetModules();
    const revived = await startWorker(
      { nativeReply: () => ({ kind: "identifyTab", ok: true }) },
      mock.storage,
    );
    expect(revived).toBeDefined();
    const begun = await mock.ask({ type: "BEGIN" });
    expect(begun.ok).toBe(true);
    expect(begun.phase).toBe("READY");

    await mock.ask({ type: "USE_ACTIVE_TAB" });
    const resumedSequence = mock.sent.at(-1).message.sequence;
    expect(resumedSequence).toBeGreaterThan(firstSequence);
    expect(mock.storage.get("companionSequence")).toBe(resumedSequence);
  });

  it("refuses a stored key that is not 32 bytes instead of resurrecting a session", async () => {
    const storage = new Map([
      ["companionSecret", "not-a-key"],
      ["companionProtocol", PROTOCOL_VERSION],
      ["companionSequence", 9],
    ]);
    await startWorker({ nativeReply: handshakeReply("irrelevant") }, storage);
    const begun = await mock.ask({ type: "BEGIN" });
    expect(begun.ok).toBe(true);
    expect(begun.phase).toBe("NEEDS_PAIRING");
  });

  it("answers a message whose handler throws instead of closing the port", async () => {
    const { text } = urlCharSecret();
    await startWorker({
      nativeReply: handshakeReply(text),
      storageWrite() {
        throw new Error("quota exceeded");
      },
    });
    const reply = await mock.ask({ type: "PAIR", code: "ABCD2345" });
    expect(reply.ok).toBe(false);
    expect(reply.error).toBe("MalformedFrame");
    expect(typeof reply.guidance).toBe("string");
  });
});
