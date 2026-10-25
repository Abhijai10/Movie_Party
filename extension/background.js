// Movie Party companion — service worker.
//
// This worker never reads a tab on a timer and never watches the rest of the
// browser. Every read it performs follows an explicit click in the popup, which
// is what `activeTab` grants and what keeps ordinary browsing untouched.

import {
  HOST_NAME,
  PROTOCOL_VERSION,
  buildMessage,
  guidanceFor,
  parseResponse,
} from "./protocol.js";
import { canSend, initialState, nextSequence, reduce, Phase } from "./state.js";
import { deriveToken, isProviderUrl } from "./security.js";

let session = initialState();
let secret = null;

async function loadPairedSecret() {
  const stored = await chrome.storage.local.get([
    "companionSecret",
    "companionProtocol",
    "companionCapabilities",
  ]);
  if (typeof stored.companionSecret !== "string" || stored.companionProtocol !== PROTOCOL_VERSION) {
    return null;
  }
  return { secret: stored.companionSecret, capabilities: stored.companionCapabilities ?? [] };
}

/** One message to Movie Party, through the bridge. Failures become states. */
async function send(kind, fields = {}) {
  if (secret === null) {
    session = reduce(session, { type: "PAIRING_REJECTED" });
    return { ok: false, error: "NotPaired", guidance: guidanceFor("NotPaired") };
  }
  if (!canSend(session, kind)) {
    return { ok: false, error: "CapabilityDisabled", guidance: guidanceFor("CapabilityDisabled") };
  }
  const sequence = nextSequence(session);
  const token = await deriveToken(secret, sequence, kind);
  const message = buildMessage({ kind, sequence, token, ...fields });
  session = { ...session, sequence };
  let reply;
  try {
    reply = await chrome.runtime.sendNativeMessage(HOST_NAME, message);
  } catch {
    // The bridge is unreachable: no manifest, no app, or the browser closed it.
    session = reduce(session, { type: "HOST_UNREACHABLE" });
    return { ok: false, error: "ExtensionMissing", guidance: guidanceFor("ExtensionMissing") };
  }
  const parsed = parseResponse(reply);
  if (!parsed) {
    session = reduce(session, { type: "VERSION_MISMATCH" });
    return { ok: false, error: "MalformedFrame", guidance: guidanceFor("MalformedFrame") };
  }
  if (!parsed.ok && parsed.error === "VersionMismatch") {
    session = reduce(session, { type: "VERSION_MISMATCH" });
  }
  return parsed;
}

async function activateTab(tabId, url) {
  if (!isProviderUrl(url)) {
    session = reduce(session, { type: "SELECT_TAB", tabId, url });
    return { ok: false, error: "OriginNotAllowed", guidance: guidanceFor("OriginNotAllowed") };
  }
  session = reduce(session, { type: "SELECT_TAB", tabId, url });
  const identified = await send("identifyTab", { url, tabId });
  if (!identified.ok) return identified;
  return {
    ok: true,
    phase: session.phase,
    host: new URL(url).host,
    granted: [...session.granted],
  };
}

async function disconnect() {
  session = reduce(session, { type: "DISCONNECT" });
  secret = null;
  await chrome.storage.local.remove(["companionSecret", "companionProtocol"]);
  return { ok: true };
}

/**
 * Read only what a provider page must expose to be selected: its address, title
 * and the single visible media element. Runs because the user clicked, which is
 * what `activeTab` pays for. Nothing else on the page is read or stored.
 */
async function observe(tabId) {
  if (tabId === null || tabId === undefined) return null;
  try {
    const [injection] = await chrome.scripting.executeScript({
      target: { tabId },
      func: () => {
        const visible = [...document.querySelectorAll("video")].filter((node) => {
          const box = node.getBoundingClientRect();
          return box.width > 0 && box.height > 0;
        });
        const media = visible.length === 1 ? visible[0] : null;
        return {
          url: location.href,
          title:
            document.querySelector('meta[property="og:title"]')?.content || document.title || "",
          durationMs:
            media && Number.isFinite(media.duration) ? Math.round(media.duration * 1000) : null,
          positionMs:
            media && Number.isFinite(media.currentTime)
              ? Math.round(media.currentTime * 1000)
              : null,
          paused: media ? media.paused : null,
        };
      },
    });
    return injection?.result ?? null;
  } catch {
    // The page refused, or the user has since moved off it. The caller reports
    // that rather than inventing a reading.
    return null;
  }
}

chrome.runtime.onMessage.addListener((message, _sender, respond) => {
  const handle = async () => {
    switch (message?.type) {
      case "STATUS":
        return {
          ok: true,
          phase: session.phase,
          error: session.error,
          granted: [...session.granted],
        };
      case "BEGIN": {
        const stored = await loadPairedSecret();
        if (stored) {
          secret = Uint8Array.from(atob(stored.secret), (character) => character.charCodeAt(0));
          session = reduce(
            { ...initialState(), phase: Phase.NeedsPairing },
            { type: "PAIRED", capabilities: stored.capabilities },
          );
          return { ok: true, phase: session.phase, capabilities: [...session.available] };
        }
        session = reduce(session, { type: "ENABLE" });
        return { ok: true, phase: session.phase };
      }
      case "PAIR": {
        // The app issues a short code and the user types it here; Movie Party
        // then returns the session key over the channel Chrome has already
        // pinned to this extension id.
        const code = typeof message.code === "string" ? message.code.trim() : "";
        if (code.length < 6 || code.length > 24) {
          return { ok: false, error: "PairingFailed", guidance: guidanceFor("PairingFailed") };
        }
        session = reduce(session, { type: "ENABLE" });
        const sequence = nextSequence(session);
        let reply;
        try {
          reply = await chrome.runtime.sendNativeMessage(
            HOST_NAME,
            buildMessage({ kind: "handshake", sequence, code }),
          );
        } catch {
          session = reduce(session, { type: "HOST_UNREACHABLE" });
          return {
            ok: false,
            error: "ExtensionMissing",
            guidance: guidanceFor("ExtensionMissing"),
          };
        }
        const parsed = parseResponse(reply);
        if (!parsed?.ok || typeof parsed.secret !== "string") {
          session = reduce(session, { type: "PAIRING_REJECTED" });
          return { ok: false, error: "PairingFailed", guidance: guidanceFor("PairingFailed") };
        }
        // The app answers with the operations this provider really supports.
        // Storing them means a reconnect cannot quietly gain a capability.
        const capabilities = Array.isArray(parsed.capabilities) ? parsed.capabilities : [];
        secret = Uint8Array.from(atob(parsed.secret), (character) => character.charCodeAt(0));
        await chrome.storage.local.set({
          companionSecret: parsed.secret,
          companionProtocol: PROTOCOL_VERSION,
          companionCapabilities: capabilities,
        });
        session = reduce(session, { type: "PAIRED", startSequence: sequence, capabilities });
        return { ok: true, phase: session.phase, capabilities };
      }
      case "USE_ACTIVE_TAB": {
        const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
        if (!tab?.id || typeof tab.url !== "string") {
          return {
            ok: false,
            error: "PermissionDenied",
            guidance: guidanceFor("PermissionDenied"),
          };
        }
        return activateTab(tab.id, tab.url);
      }
      case "REPORT_CONTENT": {
        const observed = await observe(session.tabId);
        return send("reportContent", {
          url: observed?.url ?? session.url,
          tabId: session.tabId,
          title: observed?.title,
          durationMs: observed?.durationMs,
          positionMs: observed?.positionMs,
          paused: observed?.paused,
        });
      }
      case "REPORT_PLAYBACK":
        return send("reportPlayback", { tabId: session.tabId });
      case "NAVIGATE":
        return send("requestNavigation", { url: message.url, tabId: session.tabId });
      case "CONTROL":
        return send("controlPlayback", {
          action: message.action,
          positionMs: message.positionMs,
          tabId: session.tabId,
        });
      case "BACK":
        session = reduce(session, { type: "BACK_TO_READY" });
        return { ok: true, phase: session.phase };
      case "DISCONNECT":
        return disconnect();
      default:
        return { ok: false, error: "MalformedFrame", guidance: guidanceFor("MalformedFrame") };
    }
  };
  handle().then(respond);
  return true;
});

// Only the tab the user chose. Nothing else in the browser is observed, and when
// that tab leaves the provider allowlist the session ends rather than following
// the user around the web.
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (session.phase !== Phase.Active || tabId !== session.tabId) return;
  if (changeInfo.status !== "complete" && !changeInfo.url) return;
  chrome.tabs
    .get(tabId)
    .then((tab) => {
      if (typeof tab.url === "string" && isProviderUrl(tab.url)) {
        session = { ...session, url: tab.url };
        void send("reportContent", { url: tab.url, tabId });
      } else {
        session = reduce(session, { type: "TAB_CHANGED" });
      }
    })
    .catch(() => reduce(session, { type: "TAB_CHANGED" }));
});

chrome.tabs.onRemoved.addListener((tabId) => {
  if (session.phase === Phase.Active && tabId === session.tabId) {
    session = reduce(session, { type: "BROWSER_CLOSED" });
  }
});
