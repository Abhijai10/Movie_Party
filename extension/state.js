// The companion session as a pure state machine, so "what is the extension
// allowed to do right now" has one answer that can be tested without a browser.

import { guidanceFor, isKnownKind, KIND_CAPABILITY } from "./protocol.js";
import { isProviderUrl } from "./security.js";

export const Phase = Object.freeze({
  Off: "OFF",
  NeedsPairing: "NEEDS_PAIRING",
  Ready: "READY",
  Active: "ACTIVE",
  Error: "ERROR",
});

export function initialState() {
  return {
    phase: Phase.Off,
    tabId: null,
    url: null,
    sequence: 0,
    // Nothing is ever read until the user asks for this specific tab, which is
    // what keeps ordinary browsing separate from a Movie Party session.
    granted: new Set(),
    // What Movie Party said this provider can do. Populated at pairing, never by
    // the extension deciding for itself.
    available: new Set(),
    disabled: new Map(),
    error: null,
  };
}

function withError(state, code) {
  return { ...state, phase: Phase.Error, tabId: null, url: null, error: code };
}

/**
 * One transition at a time. Unknown events leave the state untouched rather than
 * treating a typo as permission.
 */
export function reduce(state, event) {
  if (event === null || typeof event !== "object" || typeof event.type !== "string") {
    // Fail closed: a message this module cannot understand is never a reason to
    // grant anything.
    return state;
  }
  switch (event.type) {
    case "ENABLE":
      // Re-enabling keeps what Movie Party already told us this provider can do,
      // but never keeps a granted capability or a selected tab.
      return state.phase === Phase.Off
        ? { ...initialState(), phase: Phase.NeedsPairing, available: new Set(state.available) }
        : state;

    case "PAIRED":
      if (state.phase !== Phase.NeedsPairing) return state;
      return {
        ...state,
        phase: Phase.Ready,
        error: null,
        sequence: event.startSequence ?? 0,
        available: new Set(Array.isArray(event.capabilities) ? event.capabilities : []),
      };

    case "PAIRING_REJECTED":
      return state.phase === Phase.NeedsPairing
        ? { ...state, phase: Phase.Error, error: "PairingFailed" }
        : state;

    case "SELECT_TAB": {
      if (state.phase !== Phase.Ready && state.phase !== Phase.Active) return state;
      if (!isProviderUrl(event.url)) {
        // Refused without reading the tab: the URL never enters the state, so an
        // unrelated page cannot end up recorded or sent anywhere.
        return { ...state, phase: Phase.Ready, tabId: null, url: null, error: "OriginNotAllowed" };
      }
      const sequence = state.sequence + 1;
      // Movie Party decides which operations this provider actually supports; the
      // extension can only narrow that list, never widen it.
      const requested = Array.isArray(event.capabilities)
        ? event.capabilities.filter((name) => state.available.has(name))
        : [...state.available];
      const granted = new Set(requested);
      return {
        ...state,
        phase: Phase.Active,
        tabId: event.tabId,
        url: event.url,
        sequence,
        granted,
        error: null,
      };
    }

    case "TAB_CHANGED":
    case "BROWSER_CLOSED":
    case "PERMISSION_DENIED":
    case "VERSION_MISMATCH":
    case "HOST_UNREACHABLE": {
      const code =
        event.type === "TAB_CHANGED"
          ? "TabChanged"
          : event.type === "BROWSER_CLOSED"
            ? "BrowserClosed"
            : event.type === "PERMISSION_DENIED"
              ? "PermissionDenied"
              : event.type === "VERSION_MISMATCH"
                ? "VersionMismatch"
                : "ExtensionMissing";
      return withError(state, code);
    }

    case "CAPABILITY_REFUSED": {
      if (!event.capability) return state;
      const granted = new Set(state.granted);
      granted.delete(event.capability);
      const disabled = new Map(state.disabled);
      disabled.set(event.capability, event.reason ?? guidanceFor("CapabilityDisabled"));
      // Losing one capability must not drop the whole session; the rest keep
      // working and the reason is shown where the control used to be.
      return { ...state, granted, disabled };
    }

    case "BACK_TO_READY":
      return state.phase === Phase.Error || state.phase === Phase.Active
        ? { ...state, phase: Phase.Ready, tabId: null, url: null, error: null }
        : state;

    case "DISCONNECT":
      return { ...initialState(), phase: Phase.NeedsPairing };

    case "LEAVE_APP":
      // Closing Movie Party ends the session and forgets the tab, while the
      // pairing survives so the user is not asked to pair every session.
      return { ...state, phase: Phase.Ready, tabId: null, url: null, granted: new Set() };

    default:
      return state;
  }
}

export function canSend(state, kind) {
  if (!isKnownKind(kind)) return false;
  const capability = KIND_CAPABILITY[kind];
  if (capability === null) return state.phase === Phase.NeedsPairing || state.phase === Phase.Ready;
  if (state.phase !== Phase.Active) return false;
  return state.granted.has(capability);
}

export function nextSequence(state) {
  return state.sequence + 1;
}

export function disabledReasons(state) {
  return Object.fromEntries(state.disabled);
}
